//! Water flow (docs/WATER_FLOW.md, ADR 0053), W1: basins with one level joined by outlets, on the
//! server. A **basin** is a connected body of liquid: its container (the cells it fills, row by
//! row from its bottom, each row spread over what holds it up) and its volume, which sets its
//! level. Water leaves by **outlets** (an open cell beside or under it with nothing under that:
//! the weir law, `weir × width × head^1.5`) and **leaks** (a block of `leak` data beside it with an
//! open cell past it: `leak × area × head`), and goes where it falls and runs: another basin, a
//! new puddle where it pools, or nowhere (out of the loaded world). A basin's cells are written
//! only when its level crosses half a row; its outlets are found again only when an edit touches
//! it. Each face's sea and any body larger than `maxBasinCells` is a fixed **reservoir**: a sink,
//! and a source for a cut below its level.
//!
//! A body of water is found when an edit first touches it, **at rest as found** (docs/WATER_FLOW.md
//! §2.2, ADR 0055): the open cells beside it it didn't fill and the leaking blocks it stands against
//! hold it in until an edit touches them, and its natural outlets (a fall, the step onto the next
//! pool) together pass the world map's flow through it (`Runoff`) at the level it was found at, or
//! it seeps that flow away unseen with none. Found bodies a natural outlet joins are linked into one
//! flow network (`Water::balance`), so a river of many pools passes its flow once, and a dam above
//! takes it from those below.
//!
//! Written face-locally (+y up): each basin has its face's frame, and rows are local rows.

use dwell_blocks::{BLOCKS, StateId, info};
use dwell_world::hash::{HashMap, HashSet};
use dwell_world::{CELL_H, FaceFrame, M, VoxelWorld, chunk_of, m_of_row};

pub use dwell_world::flow::{FLOW, FlowData, LiquidFlow};
use dwell_world::flow::{G, discharge, face_flow, weir_of};

/// Rows a basin may stack at most (a well 256 m deep).
const MAX_ROWS: usize = 512;
/// Outlets whose way is found a water tick at most (bodies found on the way included).
const ROUTES_PER_TICK: usize = 1024;
/// Two basins that meet merge when their levels are this close (m).
const MERGE_GAP: f64 = 0.02;
/// Flow steps a water tick takes at most.
const MAX_STEPS: usize = 64;
/// How far over a body (rows) water standing beside the open cells above it is found pouring in
/// (the top of a fall onto it, ADR 0058).
const SOURCE_REACH_ROWS: usize = 32;
/// How far from its ways out (cells) the unloaded world must be beside a body with nothing seen
/// coming in for its river to come in from there (ADR 0058).
const EDGE_SOURCE_AWAY: i64 = 4;

/// How much a block lets through (`leaks`; 0: sealed).
fn leak_of(m: StateId) -> f64 {
    let name = BLOCKS[info(m).block].name;
    FLOW.leaks
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0.0, |&(_, v)| v)
}

/// The liquids' running blocks (`flow.json` `running`), by state: water between basins.
static RUNNING: std::sync::LazyLock<Vec<(StateId, StateId)>> = std::sync::LazyLock::new(|| {
    FLOW.liquids
        .iter()
        .filter_map(|l| {
            Some((
                dwell_blocks::state(l.block)?,
                dwell_blocks::state(l.running)?,
            ))
        })
        .collect()
});

/// Whether `m` is a liquid's running block.
fn is_running(m: StateId) -> bool {
    RUNNING.iter().any(|&(_, r)| r == m)
}

/// The running block of liquid `m`.
fn running_of(m: StateId) -> Option<StateId> {
    RUNNING.iter().find(|&&(l, _)| l == m).map(|&(_, r)| r)
}

/// What a cell is to water.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Holds water back (also an unloaded cell).
    Solid,
    /// Air, or a block water replaces (a plant).
    Open,
    Liquid(StateId),
    /// A block whose open part holds water (a ramp, with a `flooded` state): the water's through
    /// its open sides and top (`joins`), never written over; `flooded` while it holds some.
    Holds {
        flooded: bool,
    },
}

/// The full state of liquid state `m` (itself when full or no liquid): a cell part full is the
/// same liquid (`dwell_blocks::liquid_full`).
fn full_of(m: StateId) -> StateId {
    dwell_blocks::liquid_full(m)
}

/// How many fills liquid `full` is drawn in (its cells from part full to full).
fn fill_steps(full: StateId) -> usize {
    static STEPS: std::sync::LazyLock<HashMap<StateId, usize>> = std::sync::LazyLock::new(|| {
        let mut out: HashMap<StateId, usize> = HashMap::default();
        for (m, s) in dwell_blocks::STATES.iter().enumerate() {
            if s.liquid {
                *out.entry(full_of(m as StateId)).or_default() += 1;
            }
        }
        out
    });
    STEPS.get(&full).copied().unwrap_or(1).max(1)
}

/// Liquid `full`'s state for a cell `frac` full: the nearest of its fills
/// (`dwell_blocks::liquid_at_fill`).
fn fill_state(full: StateId, frac: f64) -> StateId {
    dwell_blocks::liquid_at_fill(full, frac)
}

/// Whether state `m` is a block whose open part holds water: a shaped block with a `flooded`
/// state (a ramp; docs/WATER_FLOW.md §15).
fn holds_water(m: StateId) -> bool {
    static HOLDS: std::sync::LazyLock<Vec<bool>> = std::sync::LazyLock::new(|| {
        (0..dwell_blocks::STATES.len())
            .map(|m| {
                let s = &dwell_blocks::STATES[m];
                s.shape == dwell_blocks::VoxelShape::Shaped
                    && dwell_blocks::with_property(m as StateId, "flooded", "true").is_some()
            })
            .collect()
    });
    HOLDS.get(usize::from(m)).copied().unwrap_or(false)
}

/// Whether water passes between neighbouring cells `a` and `b` (in `view`): a block holding water
/// (a ramp) lets it through only its open faces — a side whose profile stands lower than the cell
/// somewhere, a top not full — never its full back or its bottom (docs/WATER_FLOW.md §15).
fn joins(view: &View<'_>, a: [i32; 3], b: [i32; 3]) -> bool {
    let open = |c: [i32; 3], d: [i32; 3]| -> bool {
        let Some(m) = view.get(c) else {
            return false;
        };
        if !holds_water(m) {
            return true;
        }
        let shape = dwell_blocks::shape::shape_of(m);
        match d {
            [1, 0, 0] => shape.sides[0].iter().any(|&h| h < 4),
            [-1, 0, 0] => shape.sides[1].iter().any(|&h| h < 4),
            [0, 0, 1] => shape.sides[2].iter().any(|&h| h < 4),
            [0, 0, -1] => shape.sides[3].iter().any(|&h| h < 4),
            [0, 1, 0] => !shape.full_top,
            [0, -1, 0] => !shape.full_bottom,
            _ => false,
        }
    };
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    open(a, d) && open(b, [-d[0], -d[1], -d[2]])
}

/// Whether a cell of kind `k` holds water up or back as a wall does (a block holding water in its
/// open part too, for what stands on it or beside its full faces).
fn walls(k: Kind) -> bool {
    matches!(k, Kind::Solid | Kind::Holds { .. })
}

fn kind(m: Option<StateId>) -> Kind {
    let Some(m) = m else {
        return Kind::Solid;
    };
    if m == M.air || is_running(m) {
        return Kind::Open;
    }
    let s = info(m);
    if s.liquid {
        Kind::Liquid(full_of(m))
    } else if holds_water(m) {
        Kind::Holds { flooded: s.flooded }
    } else if s.replaceable && !s.solid {
        Kind::Open
    } else {
        Kind::Solid
    }
}

/// The world as water reads it: loaded cells, with an edit's old states over them (to find the
/// flow a body had before the edit).
struct View<'a> {
    world: &'a VoxelWorld,
    old: Option<&'a HashMap<[i32; 3], StateId>>,
}

impl View<'_> {
    fn get(&self, c: [i32; 3]) -> Option<StateId> {
        if let Some(&m) = self.old.and_then(|o| o.get(&c)) {
            return Some(m);
        }
        self.world.find(chunk_of(c[0], c[1], c[2]))?;
        Some(self.world.get_voxel_if_loaded(c))
    }
    fn kind(&self, c: [i32; 3]) -> Kind {
        kind(self.get(c))
    }
}

/// The cell `d` local rows above `c` (below, negative) on face `f`.
fn up(f: FaceFrame, c: [i32; 3], d: i32) -> [i32; 3] {
    [c[0], f.row(f.row(c[1]) + d), c[2]]
}

