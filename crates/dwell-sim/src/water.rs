//! Water as a surface grid (Phase 53 G2, [ADR 0060](../../../docs/adr/0060-water-surface-grid.md),
//! [`WATER_SURFACE_GRID.md`](../../../docs/WATER_SURFACE_GRID.md)): a depth on each 1 m ground
//! column, moving to its four neighbours across their shared faces by the local-inertial
//! shallow-water law (θ-weighting, friction from the flow's whole speed, no faster than critical,
//! a column never sending more than it holds), stepped in water chunks of 16 × 16 columns.
//!
//! - **The world as generated is at rest:** a generated river's or lake's column joins the grid
//!   *held* at the level it was found at (it feeds and drains what's free beside it, as the river
//!   beyond would), costing nothing. An edit frees the water around it; a free column whose level
//!   differs from its rest by more than `releaseM` frees that neighbour too, so a dam's
//!   backwater spreads upstream and its drop downstream as far as they reach.
//! - **Sleep:** a chunk whose free levels held within the sleep band for its calm time sleeps and isn't
//!   stepped; what its awake neighbours send it is kept exactly (`owed`), and it wakes when that
//!   would move its wet columns half the wake band, or when a shared face jumps (an edit, a flood
//!   front). Waking, it takes what it owes over `releaseS`, in proportion to its columns' depth.
//! - **Blocks:** a free column's water is written as water blocks (full cells, its top cell in
//!   eighths) when its level crosses a step, as clients draw water today (G3 gives water its own
//!   layer and mesh).
//! - **Conserved** among free columns exactly; held columns and the loaded world's edge are where
//!   water enters and leaves.
//!
//! - **Data:** its settings and liquids from `shared/fluids/flow.json` (`Settings`, G4).
//!
//! Face-local throughout: a column is (face, x, z) with heights in its face's frame
//! (`FaceFrame::row`), so face B is face A's mirror.

use std::sync::Arc;

use dwell_blocks::StateId;
use dwell_world::currents::ChunkCurrents;
use dwell_world::hash::{HashMap, HashSet};
use dwell_world::{CELL_H, ChunkCoord, FaceFrame, M, VoxelWorld, chunk_of, m_of_row, row_of_m};

pub mod cells;
pub mod host;

use cells::{Cells, ChunkCells, Kind, Mirror};

pub use dwell_world::flow::{FLOW, FlowData, LiquidFlow};

/// The world map's flow through a face-local column (m³/s): kept for sources (rain, springs) to
/// come; the grid reads the world's generated water itself.
pub type Runoff = Arc<dyn Fn(FaceFrame, i32, i32) -> f64 + Send + Sync>;
/// An untouched river's current (m/s along x and z) at face `f`'s column (x, z) whose water is
/// `depth` m deep, by the world map (Phase 53 G3b; `TerrainGenerator::river_current`).
pub type RiverCurrent = Arc<dyn Fn(FaceFrame, i32, i32, f64) -> Option<[f64; 2]> + Send + Sync>;

/// Columns a water chunk spans each way.
pub const CS: i32 = 16;
const CA: usize = (CS * CS) as usize;
const NONE: u32 = u32::MAX;

const G: f32 = 9.81;
const DRY: f32 = 1e-4;

/// The grid's settings, from its data (`shared/fluids/flow.json`, `dwell_world::flow::FlowData`;
/// what each means is in the file's `$comment`).
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub manning: f32,
    pub theta: f32,
    pub cfl: f32,
    pub max_substeps: usize,
    /// The most column-substeps a tick may take (at least one substep): past it the rest of the
    /// tick's water time is lost (docs/WATER_SURFACE_GRID.md §2b).
    pub step_budget: usize,
    pub sleep_band: f32,
    pub wake_band: f32,
    pub calm_s: f64,
    pub release_s: f64,
    pub jump: f32,
    pub release: f32,
    pub edit_reach: i32,
    /// How far a level moves past a drawn step before its cells are written again (m).
    pub hyst: f32,
    /// Slower water than this (m/s) has no current, nor water shallower than `current_depth` (m)
    /// (docs/WATER_SURFACE_GRID.md §2c).
    pub still: f32,
    pub current_depth: f32,
    /// The liquid it draws (the data's first).
    pub liquid: StateId,
}

