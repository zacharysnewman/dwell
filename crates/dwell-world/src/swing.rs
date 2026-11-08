//! A melee swing's path (docs/adr/0067-stone-axe-and-chopping.md, "The swing's path"): the tool's
//! head sweeps an arc round the eye, and the first solid cell on that path is what it hits, not
//! what the crosshair points at. The server sweeps it (authoritative) and the client sweeps the
//! same arc for its feel (the hit-stop, the bounce, the chips).
//!
//! The arc is face-local (+y up, BIFACIAL_WORLD.md §11): a level swing turns the look about the
//! face's up from the screen's right to its left (a mirror reverses handedness: `FaceFrame::handed`),
//! a vertical one pitches it from above the look to below. Each step is a ray from the eye out to
//! the head's reach, so the swept sector is covered in the order the head passes through it.

use crate::edit::{BlockHit, raycast_block_past};
use crate::{FaceFrame, VoxelWorld, row_of_m};

/// What a swing's arc is (from the tool's data: `dwell_protocol::chopping::SwingDef`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    /// The head's distance from the eye (m).
    pub reach_m: f64,
    /// The arc swept, centred on the look (degrees).
    pub arc_deg: f64,
    /// Steps along it (at least 2).
    pub samples: u32,
}

impl Arc {
    /// A tool's arc (its `swing` data).
    pub fn of(t: &dwell_protocol::chopping::ToolDef) -> Self {
        Self {
            reach_m: t.swing.reach_m,
            arc_deg: t.swing.arc_deg,
            samples: t.swing.samples,
        }
    }
}

/// A swing's first hit along its arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingHit {
    pub hit: BlockHit,
    /// Where the head met it (world metres).
    pub point: [f64; 3],
    /// The arc step it was met at (0: the arc's start).
    pub step: u32,
}

/// The arc's ray directions (world, unit), in the order the head sweeps them, for a look `look`
/// (world, unit) from an eye on face `f`.
pub fn arc_dirs(f: FaceFrame, look: [f64; 3], vertical: bool, arc: &Arc) -> Vec<[f64; 3]> {
    let l = f.vector(look);
    let yaw = l[0].atan2(l[2]);
    let pitch = l[1].clamp(-1.0, 1.0).asin();
    let half = arc.arc_deg.to_radians() / 2.0;
    let n = arc.samples.max(2);
    (0..n)
        .map(|i| {
            let u = f64::from(i) / f64::from(n - 1);
            let (y, p) = if vertical {
                // Top to bottom through the look.
                let p = (pitch + half - 2.0 * half * u).clamp(-1.55, 1.55);
                (yaw, p)
            } else {
                // The screen's right to its left: the face-local turn toward +yaw is to the left on
                // face A, to the right on face B (the mirror).
                (yaw + f.handed(-half + 2.0 * half * u), pitch)
            };
            f.vector([y.sin() * p.cos(), p.sin(), y.cos() * p.cos()])
        })
        .collect()
}