const SIDES: [[i32; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

fn side(c: [i32; 3], s: [i32; 2]) -> [i32; 3] {
    [c[0] + s[0], c[1], c[2] + s[1]]
}

/// Where an outlet's water goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Not found yet (found a few a tick).
    Unresolved,
    Basin(u32),
    /// Out of the loaded world, or spread too thin: gone.
    Lost,
    /// Into a generated pool of the world map's pool graph nobody has measured (ADR 0059).
    Virtual(dwell_world::Face, PoolKey),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum OutKind {
    /// Through a cell's side face from `lo` to `hi` (local m; `hi` infinite where it is open
    /// above): the opening law (`face_flow`), over a sill `weir × width × head^1.5`.
    Face { lo: f64, hi: f64 },
    /// Down through a cell's floor at `z` (local m), a 1 m² hole: `C × area × √(2 g head)`.
    Floor { z: f64 },
    /// Through a block: `leak × area × head` above the face's centre (local m).
    Leak { k: f64, centre: f64 },
}

/// A way out of a basin: the open cell its water leaves into, how, and where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outlet {
    pub from: [i32; 3],
    kind: OutKind,
    pub target: Target,
    /// Its flow last tick (m³/s, real).
    pub flow: f64,
    /// × the law's flow: 1, or a natural outlet's share of the map's flow (`Basin::natural`).
    scale: f64,
    /// A face it shares with another basin's container (its target): water crosses it by the
    /// opening law between the two levels, never routed or drawn running.
    contact: bool,
    /// How many cells wide it is (1 but for a lip out of sight by the graph, ADR 0059).
    cells: f64,
}

impl Outlet {
    /// Its flow (m³/s) at level `level` (local m), for a liquid of weir coefficient `weir`, into
    /// water standing at `below` (local m; `-∞` for a free fall): drowned, the head is the
    /// difference of the two levels, so two bodies joined by it even out.
    fn rate(&self, level: f64, weir: f64, below: f64) -> f64 {
        if below >= level {
            return 0.0; // the other side is as high: it flows the other way, if at all
        }
        match self.kind {
            OutKind::Face { lo, hi } => {
                self.scale * face_flow(discharge(weir), level, below, lo, hi)
            }
            OutKind::Floor { z } => {
                if level <= z {
                    0.0
                } else {
                    self.scale * discharge(weir) * (2.0 * G * (level - below.max(z))).sqrt()
                }
            }
            OutKind::Leak { k, centre } => {
                self.scale * k * CELL_H * (level - centre.max(below)).max(0.0)
            }
        }
    }
    /// How fast its flow grows with the level (m²/s) near `level`: what limits a flow step.
    fn growth(&self, level: f64, weir: f64, below: f64) -> f64 {
        // A secant over 0.1 m: the √ of a drowned opening grows without bound at no head.
        let base = self.floor().max(below).max(level - 0.1);
        (self.rate(base + 0.1, weir, below) - self.rate(base, weir, below)) / 0.1
    }
    fn floor(&self) -> f64 {
        match self.kind {
            OutKind::Face { lo, .. } => lo,
            OutKind::Floor { z } => z,
            OutKind::Leak { centre, .. } => centre,
        }
    }
}

/// A body of liquid with one level (docs/WATER_FLOW.md §3).
#[derive(Clone, Debug)]
pub struct Basin {
    pub id: u32,
    face: FaceFrame,
    pub liquid: StateId,
    weir: f64,
    /// A fixed level (the sea, a body larger than `maxBasinCells`): never fills or drains.
    pub reservoir: bool,
    /// The container, a row per local row from `bottom` up: the cells the liquid fills.
    rows: Vec<Vec<[i32; 3]>>,
    bottom: i32,
    /// m³ of liquid in it.
    pub volume: f64,
    /// Its inflows (m³/s, real), each where it comes in: W1's stand-in for runoff. Kept by place
    /// so that a basin that comes apart (a flood over a river's steps receding) leaves each part
    /// the water that comes into it.
    inflows: Vec<([i32; 3], f64)>,
    /// Its volume a tick ago (falling: a flood receding).
    last_volume: f64,
    pub outlets: Vec<Outlet>,
    /// The cells drawn as liquid.
    wet: HashSet<[i32; 3]>,
    /// The top row could not spread (too wide): the basin rises no higher, the excess lost.
    capped: bool,
    /// A reservoir's level (local m).
    fixed_level: f64,
    /// Its bounds (world cells), for being near a player.
    lo: [i32; 3],
    hi: [i32; 3],
    /// Flow arriving from other basins last tick (m³/s).
    pub arriving: f64,
    /// What arrived in the last flow step (m³/s): a waiting basin passes it on.
    through: f64,
    /// What arrived last tick by ways that aren't links (m³/s): a spill over a dam, a cut. A body
    /// found at rest passes it on with its steady flow.
    extra: f64,
    /// Water time (s) it waited out of reach or unloaded: brought forward when it is live again
    /// (`Water::catch_up`, docs/WATER_FLOW.md §6).
    pub owed: f64,
    /// Filling or draining (its level moved faster than `settleMmPerHour` last tick, or it owes
    /// time): it flows with no player near, its chunks kept loaded (`Water::wanted_chunks`).
    pub active: bool,
    /// The surface row and fill its cells were last written at (`top_steps`).
    drawn_fill: (usize, usize),
    /// Its natural outlets (the ways out it was found with: over a step onto the next pool, or a
    /// fall) and each one's scale: together they pass the map's flow at the level it was found at.
    natural: HashMap<[i32; 3], f64>,
    /// The chunks its container lies in: it flows only while all are loaded.
    chunks: Vec<dwell_world::ChunkCoord>,
    /// The chunks beside its cells that weren't loaded when it was found or rebuilt (walls to it
    /// then): when one loads, it is found again there (the water goes on into it).
    edge: Vec<dwell_world::ChunkCoord>,
    /// Found at rest: what it passed then (`Steady`).
    steady: Option<Steady>,
    /// The generated pools of the world map's pool graph it holds (ADR 0059).
    keys: Vec<PoolKey>,
}

/// Inflows by where each comes in (m³/s at a cell).
type Feeds = Vec<([i32; 3], f64)>;

/// What a body found at rest passed then (docs/WATER_FLOW.md §2.2): it passes it at the level it
/// was found at, by its natural outlets, or, with none, unseen (a seep: the water went on in the
/// world as generated where the basins can't follow it, into the ground or out of the loaded world).
#[derive(Clone, Debug)]
struct Steady {
    /// The flow it passes at `level` (m³/s).
    through: f64,
    /// The world map's discharge where it lies (m³/s, as found; a piece of it split off keeps it,
    /// a merge the larger): its natural outlets pass it at `level`, so its level follows what
    /// actually comes in (a river with less coming in drops, one with more rises).
    flow: f64,
    /// Its natural outlets' flow at `level` at scale 1: those that are still ways out of it (a
    /// natural outlet inside it, a merged body's, is none; one an edit blocked still counts, so a
    /// dam holds back what it passed). Found again each tick (`Water::balance`).
    law: f64,
    level: f64,
    /// The world map's flow through it (m³/s), by where it comes in: what it passes, less what
    /// found basins upstream send it (`Water::links`).
    feeds: Vec<([i32; 3], f64)>,
    /// What comes in at its river's head (m³/s): the world map's pool graph's source for the
    /// generated pools it holds (ADR 0059).
    spring: f64,
}

impl Basin {
    /// Whether outlet `o` of it is drawn running (docs/WATER_FLOW.md §15): one an edit made (not a
    /// natural outlet or a seep), when its spill together passes `streamMinFlow` and this one
    /// carries a share of it (at least a sixteenth of that): a spill shared out over a wide edge
    /// runs as a whole.
    fn runs(&self, o: &Outlet) -> bool {
        let made = |x: &Outlet| !self.natural.contains_key(&x.from);
        if !made(o) || o.flow <= 0.0 {
            return false;
        }
        // Spilling (`spillMinFlow`): every way it pours by is drawn running, however thin.
        self.spilling() || o.flow >= FLOW.stream_min_flow
    }

    /// What it pours over edges an edit made (m³/s): into the open (not a face it shares with
    /// another body), not its lips as generated.
    fn spill(&self) -> f64 {
        self.outlets
            .iter()
            .filter(|x| !x.contact && !self.natural.contains_key(&x.from))
            .map(|x| x.flow)
            .sum()
    }

    /// Whether it is drawn spilling (owner, 2026-11-04: "it should flow over at a full voxel
    /// height 0.5m, at least visually"): fed, and pouring `spillMinFlow` over edges an edit made
    /// (overflowing; not a lake draining away through a cut).
    fn spilling(&self) -> bool {
        !self.reservoir
            && self.spill() >= FLOW.spill_min_flow
            && self.inflow() + self.arriving >= FLOW.spill_min_flow
    }

    #[doc(hidden)]
    pub fn debug(&self) -> String {
        format!(
            "id{} lvl{:.3} vol{:.1} in{:.3} arr{:.3} cells{} lo{:?} hi{:?} steady{:?} outs{:?}",
            self.id,
            self.world_level(),
            self.volume,
            self.inflow(),
            self.arriving,
            self.rows.iter().map(Vec::len).sum::<usize>(),
            self.lo,
            self.hi,
            self.steady.as_ref().map(|s| (s.flow, s.law, s.level)),
            self.outlets
                .iter()
                .filter(|o| o.flow > 1e-4)
                .map(|o| (
                    o.from,
                    (o.flow * 1000.0).round() / 1000.0,
                    o.target,
                    self.natural.contains_key(&o.from),
                    o.contact
                ))
                .collect::<Vec<_>>()
        )
    }

    /// Its inflow (m³/s, real).
    /// A loss among them (a lip above it carrying a little more than its own, ADR 0057) takes
    /// only what arrives: with nothing coming down (a dam above filling) the pool keeps its water.
    pub fn inflow(&self) -> f64 {
        let q: f64 = self.inflows.iter().map(|(_, q)| q).sum();
        if q < 0.0 {
            q.max(-self.through.max(0.0))
        } else {
            q
        }
    }

    /// Its surface (m²): the cells of the row its level is in.
    fn surface_area(&self) -> f64 {
        let (k, _) = self.fill();
        self.rows.get(k).map_or(0.0, |r| r.len() as f64).max(1.0)
    }

    fn capacity(&self) -> f64 {
        self.rows.iter().map(|r| r.len() as f64 * CELL_H).sum()
    }

    /// The filled rows: the index of the row the surface is in and how full it is (0–1).
    fn fill(&self) -> (usize, f64) {
        let mut v = self.volume.max(0.0);
        for (i, r) in self.rows.iter().enumerate() {
            let cap = r.len() as f64 * CELL_H;
            if v <= cap || i + 1 == self.rows.len() {
                return (i, if cap > 0.0 { (v / cap).min(1.0) } else { 0.0 });
            }
            v -= cap;
        }
        (0, 0.0)
    }

    /// Its level (local m).
    pub fn level(&self) -> f64 {
        if self.reservoir {
            return self.fixed_level;
        }
        let (i, frac) = self.fill();
        m_of_row(self.bottom + i as i32) + frac * CELL_H
    }

    /// Its level in world metres.
    pub fn world_level(&self) -> f64 {
        self.face.y(self.level())
    }

    /// The fill its surface's row is drawn at, in its liquid's steps (0 none, `fill_steps` full):
    /// the nearest, but the one drawn now while the level is within `fillHysteresis` past half a
    /// step of it, so a level hovering at a step (or a row's floor) doesn't flicker between two
    /// (docs/WATER_FLOW.md §15).
    fn top_steps(&self) -> (usize, usize) {
        let (k, frac) = self.fill();
        let n = fill_steps(self.liquid);
        // Found at rest and still within `foundSnapM` of its level then: drawn at that level (a
        // lip a cell off its river's leaves a pool a few cm off; ADR 0058).
        if let Some(st) = &self.steady
            && !self.reservoir
            && (self.level() - st.level).abs() <= FLOW.found_snap_m
        {
            let steps = ((st.level - m_of_row(self.bottom)) / CELL_H * n as f64).round();
            if steps >= 1.0 {
                let s = steps as usize;
                let kf = (s - 1) / n;
                if kf < self.rows.len() {
                    return (kf, s - kf * n);
                }
            }
        }
        // Spilling over an edit's edge at its surface row's floor (water over a dam's top): that
        // row drawn full, a half block over the edge (the level rounded up, however thin the sheet
        // over it).
        let floor = m_of_row(self.bottom + k as i32);
        if frac > 0.0
            && self.spilling()
            && self.outlets.iter().any(|o| {
                o.flow > 0.0
                    && !o.contact
                    && !self.natural.contains_key(&o.from)
                    && (o.floor() - floor).abs() < 0.01
            })
        {
            return (k, n);
        }
        let nearest = ((frac * n as f64).round() as usize).min(n);
        let (dk, dq) = self.drawn_fill;
        if dk == usize::MAX || dq > n {
            return (k, nearest);
        }
        // In steps from its bottom row's floor.
        let exact = (k * n) as f64 + frac * n as f64;
        let drawn = dk * n + dq;
        if (exact - drawn as f64).abs() <= 0.5 + FLOW.fill_hysteresis
            && (k * n..=k * n + n).contains(&drawn)
        {
            return (k, drawn - k * n);
        }
        (k, nearest)
    }

    /// The cells drawn as liquid at this volume: the full rows, and the surface's row once its
    /// least fill rounds in.
    fn wet_now(&self) -> HashSet<[i32; 3]> {
        let (k, q) = self.top_steps();
        let mut out = HashSet::default();
        for (i, r) in self.rows.iter().enumerate() {
            if i < k || (i == k && q >= 1) {
                out.extend(r.iter().copied());
            }
        }
        out
    }

    /// The liquid it holds below local metres `h` at its present fill.
    fn volume_below(&self, h: f64) -> f64 {
        let mut v = 0.0;
        for (i, r) in self.rows.iter().enumerate() {
            let base = m_of_row(self.bottom + i as i32);
            let depth = (h - base).clamp(0.0, CELL_H);
            v += r.len() as f64 * depth;
        }
        v.min(self.volume)
    }

    fn cells(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.rows.iter().flatten().copied()
    }

    /// A body found at rest passes `q` (m³/s) at the level it was found at: its natural outlets
    /// scaled to it, or its seep.
    fn pass(&mut self, q: f64) {
        let Some(st) = &mut self.steady else {
            return;
        };
        st.through = q;
        let scale = self.natural_scale();
        for o in &mut self.outlets {
            if self.natural.contains_key(&o.from) {
                o.scale = scale * o.cells;
            }
        }
    }

    /// The scale of its natural outlets: together they pass its steady flow at the level it was
    /// found at.
    fn natural_scale(&self) -> f64 {
        match &self.steady {
            Some(st) if st.law > 0.0 => st.through / st.law,
            _ => 0.0,
        }
    }

    /// Whether all of its container's chunks are loaded (an unloaded cell is a wall to water).
    fn loaded(&self, world: &VoxelWorld) -> bool {
        self.chunks.iter().all(|&c| world.find(c).is_some())
    }

    fn near(&self, players: &[[f64; 3]], radius: f64) -> bool {
        players.iter().any(|p| {
            let mut d2 = 0.0;
            for (a, &at) in p.iter().enumerate() {
                let (lo, hi) = if a == 1 {
                    (m_of_row(self.lo[1]), m_of_row(self.hi[1] + 1))
                } else {
                    (f64::from(self.lo[a]), f64::from(self.hi[a] + 1))
                };
                let d = (lo - at).max(at - hi).max(0.0);
                d2 += d * d;
            }
            d2 <= radius * radius
        })
    }
}

/// What finding a row gave: its cells, and the bodies it met at that row.
struct RowScan {
    cells: Vec<[i32; 3]>,
    met: Vec<[i32; 3]>,
    wide: bool,
}

/// The basins (docs/WATER_FLOW.md): found from the liquid cells edits touch, ticked near players.
pub struct Water {
    basins: HashMap<u32, Basin>,
    /// Container cell → its basin (a reservoir's only as far as it was searched).
    owner: HashMap<[i32; 3], u32>,
    next: u32,
    /// Edits since the last water tick: each cell and its state before.
    touched: Vec<([i32; 3], StateId)>,
    /// m³ brought in (inflows, reservoirs' outflow) and lost (to reservoirs, out of the world),
    /// so a test can check the liquid is conserved.
    pub brought: f64,
    pub lost: f64,
    /// The world map's flow through a column (docs/WATER_FLOW.md §2.1): what feeds the bodies.
    runoff: Option<Runoff>,
    /// Liquid that left by an outlet whose way isn't found yet (basin, outlet cell → m³): it
    /// arrives where the way ends once found.
    pending: HashMap<(u32, [i32; 3]), f64>,
    /// Basins beside cells another's level wetted or dried this tick: their ways out are found
    /// again (a pool whose step the pool below flooded spills over it again once it recedes).
    rescan: HashSet<u32>,
    /// Each found way's cells (basin, outlet cell → the open cells its liquid runs through).
    paths: HashMap<(u32, [i32; 3]), Vec<[i32; 3]>>,
    /// The cells drawn running now.
    drawn: HashSet<[i32; 3]>,
    /// Cells edits made solid: a body narrowed by them comes apart there (`split_if_apart`).
    edited: HashSet<[i32; 3]>,
    /// Basins split this tick: not merged again before the next (no split and merge in turn).
    split: HashSet<u32>,
    /// The world map's rivers (docs/WATER_FLOW.md §6, W2d): changes carried down them.
    river_map: Option<std::sync::Arc<dyn Rivers>>,
    /// The generated pools nobody measured that are off their rest (ADR 0059, `virtuals.rs`).
    virtuals: HashMap<(dwell_world::Face, PoolKey), virtuals::Virtual>,
    /// What measured pools poured into virtual ones this tick (m³).
    virtual_in: HashMap<(dwell_world::Face, PoolKey), f64>,
}

/// The flow (m³/s) through a face's column by the world map: (face, face-local x, z).
pub type Runoff = std::sync::Arc<dyn Fn(FaceFrame, i32, i32) -> f64 + Send + Sync>;

/// Columns of a body the world map is asked about at most (spread over it).
const RUNOFF_SAMPLES: usize = 64;

impl Default for Water {
    fn default() -> Self {
        Self::new()
    }
}

impl Water {
    pub fn new() -> Self {
        Self {
            basins: HashMap::default(),
            owner: HashMap::default(),
            next: 1,
            touched: Vec::new(),
            brought: 0.0,
            lost: 0.0,
            runoff: None,
            pending: HashMap::default(),
            rescan: HashSet::default(),
            paths: HashMap::default(),
            drawn: HashSet::default(),
            edited: HashSet::default(),
            split: HashSet::default(),
            river_map: None,
            virtuals: HashMap::default(),
            virtual_in: HashMap::default(),
        }
    }

    /// Water fed by `runoff` (the world map's flow).
    pub fn with_runoff(runoff: Runoff) -> Self {
        Self {
            runoff: Some(runoff),
            ..Self::new()
        }
    }

    /// The generated pools nobody measured that are off their rest now (ADR 0059): what a change
    /// is still carrying down the rivers out of sight.
    pub fn off_rest(&self) -> usize {
        self.virtuals.len()
    }

    /// Water whose changes go down the world map's rivers `rivers` (docs/WATER_FLOW.md §6).
    pub fn with_rivers(mut self, rivers: std::sync::Arc<dyn Rivers>) -> Self {
        self.river_map = Some(rivers);
        self
    }

    /// The map's flow through a body of cells `cells` on face `f`: the most through any of its
    /// columns (a sample spread over them).
    fn runoff_of(&self, f: FaceFrame, cells: &HashSet<[i32; 3]>) -> f64 {
        let Some(runoff) = &self.runoff else {
            return 0.0;
        };
        let mut cols: Vec<[i32; 2]> = cells.iter().map(|c| [c[0], c[2]]).collect();
        cols.sort_unstable();
        cols.dedup();
        let step = cols.len().div_ceil(RUNOFF_SAMPLES).max(1);
        cols.iter()
            .step_by(step)
            .map(|c| runoff(f, c[0], c[1]))
            .fold(0.0, f64::max)
    }

    /// What comes into a body of cells `cells` at level `level` (local m), only where it is seen
    /// to (ADR 0058): the water standing higher beside or over it (or beside the open cells over
    /// it, the top of a fall onto it) as generated, no body's, each such cell by what the opening
    /// law passes over it (a face, its depth over its floor); a found body's comes by that body's
    /// own ways out. With none, where it runs into the unloaded world away from its ways out
    /// (`away`), the river comes in from there: the map's flow, at its cell beyond the edge.
    fn feeds_of(
        &self,
        view: &View<'_>,
        f: FaceFrame,
        cells: &HashSet<[i32; 3]>,
        level: f64,
        weir: f64,
        away: &[[i32; 3]],
    ) -> (Vec<([i32; 3], f64)>, f64) {
        if let Some(found) = self.graph_feeds(f, cells) {
            return found;
        }
        (self.seen_feeds(view, f, cells, level, weir, away), 0.0)
    }

    /// The generated pools of the world map's pool graph a body of cells `cells` holds (ADR 0059).
    fn graph_keys(&self, f: FaceFrame, cells: &HashSet<[i32; 3]>) -> Vec<PoolKey> {
        let Some(rivers) = self.river_map.as_ref() else {
            return Vec::new();
        };
        let mut cols: Vec<[i32; 2]> = cells.iter().map(|c| [c[0], c[2]]).collect();
        cols.sort_unstable();
        cols.dedup();
        let mut keys: Vec<PoolKey> = Vec::new();
        for c in cols {
            if let Some(k @ PoolKey::River { .. }) = rivers.pool(f.face, c[0], c[1])
                && !keys.contains(&k)
            {
                keys.push(k);
            }
        }
        keys.sort_unstable();
        keys
    }

    /// A body's inflow by the world map's pool graph (ADR 0059), if its cells lie in generated
    /// pools: each lip into those pools from a pool it doesn't hold brings what that lip passes at
    /// rest, at the lip's crest (the water above it), and a river's head its source.
    fn graph_feeds(
        &self,
        f: FaceFrame,
        cells: &HashSet<[i32; 3]>,
    ) -> Option<(Feeds, f64)> {
        let rivers = self.river_map.as_ref()?;
        let keys = self.graph_keys(f, cells);
        if keys.is_empty() {
            return None;
        }
        let mut feeds: Vec<([i32; 3], f64)> = Vec::new();
        let mut spring = 0.0;
        for &k in &keys {
            let inn = rivers.inflows(f.face, k);
            spring += inn.source;
            for (from, at, q) in inn.lips {
                if keys.contains(&from) || q <= 0.0 {
                    continue;
                }
                let PoolKey::River { row, .. } = from else {
                    continue;
                };
                feeds.push((f.cell([at.0 as i32, row - 1, at.1 as i32]), q));
            }
        }
        Some((feeds, spring))
    }

    /// The lips of the generated pools a body holds that lie out of sight (a chunk not loaded):
    /// its ways out by the graph there, each at its sill with its cells, into the pool below it
    /// (measured or virtual; ADR 0059).
    fn graph_outlets(&self, world: &VoxelWorld, b: &Basin) -> Vec<Outlet> {
        let Some(rivers) = &self.river_map else {
            return Vec::new();
        };
        let f = b.face;
        let mut out = Vec::new();
        for &key in &b.keys {
            let PoolKey::River { row, .. } = key else {
                continue;
            };
            let Some(lip) = rivers.lip(f.face, key) else {
                continue;
            };
            if b.keys.contains(&lip.to) {
                continue;
            }
            let from = f.cell([lip.at.0 as i32, row - 1, lip.at.1 as i32]);
            // In sight (its crest and the cells it pours into loaded): its faces are found on
            // the blocks.
            let loaded = |c: [i32; 3]| world.find(chunk_of(c[0], c[1], c[2])).is_some();
            if loaded(from) && SIDES.iter().all(|&s| loaded(side(from, s))) {
                continue;
            }
            let lo = m_of_row(row - 1);
            out.push(Outlet {
                from,
                kind: OutKind::Face {
                    lo,
                    hi: lo + CELL_H,
                },
                target: match self.measured(f.face, lip.to) {
                    Some(t) => Target::Basin(t),
                    None => Target::Virtual(f.face, lip.to),
                },
                flow: 0.0,
                scale: lip.pours as f64,
                contact: false,
                cells: lip.pours as f64,
            });
        }
        out
    }

    /// A body's inflow seen from the blocks (no pool graph: a world without a map, a test's).
    fn seen_feeds(
        &self,
        view: &View<'_>,
        f: FaceFrame,
        cells: &HashSet<[i32; 3]>,
        level: f64,
        weir: f64,
        away: &[[i32; 3]],
    ) -> Vec<([i32; 3], f64)> {
        let mut edge: Vec<[i32; 3]> = Vec::new();
        // Each face a source pours over (a cell beside two of the cells it pours into pours over
        // both): the source, the cell it pours into.
        let mut faces: HashSet<([i32; 3], [i32; 3])> = HashSet::default();
        for &c in cells {
            let mut near: Vec<([i32; 3], [i32; 3])> =
                SIDES.iter().map(|&s| (side(c, s), c)).collect();
            for &(n, _) in &near {
                if view.get(n).is_none() && !cells.contains(&n) {
                    edge.push(n);
                }
            }
            let mut o = up(f, c, 1);
            for _ in 0..SOURCE_REACH_ROWS {
                if cells.contains(&o) {
                    break;
                }
                near.push((o, c));
                if view.kind(o) != Kind::Open || view.get(o).is_none() {
                    break;
                }
                near.extend(SIDES.iter().map(|&s| (side(o, s), o)));
                o = up(f, o, 1);
            }
            for (n, into) in near {
                if !cells.contains(&n) {
                    faces.insert((n, into));
                }
            }
        }
        // Each cell poured into takes one face's flow (as a body's ways out are counted: one
        // by the cell each leaves into, `scan_outlets`), shared by the sources beside it.
        let mut into: HashMap<[i32; 3], Vec<([i32; 3], f64)>> = HashMap::default();
        let mut stand: HashMap<[i32; 3], Option<(f64, f64)>> = HashMap::default();
        for &(n, c) in &faces {
            let src = *stand.entry(n).or_insert_with(|| source_of(view, f, n));
            if let Some((top, depth)) = src
                && top > level + 0.01
            {
                into.entry(c)
                    .or_default()
                    .push((n, weir * depth.clamp(0.0, CELL_H).powf(1.5)));
            }
        }
        let mut per: HashMap<[i32; 3], f64> = HashMap::default();
        for (_, mut v) in into {
            v.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let k = v.len() as f64;
            let w = v.iter().map(|x| x.1).fold(0.0, f64::max);
            for (n, _) in v {
                *per.entry(n).or_default() += w / k;
            }
        }
        let mut by: Vec<([i32; 3], f64, bool)> = per
            .into_iter()
            .map(|(n, w)| (n, w, self.owner.contains_key(&n)))
            .collect();
        if by.is_empty() && !edge.is_empty() {
            let d2 = |a: [i32; 3], b: [i32; 3]| {
                (0..3).map(|i| i64::from(a[i] - b[i]).pow(2)).sum::<i64>()
            };
            let near_away = |n: [i32; 3]| away.iter().map(|&a| d2(a, n)).min().unwrap_or(i64::MAX);
            edge.sort_unstable();
            edge.dedup();
            if let Some(&n) = edge
                .iter()
                .max_by(|a, b| near_away(**a).cmp(&near_away(**b)).then(b.cmp(a)))
                && near_away(n) >= EDGE_SOURCE_AWAY * EDGE_SOURCE_AWAY
            {
                let q = self.runoff_of(f, cells);
                if q > 0.0 {
                    return vec![(n, q)];
                }
            }
            return Vec::new();
        }
        by.sort_unstable_by_key(|x| x.0);
        by.into_iter()
            .filter(|x| !x.2 && x.1 > 0.0)
            .map(|x| (x.0, x.1))
            .collect()
    }

    /// Finds again what comes into found body `id` (its container changed: grown into a chunk
    /// that loaded, risen or fallen; ADR 0058).
    fn refeed(&mut self, world: &VoxelWorld, id: u32) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        let Some(st) = &b.steady else {
            return;
        };
        let view = View { world, old: None };
        let cells: HashSet<[i32; 3]> = b.cells().collect();
        let _ = st;
        let away: Vec<[i32; 3]> = b.natural.keys().copied().collect();
        let (feeds, spring) = self.feeds_of(&view, b.face, &cells, b.level(), b.weir, &away);
        let keys = self.graph_keys(b.face, &cells);
        if let Some(b) = self.basins.get_mut(&id) {
            b.keys = keys;
            if let Some(st) = b.steady.as_mut() {
                st.feeds = feeds;
                st.spring = spring;
            }
        }
    }

    fn reservoir_ids(&self) -> HashSet<u32> {
        self.basins
            .values()
            .filter(|b| b.reservoir)
            .map(|b| b.id)
            .collect()
    }

    /// Cells an edit changed (not water's own), with what was there before.
    pub fn touch(&mut self, cells: impl IntoIterator<Item = ([i32; 3], StateId)>) {
        self.touched.extend(cells);
    }

    pub fn basin(&self, id: u32) -> Option<&Basin> {
        self.basins.get(&id)
    }
    /// The basin whose container holds world cell `c`.
    pub fn basin_at(&self, c: [i32; 3]) -> Option<&Basin> {
        self.owner.get(&c).and_then(|id| self.basins.get(id))
    }
    pub fn basins(&self) -> impl Iterator<Item = &Basin> {
        self.basins.values()
    }
    /// The chunks water needs loaded besides the players': those of the basins filling or
    /// draining (and beside them, to grow into), so they flow on with no player near.
    pub fn wanted_chunks(&self) -> Vec<dwell_world::ChunkCoord> {
        let mut out: Vec<dwell_world::ChunkCoord> = self
            .basins
            .values()
            .filter(|b| b.active && !b.reservoir)
            .flat_map(|b| b.chunks.iter().chain(&b.edge).copied())
            .collect();
        out.sort_unstable_by_key(|c| (c.x, c.y, c.z));
        out.dedup();
        out
    }

    /// A diagnostic report (JSON) of the water near cell `at` (within `radius` cells of a basin's
    /// bounds): each basin's level, volume, flows in, whether it is filling or draining, its
    /// steady state as found and its ways out (each one's flow, kind and where it goes); and the
    /// world's totals. For the block dump and tools (docs/DEV_TOOLS.md); not part of the game.
    pub fn report(&self, at: [i32; 3], radius: i32) -> String {
        let num = |v: f64| {
            if v.is_finite() {
                format!("{}", (v * 1000.0).round() / 1000.0)
            } else {
                "null".to_owned()
            }
        };
        let cell = |c: [i32; 3]| format!("[{},{},{}]", c[0], c[1], c[2]);
        let near =
            |b: &Basin| (0..3).all(|a| at[a] >= b.lo[a] - radius && at[a] <= b.hi[a] + radius);
        let mut ids: Vec<u32> = self
            .basins
            .values()
            .filter(|b| !b.reservoir && near(b))
            .map(|b| b.id)
            .collect();
        ids.sort_unstable();
        let mut basins = Vec::new();
        for id in ids {
            let b = &self.basins[&id];
            let steady = b.steady.as_ref().map_or("null".to_owned(), |st| {
                format!(
                    "{{\"flow\":{},\"law\":{},\"level\":{}}}",
                    num(st.flow),
                    num(st.law),
                    num(b.face.y(st.level))
                )
            });
            let outlets: Vec<String> = b
                .outlets
                .iter()
                .filter(|o| o.flow > 1e-4)
                .map(|o| {
                    let kind = match o.kind {
                        OutKind::Face { .. } => "face",
                        OutKind::Floor { .. } => "floor",
                        OutKind::Leak { .. } => "leak",
                    };
                    let to = match o.target {
                        Target::Basin(t) => t.to_string(),
                        Target::Lost => "\"lost\"".to_owned(),
                        Target::Unresolved => "\"unresolved\"".to_owned(),
                        Target::Virtual(_, PoolKey::River { river, row }) => {
                            format!("\"pool {river}:{row}\"")
                        }
                        Target::Virtual(_, PoolKey::Sea) => "\"sea\"".to_owned(),
                    };
                    format!(
                        "{{\"from\":{},\"flow\":{},\"kind\":\"{kind}\",\"to\":{to},\"natural\":{},\"contact\":{}}}",
                        cell(o.from),
                        num(o.flow),
                        b.natural.contains_key(&o.from),
                        o.contact
                    )
                })
                .collect();
            // Where water comes into it, seen (ADR 0058), and how much.
            let feeds: Vec<String> = b
                .steady
                .as_ref()
                .map(|st| {
                    st.feeds
                        .iter()
                        .map(|&(c, q)| format!("{{\"at\":{},\"q\":{}}}", cell(c), num(q)))
                        .collect()
                })
                .unwrap_or_default();
            basins.push(format!(
                "{{\"id\":{},\"level\":{},\"volume\":{},\"inflow\":{},\"arriving\":{},\"active\":{},\"owed\":{},\"cells\":{},\"lo\":{},\"hi\":{},\"steady\":{},\"feeds\":[{}],\"outlets\":[{}]}}",
                b.id,
                num(b.world_level()),
                num(b.volume),
                num(b.inflow()),
                num(b.arriving),
                b.active,
                num(b.owed),
                b.rows.iter().map(Vec::len).sum::<usize>(),
                cell(b.lo),
                cell(b.hi),
                steady,
                feeds.join(","),
                outlets.join(",")
            ));
        }
        format!(
            "{{\"at\":{},\"radius\":{},\"basins\":[{}],\"total\":{},\"brought\":{},\"lost\":{},\"virtuals\":{}}}",
            cell(at),
            radius,
            basins.join(","),
            self.basins.len(),
            num(self.brought),
            num(self.lost),
            self.virtuals.len()
        )
    }

    /// The liquid held in all basins but reservoirs (m³).
    pub fn stored(&self) -> f64 {
        self.basins
            .values()
            .filter(|b| !b.reservoir)
            .map(|b| b.volume)
            .sum::<f64>()
            + self.pending.values().sum::<f64>()
    }

    /// One water tick of `dt` seconds of water time (game time × the water speed): edits since the
    /// last, ways found, flows, levels; returns the cells it wrote (already in `world`).
    pub fn tick(
        &mut self,
        world: &mut VoxelWorld,
        players: &[[f64; 3]],
        dt: f64,
    ) -> Vec<([i32; 3], StateId)> {
        let mut writes = Vec::new();
        self.split.clear();
        // Bodies against a chunk that has loaded since: found again there.
        let mut grown: Vec<u32> = self
            .basins
            .values()
            .filter(|b| b.edge.iter().any(|&c| world.find(c).is_some()))
            .map(|b| b.id)
            .collect();
        grown.sort_unstable();
        for id in grown {
            let before: HashSet<[i32; 3]> =
                self.basins[&id].outlets.iter().map(|o| o.from).collect();
            self.rebuild(world, id, &mut writes);
            self.natural_grown(id, &before);
            self.refeed(world, id);
        }
        self.apply_touched(world, &mut writes);
        self.route(world, &mut writes);
        self.unfeed(world);
        self.balance();
        let live: HashSet<u32> = self
            .basins
            .values()
            .filter(|b| (b.active || b.near(players, FLOW.live_radius_m)) && b.loaded(world))
            .map(|b| b.id)
            .collect();
        let start: HashMap<u32, f64> = self.basins.values().map(|b| (b.id, b.volume)).collect();
        // In steps short enough that no pool overshoots: a step moves a pool's level no more
        // than half the way to where its outflow would balance (its surface over how fast its
        // outflow grows with its level).
        let levels: HashMap<u32, f64> = self.basins.values().map(|b| (b.id, b.level())).collect();
        let mut fastest = f64::INFINITY;
        for b in self
            .basins
            .values()
            .filter(|b| live.contains(&b.id) && !b.reservoir)
        {
            let level = b.level();
            let grow: f64 = b
                .outlets
                .iter()
                .map(|o| {
                    let below = match o.target {
                        Target::Basin(t) => levels.get(&t).copied().unwrap_or(f64::NEG_INFINITY),
                        _ => f64::NEG_INFINITY,
                    };
                    o.growth(level, b.weir, below)
                })
                .sum();
            if grow > 0.0 {
                fastest = fastest.min(b.surface_area() / grow);
            }
        }
        let steps = if fastest.is_finite() && fastest > 0.0 {
            ((dt / (0.5 * fastest)).ceil() as usize).clamp(1, MAX_STEPS)
        } else {
            1
        };
        for b in self.basins.values_mut() {
            b.arriving = 0.0;
            b.extra = 0.0;
        }
        for b in self.basins.values_mut() {
            if !b.reservoir && !live.contains(&b.id) {
                b.owed += dt;
            }
        }
        let sub = dt / steps as f64;
        for _ in 0..steps {
            self.flow_step(&live, sub, steps, None);
        }
        let mut all: Vec<u32> = self.basins.keys().copied().collect();
        all.sort_unstable();
        for id in all {
            if self.basins.contains_key(&id) {
                self.settle_rows(world, id, &mut writes);
            }
        }
        let mut again: Vec<u32> = std::mem::take(&mut self.rescan).into_iter().collect();
        again.sort_unstable();
        for id in again {
            self.rescan_outlets(world, id);
        }
        // Bodies live again after waiting: brought forward by the time they waited.
        let mut back: Vec<u32> = live
            .iter()
            .copied()
            .filter(|id| self.basins.get(id).is_some_and(|b| b.owed > 0.0))
            .collect();
        back.sort_unstable();
        let mut budget = FLOW.catch_up_steps as usize;
        for id in back {
            budget = self.catch_up(world, id, budget, &live, &mut writes);
        }
        // The generated pools nobody measured that something changed (ADR 0059).
        let arrived = std::mem::take(&mut self.virtual_in);
        for (face, key) in self.step_virtuals(arrived, dt) {
            self.measure_virtual(world, face, key, &mut writes);
        }
        // Still filling or draining: flows on wherever the players are, until settled.
        for b in self.basins.values_mut() {
            if b.reservoir || !live.contains(&b.id) {
                continue;
            }
            let moved = start
                .get(&b.id)
                .map_or(f64::INFINITY, |v| (b.volume - v).abs());
            b.active = b.owed > 0.0 || !settled(moved, b.surface_area(), dt);
        }
        self.draw_running(world, &mut writes);
        writes
    }

    /// Brings a basin that waited forward by the time it waited (docs/WATER_FLOW.md §6): its level
    /// a small integration of in − out(level), in steps as long as it affords (half the time its
    /// outflow takes to balance a change), its container found again as it fills or drains, what
    /// comes in from the bodies around it as it came last tick; up to `budget` steps (the rest
    /// next tick). Once in balance it is caught up. Returns the budget left.
    fn catch_up(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        mut budget: usize,
        live: &HashSet<u32>,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) -> usize {
        while budget > 0 {
            let Some(b) = self.basins.get(&id) else {
                break;
            };
            if b.owed <= 0.0 || b.reservoir {
                break;
            }
            let levels: HashMap<u32, f64> =
                self.basins.values().map(|b| (b.id, b.level())).collect();
            let level = b.level();
            let grow: f64 = b
                .outlets
                .iter()
                .map(|o| {
                    let below = match o.target {
                        Target::Basin(t) => levels.get(&t).copied().unwrap_or(f64::NEG_INFINITY),
                        _ => f64::NEG_INFINITY,
                    };
                    o.growth(level, b.weir, below)
                })
                .sum();
            // As long as it affords: half the time its outflow takes to balance a change, and its
            // level moving no more than a quarter row (in − out at the level now).
            let out: f64 = b
                .outlets
                .iter()
                .map(|o| {
                    let below = match o.target {
                        Target::Basin(t) => levels.get(&t).copied().unwrap_or(f64::NEG_INFINITY),
                        _ => f64::NEG_INFINITY,
                    };
                    o.rate(level, b.weir, below)
                })
                .sum();
            let net = (b.inflow() + b.arriving - out).abs();
            let mut sub = b.owed;
            if grow > 0.0 {
                sub = sub.min(0.5 * b.surface_area() / grow);
            }
            if net > 0.0 {
                sub = sub.min(0.25 * CELL_H * b.surface_area() / net);
            }
            // And no longer than its outflow takes to run what stands over its lowest way out (a
            // step can't send more, so a longer one would hold back the rest).
            let floor = b
                .outlets
                .iter()
                .filter(|o| o.rate(level, b.weir, f64::NEG_INFINITY) > 0.0)
                .map(Outlet::floor)
                .fold(f64::INFINITY, f64::min);
            if out > 0.0 && floor.is_finite() {
                let above = (b.volume - b.volume_below(floor)).max(0.0);
                sub = sub.min((above / out).max(1e-3));
            }
            let before = b.volume;
            self.flow_step(live, sub, 1, Some(id));
            budget -= 1;
            self.settle_rows(world, id, writes);
            self.route(world, writes);
            let Some(b) = self.basins.get_mut(&id) else {
                break;
            };
            b.owed -= sub;
            // In balance: where it would be.
            if b.owed <= 0.0 || settled((b.volume - before).abs(), b.surface_area(), sub) {
                b.owed = 0.0;
            }
        }
        budget
    }

    /// One step of the flows (`dt` s), from the levels at its start; `arriving` gathers a share
    /// of what comes in (one of `steps`).
    fn flow_step(&mut self, live: &HashSet<u32>, dt: f64, steps: usize, only: Option<u32>) {
        let mut arriving: HashMap<u32, f64> = HashMap::default();
        let mut extra: HashMap<u32, f64> = HashMap::default();
        let ids: Vec<u32> = match only {
            Some(id) => vec![id],
            None => self.basins.keys().copied().collect(),
        };
        // The levels outlets run into (the same face: a basin's local metres).
        let levels: HashMap<u32, f64> = self.basins.values().map(|b| (b.id, b.level())).collect();
        let areas: HashMap<u32, f64> = self
            .basins
            .values()
            .map(|b| {
                (
                    b.id,
                    if b.reservoir {
                        f64::INFINITY
                    } else {
                        b.surface_area()
                    },
                )
            })
            .collect();
        for &id in &ids {
            let b = self.basins.get_mut(&id).expect("basin");
            let waiting = !live.contains(&id);
            if waiting && b.reservoir {
                continue;
            }
            // Found at rest, it passes what comes in: its own map flow and what arrived last step
            // (over a dam, from the pools above), by its natural outlets at the level it was found
            // at; those an edit blocked still count, so it rises behind them.
            if let Some(st) = &b.steady {
                // (A seep passes what goes through it: its own and what the bodies above bring.)
                let flow = if st.law > 0.0 {
                    st.flow
                } else {
                    st.through.max(st.flow)
                };
                b.pass(flow);
            }
            let level = b.level();
            let mut out: Vec<f64> = b
                .outlets
                .iter()
                .map(|o| {
                    let below = match o.target {
                        Target::Basin(t) => levels.get(&t).copied().unwrap_or(f64::NEG_INFINITY),
                        _ => f64::NEG_INFINITY,
                    };
                    o.rate(level, b.weir, below)
                })
                .collect();
            if waiting && b.active {
                // Filling or draining but not all loaded: it holds what it has, passing nothing,
                // and is brought forward once loaded (`catch_up`), sending then what it would have.
                out.iter_mut().for_each(|q| *q = 0.0);
            } else if waiting {
                // Settled and out of reach: it holds its level and passes on what comes in, shared
                // as its ways out would share it (with none, it keeps it).
                let pass = b.inflow() + b.through;
                let law: f64 = out.iter().sum();
                let s = if law > 0.0 { pass / law } else { 0.0 };
                out.iter_mut().for_each(|q| *q *= s);
            } else {
                // Into water standing beyond: no more in a step, by all its ways there together,
                // than evens the two out (the smaller surface's share of the difference), so the
                // levels never cross.
                let area = if b.reservoir {
                    f64::INFINITY
                } else {
                    b.surface_area()
                };
                let mut into: HashMap<u32, f64> = HashMap::default();
                for (o, q) in b.outlets.iter().zip(out.iter()) {
                    if let Target::Basin(t) = o.target
                        && levels
                            .get(&t)
                            .is_some_and(|l| l.is_finite() && *l > o.floor())
                    {
                        *into.entry(t).or_default() += *q;
                    }
                }
                for (t, total) in into {
                    let below = levels[&t];
                    let other = areas.get(&t).copied().unwrap_or(area);
                    let share = match (area.is_finite(), other.is_finite()) {
                        (true, true) => area * other / (area + other),
                        (true, false) => area, // into a reservoir: only this one moves
                        (false, _) => other,   // out of one: only the other moves
                    };
                    let even = (level - below).max(0.0) * share / dt;
                    if total > even && total > 0.0 {
                        let k = even / total;
                        for (o, q) in b.outlets.iter().zip(out.iter_mut()) {
                            if o.target == Target::Basin(t) && below > o.floor() {
                                *q *= k;
                            }
                        }
                    }
                }
            }
            if !waiting && !b.reservoir {
                // No more than it holds above the lowest way out.
                let floor = b
                    .outlets
                    .iter()
                    .zip(&out)
                    .filter(|(_, q)| **q > 0.0)
                    .map(|(o, _)| o.floor())
                    .fold(f64::INFINITY, f64::min);
                let avail = (b.volume - b.volume_below(floor)).max(0.0);
                let total: f64 = out.iter().sum::<f64>() * dt;
                if total > avail && total > 0.0 {
                    let s = avail / total;
                    out.iter_mut().for_each(|q| *q *= s);
                }
            }
            for (o, &q) in b.outlets.iter_mut().zip(&out) {
                o.flow = q;
            }
            let total: f64 = out.iter().sum();
            if b.reservoir {
                self.brought += total * dt;
            } else if !(waiting && b.active) {
                // (One filling or draining that waits takes its own inflow when brought forward.)
                b.volume -= total * dt;
                b.volume += b.inflow() * dt;
                self.brought += b.inflow() * dt;
            }
            for o in &b.outlets {
                match o.target {
                    Target::Basin(t) => {
                        *arriving.entry(t).or_default() += o.flow;
                        *extra.entry(t).or_default() += o.flow;
                    }
                    Target::Lost => self.lost += o.flow * dt,
                    Target::Virtual(face, k) => {
                        *self.virtual_in.entry((face, k)).or_default() += o.flow * dt;
                    }
                    // Waits for its way (found next tick).
                    Target::Unresolved => {
                        *self.pending.entry((id, o.from)).or_default() += o.flow * dt;
                    }
                }
            }
        }
        if only.is_some() {
            // Brought forward alone: what it sends the bodies around it is theirs (they hold their
            // levels meanwhile); what they sent it while it waited is already in it.
            for (t, q) in arriving {
                match self.basins.get_mut(&t) {
                    Some(b) if !b.reservoir => b.volume += q * dt,
                    _ => self.lost += q * dt,
                }
            }
            return;
        }
        for b in self.basins.values_mut() {
            b.through = 0.0;
        }
        for (t, q) in extra {
            if let Some(b) = self.basins.get_mut(&t) {
                b.extra += q / steps as f64;
            }
        }
        for (t, q) in arriving {
            match self.basins.get_mut(&t) {
                Some(b) if b.reservoir => self.lost += q * dt,
                Some(b) => {
                    b.volume += q * dt;
                    b.arriving += q / steps as f64;
                    b.through = q;
                }
                None => self.lost += q * dt,
            }
        }
    }

    /// Finds the bodies edits touched (before the edit, for their inflow) and finds again the
    /// containers and outlets of those they changed.
    fn apply_touched(&mut self, world: &mut VoxelWorld, writes: &mut Vec<([i32; 3], StateId)>) {
        if self.touched.is_empty() {
            return;
        }
        let touched = std::mem::take(&mut self.touched);
        for &(c, _) in &touched {
            let solid = world.find(chunk_of(c[0], c[1], c[2])).is_some()
                && walls(kind(Some(world.get_voxel_if_loaded(c))));
            if solid {
                self.edited.insert(c);
            } else {
                self.edited.remove(&c);
            }
        }
        let mut old: HashMap<[i32; 3], StateId> = HashMap::default();
        for &(c, m) in &touched {
            old.entry(c).or_insert(m);
        }
        let mut dirty: HashSet<u32> = HashSet::default();
        let mut near: Vec<[i32; 3]> = Vec::new();
        for &(c, _) in &touched {
            near.push(c);
            for s in SIDES {
                near.push(side(c, s));
            }
            near.push([c[0], c[1] + 1, c[2]]);
            near.push([c[0], c[1] - 1, c[2]]);
        }
        // Outlets an edit is beside or on (their way may have changed).
        let by_outlet: Vec<(u32, [i32; 3])> = self
            .basins
            .values()
            .flat_map(|b| b.outlets.iter().map(move |o| (b.id, o.from)))
            .collect();
        for &(id, from) in &by_outlet {
            if touched.iter().any(|&(c, _)| {
                (c[0] - from[0]).abs() <= 1
                    && (c[1] - from[1]).abs() <= 2
                    && (c[2] - from[2]).abs() <= 1
            }) {
                dirty.insert(id);
            }
        }
        for &c in &near {
            if let Some(&id) = self.owner.get(&c) {
                dirty.insert(id);
                continue;
            }
            // A body nobody has found yet, as it was before the edit (its inflow what left it then);
            // liquid the edit itself put there (none before) brings nothing in.
            let was = View {
                world,
                old: Some(&old),
            };
            let now = View { world, old: None };
            if let Kind::Liquid(m) = was.kind(c)
                && weir_of(m).is_some()
            {
                if let Some(id) = self.discover(world, Some(&old), c, true) {
                    dirty.insert(id);
                }
            } else if let Kind::Liquid(m) = now.kind(c)
                && weir_of(m).is_some()
                && let Some(id) = self.discover(world, None, c, false)
            {
                dirty.insert(id);
            }
        }
        for id in dirty {
            if let Some(b) = self.basins.get_mut(&id) {
                b.active = true;
            }
            self.rebuild(world, id, writes);
        }
    }

    /// A body at its level as found, grown into a chunk that loaded since: the ways out it found
    /// there (`before`: the ones it had) are natural too, the world as found, and its natural
    /// outlets together pass the map's flow again (docs/WATER_FLOW.md §2.2). Otherwise they ran by
    /// the opening law and drained it to the next step's level.
    fn natural_grown(&mut self, id: u32, before: &HashSet<[i32; 3]>) {
        let Some(b) = self.basins.get_mut(&id) else {
            return;
        };
        let Some(st) = &b.steady else {
            return;
        };
        if (b.level() - st.level).abs() > 0.01 {
            return; // risen or fallen since: a way it reaches now is the change's
        }
        let (level, weir) = (st.level, b.weir);
        let new: Vec<([i32; 3], f64)> = b
            .outlets
            .iter()
            .filter(|o| !before.contains(&o.from) && !b.natural.contains_key(&o.from))
            .map(|o| (o.from, o.rate(level, weir, f64::NEG_INFINITY)))
            .collect();
        if new.is_empty() {
            return;
        }
        let through = st.through;
        let added: f64 = new.iter().map(|(_, r)| r).sum();
        for (c, w) in new {
            *b.natural.entry(c).or_default() += w;
        }
        let first = b.rows.iter().flatten().next().copied();
        if let Some(st) = &mut b.steady {
            st.law += added;
            st.flow = st.law;
            // What its new ways out carry comes in with its own (the river it was found on).
            // (What comes in is what is seen pouring in, ADR 0058: its new ways out don't change it.)
            let _ = first;
        }
        b.pass(through + added);
    }

    /// Finds the body of liquid at `seed` (in the world as `old` had it): its cells and volume. A
    /// body found `resting` (as the world had it, not liquid an edit just put there) is at rest
    /// as found (docs/WATER_FLOW.md §2.2): the open cells beside it it didn't fill and the leaking
    /// blocks it stood against hold it in, and its natural outlets together pass the world map's
    /// flow through it (the runoff, §2.1) at the level it was found at, which comes in as its
    /// inflow. A body larger than `maxBasinCells` is a reservoir at its level.
    fn discover(
        &mut self,
        world: &VoxelWorld,
        old: Option<&HashMap<[i32; 3], StateId>>,
        seed: [i32; 3],
        resting: bool,
    ) -> Option<u32> {
        let view = View { world, old };
        let Kind::Liquid(liquid) = view.kind(seed) else {
            return None;
        };
        let weir = weir_of(liquid)?;
        let f = FaceFrame::at_row(seed[1]);
        // Its liquid, or a block holding it (a flooded ramp in the riverbed: the river goes on
        // through it).
        let same = |c: [i32; 3]| {
            let k = view.kind(c);
            k == Kind::Liquid(liquid) || (k == Kind::Holds { flooded: true } && liquid == M.water)
        };
        // The surface: up the seed's column.
        let mut top = seed;
        while same(up(f, top, 1)) && joins(&view, top, up(f, top, 1)) {
            top = up(f, top, 1);
        }
        let surface = f.row(top[1]);
        let mut seen: HashSet<[i32; 3]> = HashSet::default();
        let mut queue = vec![seed];
        seen.insert(seed);
        let mut reservoir_met = None;
        let mut too_big = false;
        while let Some(c) = queue.pop() {
            if reservoir_met.is_some() {
                break; // part of a known reservoir: no need to walk it again
            }
            if seen.len() > FLOW.max_basin_cells as usize {
                too_big = true;
                break;
            }
            let mut next: Vec<[i32; 3]> = Vec::with_capacity(6);
            for s in SIDES {
                let n = side(c, s);
                // Into a column whose liquid reaches the same surface (a river's lower pool, past
                // its step, is a body of its own).
                if same(n) && same([n[0], f.row(surface), n[2]]) && joins(&view, c, n) {
                    next.push(n);
                }
            }
            let below = up(f, c, -1);
            if same(below) && joins(&view, c, below) {
                next.push(below);
            }
            let above = up(f, c, 1);
            if f.row(above[1]) <= surface && same(above) && joins(&view, c, above) {
                next.push(above);
            }
            for n in next {
                if let Some(&id) = self.owner.get(&n) {
                    if self.basins.get(&id).is_some_and(|b| b.reservoir) {
                        reservoir_met = Some(id);
                    }
                    continue;
                }
                if seen.insert(n) {
                    queue.push(n);
                }
            }
        }
        if let Some(r) = reservoir_met {
            for c in seen {
                self.owner.insert(c, r);
            }
            return Some(r);
        }
        let id = self.next;
        self.next += 1;
        if too_big {
            let mut lo = seed;
            let mut hi = seed;
            for c in &seen {
                for a in 0..3 {
                    lo[a] = lo[a].min(c[a]);
                    hi[a] = hi[a].max(c[a]);
                }
                self.owner.insert(*c, id);
            }
            self.basins.insert(
                id,
                Basin {
                    id,
                    face: f,
                    liquid,
                    weir,
                    reservoir: true,
                    rows: Vec::new(),
                    bottom: surface,
                    volume: 0.0,
                    inflows: Vec::new(),
                    last_volume: 0.0,
                    outlets: Vec::new(),
                    wet: HashSet::default(),
                    capped: false,
                    fixed_level: m_of_row(surface + 1),
                    lo,
                    hi,
                    arriving: 0.0,
                    through: 0.0,
                    extra: 0.0,
                    owed: 0.0,
                    active: false,
                    drawn_fill: (usize::MAX, 0),
                    natural: HashMap::default(),
                    chunks: Vec::new(),
                    edge: Vec::new(),

                    keys: Vec::new(),
                    steady: None,
                },
            );
            return Some(id);
        }
        let bottom = seen.iter().map(|c| f.row(c[1])).min().unwrap_or(surface);
        let mut rows = vec![Vec::new(); (surface - bottom + 1) as usize];
        for &c in &seen {
            rows[(f.row(c[1]) - bottom) as usize].push(c);
        }
        for r in &mut rows {
            r.sort_unstable();
        }
        // A cell part full holds its fill (a surface row written at its level).
        let volume: f64 = seen
            .iter()
            .map(|&c| view.get(c).map_or(1.0, |m| f64::from(info(m).fill)) * CELL_H)
            .sum();
        let mut b = Basin {
            id,
            face: f,
            liquid,
            weir,
            reservoir: false,
            rows,
            bottom,
            volume,
            inflows: Vec::new(),
            last_volume: volume,
            outlets: Vec::new(),
            wet: seen.iter().copied().collect(),
            capped: false,
            fixed_level: 0.0,
            lo: seed,
            hi: seed,
            arriving: 0.0,
            through: 0.0,
            extra: 0.0,
            owed: 0.0,
            active: false,
            drawn_fill: (usize::MAX, 0),
            natural: HashMap::default(),
            chunks: Vec::new(),
            edge: Vec::new(),

            keys: Vec::new(),
            steady: None,
        };
        if resting {
            // Its ways out: those on the blocks, and its lips out of sight by the graph.
            b.keys = self.graph_keys(f, &seen);
            let mut outlets = scan_outlets(&view, &b, &self.owner, &self.reservoir_ids(), id);
            outlets.extend(self.graph_outlets(world, &b));
            let level = b.level();
            // Each way out by what stands beyond it: a face shared with a body found before (the
            // next pool's water beside it) by that body's level, drowned, never as a free fall.
            let levels: HashMap<u32, f64> = outlets
                .iter()
                .filter_map(|o| match o.target {
                    Target::Basin(t) => Some((t, self.basins.get(&t)?.level())),
                    _ => None,
                })
                .collect();
            let beyond = |o: &Outlet| match o.target {
                Target::Basin(t) => levels.get(&t).copied().unwrap_or(f64::NEG_INFINITY),
                _ => f64::NEG_INFINITY,
            };
            let law: f64 = outlets.iter().map(|o| o.rate(level, weir, beyond(o))).sum();
            // What it passes at rest: what its ways out carry at its level by the one law (its
            // lips sized for the river's flow, ADR 0057); with none, the map's flow, seeping
            // where the river leaves the loaded world.
            let q = if law > 0.0 {
                law
            } else {
                self.runoff_of(f, &seen)
            };
            // By the cell they leave from, all its faces together (a corner cell pours two ways).
            b.natural = HashMap::default();
            for o in &outlets {
                *b.natural.entry(o.from).or_default() += o.rate(level, weir, beyond(o));
            }
            let away: Vec<[i32; 3]> = outlets.iter().map(|o| o.from).collect();
            let (feeds, spring) = self.feeds_of(&view, f, &seen, level, weir, &away);
            b.outlets = outlets;
            b.inflows = feeds.clone();
            b.steady = Some(Steady {
                through: 0.0,
                flow: q,
                law,
                level,
                feeds,
                spring,
            });
            b.pass(q);
        }
        b.chunks = chunks_of(b.cells());
        b.edge = edge_of(world, b.cells());
        for c in b.cells() {
            self.owner.insert(c, id);
        }
        self.basins.insert(id, b);
        Some(id)
    }

    /// Finds a basin's container again from its bottom row up (in the world now), as many rows
    /// as its volume fills (finding the bodies it meets), and its outlets; writes what changed.
    fn rebuild(&mut self, world: &mut VoxelWorld, id: u32, writes: &mut Vec<([i32; 3], StateId)>) {
        {
            let Some(b) = self.basins.get(&id) else {
                return;
            };
            if b.reservoir {
                self.reservoir_outlets(world, id);
                return;
            }
            if !b.loaded(world) {
                return; // found again once its chunks are back
            }
            let f = b.face;
            let liquid = b.liquid;
            let old_rows = b.rows.clone();
            let bottom = b.bottom;
            let volume = b.volume;
            let reservoirs: HashSet<u32> = self
                .basins
                .values()
                .filter(|b| b.reservoir)
                .map(|b| b.id)
                .collect();
            let view = View { world, old: None };
            let mut rows: Vec<Vec<[i32; 3]>> = Vec::new();
            let mut met: Vec<[i32; 3]> = Vec::new();
            let mut capacity = 0.0;
            let mut capped = false;
            let mut under: Option<u32> = None;
            let mut prev: HashSet<[i32; 3]> = HashSet::default();
            let mut lr = bottom;
            loop {
                let i = (lr - bottom) as usize;
                let mut seeds: Vec<[i32; 3]> = old_rows.get(i).cloned().unwrap_or_default();
                seeds.extend(prev.iter().map(|&c| up(f, c, 1)));
                let scan = spread_row(
                    &view,
                    f,
                    liquid,
                    lr,
                    &seeds,
                    &prev,
                    &self.owner,
                    &reservoirs,
                    id,
                    i == 0,
                );
                // Another body's water over its top row: the same water (a ramp's open part
                // under a pool), joined to it below.
                if scan.cells.is_empty()
                    && let Some(&t) = prev
                        .iter()
                        .filter_map(|&c| self.owner.get(&up(f, c, 1)))
                        .find(|&&t| t != id && !reservoirs.contains(&t))
                {
                    under = Some(t);
                }
                met.extend(scan.met);
                if scan.cells.is_empty() {
                    capped = true; // a ceiling: it can rise no higher
                    break;
                }
                capacity += scan.cells.len() as f64 * CELL_H;
                prev = scan.cells.iter().copied().collect();
                rows.push(scan.cells);
                if scan.wide {
                    capped = true;
                    break;
                }
                if capacity >= volume - 1e-9 || rows.len() >= MAX_ROWS {
                    break;
                }
                lr += 1;
            }
            // What it met at its rows: bodies found now if no one has (they share faces with it,
            // contacts: `merge_wide` joins the ones that even out with it at once).
            for c in met {
                if !self.owner.contains_key(&c) {
                    self.discover(world, None, c, true);
                }
            }
            let b = self.basins.get_mut(&id).expect("basin");
            for c in b.cells() {
                if self.owner.get(&c) == Some(&id) {
                    self.owner.remove(&c);
                }
            }
            b.rows = rows;
            b.capped = capped;
            let mut lo = [i32::MAX; 3];
            let mut hi = [i32::MIN; 3];
            for c in b.rows.iter().flatten() {
                for a in 0..3 {
                    lo[a] = lo[a].min(c[a]);
                    hi[a] = hi[a].max(c[a]);
                }
            }
            (b.lo, b.hi) = (lo, hi);
            b.chunks = chunks_of(b.cells());
            b.edge = edge_of(world, b.cells());
            let cells: Vec<[i32; 3]> = b.cells().collect();
            for c in cells {
                self.owner.insert(c, id);
            }
            let view = View { world, old: None };
            let b = &self.basins[&id];
            let mut outlets = scan_outlets(&view, b, &self.owner, &self.reservoir_ids(), id);
            outlets.extend(self.graph_outlets(world, b));
            self.basins.get_mut(&id).expect("basin").outlets = outlets;
            self.split_if_apart(world, id, writes);
            // Under another body's water (its top row covered by it): one body with it.
            if let Some(t) = under
                && self.basins.contains_key(&id)
                && self.basins.contains_key(&t)
            {
                self.merge(t, id);
                self.rebuild(world, t, writes);
            }
        }
    }

    /// The ways that run, drawn: each outlet (not a natural one) passing at least `streamMinFlow` has the open cells
    /// its liquid runs through written as its liquid's running block; cells no way runs through
    /// any more go back to air.
    fn draw_running(&mut self, world: &mut VoxelWorld, writes: &mut Vec<([i32; 3], StateId)>) {
        let basins = &self.basins;
        self.paths.retain(|(id, from), _| {
            basins
                .get(id)
                .is_some_and(|b| b.outlets.iter().any(|o| o.from == *from))
        });
        let mut want: HashMap<[i32; 3], StateId> = HashMap::default();
        for b in self.basins.values() {
            let Some(run) = running_of(b.liquid) else {
                continue;
            };
            for o in &b.outlets {
                // A natural outlet's water runs as generated (the river's own steps): drawn are the
                // ways edits made (over a dam, through a cut).
                if !b.runs(o) || o.target == Target::Unresolved {
                    continue;
                }
                if o.contact {
                    // Into another basin's dry rows: the column it pours down to its water.
                    let mut c = o.from;
                    for _ in 0..64 {
                        if world.find(chunk_of(c[0], c[1], c[2])).is_none() {
                            break;
                        }
                        let m = world.get_voxel_if_loaded(c);
                        if m != M.air && !is_running(m) {
                            break;
                        }
                        want.insert(c, run);
                        c = up(b.face, c, -1);
                    }
                    continue;
                }
                if let Some(path) = self.paths.get(&(b.id, o.from)) {
                    // Open cells only (written over air): a basin's dry rows too.
                    for &c in path {
                        want.insert(c, run);
                    }
                }
            }
        }
        let gone: Vec<[i32; 3]> = self
            .drawn
            .iter()
            .filter(|c| !want.contains_key(*c))
            .copied()
            .collect();
        for c in gone {
            self.drawn.remove(&c);
            if world.find(chunk_of(c[0], c[1], c[2])).is_some()
                && is_running(world.get_voxel_if_loaded(c))
            {
                world.write_voxel(c[0], c[1], c[2], M.air);
                writes.push((c, M.air));
            }
        }
        for (c, run) in want {
            if self.drawn.contains(&c) || world.find(chunk_of(c[0], c[1], c[2])).is_none() {
                continue;
            }
            if world.get_voxel_if_loaded(c) == M.air {
                world.write_voxel(c[0], c[1], c[2], run);
                writes.push((c, run));
                self.drawn.insert(c);
            }
        }
    }

    /// A basin's ways out found again in the world now, those still there keeping where they go.
    fn rescan_outlets(&mut self, world: &VoxelWorld, id: u32) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        if b.reservoir || !b.loaded(world) {
            return;
        }
        let view = View { world, old: None };
        let mut outlets = scan_outlets(&view, b, &self.owner, &self.reservoir_ids(), id);
        outlets.extend(self.graph_outlets(world, b));
        for o in &mut outlets {
            if let Some(old) = b
                .outlets
                .iter()
                .find(|p| p.from == o.from && p.kind == o.kind)
            {
                o.target = old.target;
                o.flow = old.flow;
            }
        }
        self.basins.get_mut(&id).expect("basin").outlets = outlets;
    }

    /// A reservoir's outlets: the open cells beside the cells of it an edit found, below its level.
    fn reservoir_outlets(&mut self, world: &VoxelWorld, id: u32) {
        let view = View { world, old: None };
        let b = &self.basins[&id];
        let f = b.face;
        let level = b.fixed_level;
        let mut outlets: Vec<Outlet> = b.outlets.clone();
        let cells: Vec<[i32; 3]> = self
            .owner
            .iter()
            .filter(|&(_, &o)| o == id)
            .map(|(&c, _)| c)
            .collect();
        for c in cells {
            for s in SIDES {
                let n = side(c, s);
                let sill = m_of_row(f.row(n[1]));
                if view.kind(n) == Kind::Open
                    && sill < level
                    && !outlets.iter().any(|o| o.from == n)
                {
                    outlets.push(Outlet {
                        from: n,
                        kind: OutKind::Face {
                            lo: sill,
                            hi: f64::INFINITY,
                        },
                        target: Target::Unresolved,
                        flow: 0.0,
                        contact: false,
                        scale: 1.0,
                        cells: 1.0,
                    });
                }
            }
        }
        outlets.retain(|o| view.kind(o.from) == Kind::Open);
        self.basins.get_mut(&id).expect("basin").outlets = outlets;
    }

    /// `b` joins `a`: its liquid, inflow and container.
    fn merge(&mut self, a: u32, b: u32) {
        let other = self.basins.remove(&b).expect("basin");
        for c in other.cells() {
            self.owner.insert(c, a);
        }
        let me = self.basins.get_mut(&a).expect("basin");
        me.volume += other.volume;
        me.inflows.extend(other.inflows);
        me.natural.extend(other.natural);
        for k in other.keys {
            if !me.keys.contains(&k) {
                me.keys.push(k);
            }
        }
        me.steady = match (me.steady.take(), other.steady) {
            (Some(mut x), Some(o)) => {
                x.through += o.through;
                x.flow = x.flow.max(o.flow);
                x.law += o.law;
                x.level = x.level.min(o.level);
                x.feeds.extend(o.feeds);
                x.spring += o.spring;
                Some(x)
            }
            (x, o) => x.or(o),
        };
        me.wet.extend(other.wet);
        // Its rows into ours, by local row.
        let lo = me.bottom.min(other.bottom);
        let top = (me.bottom + me.rows.len() as i32).max(other.bottom + other.rows.len() as i32);
        let mut rows = vec![Vec::new(); (top - lo) as usize];
        for (i, r) in me.rows.iter().enumerate() {
            rows[(me.bottom + i as i32 - lo) as usize].extend(r.iter().copied());
        }
        for (i, r) in other.rows.iter().enumerate() {
            rows[(other.bottom + i as i32 - lo) as usize].extend(r.iter().copied());
        }
        // Rows neither had at the bottom stay out (its container starts from its lowest cells).
        let empty = rows.iter().take_while(|r| r.is_empty()).count();
        rows.drain(..empty);
        me.bottom = lo + empty as i32;
        me.rows = rows;
        self.retarget(b, a);
        // What it passes at rest now: its natural ways out but those inside it (the lip from one
        // of the two into the other).
        let owner = &self.owner;
        if let Some(m) = self.basins.get_mut(&a) {
            let law: f64 = m
                .natural
                .iter()
                .filter(|(c, _)| owner.get(*c) != Some(&a))
                .map(|(_, w)| w)
                .sum();
            if let Some(st) = &mut m.steady
                && law > 0.0
            {
                (st.law, st.flow) = (law, law);
            }
        }
    }

    fn retarget(&mut self, from: u32, to: u32) {
        for b in self.basins.values_mut() {
            for o in &mut b.outlets {
                if o.target == Target::Basin(from) {
                    o.target = if b.id == to {
                        Target::Unresolved
                    } else {
                        Target::Basin(to)
                    };
                }
            }
        }
    }

    /// Finds where unresolved outlets' water goes (a few a tick): down while open, then along
    /// the floor for a way down within `streamReach`, landing in a body of liquid or pooling as a
    /// new basin.
    fn route(&mut self, world: &mut VoxelWorld, writes: &mut Vec<([i32; 3], StateId)>) {
        // Rounds: the bodies a round finds have outlets of their own to follow.
        let mut budget = ROUTES_PER_TICK;
        while budget > 0 {
            let mut todo: Vec<(u32, usize)> = Vec::new();
            for b in self.basins.values() {
                for (i, o) in b.outlets.iter().enumerate() {
                    if o.target == Target::Unresolved {
                        todo.push((b.id, i));
                    }
                }
            }
            if todo.is_empty() {
                break;
            }
            todo.sort_unstable();
            todo.truncate(budget);
            budget -= todo.len();
            self.route_some(world, writes, todo);
        }
        // What waited for a way whose basin is gone is lost.
        let basins = &self.basins;
        let gone: f64 = self
            .pending
            .iter()
            .filter(|((id, _), _)| !basins.contains_key(id))
            .map(|(_, v)| v)
            .sum();
        self.pending.retain(|(id, _), _| basins.contains_key(id));
        self.lost += gone;
    }

    fn route_some(
        &mut self,
        world: &mut VoxelWorld,
        writes: &mut Vec<([i32; 3], StateId)>,
        todo: Vec<(u32, usize)>,
    ) {
        for (id, i) in todo {
            let Some(b) = self.basins.get(&id) else {
                continue;
            };
            let Some(o) = b.outlets.get(i) else {
                continue;
            };
            if o.target != Target::Unresolved {
                continue;
            }
            let (f, liquid, from) = (b.face, b.liquid, o.from);
            let (end, path) = {
                let view = View { world, old: None };
                let owner = &self.owner;
                follow_path(&view, f, from, &|c| owner.get(&c) == Some(&id))
            };
            // Into another basin's container on the way (dry now: a river below a dam that ran
            // dry while it filled), it lands there and fills it again.
            let mut path = path;
            let into = path
                .iter()
                .position(|c| self.owner.get(c).is_some_and(|&o| o != id));
            let end = match into {
                Some(k) => {
                    let c = path[k];
                    path.truncate(k);
                    End::Pool(c)
                }
                None => end,
            };
            let last_cell = path.last().copied();
            self.paths.insert((id, from), path);
            // Into a generated pool nobody measured: it feeds that pool of the graph (ADR 0059).
            let generated = |w: &Self, c: [i32; 3]| {
                let k = w.generated_pool(f, c)?;
                match w.measured(f.face, k) {
                    Some(t) if t == id => None, // its own: not a way out
                    Some(t) => Some(Target::Basin(t)),
                    None => Some(Target::Virtual(f.face, k)),
                }
            };
            let target = match end {
                End::Lost => last_cell
                    .and_then(|c| generated(self, up(f, c, -1)).or_else(|| generated(self, c)))
                    .unwrap_or(Target::Lost),
                End::Liquid(c) => match self.owner.get(&c) {
                    Some(&o) if o != id => Target::Basin(o),
                    Some(_) => Target::Lost, // unreachable: its own cells are walls to it
                    None => match generated(self, c) {
                        Some(t) => t,
                        None => self
                            .discover(world, None, c, true)
                            .map_or(Target::Lost, Target::Basin),
                    },
                },
                End::Pool(c) => match self.owner.get(&c) {
                    Some(&o) => Target::Basin(o),
                    None => Target::Basin(self.puddle(world, f, liquid, c, writes)),
                },
            };
            if let Some(b) = self.basins.get_mut(&id)
                && let Some(o) = b.outlets.get_mut(i)
            {
                o.target = target;
            }
            // What left by it while its way was being found arrives now.
            if let Some(v) = self.pending.remove(&(id, from)) {
                match target {
                    Target::Basin(t) => match self.basins.get_mut(&t) {
                        Some(tb) if !tb.reservoir => tb.volume += v,
                        _ => self.lost += v,
                    },
                    _ => self.lost += v,
                }
            }
        }
    }

    /// The found bodies' steady flows (docs/WATER_FLOW.md §2.2, ADR 0058): each takes in what
    /// the water standing as generated beside it pours in (its feeds; what a change upstream
    /// sends down the map's river through country nobody has found comes with it, W2d) and
    /// passes what its ways out carry at its level as found; with none, it seeps what comes in.
    fn balance(&mut self) {
        // What a virtual pool above passes now, for the feeds from its lip (ADR 0059).
        let rivers = self.river_map.clone();
        let virtuals = &self.virtuals;
        let now = |face: dwell_world::Face, c: [i32; 3], q: f64| -> f64 {
            let Some(r) = &rivers else {
                return q;
            };
            match r.pool(face, c[0], c[2]) {
                Some(k) => virtuals.get(&(face, k)).map_or(q, |v| v.q_out),
                None => q,
            }
        };
        let mut set: Vec<(u32, Feeds, f64)> = Vec::new();
        for b in self.basins.values() {
            let Some(st) = b.steady.as_ref() else {
                continue;
            };
            let mut inflows: Vec<([i32; 3], f64)> = st
                .feeds
                .iter()
                .map(|&(c, q)| (c, now(b.face.face, c, q.max(0.0))))
                .filter(|(_, q)| *q > 1e-12)
                .collect();
            if st.spring > 0.0
                && let Some(c) = b.rows.iter().flatten().next()
            {
                inflows.push((*c, st.spring));
            }
            let own: f64 = inflows.iter().map(|(_, q)| q).sum();
            set.push((b.id, inflows, own));
        }
        for (id, inflows, own) in set {
            let b = self.basins.get_mut(&id).expect("basin");
            b.inflows = inflows;
            if let Some(st) = b.steady.as_mut() {
                st.through = if st.law > 0.0 { st.law } else { own };
            }
        }
    }

    /// Drops the feeds no longer seen pouring in (ADR 0058): where the water standing as
    /// generated was found since (a body's now: it comes by that body's ways out) or is gone.
    fn unfeed(&mut self, world: &VoxelWorld) {
        let view = View { world, old: None };
        let owner = &self.owner;
        for b in self.basins.values_mut() {
            let f = b.face;
            if let Some(st) = &mut b.steady {
                st.feeds.retain(|(c, _)| {
                    !owner.contains_key(c)
                        && (view.get(*c).is_none() || source_of(&view, f, *c).is_some())
                });
            }
        }
    }

    /// A new, empty basin at the open cell `c` where running liquid pools.
    fn puddle(
        &mut self,
        world: &mut VoxelWorld,
        f: FaceFrame,
        liquid: StateId,
        c: [i32; 3],
        writes: &mut Vec<([i32; 3], StateId)>,
    ) -> u32 {
        let id = self.next;
        self.next += 1;
        self.basins.insert(
            id,
            Basin {
                id,
                face: f,
                liquid,
                weir: weir_of(liquid).unwrap_or(1.7),
                reservoir: false,
                rows: vec![vec![c]],
                bottom: f.row(c[1]),
                volume: 0.0,
                inflows: Vec::new(),
                last_volume: 0.0,
                outlets: Vec::new(),
                wet: HashSet::default(),
                capped: false,
                fixed_level: 0.0,
                lo: c,
                hi: c,
                arriving: 0.0,
                through: 0.0,
                extra: 0.0,
                owed: 0.0,
                active: false,
                drawn_fill: (usize::MAX, 0),
                natural: HashMap::default(),
                chunks: Vec::new(),
                edge: Vec::new(),

                keys: Vec::new(),
                steady: None,
            },
        );
        self.owner.insert(c, id);
        self.rebuild(world, id, writes);
        id
    }

    /// After a tick's flows: rows added as the level rises past the container, rows dropped as it
    /// falls below one, the wet cells written; an empty basin with nothing coming in goes.
    fn settle_rows(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        if b.reservoir || !b.loaded(world) {
            return;
        }
        // Over the brim: one more row (or the excess lost, capped).
        if b.volume > b.capacity() + 1e-9 {
            if b.capped {
                let b = self.basins.get_mut(&id).expect("basin");
                let excess = b.volume - b.capacity();
                b.volume -= excess;
                self.lost += excess;
            } else {
                self.rebuild(world, id, writes);
            }
        } else if b.rows.len() > 1 {
            // Under the top row: it goes.
            let top_cap = b.rows.last().map_or(0.0, |r| r.len() as f64 * CELL_H);
            if b.volume < b.capacity() - top_cap - 1e-9 {
                self.rebuild(world, id, writes);
            }
        }
        self.merge_wide(world, id, writes);
        self.spin_off_shelves(world, id, writes);
        let Some(b) = self.basins.get_mut(&id) else {
            return;
        };
        if b.volume <= 1e-6 && b.inflow() <= 0.0 && b.arriving <= 0.0 {
            b.volume = 0.0;
        }
        b.last_volume = b.volume;
        let mut want = b.wet_now();
        // Its crest: the surface row's cells beside a way out that runs, drawn while it does (a
        // spill over a dam shows over it, however thin).
        let (k, frac) = b.fill();
        if frac > 0.0
            && let Some(row) = b.rows.get(k)
        {
            let running: HashSet<[i32; 3]> = b
                .outlets
                .iter()
                .filter(|o| b.runs(o))
                .map(|o| o.from)
                .collect();
            if !running.is_empty() {
                want.extend(
                    row.iter()
                        .copied()
                        .filter(|&c| SIDES.iter().any(|&s| running.contains(&side(c, s)))),
                );
            }
        }
        let liquid = b.liquid;
        // Its surface's row at its fill, the rows under it full (docs/WATER_FLOW.md §15).
        let (k, q) = b.top_steps();
        let n = fill_steps(liquid);
        let top: HashSet<[i32; 3]> = b
            .rows
            .get(k)
            .map(|r| r.iter().copied().collect())
            .unwrap_or_default();
        let top_state = fill_state(liquid, q.max(1) as f64 / n as f64);
        let state_of = |c: &[i32; 3]| {
            if top.contains(c) && q < n {
                top_state
            } else {
                liquid
            }
        };
        let gone: Vec<[i32; 3]> = b.wet.difference(&want).copied().collect();
        let new: Vec<[i32; 3]> = want.difference(&b.wet).copied().collect();
        // A new fill (or surface row): its drawn cells written again at theirs.
        let refill: Vec<[i32; 3]> = if b.drawn_fill != (k, q) {
            want.intersection(&b.wet).copied().collect()
        } else {
            Vec::new()
        };
        let b_drawn = (k, q);
        for &c in &gone {
            if world.find(chunk_of(c[0], c[1], c[2])).is_some()
                && full_of(world.get_voxel_if_loaded(c)) == liquid
            {
                world.write_voxel(c[0], c[1], c[2], M.air);
                writes.push((c, M.air));
            }
        }
        // A block holding water (a ramp) in its rows is flooded while the water stands in it, and
        // dried when it goes; never written over.
        for (cells, wet) in [(&gone, false), (&new, true)] {
            for &c in cells.iter() {
                if world.find(chunk_of(c[0], c[1], c[2])).is_none() {
                    continue;
                }
                let m = world.get_voxel_if_loaded(c);
                if holds_water(m)
                    && let Some(to) = dwell_blocks::with_property(
                        m,
                        "flooded",
                        if wet { "true" } else { "false" },
                    )
                    && to != m
                {
                    world.write_voxel(c[0], c[1], c[2], to);
                    writes.push((c, to));
                }
            }
        }
        for &c in &new {
            if kind(Some(world.get_voxel_if_loaded(c))) == Kind::Open
                && world.find(chunk_of(c[0], c[1], c[2])).is_some()
            {
                let m = state_of(&c);
                world.write_voxel(c[0], c[1], c[2], m);
                writes.push((c, m));
            }
        }
        for &c in &refill {
            if world.find(chunk_of(c[0], c[1], c[2])).is_none() {
                continue;
            }
            let now = world.get_voxel_if_loaded(c);
            let m = state_of(&c);
            if full_of(now) == liquid && now != m {
                world.write_voxel(c[0], c[1], c[2], m);
                writes.push((c, m));
            }
        }
        if let Some(b) = self.basins.get_mut(&id) {
            b.drawn_fill = b_drawn;
        }
        let Some(b) = self.basins.get_mut(&id) else {
            return;
        };
        // The blocks at its edge with an open part (ramps) hold the liquid in it, or not; the
        // basins beside find their ways out again.
        for c in gone.iter().chain(&new) {
            let mut beside: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(*c, s)).collect();
            beside.push([c[0], c[1] + 1, c[2]]);
            beside.push([c[0], c[1] - 1, c[2]]);
            for (k, n) in beside.into_iter().enumerate() {
                if k < SIDES.len() {
                    flood_edge(world, n, writes);
                }
                if let Some(&o) = self.owner.get(&n)
                    && o != id
                {
                    self.rescan.insert(o);
                }
            }
        }
        b.wet = want;
        let empty = b.volume <= 0.0 && b.inflow() <= 0.0 && b.arriving <= 0.0;
        if empty {
            let b = self.basins.remove(&id).expect("basin");
            for c in b.cells() {
                if self.owner.get(&c) == Some(&id) {
                    self.owner.remove(&c);
                }
            }
            for o in self.basins.values_mut().flat_map(|x| x.outlets.iter_mut()) {
                if o.target == Target::Basin(id) {
                    o.target = Target::Unresolved;
                }
            }
        }
    }

    /// Basins it shares faces with that even out with it at once merge with it: within
    /// `MERGE_GAP` of its level, and the faces between them wide enough that the difference would
    /// be gone within `mergeSeconds` (their conductance against the smaller surface). A wide gap
    /// is one body; a hole in a wall, or a sheet over a crest, stays an opening between two.
    fn merge_wide(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        if b.reservoir || self.split.contains(&id) {
            return;
        }
        let mine = b.level();
        let mut by: HashMap<u32, f64> = HashMap::default();
        for o in b.outlets.iter().filter(|o| o.contact) {
            let Target::Basin(t) = o.target else { continue };
            let Some(tb) = self.basins.get(&t) else {
                continue;
            };
            let theirs = tb.level();
            if (mine - theirs).abs() > MERGE_GAP {
                continue;
            }
            // Its conductance (m²/s): the flow a difference of MERGE_GAP drives, over it.
            let high = mine.max(theirs);
            let q = Outlet { scale: 1.0, ..*o }.rate(high, b.weir, high - MERGE_GAP);
            *by.entry(t).or_default() += q / MERGE_GAP;
        }
        let area = b.surface_area();
        let flow = b.inflow() + b.arriving;
        let mut join: Vec<u32> = by
            .into_iter()
            .filter(|&(t, g)| {
                let Some(tb) = self.basins.get(&t) else {
                    return false;
                };
                if self.split.contains(&t) {
                    return false;
                }
                let a = tb.surface_area().min(area);
                // The same test a basin is kept whole by (`split_if_apart`): fast to even out,
                // and the faces carry what flows through either within MERGE_GAP.
                g > 0.0
                    && a / g < FLOW.merge_seconds
                    && g * MERGE_GAP >= flow.max(tb.inflow() + tb.arriving)
            })
            .map(|(t, _)| t)
            .collect();
        if join.is_empty() {
            return;
        }
        join.sort_unstable();
        for t in join {
            if self.basins.contains_key(&t) {
                self.merge(id, t);
            }
        }
        self.rebuild(world, id, writes);
    }

    /// A receding basin whose surface row is about to dry: the parts of that row on a floor of
    /// their own (a shelf: solid under it, not the basin) that water comes into become basins of
    /// their own, keeping their water and inflow (a flood over a river's steps receding leaves
    /// each step its pool).
    fn spin_off_shelves(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        let (k, frac) = b.fill();
        if b.reservoir || k == 0 || frac >= 0.5 || b.volume >= b.last_volume || b.inflows.is_empty()
        {
            return;
        }
        let f = b.face;
        let row: HashSet<[i32; 3]> = b.rows[k].iter().copied().collect();
        let under: HashSet<[i32; 3]> = b.rows[k - 1].iter().copied().collect();
        let view = View { world, old: None };
        let shelf: HashSet<[i32; 3]> = row
            .iter()
            .copied()
            .filter(|&c| {
                let d = up(f, c, -1);
                !under.contains(&d) && walls(view.kind(d))
            })
            .collect();
        let mut parts: Vec<HashSet<[i32; 3]>> = Vec::new();
        let mut seen: HashSet<[i32; 3]> = HashSet::default();
        for &start in &shelf {
            if !seen.insert(start) {
                continue;
            }
            let mut part = HashSet::default();
            let mut q = vec![start];
            while let Some(c) = q.pop() {
                part.insert(c);
                for s in SIDES {
                    let n = side(c, s);
                    if shelf.contains(&n) && seen.insert(n) {
                        q.push(n);
                    }
                }
            }
            if b.inflows.iter().any(|(c, _)| part.contains(c)) {
                parts.push(part);
            }
        }
        if parts.is_empty() {
            return;
        }
        let mut made = Vec::new();
        for part in parts {
            let main = self.basins.get_mut(&id).expect("basin");
            let moved = part.len() as f64 * CELL_H * frac;
            main.volume -= moved;
            main.last_volume -= moved;
            main.rows[k].retain(|c| !part.contains(c));
            let inflows: Vec<([i32; 3], f64)> = main
                .inflows
                .iter()
                .filter(|(c, _)| part.contains(c))
                .copied()
                .collect();
            main.inflows.retain(|(c, _)| !part.contains(c));
            let wet: HashSet<[i32; 3]> = main.wet.intersection(&part).copied().collect();
            main.wet.retain(|c| !part.contains(c));
            let mut nb = main.clone();
            let pid = self.next;
            self.next += 1;
            nb.id = pid;
            nb.rows = vec![part.iter().copied().collect()];
            nb.bottom = f.row(part.iter().next().expect("cells")[1]);
            nb.volume = moved;
            nb.last_volume = moved;
            nb.inflows = inflows;
            nb.wet = wet;
            nb.outlets = Vec::new();
            // The map's flow coming in on the shelf comes in to it.
            if let Some(st) = &mut nb.steady {
                st.feeds.retain(|(c, _)| part.contains(c));
            }
            if let Some(m) = &mut self.basins.get_mut(&id).expect("basin").steady {
                m.feeds.retain(|(c, _)| !part.contains(c));
            }
            for &c in &part {
                self.owner.insert(c, pid);
            }
            self.basins.insert(pid, nb);
            made.push(pid);
        }
        self.retarget_all(id);
        self.rebuild(world, id, writes);
        for pid in made {
            self.rebuild(world, pid, writes);
        }
    }

    /// Outlets running into `id` find their way again (it came apart: they may land in a part).
    fn retarget_all(&mut self, id: u32) {
        for o in self.basins.values_mut().flat_map(|b| b.outlets.iter_mut()) {
            if o.target == Target::Basin(id) {
                o.target = Target::Unresolved;
            }
        }
    }

    /// A basin whose container fell apart (a row dropped below a ridge in its bed, an edit) is as
    /// many basins as it has parts, each with the liquid over its own cells.
    fn split_if_apart(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let b = &self.basins[&id];
        let cells: HashSet<[i32; 3]> = b.cells().collect();
        if cells.is_empty() {
            return;
        }
        // Joined by its full rows (the surface row once drawn, half full): parts that meet only
        // above them,
        // over a ridge or a wall, are pools of their own, the water over the crest running from
        // one to the other by the weir law (a wall level with a river's water raises it).
        let (k, frac) = b.fill();
        let full_rows = if frac >= 0.5 { k + 1 } else { k };
        let (f, bottom) = (b.face, b.bottom);
        let full: HashSet<[i32; 3]> = cells
            .iter()
            .copied()
            .filter(|c| ((f.row(c[1]) - bottom) as usize) < full_rows)
            .collect();
        let near = |c: [i32; 3]| -> Vec<[i32; 3]> {
            let mut ns: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
            ns.push([c[0], c[1] + 1, c[2]]);
            ns.push([c[0], c[1] - 1, c[2]]);
            ns
        };
        let components = |set: &HashSet<[i32; 3]>| -> Vec<HashSet<[i32; 3]>> {
            let mut starts: Vec<[i32; 3]> = set.iter().copied().collect();
            starts.sort_unstable();
            let mut out: Vec<HashSet<[i32; 3]>> = Vec::new();
            let mut seen: HashSet<[i32; 3]> = HashSet::default();
            for start in starts {
                if !seen.insert(start) {
                    continue;
                }
                let mut part = HashSet::default();
                let mut q = vec![start];
                while let Some(c) = q.pop() {
                    part.insert(c);
                    for n in near(c) {
                        if set.contains(&n) && seen.insert(n) {
                            q.push(n);
                        }
                    }
                }
                out.push(part);
            }
            out
        };
        // Narrowed by what edits built (a wall, a hole left in it): the cells beside a block an
        // edit placed don't join parts; the generated ground never splits a body.
        let edited = &self.edited;
        let core: HashSet<[i32; 3]> = full
            .iter()
            .copied()
            .filter(|&c| !SIDES.iter().any(|&s| edited.contains(&side(c, s))))
            .collect();
        let seeds = if !core.is_empty() {
            &core
        } else if !full.is_empty() {
            &full
        } else {
            &cells
        };
        let mut parts = components(seeds);
        let full = seeds.clone();
        if !full.is_empty() {
            // The rest of the container goes with the part it reaches first.
            let mut of: HashMap<[i32; 3], usize> = HashMap::default();
            let mut frontier: Vec<[i32; 3]> = Vec::new();
            for (i, p) in parts.iter().enumerate() {
                for &c in p {
                    of.insert(c, i);
                    frontier.push(c);
                }
            }
            frontier.sort_unstable();
            while !frontier.is_empty() {
                let mut next = Vec::new();
                for c in frontier {
                    let i = of[&c];
                    for n in near(c) {
                        if cells.contains(&n) && !of.contains_key(&n) {
                            of.insert(n, i);
                            next.push(n);
                        }
                    }
                }
                next.sort_unstable();
                frontier = next;
            }
            for (c, i) in &of {
                parts[*i].insert(*c);
            }
            let rest: HashSet<[i32; 3]> = cells
                .iter()
                .filter(|c| !of.contains_key(*c))
                .copied()
                .collect();
            parts.extend(components(&rest));
        }
        if parts.len() < 2 {
            return;
        }
        // Parts whose faces between them would even them out within `mergeSeconds` are one.
        let level = b.level();
        let c = discharge(b.weir);
        let of: HashMap<[i32; 3], usize> = parts
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.iter().map(move |&c| (c, i)))
            .collect();
        let mut g: HashMap<(usize, usize), f64> = HashMap::default();
        for (&cell, &i) in &of {
            let lo = m_of_row(f.row(cell[1]));
            for s in SIDES {
                if let Some(&j) = of.get(&side(cell, s))
                    && j > i
                {
                    let q = face_flow(c, level, level - MERGE_GAP, lo, lo + CELL_H);
                    *g.entry((i, j)).or_default() += q / MERGE_GAP;
                }
            }
            if let Some(&j) = of.get(&up(f, cell, -1))
                && j != i
                && level > lo
            {
                let q = c * (2.0 * G * MERGE_GAP).sqrt();
                *g.entry((i.min(j), i.max(j))).or_default() += q / MERGE_GAP;
            }
        }
        let (k, _) = b.fill();
        let surf = |p: &HashSet<[i32; 3]>| -> f64 {
            (p.iter()
                .filter(|c| (f.row(c[1]) - bottom) as usize == k)
                .count() as f64)
                .max(1.0)
        };
        let mut root: Vec<usize> = (0..parts.len()).collect();
        fn find(root: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while root[r] != r {
                r = root[r];
            }
            root[i] = r;
            r
        }
        let mut pairs: Vec<((usize, usize), f64)> = g.into_iter().collect();
        pairs.sort_by_key(|&(p, _)| p);
        // And only if the faces carry what flows through the basin (its inflow and what arrives)
        // within MERGE_GAP: a river through a hole needs a head across it.
        let throughput = b.inflow() + b.arriving;
        // A part wholly in the surface row (water over a sill: a lip's crest) is one body with
        // the pool it joins: the river pours off its edge, a head over the sill, not across the
        // faces between them (ADR 0057).
        let crest = |p: &HashSet<[i32; 3]>| p.iter().all(|c| (f.row(c[1]) - bottom) as usize == k);
        for ((i, j), cond) in pairs {
            let a = surf(&parts[i]).min(surf(&parts[j]));
            let carries = cond * MERGE_GAP >= throughput || crest(&parts[i]) || crest(&parts[j]);
            if cond > 0.0 && a / cond < FLOW.merge_seconds && carries {
                let (ri, rj) = (find(&mut root, i), find(&mut root, j));
                if ri != rj {
                    root[rj] = ri;
                }
            }
        }
        let mut joined: HashMap<usize, HashSet<[i32; 3]>> = HashMap::default();
        for (i, p) in parts.into_iter().enumerate() {
            let r = find(&mut root, i);
            joined.entry(r).or_default().extend(p);
        }
        let mut parts: Vec<HashSet<[i32; 3]>> = joined.into_values().collect();
        if parts.len() < 2 {
            return;
        }
        parts.sort_by_key(|p| (std::cmp::Reverse(p.len()), p.iter().min().copied()));
        let template = b.clone();
        let share = |part: &HashSet<[i32; 3]>| -> f64 {
            part.iter()
                .map(|c| {
                    let i = (f.row(c[1]) - bottom) as usize;
                    if i < k {
                        CELL_H
                    } else if i == k {
                        CELL_H * frac
                    } else {
                        0.0
                    }
                })
                .sum()
        };
        let mut made = Vec::new();
        for (n, part) in parts.iter().enumerate() {
            let pid = if n == 0 {
                id
            } else {
                let p = self.next;
                self.next += 1;
                p
            };
            let lo = part.iter().map(|c| f.row(c[1])).min().expect("cells");
            let hi = part.iter().map(|c| f.row(c[1])).max().expect("cells");
            let mut rows = vec![Vec::new(); (hi - lo + 1) as usize];
            for &c in part {
                rows[(f.row(c[1]) - lo) as usize].push(c);
            }
            let mut nb = template.clone();
            nb.id = pid;
            nb.rows = rows;
            nb.bottom = lo;
            nb.volume = share(part);
            nb.inflows = template
                .inflows
                .iter()
                .filter(|(c, _)| part.contains(c))
                .copied()
                .collect();
            if n == 0 {
                // An inflow at a cell no part holds stays with the largest.
                let all: HashSet<[i32; 3]> = parts.iter().flatten().copied().collect();
                nb.inflows.extend(
                    template
                        .inflows
                        .iter()
                        .filter(|(c, _)| !all.contains(c))
                        .copied(),
                );
            }
            nb.wet = template.wet.intersection(part).copied().collect();
            nb.outlets = Vec::new();
            // The map's flow comes in at one place: the part there keeps it (the nearest part, if it
            // is in none: a cell an edit filled).
            let nearest = |c: &[i32; 3]| -> usize {
                parts
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| {
                        p.iter()
                            .map(|q| {
                                let d = [q[0] - c[0], q[1] - c[1], q[2] - c[2]];
                                i64::from(d[0]).pow(2)
                                    + i64::from(d[1]).pow(2)
                                    + i64::from(d[2]).pow(2)
                            })
                            .min()
                            .unwrap_or(i64::MAX)
                    })
                    .map_or(0, |(i, _)| i)
            };
            // Its natural ways out are the ones by it (each to the nearest part), and what it
            // passes at rest is what they carry (ADR 0057); a part with none passes the map's
            // flow only if the river leaves through it (its seep's exit), else nothing.
            nb.natural.retain(|c, _| nearest(c) == n);
            let law: f64 = nb.natural.values().sum();
            if let Some(st) = &mut nb.steady {
                st.feeds.retain(|(c, _)| {
                    part.contains(c) || (!parts.iter().any(|p| p.contains(c)) && nearest(c) == n)
                });
                // It passes what its ways out carry; with none, what comes in (it fills).
                let fed: f64 = st.feeds.iter().map(|(_, q)| q).sum::<f64>() + st.spring;
                if law > 0.0 {
                    (st.law, st.flow, st.through) = (law, law, law);
                } else {
                    (st.law, st.flow, st.through) = (0.0, fed, fed);
                }
            }
            for &c in part {
                self.owner.insert(c, pid);
            }
            self.basins.insert(pid, nb);
            made.push(pid);
        }
        self.retarget_all(id);
        self.split.extend(made.iter().copied());
        for pid in made {
            // A part settled before may have merged this one away.
            let Some(b) = self.basins.get(&pid) else {
                continue;
            };
            let view = View { world, old: None };
            let mut outlets = scan_outlets(&view, b, &self.owner, &self.reservoir_ids(), pid);
            outlets.extend(self.graph_outlets(world, b));
            self.basins.get_mut(&pid).expect("basin").outlets = outlets;
            self.settle_rows(world, pid, writes);
        }
    }
}

