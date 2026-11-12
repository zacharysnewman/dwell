//! Travel between the grids (docs/proposals/MOON_WORLD.md §7, M-7): every grid simulated in its
//! own space, a flier carried from one to the next with nothing jumping.
//!
//! - **A space per grid** ([`GridSpace`]): the grid's own voxel world (its chunks generated from the
//!   grid's generator in the grid's local frame: home's terrain, the moon's `cratered` rules),
//!   its own physics and its own players. Positions in a space are **grid-local** (the home grid's
//!   are the world's), so the controller runs unchanged on every grid: down is toward the grid's
//!   midplane by the face the player is on, both faces through `FaceFrame` as at home; collision is
//!   the grid's own chunks. A player in a moving grid's space is locked to it: it moves with the
//!   grid (the space's frame is the grid's [`GridFrame`] at the tick, from the world clock).
//! - **The frames** ([`grid_frame_at`]): the home grid at rest (`GridFrame::HOME`), the moon on
//!   M-4's orbit (`Celestial::moon_position`, tidally locked `moon_axes`), its velocity and spin
//!   from the orbit, so the server and every client place it alike from the clock.
//! - **Capture and release** ([`Travel::update`]): a flier inside a grid's `captureM` of its
//!   centre (the midplane's centre) moves into its space, position and velocity through
//!   `GridFrame::release` then `capture` at the same instant, so the world position is kept to the
//!   µm; past `captureM × releaseFactor` it is handed back to home the same way. Its controller
//!   state goes with it, its face is the side of the new grid's midplane (the vertical state
//!   mirrored as at a midplane crossing), its gravity the grid's `gravityScale`.
//! - **Far flight** ([`Travel::far_speed`]): farther than `aboveM` from every grid's slab, flight
//!   is at least `perS × d` fast, so the ~597 km take about a minute and a half.
//! - **Between the grids** home gravity holds until captured (§12 Q6's default): only fliers are
//!   captured, and far from every grid a player only flies.
//!
//! The server (M-7b, `server/grids.rs`) holds the other grids' spaces beside its home world and
//! moves its players between them with [`carry`]; the client's view turning to the new down (its
//! look continues across by [`look_across`]) is the client's follow-up.

use std::sync::Arc;