impl Settings {
    /// The settings of flow data `d`.
    pub fn of(d: &FlowData) -> Self {
        let first = d
            .liquids
            .first()
            .expect("flow data lists a liquid (validated)");
        Settings {
            manning: first.manning as f32,
            theta: d.theta as f32,
            cfl: d.cfl as f32,
            max_substeps: d.max_substeps as usize,
            step_budget: d.step_budget as usize,
            sleep_band: d.sleep_band_m as f32,
            wake_band: d.wake_band_m as f32,
            calm_s: d.calm_s,
            release_s: d.release_s.max(1e-6),
            jump: d.jump_m2s as f32,
            release: d.release_m as f32,
            edit_reach: d.edit_reach as i32,
            hyst: d.fill_hysteresis as f32 * STEP,
            still: d.current_still_ms as f32,
            current_depth: d.current_depth_m as f32,
            liquid: dwell_blocks::require(first.block),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::of(&FLOW)
    }
}

/// A cell's water is drawn in eighths.
const STEP: f32 = CELL_H as f32 / 8.0;
/// Rows a column's floor is looked for at most, either way.
const SCAN_ROWS: i32 = 96;

/// A cell's kind, or none where the world isn't loaded (the grid's copy, `cells.rs`).
fn cell_kind(world: &Cells, c: [i32; 3]) -> Option<Kind> {
    world.kind(c)
}

/// The world cell of face `f`'s column (x, z) at local row `r`.
fn cell(f: FaceFrame, x: i32, r: i32, z: i32) -> [i32; 3] {
    [x, f.row(r), z]
}

/// The floor under face `f`'s column (x, z) for water at local height `at` (m): the top of the
/// solid run that height meets, or, where it meets open cells, the top of the first solid below.
/// None where the world there isn't loaded or no floor is near.
fn floor_at(world: &Cells, f: FaceFrame, x: i32, z: i32, at: f64) -> Option<f32> {
    let r0 = row_of_m(at);
    match cell_kind(world, cell(f, x, r0, z))? {
        Kind::Solid => {
            let mut r = r0;
            while r < r0 + SCAN_ROWS && cell_kind(world, cell(f, x, r + 1, z))? == Kind::Solid {
                r += 1;
            }
            Some(m_of_row(r + 1) as f32)
        }
        _ => {
            let mut r = r0;
            while r > r0 - SCAN_ROWS {
                if cell_kind(world, cell(f, x, r - 1, z))? == Kind::Solid {
                    return Some(m_of_row(r) as f32);
                }
                r -= 1;
            }
            None
        }
    }
}

/// The face law across a face from column a to b: the next discharge (m²/s, + from a to b).
#[allow(clippy::too_many_arguments)]
#[inline]
fn face(
    s: &Settings,
    ga: f32,
    da: f32,
    gb: f32,
    db: f32,
    q: f32,
    qa: f32,
    qb: f32,
    qp: f32,
    dt: f32,
) -> f32 {
    let (ea, eb) = (ga + da, gb + db);
    let h = ea.max(eb) - ga.max(gb);
    if h <= DRY || !(h.is_finite()) {
        return 0.0;
    }
    let speed = (q * q + qp * qp).sqrt();
    let friction = 1.0 + G * dt * s.manning * s.manning * speed / (h * h * h.cbrt());
    let qt = s.theta * q + (1.0 - s.theta) * 0.5 * (qa + qb);
    let nq = (qt - G * h * dt * (eb - ea)) / friction;
    let limit = h * (G * h).sqrt();
    nq.clamp(-limit, limit)
}

/// A chunk's water as it is, or as generated.
fn lay(k: &Chunk, rest: bool) -> &Layer {
    if rest { &k.r } else { &k.w }
}

/// One state of a chunk's water: floors, depths and face flows.
#[derive(Clone)]
struct Layer {
    /// The column's floor (face-local m); NaN: no column (a wall to water).
    floor: [f32; CA],
    depth: [f32; CA],
    /// Discharge across each column's +x and +z face (m²/s), and this substep's.
    qx: [f32; CA],
    qz: [f32; CA],
    nqx: [f32; CA],
    nqz: [f32; CA],
    scale: [f32; CA],
}

impl Layer {
    fn new() -> Self {
        Layer {
            floor: [f32::NAN; CA],
            depth: [0.0; CA],
            qx: [0.0; CA],
            qz: [0.0; CA],
            nqx: [0.0; CA],
            nqz: [0.0; CA],
            scale: [1.0; CA],
        }
    }
}

#[derive(Clone)]
struct Chunk {
    f: FaceFrame,
    cx: i32,
    cz: i32,
    /// E (+x), W, S (+z), N.
    nbr: [u32; 4],
    /// The water as it is.
    w: Layer,
    /// The water as generated, stepped beside it with its depths fixed: a column changes by how
    /// far its flow differs from this one's, so the world as generated is exactly at rest under
    /// the law, wherever it was drawn off a steady flow (a river's steps), and a dam, a cut or a
    /// flood still moves the water (they change its flow, not the rest's). A floor NaN here: a wall
    /// at rest (a column an edit opened).
    r: Layer,
    /// Held at its level (generated water at rest, or the loaded world's edge).
    held: [bool; CA],
    /// A held column's current by the world map's river (G3b; m/s along x and z).
    river: [[f32; 2]; CA],
    /// The level last written as blocks (NaN: none, or to write).
    drawn: [f32; CA],
    /// The calm window's depths and start.
    win: [f32; CA],
    calm_t: f64,
    awake: bool,
    /// Water owed while asleep, then still to come in after waking (m³), and how fast.
    owed: f64,
    rel_rate: f64,
    /// Faces computed this substep (this chunk's own, or its edges toward awake neighbours).
    active: u8,
}

impl Chunk {
    fn new(f: FaceFrame, cx: i32, cz: i32) -> Box<Self> {
        Box::new(Chunk {
            f,
            cx,
            cz,
            nbr: [NONE; 4],
            w: Layer::new(),
            r: Layer::new(),
            held: [false; CA],
            river: [[0.0; 2]; CA],
            drawn: [f32::NAN; CA],
            win: [0.0; CA],
            calm_t: -1.0,
            awake: false,
            owed: 0.0,
            rel_rate: 0.0,
            active: 0,
        })
    }
    fn xz(&self, l: usize) -> (i32, i32) {
        (
            self.cx * CS + (l as i32 % CS),
            self.cz * CS + (l as i32 / CS),
        )
    }
    fn wet(&self) -> usize {
        (0..CA)
            .filter(|&l| self.w.depth[l] > DRY && !self.w.floor[l].is_nan())
            .count()
    }
}

/// A water cell written: its new state, and the byte (`cells::byte_of`) of what it replaced; or,
/// with `flood`, a block holding water in its open part (a ramp) set flooded or dry, its state
/// found from the world's (`flooded` by name; `state` unused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Write {
    pub cell: [i32; 3],
    pub state: StateId,
    pub was: u8,
    pub flood: Option<bool>,
}

impl Write {
    /// The state this write leaves in a world whose cell is `now`, or None if it no longer applies
    /// (overtaken: the cell made solid, or no longer a block holding water).
    pub fn apply_to(&self, now: StateId) -> Option<StateId> {
        let b = cells::byte_of(now);
        match self.flood {
            None => (cells::kind_of(b) != Kind::Solid).then_some(self.state),
            Some(wet) => {
                cells::holds(b)?;
                dwell_blocks::with_property(now, "flooded", if wet { "true" } else { "false" })
            }
        }
    }
}

/// A water chunk's key in `VoxelWorld::currents`: (face B, chunk x, chunk z).
pub type CurrentsKey = (bool, i32, i32);

/// What one [`Water::step`] did.
#[derive(Debug, Default)]
pub struct Step {
    /// The water time run (s): all it was given, unless the budget stopped it short.
    pub ran: f64,
    pub writes: Vec<Write>,
    /// The awake chunks' currents (and those of chunks whose held water joined).
    pub currents: Vec<(CurrentsKey, Option<ChunkCurrents>)>,
}

/// The water of a world (both faces).
#[derive(Default)]
pub struct Water {
    /// Its settings (`Settings::default()`: the data's).
    cfg: Settings,
    chunks: Vec<Box<Chunk>>,
    index: HashMap<(FaceFrame, i32, i32), u32>,
    /// Cells edits changed since the last tick.
    touched: Vec<[i32; 3]>,
    runoff: Option<Runoff>,
    /// Held water's currents by the world map's rivers (G3b); none: held water is still.
    rivers: Option<RiverCurrent>,
    /// Chunks whose held water joined since their currents were last published.
    fresh: HashSet<usize>,
    /// Water time (s).
    t: f64,
    /// The grid's copy of the world it reads (ADR 0062, `cells.rs`).
    cells: Cells,
    /// Chunks loaded since the last tick, whose water is still to read.
    loaded: Vec<ChunkCoord>,
    /// What [`Water::tick`] told the copy of a world it reads itself.
    mirror: Mirror,
    /// Counters (diagnostics, tests).
    pub substeps: u64,
    pub wakes: u64,
    pub released: u64,
}

impl std::fmt::Debug for Water {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Water")
            .field("chunks", &self.chunks.len())
            .field("t", &self.t)
            .finish()
    }
}

impl Water {
    pub fn new() -> Self {
        Self::default()
    }