/// A row of a basin's container at local row `lr`: from the seeds (the cells over the row below,
/// and its own cells before), spread sideways over open cells held up by something (a solid block
/// or the row below; the bottom row's seeds need nothing under them). Bodies it meets at the row
/// are reported, not crossed.
#[allow(clippy::too_many_arguments)]
fn spread_row(
    view: &View<'_>,
    f: FaceFrame,
    liquid: StateId,
    lr: i32,
    seeds: &[[i32; 3]],
    below: &HashSet<[i32; 3]>,
    owner: &HashMap<[i32; 3], u32>,
    reservoirs: &HashSet<u32>,
    id: u32,
    bottom: bool,
) -> RowScan {
    let ours = |c: [i32; 3]| owner.get(&c) == Some(&id);
    // A reservoir's cells hold it like a wall: it fills toward the reservoir's level through the
    // outlets between them (drowned), and stays a basin.
    let walled = |c: [i32; 3]| owner.get(&c).is_some_and(|o| reservoirs.contains(o));
    let held = |c: [i32; 3]| {
        let d = up(f, c, -1);
        below.contains(&d) || walls(view.kind(d))
    };
    // Another basin's container: met, not spread into (over a wall, the pool beyond).
    let theirs = |c: [i32; 3]| owner.get(&c).is_some_and(|&o| o != id) && !walled(c);
    let mut cells = Vec::new();
    let mut met = Vec::new();
    let mut seen: HashSet<[i32; 3]> = HashSet::default();
    let mut queue: Vec<[i32; 3]> = Vec::new();
    for &s in seeds {
        if f.row(s[1]) != lr || !seen.insert(s) {
            continue;
        }
        let open = match view.kind(s) {
            Kind::Open if theirs(s) => {
                met.push(s);
                false
            }
            Kind::Open => true,
            // Its own; liquid no one holds is another body's water, to be found (met), and
            // joined if it stands at its level (`merge_wide`).
            Kind::Liquid(m) if m == liquid && ours(s) => true,
            Kind::Liquid(_) if walled(s) => false,
            Kind::Liquid(_) => {
                met.push(s);
                false
            }
            // A block holding water (a ramp): its open part, reached through its open faces.
            Kind::Holds { .. } if theirs(s) => {
                met.push(s);
                false
            }
            Kind::Holds { .. } => ours(s),
            Kind::Solid => false,
        };
        if open && (bottom || held(s) || ours(s)) {
            queue.push(s);
        }
    }
    let mut wide = false;
    while let Some(c) = queue.pop() {
        cells.push(c);
        if cells.len() >= FLOW.max_row_cells as usize {
            wide = true;
            break;
        }
        for s in SIDES {
            let n = side(c, s);
            // Not through a ramp's full side (it may be reached another way).
            if seen.contains(&n) || !joins(view, c, n) {
                continue;
            }
            seen.insert(n);
            match view.kind(n) {
                Kind::Open if theirs(n) => met.push(n),
                Kind::Open if held(n) => queue.push(n),
                Kind::Liquid(m) if m == liquid && ours(n) => queue.push(n),
                Kind::Liquid(_) if walled(n) => {}
                Kind::Liquid(m) if m == liquid => met.push(n),
                Kind::Holds { .. } if theirs(n) => met.push(n),
                Kind::Holds { .. } if ours(n) || held(n) => queue.push(n),
                _ => {}
            }
        }
    }
    cells.sort_unstable();
    RowScan { cells, met, wide }
}