use dwell_look::celestial::Celestial;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{PlayerHandle, Players};
use dwell_physics::{DVec3, Physics, Vec3};
use dwell_world::{
    AirChunkTest, CELL_H, CHUNK_SIZE, ChunkCoord, ChunkGenerator, FaceFrame, GridFrame, MIDPLANE_M,
    VoxelWorld,
};
use dwell_worldgen::grids::{CrateredTerrain, GridDef, GridFrameKind, GridGenerator, TravelRules};

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The unit quaternion `[x, y, z, w]` turning the world axes onto `axes` (orthonormal,
/// right-handed: grid x, y, z in the world).
pub fn quat_of_axes(axes: [V3; 3]) -> [f64; 4] {
    // The rotation matrix's columns are the axes: m[r][c] = axes[c][r].
    let m = |r: usize, c: usize| axes[c][r];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m(2, 1) - m(1, 2)) / s,
            (m(0, 2) - m(2, 0)) / s,
            (m(1, 0) - m(0, 1)) / s,
            0.25 * s,
        ]
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (m(0, 1) + m(1, 0)) / s,
            (m(0, 2) + m(2, 0)) / s,
            (m(2, 1) - m(1, 2)) / s,
        ]
    } else if m(1, 1) > m(2, 2) {
        let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
        [
            (m(0, 1) + m(1, 0)) / s,
            0.25 * s,
            (m(1, 2) + m(2, 1)) / s,
            (m(0, 2) - m(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
        [
            (m(0, 2) + m(2, 0)) / s,
            (m(1, 2) + m(2, 1)) / s,
            0.25 * s,
            (m(1, 0) - m(0, 1)) / s,
        ]
    };
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    q.map(|v| v / n)
}

/// A grid's frame at `world_s` (MOON_WORLD.md §6): the home grid at rest; an orbiting grid at the
/// celestial model's moon position and tidally locked axes, moving with the orbit (its velocity
/// and spin by a central difference over ½ s, smooth to well under a µm/s at the orbit's speeds).
pub fn grid_frame_at(celestial: &Celestial, def: &GridDef, world_s: f64) -> GridFrame {
    match def.frame {
        GridFrameKind::Rest => GridFrame::HOME,
        GridFrameKind::Orbit => {
            const H: f64 = 0.25;
            let origin = celestial.moon_position(world_s);
            let axes = celestial.moon_axes(world_s);
            let (a0, a1) = (
                celestial.moon_axes(world_s - H),
                celestial.moon_axes(world_s + H),
            );
            let (p0, p1) = (
                celestial.moon_position(world_s - H),
                celestial.moon_position(world_s + H),
            );
            let velocity = sub(p1, p0).map(|v| v / (2.0 * H));
            // ω = ½ Σ aᵢ × ȧᵢ for an orthonormal frame turning at ω.
            let mut spin = [0.0; 3];
            for i in 0..3 {
                let rate = sub(a1[i], a0[i]).map(|v| v / (2.0 * H));
                let c = cross(axes[i], rate);
                for k in 0..3 {
                    spin[k] += 0.5 * c[k];
                }
            }
            GridFrame::at_pose(origin, quat_of_axes(axes))
                .moving(velocity, spin)
                .with_gravity_scale(def.gravity_scale)
        }
    }
}

/// A grid's centre in the grid's space: its midplane's centre.
pub const GRID_CENTRE: V3 = [0.0, MIDPLANE_M, 0.0];

/// How far (m) a grid-local point is from the grid's slab: its disc of `radius_m`, `surface_m`
/// each side of the midplane (0 inside it).
pub fn slab_distance(radius_m: f64, surface_m: f64, p: V3) -> f64 {
    let dr = (p[0].hypot(p[2]) - radius_m).max(0.0);
    let dy = ((p[1] - MIDPLANE_M).abs() - surface_m).max(0.0);
    dr.hypot(dy)
}

/// A face-local look (yaw, pitch; degrees) on grid `from`'s face `ff`, continued onto grid `to`'s
/// face `tf`: the same world direction (the view keeps where it points as the player's frame
/// changes; the client then rolls it upright, as at a midplane crossing).
pub fn look_across(
    from: &GridFrame,
    ff: FaceFrame,
    to: &GridFrame,
    tf: FaceFrame,
    yaw_deg: f64,
    pitch_deg: f64,
) -> (f64, f64) {
    let d = to.vector_to_local(tf, from.look(ff, yaw_deg, pitch_deg));
    let pitch = d[1].clamp(-1.0, 1.0).asin().to_degrees();
    let yaw = d[0].atan2(d[2]).to_degrees();
    (yaw, pitch)
}

/// The chunk generator for a grid's own space: its generator in the grid's local frame, and the
/// test for chunks that are all air (outside its slab), which are never generated or stored.
pub fn grid_generator(def: &GridDef, world_seed: u64) -> Option<(ChunkGenerator, AirChunkTest)> {
    match &def.generator {
        GridGenerator::Cratered(_) => {
            let t = Arc::new(CrateredTerrain::new(def, world_seed)?);
            let (radius, surface) = (t.radius_m(), def.surface_m);
            let air: AirChunkTest = Arc::new(move |c: ChunkCoord| {
                let s = f64::from(CHUNK_SIZE);
                let (x0, z0) = (f64::from(c.x) * s, f64::from(c.z) * s);
                let (y0, y1) = (f64::from(c.y) * s * CELL_H, f64::from(c.y + 1) * s * CELL_H);
                // The nearest point of the chunk's columns to the disc's axis.
                let dx = (0.0f64).clamp(x0, x0 + s);
                let dz = (0.0f64).clamp(z0, z0 + s);
                let off_slab = y0 > MIDPLANE_M + surface || y1 < MIDPLANE_M - surface;
                off_slab || dx.hypot(dz) > radius + 1.0
            });
            let generate: ChunkGenerator = Arc::new(move |c, chunk| t.generate(c, chunk));
            Some((generate, air))
        }
        // Home's terrain is the server's own generator (`generator_for`).
        GridGenerator::Terrain { .. } => None,
    }
}

pub use dwell_physics::controller::carry;

/// One grid's simulation space: its chunks, physics and players, in its local frame.
pub struct GridSpace {
    pub def: &'static GridDef,
    /// The disc's radius (m): the data's, or the world's own for the home grid.
    pub radius_m: f64,
    pub world: VoxelWorld,
    pub physics: Physics,
    pub players: Players,
}

impl GridSpace {
    pub fn new(
        def: &'static GridDef,
        radius_m: f64,
        generator: ChunkGenerator,
        air: Option<AirChunkTest>,
    ) -> Self {
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        Self {
            def,
            radius_m,
            world: VoxelWorld::new(generator, air),
            physics,
            players,
        }
    }

    /// One tick of the space: its players' controllers, then its physics.
    pub fn step(&mut self, dt: f64) {
        self.players.tick(&mut self.world, &mut self.physics);
        self.physics.step(dt);
    }
}

/// Which grid's space a player is in, and its handle there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Traveller {
    pub grid: usize,
    pub handle: PlayerHandle,
}

/// A move between spaces, as [`Travel::update`] reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transfer {
    pub from: usize,
    pub to: usize,
    /// The player's world position just before and just after (equal to the µm).
    pub world_before: V3,
    pub world_after: V3,
    /// Its world velocity just before and just after (equal to the µm/s).
    pub velocity_before: V3,
    pub velocity_after: V3,
}