    /// Water with settings other than the data's (a fixture, a tool).
    pub fn with_settings(cfg: Settings) -> Self {
        Self {
            cfg,
            ..Self::default()
        }
    }

    /// The water of a world of generator `generator_version` and `world_seed`: with the world
    /// map's flow and its untouched rivers' currents (G3b) where it has one, plain otherwise (a
    /// flat world, a fixture's generator: `map` false).
    pub fn for_world(generator_version: u32, world_seed: u64, map: bool) -> Self {
        let terrain = map
            .then(|| dwell_worldgen::BifacialTerrain::for_generator(generator_version, world_seed))
            .flatten();
        let Some(terrain) = terrain else {
            return Self::new();
        };
        let map = Arc::new(terrain);
        let rivers = map.clone();
        Self::with_runoff(Arc::new(move |f: FaceFrame, x, z| {
            map.local(f.face).discharge_at(x, z)
        }))
        // Untouched rivers' currents (Phase 53 G3b).
        .with_rivers(Arc::new(move |f: FaceFrame, x, z, depth| {
            rivers.local(f.face).river_current(x, z, depth)
        }))
    }

    /// Water with the world map's flow (kept for sources to come).
    pub fn with_runoff(runoff: Runoff) -> Self {
        Self {
            runoff: Some(runoff),
            ..Self::default()
        }
    }

    /// Held water runs as the world map's rivers say (G3b): its currents, for swimmers and bodies.
    pub fn with_rivers(mut self, rivers: RiverCurrent) -> Self {
        self.rivers = Some(rivers);
        self
    }

    /// The world map's flow through face-local column (x, z), if known (m³/s).
    pub fn runoff_at(&self, f: FaceFrame, x: i32, z: i32) -> Option<f64> {
        self.runoff.as_ref().map(|r| r(f, x, z))
    }

    fn chunk_index(&mut self, f: FaceFrame, cx: i32, cz: i32) -> usize {
        if let Some(&i) = self.index.get(&(f, cx, cz)) {
            return i as usize;
        }
        let i = self.chunks.len();
        let mut c = Chunk::new(f, cx, cz);
        for (k, (dx, dz)) in [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().enumerate() {
            if let Some(&n) = self.index.get(&(f, cx + dx, cz + dz)) {
                c.nbr[k] = n;
                self.chunks[n as usize].nbr[k ^ 1] = i as u32;
            }
        }
        self.chunks.push(c);
        self.index.insert((f, cx, cz), i as u32);
        i
    }

    fn find(&self, f: FaceFrame, x: i32, z: i32) -> Option<(usize, usize)> {
        let &c = self.index.get(&(f, x.div_euclid(CS), z.div_euclid(CS)))?;
        Some((
            c as usize,
            (z.rem_euclid(CS) * CS + x.rem_euclid(CS)) as usize,
        ))
    }

    fn column(&mut self, f: FaceFrame, x: i32, z: i32) -> (usize, usize) {
        let c = self.chunk_index(f, x.div_euclid(CS), z.div_euclid(CS));
        (c, (z.rem_euclid(CS) * CS + x.rem_euclid(CS)) as usize)
    }

    /// The water level at face-local column (x, z), if it holds water (m, face-local).
    pub fn level_at(&self, f: FaceFrame, x: i32, z: i32) -> Option<f64> {
        let (c, l) = self.find(f, x, z)?;
        let k = &self.chunks[c];
        (!k.w.floor[l].is_nan() && k.w.depth[l] > DRY)
            .then(|| f64::from(k.w.floor[l] + k.w.depth[l]))
    }

    /// Whether face-local column (x, z) is held (generated water at rest).
    pub fn held_at(&self, f: FaceFrame, x: i32, z: i32) -> bool {
        self.find(f, x, z)
            .is_some_and(|(c, l)| self.chunks[c].held[l])
    }

    /// The water stored in free columns (m³): conserved but for what held columns trade.
    pub fn free_volume(&self) -> f64 {
        self.chunks
            .iter()
            .map(|k| {
                (0..CA)
                    .filter(|&l| !k.w.floor[l].is_nan() && !k.held[l])
                    .map(|l| f64::from(k.w.depth[l]))
                    .sum::<f64>()
                    + k.owed
            })
            .sum()
    }

    /// Held columns, and the water they hold (diagnostics, tests).
    pub fn held(&self) -> (usize, f64) {
        let mut n = 0;
        let mut v = 0.0;
        for k in &self.chunks {
            for l in 0..CA {
                if k.held[l] && !k.w.floor[l].is_nan() {
                    n += 1;
                    v += f64::from(k.w.depth[l]);
                }
            }
        }
        (n, v)
    }

    /// Chunks awake (diagnostics, tests).
    pub fn awake_chunks(&self) -> usize {
        self.chunks.iter().filter(|k| k.awake).count()
    }

    /// Cells edits changed (their old states), to free the water near them next tick.
    pub fn touch(&mut self, cells: impl IntoIterator<Item = ([i32; 3], StateId)>) {
        self.touched.extend(cells.into_iter().map(|(c, _)| c));
    }

    /// The world's chunks water wants loaded: those of awake water and those beside it (so moving
    /// water goes on with no player near, and spreads on into the world beside it).
    pub fn wanted_chunks(&self) -> Vec<ChunkCoord> {
        let mut out: HashSet<ChunkCoord> = HashSet::default();
        for k in self.chunks.iter().filter(|k| k.awake) {
            for l in 0..CA {
                if k.w.floor[l].is_nan() || k.w.depth[l] <= DRY {
                    continue;
                }
                let (x, z) = k.xz(l);
                let r = row_of_m(f64::from(k.w.floor[l] + k.w.depth[l]));
                // its own chunk and those beside it (where it may spread next)
                for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let c = cell(k.f, x + dx, r, z + dz);
                    out.insert(chunk_of(c[0], c[1], c[2]));
                }
            }
        }
        let mut out: Vec<ChunkCoord> = out.into_iter().collect();
        out.sort_by_key(|c| (c.x, c.y, c.z));
        out
    }

