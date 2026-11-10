//! How moving water shows its flow (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3d): ripples carried
//! by the current, foam streaks along it and particles drifting on it, per liquid
//! (`shared/look/waterflow.json`, generated into `waterflow.gen.rs`). The current is the client's
//! current table's (`dwell_world::currents`); the renderer and the game read these looks.

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("waterflow.gen.rs");
}
pub use generated::{LIQUID_FLOWS, WATER_FLOW};

/// `shared/look/waterflow.json`'s tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterFlowTunables {
    /// Columns square the flow texture holds around the eye (a power of two; it wraps).
    pub window_columns: u32,
    /// The fastest current the texture holds (m/s).
    pub range_ms: f64,
    /// The flow shows fully within this of the eye, and fades out by `fade_to_m` (m).
    pub fade_from_m: f64,
    pub fade_to_m: f64,
}

/// A liquid's flow look (`liquids` in `shared/look/waterflow.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidFlowLook {
    /// The registry fluid's name, e.g. `dwell:water`.
    pub fluid: &'static str,
    /// Ripples: how far they are carried per metre the water moves, and how long before a ripple
    /// blends in afresh (s).
    pub advect: f64,
    pub cycle_s: f64,
    /// Foam streaks: the current where they begin and where they are full (m/s); sRGB
    /// `0xRRGGBB`; strength at full (0–1); a streak's size along and across the flow (m).
    pub foam_from_ms: f64,
    pub foam_to_ms: f64,
    pub foam_color: u32,
    pub foam_strength: f64,
    pub foam_length_m: f64,
    pub foam_width_m: f64,
    /// Particles (flecks): at most this many near the eye (`particle_cpu_count` on a CPU
    /// adapter), spawned within `particle_radius_m` in currents of at least `particle_from_ms`,
    /// more often and more opaque up to `particle_full_ms`; each `particle_size_m` half-width
    /// across the flow and up to `particle_stretch` times as long along it (at `particle_full_ms`),
    /// this colour (sRGB) at most `particle_opacity`, drifting for `particle_life_s`, faded in
    /// over its first `particle_fade_in_s` and out over its last `particle_fade_out_s`; its edge
    /// broken by `particle_ragged` (0–1) in `particle_lumps` lobes; faded out where it is under
    /// `particle_min_pixels` wide on screen.
    pub particle_count: u32,
    pub particle_cpu_count: u32,
    pub particle_from_ms: f64,
    pub particle_full_ms: f64,
    pub particle_size_m: f64,
    pub particle_stretch: f64,
    pub particle_color: u32,
    pub particle_opacity: f64,
    pub particle_life_s: f64,
    pub particle_fade_in_s: f64,
    pub particle_fade_out_s: f64,
    pub particle_radius_m: f64,
    pub particle_ragged: f64,
    pub particle_lumps: u32,
    pub particle_min_pixels: f64,
    /// Shore foam (WATER_RENDERING.md §14): a solid contact line `shore_line_m` wide where the
    /// water meets land or a block, and `shore_bands` soft bands parallel to the shore, each born
    /// at the edge every `shore_period_s`, rolling out over the fluid's foam depth while it thins
    /// (from `shore_width_m`) and fades; slow noise shifts each band's timing and width along the
    /// shore by up to `shore_wobble` (0–1) without cutting holes; at most `shore_strength`.
    pub shore_bands: u32,
    pub shore_period_s: f64,
    pub shore_line_m: f64,
    pub shore_width_m: f64,
    pub shore_strength: f64,
    pub shore_wobble: f64,
}

/// Liquid `fluid`'s flow look, if it has one (none: its water shows no flow).
pub fn flow_look(fluid: &str) -> Option<&'static LiquidFlowLook> {
    LIQUID_FLOWS.iter().find(|l| l.fluid == fluid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_has_a_flow_look_and_an_unknown_liquid_none() {
        let w = flow_look("dwell:water").expect("water's flow look");
        assert!(w.foam_to_ms > w.foam_from_ms);
        assert!(flow_look("test:nothing").is_none());
        assert!(WATER_FLOW.window_columns.is_power_of_two());
        assert!(WATER_FLOW.fade_to_m <= f64::from(WATER_FLOW.window_columns) / 2.0);
    }
}