/// The grids' spaces and the rules carrying players between them.
pub struct Travel {
    /// One space per grid, in `grids.json`'s order; index 0 is the home grid.
    pub spaces: Vec<GridSpace>,
    pub celestial: Celestial,
    pub rules: TravelRules,
}

impl Travel {
    /// The grid's frame at `world_s`.
    pub fn frame(&self, grid: usize, world_s: f64) -> GridFrame {
        grid_frame_at(&self.celestial, self.spaces[grid].def, world_s)
    }

    /// A player's grid-local position and velocity in its space.
    pub fn local(&self, t: Traveller) -> (V3, V3) {
        let s = &self.spaces[t.grid];
        (
            s.players.position(&s.physics, t.handle).to_array(),
            s.players
                .velocity(&s.physics, t.handle)
                .as_dvec3()
                .to_array(),
        )
    }

    /// A player's world position and velocity at `world_s`.
    pub fn world(&self, t: Traveller, world_s: f64) -> (V3, V3) {
        let (p, v) = self.local(t);
        self.frame(t.grid, world_s).release(p, v)
    }

    /// The far-flight speed at world point `p` (m/s; 0 within `aboveM` of a grid's slab).
    pub fn far_speed(&self, p: V3, world_s: f64) -> f64 {
        let d = (0..self.spaces.len())
            .map(|g| {
                let s = &self.spaces[g];
                let local = self.frame(g, world_s).to_grid(p);
                slab_distance(s.radius_m, s.def.surface_m, local)
            })
            .fold(f64::INFINITY, f64::min);
        self.rules.far_speed(d)
    }

    /// Before a tick at `world_s`: captures a flier inside a grid's `captureM`, releases a player
    /// past `captureM × releaseFactor` back to home, and sets the player's far-flight speed.
    pub fn update(&mut self, t: &mut Traveller, world_s: f64) -> Option<Transfer> {
        let (world_p, _) = self.world(*t, world_s);
        let flying = self.spaces[t.grid].players.controller(t.handle).fly.flying;
        let mut moved = None;
        if t.grid == 0 {
            if flying {
                let to = (1..self.spaces.len()).find(|&g| {
                    let local = self.frame(g, world_s).to_grid(world_p);
                    self.spaces[g]
                        .def
                        .capture_m
                        .is_some_and(|c| len(sub(local, GRID_CENTRE)) < c)
                });
                if let Some(to) = to {
                    moved = Some(self.transfer(t, to, world_s));
                }
            }
        } else {
            let (local, _) = self.local(*t);
            let release = self.spaces[t.grid].def.capture_m.unwrap_or(f64::INFINITY)
                * self.rules.release_factor;
            if len(sub(local, GRID_CENTRE)) > release {
                moved = Some(self.transfer(t, 0, world_s));
            }
        }
        let (world_p, _) = self.world(*t, world_s);
        let far = self.far_speed(world_p, world_s) as f32;
        let s = &mut self.spaces[t.grid];
        s.players.set_far_fly_speed(&mut s.physics, t.handle, far);
        moved
    }

