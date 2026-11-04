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

/// A liquid's flow rules (`shared/fluids/flow.json` `liquids`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidFlow {
    pub block: &'static str,
    /// m^½/s: an outlet passes `weir × width × head^1.5` m³/s.
    pub weir: f64,
    /// The block it runs as between basins (drawn as it, open to the basins).
    pub running: &'static str,
}

/// The water flow data (`shared/fluids/flow.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowData {
    /// The default water speed (the server's `water_speed`), × game time.
    pub speed: f64,
    pub tick_every: u32,
    pub live_radius_m: f64,
    pub max_basin_cells: u32,
    pub max_row_cells: u32,
    pub stream_reach: u32,
    /// An outlet passing at least this (m³/s) is drawn running.
    pub stream_min_flow: f64,
    /// Basins sharing faces that would even out within this (s) merge.
    pub merge_seconds: f64,
    /// The most flow steps a water tick spends bringing waiting basins forward.
    pub catch_up_steps: u32,
    /// A basin whose level moves slower than this (mm an hour of water time) is settled.
    pub settle_mm_per_hour: f64,
    /// How fast a change to a river's flow travels down the map's rivers (m/s of water time).
    pub river_speed: f64,
    /// The least change (m³/s) a body sends down a river.
    pub river_change_min: f64,
    /// A map lake's head over its outlet (m): how fast its outflow follows a change coming in.
    pub lake_outlet_head: f64,
    pub liquids: &'static [LiquidFlow],
    /// Block name → m³/s per m² of face per m of head.
    pub leaks: &'static [(&'static str, f64)],
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod flow_generated {
    include!("flow.gen.rs");
}
pub use flow_generated::FLOW;

/// Rows a basin may stack at most (a well 256 m deep).
const MAX_ROWS: usize = 512;
/// Outlets whose way is found a water tick at most (bodies found on the way included).
const ROUTES_PER_TICK: usize = 1024;
/// Two basins that meet merge when their levels are this close (m).
const MERGE_GAP: f64 = 0.02;
/// Flow steps a water tick takes at most.
const MAX_STEPS: usize = 64;

/// The weir coefficient of a liquid state, if it flows.
fn weir_of(m: StateId) -> Option<f64> {
    let s = info(m);
    if !s.liquid {
        return None;
    }
    let name = BLOCKS[s.block].name;
    FLOW.liquids
        .iter()
        .find(|l| l.block == name)
        .map(|l| l.weir)
}

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
}

/// Each liquid state's full state (its fill 1): a cell part full is the same liquid.
static FULL: std::sync::LazyLock<Vec<StateId>> = std::sync::LazyLock::new(|| {
    let states = dwell_blocks::STATES;
    (0..states.len())
        .map(|m| {
            let s = &states[m];
            if !s.liquid || s.fill >= 1.0 {
                return m as StateId;
            }
            states
                .iter()
                .position(|x| x.block == s.block && x.fill >= 1.0)
                .map_or(m as StateId, |i| i as StateId)
        })
        .collect()
});

/// The full state of liquid state `m` (itself when full or no liquid).
fn full_of(m: StateId) -> StateId {
    FULL.get(usize::from(m)).copied().unwrap_or(m)
}

