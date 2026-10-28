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
use dwell_protocol::{BlockEditRequest, RegionEdit, RegionEditOp, SIM_HZ, region_edit};
use dwell_world::region::{box_distance_sq, box_in_world};
use dwell_world::{CELL, pos_of_cell};

pub use crate::items::{BUILD_CLEARANCE, BUILD_MODE_KEY, BUILD_MODES, ItemDrag};

/// How a build mode builds on (the engine's vocabulary; data names it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    /// Straight out of the face first clicked, as far as the aim reaches along that line.
    Line,
    /// The cells the aim sweeps over, at the first block's level.
    Level,
    /// Straight up or down from the first piece, whichever way the aim goes first: an item's own
    /// (`drag: vertical`, ladders), whatever the build mode.
    Vertical,
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
    /// The cell may be filled: air, a replaceable plant or water, and clear of the player's body,
    /// where it is and where the server may still see it ([`lagged_offset`]).
    fn free(&mut self, cell: [i32; 3]) -> bool;
    /// A block is in the cell (not air, a replaceable plant or water): what a floor joins onto and
    /// what the player stands on.
    fn occupied(&mut self, cell: [i32; 3]) -> bool;
    /// `material` would be held in `cell` (a ladder by the wall behind it); a region fill runs no
    /// place rules, so the drag checks it.
    fn holds(&mut self, _cell: [i32; 3], _material: StateId) -> bool {
        true
    }
}

/// Pieces a line keeps built ahead of the player walking along it (bridging).
const LEAD: i32 = 1;

/// Bridging help (owner, 2026-10-28, as Minecraft Bedrock's): Place with nothing targeted builds
/// off the block the player stands on, against its side they face. The block under `feet` (m,
/// world) — under the body's centre, else the nearest under the body (`radius`, m) — and the
/// side's direction, the horizontal look (`yaw_deg`) to the nearest of the four. None with nothing
/// under the body.
pub fn edge_target(
    feet: [f64; 3],
    radius: f64,
    yaw_deg: f64,
    world: &mut impl DragWorld,
) -> Option<([i32; 3], [i32; 3])> {
    let face = dwell_world::FaceFrame::at_height(feet[1]);
    let local = face.point(feet);
    // Stood on, the feet are at a cell's bottom: a little under it is the row below.
    let row = (local[1] / CELL[1] - 0.25).floor() as i32;
    let (cx, cz) = (
        local[0].div_euclid(CELL[0]) as i32,
        local[2].div_euclid(CELL[2]) as i32,
    );
    let gap = |x: i32, z: i32| {
        let g = |p: f64, lo: f64| (lo - p).max(p - (lo + 1.0)).max(0.0);
        g(local[0], f64::from(x)).hypot(g(local[2], f64::from(z)))
    };
    let mut under: Option<([i32; 3], f64)> = None;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (x, z) = (cx + dx, cz + dz);
            let d = gap(x, z);
            let cell = face.cell([x, row, z]);
            if d <= radius && under.is_none_or(|(_, best)| d < best) && world.occupied(cell) {
                under = Some((cell, d));
            }
        }
    }
    let (cell, _) = under?;
    let look = face.look(yaw_deg, 0.0);
    let dir = if look[0].abs() > look[2].abs() {
        [look[0].signum() as i32, 0, 0]
    } else {
        [0, 0, look[2].signum() as i32]
    };
    Some((cell, dir))
}

/// Region edits sent per server tick at most: a quarter of the server's per-tick queue
/// (`region_edit::MAX_QUEUED`), which refuses the rest. Edits bunch on the way: a local world's
/// worker took two frames' fills in one tick and refused a Flat sweep's at two a tick.
const PER_TICK: f64 = region_edit::MAX_QUEUED as f64 / 4.0;
/// Times a refused fill is sent again ([`BuildDrag::retry`]).
pub const RETRIES: u8 = 3;
/// Columns a level's sweep fills between two frames' aims at most (a jump further is not a sweep).
const MAX_SWEEP: i32 = 64;

/// Server ticks an input waits for on the server beyond half the round trip (its input buffer),
/// in [`lagged_offset`].
const INPUT_TICKS: f64 = 2.0;

