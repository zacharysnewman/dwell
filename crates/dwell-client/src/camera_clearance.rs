//! The camera clearance check (docs/THEATER_MODE.md §6.2; owner, 2026-11-14: "That flyover video goes
//! straight through a wall. … that shouldn't be included in most recordings."): whether a camera eye is
//! clear of the world's solid blocks within a radius, and how far along its face's up it would need to
//! rise to be clear. The voxels come from a reader: the local world's server (`LocalCore::camera_clearance`:
//! its chunks loaded or generated, so every eye is known, trees and edits included), or the client's
//! loaded chunks (a read that never generates: a remote world's eye far from the player is unknown).
//!
//! The eye is probed at its centre and at the six points `radius` away along the axes; a probe point is
//! in a solid when its cell's state is `solid` and the point is inside the cell's shape
//! (`dwell_blocks::shape::point_in_solid`, so ramps and inverted face-B shapes count by their solid
//! only). A probe point the reader can't give (`None`: a chunk not held) makes the eye
//! [`Clearance::Unknown`] (unless another point is already in a solid). The probe is symmetric, so it is face-independent; the lift
//! goes along the eye's face's up (`FaceFrame`).

use dwell_blocks::StateId;
use dwell_world::{FaceFrame, VoxelWorld};

/// What a probe of one eye found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clearance {
    Clear,
    /// Within the radius of a solid block.
    Solid,
    /// Part of the probe fell in a chunk that isn't loaded.
    Unknown,
}

impl Clearance {
    /// The page's code for it: 1 solid, 0 clear, -1 unknown.
    pub fn code(self) -> i8 {
        match self {
            Clearance::Clear => 0,
            Clearance::Solid => 1,
            Clearance::Unknown => -1,
        }
    }
}

/// The client's reader: its loaded chunks only (`None` where a chunk isn't held).
pub fn held(world: &VoxelWorld) -> impl FnMut([i32; 3]) -> Option<StateId> + '_ {
    |c| world.voxel_if_held(c)
}

/// Probes the eye at `p` (m) against the solid blocks within `radius_m`, its voxels read by `voxel`.
pub fn probe(
    voxel: &mut impl FnMut([i32; 3]) -> Option<StateId>,
    p: [f64; 3],
    radius_m: f64,
) -> Clearance {
    let r = radius_m.max(0.0);
    let offsets: [[f64; 3]; 7] = [
        [0.0, 0.0, 0.0],
        [r, 0.0, 0.0],
        [-r, 0.0, 0.0],
        [0.0, r, 0.0],
        [0.0, -r, 0.0],
        [0.0, 0.0, r],
        [0.0, 0.0, -r],
    ];
    let mut unknown = false;
    for o in offsets {
        let q = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
        let cell = dwell_world::cell_of_pos(q);
        let Some(state) = voxel(cell) else {
            unknown = true;
            continue;
        };
        if !dwell_blocks::info(state).solid {
            continue;
        }
        let base = dwell_world::pos_of_cell(cell);
        let [fx, fy, fz] = [
            (q[0] - base[0]) as f32,
            ((q[1] - base[1]) / dwell_world::CELL_H) as f32,
            (q[2] - base[2]) as f32,
        ];
        let shape = dwell_blocks::shape::shape_of(state);
        if dwell_blocks::shape::point_in_solid(shape, fx, fy, fz, 0.0) {
            return Clearance::Solid;
        }
    }
    if unknown {
        Clearance::Unknown
    } else {
        Clearance::Clear
    }
}

