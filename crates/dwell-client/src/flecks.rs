//! Flecks drifting on moving water (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3d; the Drifting
//! flecks setting): small scraps of foam spawned on the surface of water running faster than the
//! liquid's `particle_from_ms` within `particle_radius_m` of the eye (more often and more opaque
//! up to `particle_full_ms`: few in slow water, none in still), carried by the current table
//! (`VoxelWorld::currents`, the same the swimmers and bodies read), kept on the surface, eased in
//! and out over their `particle_life_s`, drawn out along the current, per liquid as
//! `shared/look/waterflow.json` says (owner, 2026-11-06: not "perfect white circles"). The
//! renderer draws them (`render::flames::fleck_quads`, its shader the ragged outline). Face-local:
//! a column's surface is found toward the face's up, through `FaceFrame`.

use dwell_blocks::{FLUIDS, try_info};
use dwell_look::waterflow::{LIQUID_FLOWS, LiquidFlowLook};
use dwell_world::{CELL_H, FaceFrame, VoxelWorld, m_of_row};

/// A fleck: where it is (world m), how old (s), its liquid's look (index in `LIQUID_FLOWS`), the
/// current carrying it (world x, z m/s; eased toward the column's, so it turns smoothly where the
/// current bends) and its seed (its ragged outline's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fleck {
    pub at: [f64; 3],
    pub age: f64,
    pub look: usize,
    pub vel: [f32; 2],
    pub seed: u32,
}

/// `x` from 0 to 1 eased (0 below, 1 above).
fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How strongly a fleck shows, and how often one is spawned, in a current of `speed` (m/s): none
/// at `particle_from_ms` or slower, all at `particle_full_ms`, eased between.
pub fn speed_share(l: &LiquidFlowLook, speed: f64) -> f64 {
    ease((speed - l.particle_from_ms) / (l.particle_full_ms - l.particle_from_ms))
}

/// A fleck's opacity at `age` (s) in a current of `speed` (m/s): at most `particle_opacity`,
/// eased in over its first `particle_fade_in_s` and out over its last `particle_fade_out_s` (no
/// popping), times its current's `speed_share` (none in slow water).
pub fn fleck_alpha(l: &LiquidFlowLook, age: f64, speed: f64) -> f64 {
    let life = l.particle_life_s;
    if age <= 0.0 || age >= life {
        return 0.0;
    }
    let ramp = |t: f64, over: f64| if over > 0.0 { ease(t / over) } else { 1.0 };
    l.particle_opacity
        * ramp(age, l.particle_fade_in_s)
        * ramp(life - age, l.particle_fade_out_s)
        * speed_share(l, speed)
}

/// A fleck's outline on the water in a current `vel` (world x, z m/s): the unit direction along
/// the flow, and its half-length along it and half-width across (m): `particle_size_m` across,
/// drawn out along the flow up to `particle_stretch` times at `particle_full_ms`.
pub fn fleck_shape(l: &LiquidFlowLook, vel: [f32; 2]) -> ([f64; 2], f64, f64) {
    let v = [f64::from(vel[0]), f64::from(vel[1])];
    let speed = v[0].hypot(v[1]);
    let along = if speed > 1e-6 {
        [v[0] / speed, v[1] / speed]
    } else {
        [1.0, 0.0]
    };
    let w = l.particle_size_m;
    let stretch = 1.0 + (l.particle_stretch - 1.0) * speed_share(l, speed);
    (along, w * stretch, w)
}

impl Fleck {
    /// The current's speed carrying it (m/s).
    pub fn speed(&self) -> f64 {
        f64::from(self.vel[0].hypot(self.vel[1]))
    }

    /// Its opacity now (`fleck_alpha`).
    pub fn alpha(&self) -> f32 {
        fleck_alpha(&LIQUID_FLOWS[self.look], self.age, self.speed()) as f32
    }
}

/// How fast a fleck's carrying current eases toward its column's (1/s): a bend turns it over a
/// fraction of a second, not in a frame.
const VEL_EASE: f64 = 4.0;

/// The flecks near the eye.
pub struct Flecks {
    pub list: Vec<Fleck>,
    credit: f64,
    rng: u64,
}