/// Where the server may still see the player's body, from where it is moving at `velocity` (m/s)
/// with `rtt_ms` to the server: back along the motion by [`BUILD_CLEARANCE`] plus how far it moves
/// while the server's view catches up (half the round trip and its input buffer); at rest, where it
/// is. The server leaves a cell its view of the body is in, so drag building keeps clear of the body
/// here as well as where it is (towering: below the rising body; walking back: ahead of it).
pub fn lagged_offset(velocity: [f64; 3], rtt_ms: Option<f64>) -> [f64; 3] {
    let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
    if speed < 0.1 {
        return [0.0; 3];
    }
    let lag_s = (rtt_ms.unwrap_or(0.0) / 2.0) / 1000.0 + INPUT_TICKS / f64::from(SIM_HZ);
    let back = BUILD_CLEARANCE + speed * lag_s;
    velocity.map(|v| -v / speed * back)
}

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
            && (0..self.rows).all(|r| {
                let c = [min[0], min[1] + r, min[2]];
                world.free(c) && world.holds(c, self.material)
            })
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

/// A first place held down that the player's own body is in the way of (standing where it goes):
/// it is placed, and the stroke starts, once a jump lifts the body clear.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub holder: Holder,
    pub request: BlockEditRequest,
    /// The face's direction, and the block placed.
    pub dir: [i32; 3],
    pub material: StateId,
}

/// The build mode and the stroke being dragged, with its fills waiting to go out.
#[derive(Clone, Debug)]
pub struct BuildDrag {
    mode: usize,
    stroke: Option<Stroke>,
    waiting: Option<Waiting>,
    /// Fills waiting for the rate, each with the times it was refused already.
    pending: VecDeque<(RegionEdit, u8)>,
    credit: f64,
    credit_ms: f64,
}

