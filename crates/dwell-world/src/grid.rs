//! The grid frame (MOON_WORLD.md §2, phase M-3): a voxel grid's rigid transform in the world, so
//! code crosses **world ↔ grid ↔ face-local** through one API, as face-local code crosses into the
//! world through [`FaceFrame`] (BIFACIAL_WORLD.md §11).
//!
//! A **grid** is a voxel lattice with its own frame: the home world (at rest, the identity), the
//! moon (on its orbit, M-4/M-7), a built body (its Rapier pose, BUILT_BODIES.md). A [`GridFrame`]
//! is that frame: the grid's origin in the world (f64), its orientation (a unit quaternion, grid →
//! world), its motion (the velocity of its origin and its spin, for capture, MOON_WORLD.md §7) and
//! its gravity scale. Inside a grid, the grid's own faces are crossed by [`FaceFrame`] exactly as on
//! the home world: a point's face is the side of the grid's midplane it is on.
//!
//! **The rule.** Code that places something in a grid takes `(GridFrame, FaceFrame)`: the grid
//! frame turns the world into the grid's space ([`GridFrame::to_grid`], [`GridFrame::vector_to_grid`]),
//! the face frame turns that into face-local ([`GridFrame::to_local`] does both). The home grid's
//! frame, [`GridFrame::HOME`], is the identity and returns its input bit for bit, so every caller
//! of the home world keeps its results exactly (tests pin it).
//!
//! The rotation is the same arithmetic as glam's `DQuat * DVec3` (the physics' types), term for
//! term, so a body's transform swapped onto a `GridFrame` gives the same bits
//! (`dwell-physics` `tests/grid_frame.rs`).

use crate::{FaceFrame, cell_of_pos, centre_of_cell, pos_of_cell};

/// A grid's frame: where it is in the world, how it is turned, how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridFrame {
    /// The grid's origin (its `[0, 0, 0]`) in the world, m.
    pub origin: [f64; 3],
    /// The grid's orientation, grid → world, a unit quaternion `[x, y, z, w]`.
    pub rotation: [f64; 4],
    /// The velocity of the grid's origin, world, m/s.
    pub velocity: [f64; 3],
    /// The grid's angular velocity, world, rad/s.
    pub spin: [f64; 3],
    /// Its surface gravity × the home world's (`grids.json` `gravityScale`).
    pub gravity_scale: f64,
}

impl Default for GridFrame {
    fn default() -> Self {
        Self::HOME
    }
}

const IDENTITY: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

impl GridFrame {
    /// The home grid at rest: the identity.
    pub const HOME: Self = Self {
        origin: [0.0; 3],
        rotation: IDENTITY,
        velocity: [0.0; 3],
        spin: [0.0; 3],
        gravity_scale: 1.0,
    };

    /// A grid at a pose (origin, unit quaternion `[x, y, z, w]`), at rest, home gravity.
    pub const fn at_pose(origin: [f64; 3], rotation: [f64; 4]) -> Self {
        Self {
            origin,
            rotation,
            velocity: [0.0; 3],
            spin: [0.0; 3],
            gravity_scale: 1.0,
        }
    }
    /// This frame moving with `velocity` (its origin's) and `spin` (world).
    pub const fn moving(self, velocity: [f64; 3], spin: [f64; 3]) -> Self {
        Self {
            velocity,
            spin,
            ..self
        }
    }
    /// This frame with gravity × `scale`.
    pub const fn with_gravity_scale(self, scale: f64) -> Self {
        Self {
            gravity_scale: scale,
            ..self
        }
    }

    /// Whether the frame's pose is the identity (the home grid's): its maps return their input.
    pub fn is_rest(&self) -> bool {
        self.origin == [0.0; 3] && self.rotation == IDENTITY
    }

    // --- world ↔ grid -------------------------------------------------------------------------

