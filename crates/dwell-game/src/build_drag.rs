//! Drag building (docs/ITEMS.md §5): holding Place after the first block keeps building as the aim
//! moves, in the build mode chosen (data: `shared/items/items.json` `buildModes` → `items.gen.rs`;
//! the kinds below are the engine's fixed vocabulary). A `line` builds on straight out of the face
//! first clicked, as far as the aim reaches along it; a `level` paints the cells the aim sweeps over
//! at the first block's level (floors, ceilings).
//!
//! The first block is the ordinary place (`BlockEditRequest`). The ones after it go out as region
//! fills of one piece each (`RegionEdit` Fill, ARCHITECTURE.md §6.5): a place request is refused
//! against a face the eye can't see (line of sight), which is what a floor reaching away from the
//! player or a line passing the eye needs, and a region edit needs none. Only free cells are filled
//! (air, a replaceable plant, water), so a fill never overwrites a block. Pure: the client's world
//! is read through [`DragWorld`]; everything is in world cells, the same on both faces.

use std::collections::{HashSet, VecDeque};

use dwell_blocks::{Facing, StateId, Support, info};
use dwell_protocol::{RegionEdit, RegionEditOp, SIM_HZ, region_edit};
use dwell_world::region::{box_distance_sq, box_in_world};
use dwell_world::{CELL, pos_of_cell};

pub use crate::items::{BUILD_MODE_KEY, BUILD_MODES};

/// How a build mode builds on (the engine's vocabulary; data names it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    /// Straight out of the face first clicked, as far as the aim reaches along that line.
    Line,
    /// The cells the aim sweeps over, at the first block's level.
    Level,
}

/// A build mode (data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildModeDef {
    /// Namespaced, e.g. `dwell:line`.
    pub name: &'static str,
    /// What the HUD calls it.
    pub label: &'static str,
    pub kind: DragKind,
}

/// What holds Place down: a stroke ends when the one that started it lets go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Mouse,
    Gamepad,
    Touch,
}

/// What drag building reads from the client's world.
pub trait DragWorld {
    /// The cell may be filled: air, a replaceable plant or water, and not in the player's body.
    fn free(&mut self, cell: [i32; 3]) -> bool;
}

/// Region edits sent per server tick at most: half the server's per-tick queue
/// (`region_edit::MAX_QUEUED`), so edits bunched by the network still fit it.
const PER_TICK: f64 = region_edit::MAX_QUEUED as f64 / 2.0;
/// Columns a level's sweep fills between two frames' aims at most (a jump further is not a sweep).
const MAX_SWEEP: i32 = 64;

/// Whether a block is built on by dragging: a plain block. A region fill runs none of the place
/// rules, so a block with a facing (a ladder), other cells (a torch's flame), something to stand on
/// or hang from (`needsSupport`, `placeOn`) is placed one click at a time.
pub fn drags(m: StateId) -> bool {
    let s = info(m);
    s.facing == Facing::None
        && s.extends_up.is_none()
        && s.part_of.is_none()
        && s.place_on.is_empty()
        && s.needs_support == Support::None
}

#[derive(Clone, Debug)]
struct Stroke {
    kind: DragKind,
    holder: Holder,
    material: StateId,
    /// The first piece: its lowest cell and its height in cells.
    base: [i32; 3],
    rows: i32,
    /// Line: the clicked face's direction and the next piece along it (the first is 0).
    dir: [i32; 3],
    next: i32,
    /// Level: the columns filled (or queued), and the one aimed at last.
    filled: HashSet<(i32, i32)>,
    last: (i32, i32),
}

impl Stroke {
    /// The piece whose lowest cell is `min`, as a fill.
    fn fill(&self, min: [i32; 3]) -> RegionEdit {
        RegionEdit {
            op: RegionEditOp::Fill,
            min,
            size: [1, self.rows as u8, 1],
            material: self.material,
            from: 0,
            cells: Vec::new(),
        }
    }

    fn size(&self) -> [i32; 3] {
        [1, self.rows, 1]
    }

    /// The piece at `min` may be filled now: in the world, within `reach` of the eye, every cell free.
    fn fits(&self, min: [i32; 3], eye: [f64; 3], reach: f64, world: &mut impl DragWorld) -> bool {
        box_in_world(min, self.size())
            && box_distance_sq(min, self.size(), eye) <= reach * reach
            && (0..self.rows).all(|r| world.free([min[0], min[1] + r, min[2]]))
    }

