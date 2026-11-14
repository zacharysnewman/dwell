//! The grid frame in the physics (MOON_WORLD.md §2, M-3): a body's pose expressed as a
//! `dwell_world::GridFrame` maps points and vectors to the same bits as glam's `p + r * x` and
//! `r.inverse() * (x - p)` (the bodies' transform code that now goes through it), and the home
//! grid's frame picks the same controller frame as the face of a height. Both faces: the points
//! are drawn on both sides of the midplane.

use dwell_physics::frame::{Frame, GridMaps, grid_of_motion, grid_of_pose};
use dwell_physics::{DQuat, DVec3};
use dwell_world::{FaceFrame, GridFrame, MIDPLANE_M};

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
    fn v(&mut self, s: f64) -> DVec3 {
        DVec3::new(self.next() * s, self.next() * s, self.next() * s)
    }
    fn q(&mut self) -> DQuat {
        DQuat::from_xyzw(self.next(), self.next(), self.next(), self.next() + 1.5).normalize()
    }
}

fn bits(v: DVec3) -> [u64; 3] {
    v.to_array().map(f64::to_bits)
}

#[test]
fn a_bodys_pose_as_a_grid_frame_maps_to_the_same_bits_as_glam() {
    let mut r = Rng(11);
    for _ in 0..2000 {
        let (position, rotation) = (r.v(30_000.0), r.q());
        let g = grid_of_pose(position, rotation);
        let inverse = rotation.inverse();
        for _ in 0..8 {
            let x = r.v(50.0);
            assert_eq!(bits(g.to_world_v(x)), bits(position + rotation * x));
            // grid-boundary: glam's own transform, the reference the frame is compared with
            assert_eq!(bits(g.to_grid_v(x)), bits(inverse * (x - position)));
            assert_eq!(bits(g.vector_to_world_v(x)), bits(rotation * x));
            assert_eq!(bits(g.vector_to_grid_v(x)), bits(inverse * x));
        }
    }
}

#[test]
fn a_bodys_point_velocity_is_glams() {
    let mut r = Rng(12);
    for _ in 0..500 {
        let (p, q, v, w) = (r.v(1000.0), r.q(), r.v(20.0), r.v(3.0));
        let g = grid_of_motion(p, q, v, w);
        let at = r.v(1000.0);
        let want = v + w.cross(at - p);
        let got = DVec3::from(g.point_velocity(at.to_array()));
        assert!((got - want).length() < 1e-9, "{got:?} {want:?}");
    }
}

#[test]
fn the_home_grid_picks_the_controller_frame_of_the_height() {
    // both-faces: heights on both sides of the midplane, compared with FaceFrame's own choice.
    let mut r = Rng(13);
    for _ in 0..2000 {
        let p = r.v(5000.0) + DVec3::new(0.0, MIDPLANE_M, 0.0);
        let want = Frame {
            face: FaceFrame::at_height(p.y),
        };
        assert_eq!(GridFrame::HOME.frame_at(p), want);
        assert_eq!(bits(GridFrame::HOME.to_grid_v(p)), bits(p));
    }
    for y in [MIDPLANE_M, MIDPLANE_M - 1e-9, MIDPLANE_M + 1e-9] {
        let p = DVec3::new(1.0, y, 2.0);
        assert_eq!(GridFrame::HOME.frame_at(p).face, FaceFrame::at_height(y));
    }
}