    /// A summary of the water near world cell `at` (a diagnostic).
    pub fn report(&self, at: [i32; 3], radius: i32) -> String {
        let num = |v: f64| {
            if v.is_finite() {
                format!("{}", (v * 1000.0).round() / 1000.0)
            } else {
                "null".to_owned()
            }
        };
        let f = FaceFrame::at_row(at[1]);
        let mut columns = Vec::new();
        for z in at[2] - radius..=at[2] + radius {
            for x in at[0] - radius..=at[0] + radius {
                let Some((c, l)) = self.find(f, x, z) else {
                    continue;
                };
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() {
                    continue;
                }
                columns.push(format!(
                    r#"{{"x":{x},"z":{z},"floor":{},"level":{},"rest":{},"held":{},"awake":{}}}"#,
                    num(f64::from(k.w.floor[l])),
                    num(f64::from(k.w.floor[l] + k.w.depth[l])),
                    num(f64::from(k.r.floor[l] + k.r.depth[l])),
                    k.held[l],
                    k.awake
                ));
            }
        }
        let (held, held_m3) = self.held();
        format!(
            r#"{{"model":"surface grid (docs/WATER_SURFACE_GRID.md), heights face-local m","chunks":{},"awake":{},"t":{},"free_m3":{},"held":{held},"held_m3":{},"substeps":{},"wakes":{},"released":{},"at":[{},{},{}],"columns":[{}]}}"#,
            self.chunks.len(),
            self.awake_chunks(),
            num(self.t),
            num(self.free_volume()),
            num(held_m3),
            self.substeps,
            self.wakes,
            self.released,
            at[0],
            at[1],
            at[2],
            columns.join(",")
        )
    }

    /// One water tick of `dt` s of water time on a world it reads itself (inline: tests, tools):
    /// its copy of the world brought up to date (`cells::Mirror`), then [`Water::step`] within the
    /// step budget, its writes and currents put in `world`. Returns the cells written.
    pub fn tick(
        &mut self,
        world: &mut VoxelWorld,
        _players: &[[f64; 3]],
        dt: f64,
    ) -> Vec<([i32; 3], StateId)> {
        let mut mirror = std::mem::take(&mut self.mirror);
        for ch in mirror.sync(world) {
            self.change(ch);
        }
        self.mirror = mirror;
        let step = self.step(dt, self.cfg.step_budget);
        for (key, c) in step.currents {
            world.currents.set(key, c);
        }
        step.writes
            .into_iter()
            .filter_map(|w| {
                let m = w.apply_to(world.get_voxel_if_loaded(w.cell))?;
                world.write_voxel(w.cell[0], w.cell[1], w.cell[2], m);
                Some((w.cell, m))
            })
            .collect()
    }

    /// A change to the grid's copy of the world: a chunk loaded (`fresh`: its water is read at
    /// the next step) or sent again, or unloaded.
    pub fn change(&mut self, ch: cells::Change) {
        match ch {
            cells::Change::Load { at, cells, fresh } => self.load(at, cells, fresh),
            cells::Change::Unload(at) => self.unload(at),
        }
    }

    /// A chunk's cells into the copy; `fresh`, its water is read at the next step.
    pub fn load(&mut self, at: ChunkCoord, cells: ChunkCells, fresh: bool) {
        self.cells.insert(at, cells);
        if fresh {
            self.loaded.push(at);
        }
    }

    pub fn unload(&mut self, at: ChunkCoord) {
        self.cells.remove(at);
        self.loaded.retain(|&c| c != at);
    }

    /// An edited cell (its new state's byte, `cells::byte_of`): set in the copy, and its column
    /// touched (the water near it freed at the next step).
    pub fn edited(&mut self, cell: [i32; 3], byte: u8) {
        self.cells.set(cell, byte);
        self.touched.push(cell);
    }

    /// The grid's copy of the world (diagnostics, tests).
    pub fn cells(&self) -> &Cells {
        &self.cells
    }

    /// Water time run (s).
    pub fn time(&self) -> f64 {
        self.t
    }

    /// One step of up to `dt` s of water time on its copy of the world: new chunks read, edits
    /// applied, the free water stepped within `budget` column-substeps, its changed cells
    /// written (into the copy; the caller puts them in the world).
    pub fn step(&mut self, dt: f64, budget: usize) -> Step {
        self.scan();
        self.apply_touched();
        self.grow_ring();
        // substeps, each as long as the deepest awake water allows; at most the step budget's
        // worth over the awake columns (water runs on game time: a tick is seconds of water, and
        // much water awake must not take a thread longer than a tick)
        let mut left = dt;
        let mut n = 0;
        let awake = self.awake_chunks().max(1) * CA;
        let cap = self.cfg.max_substeps.min((budget / awake).max(1));
        while left > 1e-9 && n < cap {
            let deepest = self
                .chunks
                .iter()
                .filter(|k| k.awake)
                .flat_map(|k| (0..CA).filter(|&l| !k.held[l]).map(move |l| k.w.depth[l]))
                .fold(0.01f32, f32::max);
            if self.awake_chunks() == 0 {
                self.t += left;
                left = 0.0;
                break;
            }
            let cfl = f64::from(self.cfg.cfl / (G * deepest).sqrt());
            let steps = (left / cfl).ceil().max(1.0);
            let sub = left / steps;
            self.substep(sub as f32);
            left -= sub;
            n += 1;
        }
        // The rest of the time, past the budget, isn't run (`Step::ran` says how much was): inline
        // it is lost, the water running slow, and its clock (the calm windows, the release of
        // what a sleeping chunk owes) with it, so slowed water isn't taken for calm; on its own
        // thread it is owed (`host.rs`).
        self.grow_ring();
        let currents = self.publish_currents();
        let writes = self.draw();
        Step {
            ran: if left <= 1e-9 { dt } else { dt - left },
            writes,
            currents,
        }
    }

    /// The awake chunks' currents (for `VoxelWorld::currents`, docs/WATER_SURFACE_GRID.md §2c), for
    /// swimmers and floating bodies; a sleeping chunk keeps the currents it fell asleep with (its
    /// flows are kept too).
    fn publish_currents(&mut self) -> Vec<(CurrentsKey, Option<ChunkCurrents>)> {
        let fresh = std::mem::take(&mut self.fresh);
        let mut out = Vec::new();
        for (c, k) in self.chunks.iter().enumerate() {
            if k.awake || fresh.contains(&c) {
                out.push(((k.f.is_b(), k.cx, k.cz), self.currents_of(c)));
            }
        }
        out
    }

    /// Chunk `c`'s currents: each free column's velocity (m/s), the mean of its faces' discharges
    /// over its depth, and each held column's by the world map's river (G3b); none if all are still
    /// (`Settings::still`).
    fn currents_of(&self, c: usize) -> Option<ChunkCurrents> {
        let k = &self.chunks[c];
        let mut out: ChunkCurrents = Box::new([[0.0; 2]; CA]);
        let mut any = false;
        for l in 0..CA {
            let h = k.w.depth[l];
            if k.held[l] {
                if k.river[l] != [0.0; 2] {
                    out[l] = k.river[l];
                    any = true;
                }
                continue;
            }
            if k.w.floor[l].is_nan() || h < self.cfg.current_depth {
                continue;
            }
            let behind = |d: usize, z: bool| {
                self.step_col(c, l, d).map_or(0.0, |(c2, l2)| {
                    let n = &self.chunks[c2].w;
                    if z { n.qz[l2] } else { n.qx[l2] }
                })
            };
            let u = 0.5 * (k.w.qx[l] + behind(1, false)) / h;
            let v = 0.5 * (k.w.qz[l] + behind(3, true)) / h;
            if u.hypot(v) >= self.cfg.still {
                out[l] = [u, v];
                any = true;
            }
        }
        any.then_some(out)
    }

