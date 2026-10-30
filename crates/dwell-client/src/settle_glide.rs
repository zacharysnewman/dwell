//! The settle glide (docs/FELLING.md §4.1): when a falling piece settles, the server writes its
//! blocks at once (collision and targeting have them at once). This client draws them gliding from
//! where the piece rested into their places, with a small bounce, and leaves them out of the
//! terrain meshes until the glide ends and the terrain draws them. Visual only: nothing on the
//! wire. The client finds where each cell went by the server's own rule (`dwell_sim::felling`'s
//! `resample` at the snapped rotation) matched against the blocks the `Rebake` change wrote: the
//! whole piece one or two rows higher where its place was taken, and a cell moved on its own (sand
//! sliding off a player) gliding from where it would have gone to where it went. The timing is
//! data (`shared/physics/felling.json`: `settleGlideSeconds`, `settleBounce`).

use dwell_blocks::{StateId, SupportRole, try_info};
use dwell_mesh::ChunkMeshes;
use dwell_physics::{DQuat, DVec3};
use dwell_sim::felling::{FELLING, cell_centre, resample, snap};
use dwell_world::hash::HashMap;
use dwell_world::{CELL_H, FaceFrame};

/// Cells and their states.
pub type Cells = Vec<([i32; 3], StateId)>;

/// Scene ids of the glides' meshes: the pieces' own ids (the server's) stay below it.
const SCENE_ID_BASE: u32 = 0x8000_0000;

/// A settled cell not claimed by a piece within this long (ms) is drawn by the terrain again.
const UNCLAIMED_MS: f64 = 500.0;

/// After its glide, a glide's meshes stay (at their places) until the terrain draws its cells, at
/// most this long (ms).
const HANDOFF_MS: f64 = 1000.0;

/// A piece that settled: its cells (offsets from its reference cell, this client's states), the
/// pose it is drawn at now, and its latest snapshot's pose (where the server settled it).
#[derive(Clone, Debug)]
pub struct SettledPiece {
    pub cells: Vec<([i32; 3], StateId)>,
    pub drawn: (DVec3, DQuat),
    pub last: (DVec3, DQuat),
}

/// One rigid part of a glide: world cells meshed round `anchor`'s centre, drawn from `start`
/// (the mesh origin's position and rotation) to `anchor`'s centre, unturned.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanPart {
    pub cells: Vec<([i32; 3], StateId)>,
    pub anchor: [i32; 3],
    pub start: (DVec3, DQuat),
}

/// Where a settled piece's blocks glide from: the piece's cells that landed as one (on the grid
/// at its snapped pose, lifted as the server lifted it), and each cell that moved on its own.
/// The cells it claims are taken out of `written` (a `Rebake` may hold several pieces).
pub fn plan(piece: &SettledPiece, written: &mut HashMap<[i32; 3], StateId>) -> Vec<PlanPart> {
    let rotation = snap(piece.last.1);
    let (aligned, placed) = resample(&piece.cells, piece.last.0, rotation);
    // The lift the server chose: the one most of the cells landed at (the lowest of equals).
    let lift = (0..=2)
        .max_by_key(|&up| {
            let n = placed
                .iter()
                .filter(|&&(c, m)| written.get(&[c[0], c[1] + up, c[2]]) == Some(&m))
                .count();
            (n, -up)
        })
        .unwrap_or(0);
    let target = aligned + DVec3::new(0.0, f64::from(lift) * CELL_H, 0.0);
    // The rigid move from where the piece settles to where it is drawn: target pose → drawn pose.
    let turn = piece.drawn.1 * rotation.inverse();
    let from = |world: DVec3| piece.drawn.0 + turn * (world - target);
    let mut rigid = Vec::new();
    let mut parts = Vec::new();
    for (c, m) in placed {
        let at = [c[0], c[1] + lift, c[2]];
        if written.get(&at) == Some(&m) {
            written.remove(&at);
            rigid.push((at, m));
            continue;
        }
        // A cell that falls slides off beside a player (the server's `settle`): from where it would
        // have gone to where it went.
        let falls = try_info(m).is_some_and(|s| s.support_role == SupportRole::Falls);
        if !falls {
            continue; // lost: its cell was taken
        }
        let beside = [[1, 0], [-1, 0], [0, 1], [0, -1]]
            .map(|d| [at[0] + d[0], at[1], at[2] + d[1]])
            .into_iter()
            .find(|n| written.get(n) == Some(&m));
        if let Some(n) = beside {
            written.remove(&n);
            parts.push(PlanPart {
                cells: vec![(n, m)],
                anchor: n,
                start: (from(cell_centre(at)), turn),
            });
        }
    }
    if let Some(&(anchor, _)) = rigid.first() {
        parts.insert(
            0,
            PlanPart {
                start: (from(cell_centre(anchor)), turn),
                anchor,
                cells: rigid,
            },
        );
    }
    parts
}