/// A basin's ways out (in `view`): open cells beside its cells with nothing under them (a face
/// from that row's floor, open above unless a block closes it: a hole in a wall), open cells under
/// its cells (a hole in its floor), leaking blocks beside it with an open cell past them, and the
/// faces it shares with other basins' containers (contacts, but where found bodies touched as
/// found: `held`); none through the cells it rests against (`rest`), and a natural outlet at its
/// scale.
fn scan_outlets(
    view: &View<'_>,
    b: &Basin,
    owner: &HashMap<[i32; 3], u32>,
    reservoirs: &HashSet<u32>,
    id: u32,
) -> Vec<Outlet> {
    let f = b.face;
    let cells: HashSet<[i32; 3]> = b.cells().collect();
    let ours = |c: [i32; 3]| cells.contains(&c) || owner.get(&c) == Some(&id);
    let theirs = |c: [i32; 3]| match owner.get(&c) {
        Some(&o) if o != id && !reservoirs.contains(&o) && !cells.contains(&c) => Some(o),
        _ => None,
    };
    let level = b.level();
    let mut out: Vec<Outlet> = Vec::new();
    let mut at: HashSet<([i32; 3], bool)> = HashSet::default();
    let mut add =
        |out: &mut Vec<Outlet>, from: [i32; 3], kind: OutKind, leak: bool, to: Option<u32>| {
            if at.insert((from, leak)) {
                out.push(Outlet {
                    from,
                    kind,
                    target: to.map_or(Target::Unresolved, Target::Basin),
                    flow: 0.0,
                    scale: if !leak && b.natural.contains_key(&from) {
                        b.natural_scale()
                    } else {
                        1.0
                    },
                    contact: to.is_some(),
                    cells: 1.0,
                });
            }
        };
    for (i, row) in b.rows.iter().enumerate() {
        let lr = b.bottom + i as i32;
        let floor = m_of_row(lr);
        for &c in row {
            let d = up(f, c, -1);
            if !ours(d) && joins(view, c, d) {
                if let Some(o) = theirs(d) {
                    add(&mut out, d, OutKind::Floor { z: floor }, false, Some(o));
                } else if view.kind(d) == Kind::Open {
                    add(&mut out, d, OutKind::Floor { z: floor }, false, None);
                }
            }
            for s in SIDES {
                let n = side(c, s);
                // Not out through a ramp's full side.
                if ours(n) || !joins(view, c, n) {
                    continue;
                }
                // Open above it (another cell to pour over), or a hole capped by a block.
                let hi = if walls(view.kind(up(f, n, 1))) {
                    floor + CELL_H
                } else {
                    f64::INFINITY
                };
                if let Some(o) = theirs(n) {
                    let face = OutKind::Face {
                        lo: floor,
                        hi: floor + CELL_H,
                    };
                    add(&mut out, n, face, false, Some(o));
                    continue;
                }
                match view.kind(n) {
                    Kind::Open => {
                        let under = up(f, n, -1);
                        let held = ours(under) || walls(view.kind(under));
                        if !held {
                            add(&mut out, n, OutKind::Face { lo: floor, hi }, false, None);
                        }
                    }
                    Kind::Solid => {
                        let k = view.get(n).map_or(0.0, leak_of);
                        let past = side(n, s);
                        if k > 0.0 && !ours(past) && view.kind(past) == Kind::Open {
                            add(
                                &mut out,
                                past,
                                OutKind::Leak {
                                    k,
                                    centre: floor + CELL_H / 2.0,
                                },
                                true,
                                None,
                            );
                        }
                    }
                    // Water no body holds yet standing lower (the next pool, meeting this one
                    // under its surface): a way out, found when it is followed (ADR 0059).
                    Kind::Liquid(_) | Kind::Holds { flooded: true }
                        if source_of(view, f, n).is_some_and(|(top, _)| top < level - 0.01) =>
                    {
                        let face = OutKind::Face {
                            lo: floor,
                            hi: floor + CELL_H,
                        };
                        add(&mut out, n, face, false, None);
                    }
                    Kind::Liquid(_) | Kind::Holds { .. } => {}
                }
            }
        }
    }
    out
}

