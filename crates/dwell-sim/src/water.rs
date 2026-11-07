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
//!   stepped; it keeps passing on the flow its edges carried as it fell asleep (`kept`) and its
//!   imbalance then (`drift`), so a river runs through sleeping chunks; what its awake neighbours
//!   send it past what they sent then is kept exactly (`owed`), and it wakes when that would move
//!   its wet columns half the wake band, when it owes half the water it has, or when a shared face
//!   jumps (an edit, a flood front). Waking, it takes what it owes over `releaseS`, in proportion
//!   to its columns' depth. A film out of the rest still running off is never calm (BUGS.md #41b).
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
/// An untouched river's flow (m²/s along x and z, per metre of width) through face `f`'s column
/// (x, z), by the world map (`TerrainGenerator::river_flow`): carried through the water as
/// generated, so a dam backs the river up by it (docs/WATER_SURFACE_GRID.md §2b).
pub type RiverFlow = Arc<dyn Fn(FaceFrame, i32, i32) -> Option<[f64; 2]> + Send + Sync>;

/// Columns a water chunk spans each way.
pub const CS: i32 = 16;
const CA: usize = (CS * CS) as usize;
const NONE: u32 = u32::MAX;

const G: f32 = 9.81;
const DRY: f32 = 1e-4;
/// A film out of the rest falling this much in a calm window is still running off, not calm (m).
const FILM_FALL: f32 = 1e-4;
/// A sleeping chunk with less free water than this (m³) owing it doesn't wake for it.
const OWED_MIN: f64 = 0.01;

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
    /// A column whose inflow falls under `rest_cut` of the rest's leaves the rest (its springs
    /// and sinks), and one whose inflow comes back over `rest_back` of it returns (BUGS.md #41).
    pub rest_cut: f32,
    pub rest_back: f32,
    pub edit_reach: i32,
    /// How far a level moves past a drawn step before its cells are written again (m).
    pub hyst: f32,
    /// Slower water than this (m/s) has no current, nor water shallower than `current_depth` (m)
    /// (docs/WATER_SURFACE_GRID.md §2c).
    pub still: f32,
    pub current_depth: f32,
    /// A freed column still in the rest shows the map's current, blended to its own as its depth
    /// leaves its generated depth by up to this (m; `riverCurrentBandM`).
    pub river_band: f32,
    /// The slosh's drag (BUGS.md #41b): a face's swing off its running mean (over `slosh_mean`
    /// water s) is pulled back to it over `slosh_damp` s (0: none), less as the swing runs faster
    /// than `slosh_speed` (m/s).
    pub slosh_damp: f32,
    pub slosh_mean: f32,
    pub slosh_speed: f32,
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
            rest_cut: d.rest_cut as f32,
            rest_back: d.rest_back as f32,
            edit_reach: d.edit_reach as i32,
            hyst: d.fill_hysteresis as f32 * STEP,
            still: d.current_still_ms as f32,
            current_depth: d.current_depth_m as f32,
            river_band: d.river_current_band_m.max(1e-6) as f32,
            slosh_damp: d.slosh_damp_s.max(0.0) as f32,
            slosh_mean: d.slosh_mean_s.max(1e-3) as f32,
            slosh_speed: d.slosh_speed_ms.max(1e-6) as f32,
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
    mean: f32,
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
    let mut nq = (qt - G * h * dt * (eb - ea)) / friction;
    // The slosh's drag: the swing off the face's running mean is pulled back to it, the more the
    // slower it runs, so a steady flow is never dragged and a big wave hardly (BUGS.md #41b).
    if s.slosh_damp > 0.0 && mean.is_finite() {
        let swing = nq - mean;
        let u = swing.abs() / h / s.slosh_speed;
        let w = 1.0 / (1.0 + u * u);
        nq = mean + swing / (1.0 + w * dt / s.slosh_damp);
    }
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
    /// The river's flow carried across each column's +x and +z face (m²/s; the map's, through
    /// the faces open to it), and this substep's: kept apart from the law's, which its friction
    /// would otherwise take back.
    px: [f32; CA],
    pz: [f32; CA],
    npx: [f32; CA],
    npz: [f32; CA],
    scale: [f32; CA],
    /// Each face's running mean discharge (m²/s; NaN: none yet, its flow taken), for the slosh's
    /// drag; not saved (a loaded world takes its flows as their means).
    mqx: [f32; CA],
    mqz: [f32; CA],
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
            px: [0.0; CA],
            pz: [0.0; CA],
            npx: [0.0; CA],
            npz: [0.0; CA],
            scale: [1.0; CA],
            mqx: [f32::NAN; CA],
            mqz: [f32::NAN; CA],
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
    /// A generated column's river flow by the world map (m²/s along x and z; `RiverFlow`).
    flow: [[f32; 2]; CA],
    /// The level last written as blocks (NaN: none, or to write).
    drawn: [f32; CA],
    /// How much of the flow the water as generated brings into each column still comes in (0..=1,
    /// `Water::share`): the rest layer's divergence there is scaled by it, so a reach whose
    /// river was cut off upstream keeps none of its springs and sinks (BUGS.md #41). Frozen
    /// while the chunk sleeps, for its account.
    share: [f32; CA],
    /// The calm window's depths and start.
    win: [f32; CA],
    calm_t: f64,
    awake: bool,
    /// Water owed while asleep, then still to come in after waking (m³), and how fast.
    owed: f64,
    rel_rate: f64,
    /// Asleep, the flow each edge face brought into its column as the chunk fell asleep (m/s,
    /// the water's less the rest's by its share; E, W, S, N) and its free columns' whole balance
    /// then (m³/s): a sleeping chunk owes only how far its edges toward awake chunks drift from
    /// what they carried, and keeps its imbalance, so a river running through sleeping chunks
    /// is carried through them, not owed (BUGS.md #41b).
    kept: [[f32; 4]; CA],
    drift: f64,
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
            flow: [[0.0; 2]; CA],
            drawn: [f32::NAN; CA],
            share: [1.0; CA],
            win: [0.0; CA],
            calm_t: -1.0,
            awake: false,
            owed: 0.0,
            rel_rate: 0.0,
            kept: [[0.0; 4]; CA],
            drift: 0.0,
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

