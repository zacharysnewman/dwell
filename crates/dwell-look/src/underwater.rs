//! How the view looks from inside a fluid (docs/WATER_RENDERING.md §10, the waterline): where a
//! pixel's view ray is decided under the surface (`lens_m` out from the eye), the line's waves,
//! the band along it, and the fog over the distance in the fluid, per fluid
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
    /// How far ahead of the eye (m, along the camera's forward axis) the lens plane is: a pixel is
    /// under the surface when its view ray's point on that plane is (so the line is straight).
    pub lens_m: f64,
    /// The line's waves: height (m), wavelength (m), cycles a second.
    pub line_amplitude_m: f64,
    pub line_wavelength_m: f64,
    pub line_speed: f64,
    /// The band: the surface's slab seen edge-on under the line, `band_depth_m` deep on the lens
    /// plane; its colour from its surroundings times `band_tint` (sRGB `0xRRGGBB`), its texture's
    /// strength, its glints; the dark rim along its top (px, shade) and the bright line under it
    /// (px, glow).
    pub band_depth_m: f64,
    pub band_tint: u32,
    pub band_streaks: f64,
    pub band_glints: f64,
    pub band_rim_px: f64,
    pub band_rim_shade: f64,
    pub band_line_px: f64,
    pub band_line_glow: f64,
    /// How broken the band's ripples are (0: smooth stripes), the most of it the mirrored scene
    /// above is, and how far (px) the ripples shift and blur that mirror (0: literal).
    pub band_breakup: f64,
    pub band_mirror: f64,
    pub band_mirror_warp_px: f64,
    /// The light and dark cells on the surface's underside seen from below (0: flat); how much
    /// they read as broken bright cells with dark gaps (0: the caustic web); the sun patch's
    /// strength toward the sun's image through Snell's window and its tightness (a power).
    pub underside_texture: f64,
    pub underside_breakup: f64,
    pub underside_sun_patch: f64,
    pub underside_sun_power: f64,
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

/// The longest a view ray scaled to view depth 1 can be over any field of view the game draws
/// (`sqrt(1 + tan²(h/2) + tan²(v/2))` at the screen's corner; 1.72 at the default 100° × 75°,
/// 2.33 at 120° × 100°): how far above or below the eye a lens-plane point can lie, per metre of
/// lens. The render crate's test pins it over the FOV range.
pub const LENS_RAY_MAX: f64 = 2.5;

impl UnderwaterLook {
    /// How far from the surface the eye can be and still have a pixel on the other side of the
    /// line or in its band (m): the furthest a lens-plane point rises or falls ([`LENS_RAY_MAX`] ×
    /// the lens) plus the line's waves and the band's depth. Past it no pixel splits or shows the
    /// band, so entering or leaving the split states there changes no pixel (no pop).
    pub fn reach_m(&self) -> f64 {
        self.lens_m * LENS_RAY_MAX + self.line_amplitude_m + self.band_depth_m
    }
}

/// How far from a fluid's surface the eye can be and still see the line (m), over every look.
pub fn line_reach_m() -> f64 {
    UNDERWATER_LOOKS
        .iter()
        .chain([&DEFAULT_UNDERWATER])
        .map(UnderwaterLook::reach_m)
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
        assert!(line_reach_m() >= w.reach_m());
        assert_eq!(
            w.reach_m(),
            w.lens_m * LENS_RAY_MAX + w.line_amplitude_m + w.band_depth_m
        );
    }
}
