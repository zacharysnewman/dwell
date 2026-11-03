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
//! W1's stand-in for runoff (W2): a body of water is found when an edit first touches it, and its
//! inflow is whatever left it just before the edit (its outlets in the world as it was), so a
//! river dammed fills at its old flow, and a lake with no outlet has none.
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
/// Outlets whose way is found again a water tick at most.
const ROUTES_PER_TICK: usize = 64;
/// Two basins that meet merge when their levels are this close (m).
const MERGE_GAP: f64 = 0.02;

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

/// What a cell is to water.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Holds water back (also an unloaded cell).
    Solid,
    /// Air, or a block water replaces (a plant).
    Open,
    Liquid(StateId),
}

fn kind(m: Option<StateId>) -> Kind {
    let Some(m) = m else {
        return Kind::Solid;
    };
    if m == M.air {
        return Kind::Open;
    }
    let s = info(m);
    if s.liquid {
        Kind::Liquid(m)
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
    /// Over a sill (local m): `weir × width × head^1.5`.
    Weir { sill: f64 },
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
}

impl Outlet {
    /// Its flow (m³/s) at level `level` (local m), for a liquid of weir coefficient `weir`, into
    /// water standing at `below` (local m; `-∞` for a free fall): drowned, the head is the
    /// difference of the two levels, so two bodies joined by it even out.
    fn rate(&self, level: f64, weir: f64, below: f64) -> f64 {
        match self.kind {
            OutKind::Weir { sill } => weir * (level - sill.max(below)).max(0.0).powf(1.5),
            OutKind::Leak { k, centre } => k * CELL_H * (level - centre.max(below)).max(0.0),
        }
    }
    fn floor(&self) -> f64 {
        match self.kind {
            OutKind::Weir { sill } => sill,
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
    /// The outlets (source basin, its cell) whose flow has been taken off this basin's inflows:
    /// found as it was, its inflow already held what they send it.
    credited: HashSet<(u32, [i32; 3])>,
    /// Basins it met at its rows and didn't merge with (their level above its own): merged when
    /// the levels meet.
    neighbours: Vec<u32>,
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
}

impl Basin {
    /// Its inflow (m³/s, real).
    pub fn inflow(&self) -> f64 {
        self.inflows.iter().map(|(_, q)| q).sum()
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

    /// The cells drawn as liquid at this volume: the full rows, and the surface's row once half
    /// full.
    fn wet_now(&self) -> HashSet<[i32; 3]> {
        let (k, frac) = self.fill();
        let mut out = HashSet::default();
        for (i, r) in self.rows.iter().enumerate() {
            if i < k || (i == k && frac >= 0.5) {
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
}

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
        }
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
    /// The liquid held in all basins but reservoirs (m³).
    pub fn stored(&self) -> f64 {
        self.basins
            .values()
            .filter(|b| !b.reservoir)
            .map(|b| b.volume)
            .sum()
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
        self.apply_touched(world, &mut writes);
        self.route(world, &mut writes);
        // Flows from the levels at the start of the tick.
        let live: HashSet<u32> = self
            .basins
            .values()
            .filter(|b| b.near(players, FLOW.live_radius_m))
            .map(|b| b.id)
            .collect();
        let mut arriving: HashMap<u32, f64> = HashMap::default();
        let ids: Vec<u32> = self.basins.keys().copied().collect();
        // The levels outlets run into (the same face: a basin's local metres).
        let levels: HashMap<u32, f64> = self.basins.values().map(|b| (b.id, b.level())).collect();
        for &id in &ids {
            if !live.contains(&id) {
                continue;
            }
            let b = self.basins.get_mut(&id).expect("basin");
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
            if !b.reservoir {
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
            } else {
                b.volume -= total * dt;
                b.volume += b.inflow() * dt;
                self.brought += b.inflow() * dt;
            }
            for o in &b.outlets {
                match o.target {
                    Target::Basin(t) => *arriving.entry(t).or_default() += o.flow,
                    Target::Lost => self.lost += o.flow * dt,
                    Target::Unresolved => {} // waits in the basin: nothing left
                }
            }
            // An unresolved way keeps its water.
            let held: f64 = b
                .outlets
                .iter()
                .filter(|o| o.target == Target::Unresolved)
                .map(|o| o.flow)
                .sum();
            if !b.reservoir {
                b.volume += held * dt;
            } else {
                self.brought -= held * dt;
            }
        }
        for b in self.basins.values_mut() {
            b.arriving = 0.0;
        }
        for (t, q) in arriving {
            match self.basins.get_mut(&t) {
                Some(b) if b.reservoir => self.lost += q * dt,
                Some(b) => {
                    b.volume += q * dt;
                    b.arriving = q;
                }
                None => self.lost += q * dt,
            }
        }
        let mut all: Vec<u32> = self.basins.keys().copied().collect();
        all.sort_unstable();
        for id in all {
            if self.basins.contains_key(&id) {
                self.settle_rows(world, id, &mut writes);
            }
        }
        writes
    }

    /// Finds the bodies edits touched (before the edit, for their inflow) and finds again the
    /// containers and outlets of those they changed.
    fn apply_touched(&mut self, world: &mut VoxelWorld, writes: &mut Vec<([i32; 3], StateId)>) {
        if self.touched.is_empty() {
            return;
        }
        let touched = std::mem::take(&mut self.touched);
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
        for c in near {
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
                if let Some(id) = self.discover(world, Some(&old), c) {
                    dirty.insert(id);
                }
            } else if let Kind::Liquid(m) = now.kind(c)
                && weir_of(m).is_some()
                && let Some(id) = self.discover(world, None, c)
            {
                if let Some(b) = self.basins.get_mut(&id)
                    && !b.reservoir
                {
                    b.inflows.clear();
                }
                dirty.insert(id);
            }
        }
        for id in dirty {
            self.rebuild(world, id, writes);
        }
    }

    /// Finds the body of liquid at `seed` (in the world as `old` had it): its cells, its volume,
    /// and its inflow (what its outlets passed then). A body larger than `maxBasinCells` is a
    /// reservoir at its level.
    fn discover(
        &mut self,
        world: &VoxelWorld,
        old: Option<&HashMap<[i32; 3], StateId>>,
        seed: [i32; 3],
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
                    credited: HashSet::default(),
                    neighbours: Vec::new(),
                    last_volume: 0.0,
                    outlets: Vec::new(),
                    wet: HashSet::default(),
                    capped: false,
                    fixed_level: m_of_row(surface + 1),
                    lo,
                    hi,
                    arriving: 0.0,
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
        let volume = seen.len() as f64 * CELL_H;
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
            credited: HashSet::default(),
            neighbours: Vec::new(),
            last_volume: volume,
            outlets: Vec::new(),
            wet: seen.iter().copied().collect(),
            capped: false,
            fixed_level: 0.0,
            lo: seed,
            hi: seed,
            arriving: 0.0,
        };
        // Its inflow: what left it as it was (steady then), coming in at its cell farthest from its
        // ways out (a river pool's upstream end).
        let outlets = scan_outlets(&view, &b, &self.owner, id);
        let level = b.level();
        let rate: f64 = outlets
            .iter()
            .map(|o| o.rate(level, weir, f64::NEG_INFINITY))
            .sum();
        if rate > 0.0 {
            let entry = farthest_from(&seen, outlets.iter().map(|o| o.from));
            b.inflows.push((entry, rate));
        }
        for c in b.cells() {
            self.owner.insert(c, id);
        }
        self.basins.insert(id, b);
        Some(id)
    }

    /// Finds a basin's container again from its bottom row up (in the world now), as many rows
    /// as its volume fills, merging what it meets, and its outlets; writes what changed.
    fn rebuild(&mut self, world: &mut VoxelWorld, id: u32, writes: &mut Vec<([i32; 3], StateId)>) {
        for _ in 0..8 {
            let Some(b) = self.basins.get(&id) else {
                return;
            };
            if b.reservoir {
                self.reservoir_outlets(world, id);
                return;
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
            // What it met at its rows: merged once the levels meet (until then the other's
            // liquid is a wall to it, and the other falls into it), then found again.
            let mut merged = false;
            let mut neighbours: Vec<u32> = Vec::new();
            let mine = self.basins[&id].level();
            let receding = self.basins[&id].volume < self.basins[&id].last_volume;
            for c in met {
                let other = match self.owner.get(&c) {
                    Some(&o) if o != id => Some(o),
                    Some(_) => None,
                    None => self.discover(world, None, c),
                };
                if let Some(o) = other
                    && o != id
                    && let Some(ob) = self.basins.get(&o)
                    && !ob.reservoir
                {
                    if mine >= ob.level() - MERGE_GAP && !receding {
                        self.merge(id, o);
                        merged = true;
                    } else if !neighbours.contains(&o) {
                        neighbours.push(o);
                    }
                }
            }
            if merged {
                continue;
            }
            self.basins.get_mut(&id).expect("basin").neighbours = neighbours;
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
            let cells: Vec<[i32; 3]> = b.cells().collect();
            for c in cells {
                self.owner.insert(c, id);
            }
            let view = View { world, old: None };
            let b = &self.basins[&id];
            let outlets = scan_outlets(&view, b, &self.owner, id);
            self.basins.get_mut(&id).expect("basin").outlets = outlets;
            self.split_if_apart(world, id, writes);
            return;
        }
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
                        kind: OutKind::Weir { sill },
                        target: Target::Unresolved,
                        flow: 0.0,
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
        me.credited.extend(other.credited);
        me.neighbours.retain(|&n| n != b);
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
        let mut todo: Vec<(u32, usize)> = Vec::new();
        for b in self.basins.values() {
            for (i, o) in b.outlets.iter().enumerate() {
                if o.target == Target::Unresolved {
                    todo.push((b.id, i));
                }
            }
        }
        todo.sort_unstable();
        todo.truncate(ROUTES_PER_TICK);
        for (id, i) in todo {
            let Some(b) = self.basins.get(&id) else {
                continue;
            };
            let (f, liquid, from) = (b.face, b.liquid, b.outlets[i].from);
            let end = {
                let view = View { world, old: None };
                let owner = &self.owner;
                follow(&view, f, from, &|c| owner.get(&c) == Some(&id))
            };
            let target = match end {
                End::Lost => Target::Lost,
                End::Liquid(c) => match self.owner.get(&c) {
                    Some(&o) if o != id => Target::Basin(o),
                    Some(_) => Target::Lost, // unreachable: its own cells are walls to it
                    None => self
                        .discover(world, None, c)
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
                self.credit(id, i, t);
            }
        }
    }

    /// Outlet `i` of basin `id` runs into basin `t`: the first time, its flow comes off `t`'s
    /// inflows (nearest where it lands first), which held it already (`t` was found steady, its
    /// inflow what left it, this flow included).
    fn credit(&mut self, id: u32, i: usize, t: u32) {
        if t == id {
            return;
        }
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        let Some(o) = b.outlets.get(i).copied() else {
            return;
        };
        let below = self.basins.get(&t).map_or(f64::NEG_INFINITY, Basin::level);
        let mut q = o.rate(b.level(), b.weir, below);
        let Some(tb) = self.basins.get_mut(&t) else {
            return;
        };
        if tb.reservoir || !tb.credited.insert((id, o.from)) {
            return;
        }
        let d2 = |c: [i32; 3]| -> i64 { (0..3).map(|a| i64::from(c[a] - o.from[a]).pow(2)).sum() };
        tb.inflows.sort_by_key(|(c, _)| d2(*c));
        for (_, rate) in &mut tb.inflows {
            let take = q.min(*rate);
            *rate -= take;
            q -= take;
            if q <= 0.0 {
                break;
            }
        }
        tb.inflows.retain(|(_, r)| *r > 1e-12);
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
                credited: HashSet::default(),
                neighbours: Vec::new(),
                last_volume: 0.0,
                outlets: Vec::new(),
                wet: HashSet::default(),
                capped: false,
                fixed_level: 0.0,
                lo: c,
                hi: c,
                arriving: 0.0,
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
        if b.reservoir {
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
        self.meet_neighbours(world, id, writes);
        self.spin_off_shelves(world, id, writes);
        let Some(b) = self.basins.get_mut(&id) else {
            return;
        };
        if b.volume <= 1e-6 && b.inflow() <= 0.0 && b.arriving <= 0.0 {
            b.volume = 0.0;
        }
        b.last_volume = b.volume;
        let want = b.wet_now();
        let liquid = b.liquid;
        let gone: Vec<[i32; 3]> = b.wet.difference(&want).copied().collect();
        let new: Vec<[i32; 3]> = want.difference(&b.wet).copied().collect();
        for c in gone {
            if world.find(chunk_of(c[0], c[1], c[2])).is_some()
                && world.get_voxel_if_loaded(c) == liquid
            {
                world.set_voxel(c[0], c[1], c[2], M.air);
                writes.push((c, M.air));
            }
        }
        for c in new {
            if kind(Some(world.get_voxel_if_loaded(c))) == Kind::Open
                && world.find(chunk_of(c[0], c[1], c[2])).is_some()
            {
                world.set_voxel(c[0], c[1], c[2], liquid);
                writes.push((c, liquid));
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

    /// A rising basin whose level has reached a basin it met (a higher pool it was filling
    /// toward) merges with it.
    fn meet_neighbours(
        &mut self,
        world: &mut VoxelWorld,
        id: u32,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let Some(b) = self.basins.get(&id) else {
            return;
        };
        if b.volume < b.last_volume || b.neighbours.is_empty() {
            return;
        }
        let level = b.level();
        let meet: Vec<u32> = b
            .neighbours
            .iter()
            .copied()
            .filter(|n| {
                self.basins
                    .get(n)
                    .is_some_and(|o| !o.reservoir && level >= o.level() - MERGE_GAP)
            })
            .collect();
        if meet.is_empty() {
            return;
        }
        for n in meet {
            if self.basins.contains_key(&n) {
                self.merge(id, n);
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
            nb.neighbours = Vec::new();
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
        let mut parts: Vec<HashSet<[i32; 3]>> = Vec::new();
        let mut seen: HashSet<[i32; 3]> = HashSet::default();
        for &start in &cells {
            if seen.contains(&start) {
                continue;
            }
            let mut part = HashSet::default();
            let mut q = vec![start];
            seen.insert(start);
            while let Some(c) = q.pop() {
                part.insert(c);
                let mut ns: Vec<[i32; 3]> = SIDES.iter().map(|&s| side(c, s)).collect();
                ns.push([c[0], c[1] + 1, c[2]]);
                ns.push([c[0], c[1] - 1, c[2]]);
                for n in ns {
                    if cells.contains(&n) && seen.insert(n) {
                        q.push(n);
                    }
                }
            }
            parts.push(part);
        }
        if parts.len() < 2 {
            return;
        }
        parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
        let (k, frac) = b.fill();
        let f = b.face;
        let bottom = b.bottom;
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
            for &c in part {
                self.owner.insert(c, pid);
            }
            self.basins.insert(pid, nb);
            made.push(pid);
        }
        self.retarget_all(id);
        for pid in made {
            let view = View { world, old: None };
            let outlets = scan_outlets(&view, &self.basins[&pid], &self.owner, pid);
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
        below.contains(&d) || view.kind(d) == Kind::Solid
    };
    let mut cells = Vec::new();
    let mut met = Vec::new();
    let mut seen: HashSet<[i32; 3]> = HashSet::default();
    let mut queue: Vec<[i32; 3]> = Vec::new();
    for &s in seeds {
        if f.row(s[1]) != lr || !seen.insert(s) {
            continue;
        }
        let open = match view.kind(s) {
            Kind::Open => true,
            Kind::Liquid(m) if m == liquid && (ours(s) || !owner.contains_key(&s)) => true,
            Kind::Liquid(_) if walled(s) => false,
            Kind::Liquid(_) => {
                met.push(s);
                false
            }
            Kind::Solid => false,
        };
        if open && (bottom || held(s)) {
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
                Kind::Open if held(n) => queue.push(n),
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

/// A basin's ways out (in `view`): open cells beside its cells with nothing under them (over a
/// sill at that row's floor), open cells under its cells (a hole in its floor), and leaking blocks
/// beside it with an open cell past them.
fn scan_outlets(
    view: &View<'_>,
    b: &Basin,
    owner: &HashMap<[i32; 3], u32>,
    id: u32,
) -> Vec<Outlet> {
    let f = b.face;
    let cells: HashSet<[i32; 3]> = b.cells().collect();
    let ours = |c: [i32; 3]| cells.contains(&c) || owner.get(&c) == Some(&id);
    let mut out: Vec<Outlet> = Vec::new();
    let mut at: HashSet<([i32; 3], bool)> = HashSet::default();
    let mut add = |out: &mut Vec<Outlet>, from: [i32; 3], kind: OutKind, leak: bool| {
        if at.insert((from, leak)) {
            out.push(Outlet {
                from,
                kind,
                target: Target::Unresolved,
                flow: 0.0,
            });
        }
    };
    for (i, row) in b.rows.iter().enumerate() {
        let lr = b.bottom + i as i32;
        let floor = m_of_row(lr);
        for &c in row {
            let d = up(f, c, -1);
            if !ours(d) && view.kind(d) == Kind::Open {
                add(&mut out, d, OutKind::Weir { sill: floor }, false);
            }
            for s in SIDES {
                let n = side(c, s);
                if ours(n) {
                    continue;
                }
                match view.kind(n) {
                    Kind::Open => {
                        let under = up(f, n, -1);
                        let held = ours(under) || view.kind(under) == Kind::Solid;
                        if !held {
                            add(&mut out, n, OutKind::Weir { sill: floor }, false);
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
/// with nothing under it within `streamReach` (breadth first), and down again.
fn follow(view: &View<'_>, f: FaceFrame, from: [i32; 3], own: &dyn Fn([i32; 3]) -> bool) -> End {
    // The source's own cells are walls to its water.
    let kind_of = |c: [i32; 3]| if own(c) { Kind::Solid } else { view.kind(c) };
    let mut c = from;
    for _ in 0..64 {
        // Down.
        let mut falls = 0;
        loop {
            let d = up(f, c, -1);
            match kind_of(d) {
                Kind::Open => {
                    c = d;
                    falls += 1;
                    if falls > 2048 {
                        return End::Lost;
                    }
                }
                Kind::Liquid(_) => return End::Liquid(d),
                Kind::Solid => break,
            }
            if view.get(d).is_none() {
                return End::Lost;
            }
        }
        if view.get(up(f, c, -1)).is_none() {
            return End::Lost; // the floor is unloaded
        }
        // Along the floor.
        let mut seen: HashSet<[i32; 3]> = HashSet::default();
        seen.insert(c);
        let mut frontier = vec![c];
        let mut drop = None;
        'search: for _ in 0..FLOW.stream_reach {
            let mut next = Vec::new();
            for &p in &frontier {
                for s in SIDES {
                    let n = side(p, s);
                    if !seen.insert(n) {
                        continue;
                    }
                    match kind_of(n) {
                        Kind::Open => {
                            if view.get(n).is_none() {
                                continue;
                            }
                            match kind_of(up(f, n, -1)) {
                                Kind::Open | Kind::Liquid(_) => {
                                    drop = Some(n);
                                    break 'search;
                                }
                                Kind::Solid => next.push(n),
                            }
                        }
                        Kind::Liquid(_) => return End::Liquid(n),
                        Kind::Solid => {}
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        match drop {
            Some(d) => c = d,
            None => return End::Pool(c),
        }
    }
    End::Lost
}

#[cfg(test)]
mod tests;