/// Water standing at cell `c` that isn't moving (a liquid's cell, a ramp holding some): its
/// surface (local m) and how deep it stands in the cell (m). None for anything else.
fn source_of(view: &View<'_>, f: FaceFrame, c: [i32; 3]) -> Option<(f64, f64)> {
    let m = view.get(c)?;
    let floor = m_of_row(f.row(c[1]));
    match kind(Some(m)) {
        Kind::Liquid(_) => {
            let fill = f64::from(info(m).fill).clamp(0.0, 1.0);
            Some((floor + fill * CELL_H, fill * CELL_H))
        }
        Kind::Holds { flooded: true } => Some((floor + CELL_H, CELL_H)),
        _ => None,
    }
}

/// A block with a `flooded` state (a ramp: water stands in its open part) at `c` takes it while
/// liquid is beside it, and loses it when none is (docs/WATER_FLOW.md §15).
fn flood_edge(world: &mut VoxelWorld, c: [i32; 3], writes: &mut Vec<([i32; 3], StateId)>) {
    if world.find(chunk_of(c[0], c[1], c[2])).is_none() {
        return;
    }
    let m = world.get_voxel_if_loaded(c);
    if dwell_blocks::try_info(m).is_none_or(|s| s.liquid) {
        return;
    }
    let mut beside: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
    beside.push([c[0], c[1] + 1, c[2]]);
    beside.push([c[0], c[1] - 1, c[2]]);
    let wet = beside
        .iter()
        .any(|&n| dwell_blocks::try_info(world.get_voxel_if_loaded(n)).is_some_and(|i| i.liquid));
    let want = if wet { "true" } else { "false" };
    if let Some(to) = dwell_blocks::with_property(m, "flooded", want)
        && to != m
    {
        world.write_voxel(c[0], c[1], c[2], to);
        writes.push((c, to));
    }
}