    /// Moves a player into grid `to`'s space at `world_s`: the same world position and velocity,
    /// its controller state, the new grid's face and gravity.
    fn transfer(&mut self, t: &mut Traveller, to: usize, world_s: f64) -> Transfer {
        let from = t.grid;
        let (world_p, world_v) = self.world(*t, world_s);
        let (p, v) = self.frame(to, world_s).capture(world_p, world_v);
        let (src, dst) = if from < to {
            let (a, b) = self.spaces.split_at_mut(to);
            (&mut a[from], &mut b[0])
        } else {
            let (a, b) = self.spaces.split_at_mut(from);
            (&mut b[0], &mut a[to])
        };
        let scale = dst.def.gravity_scale as f32;
        let h = carry(
            (&mut src.players, &mut src.physics),
            t.handle,
            (&mut dst.players, &mut dst.physics),
            scale,
            p,
            v,
        );
        *t = Traveller {
            grid: to,
            handle: h,
        };
        let (world_after, velocity_after) = self.world(*t, world_s);
        Transfer {
            from,
            to,
            world_before: world_p,
            world_after,
            velocity_before: world_v,
            velocity_after,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_round_trip_through_the_quaternion() {
        let c = Celestial::new(dwell_look::daynight::DAY_NIGHT, None);
        for i in 0..50 {
            let t = f64::from(i) * 397.0;
            let axes = c.moon_axes(t);
            let f = GridFrame::at_pose([0.0; 3], quat_of_axes(axes));
            for (k, a) in axes.iter().enumerate() {
                let mut e = [0.0; 3];
                e[k] = 1.0;
                let w = f.vector_to_world(e);
                assert!(len(sub(w, *a)) < 1e-12, "t {t} axis {k}: {w:?} vs {a:?}");
            }
        }
    }

    #[test]
    fn a_point_carried_by_the_moon_stays_put_in_its_frame() {
        // both-faces: points on each side of the moon's midplane.
        // The frame's velocity and spin are the orbit's: a grid point carried one tick lands where
        // the next tick's frame puts it (the player locked to the grid).
        let c = Celestial::new(dwell_look::daynight::DAY_NIGHT, None);
        let moon = dwell_worldgen::grids::grid("dwell:moon").unwrap();
        for i in 0..20 {
            let t = 100.0 + f64::from(i) * 611.0;
            let (f0, f1) = (
                grid_frame_at(&c, moon, t),
                grid_frame_at(&c, moon, t + 1.0 / 60.0),
            );
            for p in [
                [9000.0, MIDPLANE_M + 80.0, -4000.0],
                [-3000.0, MIDPLANE_M - 80.0, 7000.0],
            ] {
                let w0 = f0.to_world(p);
                let pv = f0.point_velocity(w0);
                let carried = [0, 1, 2].map(|k| w0[k] + pv[k] / 60.0);
                let err = len(sub(carried, f1.to_world(p)));
                assert!(err < 0.01, "t {t}: {err} m off");
            }
        }
    }

    #[test]
    fn far_flight_is_zero_near_and_grows_far_out() {
        // both-faces: the slab is measured each side of the midplane.
        let r = dwell_worldgen::grids::travel();
        assert_eq!(r.far_speed(10.0), 0.0);
        assert_eq!(r.far_speed(r.far_above_m), 0.0);
        assert!(r.far_speed(r.far_above_m * 2.0) > 300.0);
        assert_eq!(r.far_speed(1e9), r.far_max_mps);
        assert_eq!(
            slab_distance(100.0, 10.0, [50.0, MIDPLANE_M + 5.0, 0.0]),
            0.0
        );
        assert_eq!(
            slab_distance(100.0, 10.0, [0.0, MIDPLANE_M + 30.0, 0.0]),
            20.0
        );
    }

    #[test]
    fn a_look_across_grids_keeps_its_world_direction() {
        // both-faces: from home's face A onto each of the moon's faces.
        let c = Celestial::new(dwell_look::daynight::DAY_NIGHT, None);
        let moon = dwell_worldgen::grids::grid("dwell:moon").unwrap();
        let f = grid_frame_at(&c, moon, 1234.0);
        for face in [FaceFrame::A, FaceFrame::B] {
            for (yaw, pitch) in [(0.0, 0.0), (37.0, 20.0), (-120.0, -60.0)] {
                let (y2, p2) = look_across(&GridFrame::HOME, FaceFrame::A, &f, face, yaw, pitch);
                let a = GridFrame::HOME.look(FaceFrame::A, yaw, pitch);
                let b = f.look(face, y2, p2);
                assert!(len(sub(a, b)) < 1e-9, "{face:?} {yaw} {pitch}");
            }
        }
    }
}
