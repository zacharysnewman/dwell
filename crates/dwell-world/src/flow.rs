//! Water's flow data and its one opening law (docs/WATER_FLOW.md §4, ADR 0056), shared by the
//! water engine (`dwell-sim` `water.rs`) and the terrain generator, which sizes generated rivers'
//! lips by it so they rest where the engine balances them (ADR 0057, WATER_FLOW.md §17). Data:
//! `shared/fluids/flow.json` → `flow.gen.rs` (`node shared/fluids/gen.mjs`).

use dwell_blocks::{BLOCKS, StateId, info};

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
    /// And the least share of the river's flow it must be.
    pub river_change_share: f64,
    /// And how long (water s) it must last before it is sent.
    pub river_change_hold_s: f64,
    /// A map lake's head over its outlet (m): how fast its outflow follows a change coming in.
    pub lake_outlet_head: f64,
    /// How far past half a step (fill steps) a level must move before its surface is drawn at
    /// another fill.
    pub fill_hysteresis: f64,
    /// How near its level as found (m) a body found at rest is drawn at that level.
    pub found_snap_m: f64,
    pub liquids: &'static [LiquidFlow],
    /// Block name → m³/s per m² of face per m of head.
    pub leaks: &'static [(&'static str, f64)],
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod flow_generated {
    include!("flow.gen.rs");
}
pub use flow_generated::FLOW;

/// The weir coefficient of a liquid state, if it flows.
pub fn weir_of(m: StateId) -> Option<f64> {
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

/// g (m/s²).
pub const G: f64 = 9.81;

/// The discharge coefficient of an opening, from a liquid's weir coefficient: a free sill's
/// `weir × head^1.5` is the opening law summed up its face, `C × (2/3) × √(2g) × head^1.5`.
pub fn discharge(weir: f64) -> f64 {
    weir / (2.0 / 3.0 * (2.0 * G).sqrt())
}

/// The flow (m³/s per m of width) through a vertical face from `lo` to `hi` (local m) for water
/// standing at `high` on one side and `low` on the other (`-∞`: none): below `low` the face is
/// drowned, every part of it pushed by the difference of the levels, `C × area × √(2 g Δh)`; between
/// `low` and `high` the water pours freely, each part by its own depth under `high` (summed, the
/// weir law). Hydrostatics: the pressure difference across a submerged opening is `ρ g Δh` at
/// every depth (docs/WATER_FLOW.md §4).
pub fn face_flow(c: f64, high: f64, low: f64, lo: f64, hi: f64) -> f64 {
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