    /// The first piece's centre (m).
    fn centre(&self) -> [f64; 3] {
        let c = pos_of_cell(self.base);
        [
            c[0] + 0.5 * CELL[0],
            c[1] + 0.5 * f64::from(self.rows) * CELL[1],
            c[2] + 0.5 * CELL[2],
        ]
    }
}

/// The build mode and the stroke being dragged, with its fills waiting to go out.
#[derive(Clone, Debug)]
pub struct BuildDrag {
    mode: usize,
    stroke: Option<Stroke>,
    pending: VecDeque<RegionEdit>,
    credit: f64,
    credit_ms: f64,
}

impl Default for BuildDrag {
    fn default() -> Self {
        Self {
            mode: 0,
            stroke: None,
            pending: VecDeque::new(),
            credit: PER_TICK,
            credit_ms: f64::NAN,
        }
    }
}

impl BuildDrag {
    pub fn new() -> Self {
        Self::default()
    }

    /// The build mode: the data's first by default.
    pub fn mode(&self) -> &'static BuildModeDef {
        &BUILD_MODES[self.mode]
    }

    /// The next build mode (wrapping around); a stroke being dragged ends.
    pub fn cycle(&mut self) -> &'static BuildModeDef {
        self.mode = (self.mode + 1) % BUILD_MODES.len();
        self.stroke = None;
        self.mode()
    }

    /// A stroke is being dragged.
    pub fn active(&self) -> bool {
        self.stroke.is_some()
    }

    /// The first block was placed (`cells`, as predicted: one column of one piece) against face
    /// direction `dir`, held by `holder`: dragging builds on from it. Nothing for a block that
    /// doesn't drag ([`drags`]).
    pub fn start(&mut self, holder: Holder, cells: &[[i32; 3]], dir: [i32; 3], material: StateId) {
        self.stroke = None;
        let Some(&first) = cells.first() else {
            return;
        };
        let column = cells.iter().all(|c| c[0] == first[0] && c[2] == first[2]);
        let lowest = cells.iter().map(|c| c[1]).min().unwrap_or(first[1]);
        let rows = cells.len() as i32;
        let contiguous = cells.iter().map(|c| c[1]).max() == Some(lowest + rows - 1);
        if !drags(material) || !column || !contiguous {
            return;
        }
        let base = [first[0], lowest, first[2]];
        let kind = self.mode().kind;
        self.stroke = Some(Stroke {
            kind,
            holder,
            material,
            base,
            rows,
            dir,
            next: 1,
            filled: HashSet::from([(base[0], base[2])]),
            last: (base[0], base[2]),
        });
    }

    /// `holder` let go of Place: its stroke ends (the fills already made still go out).
    pub fn release(&mut self, holder: Holder) {
        if self.stroke.as_ref().is_some_and(|s| s.holder == holder) {
            self.stroke = None;
        }
    }

    /// Ends the stroke, whoever holds it (the player died, a menu opened).
    pub fn cancel(&mut self) {
        self.stroke = None;
    }

    /// The aim moved: from `eye` (m) along unit `dir`, building no farther than `reach` (m) from
    /// the eye. The new pieces wait for [`pump`](Self::pump).
    pub fn aim(&mut self, eye: [f64; 3], dir: [f64; 3], reach: f64, world: &mut impl DragWorld) {
        let Some(s) = self.stroke.as_mut() else {
            return;
        };
        let new = match s.kind {
            DragKind::Line => line(s, eye, dir, reach, world),
            DragKind::Level => level(s, eye, dir, reach, world),
        };
        self.pending.extend(new);
    }

    /// The fills the server's region queue takes now (each frame): [`PER_TICK`] a tick.
    pub fn pump(&mut self, now_ms: f64) -> Vec<RegionEdit> {
        if self.credit_ms.is_finite() {
            let ticks = (now_ms - self.credit_ms).max(0.0) * f64::from(SIM_HZ) / 1000.0;
            self.credit = (self.credit + ticks * PER_TICK).min(PER_TICK);
        }
        self.credit_ms = now_ms;
        let mut out = Vec::new();
        while self.credit >= 1.0
            && let Some(e) = self.pending.pop_front()
        {
            self.credit -= 1.0;
            out.push(e);
        }
        out
    }
}