/// The first solid cell the arc meets (non-solid cells, such as plants, let the head through).
pub fn sweep(
    world: &mut VoxelWorld,
    eye: [f64; 3],
    look: [f64; 3],
    vertical: bool,
    arc: &Arc,
) -> Option<SwingHit> {
    let f = FaceFrame::at_row(row_of_m(eye[1]));
    for (step, d) in arc_dirs(f, look, vertical, arc).into_iter().enumerate() {
        let dir = d.map(|v| v as f32);
        let hit = raycast_block_past(world, eye, dir, arc.reach_m as f32, |_, m| {
            !dwell_blocks::info(m).solid
        });
        if let Some(hit) = hit {
            let t = f64::from(hit.distance);
            return Some(SwingHit {
                hit,
                point: std::array::from_fn(|k| eye[k] + d[k] * t),
                step: step as u32,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{M, MIDPLANE_Y, empty_generator, m_of_row};

    const ARC: Arc = Arc {
        reach_m: 2.2,
        arc_deg: 110.0,
        samples: 12,
    };
    /// A face-local row well up on face A (and its mirror on face B).
    const R0: i32 = MIDPLANE_Y + 40;

    fn log() -> dwell_blocks::StateId {
        dwell_blocks::parse_state("dwell:log").expect("a log")
    }
    fn world() -> VoxelWorld {
        VoxelWorld::new(empty_generator(), None)
    }
    /// Sets the face-local cell `l` on face `f`.
    fn put(w: &mut VoxelWorld, f: FaceFrame, l: [i32; 3], m: dwell_blocks::StateId) -> [i32; 3] {
        let c = f.cell([l[0], R0 + l[1], l[2]]);
        w.set_voxel(c[0], c[1], c[2], m);
        c
    }
    /// A face-local eye at the middle of cell (0, 3 rows up, 0), looking along face-local `look`.
    fn eye(f: FaceFrame) -> [f64; 3] {
        f.point([0.5, m_of_row(R0 + 3) + 0.25, 0.5])
    }
    /// The look turned `deg` to the player's right of +z, face-locally (a mirror: handed).
    fn look_right(f: FaceFrame, deg: f64) -> [f64; 3] {
        let a = f.handed(-deg.to_radians());
        f.vector([a.sin(), 0.0, a.cos()])
    }

    crate::both_faces!(
        fn a_level_arc_sweeps_from_the_screens_right_to_its_left(f: FaceFrame) {
            let look = f.vector([0.0, 0.0, 1.0]);
            let d = arc_dirs(f, look, false, &ARC);
            let right = look_right(f, 90.0);
            let r = |v: [f64; 3]| (0..3).map(|k| v[k] * right[k]).sum::<f64>();
            assert!(r(d[0]) > 0.7, "it starts on the right: {:?}", d[0]);
            assert!(r(d[d.len() - 1]) < -0.7, "and ends on the left");
            // Level: no climb, face-locally.
            assert!(d.iter().all(|v| f.vector(*v)[1].abs() < 1e-9));
            let v = arc_dirs(f, look, true, &ARC);
            assert!(
                f.vector(v[0])[1] > 0.7 && f.vector(v[v.len() - 1])[1] < -0.7,
                "top to bottom"
            );
        }
    );

    crate::both_faces!(
        fn an_arc_that_passes_a_trunk_hits_it_though_the_crosshair_misses(f: FaceFrame) {
            let mut w = world();
            // A log 1.2 m off to the right of the look, at eye height: the crosshair (straight
            // ahead) passes it by, but the level swing starts on the right and meets it.
            let side = f.handed(-1.0) as i32;
            let mut cells = Vec::new();
            for r in 0..8 {
                cells.push(put(&mut w, f, [side, r, 1], log()));
            }
            let e = eye(f);
            let look = f.vector([0.0, 0.0, 1.0]);
            let crosshair = crate::edit::raycast_block(&mut w, e, look.map(|v| v as f32), 2.2);
            assert!(crosshair.is_none(), "the crosshair misses: {crosshair:?}");
            let hit = sweep(&mut w, e, look, false, &ARC).expect("the swing's path meets the log");
            assert!(cells.contains(&hit.hit.cell), "{hit:?}");
            assert!(
                hit.step < ARC.samples / 2,
                "on the arc's first half (the right): {hit:?}"
            );
            // A vertical swing's path (straight ahead) passes it by.
            assert_eq!(sweep(&mut w, e, look, true, &ARC), None);
        }
    );

    crate::both_faces!(
        fn a_nearer_block_on_the_path_takes_the_hit_before_the_trunk_aimed_at(f: FaceFrame) {
            let mut w = world();
            let mut trunk = Vec::new();
            for r in 0..8 {
                trunk.push(put(&mut w, f, [0, r, 2], log()));
            }
            let e = eye(f);
            let look = f.vector([0.0, 0.0, 1.0]);
            let hit = sweep(&mut w, e, look, false, &ARC).expect("the trunk ahead");
            assert!(
                trunk.contains(&hit.hit.cell),
                "alone, the trunk aimed at: {hit:?}"
            );
            // A stone post at the right, between the swing's start and the trunk.
            let side = f.handed(-1.0) as i32;
            let post = put(&mut w, f, [side, 3, 1], M.stone);
            let hit = sweep(&mut w, e, look, false, &ARC).expect("something");
            assert_eq!(hit.hit.cell, post, "the post on the path takes it: {hit:?}");
        }
    );

    crate::both_faces!(
        fn a_swing_at_nothing_hits_nothing(f: FaceFrame) {
            let mut w = world();
            let e = eye(f);
            let look = f.vector([0.0, 0.0, 1.0]);
            assert_eq!(sweep(&mut w, e, look, false, &ARC), None);
            assert_eq!(sweep(&mut w, e, look, true, &ARC), None);
        }
    );
}