    /// A world point in the grid's space.
    pub fn to_grid(&self, p: [f64; 3]) -> [f64; 3] {
        if self.is_rest() {
            return p;
        }
        rotate(conjugate(self.rotation), sub(p, self.origin))
    }
    /// A grid point in the world.
    pub fn to_world(&self, p: [f64; 3]) -> [f64; 3] {
        if self.is_rest() {
            return p;
        }
        add(self.origin, rotate(self.rotation, p))
    }
    /// A world direction (velocity, normal, ray) in the grid's space.
    pub fn vector_to_grid(&self, v: [f64; 3]) -> [f64; 3] {
        if self.rotation == IDENTITY {
            return v;
        }
        rotate(conjugate(self.rotation), v)
    }
    /// A grid direction in the world.
    pub fn vector_to_world(&self, v: [f64; 3]) -> [f64; 3] {
        if self.rotation == IDENTITY {
            return v;
        }
        rotate(self.rotation, v)
    }
    /// [`GridFrame::vector_to_grid`] for an `f32` direction (a view ray).
    pub fn vector_to_grid_f32(&self, v: [f32; 3]) -> [f32; 3] {
        if self.rotation == IDENTITY {
            return v;
        }
        self.vector_to_grid(v.map(f64::from)).map(|c| c as f32)
    }

    // --- world ↔ grid ↔ face-local --------------------------------------------------------

    /// The grid face a world point is on: the side of the grid's midplane it is on.
    pub fn face_at(&self, p: [f64; 3]) -> FaceFrame {
        FaceFrame::at_height(self.to_grid(p)[1])
    }
    /// A world point in face `f`'s local coordinates of this grid (+y the face's up).
    pub fn to_local(&self, f: FaceFrame, p: [f64; 3]) -> [f64; 3] {
        f.point(self.to_grid(p))
    }
    /// A face-local point of face `f` of this grid in the world.
    pub fn from_local(&self, f: FaceFrame, p: [f64; 3]) -> [f64; 3] {
        self.to_world(f.point(p))
    }
    /// A world direction in face `f`'s local coordinates.
    pub fn vector_to_local(&self, f: FaceFrame, v: [f64; 3]) -> [f64; 3] {
        f.vector(self.vector_to_grid(v))
    }
    /// A face-local direction of face `f` in the world.
    pub fn vector_from_local(&self, f: FaceFrame, v: [f64; 3]) -> [f64; 3] {
        self.vector_to_world(f.vector(v))
    }
    /// Face `f`'s up in the world.
    pub fn up(&self, f: FaceFrame) -> [f64; 3] {
        self.vector_to_world(f.up())
    }
    /// The unit direction of a face-local look on face `f` (yaw 0 = +Z, pitch toward the face's
    /// up, degrees), in the world.
    pub fn look(&self, f: FaceFrame, yaw_deg: f64, pitch_deg: f64) -> [f64; 3] {
        self.vector_to_world(f.look(yaw_deg, pitch_deg))
    }
    /// Gravity (m/s², world) at a world point for a home surface gravity of `g` m/s²: toward the
    /// grid's midplane from the face the point is on, × the grid's gravity scale.
    pub fn gravity(&self, p: [f64; 3], g: f64) -> [f64; 3] {
        let up = self.up(self.face_at(p));
        let k = -g * self.gravity_scale;
        [up[0] * k, up[1] * k, up[2] * k]
    }

    // --- cells ---------------------------------------------------------------------------

    /// The grid cell holding a world point.
    pub fn cell_of(&self, p: [f64; 3]) -> [i32; 3] {
        cell_of_pos(self.to_grid(p))
    }
    /// A grid cell's minimum corner, in the world.
    pub fn cell_min(&self, c: [i32; 3]) -> [f64; 3] {
        self.to_world(pos_of_cell(c))
    }
    /// A grid cell's centre, in the world.
    pub fn cell_centre(&self, c: [i32; 3]) -> [f64; 3] {
        self.to_world(centre_of_cell(c))
    }

    // --- motion (capture, MOON_WORLD.md §7) ------------------------------------------------

    /// The world velocity of the grid's point at world `p`: its origin's velocity plus its spin
    /// about the origin.
    pub fn point_velocity(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.velocity, cross(self.spin, sub(p, self.origin)))
    }
    /// A world point and velocity in the grid's space, as a body moving with the grid sees them
    /// (`p' = R⁻¹(p − c)`, `v' = R⁻¹(v − ċ − ω × (p − c))`): what changing a player's frame to the
    /// grid's keeps, so nothing jumps.
    pub fn capture(&self, p: [f64; 3], v: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        (
            self.to_grid(p),
            self.vector_to_grid(sub(v, self.point_velocity(p))),
        )
    }
    /// [`GridFrame::capture`]'s inverse: a grid point and velocity back in the world.
    pub fn release(&self, p: [f64; 3], v: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let w = self.to_world(p);
        (w, add(self.vector_to_world(v), self.point_velocity(w)))
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0]) + (a[1] * b[1]) + (a[2] * b[2])
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - b[1] * a[2],
        a[2] * b[0] - b[2] * a[0],
        a[0] * b[1] - b[0] * a[1],
    ]
}
fn conjugate([x, y, z, w]: [f64; 4]) -> [f64; 4] {
    [-x, -y, -z, w]
}
/// `q * v`, term for term as glam's `DQuat::mul_vec3` (so a body's pose gives the same bits).
fn rotate([x, y, z, w]: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let b = [x, y, z];
    let b2 = dot(b, b);
    let s = w * w - b2;
    let d = dot(v, b) * 2.0;
    let c = cross(b, v);
    let w2 = w * 2.0;
    [
        v[0] * s + b[0] * d + c[0] * w2,
        v[1] * s + b[1] * d + c[1] * w2,
        v[2] * s + b[2] * d + c[2] * w2,
    ]
}