/// Each full liquid's states by fill, emptiest first (one, full, for a liquid without a fill).
static FILLS: std::sync::LazyLock<HashMap<StateId, Vec<(f32, StateId)>>> =
    std::sync::LazyLock::new(|| {
        let mut out: HashMap<StateId, Vec<(f32, StateId)>> = HashMap::default();
        for (m, s) in dwell_blocks::STATES.iter().enumerate() {
            if s.liquid {
                out.entry(full_of(m as StateId))
                    .or_default()
                    .push((s.fill, m as StateId));
            }
        }
        for v in out.values_mut() {
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        out
    });

/// How many fills liquid `full` is drawn in (its cells from part full to full).
fn fill_steps(full: StateId) -> usize {
    FILLS.get(&full).map_or(1, Vec::len).max(1)
}

/// Liquid `full`'s state for a cell `frac` full: the nearest of its fills (at least its least).
fn fill_state(full: StateId, frac: f64) -> StateId {
    let Some(v) = FILLS.get(&full) else {
        return full;
    };
    v.iter()
        .min_by(|a, b| {
            (f64::from(a.0) - frac)
                .abs()
                .total_cmp(&(f64::from(b.0) - frac).abs())
        })
        .map_or(full, |&(_, m)| m)
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
    /// A body found at rest with no natural outlet: what it passed then, gone unseen, by the weir
    /// law over a sill half a row under the level it was found at (`Steady`).
    Seep { sill: f64 },
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
}

/// g (m/s²).
const G: f64 = 9.81;

/// The discharge coefficient of an opening, from a liquid's weir coefficient: a free sill's
/// `weir × head^1.5` is the opening law summed up its face, `C × (2/3) × √(2g) × head^1.5`.
fn discharge(weir: f64) -> f64 {
    weir / (2.0 / 3.0 * (2.0 * G).sqrt())
}

/// The flow (m³/s per m of width) through a vertical face from `lo` to `hi` (local m) for water
/// standing at `high` on one side and `low` on the other (`-∞`: none): below `low` the face is
/// drowned, every part of it pushed by the difference of the levels, `C × area × √(2 g Δh)`; between
/// `low` and `high` the water pours freely, each part by its own depth under `high` (summed, the
/// weir law). Hydrostatics: the pressure difference across a submerged opening is `ρ g Δh` at
/// every depth (docs/WATER_FLOW.md §4).
fn face_flow(c: f64, high: f64, low: f64, lo: f64, hi: f64) -> f64 {
    if high <= lo {
        return 0.0;
    }
    let top = hi.min(high);
    let mut q = 0.0;
    if low > lo {
        let d_hi = low.min(top);
        q += c * (d_hi - lo) * (2.0 * G * (high - low)).sqrt();
    }
    let f_lo = lo.max(low);
    if top > f_lo {
        q += c
            * (2.0 * G).sqrt()
            * (2.0 / 3.0)
            * ((high - f_lo).powf(1.5) - (high - top).max(0.0).powf(1.5));
    }
    q
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
            // No more than it passed as found (at the level it was found at, a row over its sill).
            OutKind::Seep { sill } => {
                self.scale * weir * (level - sill).clamp(0.0, CELL_H).powf(1.5)
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
    fn is_seep(&self) -> bool {
        matches!(self.kind, OutKind::Seep { .. })
    }
    fn floor(&self) -> f64 {
        match self.kind {
            OutKind::Face { lo, .. } => lo,
            OutKind::Floor { z } => z,
            OutKind::Seep { sill } => sill,
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
    /// What came in last flow step by found bodies' natural outlets (`Water::links`): with no
    /// natural outlet of its own, all it passes on unseen besides its map flow (water an edit
    /// sends it, or a flood beside it, fills it).
    linked: f64,
    /// Water time (s) it waited out of reach or unloaded: brought forward when it is live again
    /// (`Water::catch_up`, docs/WATER_FLOW.md §6).
    pub owed: f64,
    /// Filling or draining (its level moved faster than `settleMmPerHour` last tick, or it owes
    /// time): it flows with no player near, its chunks kept loaded (`Water::wanted_chunks`).
    pub active: bool,
    /// The surface row and fill its cells were last written at (`top_steps`).
    drawn_fill: (usize, usize),
    /// Cells that hold it like walls: the world as it was found is at rest (docs/WATER_FLOW.md
    /// §2.2), so an open cell beside a body as generated that it didn't fill, and a leaking block
    /// it stood against, keep it in until an edit touches them.
    rest: HashSet<[i32; 3]>,
    /// Liquid of other bodies it touched as found (a river's next step under its surface): faces
    /// that pass nothing, as generated (the river's flow goes by the natural outlets).
    held: HashSet<[i32; 3]>,
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
}

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
}

impl Steady {
    fn feed(&self) -> f64 {
        self.feeds.iter().map(|(_, q)| q).sum()
    }
}

impl Basin {
    /// Its inflow (m³/s, real).
    pub fn inflow(&self) -> f64 {
        self.inflows.iter().map(|(_, q)| q).sum()
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
    /// the nearest (docs/WATER_FLOW.md §15).
    fn top_steps(&self) -> (usize, usize) {
        let (k, frac) = self.fill();
        let n = fill_steps(self.liquid);
        (k, ((frac * n as f64).round() as usize).min(n))
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
            if !o.is_seep() && self.natural.contains_key(&o.from) {
                o.scale = scale;
            }
        }
        self.outlets.retain(|o| !o.is_seep());
        if let Some(seep) = self.seep() {
            self.outlets.push(seep);
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

    /// Its seep (with no natural outlet: the flow it was found passing, gone unseen).
    fn seep(&self) -> Option<Outlet> {
        let st = self.steady.as_ref()?;
        if st.law > 0.0 || st.through <= 0.0 {
            return None;
        }
        let from = *self.rows.first()?.first()?;
        Some(Outlet {
            from,
            kind: OutKind::Seep {
                sill: st.level - CELL_H,
            },
            target: Target::Lost,
            flow: 0.0,
            scale: st.through / (self.weir * CELL_H.powf(1.5)),
            contact: false,
        })
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
    /// How found bodies' steady flows join (docs/WATER_FLOW.md §2.2): a natural outlet of one
    /// (basin, its cell) found running into another → that basin and the outlet's weight (its
    /// flow at the level it was found at, at scale 1). Kept when the outlet is blocked: what the
    /// map's flow below already held comes off it either way.
    links: HashMap<(u32, [i32; 3]), (u32, f64)>,
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
    /// Where found bodies' natural outlets ran, as found, into liquid no body owned yet (cell →
    /// basin, outlet cell, weight): linked when a body there is found.
    lands: HashMap<[i32; 3], Vec<Land>>,
    /// The world map's rivers (docs/WATER_FLOW.md §6, W2d): changes carried down them.
    river_map: Option<std::sync::Arc<dyn Rivers>>,
    /// The fronts on them and the lakes they ran into.
    flows: rivers::Flows,
    /// Where cells lie on the map's rivers (asked once each).
    located: HashMap<[i32; 3], Option<(dwell_world::Face, u32, f64)>>,
}

/// A found body's natural outlet as it ran (basin, outlet cell, weight), waiting for a body where
/// it landed.
type Land = (u32, [i32; 3], f64);

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
            links: HashMap::default(),
            pending: HashMap::default(),
            rescan: HashSet::default(),
            paths: HashMap::default(),
            drawn: HashSet::default(),
            edited: HashSet::default(),
            split: HashSet::default(),
            lands: HashMap::default(),
            river_map: None,
            flows: rivers::Flows::default(),
            located: HashMap::default(),
        }
    }

    /// Water fed by `runoff` (the world map's flow).
    pub fn with_runoff(runoff: Runoff) -> Self {
        Self {
            runoff: Some(runoff),
            ..Self::new()
        }
    }

    /// The changes going down the map's rivers now: (river, from, at, m³/s) each.
    pub fn fronts(&self) -> Vec<(u32, f64, f64, f64)> {
        self.flows
            .fronts
            .iter()
            .map(|f| (f.river, f.from, f.at, f.dq))
            .collect()
    }

    /// Water whose changes go down the world map's rivers `rivers` (docs/WATER_FLOW.md §6).
    pub fn with_rivers(mut self, rivers: std::sync::Arc<dyn Rivers>) -> Self {
        self.river_map = Some(rivers);
        self
    }

    /// Where cell `c` lies on the map's rivers (its face's river and how far along it).
    fn locate(&mut self, c: [i32; 3]) -> Option<(dwell_world::Face, u32, f64)> {
        let rivers = self.river_map.as_ref()?;
        *self.located.entry(c).or_insert_with(|| {
            let f = FaceFrame::at_row(c[1]);
            rivers.at(f.face, c[0], c[2]).map(|(r, s)| (f.face, r, s))
        })
    }

    /// The change in the map river's flow where a body's water comes in (`entry`).
    fn offset_at(&mut self, entry: [i32; 3]) -> f64 {
        match self.locate(entry) {
            Some((face, r, s)) => self.flows.offset(face, r, s),
            None => 0.0,
        }
    }

    /// What the water sends down the map's rivers (docs/WATER_FLOW.md §6, W2d): each body's
    /// change in what it sends into country nobody has found (its natural outlets' flow as
    /// found against what leaves by its ways out onto the map's river now) goes down that river
    /// as a front; the fronts go on `dt` s, stopping at the first body taking them in.
    fn carry_down(&mut self, live: &HashSet<u32>, dt: f64) {
        let Some(rivers) = self.river_map.clone() else {
            return;
        };
        // The heads of the bodies found on each river: where its flow comes into them.
        let mut heads: HashMap<(dwell_world::Face, u32), Vec<f64>> = HashMap::default();
        let entries: Vec<[i32; 3]> = self
            .basins
            .values()
            .filter(|b| !b.reservoir && b.inflow() > 0.0)
            .filter_map(|b| b.steady.as_ref()?.feeds.first().map(|f| f.0))
            .collect();
        for c in entries {
            if let Some((face, r, s)) = self.locate(c) {
                heads.entry((face, r)).or_default().push(s);
            }
        }
        // What each body sends into the unfound now, against what it did as found.
        let mut ids: Vec<u32> = self.basins.keys().copied().collect();
        ids.sort_unstable();
        let mut now: Vec<(u32, [i32; 3], f64)> = Vec::new();
        for &id in &ids {
            let b = &self.basins[&id];
            // Only the found river's bodies: water collecting elsewhere came from them.
            if b.reservoir || b.steady.is_none() {
                continue;
            }
            // Into the found river (bodies found at rest: their water is the river's, carried on
            // by them) is not sent down the map's; anything else on the river is.
            let found = |t: Target| match t {
                Target::Basin(t) => self.basins.get(&t).is_some_and(|x| x.steady.is_some()),
                _ => false,
            };
            let mut site: Option<[i32; 3]> = None;
            let mut expected = 0.0;
            if let Some(st) = &b.steady
                && st.law > 0.0
            {
                let mut natural: Vec<([i32; 3], f64)> =
                    b.natural.iter().map(|(&c, &w)| (c, w)).collect();
                natural.sort_unstable_by_key(|&(c, _)| c);
                for (c, w) in natural {
                    let into_found = b.outlets.iter().any(|o| o.from == c && found(o.target));
                    if self.owner.get(&c) == Some(&id)
                        || self.links.contains_key(&(id, c))
                        || into_found
                    {
                        continue;
                    }
                    expected += st.flow * w / st.law;
                    site.get_or_insert(c);
                }
            } else if let Some(st) = &b.steady {
                expected = st.flow;
            }
            let mut actual = 0.0;
            // Its flow by the opening law at its level now (an outlet just found has passed
            // nothing yet); one filling or draining that waits unloaded passes nothing.
            let level = b.level();
            let holds = b.active && !live.contains(&id);
            let rate = |o: &Outlet| {
                if holds {
                    return 0.0;
                }
                let below = match o.target {
                    Target::Basin(t) => {
                        self.basins.get(&t).map_or(f64::NEG_INFINITY, |x| x.level())
                    }
                    _ => f64::NEG_INFINITY,
                };
                o.rate(level, b.weir, below)
            };
            // Where it goes on the map's river: out of the found world where it leaves, into water
            // collecting below (a pool under a fall) where that lies.
            let leaving: Vec<([i32; 3], [i32; 3], f64)> = b
                .outlets
                .iter()
                .filter(|o| !o.contact && !o.is_seep() && !found(o.target))
                .filter_map(|o| match o.target {
                    Target::Lost => Some((o.from, o.from, rate(o))),
                    Target::Basin(t) => self
                        .basins
                        .get(&t)
                        .and_then(|x| x.rows.first()?.first().copied())
                        .map(|c| (o.from, c, rate(o))),
                    Target::Unresolved => None,
                })
                .collect();
            let seep = b.outlets.iter().find(|o| o.is_seep()).map(rate);
            let first = b.rows.first().and_then(|r| r.first()).copied();
            for (from, c, q) in leaving {
                for at in [from, c] {
                    if self.locate(at).is_some() {
                        actual += q;
                        site.get_or_insert(at);
                        break;
                    }
                }
            }
            if let (Some(q), Some(c)) = (seep, first) {
                actual += q;
                site.get_or_insert(c);
            }
            if let Some(c) = site {
                now.push((id, c, actual - expected));
            }
        }
        for (id, c, d) in now {
            let last = self.flows.sent.get(&id).copied();
            let base = last.map_or(0.0, |l| l.dq);
            if (d - base).abs() < FLOW.river_change_min {
                continue;
            }
            let (face, r, s) = match last {
                Some(l) => (l.face, l.river, l.s),
                None => match self.locate(c) {
                    Some(at) => at,
                    None => continue,
                },
            };
            self.flows.send(face, r, s, d - base);
            self.flows.sent.insert(
                id,
                rivers::Sent {
                    face,
                    river: r,
                    s,
                    dq: d,
                },
            );
        }
        // Bodies gone (merged, dried): what they sent is taken back.
        let gone: Vec<u32> = self
            .flows
            .sent
            .keys()
            .copied()
            .filter(|id| !self.basins.contains_key(id))
            .collect();
        for id in gone {
            if let Some(l) = self.flows.sent.remove(&id) {
                self.flows.send(l.face, l.river, l.s, -l.dq);
            }
        }
        self.flows
            .advance(rivers.as_ref(), &heads, FLOW.river_speed, dt);
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
            self.rebuild(world, id, &mut writes);
        }
        self.apply_touched(world, &mut writes);
        self.route(world, &mut writes);
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
        self.carry_down(&live, dt);
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
        let mut linked: HashMap<u32, f64> = HashMap::default();
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
                let flow = st.flow;
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
                        if self.links.contains_key(&(id, o.from)) {
                            *linked.entry(t).or_default() += o.flow;
                        } else {
                            *extra.entry(t).or_default() += o.flow;
                        }
                    }
                    Target::Lost => self.lost += o.flow * dt,
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
            b.linked = linked.get(&b.id).copied().unwrap_or(0.0);
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
                && kind(Some(world.get_voxel_if_loaded(c))) == Kind::Solid;
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
        // An edit beside a body changes what holds it at rest: those cells are the world now.
        for b in self.basins.values_mut() {
            let before = b.rest.len();
            for c in &near {
                b.rest.remove(c);
            }
            if b.rest.len() != before {
                dirty.insert(b.id);
            }
        }
        for id in dirty {
            if let Some(b) = self.basins.get_mut(&id) {
                b.active = true;
            }
            self.rebuild(world, id, writes);
        }
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
        let same = |c: [i32; 3]| view.kind(c) == Kind::Liquid(liquid);
        // The surface: up the seed's column.
        let mut top = seed;
        while same(up(f, top, 1)) {
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
                if same(n) && same([n[0], f.row(surface), n[2]]) {
                    next.push(n);
                }
            }
            let below = up(f, c, -1);
            if same(below) {
                next.push(below);
            }
            let above = up(f, c, 1);
            if f.row(above[1]) <= surface && same(above) {
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
                    linked: 0.0,
                    owed: 0.0,
                    active: false,
                    drawn_fill: (usize::MAX, 0),
                    rest: HashSet::default(),
                    held: HashSet::default(),
                    natural: HashMap::default(),
                    chunks: Vec::new(),
                    edge: Vec::new(),
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
            linked: 0.0,
            owed: 0.0,
            active: false,
            drawn_fill: (usize::MAX, 0),
            rest: HashSet::default(),
            held: HashSet::default(),
            natural: HashMap::default(),
            chunks: Vec::new(),
            edge: Vec::new(),
            steady: None,
        };
        if resting {
            b.rest = at_rest(&view, f, &seen);
            b.held = seen
                .iter()
                .flat_map(|&c| {
                    let mut n: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
                    n.push([c[0], c[1] + 1, c[2]]);
                    n.push([c[0], c[1] - 1, c[2]]);
                    n
                })
                .filter(|n| !seen.contains(n) && matches!(view.kind(*n), Kind::Liquid(_)))
                .collect();
            // Its natural outlets pass the map's flow at the level it was found at, coming in at
            // its cell farthest from them (a river pool's upstream end).
            let outlets = scan_outlets(&view, &b, &self.owner, &self.reservoir_ids(), id);
            let level = b.level();
            let law: f64 = outlets
                .iter()
                .map(|o| o.rate(level, weir, f64::NEG_INFINITY))
                .sum();
            let q = self.runoff_of(f, &seen);
            b.natural = outlets
                .iter()
                .map(|o| (o.from, o.rate(level, weir, f64::NEG_INFINITY)))
                .collect();
            let entry = farthest_from(&seen, outlets.iter().map(|o| o.from));
            b.outlets = outlets;
            b.steady = Some(Steady {
                through: 0.0,
                flow: q,
                law,
                level,
                feeds: if q > 0.0 {
                    vec![(entry, q)]
                } else {
                    Vec::new()
                },
            });
            b.pass(q);
            if q > 0.0 {
                b.inflows.push((entry, q));
            }
        }
        b.chunks = chunks_of(b.cells());
        b.edge = edge_of(world, b.cells());
        // Where its natural outlets ran in the world as found (an edit may block them now): the
        // bodies there are linked to it, now or when found.
        let mut lands: Vec<([i32; 3], [i32; 3], f64)> = Vec::new();
        if b.steady.is_some() {
            for (&from, &w) in &b.natural {
                if let (End::Liquid(c), _) = follow_path(&view, f, from, &|c| seen.contains(&c)) {
                    lands.push((c, from, w));
                }
            }
        }
        for c in b.cells() {
            self.owner.insert(c, id);
        }
        self.basins.insert(id, b);
        for (c, from, w) in lands {
            match self.owner.get(&c) {
                Some(&t) if t != id => {
                    self.links.entry((id, from)).or_insert((t, w));
                }
                Some(_) => {}
                None => self.lands.entry(c).or_default().push((id, from, w)),
            }
        }
        // Natural outlets of bodies found before that ran into this one.
        let mine: Vec<[i32; 3]> = self
            .lands
            .keys()
            .filter(|c| seen.contains(*c))
            .copied()
            .collect();
        for c in mine {
            for (u, from, w) in self.lands.remove(&c).unwrap_or_default() {
                if u != id && self.basins.contains_key(&u) {
                    self.links.entry((u, from)).or_insert((id, w));
                }
            }
        }
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
            let rest = b.rest.clone();
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
                    &rest,
                );
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
            outlets.extend(b.seep());
            self.basins.get_mut(&id).expect("basin").outlets = outlets;
            self.split_if_apart(world, id, writes);
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
                .is_some_and(|b| b.outlets.iter().any(|o| o.from == *from && !o.is_seep()))
        });
        let mut want: HashMap<[i32; 3], StateId> = HashMap::default();
        for b in self.basins.values() {
            let Some(run) = running_of(b.liquid) else {
                continue;
            };
            for o in &b.outlets {
                // A natural outlet's water runs as generated (the river's own steps): drawn are the
                // ways edits made (over a dam, through a cut).
                if o.flow < FLOW.stream_min_flow
                    || o.target == Target::Unresolved
                    || o.is_seep()
                    || b.natural.contains_key(&o.from)
                {
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
                world.set_voxel(c[0], c[1], c[2], M.air);
                writes.push((c, M.air));
            }
        }
        for (c, run) in want {
            if self.drawn.contains(&c) || world.find(chunk_of(c[0], c[1], c[2])).is_none() {
                continue;
            }
            if world.get_voxel_if_loaded(c) == M.air {
                world.set_voxel(c[0], c[1], c[2], run);
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
        outlets.extend(b.seep());
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
                    });
                }
            }
        }
        outlets.retain(|o| view.kind(o.from) == Kind::Open);
        self.basins.get_mut(&id).expect("basin").outlets = outlets;
    }

    /// `b` joins `a`: its liquid, inflow and container.
    fn merge(&mut self, a: u32, b: u32) {
        // The map's flow one of them sends the other is counted once: it was in both feeds, so
        // it comes off the receiving one's.
        let mut internal: HashMap<u32, f64> = HashMap::default();
        for (&(f, _), &(t, w)) in &self.links {
            if ((f == a && t == b) || (f == b && t == a))
                && let Some(st) = self.basins.get(&f).and_then(|x| x.steady.as_ref())
                && st.law > 0.0
            {
                *internal.entry(t).or_default() += st.through * w / st.law;
            }
        }
        for (t, mut q) in internal {
            if let Some(st) = self.basins.get_mut(&t).and_then(|x| x.steady.as_mut()) {
                for (_, f) in &mut st.feeds {
                    let take = q.min(*f);
                    *f -= take;
                    q -= take;
                }
                st.feeds.retain(|(_, f)| *f > 1e-12);
            }
        }
        let other = self.basins.remove(&b).expect("basin");
        for c in other.cells() {
            self.owner.insert(c, a);
        }
        let me = self.basins.get_mut(&a).expect("basin");
        me.volume += other.volume;
        me.inflows.extend(other.inflows);
        me.rest.extend(other.rest);
        me.held.extend(other.held);
        me.natural.extend(other.natural);
        me.steady = match (me.steady.take(), other.steady) {
            (Some(mut x), Some(o)) => {
                x.through += o.through;
                x.flow = x.flow.max(o.flow);
                x.law += o.law;
                x.level = x.level.min(o.level);
                x.feeds.extend(o.feeds);
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
        self.links = std::mem::take(&mut self.links)
            .into_iter()
            .map(|((f, c), (t, w))| {
                let f = if f == b { a } else { f };
                let t = if t == b { a } else { t };
                ((f, c), (t, w))
            })
            .filter(|((f, _), (t, _))| f != t)
            .collect();
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
            self.paths.insert((id, from), path);
            let target = match end {
                End::Lost => Target::Lost,
                End::Liquid(c) => match self.owner.get(&c) {
                    Some(&o) if o != id => Target::Basin(o),
                    Some(_) => Target::Lost, // unreachable: its own cells are walls to it
                    None => self
                        .discover(world, None, c, true)
                        .map_or(Target::Lost, Target::Basin),
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
            if let Target::Basin(t) = target {
                self.link(id, i, t);
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

    /// A natural outlet `i` of a found basin `id` runs into found basin `t`: linked, once.
    fn link(&mut self, id: u32, i: usize, t: u32) {
        if t == id {
            return;
        }
        let (Some(b), Some(tb)) = (self.basins.get(&id), self.basins.get(&t)) else {
            return;
        };
        if b.steady.is_none() || tb.steady.is_none() {
            return;
        }
        let Some(o) = b.outlets.get(i) else {
            return;
        };
        if let Some(&w) = b.natural.get(&o.from) {
            self.links.entry((id, o.from)).or_insert((t, w));
        }
    }

    /// The found bodies' steady flows, upstream first (by the level each was found at): each
    /// passes the more of its map flow and what its links bring it, and takes in its map flow
    /// less what they bring (docs/WATER_FLOW.md §2.2), by its natural outlets that are still ways
    /// out of it, or unseen with none.
    fn balance(&mut self) {
        // What changes upstream send down the map's rivers to where each body's water comes in.
        let entries: Vec<(u32, [i32; 3])> = self
            .basins
            .values()
            .filter_map(|b| Some((b.id, b.steady.as_ref()?.feeds.first()?.0)))
            .collect();
        let mut offsets: HashMap<u32, f64> = HashMap::default();
        if self.river_map.is_some() {
            for (id, c) in entries {
                let off = self.offset_at(c);
                if off != 0.0 {
                    offsets.insert(id, off);
                }
            }
        }
        let mut order: Vec<(f64, u32)> = self
            .basins
            .values()
            .filter_map(|b| b.steady.as_ref().map(|st| (st.level, b.id)))
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut by_from: HashMap<u32, Vec<(u32, f64)>> = HashMap::default();
        for (&(from, _), &(to, w)) in &self.links {
            if from != to {
                by_from.entry(from).or_default().push((to, w));
            }
        }
        let mut up: HashMap<u32, f64> = HashMap::default();
        for (_, id) in order {
            let owner = &self.owner;
            let b = self.basins.get_mut(&id).expect("basin");
            let law: f64 = b
                .natural
                .iter()
                .filter(|(c, _)| owner.get(*c) != Some(&id))
                .map(|(_, w)| w)
                .sum();
            let st = b.steady.as_mut().expect("found at rest");
            st.law = law;
            let feed = st.feed();
            let brought = up.get(&id).copied().unwrap_or(0.0);
            // Its steady flow (what the links bring by the map, for the bodies below it); what it
            // passes each flow step is what comes in then (`flow_step`).
            let through = feed.max(brought);
            let mut own = (feed - brought).max(0.0);
            // The head of the bodies found on its river: what a change upstream sends down it
            // through country nobody has found comes in with its own (W2d).
            if own > 0.0
                && let Some(&off) = offsets.get(&id)
            {
                own = (own + off).max(0.0);
            }
            b.inflows = if feed > 0.0 {
                st.feeds
                    .iter()
                    .map(|&(c, q)| (c, q * own / feed))
                    .filter(|(_, q)| *q > 0.0)
                    .collect()
            } else {
                Vec::new()
            };
            st.through = through;
            if law > 0.0
                && let Some(to) = by_from.get(&id)
            {
                for &(t, w) in to {
                    *up.entry(t).or_default() += through * w / law;
                }
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
                linked: 0.0,
                owed: 0.0,
                active: false,
                drawn_fill: (usize::MAX, 0),
                rest: HashSet::default(),
                held: HashSet::default(),
                natural: HashMap::default(),
                chunks: Vec::new(),
                edge: Vec::new(),
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
                .filter(|o| {
                    o.flow >= FLOW.stream_min_flow
                        && !o.is_seep()
                        && !b.natural.contains_key(&o.from)
                })
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
                world.set_voxel(c[0], c[1], c[2], M.air);
                writes.push((c, M.air));
            }
        }
        for &c in &new {
            if kind(Some(world.get_voxel_if_loaded(c))) == Kind::Open
                && world.find(chunk_of(c[0], c[1], c[2])).is_some()
            {
                let m = state_of(&c);
                world.set_voxel(c[0], c[1], c[2], m);
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
                world.set_voxel(c[0], c[1], c[2], m);
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
            self.links.retain(|(f, _), (t, _)| *f != id && *t != id);
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
                !under.contains(&d) && view.kind(d) == Kind::Solid
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
        for ((i, j), cond) in pairs {
            let a = surf(&parts[i]).min(surf(&parts[j]));
            if cond > 0.0 && a / cond < FLOW.merge_seconds && cond * MERGE_GAP >= throughput {
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
            // The map's flow comes in at one place: the part there keeps it (the largest, if none).
            if let Some(st) = &mut nb.steady {
                st.feeds.retain(|(c, _)| {
                    part.contains(c) || (n == 0 && !parts.iter().any(|p| p.contains(c)))
                });
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
            outlets.extend(b.seep());
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
    rest: &HashSet<[i32; 3]>,
) -> RowScan {
    let ours = |c: [i32; 3]| owner.get(&c) == Some(&id);
    // A reservoir's cells hold it like a wall: it fills toward the reservoir's level through the
    // outlets between them (drowned), and stays a basin.
    let walled = |c: [i32; 3]| owner.get(&c).is_some_and(|o| reservoirs.contains(o));
    let held = |c: [i32; 3]| {
        let d = up(f, c, -1);
        below.contains(&d) || view.kind(d) == Kind::Solid
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
            Kind::Open => !rest.contains(&s),
            Kind::Liquid(m) if m == liquid && (ours(s) || !owner.contains_key(&s)) => true,
            Kind::Liquid(_) if walled(s) => false,
            Kind::Liquid(_) => {
                met.push(s);
                false
            }
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
            if !seen.insert(n) {
                continue;
            }
            match view.kind(n) {
                Kind::Open if theirs(n) => met.push(n),
                Kind::Open if held(n) && !rest.contains(&n) => queue.push(n),
                Kind::Liquid(m) if m == liquid && ours(n) => queue.push(n),
                Kind::Liquid(_) if walled(n) => {}
                Kind::Liquid(m) if m == liquid => met.push(n),
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
                });
            }
        };
    for (i, row) in b.rows.iter().enumerate() {
        let lr = b.bottom + i as i32;
        let floor = m_of_row(lr);
        for &c in row {
            let d = up(f, c, -1);
            if !ours(d) && !b.rest.contains(&d) {
                if let Some(o) = theirs(d) {
                    if !b.held.contains(&d) {
                        add(&mut out, d, OutKind::Floor { z: floor }, false, Some(o));
                    }
                } else if view.kind(d) == Kind::Open {
                    add(&mut out, d, OutKind::Floor { z: floor }, false, None);
                }
            }
            for s in SIDES {
                let n = side(c, s);
                if ours(n) || b.rest.contains(&n) {
                    continue;
                }
                // Open above it (another cell to pour over), or a hole capped by a block.
                let hi = if view.kind(up(f, n, 1)) == Kind::Solid {
                    floor + CELL_H
                } else {
                    f64::INFINITY
                };
                if let Some(o) = theirs(n) {
                    if !b.held.contains(&n) {
                        let face = OutKind::Face {
                            lo: floor,
                            hi: floor + CELL_H,
                        };
                        add(&mut out, n, face, false, Some(o));
                    }
                    continue;
                }
                match view.kind(n) {
                    Kind::Open => {
                        let under = up(f, n, -1);
                        let held = ours(under) || view.kind(under) == Kind::Solid;
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
                    Kind::Liquid(_) => {}
                }
            }
        }
    }
    out
}

/// The cells beside a body as found (in `view`) that hold it at rest (`Basin::rest`): the open
/// cells beside or under it it didn't fill, but for natural outlets (an open cell onto air or
/// another body: a fall or the step to the next pool), and the leaking blocks beside it.
fn at_rest(view: &View<'_>, f: FaceFrame, cells: &HashSet<[i32; 3]>) -> HashSet<[i32; 3]> {
    let mut rest = HashSet::default();
    let falls = |c: [i32; 3]| match view.get(up(f, c, -1)) {
        Some(m) if m == M.air || is_running(m) => true,
        m => matches!(kind(m), Kind::Liquid(_)) && !cells.contains(&up(f, c, -1)),
    };
    for &c in cells {
        let mut near: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
        near.push(up(f, c, -1));
        for n in near {
            if cells.contains(&n) {
                continue;
            }
            match view.kind(n) {
                Kind::Open if !falls(n) => {
                    rest.insert(n);
                }
                Kind::Solid if view.get(n).is_some_and(|m| leak_of(m) > 0.0) => {
                    rest.insert(n);
                }
                _ => {}
            }
        }
    }
    rest
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
        world.set_voxel(c[0], c[1], c[2], to);
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

/// The cell of `cells` farthest (in steps through them) from those beside `from`: where a pool's
/// water comes in, as far as it can be from where it goes out. Ties: the least cell.
fn farthest_from(cells: &HashSet<[i32; 3]>, from: impl Iterator<Item = [i32; 3]>) -> [i32; 3] {
    let near = |c: [i32; 3]| {
        let mut n: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
        n.push([c[0], c[1] + 1, c[2]]);
        n.push([c[0], c[1] - 1, c[2]]);
        n
    };
    let mut dist: HashMap<[i32; 3], u32> = HashMap::default();
    let mut frontier: Vec<[i32; 3]> = Vec::new();
    for o in from {
        for n in near(o) {
            if cells.contains(&n) && !dist.contains_key(&n) {
                dist.insert(n, 0);
                frontier.push(n);
            }
        }
    }
    let mut d = 0;
    while !frontier.is_empty() {
        d += 1;
        let mut next = Vec::new();
        for c in frontier {
            for n in near(c) {
                if cells.contains(&n) && !dist.contains_key(&n) {
                    dist.insert(n, d);
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
    cells
        .iter()
        .copied()
        .max_by(|a, b| {
            let (da, db) = (
                dist.get(a).copied().unwrap_or(u32::MAX),
                dist.get(b).copied().unwrap_or(u32::MAX),
            );
            da.cmp(&db).then(b.cmp(a))
        })
        .expect("cells")
}

/// Where running liquid from an open cell ends.
enum End {
    /// On a body of liquid (this cell of it).
    Liquid([i32; 3]),
    /// On the floor with no way down within reach: it pools at this open cell.
    Pool([i32; 3]),
    Lost,
}

/// Follows liquid from the open cell `from`: down while open; on the floor, the nearest open cell
/// with nothing under it within `streamReach` (breadth first), and down again; with the open cells
/// it runs through on its way (from `from`: the cells it falls through and runs along the floor),
/// which are drawn running while it flows.
fn follow_path(
    view: &View<'_>,
    f: FaceFrame,
    from: [i32; 3],
    own: &dyn Fn([i32; 3]) -> bool,
) -> (End, Vec<[i32; 3]>) {
    // The source's own cells are walls to its water.
    let kind_of = |c: [i32; 3]| if own(c) { Kind::Solid } else { view.kind(c) };
    let mut path = vec![from];
    let mut c = from;
    for _ in 0..64 {
        // Down.
        let mut falls = 0;
        loop {
            let d = up(f, c, -1);
            match kind_of(d) {
                Kind::Open => {
                    c = d;
                    path.push(d);
                    falls += 1;
                    if falls > 2048 {
                        return (End::Lost, path);
                    }
                }
                Kind::Liquid(_) => return (End::Liquid(d), path),
                Kind::Solid => break,
            }
            if view.get(d).is_none() {
                return (End::Lost, path);
            }
        }
        if view.get(up(f, c, -1)).is_none() {
            return (End::Lost, path); // the floor is unloaded
        }
        // Along the floor, each cell's way back kept.
        let mut back: HashMap<[i32; 3], [i32; 3]> = HashMap::default();
        back.insert(c, c);
        let mut frontier = vec![c];
        let mut end = None;
        'search: for _ in 0..FLOW.stream_reach {
            let mut next = Vec::new();
            for &p in &frontier {
                for s in SIDES {
                    let n = side(p, s);
                    if back.contains_key(&n) {
                        continue;
                    }
                    back.insert(n, p);
                    match kind_of(n) {
                        Kind::Open => {
                            if view.get(n).is_none() {
                                continue;
                            }
                            match kind_of(up(f, n, -1)) {
                                Kind::Open | Kind::Liquid(_) => {
                                    end = Some((n, false));
                                    break 'search;
                                }
                                Kind::Solid => next.push(n),
                            }
                        }
                        Kind::Liquid(_) => {
                            end = Some((n, true));
                            break 'search;
                        }
                        Kind::Solid => {}
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let Some((n, liquid)) = end else {
            return (End::Pool(c), path);
        };
        // The run along the floor, from where it landed.
        let mut run = Vec::new();
        let mut at = back[&n];
        while at != c {
            run.push(at);
            at = back[&at];
        }
        run.reverse();
        path.extend(run);
        if liquid {
            return (End::Liquid(n), path);
        }
        path.push(n);
        c = n;
    }
    (End::Lost, path)
}

mod rivers;
mod save;

pub use rivers::{RiverNext, Rivers};

#[cfg(test)]
mod tests;