/// A line's new pieces: on along the face's direction up to the point of the line nearest the aim,
/// stopping at the first piece that doesn't fit (a block in the way waits there; so does the
/// player's body, until it moves).
fn line(
    s: &mut Stroke,
    eye: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    world: &mut impl DragWorld,
) -> Vec<RegionEdit> {
    let u = s.dir.map(f64::from);
    let a = s.centre();
    let w: [f64; 3] = std::array::from_fn(|i| a[i] - eye[i]);
    let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    // The nearest points of the line a + σu and the ray eye + τ·dir (both unit).
    let b = dot(u, dir);
    let denom = 1.0 - b * b;
    if denom < 1e-6 {
        return Vec::new(); // looking along the line: no point of it is aimed at
    }
    let (d, e) = (dot(u, w), dot(dir, w));
    let sigma = (b * e - d) / denom;
    let tau = (e - b * d) / denom;
    if tau <= 0.0 {
        return Vec::new();
    }
    // A step is a piece: a cell across, the piece's height up or down.
    let axis = s.dir.iter().position(|&v| v != 0).unwrap_or(0);
    let cells = if axis == 1 { s.rows } else { 1 };
    let step = f64::from(cells) * CELL[axis];
    let want = (sigma / step + 0.5).floor() as i32;
    let mut out = Vec::new();
    while s.next <= want {
        let k = s.next * cells;
        let min: [i32; 3] = std::array::from_fn(|i| s.base[i] + k * s.dir[i]);
        if !s.fits(min, eye, reach, world) {
            break;
        }
        out.push(s.fill(min));
        s.next += 1;
    }
    out
}