    /// Reads newly loaded chunks: their water joins the grid held at the level found (unless the
    /// grid already holds that column); columns already in the grid are drawn again there.
    fn scan(&mut self) {
        let mut new = std::mem::take(&mut self.loaded);
        new.dedup();
        for cc in new {
            let size = dwell_world::CHUNK_SIZE;
            let (x0, y0, z0) = (cc.x * size, cc.y * size, cc.z * size);
            // the chunk's liquid cells, from its cells at once (most chunks hold none)
            let Some(chunk) = self.cells.chunk(cc) else {
                continue;
            };
            let wet: Vec<[i32; 3]> = chunk
                .liquids()
                .into_iter()
                .map(|i| {
                    let i = i as i32;
                    // `dwell_world::local_index`'s layout: x, then y, then z, 32 each
                    [x0 + (i & 31), y0 + ((i >> 5) & 31), z0 + (i >> 10)]
                })
                .collect();
            for [x, y, z] in wet {
                let f = FaceFrame::at_row(y);
                if let Some((c, l)) = self.find(f, x, z)
                    && !self.chunks[c].w.floor[l].is_nan()
                {
                    self.chunks[c].drawn[l] = f32::NAN; // written again from the grid
                    continue;
                }
                let Some((floor, level)) = Self::found(&self.cells, f, x, f.row(y), z) else {
                    continue;
                };
                let (c, l) = self.column(f, x, z);
                let k = &mut self.chunks[c];
                k.w.floor[l] = floor;
                k.w.depth[l] = (level - floor).max(0.0);
                k.held[l] = true;
                k.drawn[l] = level;
                k.r.floor[l] = floor;
                k.r.depth[l] = k.w.depth[l];
                k.river[l] = [0.0; 2];
                if let Some(rivers) = &self.rivers
                    && let Some([u, v]) = rivers(f, x, z, f64::from(k.w.depth[l]))
                {
                    k.river[l] = [u as f32, v as f32];
                    self.fresh.insert(c);
                }
            }
        }
    }

    /// The floor and level of the water run at face `f`'s column (x, z) through local row `r`.
    fn found(world: &Cells, f: FaceFrame, x: i32, r: i32, z: i32) -> Option<(f32, f32)> {
        let liquid = |r: i32| cell_kind(world, cell(f, x, r, z)) == Some(Kind::Liquid);
        let mut top = r;
        while liquid(top + 1) {
            top += 1;
        }
        let mut bottom = r;
        while liquid(bottom - 1) {
            bottom -= 1;
        }
        if cell_kind(world, cell(f, x, bottom - 1, z))? != Kind::Solid {
            return None; // water over open air (a fall): not a column's
        }
        let fill = cells::fill_of(world.get(cell(f, x, top, z)).unwrap_or(cells::FULL));
        Some((
            m_of_row(bottom) as f32,
            (m_of_row(top) + f64::from(fill) * CELL_H) as f32,
        ))
    }