impl Default for Flecks {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            credit: 0.0,
            rng: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

/// How many rows a column's surface is looked for above and below a point.
const SCAN_UP: i32 = 4;
const SCAN_DOWN: i32 = 12;

/// The drawn surface of the water in column (`x`, `z`) nearest under `near_y` (world m) on face
/// `f`, and its liquid's flow look: (world y, look index). None: no water there, or a liquid with
/// no flow look.
pub fn surface_at(world: &VoxelWorld, x: i32, z: i32, near_y: f64) -> Option<(f64, usize)> {
    let f = FaceFrame::at_height(near_y);
    let local = f.y(near_y);
    let row = (local / CELL_H).floor() as i32;
    let liquid = |r: i32| {
        let m = world.get_voxel_if_loaded(f.cell([x, r, z]));
        try_info(m).filter(|i| i.liquid)
    };
    for r in (row - SCAN_DOWN..=row + SCAN_UP).rev() {
        let Some(info) = liquid(r) else {
            continue;
        };
        if liquid(r + 1).is_some() {
            continue;
        }
        let fluid = &FLUIDS[usize::from(info.fluid?)];
        let look = LIQUID_FLOWS.iter().position(|l| l.fluid == fluid.name)?;
        let fill = if info.fill > 0.0 {
            f64::from(info.fill)
        } else {
            1.0
        };
        let top = m_of_row(r) + CELL_H * fill - fluid.surface_drop * CELL_H;
        return Some((f.y(top), look));
    }
    None
}

impl Flecks {
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A step of `dt` seconds around `eye` (world m): the flecks carried by the current and kept on
    /// the surface, the old, stranded or far ones gone, new ones spawned on moving water; none
    /// while `on` is false.
    pub fn update(&mut self, world: &VoxelWorld, eye: [f64; 3], dt: f64, on: bool) {
        if !on {
            self.list.clear();
            self.credit = 0.0;
            return;
        }
        let speed = |c: [f32; 2]| f64::from(c[0].hypot(c[1]));
        self.list.retain_mut(|p| {
            let look: &LiquidFlowLook = &LIQUID_FLOWS[p.look];
            let c = world.currents.at(p.at);
            p.at[0] += f64::from(c[0]) * dt;
            p.at[2] += f64::from(c[1]) * dt;
            p.age += dt;
            let k = (VEL_EASE * dt).min(1.0) as f32;
            p.vel = [
                p.vel[0] + (c[0] - p.vel[0]) * k,
                p.vel[1] + (c[1] - p.vel[1]) * k,
            ];
            let far = (p.at[0] - eye[0]).hypot(p.at[2] - eye[2]) > look.particle_radius_m * 1.5;
            // Gone once its eased current is as slow as `particle_from_ms`: it has faded out by
            // then (`speed_share`), so it doesn't pop.
            if p.age >= look.particle_life_s || far || p.speed() <= look.particle_from_ms {
                return false;
            }
            match surface_at(
                world,
                p.at[0].floor() as i32,
                p.at[2].floor() as i32,
                p.at[1],
            ) {
                Some((y, _)) if (y - p.at[1]).abs() < 1.0 => {
                    p.at[1] = y;
                    true
                }
                _ => false,
            }
        });
        // New ones: each liquid's count over its life a second, a few tries a frame; slower water
        // keeps fewer of its tries (`speed_share`), so slow water has few and still water none.
        let Some(first) = LIQUID_FLOWS.first() else {
            return;
        };
        self.credit += f64::from(first.particle_count) / first.particle_life_s * dt;
        let mut tries = 0;
        while self.credit >= 1.0 && tries < 16 {
            tries += 1;
            let (a, r) = (self.random() * std::f64::consts::TAU, self.random().sqrt());
            let x = eye[0] + a.cos() * r * first.particle_radius_m;
            let z = eye[2] + a.sin() * r * first.particle_radius_m;
            let Some((y, look)) = surface_at(world, x.floor() as i32, z.floor() as i32, eye[1])
            else {
                continue;
            };
            let l = &LIQUID_FLOWS[look];
            let at = [x, y, z];
            let here = self.list.iter().filter(|p| p.look == look).count();
            let vel = world.currents.at(at);
            let share = speed_share(l, speed(vel));
            if share <= 0.0 || here >= l.particle_count as usize {
                continue;
            }
            // A try in slower water is spent without a fleck as often as the water is slow.
            self.credit -= 1.0;
            if self.random() >= share {
                continue;
            }
            let seed = (self.random() * f64::from(u32::MAX)) as u32;
            self.list.push(Fleck {
                at,
                age: 0.0,
                look,
                vel,
                seed,
            });
        }
        // Still water all round: no backlog.
        self.credit = self.credit.min(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::currents::{CA, current_key};

    /// A pool on face `f`: water rows 0 and 1 over stone, x −20..20, z −20..20, near y 0 local,
    /// its columns given a current `c` (m/s along x, z).
    fn pool(f: FaceFrame, c: [f32; 2]) -> VoxelWorld {
        let mut w = VoxelWorld::new(dwell_world::empty_generator(), None);
        let base = dwell_world::FLAT_TOP_Y + 40;
        for x in -24..24 {
            for z in -24..24 {
                {
                    let [a, b, c] = f.cell([x, base - 1, z]);
                    w.set_voxel(a, b, c, require("dwell:stone"));
                }
                {
                    let [a, b, c] = f.cell([x, base, z]);
                    w.set_voxel(a, b, c, require("dwell:water"));
                }
                {
                    let [a, b, c] = f.cell([x, base + 1, z]);
                    w.set_voxel(a, b, c, require("dwell:water"));
                }
                let (key, i) = current_key(f, x, z);
                let mut cells = w
                    .currents
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| Box::new([[0.0; 2]; CA]));
                cells[i] = c;
                w.currents.set(key, Some(cells));
            }
        }
        w
    }

    fn eye(f: FaceFrame) -> [f64; 3] {
        let base = dwell_world::FLAT_TOP_Y + 40;
        f.point([0.5, m_of_row(base + 2) + 1.5, 0.5])
    }

    dwell_world::both_faces!(
        fn flecks_spawn_on_moving_water_and_drift_with_it(f: FaceFrame) {
            let w = pool(f, [0.0, 1.0]);
            let mut fl = Flecks::default();
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(fl.list.len() > 10, "{} flecks", fl.list.len());
            // On the surface: the top cell's drawn level, face-locally.
            let base = dwell_world::FLAT_TOP_Y + 40;
            let drop = FLUIDS[0].surface_drop * CELL_H;
            let surface = f.y(m_of_row(base + 2) - drop);
            for p in &fl.list {
                assert!(
                    (p.at[1] - surface).abs() < 1e-6,
                    "{p:?} off the surface {surface}"
                );
            }
            // Carried downstream (+z) at the current's speed.
            let before: Vec<f64> = fl.list.iter().map(|p| p.at[2]).collect();
            let ids = fl.list.len();
            fl.update(&w, eye(f), 0.5, true);
            let moved = fl
                .list
                .iter()
                .take(ids.min(fl.list.len()))
                .zip(&before)
                .filter(|(p, z)| (p.at[2] - **z - 0.5).abs() < 1e-6)
                .count();
            assert!(moved > 5, "{moved} carried 0.5 m");
        }
    );

    dwell_world::both_faces!(
        fn still_water_and_the_setting_off_have_none(f: FaceFrame) {
            let w = pool(f, [0.0, 0.0]);
            let mut fl = Flecks::default();
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(fl.list.is_empty(), "still water: {} flecks", fl.list.len());
            let w = pool(f, [0.6, 0.0]);
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(!fl.list.is_empty());
            fl.update(&w, eye(f), 1.0 / 60.0, false);
            assert!(fl.list.is_empty(), "the setting off clears them");
        }
    );

    dwell_world::both_faces!(
        fn slow_water_has_fewer_flecks_than_fast(f: FaceFrame) {
            // Owner, 2026-11-06: fewer (or none) in slow water.
            let l = &LIQUID_FLOWS[0];
            let count = |speed: f64| {
                let w = pool(f, [speed as f32, 0.0]);
                let mut fl = Flecks::default();
                let mut most = 0;
                for _ in 0..360 {
                    fl.update(&w, eye(f), 1.0 / 60.0, true);
                    most = most.max(fl.list.len());
                }
                most
            };
            let slow = count(l.particle_from_ms + 0.25 * (l.particle_full_ms - l.particle_from_ms));
            let fast = count(l.particle_full_ms + 0.2);
            assert_eq!(count(l.particle_from_ms), 0, "none at the threshold");
            assert!(fast > 10 && slow * 2 < fast, "slow {slow}, fast {fast}");
        }
    );

    #[test]
    fn a_fleck_eases_in_and_out_and_is_never_opaque() {
        // Owner, 2026-11-06: no popping and no solid white. Its opacity rises from 0 over
        // `particle_fade_in_s` (eased: a quarter of the way in, under a fifth of its most), holds
        // at `particle_opacity` (< 1) and falls to 0 over `particle_fade_out_s`; scaled by the
        // current, none at `particle_from_ms`.
        let l = &LIQUID_FLOWS[0];
        let fast = l.particle_full_ms + 0.5;
        let a = |age: f64| fleck_alpha(l, age, fast);
        let (life, fin, fout) = (
            l.particle_life_s,
            l.particle_fade_in_s,
            l.particle_fade_out_s,
        );
        assert!(
            fin >= 1.0 && fout >= 1.0,
            "seconds, not a pop: {fin} {fout}"
        );
        assert_eq!(a(0.0), 0.0);
        assert!(
            a(0.25 * fin) < 0.2 * l.particle_opacity,
            "{}",
            a(0.25 * fin)
        );
        assert!((a(0.5 * fin) - 0.5 * l.particle_opacity).abs() < 1e-9);
        let mid = 0.5 * (fin + life - fout);
        assert!((a(mid) - l.particle_opacity).abs() < 1e-9 && l.particle_opacity < 1.0);
        assert!(a(life - 0.25 * fout) < 0.2 * l.particle_opacity);
        assert_eq!(a(life), 0.0);
        // Rising through the fade-in, falling through the fade-out, step by step.
        let steps: Vec<f64> = (0..=100).map(|k| a(life * f64::from(k) / 100.0)).collect();
        let peak = steps.iter().cloned().fold(0.0, f64::max);
        let top = steps.iter().position(|&v| v == peak).unwrap();
        assert!(steps[..=top].windows(2).all(|w| w[1] >= w[0]));
        assert!(steps[top..].windows(2).all(|w| w[1] <= w[0]));
        // The current: none at the threshold, part way between, all at full.
        assert_eq!(fleck_alpha(l, mid, l.particle_from_ms), 0.0);
        let half = fleck_alpha(l, mid, 0.5 * (l.particle_from_ms + l.particle_full_ms));
        assert!((half - 0.5 * l.particle_opacity).abs() < 1e-9, "{half}");
    }
}