/// An outward kick to the water round a point (a blast, docs/EXPLOSIVES.md §3; §2e of
/// docs/WATER_SURFACE_GRID.md): every face between two columns within `reach` (m) of `at` gets
/// discharge away from it, `speed` (m/s) × its depth × (1 − d / reach), d its distance (the
/// columns' water span's, vertically); the columns within it are freed and their chunks woken.
/// Only discharge is added, so it moves water and conserves it; the law then carries the wave on.
/// Held water within `free` (m, across) is freed too: held water stays at its level and would take
/// the wave in, so freed it reflects off the banks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    /// The point (world m).
    pub at: [f64; 3],
    pub reach: f64,
    pub speed: f64,
    pub free: f64,
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
    /// Generated rivers' flow, carried through the water as generated (§2b); none: still.
    flows: Option<RiverFlow>,
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
    /// Kicks given since the last step (`Water::wave`), applied at the next.
    waves: Vec<Wave>,
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
        let flows = map.clone();
        Self::with_runoff(Arc::new(move |f: FaceFrame, x, z| {
            map.local(f.face).discharge_at(x, z)
        }))
        // Untouched rivers' currents (Phase 53 G3b).
        .with_rivers(Arc::new(move |f: FaceFrame, x, z, depth| {
            rivers.local(f.face).river_current(x, z, depth)
        }))
        // ... and the flow they carry, which a dam backs up.
        .with_river_flow(Arc::new(move |f: FaceFrame, x, z| {
            flows.local(f.face).river_flow(x, z)
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

    /// Generated rivers carry the world map's flow (§2b): through the water as generated and the
    /// water as it is alike, so an untouched river stays as it is, and what a dam stops of it
    /// backs up behind it.
    pub fn with_river_flow(mut self, flows: RiverFlow) -> Self {
        self.flows = Some(flows);
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

    /// Whether the water chunk holding face-local column (x, z) is awake (diagnostics, tests).
    pub fn awake_at(&self, f: FaceFrame, x: i32, z: i32) -> bool {
        self.find(f, x, z)
            .is_some_and(|(c, _)| self.chunks[c].awake)
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

    /// A kick to the water round a point, given at the next step ([`Wave`]).
    pub fn wave(&mut self, w: Wave) {
        if w.reach > 0.0 && w.speed != 0.0 && w.at.iter().all(|v| v.is_finite()) {
            self.waves.push(w);
        }
    }

    fn apply_waves(&mut self) {
        for w in std::mem::take(&mut self.waves) {
            let f = FaceFrame::at_height(w.at[1]);
            let ly = f.y(w.at[1]);
            let (cx, cz) = (w.at[0], w.at[2]);
            let r = w.reach.ceil() as i32 + 1;
            let (x0, z0) = (cx.floor() as i32, cz.floor() as i32);
            // the distance (m) from the point to a face-local spot over a water span
            let dist = |px: f64, pz: f64, lo: f32, hi: f32| {
                let dy = (f64::from(lo) - ly).max(ly - f64::from(hi)).max(0.0);
                ((px - cx).powi(2) + (pz - cz).powi(2) + dy * dy).sqrt()
            };
            let mut kicks: Vec<(usize, usize, bool, f32)> = Vec::new();
            for z in z0 - r..=z0 + r {
                for x in x0 - r..=x0 + r {
                    let Some((c, l)) = self.find(f, x, z) else {
                        continue;
                    };
                    let (ga, da) = {
                        let k = &self.chunks[c];
                        (k.w.floor[l], k.w.depth[l])
                    };
                    if ga.is_nan() {
                        continue;
                    }
                    for (d, zf) in [(0usize, false), (2usize, true)] {
                        let Some((c2, l2)) = self.step_col(c, l, d) else {
                            continue;
                        };
                        let (gb, db) = {
                            let k = &self.chunks[c2];
                            (k.w.floor[l2], k.w.depth[l2])
                        };
                        if gb.is_nan() {
                            continue;
                        }
                        // the face's wet height, as the law takes it
                        let h = (ga + da).max(gb + db) - ga.max(gb);
                        if h <= DRY {
                            continue;
                        }
                        let (px, pz) = if zf {
                            (f64::from(x) + 0.5, f64::from(z) + 1.0)
                        } else {
                            (f64::from(x) + 1.0, f64::from(z) + 0.5)
                        };
                        let dd = dist(px, pz, ga.max(gb), ga.max(gb) + h);
                        let flat = ((px - cx).powi(2) + (pz - cz).powi(2)).sqrt();
                        if dd >= w.reach || flat < 1e-6 {
                            continue;
                        }
                        let along = if zf { pz - cz } else { px - cx } / flat;
                        let q = w.speed * (1.0 - dd / w.reach) * along * f64::from(h);
                        kicks.push((c, l, zf, q as f32));
                        kicks.push((c2, l2, zf, 0.0));
                    }
                }
            }
            for (c, l, zf, q) in kicks {
                self.free(c, l);
                if q == 0.0 {
                    continue;
                }
                let k = &mut self.chunks[c].w;
                *if zf { &mut k.qz[l] } else { &mut k.qx[l] } += q;
            }
            // the water round it freed (held water takes a wave in), so the wave runs to the banks
            let rf = w.free.max(w.reach).ceil() as i32;
            for z in z0 - rf..=z0 + rf {
                for x in x0 - rf..=x0 + rf {
                    let flat = ((f64::from(x) + 0.5 - cx).powi(2)
                        + (f64::from(z) + 0.5 - cz).powi(2))
                    .sqrt();
                    if flat > w.free.max(w.reach) {
                        continue;
                    }
                    if let Some((c, l)) = self.find(f, x, z)
                        && self.chunks[c].held[l]
                        && !self.chunks[c].w.floor[l].is_nan()
                    {
                        self.free(c, l);
                    }
                }
            }
        }
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
        self.apply_waves();
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
            let [mut u, mut v] = self.velocity(c, l, false);
            if let Some((along, near)) = self.as_river(c, l) {
                // still the river: the map's current, as much of it as the water still runs,
                // blended to its own as it leaves its generated depth
                let [ru, rv] = k.river[l];
                u += near * (along * ru - u);
                v += near * (along * rv - v);
            }
            if u.hypot(v) >= self.cfg.still {
                out[l] = [u, v];
                any = true;
            }
        }
        any.then_some(out)
    }

    /// Free column (c, l)'s velocity (m/s along x and z) in the water as it is, or as generated
    /// (`rest`): the mean of its faces' discharges, the law's and the carried, over its depth.
    fn velocity(&self, c: usize, l: usize, rest: bool) -> [f32; 2] {
        let k = lay(&self.chunks[c], rest);
        let h = k.depth[l];
        if h <= DRY {
            return [0.0; 2];
        }
        let behind = |d: usize, z: bool| {
            self.step_col(c, l, d).map_or(0.0, |(c2, l2)| {
                let n = lay(&self.chunks[c2], rest);
                if z {
                    n.qz[l2] + n.pz[l2]
                } else {
                    n.qx[l2] + n.px[l2]
                }
            })
        };
        [
            0.5 * (k.qx[l] + k.px[l] + behind(1, false)) / h,
            0.5 * (k.qz[l] + k.pz[l] + behind(3, true)) / h,
        ]
    }

    /// Whether free column (c, l) is still its generated river's, and how much: an edit frees the
    /// water near it, but the river there runs on as drawn, and its current should be the map's
    /// as its held neighbours' is, not the carried flow over its depth (often a tenth of it: the
    /// map's is Manning's on the course's mean fall, BUGS.md #42). Some((along, near)): `along`,
    /// how much of the rest's own speed the water still runs at downstream (0..=1: a dam's pool
    /// filling runs less of it); `near`, how near its depth still is its generated depth (1 as
    /// drawn, 0 by `riverCurrentBandM` off it: a pool risen behind a dam, a drained bed). None for
    /// water that isn't a river's: no map current, out of the rest (`share`: the reach below a
    /// dam), or a rest that runs no speed (a lake).
    fn as_river(&self, c: usize, l: usize) -> Option<(f32, f32)> {
        let k = &self.chunks[c];
        if k.river[l] == [0.0; 2] || k.share[l] <= 0.0 || Self::out_of_rest(k, l) {
            return None;
        }
        let [ru, rv] = self.velocity(c, l, true);
        let rest = ru.hypot(rv);
        if rest < self.cfg.still {
            return None;
        }
        let [u, v] = self.velocity(c, l, false);
        let along = ((u * ru + v * rv) / (rest * rest)).clamp(0.0, 1.0);
        let near =
            (1.0 - (k.w.depth[l] - k.r.depth[l]).abs() / self.cfg.river_band).clamp(0.0, 1.0);
        Some((along, near))
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
                k.flow[l] = Self::flow_of(self.flows.as_ref(), f, x, z, k.r.depth[l]);
                if let Some(rivers) = &self.rivers
                    && let Some([u, v]) = rivers(f, x, z, f64::from(k.w.depth[l]))
                {
                    k.river[l] = [u as f32, v as f32];
                    self.fresh.insert(c);
                }
            }
        }
    }

    /// A generated column's river flow (none where it holds no water).
    fn flow_of(flows: Option<&RiverFlow>, f: FaceFrame, x: i32, z: i32, depth: f32) -> [f32; 2] {
        match flows {
            Some(flows) if depth > DRY => {
                flows(f, x, z).map_or([0.0; 2], |[u, v]| [u as f32, v as f32])
            }
            _ => [0.0; 2],
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
                    let np = self.carried(c, l, d, zf, rest);
                    let mut mean = f32::NAN;
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
                                    if !rest {
                                        let m = if zf { ka.mqz[l] } else { ka.mqx[l] };
                                        mean = if m.is_finite() { m } else { q };
                                    }
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
                                        mean,
                                        dt,
                                    )
                                }
                            }
                            None => 0.0,
                        }
                    };
                    let k = &mut self.chunks[c];
                    let k = if rest { &mut k.r } else { &mut k.w };
                    let mean = if mean.is_finite() {
                        mean + (nq - mean) * (dt / self.cfg.slosh_mean).min(1.0)
                    } else {
                        f32::NAN
                    };
                    if zf {
                        k.nqz[l] = nq;
                        k.npz[l] = np;
                        k.mqz[l] = mean;
                    } else {
                        k.nqx[l] = nq;
                        k.npx[l] = np;
                        k.mqx[l] = mean;
                    }
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
                        (k.nqx[l] + k.npx[l]).max(0.0) + (k.nqz[l] + k.npz[l]).max(0.0),
                    )
                };
                if floor.is_nan() {
                    continue;
                }
                if let Some((c2, l2)) = self.step_col(c, l, 1) {
                    let n = lay(&self.chunks[c2], rest);
                    out += (-(n.nqx[l2] + n.npx[l2])).max(0.0);
                }
                if let Some((c2, l2)) = self.step_col(c, l, 3) {
                    let n = lay(&self.chunks[c2], rest);
                    out += (-(n.nqz[l2] + n.npz[l2])).max(0.0);
                }
                let out = out * dt;
                let k = &mut self.chunks[c];
                let k = if rest { &mut k.r } else { &mut k.w };
                k.scale[l] = if out > d { d / out } else { 1.0 };
            }
        }
    }

    /// The river's flow carried across the face of column (c, l) toward `d` (E or S) in one layer
    /// (m²/s, + toward `d`), before the limiter: the map's (`Chunk::flow`, the two columns'
    /// averaged), through the face as deep as the water as generated has it; in the water as it is,
    /// in proportion to how much of that depth it still has there (none through a wall or a block
    /// risen over the water, less where the water fell), never more. Untouched, the two layers
    /// carry the same, so the river stays as it is; a dam stops it in the water as it is alone.
    fn carried(&self, c: usize, l: usize, d: usize, zf: bool, rest: bool) -> f32 {
        let Some((c2, l2)) = self.step_col(c, l, d) else {
            return 0.0;
        };
        let (ka, kb) = (&self.chunks[c], &self.chunks[c2]);
        let ax = usize::from(zf);
        let p = 0.5 * (ka.flow[l][ax] + kb.flow[l2][ax]);
        if p == 0.0 {
            return 0.0;
        }
        // the water the sending column has over the face (upwind: none from a column whose level
        // is under the other's floor, as a hollow's under the lip it would be pushed over)
        let through = |la: &Layer, lb: &Layer| -> f32 {
            let (ga, gb) = (la.floor[l], lb.floor[l2]);
            let from = if p > 0.0 {
                ga + la.depth[l]
            } else {
                gb + lb.depth[l2]
            };
            let h = from - ga.max(gb);
            if h.is_finite() { h.max(0.0) } else { 0.0 }
        };
        let hr = through(&ka.r, &kb.r);
        if hr <= DRY {
            return 0.0;
        }
        if rest {
            return p;
        }
        p * (through(&ka.w, &kb.w) / hr).min(1.0)
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
                    let rawp = if zf { la.npz[l] } else { la.npx[l] };
                    let s = if raw + rawp > 0.0 {
                        if ka.awake { la.scale[l] } else { 1.0 }
                    } else if kb.awake {
                        lb.scale[l2]
                    } else {
                        1.0
                    };
                    let (nq, np) = (raw * s, rawp * s);
                    let old = if zf {
                        la.qz[l] + la.pz[l]
                    } else {
                        la.qx[l] + la.px[l]
                    };
                    let (wa, wb) = (ka.awake, kb.awake);
                    let k = &mut self.chunks[c];
                    let k = if rest { &mut k.r } else { &mut k.w };
                    if zf {
                        k.qz[l] = nq;
                        k.pz[l] = np;
                    } else {
                        k.qx[l] = nq;
                        k.px[l] = np;
                    }
                    if c != c2 && wa != wb {
                        let total = nq + np;
                        edges.push((c, l, zf, total, (total - old).abs() > jump));
                    }
                }
            }
        }
        edges
    }

    /// Whether column `l` of chunk `k` is out of the water as generated: no rest there (a column an
    /// edit opened), or generated water whose floor has been built up to or over the level it stood
    /// at (a dam across a river). Such a column changes by its own flows alone: the rest layer's
    /// divergence there (the implicit spring of a drawn river's step, or of a reach whose flow
    /// varies) is water that can't be there any more, and applied to the top of a dam it poured
    /// water onto it for ever (BUGS.md #40). Its neighbours keep their rest flows across its faces,
    /// so the reach above a dam still backs up and the one below still drains. Read from the
    /// column's state each substep, so a world saved with water standing on a dam heals on load.
    fn out_of_rest(k: &Chunk, l: usize) -> bool {
        let (floor, depth) = (k.r.floor[l], k.r.depth[l]);
        floor.is_nan() || (depth > DRY && k.w.floor[l] >= floor + depth - DRY)
    }

    /// How much of the flow the water as generated brings into column (c, l) still comes in, by
    /// the water as it is (0..=1): over the faces the rest's flow enters by, the flow in (no more
    /// than the rest's on each) over the rest's. The rest layer's divergence at the column (its
    /// implicit spring or sink: a drawn step's lip pours, a junction or a narrowing carries more
    /// out than in, a lake's mouth takes the river in) is scaled by it, as a river's own flow
    /// passes on what reaches it: untouched it is 1 and the river rests exactly as drawn; below a
    /// dam it falls to 0 as the reach drains, so the reach keeps no spring to refill it, nor a
    /// lake at its mouth a sink to drain it, and it dries completely (BUGS.md #41); a column the
    /// rest's flow doesn't enter (a river's head at the loaded world's edge) keeps its spring.
    /// None out of the water as generated (`out_of_rest`).
    fn share(&self, c: usize, l: usize) -> f32 {
        let k = &self.chunks[c];
        if Self::out_of_rest(k, l) {
            return 0.0;
        }
        // over the faces the rest's flow enters by, weighted by it: whether the column it comes
        // from is still in the rest. A column cut off upstream (a dam, `out_of_rest`) takes the
        // reach below it out with it, down the rest's own flow, as far as the river it carried
        // went on (to a junction whose other river brings most of the flow, or the lake it ran
        // into); a wave, a stone in the bed or the slosh of an edit moves no column out.
        let (mut got, mut want) = (0.0f32, 0.0f32);
        for d in 0..4 {
            let Some((c2, l2)) = self.step_col(c, l, d) else {
                continue;
            };
            let n = &self.chunks[c2];
            // the rest's flow into (c, l) across this face
            let q = match d {
                0 => -(k.r.qx[l] + k.r.px[l]),
                1 => n.r.qx[l2] + n.r.px[l2],
                2 => -(k.r.qz[l] + k.r.pz[l]),
                _ => n.r.qz[l2] + n.r.pz[l2],
            };
            if q <= 0.0 {
                continue;
            }
            let from = if n.w.floor[l2].is_nan() {
                0.0
            } else {
                n.share[l2]
            };
            want += q;
            got += q * from;
        }
        let ratio = if want <= 1e-5 { 1.0 } else { got / want };
        // latched: the rest whole or none, so a river nobody cut keeps it exactly
        let was = k.share[l];
        if was > 0.0 && ratio < self.cfg.rest_cut {
            0.0
        } else if was == 0.0 && ratio > self.cfg.rest_back {
            1.0
        } else {
            was
        }
    }

    /// The river's flow out of column (c, l) past the grid (m/s): across each face its map's
    /// river runs out by where the rest has no flow (the loaded world's edge, a wall the river
    /// was drawn into), as deep as its water still is of its rest. A column in the rest has it in
    /// its rest's divergence (a sink); one out of it (`share`) keeps it, so a reach cut off
    /// upstream still runs on out of the world as its river did, and drains.
    fn runs_on(&self, c: usize, l: usize) -> f32 {
        let k = &self.chunks[c];
        let (rd, wd) = (k.r.depth[l], k.w.depth[l]);
        if Self::out_of_rest(k, l) || rd <= DRY {
            return 0.0;
        }
        let [fx, fz] = k.flow[l];
        let mut out = 0.0;
        for (d, v) in [(0, fx), (1, -fx), (2, fz), (3, -fz)] {
            if v <= 0.0 {
                continue;
            }
            let rest = match (d, self.step_col(c, l, d)) {
                (_, None) => 0.0,
                (0, _) => k.r.qx[l] + k.r.px[l],
                (2, _) => k.r.qz[l] + k.r.pz[l],
                (1, Some((c2, l2))) => {
                    let n = &self.chunks[c2].r;
                    n.qx[l2] + n.px[l2]
                }
                (_, Some((c2, l2))) => {
                    let n = &self.chunks[c2].r;
                    n.qz[l2] + n.pz[l2]
                }
            };
            if rest.abs() < 1e-6 {
                out += v;
            }
        }
        out * (wd / rd).min(1.0)
    }

    /// Falling asleep: keeps the flow each edge face brings into its column (the water's less the
    /// rest's by the column's share, as the account counts it) and its balance (BUGS.md #41b).
    fn keep_edges(&mut self, c: usize, mut bal: f64) {
        let mut kept = [[0.0f32; 4]; CA];
        for (l, kl) in kept.iter_mut().enumerate() {
            let k = &self.chunks[c];
            if k.w.floor[l].is_nan() || k.held[l] {
                continue;
            }
            let share = k.share[l];
            for (d, kd) in kl.iter_mut().enumerate() {
                let Some((c2, l2)) = self.step_col(c, l, d) else {
                    continue;
                };
                if c2 == c {
                    continue;
                }
                let n = &self.chunks[c2];
                let (w, r) = match d {
                    0 => (-(k.w.qx[l] + k.w.px[l]), -(k.r.qx[l] + k.r.px[l])),
                    2 => (-(k.w.qz[l] + k.w.pz[l]), -(k.r.qz[l] + k.r.pz[l])),
                    1 => (n.w.qx[l2] + n.w.px[l2], n.r.qx[l2] + n.r.px[l2]),
                    _ => (n.w.qz[l2] + n.w.pz[l2], n.r.qz[l2] + n.r.pz[l2]),
                };
                *kd = w - share * r;
                // An edge toward a chunk already asleep carries what that one kept across it
                // from now on (no one steps it): its flow is this chunk's too, or a swing caught
                // at different moments on the two sides would make or lose water (BUGS.md #41b).
                if !n.awake && !n.held[l2] && !n.w.floor[l2].is_nan() {
                    let theirs = -n.kept[l2][d ^ 1];
                    bal += f64::from(theirs - *kd);
                    *kd = theirs;
                }
            }
        }
        let k = &mut self.chunks[c];
        k.kept = kept;
        k.drift = bal;
    }

    /// A layer's net flow into column (c, l) (m/s).
    fn net(&self, c: usize, l: usize, rest: bool) -> f32 {
        let k = lay(&self.chunks[c], rest);
        let mut net = -k.qx[l] - k.px[l] - k.qz[l] - k.pz[l];
        if let Some((c2, l2)) = self.step_col(c, l, 1) {
            let n = lay(&self.chunks[c2], rest);
            net += n.qx[l2] + n.px[l2];
        }
        if let Some((c2, l2)) = self.step_col(c, l, 3) {
            let n = lay(&self.chunks[c2], rest);
            net += n.qz[l2] + n.pz[l2];
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
            let dir = match (wa, zf) {
                (true, false) => 1,
                (true, true) => 3,
                (false, false) => 0,
                (false, true) => 2,
            };
            // the rest's flow counted as far as it still comes in at the sleeper's column (its
            // share, frozen as it fell asleep): a drained reach asleep owes nothing for the river
            // that no longer runs through it (BUGS.md #41)
            if !self.chunks[col.0].held[col.1] {
                let share = self.chunks[col.0].share[col.1];
                let kept = self.chunks[col.0].kept[col.1][dir];
                self.chunks[sleeper].owed +=
                    f64::from(sign * (nq - share * rq) - kept) * f64::from(dt);
            }
            if jump {
                self.wake(sleeper);
            }
        }
        // the water balance, for awake free columns (held ones keep their level)
        let t = f64::from(dt) + self.t;
        let cfg = self.cfg;
        let mut slept: Vec<(usize, f64)> = Vec::new();
        for c in 0..n {
            if !self.chunks[c].awake {
                continue;
            }
            let mut moved = 0.0f32;
            let mut bal = 0.0f64;
            for l in 0..CA {
                let k = &self.chunks[c];
                if k.w.floor[l].is_nan() || k.held[l] {
                    continue;
                }
                let share = self.share(c, l);
                let net = self.net(c, l, false)
                    - if share > 0.0 {
                        share * self.net(c, l, true)
                    } else {
                        self.runs_on(c, l)
                    };
                let k = &mut self.chunks[c];
                k.share[l] = share;
                bal += f64::from(net);
                k.w.depth[l] = (k.w.depth[l] + net * dt).max(0.0);
                if k.calm_t >= 0.0 {
                    // as a share of the band; in a reach out of the rest (cut off upstream), of
                    // a tenth of a shallow column's water too: its film still draining off the
                    // bed is not calm, however slowly its level falls (BUGS.md #41)
                    let band = if share == 0.0 {
                        (0.1 * k.w.depth[l].max(k.win[l]))
                            .clamp(cfg.sleep_band * 0.1, cfg.sleep_band)
                    } else {
                        cfg.sleep_band
                    };
                    moved = moved.max((k.w.depth[l] - k.win[l]).abs() / band);
                    // nor is a film out of the rest still running off at all: asleep it would
                    // keep its flow and never drain (BUGS.md #41b)
                    if share == 0.0
                        && k.w.depth[l] > DRY
                        && k.w.depth[l] < cfg.sleep_band
                        && k.win[l] - k.w.depth[l] > FILM_FALL
                    {
                        moved = 2.0;
                    }
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
                // what it owes past the water it has can't be taken (as an awake column's depth
                // stops at none): dropped, or a dry chunk would stay awake for ever
                if k.owed < 0.0 {
                    let left: f64 = (0..CA)
                        .filter(|&l| !k.w.floor[l].is_nan() && !k.held[l])
                        .map(|l| f64::from(k.w.depth[l]))
                        .sum();
                    if left <= 1e-6 {
                        k.owed = 0.0;
                    }
                }
            }
            // the calm window: asleep once still within the band for CALM_S
            if k.calm_t < 0.0 || moved > 1.0 {
                k.calm_t = t;
                k.win = k.w.depth;
            } else if t - k.calm_t >= cfg.calm_s && k.owed == 0.0 {
                k.awake = false;
                slept.push((c, bal));
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
        for (c, bal) in slept {
            self.keep_edges(c, bal);
        }
        // a sleeping chunk keeps its imbalance
        for k in self.chunks.iter_mut() {
            if !k.awake && k.drift != 0.0 {
                k.owed += k.drift * f64::from(dt);
            }
        }
        // a sleeping chunk whose account would move its water half the wake band wakes
        for c in 0..n {
            let k = &self.chunks[c];
            if k.awake || k.owed == 0.0 {
                continue;
            }
            let wet = k.wet().max(1) as f64;
            // or owes half the water it has (a film running off, asleep on its kept flows,
            // would otherwise never pay: BUGS.md #41b)
            let has = || -> f64 {
                (0..CA)
                    .filter(|&l| !k.w.floor[l].is_nan() && !k.held[l])
                    .map(|l| f64::from(k.w.depth[l]))
                    .sum()
            };
            if k.owed.abs() / wet > f64::from(self.cfg.wake_band) * 0.5
                || (k.owed < 0.0 && {
                    let h = has();
                    h > OWED_MIN && -k.owed > 0.5 * h
                })
            {
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
                    if !v[4].is_nan() {
                        let (x, z) = k.xz(l);
                        k.flow[l] = Self::flow_of(self.flows.as_ref(), f, x, z, v[5]);
                        // its river's current at its generated depth, as when it was held, so
                        // a saved world's freed river runs as the river round it on load
                        k.river[l] = match &self.rivers {
                            Some(rivers) if v[5] > DRY => rivers(f, x, z, f64::from(v[5]))
                                .map_or([0.0; 2], |[u, w]| [u as f32, w as f32]),
                            _ => [0.0; 2],
                        };
                    }
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