/// A unit quaternion `[x, y, z, w]` turning `angle` (rad) about the unit `axis`.
pub fn quat_axis_angle(axis: [f64; 3], angle: f64) -> [f64; 4] {
    let (s, c) = (angle * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CELL_H;

    /// A small deterministic generator (no dependency): splitmix64 to [−1, 1).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        }
        fn vec(&mut self, scale: f64) -> [f64; 3] {
            [
                self.next() * scale,
                self.next() * scale,
                self.next() * scale,
            ]
        }
        fn unit_quat(&mut self) -> [f64; 4] {
            loop {
                let q = [self.next(), self.next(), self.next(), self.next()];
                let n = (q.iter().map(|c| c * c).sum::<f64>()).sqrt();
                if n > 0.1 {
                    return q.map(|c| c / n);
                }
            }
        }
        /// A moving, turned grid somewhere out to the moon's distance (~600 km).
        fn grid(&mut self) -> GridFrame {
            GridFrame::at_pose(self.vec(600_000.0), self.unit_quat())
                .moving(self.vec(1000.0), self.vec(0.01))
                .with_gravity_scale(0.1667)
        }
    }

    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
    }

    #[test]
    fn the_home_frame_returns_every_input_bit_for_bit() {
        let h = GridFrame::HOME;
        assert!(h.is_rest());
        assert_eq!(GridFrame::default(), h);
        let mut r = Rng(1);
        for _ in 0..2000 {
            let p = r.vec(30_000.0);
            for got in [
                h.to_grid(p),
                h.to_world(p),
                h.vector_to_grid(p),
                h.vector_to_world(p),
            ] {
                assert_eq!(got.map(f64::to_bits), p.map(f64::to_bits));
            }
            let v = p.map(|c| c as f32);
            assert_eq!(h.vector_to_grid_f32(v), v);
            assert_eq!(h.cell_of(p), cell_of_pos(p));
            let c = cell_of_pos(p);
            assert_eq!(h.cell_min(c), pos_of_cell(c));
            assert_eq!(h.cell_centre(c), centre_of_cell(c));
            assert_eq!(h.capture(p, p), (p, p));
        }
        // Negative zeros too.
        assert_eq!(
            h.to_grid([-0.0; 3]).map(f64::to_bits),
            [(-0.0f64).to_bits(); 3]
        );
    }

    crate::both_faces!(
        /// On the home grid every face map is the face frame's own: the identity grid adds nothing.
        fn the_home_frame_is_the_face_frame_on_each_face(f: FaceFrame) {
            let h = GridFrame::HOME;
            let mut r = Rng(2);
            for _ in 0..500 {
                let p = r.vec(5000.0);
                assert_eq!(h.to_local(f, p), f.point(p));
                assert_eq!(h.from_local(f, p), f.point(p));
                assert_eq!(h.vector_to_local(f, p), f.vector(p));
                assert_eq!(h.vector_from_local(f, p), f.vector(p));
                assert_eq!(h.face_at(f.point(p.map(|c| c.abs() + 1.0))), f);
                assert_eq!(h.face_at(p), FaceFrame::at_height(p[1]));
            }
            assert_eq!(h.up(f), f.up());
            assert_eq!(h.look(f, 30.0, -20.0), f.look(30.0, -20.0));
            let g = h.gravity(f.point([3.0, 40.0, -7.0]), 9.81);
            assert_eq!(g, [0.0, -9.81 * f.sign_f64(), 0.0]);
        }
    );

    #[test]
    fn a_moving_turned_grids_points_vectors_and_cells_round_trip() {
        let mut r = Rng(3);
        for _ in 0..200 {
            let g = r.grid();
            for _ in 0..20 {
                // Points near the grid (a 12.5 km disc): ~1e-9 relative at 600 km out.
                let local = r.vec(12_500.0);
                let w = g.to_world(local);
                assert!(near(g.to_grid(w), local, 1e-6), "{local:?}");
                let wp = r.vec(600_000.0);
                assert!(near(g.to_world(g.to_grid(wp)), wp, 1e-6));
                // Vectors keep their length and round-trip.
                let v = r.vec(10.0);
                let gv = g.vector_to_grid(v);
                assert!(near(g.vector_to_world(gv), v, 1e-12));
                assert!((dot(gv, gv).sqrt() - dot(v, v).sqrt()).abs() < 1e-12);
                // Distances between points are kept (a rigid transform).
                let (a, b) = (r.vec(1000.0), r.vec(1000.0));
                let d = sub(g.to_world(a), g.to_world(b));
                assert!((dot(d, d).sqrt() - dot(sub(a, b), sub(a, b)).sqrt()).abs() < 1e-6);
                // A cell's centre, in the world, lies in that cell.
                let c = cell_of_pos(local);
                assert_eq!(g.cell_of(g.cell_centre(c)), c);
                let m = g.cell_min(c);
                assert!(near(g.to_grid(m), pos_of_cell(c), 1e-6));
                // Capture and release keep a point and velocity.
                let (pv, vv) = (r.vec(600_000.0), r.vec(900.0));
                let (cp, cv) = g.capture(pv, vv);
                let (bp, bv) = g.release(cp, cv);
                assert!(near(bp, pv, 1e-6) && near(bv, vv, 1e-6), "{pv:?} {vv:?}");
            }
        }
    }

    #[test]
    fn a_point_carried_by_the_grid_has_no_velocity_in_it() {
        let mut r = Rng(4);
        let g = r.grid();
        let p = g.to_world([10.0, 80.0, -3.0]);
        let (_, v) = g.capture(p, g.point_velocity(p));
        assert!(near(v, [0.0; 3], 1e-9), "{v:?}");
    }

    crate::both_faces!(
        /// Gravity on a turned grid points toward its midplane from the face the point is on, at
        /// the grid's scale; up and looks turn with the grid.
        fn gravity_points_to_the_grids_midplane_on_each_face(f: FaceFrame) {
            let mut r = Rng(5);
            for _ in 0..100 {
                let g = r.grid();
                // A point 40 m over face f's ground, face-local.
                let local = [
                    r.next() * 1000.0,
                    crate::MIDPLANE_M + 40.0,
                    r.next() * 1000.0,
                ];
                let w = g.from_local(f, local);
                assert_eq!(g.face_at(w), f);
                let grav = g.gravity(w, 9.81);
                // Toward the midplane: in the face's local frame, straight down.
                let l = g.vector_to_local(f, grav);
                assert!(near(l, [0.0, -9.81 * 0.1667, 0.0], 1e-9), "{l:?}");
                // Up is opposite gravity and unit length.
                let up = g.up(f);
                assert!((dot(up, up) - 1.0).abs() < 1e-12);
                assert!(dot(up, grav) < 0.0);
                // A look straight up the face is its up.
                assert!(near(g.look(f, 0.0, 90.0), up, 1e-12));
                // Face-local round trip.
                assert!(near(g.to_local(f, w), local, 1e-6));
                let v = r.vec(3.0);
                assert!(near(
                    g.vector_to_local(f, g.vector_from_local(f, v)),
                    v,
                    1e-12
                ));
            }
        }
    );

    #[test]
    fn a_quarter_turn_moves_cells_where_expected() {
        // Turned 90° about +x and moved: the grid's +y is the world's +z.
        let q = quat_axis_angle([1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2);
        let g = GridFrame::at_pose([100.0, 0.0, 0.0], q);
        assert!(near(
            g.vector_to_world([0.0, 1.0, 0.0]),
            [0.0, 0.0, 1.0],
            1e-15
        ));
        let c = [2, 6, 0];
        let centre = g.cell_centre(c);
        let expected = [102.5, 0.0 - 0.5, (6.0 + 0.5) * CELL_H];
        assert!(near(centre, expected, 1e-12), "{centre:?}");
        assert_eq!(g.cell_of(centre), c);
    }
}