/// Whether a basin of surface `area` (m²) whose volume moved by `moved` (m³) in `dt` (s) is
/// settled: its level moving slower than `settleMmPerHour`.
fn settled(moved: f64, area: f64, dt: f64) -> bool {
    moved < area * FLOW.settle_mm_per_hour / 1000.0 / 3600.0 * dt.max(1e-9)
}

/// The chunks beside `cells` (a side, above or below) that aren't loaded.
fn edge_of(
    world: &VoxelWorld,
    cells: impl Iterator<Item = [i32; 3]>,
) -> Vec<dwell_world::ChunkCoord> {
    let mut out: Vec<dwell_world::ChunkCoord> = Vec::new();
    for c in cells {
        let mut near: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
        near.push([c[0], c[1] + 1, c[2]]);
        near.push([c[0], c[1] - 1, c[2]]);
        for n in near {
            let k = chunk_of(n[0], n[1], n[2]);
            if !out.contains(&k) && world.find(k).is_none() {
                out.push(k);
            }
        }
    }
    out.sort_unstable_by_key(|c| (c.x, c.y, c.z));
    out
}

/// The chunks cells lie in.
fn chunks_of(cells: impl Iterator<Item = [i32; 3]>) -> Vec<dwell_world::ChunkCoord> {
    let mut out: Vec<dwell_world::ChunkCoord> = cells.map(|c| chunk_of(c[0], c[1], c[2])).collect();
    out.sort_unstable_by_key(|c| (c.x, c.y, c.z));
    out.dedup();
    out
}