impl Default for BuildDrag {
    fn default() -> Self {
        Self {
            mode: 0,
            stroke: None,
            waiting: None,
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
        self.waiting = None;
        self.mode()
    }

    /// A stroke is being dragged.
    pub fn active(&self) -> bool {
        self.stroke.is_some()
    }

    /// The first block was placed (`cells`, as predicted: one column of one piece) against face
    /// direction `dir`, held by `holder`: dragging builds on from it, as the item's `drag` says.
    /// Nothing for an item that doesn't drag, or a block that can't in the modes ([`drags`]).
    pub fn start(
        &mut self,
        holder: Holder,
        cells: &[[i32; 3]],
        dir: [i32; 3],
        material: StateId,
        drag: ItemDrag,
    ) {
        self.stroke = None;
        let Some(&first) = cells.first() else {
            return;
        };
        let column = cells.iter().all(|c| c[0] == first[0] && c[2] == first[2]);
        let lowest = cells.iter().map(|c| c[1]).min().unwrap_or(first[1]);
        let rows = cells.len() as i32;
        let contiguous = cells.iter().map(|c| c[1]).max() == Some(lowest + rows - 1);
        if !column || !contiguous {
            return;
        }
        let (kind, dir) = match drag {
            ItemDrag::None => return,
            ItemDrag::Modes if !drags(material) => return,
            ItemDrag::Modes => (self.mode().kind, dir),
            // Up or down: decided by the aim (`vertical`).
            ItemDrag::Vertical => (DragKind::Vertical, [0; 3]),
        };
        let base = [first[0], lowest, first[2]];
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

    /// `holder` let go of Place: its stroke ends (the fills already made still go out), and a
    /// first place still waiting for the body is dropped.
    pub fn release(&mut self, holder: Holder) {
        if self.stroke.as_ref().is_some_and(|s| s.holder == holder) {
            self.stroke = None;
        }
        if self.waiting.as_ref().is_some_and(|w| w.holder == holder) {
            self.waiting = None;
        }
    }

    /// Ends the stroke, whoever holds it (the player died, a menu opened).
    pub fn cancel(&mut self) {
        self.stroke = None;
        self.waiting = None;
    }

    /// Holds a first place the body is in the way of until it can go ([`Waiting`]); only a block
    /// that drags.
    pub fn wait(&mut self, waiting: Waiting) {
        self.stroke = None;
        self.waiting = drags(waiting.material).then_some(waiting);
    }

    /// The first place waiting for the body, if any.
    pub fn waiting(&self) -> Option<&Waiting> {
        self.waiting.as_ref()
    }

    /// Takes the waiting first place (it goes now).
    pub fn take_waiting(&mut self) -> Option<Waiting> {
        self.waiting.take()
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
            DragKind::Vertical => vertical(s, eye, dir, reach, world),
        };
        self.pending.extend(new.into_iter().map(|e| (e, 0)));
    }

    /// A fill the server refused, `tries` times now: it goes again first, up to [`RETRIES`] times.
    pub fn retry(&mut self, fill: RegionEdit, tries: u8) {
        if tries <= RETRIES {
            self.pending.push_front((fill, tries));
        }
    }

    /// The fills the server's region queue takes now (each frame): [`PER_TICK`] a tick.
    pub fn pump(&mut self, now_ms: f64) -> Vec<RegionEdit> {
        self.pump_tries(now_ms)
            .into_iter()
            .map(|(e, _)| e)
            .collect()
    }

    /// [`pump`](Self::pump), with the times each was refused already.
    pub fn pump_tries(&mut self, now_ms: f64) -> Vec<(RegionEdit, u8)> {
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
    // A step is a piece: a cell across, the piece's height up or down.
    let axis = s.dir.iter().position(|&v| v != 0).unwrap_or(0);
    let cells = if axis == 1 { s.rows } else { 1 };
    // Towering: a line up toward the eye with the eye over its column (standing on it) builds on
    // whatever the aim, each piece as soon as the body is clear of it (`DragWorld::free`), so each
    // jump puts one under the feet.
    let corner = pos_of_cell(s.base);
    let over = (eye[0] - corner[0]).clamp(0.0, CELL[0]) == eye[0] - corner[0]
        && (eye[2] - corner[2]).clamp(0.0, CELL[2]) == eye[2] - corner[2];
    if axis == 1 && over && dot(u, w) < 0.0 {
        return build_on(s, i32::MAX, cells, eye, reach, world);
    }
    let step = f64::from(cells) * CELL[axis];
    // Bridging: walking along a line across that the player stands on — the eye 1–3 m over its top,
    // within a cell of it sideways, over a piece already built — keeps it LEAD pieces ahead of the
    // feet, whatever the aim.
    let up = f64::from(dwell_world::FaceFrame::at_row(s.base[1]).sign());
    let above_top = (eye[1] - a[1]) * up - 0.5 * f64::from(s.rows) * CELL[1];
    let side = 2 - axis; // the other horizontal axis (x ↔ z)
    let on = (-dot(u, w) / step + 0.5).floor() as i32;
    let walk = (axis != 1
        && (1.0..=3.0).contains(&above_top)
        && (eye[side] - a[side]).abs() <= CELL[side]
        && on < s.next)
        .then_some(on + LEAD);
    // The nearest points of the line a + σu and the ray eye + τ·dir (both unit).
    let b = dot(u, dir);
    let denom = 1.0 - b * b;
    let (d, e) = (dot(u, w), dot(dir, w));
    let aimed = (denom >= 1e-6) // looking along the line: no point of it is aimed at
        .then(|| ((b * e - d) / denom, (e - b * d) / denom))
        .filter(|&(_, tau)| tau > 0.0)
        .map(|(sigma, _)| (sigma / step + 0.5).floor() as i32);
    match aimed.max(walk) {
        Some(want) => build_on(s, want, cells, eye, reach, world),
        None => Vec::new(),
    }
}

/// A vertical line's new pieces: up or down, whichever way the aim first goes half a piece along
/// the first piece's column (the line nearest the aim), then on as a line.
fn vertical(
    s: &mut Stroke,
    eye: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    world: &mut impl DragWorld,
) -> Vec<RegionEdit> {
    if s.dir == [0; 3] {
        let a = s.centre();
        let w: [f64; 3] = std::array::from_fn(|i| a[i] - eye[i]);
        let b = dir[1];
        let denom = 1.0 - b * b;
        if denom < 1e-6 {
            return Vec::new();
        }
        let e = dir[0] * w[0] + dir[1] * w[1] + dir[2] * w[2];
        let sigma = (b * e - w[1]) / denom;
        if sigma.abs() < 0.5 * f64::from(s.rows) * CELL[1] {
            return Vec::new();
        }
        s.dir = [0, sigma.signum() as i32, 0];
    }
    line(s, eye, dir, reach, world)
}

/// The line's pieces from the next through `want`, `cells` apart, while they fit.
fn build_on(
    s: &mut Stroke,
    want: i32,
    cells: i32,
    eye: [f64; 3],
    reach: f64,
    world: &mut impl DragWorld,
) -> Vec<RegionEdit> {
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
/// plane in now, a 4-connected walk so a fast sweep leaves no gap; each filled once, and only
/// beside a piece already filled or a block at the level.
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
        // Joined (owner, 2026-10-28): beside a piece of the stroke or a block at the level, so a
        // floor never floats apart from what it started from (a cell skipped out of reach or in
        // the body would otherwise leave the next one alone).
        let joined = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(ox, oz)| {
            let n = (x + ox, z + oz);
            s.filled.contains(&n) || (0..s.rows).any(|r| world.occupied([n.0, s.base[1] + r, n.1]))
        });
        if !joined {
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

    /// A world of air with some solid cells, and cells the body is in (world cells).
    #[derive(Default)]
    struct Fake(HashSet<[i32; 3]>, HashSet<[i32; 3]>, HashSet<[i32; 3]>);

    impl DragWorld for Fake {
        fn holds(&mut self, cell: [i32; 3], _material: StateId) -> bool {
            !self.2.contains(&cell)
        }
        fn free(&mut self, cell: [i32; 3]) -> bool {
            !self.0.contains(&cell) && !self.1.contains(&cell)
        }
        fn occupied(&mut self, cell: [i32; 3]) -> bool {
            self.0.contains(&cell)
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
        d.start(
            Holder::Mouse,
            &piece,
            world_dir,
            require("dwell:stone"),
            ItemDrag::Modes,
        );
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
        /// Towering (owner, 2026-10-28): standing on the line's column and looking down, each jump
        /// that lifts the body clear of the next piece builds it under the feet, whatever the aim.
        fn a_line_up_under_the_player_builds_as_each_jump_clears_it(f: FaceFrame) {
            let mut d = started(f, DragKind::Line, [0, 1, 0], [0, 1, 0]);
            // The body (1.8 m) over the column from `feet` (m), as the cells it is in.
            let body = |feet: f64| {
                let mut w = Fake::default();
                let (lo, hi) = (
                    (feet / 0.5).floor() as i32,
                    ((feet + 1.8) / 0.5).ceil() as i32,
                );
                for row in lo..hi {
                    w.0.insert(f.cell([0, row, 0]));
                }
                w
            };
            let down = f.vector([0.0, -1.0, 0.0]);
            let aim = |d: &mut BuildDrag, feet: f64| {
                let eye = f.point([0.5, feet + 1.62, 0.5]);
                d.aim(eye, down, 5.0, &mut body(feet));
                cells(f, &sent(d))
            };
            // Stood on the first piece (its top 1.5 m up): the next is in the body.
            assert!(aim(&mut d, 1.5).is_empty());
            // Half way up a jump: still in it.
            assert!(aim(&mut d, 2.0).is_empty());
            // Clear of it (rows 3–4 end at 2.5 m): built.
            assert_eq!(aim(&mut d, 2.6), vec![[0, 3, 0], [0, 4, 0]]);
            // Landed on it and jumped again: the next.
            assert!(aim(&mut d, 2.5).is_empty());
            assert_eq!(aim(&mut d, 3.6), vec![[0, 5, 0], [0, 6, 0]]);
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

    dwell_world::both_faces!(
        /// Flat: a cell the sweep skipped (the body in it) leaves the next one with nothing to
        /// join: it isn't built, so the floor stays one piece.
        fn a_level_builds_only_beside_what_it_joins(f: FaceFrame) {
            let mut d = started(f, DragKind::Level, [0, 2, 0], [0, 1, 0]);
            let mut w = Fake::default();
            w.1.insert(f.cell([1, 2, 0]));
            let eye = [0.5, 3.6, -2.0];
            d.aim(f.point(eye), towards(f, eye, [2.5, 1.5, 0.5]), 8.0, &mut w);
            assert!(sent(&mut d).is_empty(), "(2,0) would float");
            // Swept again with the body gone: both.
            w.1.clear();
            d.aim(f.point(eye), towards(f, eye, [0.5, 1.5, 0.5]), 8.0, &mut w);
            d.aim(f.point(eye), towards(f, eye, [2.5, 1.5, 0.5]), 8.0, &mut w);
            let cols: Vec<_> = sent(&mut d).iter().map(|e| (e.min[0], e.min[2])).collect();
            assert_eq!(cols, vec![(1, 0), (2, 0)]);
        }
    );

    dwell_world::both_faces!(
        /// Bridging help: the block under the feet (or the nearest under the body off its edge)
        /// and the side faced.
        fn the_edge_target_is_the_block_stood_on_and_the_side_faced(f: FaceFrame) {
            let mut w = Fake::default();
            w.0.insert(f.cell([0, 3, 0]));
            // Stood on it (its top 2 m up), facing +Z, then +X, then −X a little off +X's diagonal.
            let feet = f.point([0.5, 2.0, 0.5]);
            assert_eq!(
                edge_target(feet, 0.3, 0.0, &mut w),
                Some((f.cell([0, 3, 0]), [0, 0, 1]))
            );
            assert_eq!(
                edge_target(feet, 0.3, 90.0, &mut w).map(|t| t.1),
                Some([1, 0, 0])
            );
            assert_eq!(
                edge_target(feet, 0.3, -100.0, &mut w).map(|t| t.1),
                Some([-1, 0, 0])
            );
            // Over its edge with the centre over air: still it, while the body overlaps it.
            let off = f.point([1.2, 2.0, 0.5]);
            assert_eq!(
                edge_target(off, 0.3, 90.0, &mut w).map(|t| t.0),
                Some(f.cell([0, 3, 0]))
            );
            assert_eq!(
                edge_target(f.point([1.4, 2.0, 0.5]), 0.3, 90.0, &mut w),
                None
            );
        }
    );

    dwell_world::both_faces!(
        /// Bridging: walking along a line the player stands on keeps it a piece ahead of the
        /// feet, looking straight along it.
        fn walking_along_a_line_builds_it_ahead(f: FaceFrame) {
            let mut d = started(f, DragKind::Line, [0, 2, 1], [0, 0, 1]);
            let mut w = Fake::default();
            let ahead = f.vector([0.0, 0.0, 1.0]);
            let mut at = |d: &mut BuildDrag, z: f64| {
                d.aim(f.point([0.5, 1.5 + 1.62, z]), ahead, 5.0, &mut w);
                cells(f, &sent(d))
            };
            // On the block behind the first piece: nothing yet.
            assert!(at(&mut d, 0.5).is_empty());
            // On the first piece: the next one ahead; then the next.
            assert_eq!(at(&mut d, 1.5), vec![[0, 2, 2], [0, 3, 2]]);
            assert_eq!(at(&mut d, 2.5), vec![[0, 2, 3], [0, 3, 3]]);
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
    fn a_refused_fill_goes_again_first_up_to_the_retries() {
        // face-a-only: retries have no up.
        let mut d = started(FaceFrame::A, DragKind::Line, [0, 1, 0], [0, 1, 0]);
        let fill = d.stroke.as_ref().expect("a stroke").fill([0, 3, 0]);
        for tries in 1..=RETRIES {
            d.retry(fill.clone(), tries);
            assert_eq!(
                d.pump_tries(f64::from(tries) * 1000.0),
                vec![(fill.clone(), tries)]
            );
        }
        d.retry(fill, RETRIES + 1);
        assert!(d.pump_tries(10_000.0).is_empty(), "given up");
    }

    dwell_world::both_faces!(
        /// Ladders (`drag: vertical`): up or down the wall, whichever way the aim goes, a cell at
        /// a time while the wall holds them, whatever the build mode.
        fn a_ladder_builds_up_or_down_while_the_wall_holds_it(f: FaceFrame) {
            let ladder = (0..dwell_blocks::STATES.len() as StateId)
                .find(|&id| info(id).look == "ladder")
                .expect("a ladder");
            let start = |d: &mut BuildDrag| {
                // Placed against a wall's −Z side at row 4, z = 0.
                d.start(
                    Holder::Mouse,
                    &[f.cell([0, 4, 0])],
                    [0, 0, -1],
                    ladder,
                    ItemDrag::Vertical,
                );
                assert!(d.active());
            };
            let mut w = Fake::default();
            // Row 8 has no wall behind it.
            w.2.insert(f.cell([0, 8, 0]));
            let eye = [0.5, 3.0, -3.0];
            for kind in [DragKind::Line, DragKind::Level] {
                let mut d = BuildDrag::new();
                while d.mode().kind != kind {
                    d.cycle();
                }
                start(&mut d);
                // Up the wall to 4 m (row 8): rows 5–7, held; row 8 isn't.
                d.aim(f.point(eye), towards(f, eye, [0.5, 4.1, 0.5]), 5.0, &mut w);
                assert_eq!(
                    cells(f, &sent(&mut d)),
                    vec![[0, 5, 0], [0, 6, 0], [0, 7, 0]],
                    "{kind:?}"
                );
                // Once up, the aim going down builds nothing below.
                d.aim(f.point(eye), towards(f, eye, [0.5, 0.6, 0.5]), 5.0, &mut w);
                assert!(sent(&mut d).is_empty());
            }
            // Aimed down first: down.
            let mut d = BuildDrag::new();
            start(&mut d);
            d.aim(f.point(eye), towards(f, eye, [0.5, 0.8, 0.5]), 5.0, &mut w);
            assert_eq!(
                cells(f, &sent(&mut d)),
                vec![[0, 1, 0], [0, 2, 0], [0, 3, 0]]
            );
        }
    );

    #[test]
    fn only_plain_blocks_drag() {
        assert!(drags(require("dwell:stone")));
        let ladder = (0..dwell_blocks::STATES.len() as StateId)
            .find(|&id| info(id).look == "ladder")
            .expect("a ladder");
        for m in [ladder, require("dwell:torch"), require("dwell:tall_grass")] {
            assert!(!drags(m), "{}", info(m).name);
            let mut d = BuildDrag::new();
            d.start(Holder::Mouse, &[[0, 1, 0]], [0, 1, 0], m, ItemDrag::Modes);
            assert!(!d.active(), "{}", info(m).name);
        }
    }

    #[test]
    fn a_first_place_waits_for_the_body_until_let_go() {
        // face-a-only: who holds the button has no up.
        let request = BlockEditRequest {
            action: dwell_protocol::BlockEditAction::Place,
            cell: [0, 0, 0],
            face: 2,
            material: require("dwell:stone"),
            rows: 2,
            down: false,
        };
        let waiting = |holder, material| Waiting {
            holder,
            request: BlockEditRequest {
                material,
                ..request.clone()
            },
            dir: [0, 1, 0],
            material,
        };
        let mut d = BuildDrag::new();
        d.wait(waiting(Holder::Touch, require("dwell:stone")));
        assert!(d.waiting().is_some());
        d.release(Holder::Mouse);
        assert!(d.waiting().is_some(), "another holder");
        d.release(Holder::Touch);
        assert!(d.waiting().is_none());
        d.wait(waiting(Holder::Mouse, require("dwell:torch")));
        assert!(d.waiting().is_none(), "only a block that drags waits");
        d.wait(waiting(Holder::Mouse, require("dwell:stone")));
        assert_eq!(d.take_waiting().map(|w| w.holder), Some(Holder::Mouse));
        assert!(d.waiting().is_none());
    }

    #[test]
    fn the_lagged_body_is_back_along_the_motion() {
        // face-a-only: a displacement along the motion, the same on both faces.
        assert_eq!(
            lagged_offset([0.0, 0.05, 0.0], Some(200.0)),
            [0.0; 3],
            "at rest: here"
        );
        let walking = lagged_offset([0.0, 0.0, 5.0], Some(30.0));
        let back = BUILD_CLEARANCE + 5.0 * (0.015 + 2.0 / 60.0);
        assert!(
            (walking[2] + back).abs() < 1e-9 && walking[0] == 0.0,
            "{walking:?}"
        );
        assert!(
            lagged_offset([0.0, 0.0, 5.0], Some(200.0))[2] < walking[2],
            "more lag, further"
        );
        assert!(
            lagged_offset([0.0, 2.0, 0.0], None)[1] < 0.0,
            "rising: below"
        );
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