/// How far (m, in `step_m` steps) the eye at `p` must rise along its face's up to be clear, at most
/// `max_m`: `None` when it is never clear (or unknown) within that.
pub fn lift(
    voxel: &mut impl FnMut([i32; 3]) -> Option<StateId>,
    p: [f64; 3],
    radius_m: f64,
    step_m: f64,
    max_m: f64,
) -> Option<f64> {
    let up = FaceFrame::at_height(p[1]).vector([0.0, 1.0, 0.0]);
    let step = step_m.max(0.05);
    let mut h = 0.0;
    while h <= max_m + 1e-9 {
        let q = [p[0] + up[0] * h, p[1] + up[1] * h, p[2] + up[2] * h];
        match probe(voxel, q, radius_m) {
            Clearance::Clear => return Some(h),
            Clearance::Unknown => return None,
            Clearance::Solid => h += step,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    /// A face-local stone wall 4 m thick (x 10..14), 20 rows tall from row 40, over z -5..5, on face
    /// `f`, and the air around it held (the chunks loaded).
    fn walled(f: FaceFrame) -> VoxelWorld {
        let mut w = VoxelWorld::streamed();
        for x in -2..24 {
            for y in 36..70 {
                for z in -8..8 {
                    let s =
                        if (10..14).contains(&x) && (40..60).contains(&y) && (-5..5).contains(&z)
                            || y < 40
                        {
                            require("dwell:stone")
                        } else {
                            dwell_blocks::AIR
                        };
                    let [cx, cy, cz] = f.cell([x, y, z]);
                    w.set_voxel(cx, cy, cz, f.state(s));
                }
            }
        }
        w
    }

    #[test]
    fn a_generating_reader_knows_every_eye() {
        // face-a-only: the flat generator builds face A only; the probe's faces are the tests above.
        // The local world's server reads through its generator: never unknown.
        let mut w = VoxelWorld::new(dwell_world::flat_generator(), None);
        let mut read = |c: [i32; 3]| Some(w.get_voxel(c[0], c[1], c[2]));
        assert_eq!(
            probe(&mut read, [500.5, 900.0, 500.5], 0.3),
            Clearance::Clear
        );
        assert_eq!(probe(&mut read, [500.5, 1.0, 500.5], 0.3), Clearance::Solid);
    }

    /// Face-local metres (row 40 is 20 m) in the world.
    fn at(f: FaceFrame, p: [f64; 3]) -> [f64; 3] {
        f.point(p)
    }

    dwell_world::both_faces!(
        fn an_eye_in_a_wall_or_near_it_is_solid_and_one_in_the_open_is_clear(f: FaceFrame) {
            let w = walled(f);
            // Inside the wall, at its middle.
            assert_eq!(
                probe(&mut held(&w), at(f, [12.0, 25.0, 0.5]), 0.3),
                Clearance::Solid
            );
            // 0.2 m from its face: within 0.3 m.
            assert_eq!(
                probe(&mut held(&w), at(f, [9.8, 25.0, 0.5]), 0.3),
                Clearance::Solid
            );
            // 0.5 m from its face, 5 m above the ground: clear.
            assert_eq!(
                probe(&mut held(&w), at(f, [9.5, 25.0, 0.5]), 0.3),
                Clearance::Clear
            );
            // Just above the ground (its top at 20 m): within the radius.
            assert_eq!(
                probe(&mut held(&w), at(f, [5.5, 20.2, 0.5]), 0.3),
                Clearance::Solid
            );
            assert_eq!(
                probe(&mut held(&w), at(f, [5.5, 20.5, 0.5]), 0.3),
                Clearance::Clear
            );
        }
    );

    dwell_world::both_faces!(
        fn an_eye_where_no_chunk_is_held_is_unknown(f: FaceFrame) {
            let w = walled(f);
            assert_eq!(
                probe(&mut held(&w), at(f, [500.5, 25.0, 500.5]), 0.3),
                Clearance::Unknown
            );
        }
    );

    dwell_world::both_faces!(
        fn the_lift_out_of_a_wall_goes_up_the_face_to_its_top(f: FaceFrame) {
            let w = walled(f);
            // The wall's top is at row 60 = 30 m; from 25 m the eye clears at 30.3 m: a 5.5 m lift
            // in 0.5 m steps.
            assert_eq!(
                lift(&mut held(&w), at(f, [12.0, 25.0, 0.5]), 0.3, 0.5, 20.0),
                Some(5.5)
            );
            assert_eq!(
                lift(&mut held(&w), at(f, [12.0, 25.0, 0.5]), 0.3, 0.5, 3.0),
                None
            );
            assert_eq!(
                lift(&mut held(&w), at(f, [5.5, 25.0, 0.5]), 0.3, 0.5, 20.0),
                Some(0.0)
            );
        }
    );
}
