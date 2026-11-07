//! How waterfalls look (docs/WATER_SURFACE_GRID.md §2e, Phase 53 G6): a falling sheet from the lip
//! to where it lands, foam and spray where it lands, per liquid (`shared/look/waterfalls.json`,
//! generated into `waterfalls.gen.rs`). The falls themselves are `dwell_world::falls`'; the client's
//! renderer draws them with these looks (`render/flames.rs`'s fall quads).

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("waterfalls.gen.rs");
}
pub use generated::{FALL_LOOKS, WATERFALLS};

/// `shared/look/waterfalls.json`'s tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterfallTunables {
    /// Falls are drawn within this of the eye (m), the nearest `max_falls`.
    pub draw_m: f64,
    pub max_falls: u32,
    /// How often the falls near the eye are found again (s).
    pub refresh_s: f64,
}

/// A liquid's falls look (`liquids` in `shared/look/waterfalls.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallLook {
    /// The registry fluid's name, e.g. `dwell:water`.
    pub fluid: &'static str,
    /// The sheet: sRGB `0xRRGGBB`; its opacity at no flow and at full flow; pale streaks across a
    /// metre; how fast they run per m/s of the fall; the pieces its curve is drawn in.
    pub sheet_color: u32,
    pub sheet_opacity_thin: f64,
    pub sheet_opacity_full: f64,
    pub sheet_streaks_per_m: f64,
    pub sheet_scroll: f64,
    pub sheet_segments: u32,
    /// The foam where it lands: sRGB, opacity at full flow, its reach beyond the sheet (m).
    pub foam_color: u32,
    pub foam_opacity: f64,
    pub foam_radius_m: f64,
    /// The spray: puffs per metre of width at full flow (`spray_cpu_count` on a CPU adapter), each
    /// this size (m), rising this high (m) over its life (s); sRGB and opacity.
    pub spray_count: u32,
    pub spray_cpu_count: u32,
    pub spray_size_m: f64,
    pub spray_height_m: f64,
    pub spray_life_s: f64,
    pub spray_color: u32,
    pub spray_opacity: f64,
}

/// Liquid `fluid`'s falls look, if it has one (none: its falls aren't drawn).
pub fn fall_look(fluid: &str) -> Option<usize> {
    FALL_LOOKS.iter().position(|l| l.fluid == fluid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_has_a_falls_look_and_an_unknown_liquid_none() {
        let w = &FALL_LOOKS[fall_look("dwell:water").expect("water's falls look")];
        assert!(w.sheet_opacity_full >= w.sheet_opacity_thin);
        assert!(w.spray_cpu_count <= w.spray_count);
        assert!(fall_look("test:nothing").is_none());
        const { assert!(WATERFALLS.max_falls > 0) };
    }
}
