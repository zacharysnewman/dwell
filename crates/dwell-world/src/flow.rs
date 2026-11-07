//! The water grid's data (docs/WATER_SURFACE_GRID.md §4, ADR 0060): the settings of the surface grid
//! (`dwell-sim` `water.rs`) and each liquid's rules, and the weir law the terrain generator sizes
//! chute streams by. Data: `shared/fluids/flow.json` → `flow.gen.rs` (`node shared/fluids/gen.mjs`).

/// A liquid's rules (`shared/fluids/flow.json` `liquids`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidFlow {
    pub block: &'static str,
    /// Manning's roughness (s/m^⅓) of the ground under it.
    pub manning: f64,
    /// m^½/s: a free sill passes `weir × width × head^1.5` m³/s (the generator's chute streams).
    pub weir: f64,
    /// The block the generator draws its chute streams as.
    pub running: &'static str,
}

/// The water grid's data (`shared/fluids/flow.json`; what each field means is in its `$comment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowData {
    /// The default water speed (the server's `water_speed`), × game time.
    pub speed: f64,
    /// Server ticks per water tick.
    pub tick_every: u32,
    /// The face law's θ-weighting.
    pub theta: f64,
    /// A substep's share of a wave's crossing time.
    pub cfl: f64,
    /// The most substeps a water tick takes.
    pub max_substeps: u32,
    /// The most column-substeps a water tick may take.
    pub step_budget: u32,
    /// The most column-substeps a run of the water's own thread takes (ADR 0062).
    pub thread_step_budget: u32,
    /// The most water time (s) the water's own thread owes before the rest is dropped.
    pub max_owed_s: f64,
    /// A chunk sleeps once its levels held within this (m) for `calm_s`.
    pub sleep_band_m: f64,
    /// A sleeping chunk wakes when its account would move its water this far (m).
    pub wake_band_m: f64,
    /// The calm window (water s).
    pub calm_s: f64,
    /// A woken chunk takes what it owes over this long (water s).
    pub release_s: f64,
    /// A shared face's change in a substep that wakes a sleeper (m²/s).
    pub jump_m2s: f64,
    /// A free column this far off its rest frees the held water beside it (m).
    pub release_m: f64,
    /// A free column whose rest inflow comes less than this share from columns in the rest leaves
    /// the rest (BUGS.md #41).
    pub rest_cut: f64,
    /// ... and one whose comes more than this share from them returns.
    pub rest_back: f64,
    /// An edit frees the water within this many columns.
    pub edit_reach: u32,
    /// How far past half a drawn step (fill steps) a level moves before it is drawn again.
    pub fill_hysteresis: f64,
    /// Water slower than this (m/s) has no current.
    pub current_still_ms: f64,
    /// Water shallower than this (m) has no current.
    pub current_depth_m: f64,
    /// The fastest an untouched river's current runs (m/s).
    pub river_current_max_ms: f64,
    /// How far up and down its course (m) an untouched river's fall is averaged for its current.
    pub river_fall_reach_m: f64,
    /// The liquids the grid moves; the first is the one it draws.
    pub liquids: &'static [LiquidFlow],
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod flow_generated {
    include!("flow.gen.rs");
}
pub use flow_generated::FLOW;

/// What a free sill `width` m wide passes (m³/s) with water standing `head` m over it, for a
/// liquid of weir coefficient `weir`: the weir law, `weir × width × head^1.5`.
pub fn sill_flow(weir: f64, width: f64, head: f64) -> f64 {
    weir * width * head.max(0.0).powf(1.5)
}

/// Water's weir coefficient (`dwell:water` in `flow.json`).
pub fn water_weir() -> f64 {
    FLOW.liquids
        .iter()
        .find(|l| l.block == "dwell:water")
        .map_or(0.0, |l| l.weir)
}