    /// Edits: each touched column's floor found again, and the water within `EDIT_REACH` freed
    /// and woken.
    fn apply_touched(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        let mut cols: HashSet<(FaceFrame, i32, i32)> = HashSet::default();
        for c in touched {
            cols.insert((FaceFrame::at_row(c[1]), c[0], c[2]));
        }
        for &(f, x, z) in &cols {
            // the column's floor again, at its level or its neighbours'
            let mut at = None;
            for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(lv) = self.level_at(f, x + dx, z + dz) {
                    at = Some(at.map_or(lv, |a: f64| a.max(lv)));
                }
            }
            let Some(at) = at else { continue };
            let (c, l) = self.column(f, x, z);
            let old_level = {
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() {
                    f32::NAN
                } else {
                    k.w.floor[l] + k.w.depth[l]
                }
            };
            let at = if old_level.is_nan() {
                at
            } else {
                at.max(f64::from(old_level))
            };
            let floor = floor_at(&self.cells, f, x, z, at + 0.01);
            let old_floor = self.chunks[c].w.floor[l];
            // water left below a block set over it stays there, as blocks, out of the grid
            let trapped = match floor {
                Some(fl) if !old_floor.is_nan() && fl > old_floor => {
                    (row_of_m(f64::from(old_floor))..row_of_m(f64::from(fl)))
                        .map(|r| {
                            CELL_H as f32
                                * cells::fill_of(self.cells.get(cell(f, x, r, z)).unwrap_or(0))
                        })
                        .sum::<f32>()
                }
                _ => 0.0,
            };
            let k = &mut self.chunks[c];
            match floor {
                Some(fl) => {
                    // the water keeps its volume: a block set under it lifts it, one dug lowers it
                    k.w.floor[l] = fl;
                    k.w.depth[l] = (k.w.depth[l] - trapped).max(0.0);
                }
                None => {
                    k.w.floor[l] = f32::NAN;
                    k.w.depth[l] = 0.0;
                }
            }
            k.drawn[l] = f32::NAN;
            let reach = self.cfg.edit_reach;
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    if let Some((c2, l2)) = self.find(f, x + dx, z + dz) {
                        self.free(c2, l2);
                    }
                }
            }
        }
    }

    /// Frees a held column (its chunk woken).
    fn free(&mut self, c: usize, l: usize) {
        let k = &mut self.chunks[c];
        if k.held[l] {
            k.held[l] = false;
            self.released += 1;
        }
        self.wake(c);
    }

    fn wake(&mut self, c: usize) {
        let t = self.t;
        let k = &mut self.chunks[c];
        if k.awake {
            k.calm_t = -1.0;
            return;
        }
        k.awake = true;
        k.calm_t = -1.0;
        k.rel_rate = k.owed / self.cfg.release_s;
        let _ = t;
        self.wakes += 1;
    }

    /// The ring: every wet free column's four neighbours get columns (their floors found), so
    /// water can spread; a wet column beside unloaded world is held (the loaded world's edge).
    fn grow_ring(&mut self) {
        let mut wet: Vec<(FaceFrame, i32, i32, f32)> = Vec::new();
        for k in &self.chunks {
            if !k.awake {
                continue;
            }
            for l in 0..CA {
                if !k.w.floor[l].is_nan() && k.w.depth[l] > DRY && !k.held[l] {
                    let (x, z) = k.xz(l);
                    wet.push((k.f, x, z, k.w.floor[l] + k.w.depth[l]));
                }
            }
        }
        for (f, x, z, level) in wet {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x + dx, z + dz);
                if self
                    .find(f, nx, nz)
                    .is_some_and(|(c, l)| !self.chunks[c].w.floor[l].is_nan())
                {
                    continue;
                }
                let at = f64::from(level) + 0.01;
                // unloaded (or no floor near): a wall for now; `wanted_chunks` asks for the world
                // beside moving water, so it spreads on once that loads
                if let Some(fl) = floor_at(&self.cells, f, nx, nz, at) {
                    let (c, l) = self.column(f, nx, nz);
                    let k = &mut self.chunks[c];
                    k.w.floor[l] = fl;
                    k.w.depth[l] = 0.0;
                    k.held[l] = false;
                    // dry land as generated, unless its rest is known already
                    if k.r.floor[l].is_nan() {
                        k.r.floor[l] = fl;
                        k.r.depth[l] = 0.0;
                    }
                    if !k.awake {
                        self.wake(c);
                    }
                }
            }
        }
    }

    /// The column beside `(c, l)` toward `d` (E W S N), if any.
    fn step_col(&self, c: usize, l: usize, d: usize) -> Option<(usize, usize)> {
        let (x, z) = ((l as i32) % CS, (l as i32) / CS);
        let (nx, nz) = match d {
            0 => (x + 1, z),
            1 => (x - 1, z),
            2 => (x, z + 1),
            _ => (x, z - 1),
        };
        if (0..CS).contains(&nx) && (0..CS).contains(&nz) {
            return Some((c, (nz * CS + nx) as usize));
        }
        let n = self.chunks[c].nbr[d];
        if n == NONE {
            return None;
        }
        Some((
            n as usize,
            (nz.rem_euclid(CS) * CS + nx.rem_euclid(CS)) as usize,
        ))
    }

    /// Whether chunk `c` computes the face toward `d` (E or S) of its column `l` this substep.
    fn computes(a: u8, l: usize, zf: bool) -> bool {
        let (x, z) = ((l as i32) % CS, (l as i32) / CS);
        let edge = if zf { z == CS - 1 } else { x == CS - 1 };
        a & 1 != 0 || (edge && ((!zf && a & 2 != 0) || (zf && a & 4 != 0)))
    }

    /// The face law over one layer (the water as it is, or as generated): this substep's flows.
    fn momentum(&mut self, rest: bool, dt: f32) {
        for c in 0..self.chunks.len() {
            let a = self.chunks[c].active;
            if a == 0 {
                continue;
            }
            for l in 0..CA {
                for (zf, d) in [(false, 0usize), (true, 2usize)] {
                    if !Self::computes(a, l, zf) {
                        continue;
                    }
                    let nq = {
                        let q_of = |cc: usize, ll: usize, z: bool| -> f32 {
                            let k = lay(&self.chunks[cc], rest);
                            if z { k.qz[ll] } else { k.qx[ll] }
                        };
                        match self.step_col(c, l, d) {
                            Some((c2, l2)) => {
                                let (ka, kb) =
                                    (lay(&self.chunks[c], rest), lay(&self.chunks[c2], rest));
                                let (ga, gb) = (ka.floor[l], kb.floor[l2]);
                                if ga.is_nan()
                                    || gb.is_nan()
                                    || (ka.depth[l] <= 0.0 && kb.depth[l2] <= 0.0)
                                {
                                    0.0
                                } else {
                                    let q = q_of(c, l, zf);
                                    let qa = self
                                        .step_col(c, l, d ^ 1)
                                        .map_or(q, |(c3, l3)| q_of(c3, l3, zf));
                                    let qb = q_of(c2, l2, zf);
                                    // the cross-direction faces about this one: each column's own
                                    // and the one before it
                                    let perp = |cc: usize, ll: usize| -> f32 {
                                        let own = q_of(cc, ll, !zf);
                                        let before = self
                                            .step_col(cc, ll, if zf { 1 } else { 3 })
                                            .map_or(0.0, |(c4, l4)| q_of(c4, l4, !zf));
                                        0.5 * (own + before)
                                    };
                                    let qp = 0.5 * (perp(c, l) + perp(c2, l2));
                                    face(
                                        &self.cfg,
                                        ga,
                                        ka.depth[l],
                                        gb,
                                        kb.depth[l2],
                                        q,
                                        qa,
                                        qb,
                                        qp,
                                        dt,
                                    )
                                }
                            }
                            None => 0.0,
                        }
                    };
                    let k = &mut self.chunks[c];
                    let k = if rest { &mut k.r } else { &mut k.w };
                    if zf { k.nqz[l] = nq } else { k.nqx[l] = nq }
                }
            }
        }
        // the outflow limiter, for awake free columns
        for c in 0..self.chunks.len() {
            if self.chunks[c].active & 1 == 0 {
                continue;
            }
            for l in 0..CA {
                if self.chunks[c].held[l] {
                    continue;
                }
                let (floor, d, mut out) = {
                    let k = lay(&self.chunks[c], rest);
                    (
                        k.floor[l],
                        k.depth[l],
                        k.nqx[l].max(0.0) + k.nqz[l].max(0.0),
                    )
                };
                if floor.is_nan() {
                    continue;
                }
                if let Some((c2, l2)) = self.step_col(c, l, 1) {
                    out += (-lay(&self.chunks[c2], rest).nqx[l2]).max(0.0);
                }
                if let Some((c2, l2)) = self.step_col(c, l, 3) {
                    out += (-lay(&self.chunks[c2], rest).nqz[l2]).max(0.0);
                }
                let out = out * dt;
                let k = &mut self.chunks[c];
                let k = if rest { &mut k.r } else { &mut k.w };
                k.scale[l] = if out > d { d / out } else { 1.0 };
            }
        }
    }

    /// Settles one layer's faces (the limiter applied); returns, for faces between an awake and a
    /// sleeping chunk, (sleeper, the flow into it, sleeper's column, whether the face jumped).
    fn settle(&mut self, rest: bool) -> Vec<(usize, usize, bool, f32, bool)> {
        let jump = self.cfg.jump;
        let mut edges = Vec::new();
        for c in 0..self.chunks.len() {
            let a = self.chunks[c].active;
            if a == 0 {
                continue;
            }
            for l in 0..CA {
                for (zf, d) in [(false, 0usize), (true, 2usize)] {
                    if !Self::computes(a, l, zf) {
                        continue;
                    }
                    let Some((c2, l2)) = self.step_col(c, l, d) else {
                        continue;
                    };
                    let (ka, kb) = (&self.chunks[c], &self.chunks[c2]);
                    let (la, lb) = if rest { (&ka.r, &kb.r) } else { (&ka.w, &kb.w) };
                    let raw = if zf { la.nqz[l] } else { la.nqx[l] };
                    let s = if raw > 0.0 {
                        if ka.awake { la.scale[l] } else { 1.0 }
                    } else if kb.awake {
                        lb.scale[l2]
                    } else {
                        1.0
                    };
                    let nq = raw * s;
                    let old = if zf { la.qz[l] } else { la.qx[l] };
                    let (wa, wb) = (ka.awake, kb.awake);
                    let k = &mut self.chunks[c];
                    let k = if rest { &mut k.r } else { &mut k.w };
                    if zf {
                        k.qz[l] = nq
                    } else {
                        k.qx[l] = nq
                    }
                    if c != c2 && wa != wb {
                        edges.push((c, l, zf, nq, (nq - old).abs() > jump));
                    }
                }
            }
        }
        edges
    }

    /// A layer's net flow into column (c, l) (m/s).
    fn net(&self, c: usize, l: usize, rest: bool) -> f32 {
        let k = lay(&self.chunks[c], rest);
        let mut net = -k.qx[l] - k.qz[l];
        if let Some((c2, l2)) = self.step_col(c, l, 1) {
            net += lay(&self.chunks[c2], rest).qx[l2];
        }
        if let Some((c2, l2)) = self.step_col(c, l, 3) {
            net += lay(&self.chunks[c2], rest).qz[l2];
        }
        net
    }

    /// One substep of `dt` water seconds over the awake chunks (and the edges of sleeping ones
    /// beside them): the water as it is and as generated both stepped; each free column changes
    /// by the difference of their flows.
    fn substep(&mut self, dt: f32) {
        self.substeps += 1;
        let n = self.chunks.len();
        // which chunks compute which faces: an awake one all its own; a sleeping one those toward
        // an awake east or south neighbour
        for c in 0..n {
            let k = &self.chunks[c];
            let mut a = 0u8;
            if k.awake {
                a = 1;
            } else {
                if k.nbr[0] != NONE && self.chunks[k.nbr[0] as usize].awake {
                    a |= 2;
                }
                if k.nbr[2] != NONE && self.chunks[k.nbr[2] as usize].awake {
                    a |= 4;
                }
            }
            self.chunks[c].active = a;
        }
        self.momentum(false, dt);
        self.momentum(true, dt);
        let edges = self.settle(false);
        let rest_edges = self.settle(true);
        // across an awake and a sleeping chunk: the sleeper's account, by the same difference
        for ((c, l, zf, nq, jump), (_, _, _, rq, _)) in edges.into_iter().zip(rest_edges) {
            let d = if zf { 2 } else { 0 };
            let Some((c2, l2)) = self.step_col(c, l, d) else {
                continue;
            };
            let wa = self.chunks[c].awake;
            let (sleeper, sign) = if wa { (c2, 1.0) } else { (c, -1.0) };
            let col = if wa { (c2, l2) } else { (c, l) };
            if !self.chunks[col.0].held[col.1] {
                self.chunks[sleeper].owed += sign * f64::from(nq - rq) * f64::from(dt);
            }
            if jump {
                self.wake(sleeper);
            }
        }
        // the water balance, for awake free columns (held ones keep their level)
        let t = f64::from(dt) + self.t;
        let cfg = self.cfg;
        for c in 0..n {
            if !self.chunks[c].awake {
                continue;
            }
            let mut moved = 0.0f32;
            for l in 0..CA {
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() || k.held[l] {
                    continue;
                }
                let net = self.net(c, l, false)
                    - if k.r.floor[l].is_nan() {
                        0.0
                    } else {
                        self.net(c, l, true)
                    };
                let k = &mut self.chunks[c];
                k.w.depth[l] = (k.w.depth[l] + net * dt).max(0.0);
                if k.calm_t >= 0.0 {
                    moved = moved.max((k.w.depth[l] - k.win[l]).abs());
                }
            }
            // what it still owes from its sleep comes in, by depth
            let k = &mut self.chunks[c];
            if k.owed != 0.0 {
                let want = (k.rel_rate * f64::from(dt)).abs().min(k.owed.abs()) * k.owed.signum();
                let tot: f64 = (0..CA)
                    .filter(|&l| !k.w.floor[l].is_nan() && !k.held[l])
                    .map(|l| f64::from(k.w.depth[l]))
                    .sum();
                if tot > 0.0 {
                    let r = (want / tot).max(-1.0);
                    let mut got = 0.0;
                    for l in 0..CA {
                        if k.w.floor[l].is_nan() || k.held[l] {
                            continue;
                        }
                        let add = (f64::from(k.w.depth[l]) * r) as f32;
                        k.w.depth[l] += add;
                        got += f64::from(add);
                    }
                    k.owed -= got;
                    if k.owed.abs() < 1e-9 {
                        k.owed = 0.0;
                    }
                }
            }
            // the calm window: asleep once still within the band for CALM_S
            if k.calm_t < 0.0 || moved > cfg.sleep_band {
                k.calm_t = t;
                k.win = k.w.depth;
            } else if t - k.calm_t >= cfg.calm_s && k.owed == 0.0 {
                k.awake = false;
                k.w.scale = [1.0; CA];
                k.r.scale = [1.0; CA];
                // what rests is drawn as it is (no margin)
                for l in 0..CA {
                    if !k.held[l] {
                        k.drawn[l] = f32::NAN;
                    }
                }
            }
        }
        // a free column the change has reached (its level `RELEASE` off its rest) frees the held
        // columns beside it
        let mut free: Vec<(usize, usize)> = Vec::new();
        for c in 0..n {
            if !self.chunks[c].awake {
                continue;
            }
            for l in 0..CA {
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() || k.held[l] {
                    continue;
                }
                let lv = k.w.floor[l] + k.w.depth[l];
                let rest = if k.r.floor[l].is_nan() {
                    k.w.floor[l]
                } else {
                    k.r.floor[l] + k.r.depth[l]
                };
                if (lv - rest).abs() <= self.cfg.release {
                    continue;
                }
                for d in 0..4 {
                    if let Some((c2, l2)) = self.step_col(c, l, d) {
                        let o = &self.chunks[c2];
                        if o.held[l2] && !o.w.floor[l2].is_nan() {
                            free.push((c2, l2));
                        }
                    }
                }
            }
        }
        for (c, l) in free {
            self.free(c, l);
        }
        // a sleeping chunk whose account would move its water half the wake band wakes
        for c in 0..n {
            let k = &self.chunks[c];
            if k.awake || k.owed == 0.0 {
                continue;
            }
            let wet = k.wet().max(1) as f64;
            if k.owed.abs() / wet > f64::from(self.cfg.wake_band) * 0.5 {
                self.wake(c);
            }
        }
        self.t = t;
    }

    /// Writes changed free columns as blocks: full cells to the level, the top cell in eighths,
    /// emptied cells cleared (never a solid, never what isn't water or open).
    fn draw(&mut self) -> Vec<Write> {
        let liquid = self.cfg.liquid;
        let mut writes = Vec::new();
        for c in 0..self.chunks.len() {
            for l in 0..CA {
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() || k.held[l] {
                    continue;
                }
                let level = if k.w.depth[l] > 0.01 {
                    k.w.floor[l] + k.w.depth[l]
                } else {
                    k.w.floor[l]
                };
                let drawn = k.drawn[l];
                if !drawn.is_nan()
                    && (level - drawn).abs() < STEP * 0.5 + self.cfg.hyst
                    && !(drawn > k.w.floor[l] && level <= k.w.floor[l])
                {
                    continue;
                }
                let (f, (x, z), floor) = (k.f, k.xz(l), k.w.floor[l]);
                let top = level.max(if drawn.is_nan() { level } else { drawn });
                let (r0, r1) = (row_of_m(f64::from(floor)), row_of_m(f64::from(top)) + 1);
                for r in r0..=r1 {
                    let w = cell(f, x, r, z);
                    let Some(now) = self.cells.get(w) else {
                        continue; // not loaded
                    };
                    let kd = cells::kind_of(now);
                    if kd == Kind::Solid {
                        continue;
                    }
                    let h = level - m_of_row(r) as f32;
                    let want = if h >= CELL_H as f32 - STEP * 0.5 {
                        Some(liquid)
                    } else if h >= STEP * 0.5 {
                        Some(dwell_blocks::liquid_at_fill(liquid, f64::from(h) / CELL_H))
                    } else if kd == Kind::Liquid {
                        Some(M.air)
                    } else {
                        None
                    };
                    if let Some(want) = want {
                        let b = cells::byte_of(want);
                        if b != now {
                            self.cells.set(w, b);
                            writes.push(Write {
                                cell: w,
                                state: want,
                                was: now,
                                flood: None,
                            });
                        }
                    }
                }
                self.chunks[c].drawn[l] = level;
                self.flood_near(f, x, z, r0 - 2, r1, &mut writes);
            }
        }
        writes
    }

    /// The highest water level in face `f`'s column (x, z) and the four beside it (face-local m).
    fn level_near(&self, f: FaceFrame, x: i32, z: i32) -> Option<f32> {
        [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .filter_map(|(dx, dz)| {
                let (c, l) = self.find(f, x + dx, z + dz)?;
                let k = &self.chunks[c];
                (!k.w.floor[l].is_nan() && k.w.depth[l] > 0.01).then(|| k.w.floor[l] + k.w.depth[l])
            })
            .reduce(f32::max)
    }

    /// Blocks holding water in their open part (ramps, `cells::HOLDS_*`) in rows `r0..=r1` of
    /// column (x, z) and the four beside it: flooded while the water beside or over them stands
    /// half a fill step into their cell, dry when it goes (BUGS.md #36: the grid had kept them dry).
    fn flood_near(
        &mut self,
        f: FaceFrame,
        x: i32,
        z: i32,
        r0: i32,
        r1: i32,
        writes: &mut Vec<Write>,
    ) {
        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (cx, cz) = (x + dx, z + dz);
            let mut near = None;
            for r in r0..=r1 {
                let w = cell(f, cx, r, cz);
                let Some(now) = self.cells.get(w) else {
                    continue;
                };
                let Some(wet) = cells::holds(now) else {
                    continue;
                };
                let level = *near.get_or_insert_with(|| self.level_near(f, cx, cz));
                let want = level.is_some_and(|lv| lv >= m_of_row(r) as f32 + STEP * 0.5);
                if want != wet {
                    let b = if want {
                        cells::HOLDS_WET
                    } else {
                        cells::HOLDS_DRY
                    };
                    self.cells.set(w, b);
                    writes.push(Write {
                        cell: w,
                        state: 0,
                        was: now,
                        flood: Some(want),
                    });
                }
            }
        }
    }

    /// The free water as saved (version 1 of the grid's record): each chunk with free columns,
    /// their floors, depths and flows. Held columns are found again from the world's blocks.
    pub fn save(&self, _revision: impl Fn(ChunkCoord) -> u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.t.to_le_bytes());
        let kept: Vec<&Box<Chunk>> = self
            .chunks
            .iter()
            .filter(|k| (0..CA).any(|l| !k.w.floor[l].is_nan() && !k.held[l]))
            .collect();
        out.extend_from_slice(&(kept.len() as u32).to_le_bytes());
        for k in kept {
            out.push(match k.f.face {
                dwell_world::Face::A => 0,
                dwell_world::Face::B => 1,
            });
            out.extend_from_slice(&k.cx.to_le_bytes());
            out.extend_from_slice(&k.cz.to_le_bytes());
            for l in 0..CA {
                let free = !k.w.floor[l].is_nan() && !k.held[l];
                out.push(free as u8);
                if free {
                    for v in [
                        k.w.floor[l],
                        k.w.depth[l],
                        k.w.qx[l],
                        k.w.qz[l],
                        k.r.floor[l],
                        k.r.depth[l],
                        k.r.qx[l],
                        k.r.qz[l],
                    ] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            out.extend_from_slice(&k.owed.to_le_bytes());
        }
        out
    }

    /// The free water restored (None if the record isn't the grid's: older worlds' water is found
    /// again from their blocks). Returns the chunks restored.
    pub fn restore(
        &mut self,
        bytes: &[u8],
        _revision: impl Fn(ChunkCoord) -> u32,
    ) -> Option<usize> {
        let mut r = Reader { b: bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return None;
        }
        self.t = f64::from_le_bytes(r.take(8)?.try_into().ok()?);
        let n = u32::from_le_bytes(r.take(4)?.try_into().ok()?) as usize;
        for _ in 0..n {
            let f = if r.take(1)?[0] == 0 {
                FaceFrame::A
            } else {
                FaceFrame::B
            };
            let cx = i32::from_le_bytes(r.take(4)?.try_into().ok()?);
            let cz = i32::from_le_bytes(r.take(4)?.try_into().ok()?);
            let c = self.chunk_index(f, cx, cz);
            for l in 0..CA {
                if r.take(1)?[0] == 1 {
                    let mut v = [0.0f32; 8];
                    for x in &mut v {
                        *x = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
                    }
                    let k = &mut self.chunks[c];
                    k.w.floor[l] = v[0];
                    k.w.depth[l] = v[1];
                    k.w.qx[l] = v[2];
                    k.w.qz[l] = v[3];
                    k.r.floor[l] = v[4];
                    k.r.depth[l] = v[5];
                    k.r.qx[l] = v[6];
                    k.r.qz[l] = v[7];
                    k.held[l] = false;
                    k.drawn[l] = f32::NAN;
                }
            }
            let owed = f64::from_le_bytes(r.take(8)?.try_into().ok()?);
            let k = &mut self.chunks[c];
            k.owed = owed;
            k.awake = true;
            k.calm_t = -1.0;
            k.rel_rate = owed / self.cfg.release_s;
        }
        Some(n)
    }
}

const MAGIC: &[u8; 4] = b"DWG1";

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
}

#[cfg(test)]
mod tests;