/// A level's new pieces: the columns from the last aimed to the one the aim meets the level's
/// plane in now, a 4-connected walk so a fast sweep leaves no gap; each filled once.
fn level(
    s: &mut Stroke,
    eye: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    world: &mut impl DragWorld,
) -> Vec<RegionEdit> {
    let y = s.centre()[1];
    if dir[1].abs() < 1e-6 {
        return Vec::new();
    }
    let t = (y - eye[1]) / dir[1];
    if t <= 0.0 || t > reach + 2.0 {
        return Vec::new(); // the plane is behind the aim or out of reach along it
    }
    let col = (
        (eye[0] + t * dir[0]).div_euclid(CELL[0]) as i32,
        (eye[2] + t * dir[2]).div_euclid(CELL[2]) as i32,
    );
    let (dx, dz) = (col.0 - s.last.0, col.1 - s.last.1);
    let mut walk = Vec::new();
    if dx.abs() + dz.abs() > MAX_SWEEP {
        walk.push(col);
    } else {
        let (mut x, mut z) = s.last;
        let (ax, az) = (dx.abs(), dz.abs());
        let (mut ix, mut iz) = (0, 0);
        while ix < ax || iz < az {
            // Toward whichever axis's next crossing comes first along the segment.
            if f64::from(2 * ix + 1) * f64::from(az) < f64::from(2 * iz + 1) * f64::from(ax) {
                x += dx.signum();
                ix += 1;
            } else {
                z += dz.signum();
                iz += 1;
            }
            walk.push((x, z));
        }
    }
    s.last = col;
    let mut out = Vec::new();
    for (x, z) in walk {
        let min = [x, s.base[1], z];
        if s.filled.contains(&(x, z)) || !s.fits(min, eye, reach, world) {
            continue;
        }
        s.filled.insert((x, z));
        out.push(s.fill(min));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::FaceFrame;

    /// A world of air with some solid cells (world cells).
    #[derive(Default)]
    struct Fake(HashSet<[i32; 3]>);

    impl DragWorld for Fake {
        fn free(&mut self, cell: [i32; 3]) -> bool {
            !self.0.contains(&cell)
        }
    }

    /// The cells the fills cover, face-local.
    fn cells(f: FaceFrame, edits: &[RegionEdit]) -> Vec<[i32; 3]> {
        let mut out: Vec<[i32; 3]> = edits
            .iter()
            .flat_map(|e| {
                (0..i32::from(e.size[1])).map(move |r| [e.min[0], e.min[1] + r, e.min[2]])
            })
            .map(|c| f.cell(c))
            .collect();
        out.sort();
        out
    }

    /// The unit direction from face-local point `from` to `to`, in the world.
    fn towards(f: FaceFrame, from: [f64; 3], to: [f64; 3]) -> [f64; 3] {
        let d: [f64; 3] = std::array::from_fn(|i| to[i] - from[i]);
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        f.vector(d.map(|v| v / n))
    }

    /// A stroke started in `mode` with a 1 m stone (two cells, face-local `lowest` and the one
    /// above) placed against the face whose face-local direction is `dir`.
    fn started(f: FaceFrame, kind: DragKind, lowest: [i32; 3], dir: [i32; 3]) -> BuildDrag {
        let mut d = BuildDrag::new();
        while d.mode().kind != kind {
            d.cycle();
        }
        let piece = [
            f.cell(lowest),
            f.cell([lowest[0], lowest[1] + 1, lowest[2]]),
        ];
        let world_dir = [dir[0], dir[1] * if f.is_b() { -1 } else { 1 }, dir[2]];
        d.start(Holder::Mouse, &piece, world_dir, require("dwell:stone"));
        assert!(d.active());
        d
    }

    fn sent(d: &mut BuildDrag) -> Vec<RegionEdit> {
        let mut out = Vec::new();
        for i in 0..1000 {
            out.extend(d.pump(f64::from(i) * 100.0));
        }
        out
    }

    #[test]
    fn the_modes_are_the_data_and_cycle_from_the_first() {
        let mut d = BuildDrag::new();
        assert_eq!(d.mode().name, BUILD_MODES[0].name);
        assert_eq!(d.mode().kind, DragKind::Line, "the default: a line");
        let names: Vec<_> = (0..BUILD_MODES.len()).map(|_| d.cycle().name).collect();
        let mut want: Vec<_> = BUILD_MODES.iter().skip(1).map(|m| m.name).collect();
        want.push(BUILD_MODES[0].name);
        assert_eq!(names, want);
        assert!(BUILD_MODES.iter().any(|m| m.kind == DragKind::Level));
    }

    dwell_world::both_faces!(
        /// Placed on a top face and held, a line goes up as far as the aim reaches up it, a piece
        /// (1 m: two cells) at a time, and stops under a block in the way.
        fn a_line_builds_straight_out_of_the_face(f: FaceFrame) {
            let mut d = started(f, DragKind::Line, [0, 1, 0], [0, 1, 0]);
            let mut w = Fake::default();
            let eye = [3.0, 1.6, 0.5];
            // The first piece is rows 1–2 (0.5–1.5 m), its centre 1 m up: 3.2 m is 2.2 pieces on.
            let aim = |d: &mut BuildDrag, w: &mut Fake, y: f64| {
                d.aim(f.point(eye), towards(f, eye, [0.5, y, 0.5]), 5.0, w);
            };
            aim(&mut d, &mut w, 3.2);
            assert_eq!(
                cells(f, &sent(&mut d)),
                vec![[0, 3, 0], [0, 4, 0], [0, 5, 0], [0, 6, 0]]
            );
            // Aiming back down builds nothing; a block at rows 9–10 stops the line under it.
            aim(&mut d, &mut w, 1.0);
            assert!(sent(&mut d).is_empty());
            w.0.insert(f.cell([0, 10, 0]));
            aim(&mut d, &mut w, 6.2);
            assert_eq!(cells(f, &sent(&mut d)), vec![[0, 7, 0], [0, 8, 0]]);
            // Let go: nothing more.
            d.release(Holder::Mouse);
            w.0.clear();
            aim(&mut d, &mut w, 6.2);
            assert!(!d.active() && sent(&mut d).is_empty());
        }
    );

    dwell_world::both_faces!(
        /// Placed against a side and held, the line runs out of that face one cell at a time,
        /// toward the player as they back away, as far as the reach.
        fn a_line_out_of_a_side_runs_across(f: FaceFrame) {
            let mut d = started(f, DragKind::Line, [0, 0, 1], [0, 0, 1]);
            let mut w = Fake::default();
            let eye = [0.5, 1.6, 6.5];
            // Aimed at the first piece's side, 4 cells on, 1 m up its middle.
            d.aim(f.point(eye), towards(f, eye, [0.2, 0.5, 5.5]), 5.0, &mut w);
            let got = cells(f, &sent(&mut d));
            let mut want: Vec<[i32; 3]> = (2..=5).flat_map(|z| [[0, 0, z], [0, 1, z]]).collect();
            want.sort();
            assert_eq!(got, want);
        }
    );

    dwell_world::both_faces!(
        /// Flat: the cells the aim sweeps over at the first piece's level are filled, with no gap
        /// between two frames' aims, each once, around a block in the way.
        fn a_level_paints_the_swept_cells(f: FaceFrame) {
            let mut d = started(f, DragKind::Level, [0, 2, 0], [0, 1, 0]);
            let mut w = Fake::default();
            w.0.insert(f.cell([2, 3, 1]));
            let eye = [0.5, 3.6, -2.0];
            // The level's plane is the first piece's middle, 1.5 m up.
            let aim = |d: &mut BuildDrag, w: &mut Fake, x: f64, z: f64| {
                d.aim(f.point(eye), towards(f, eye, [x, 1.5, z]), 8.0, w);
            };
            aim(&mut d, &mut w, 3.5, 2.5);
            let got = sent(&mut d);
            let cols: Vec<(i32, i32)> = got.iter().map(|e| (e.min[0], e.min[2])).collect();
            // (0,0) → (3,2): five steps, each beside a cell built, but the block's (2,1) skipped.
            assert_eq!(cols.len(), 4, "{cols:?}");
            let mut at = (0, 0);
            let mut all = vec![at];
            for c in &cols {
                all.push(*c);
            }
            for c in &cols {
                let near = all
                    .iter()
                    .any(|p| (p.0 - c.0).abs() + (p.1 - c.1).abs() == 1);
                assert!(near, "{c:?} in {cols:?}");
                at = *c;
            }
            assert_eq!(at, (3, 2));
            assert!(
                !cols.contains(&(2, 1)),
                "the block's column is left: {cols:?}"
            );
            for e in &got {
                assert_eq!(cells(f, std::slice::from_ref(e))[0][1], 2, "at the level");
                assert_eq!(e.size, [1, 2, 1]);
            }
            // Back over the same cells: nothing again.
            aim(&mut d, &mut w, 0.5, 0.5);
            let back = sent(&mut d);
            assert!(
                back.iter().all(|e| !cols.contains(&(e.min[0], e.min[2]))),
                "{back:?}"
            );
        }
    );

    #[test]
    fn fills_go_out_at_half_the_servers_region_queue_a_tick() {
        // face-a-only: pacing has no up.
        let mut d = started(FaceFrame::A, DragKind::Line, [0, 1, 0], [0, 1, 0]);
        let mut w = Fake::default();
        let eye = [3.0, 1.6, 0.5];
        d.aim(
            eye,
            towards(FaceFrame::A, eye, [0.5, 30.0, 0.5]),
            64.0,
            &mut w,
        );
        assert_eq!(
            d.pump(0.0).len(),
            PER_TICK as usize,
            "a burst of one tick's"
        );
        assert!(d.pump(1.0).is_empty());
        let tick = 1000.0 / f64::from(SIM_HZ);
        assert_eq!(d.pump(1.0 + tick).len(), PER_TICK as usize);
        assert!(PER_TICK as usize * 2 <= region_edit::MAX_QUEUED);
    }

    #[test]
    fn only_plain_blocks_drag() {
        assert!(drags(require("dwell:stone")));
        let ladder = (0..dwell_blocks::STATES.len() as StateId)
            .find(|&id| info(id).look == "ladder")
            .expect("a ladder");
        for m in [ladder, require("dwell:torch"), require("dwell:tall_grass")] {
            assert!(!drags(m), "{}", info(m).name);
            let mut d = BuildDrag::new();
            d.start(Holder::Mouse, &[[0, 1, 0]], [0, 1, 0], m);
            assert!(!d.active(), "{}", info(m).name);
        }
    }

    #[test]
    fn a_stroke_ends_only_for_the_holder_that_started_it() {
        // face-a-only: who holds the button has no up.
        let mut d = started(FaceFrame::A, DragKind::Line, [0, 1, 0], [0, 1, 0]);
        d.release(Holder::Gamepad);
        assert!(d.active());
        d.release(Holder::Mouse);
        assert!(!d.active());
    }
}
