//! How the view looks from inside a fluid (docs/WATER_RENDERING.md §10, the waterline): where a
//! pixel's view ray is decided under the surface (`lens_m` out from the eye), the line's waves,
//! the meniscus along it, and the fog over the distance in the fluid, per fluid
//! (`shared/look/underwater.json`, generated into `underwater.gen.rs`). The renderer packs them
//! into the fluid table beside the block registry's fluid look.

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("underwater.gen.rs");
}
pub use generated::{DEFAULT_UNDERWATER, UNDERWATER_LOOKS};

/// A fluid's underwater look (`fluids` in `shared/look/underwater.json`, or its `default`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnderwaterLook {
    /// The registry fluid's name, e.g. `dwell:water` (empty for the default).
    pub fluid: &'static str,
    /// How far out along a pixel's view ray it is decided under the surface (m).
    pub lens_m: f64,
    /// The line's waves: height (m), wavelength (m), cycles a second.
    pub line_amplitude_m: f64,
    pub line_wavelength_m: f64,
    pub line_speed: f64,
    /// The bright band along the line: sRGB `0xRRGGBB`, strength (0–1), half-width (px).
    pub meniscus_color: u32,
    pub meniscus_strength: f64,
    pub meniscus_width_px: f64,
    /// The fog: none over the first `fog_clear_m` metres, then the fluid's rates × `fog_density`.
    pub fog_clear_m: f64,
    pub fog_density: f64,
}

/// Fluid `fluid`'s underwater look (the default when it has none of its own).
pub fn underwater_look(fluid: &str) -> &'static UnderwaterLook {
    UNDERWATER_LOOKS
        .iter()
        .find(|l| l.fluid == fluid)
        .unwrap_or(&DEFAULT_UNDERWATER)
}

/// How far from a fluid's surface the eye can be and still see the line (m): the furthest a
/// view ray's lens point rises or falls, plus the line's waves, over every look.
pub fn line_reach_m() -> f64 {
    UNDERWATER_LOOKS
        .iter()
        .chain([&DEFAULT_UNDERWATER])
        .map(|l| l.lens_m + l.line_amplitude_m)
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_has_its_own_look_and_an_unknown_fluid_the_default() {
        let w = underwater_look("dwell:water");
        assert_eq!(w.fluid, "dwell:water");
        assert!(
            w.fog_clear_m > 0.0 && w.fog_density < 1.0,
            "clear water reads far"
        );
        assert_eq!(underwater_look("test:nothing"), &DEFAULT_UNDERWATER);
        assert!(line_reach_m() >= w.lens_m + w.line_amplitude_m);
    }
}
