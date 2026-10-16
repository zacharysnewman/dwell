//! A brute-force DDA: one cell at a time at a single level, over any voxel source — what the
//! marcher's skipping must agree with. It uses the marcher's plane arithmetic ([`RayMath`]), so a
//! hit both find has the same bits.

use dwell_blocks::StateId;

use crate::march::{Hit, March, Outcome, Ray, RayMath};

/// Steps cell by cell at `level` from the ray's cell; `occupied(cell)` (level cells) is the state
/// a primary ray stops at there, if any.
pub fn brute_march(
    ray: &Ray,
    level: u32,
    occupied: impl Fn([i32; 3]) -> Option<StateId>,
    max_t: f32,
    step_cap: u32,
) -> March {
    let rm = RayMath::new(ray);
    let mut cell = rm.c.map(|c| c >> level);
    let mut t = 0.0;
    let mut face = None;
    let mut steps = 0;
    loop {
        if let Some(state) = occupied(cell) {
            return March {
                outcome: Outcome::Hit(Hit {
                    level,
                    cell,
                    face,
                    t,
                    state,
                }),
                steps,
            };
        }
        steps += 1;
        if steps > step_cap {
            return March {
                outcome: Outcome::Cap,
                steps,
            };
        }
        let lo = cell.map(|c| c << level);
        let hi = cell.map(|c| (c + 1) << level);
        let (te, a) = rm.exit(lo, hi);
        if te > max_t {
            return March {
                outcome: Outcome::Miss,
                steps,
            };
        }
        t = te;
        cell[a] += rm.step[a];
        face = Some(rm.face(a));
    }
}
