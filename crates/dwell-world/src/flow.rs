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
    /// An untouched river's current is its speed by the law times this (then at most
    /// `river_current_max_ms`).
    pub river_current_scale: f64,
    /// The fastest an untouched river's current runs (m/s).
    pub river_current_max_ms: f64,
    /// How far up and down its course (m) an untouched river's fall is averaged for its current.
    pub river_fall_reach_m: f64,
    /// A freed column still in the rest shows the map's current, blended to its own as its depth
    /// leaves its generated depth by up to this (m).
    pub river_current_band_m: f64,
    /// The liquids the grid moves; the first is the one it draws.
    pub liquids: &'static [LiquidFlow],
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod flow_generated {
    include!("flow.gen.rs");
}
pub use flow_generated::FLOW;

impl FlowData {
    /// An untouched river's current speed (m/s; docs/WATER_SURFACE_GRID.md §2c) where its water is
    /// `depth` m deep on a fall of `fall` (m per m) carrying `unit_flow` m²/s: Manning's law with
    /// the drawn liquid's roughness, or the flow over the depth where faster (a level reach runs
    /// too), times `river_current_scale`, at most `river_current_max_ms`.
    pub fn river_speed(&self, fall: f64, unit_flow: f64, depth: f64) -> f64 {
        let n = self.liquids.first().map_or(0.03, |l| l.manning);
        let manning = depth.powf(2.0 / 3.0) * fall.max(0.0).sqrt() / n;
        let carried = unit_flow.max(0.0) / depth;
        (manning.max(carried) * self.river_current_scale).min(self.river_current_max_ms)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Owner, 2026-11-09: "double the river current speeds generally". A river 1 m deep on a fall
    /// of 0.0005 runs 0.745 m/s by Manning's law (n 0.03); its current is twice that, and the cap
    /// is 3 m/s, the old 1.5 doubled. Red before: 0.745 m/s, capped at 1.5.
    #[test]
    fn a_rivers_current_runs_twice_its_law_up_to_three_metres_a_second() {
        let law = 0.0005f64.sqrt() / 0.03;
        let speed = FLOW.river_speed(0.0005, 0.0, 1.0);
        assert!(
            (speed - 2.0 * law).abs() < 1e-9,
            "{speed} m/s, the law {law}"
        );
        // A level reach runs at twice its flow over its depth (0.2 m²/s over 1 m).
        let level = FLOW.river_speed(0.0, 0.2, 1.0);
        assert!((level - 0.4).abs() < 1e-9, "a level reach runs {level} m/s");
        // A steep chute is held at the cap.
        assert_eq!(FLOW.river_current_max_ms, 3.0);
        assert_eq!(FLOW.river_speed(0.05, 0.0, 1.0), 3.0);
    }

    /// The scale is data: a fixture of scale 1 runs the plain law.
    #[test]
    fn the_scale_is_the_datas() {
        let plain = FlowData {
            river_current_scale: 1.0,
            ..FLOW
        };
        let law = 0.0005f64.sqrt() / 0.03;
        assert!((plain.river_speed(0.0005, 0.0, 1.0) - law).abs() < 1e-9);
    }
}