/// The glide's progress at `t` (0 → 1 over the glide): eased out, swinging `bounce` (a fraction of
/// the move) past the end before it rests there ("back" easing).
pub fn ease(t: f64, bounce: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let s = back_constant(bounce);
    let u = t - 1.0;
    1.0 + (s + 1.0) * u * u * u + s * u * u
}

/// The back easing's constant whose overshoot is `bounce`: the peak, at u = −2s / 3(s + 1), is
/// 4s³ / 27(s + 1)², increasing in s.
fn back_constant(bounce: f64) -> f64 {
    if bounce <= 0.0 {
        return 0.0;
    }
    let peak = |s: f64| 4.0 * s * s * s / (27.0 * (s + 1.0) * (s + 1.0));
    let (mut lo, mut hi) = (0.0, 1.0);
    while peak(hi) < bounce && hi < 1e6 {
        hi *= 2.0;
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if peak(mid) < bounce {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// A part's pose at progress `e`: its mesh origin from `start` to its anchor's centre, turning
/// from `start`'s rotation to none.
pub fn pose_at(part: &PlanPart, e: f64) -> (DVec3, DQuat) {
    let end = cell_centre(part.anchor);
    let position = part.start.0 + (end - part.start.0) * e;
    // Past the end (the bounce) slerp goes on along the same turn.
    let rotation = part.start.1.slerp(DQuat::IDENTITY, e);
    (position, rotation.normalize())
}

struct Glide {
    /// Scene ids of its parts, with the parts.
    parts: Vec<(u32, PlanPart)>,
    cells: Vec<[i32; 3]>,
    start_ms: f64,
    shown: bool,
}

/// What a frame's step asks of the game: the terrain cells to draw again and the scene meshes to
/// drop.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub show: Vec<[i32; 3]>,
    pub remove: Vec<u32>,
}

/// The glides now, and the settled cells waiting for their piece's despawn.
#[derive(Default)]
pub struct Glides {
    glides: Vec<Glide>,
    written: HashMap<[i32; 3], StateId>,
    written_ms: HashMap<[i32; 3], f64>,
    next: u32,
}

impl Glides {
    /// A `Rebake` change wrote these cells (state, not air): returns the cells to hide from the
    /// terrain until a piece's glide has claimed and shown them.
    pub fn rebaked(&mut self, cells: &[([i32; 3], StateId)], now_ms: f64) -> Vec<[i32; 3]> {
        if FELLING.settle_glide_seconds <= 0.0 {
            return Vec::new();
        }
        let mut hide = Vec::new();
        for &(c, m) in cells {
            if m == 0 {
                continue;
            }
            self.written.insert(c, m);
            self.written_ms.insert(c, now_ms);
            hide.push(c);
        }
        hide
    }

    /// A piece settled: its glide, as parts to mesh (scene id, cells relative to the anchor, and
    /// whether they're face B's), starting now.
    pub fn settled(&mut self, piece: &SettledPiece, now_ms: f64) -> Vec<(u32, Cells, bool)> {
        if FELLING.settle_glide_seconds <= 0.0 {
            return Vec::new();
        }
        let parts = plan(piece, &mut self.written);
        if parts.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut glide = Glide {
            parts: Vec::new(),
            cells: Vec::new(),
            start_ms: now_ms,
            shown: false,
        };
        for part in parts {
            let id = SCENE_ID_BASE | self.next;
            self.next = (self.next + 1) & !SCENE_ID_BASE;
            let a = part.anchor;
            let rel = part
                .cells
                .iter()
                .map(|&(c, m)| ([c[0] - a[0], c[1] - a[1], c[2] - a[2]], m))
                .collect();
            for &(c, _) in &part.cells {
                self.written_ms.remove(&c);
                glide.cells.push(c);
            }
            out.push((id, rel, FaceFrame::at_row(a[1]).is_b()));
            glide.parts.push((id, part));
        }
        self.glides.push(glide);
        out
    }

    /// The glides' poses at `now_ms` (scene id, mesh origin, rotation), as the pieces' are drawn.
    pub fn poses(&self, now_ms: f64) -> Vec<(u32, [f64; 3], [f32; 4])> {
        let seconds = FELLING.settle_glide_seconds.max(1e-6);
        let mut out = Vec::new();
        for g in &self.glides {
            let t = (now_ms - g.start_ms) / (seconds * 1000.0);
            let e = ease(t, FELLING.settle_bounce);
            for (id, part) in &g.parts {
                let (p, r) = pose_at(part, e);
                out.push((
                    *id,
                    p.to_array(),
                    [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
                ));
            }
        }
        out
    }

    /// Steps to `now_ms`: a glide that has ended shows its cells in the terrain again, and once
    /// the terrain draws them (`drawn`) its meshes go; settled cells no piece claimed show again.
    pub fn step(&mut self, now_ms: f64, drawn: impl Fn(&[[i32; 3]]) -> bool) -> Step {
        let mut step = Step::default();
        let end_ms = FELLING.settle_glide_seconds * 1000.0;
        self.glides.retain_mut(|g| {
            let age = now_ms - g.start_ms;
            if age < end_ms {
                return true;
            }
            if !g.shown {
                g.shown = true;
                step.show.extend_from_slice(&g.cells);
                return true;
            }
            if drawn(&g.cells) || age >= end_ms + HANDOFF_MS {
                step.remove.extend(g.parts.iter().map(|(id, _)| *id));
                return false;
            }
            true
        });
        let stale: Vec<[i32; 3]> = self
            .written_ms
            .iter()
            .filter(|&(_, &at)| now_ms - at >= UNCLAIMED_MS)
            .map(|(&c, _)| c)
            .collect();
        for c in stale {
            self.written_ms.remove(&c);
            self.written.remove(&c);
            step.show.push(c);
        }
        // Claimed cells are out of `written` (taken by `plan`); the rest wait for their piece.
        self.written.retain(|c, _| self.written_ms.contains_key(c));
        step
    }
}

/// A glide part's meshes: its cells (relative to the anchor) meshed round the anchor's centre.
pub fn part_meshes(cells: &[([i32; 3], StateId)], mirror: bool) -> (Vec<ChunkMeshes>, f64) {
    crate::pieces::piece_meshes_mirrored(cells, mirror)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str) -> StateId {
        dwell_blocks::parse_state(name).expect(name)
    }

    fn written_of(cells: &[([i32; 3], StateId)]) -> HashMap<[i32; 3], StateId> {
        cells.iter().copied().collect()
    }

    /// A log lying along x, resting a little off the grid and turned: it glides as one part from
    /// the drawn pose and ends exactly on its cells, unturned.
    #[test]
    fn a_piece_resting_off_the_grid_glides_from_its_pose_onto_its_cells() {
        let log = state("dwell:log[axis=x]");
        let cells: Vec<([i32; 3], StateId)> = (0..3).map(|x| ([x, 0, 0], log)).collect();
        let rest = (
            DVec3::new(10.53, 64.0 * CELL_H + 0.27, 4.46),
            DQuat::from_rotation_y(5f64.to_radians()),
        );
        let (_, placed) = resample(&cells, rest.0, snap(rest.1));
        let mut written = written_of(&placed);
        let piece = SettledPiece {
            cells,
            drawn: rest,
            last: rest,
        };
        let parts = plan(&piece, &mut written);
        assert!(written.is_empty(), "every written cell claimed");
        assert_eq!(parts.len(), 1);
        let part = &parts[0];
        assert_eq!(part.cells.len(), 3);
        let (p0, r0) = pose_at(part, 0.0);
        assert!(
            (r0.dot(rest.1).abs() - 1.0).abs() < 1e-9,
            "starts turned as drawn"
        );
        assert!(
            (p0 - cell_centre(part.anchor)).length() < 0.6,
            "starts near its place"
        );
        let (p1, r1) = pose_at(part, 1.0);
        assert!((p1 - cell_centre(part.anchor)).length() < 1e-9);
        assert!((r1.dot(DQuat::IDENTITY).abs() - 1.0).abs() < 1e-9);
    }

    dwell_world::both_faces!(
        /// At the start, every cell of the glide's mesh is where the piece drew it (the start pose
        /// maps each settled cell's centre onto the drawn piece's), and it lands on the face's
        /// mirror of face A's cells.
        fn the_glide_starts_where_the_piece_was_drawn(f: FaceFrame) {
            let log = state("dwell:log[axis=y]");
            // Face-local: a trunk 4 rows tall resting tipped a little, near row 70.
            let local: Vec<([i32; 3], StateId)> = (0..4).map(|y| ([0, y, 0], log)).collect();
            let rest_a = (
                DVec3::new(3.5 + 0.08, 70.5 * CELL_H - 0.04, -2.5 + 0.1),
                DQuat::from_rotation_z(0.06) * DQuat::from_rotation_x(-0.04),
            );
            let fixture = |f: FaceFrame| {
                // Through the face: offsets and the point mirrored, and the rotation (its axis an
                // axial vector: x and z turn over under the mirror).
                let cells: Vec<([i32; 3], StateId)> = local
                    .iter()
                    .map(|&(o, m)| {
                        (
                            [o[0], if f.is_b() { -o[1] } else { o[1] }, o[2]],
                            f.state(m),
                        )
                    })
                    .collect();
                let q = rest_a.1;
                let rotation = if f.is_b() {
                    DQuat::from_xyzw(-q.x, q.y, -q.z, q.w)
                } else {
                    q
                };
                (
                    cells,
                    (DVec3::from_array(f.point(rest_a.0.to_array())), rotation),
                )
            };
            let settled = |f: FaceFrame| {
                let (cells, rest) = fixture(f);
                let (_, placed) = resample(&cells, rest.0, snap(rest.1));
                let mut written = written_of(&placed);
                let piece = SettledPiece {
                    cells: cells.clone(),
                    drawn: rest,
                    last: rest,
                };
                let parts = plan(&piece, &mut written);
                assert!(written.is_empty());
                assert_eq!(parts.len(), 1);
                let part = parts[0].clone();
                let (p0, r0) = pose_at(&part, 0.0);
                let anchor = cell_centre(part.anchor);
                for &(c, _) in &part.cells {
                    let at_start = p0 + r0 * (cell_centre(c) - anchor);
                    let nearest = cells
                        .iter()
                        .map(|&(o, _)| {
                            let l = DVec3::new(
                                f64::from(o[0]),
                                f64::from(o[1]) * CELL_H,
                                f64::from(o[2]),
                            );
                            (rest.0 + rest.1 * l - at_start).length()
                        })
                        .fold(f64::INFINITY, f64::min);
                    assert!(
                        nearest < 1e-6,
                        "cell {c:?} starts {nearest} m from the drawn piece"
                    );
                }
                let mut landed: Vec<[i32; 3]> = part.cells.iter().map(|&(c, _)| c).collect();
                landed.sort_unstable();
                landed
            };
            let a = settled(FaceFrame::A);
            let mut mirrored: Vec<[i32; 3]> = a.iter().map(|&c| f.cell(c)).collect();
            mirrored.sort_unstable();
            assert_eq!(settled(f), mirrored);
        }
    );

    /// The server lifted the piece a row (its place was taken): the glide ends a row higher, from
    /// where it was drawn.
    #[test]
    fn a_lifted_piece_glides_up_to_where_it_was_written() {
        let log = state("dwell:log[axis=y]");
        let cells: Vec<([i32; 3], StateId)> = (0..2).map(|y| ([0, y, 0], log)).collect();
        let rest = (DVec3::new(0.5, 40.25, 0.5), DQuat::IDENTITY);
        let (_, placed) = resample(&cells, rest.0, DQuat::IDENTITY);
        let lifted: Vec<([i32; 3], StateId)> = placed
            .iter()
            .map(|&(c, m)| ([c[0], c[1] + 1, c[2]], m))
            .collect();
        let mut written = written_of(&lifted);
        let piece = SettledPiece {
            cells,
            drawn: rest,
            last: rest,
        };
        let parts = plan(&piece, &mut written);
        assert!(written.is_empty());
        assert_eq!(parts.len(), 1);
        let (p0, _) = pose_at(&parts[0], 0.0);
        let (p1, _) = pose_at(&parts[0], 1.0);
        assert!(
            ((p1.y - p0.y) - CELL_H).abs() < 1e-9,
            "rises a row: {}",
            p1.y - p0.y
        );
    }

    /// Sand that slid off beside a player glides on its own from where it would have gone.
    #[test]
    fn sand_that_slid_off_glides_from_where_it_would_have_landed() {
        let sand = state("dwell:sand");
        let cells: Vec<([i32; 3], StateId)> = (0..2).map(|y| ([0, y, 0], sand)).collect();
        let rest = (DVec3::new(0.5, 40.25, 0.5), DQuat::IDENTITY);
        let (_, placed) = resample(&cells, rest.0, DQuat::IDENTITY);
        // The lower cell went where it fell; the upper one slid to +x (a player stood there).
        let (low, high) = (placed[0], placed[1]);
        let slid = [high.0[0] + 1, high.0[1], high.0[2]];
        let mut written = written_of(&[low, (slid, sand)]);
        let piece = SettledPiece {
            cells,
            drawn: rest,
            last: rest,
        };
        let parts = plan(&piece, &mut written);
        assert!(written.is_empty());
        assert_eq!(parts.len(), 2, "the piece and the slid cell");
        let moved = &parts[1];
        assert_eq!(moved.anchor, slid);
        let (p0, _) = pose_at(moved, 0.0);
        assert!(
            (p0 - cell_centre(high.0)).length() < 1e-9,
            "from its own place"
        );
        assert!((pose_at(moved, 1.0).0 - cell_centre(slid)).length() < 1e-9);
    }

    /// The easing: from 0 to 1, past 1 by the bounce, at rest at the end.
    #[test]
    fn the_glide_eases_out_with_a_bounce_of_the_datas_size() {
        let bounce = 0.12;
        assert_eq!(ease(0.0, bounce), 0.0);
        assert!((ease(1.0, bounce) - 1.0).abs() < 1e-12);
        let peak = (0..=1000)
            .map(|i| ease(f64::from(i) / 1000.0, bounce))
            .fold(0.0, f64::max);
        assert!((peak - 1.0 - bounce).abs() < 1e-3, "peak {peak}");
        // No bounce: never past the end.
        let peak = (0..=1000)
            .map(|i| ease(f64::from(i) / 1000.0, 0.0))
            .fold(0.0, f64::max);
        assert!(peak <= 1.0 + 1e-12);
        const { assert!(FELLING.settle_glide_seconds > 0.0 && FELLING.settle_bounce >= 0.0) };
    }

    /// The lifecycle: the cells are hidden while gliding, shown when it ends, and its meshes go
    /// once the terrain draws them; cells no piece claims show again.
    #[test]
    fn a_glide_hides_its_cells_shows_them_at_the_end_and_hands_off_to_the_terrain() {
        let log = state("dwell:log[axis=y]");
        let cells: Vec<([i32; 3], StateId)> = vec![([0, 0, 0], log)];
        let rest = (DVec3::new(0.5, 40.25, 0.5), DQuat::IDENTITY);
        let (_, placed) = resample(&cells, rest.0, DQuat::IDENTITY);
        let mut g = Glides::default();
        let stray = ([9, 80, 9], log);
        let hide = g.rebaked(&[placed[0], stray], 0.0);
        assert_eq!(hide.len(), 2);
        let parts = g.settled(
            &SettledPiece {
                cells,
                drawn: rest,
                last: rest,
            },
            0.0,
        );
        assert_eq!(parts.len(), 1);
        let id = parts[0].0;
        assert!(id >= SCENE_ID_BASE);
        assert_eq!(g.poses(10.0).len(), 1);
        let end = FELLING.settle_glide_seconds * 1000.0;
        assert_eq!(g.step(end * 0.5, |_| true), Step::default());
        let s = g.step(end, |_| false);
        assert_eq!(s.show, vec![placed[0].0]);
        assert!(
            s.remove.is_empty(),
            "drawn on until the terrain has the cells"
        );
        let s = g.step(end + 16.0, |_| true);
        assert_eq!(s.remove, vec![id]);
        // The stray cell (no piece claimed it) shows again after a while.
        let s = g.step(UNCLAIMED_MS + 1.0, |_| true);
        assert_eq!(s.show, vec![stray.0]);
        assert!(g.poses(UNCLAIMED_MS + 2.0).is_empty());
    }
}