/// Where running liquid from an open cell ends.
#[derive(Debug)]
enum End {
    /// On a body of liquid (this cell of it).
    Liquid([i32; 3]),
    /// On the floor with no way down within reach: it pools at this open cell.
    Pool([i32; 3]),
    Lost,
}

/// Follows liquid from the open cell `from` to the nearest lower water (owner, 2026-11-04: "it would
/// pour down the banks into the nearest lower point … steps should be like slopes to water"): over
/// the ground's surface, a step down of any height taken as part of the way (a slope to water),
/// breadth first by cells across, so it goes the shortest way down into water; with none within
/// `streamReach` cells across, out of the loaded world if it can fall there, else it pools at the
/// lowest place it reached. Returns where it ends and
/// the open cells it runs through on its way (from `from`: the cells it falls through and runs
/// along), which are drawn running while it flows.
fn follow_path(
    view: &View<'_>,
    f: FaceFrame,
    from: [i32; 3],
    own: &dyn Fn([i32; 3]) -> bool,
) -> (End, Vec<[i32; 3]>) {
    // The source's own cells are walls to its water.
    let kind_of = |c: [i32; 3]| if own(c) { Kind::Solid } else { view.kind(c) };
    // From an open cell down to where it rests: the cells it falls through, and how it ends.
    enum Fall {
        Rests([i32; 3]),
        Water([i32; 3]),
        Lost,
    }
    let fall = |c: [i32; 3], cells: &mut Vec<[i32; 3]>| -> Fall {
        let mut at = c;
        for _ in 0..2048 {
            let d = up(f, at, -1);
            if view.get(d).is_none() {
                return Fall::Lost; // the floor is unloaded
            }
            match kind_of(d) {
                Kind::Open => {
                    at = d;
                    cells.push(d);
                }
                Kind::Liquid(_) => return Fall::Water(d),
                Kind::Solid | Kind::Holds { .. } => return Fall::Rests(at),
            }
        }
        Fall::Lost
    };
    let mut first = vec![from];
    let start = match fall(from, &mut first) {
        Fall::Rests(c) => c,
        Fall::Water(w) => return (End::Liquid(w), first),
        Fall::Lost => return (End::Lost, first),
    };
    // Breadth first over where it rests, each place's way back (the place before, and the cells
    // from it to here).
    let mut back: HashMap<[i32; 3], ([i32; 3], Vec<[i32; 3]>)> = HashMap::default();
    back.insert(start, (start, Vec::new()));
    let mut frontier = vec![start];
    let mut lowest = start;
    let mut end: Option<(End, [i32; 3], Vec<[i32; 3]>)> = None;
    let mut lost: Option<([i32; 3], Vec<[i32; 3]>)> = None;
    'search: for _ in 0..FLOW.stream_reach {
        let mut next = Vec::new();
        for &p in &frontier {
            for s in SIDES {
                let n = side(p, s);
                if back.contains_key(&n) || view.get(n).is_none() {
                    continue;
                }
                match kind_of(n) {
                    Kind::Liquid(_) => {
                        end = Some((End::Liquid(n), p, Vec::new()));
                        break 'search;
                    }
                    Kind::Open => {
                        let mut cells = vec![n];
                        match fall(n, &mut cells) {
                            Fall::Rests(r) => {
                                if back.contains_key(&r) {
                                    continue;
                                }
                                if f.row(r[1]) < f.row(lowest[1]) {
                                    lowest = r;
                                }
                                back.insert(r, (p, cells));
                                next.push(r);
                            }
                            Fall::Water(w) => {
                                end = Some((End::Liquid(w), p, cells));
                                break 'search;
                            }
                            Fall::Lost => {
                                // Out of the loaded world: only if no water is nearer.
                                if lost.is_none() {
                                    lost = Some((p, cells));
                                }
                            }
                        }
                    }
                    Kind::Solid | Kind::Holds { .. } => {}
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    // The way there, from the start.
    let way = |mut at: [i32; 3]| {
        let mut parts: Vec<Vec<[i32; 3]>> = Vec::new();
        while at != start {
            let (prev, cells) = &back[&at];
            parts.push(cells.clone());
            at = *prev;
        }
        parts.reverse();
        parts.concat()
    };
    match end {
        Some((e, p, last)) => {
            let mut path = first;
            path.extend(way(p));
            path.extend(last);
            (e, path)
        }
        None if lost.is_some() => {
            // No water within reach, but a way out of the loaded world: gone there.
            let (p, last) = lost.expect("lost");
            let mut path = first;
            path.extend(way(p));
            path.extend(last);
            (End::Lost, path)
        }
        None => {
            // No water within reach: it pools at the lowest place it reached.
            let mut path = first;
            path.extend(way(lowest));
            (End::Pool(lowest), path)
        }
    }
}

mod rivers;
mod save;
mod virtuals;

pub use rivers::{GraphLip, GraphPool, PoolInflows, PoolKey, RiverNext, Rivers};

#[cfg(test)]
mod tests;
