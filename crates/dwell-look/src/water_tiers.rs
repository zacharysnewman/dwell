//! The water quality tiers (docs/WATER_RENDERING.md §4; proposals/WATER_GPU_SURFACE.md C1): which
//! of the water's pieces each tier draws, as data (`shared/look/watertiers.json`, generated into
//! `watertiers.gen.rs`), cheapest first. The renderer (`dwell-client` `render/water.rs`
//! `WaterQuality`) indexes this list and packs a tier's switches into the frame uniform; the shell
//! reads the same JSON for the Water setting's choices.

pub use crate::water_tiers_gen::{
    BANK_BAND, CAUSTIC_EVERY_FRAMES, SURFACE_CAUSTICS, WATER_TIER_ALIASES, WATER_TIERS,
};

/// One water quality tier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterTier {
    /// Its name (settings, presets, the bench).
    pub id: &'static str,
    /// Its Settings label.
    pub label: &'static str,
    /// Offered in Settings.
    pub setting: bool,
    /// The tier a CPU adapter draws.
    pub cpu: bool,
    /// Reads a copy of the scene: refraction, absorption, shoreline foam, caustics (each scene
    /// pass with water split around the copy).
    pub reads_scene: bool,
    /// In the blended pass (no copy), the share of the fluid's opacity drawn.
    pub opacity: f64,
    /// In the blended pass, the caustic tile's lines added on the water.
    pub surface_caustics: bool,
    /// Layers of normal detail (1–3).
    pub ripple_layers: u32,
    /// The foam streaks along the current.
    pub flow_streaks: bool,
    /// The bank band where the land isn't marched.
    pub bank_band: bool,
}

/// The bank band: the land reflected along the water's edge without a march.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BankBand {
    /// How far along the reflected ray the bank is looked up (m).
    pub reach_m: f64,
    /// The scene there counts as the bank while its view depth is under (1 + this) × the point's.
    pub depth_slack: f64,
    /// The taps' spread across the screen and up it (pixels).
    pub spread_px: f64,
    pub blur_px: f64,
    /// How much darker than the bank its reflection is (0–1).
    pub darken: f64,
    /// Its strength (× the surface's reflectance).
    pub strength: f64,
}

/// The blended pass's caustics: the tile's lines at the surface as if on a floor `depth_m` down,
/// at `gain`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCaustics {
    pub depth_m: f64,
    pub gain: f64,
}

/// A tier's index by name, or by a removed tier's alias (`WATER_TIER_ALIASES`).
pub fn tier_index(name: &str) -> Option<usize> {
    let name = WATER_TIER_ALIASES
        .iter()
        .find(|(from, _)| *from == name)
        .map_or(name, |(_, to)| *to);
    WATER_TIERS.iter().position(|t| t.id == name)
}

/// The CPU adapter's tier.
pub fn cpu_tier() -> usize {
    WATER_TIERS
        .iter()
        .position(|t| t.cpu)
        .expect("watertiers.mjs checks exactly one")
}

/// The tier's switches as the frame uniform carries them (`water.w`, read by the WGSL's
/// `water_tier_*`): bit 0 reads the scene, bit 1 flow streaks, bit 2 the bank band, bits 3–4 the
/// ripple layers, bit 5 the surface caustics, bits 6–9 the opacity in fifteenths. Exact in an f32.
pub fn tier_bits(t: &WaterTier) -> u32 {
    u32::from(t.reads_scene)
        | (u32::from(t.flow_streaks) << 1)
        | (u32::from(t.bank_band) << 2)
        | (t.ripple_layers.min(3) << 3)
        | (u32::from(t.surface_caustics) << 5)
        | (opacity_fifteenths(t.opacity) << 6)
}

/// The opacity as `tier_bits` carries it (0–15; the WGSL divides by 15).
pub fn opacity_fifteenths(opacity: f64) -> u32 {
    (opacity.clamp(0.0, 1.0) * 15.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's tiers (2026-11-07): a see-through Low for phones (scene, one ripple layer, no
    /// streaks, the band), Medium the whole look (High folded in), an opaque tier for the CPU only.
    #[test]
    fn low_sees_through_without_the_copy_and_only_the_cpu_tier_is_opaque() {
        // Owner, 2026-11-07: Low is see-through with no scene copy (no pass split on a phone).
        let low = &WATER_TIERS[tier_index("low").unwrap()];
        assert!(!low.reads_scene && !low.flow_streaks && low.setting && !low.cpu);
        assert!(low.opacity < 1.0 && low.surface_caustics);
        assert_eq!(low.ripple_layers, 1);
        let medium = &WATER_TIERS[tier_index("medium").unwrap()];
        assert!(medium.reads_scene && medium.flow_streaks && !medium.bank_band);
        assert_eq!(medium.ripple_layers, 3);
        let cpu = &WATER_TIERS[cpu_tier()];
        assert!(!cpu.reads_scene && !cpu.setting && cpu.id == "opaque");
        assert!(cpu.opacity >= 1.0 && !cpu.surface_caustics);
    }

    #[test]
    fn a_removed_high_reads_as_medium() {
        assert_eq!(tier_index("high"), tier_index("medium"));
        assert_eq!(tier_index("ultra"), None);
    }

    #[test]
    fn the_bits_carry_each_switch() {
        let low = &WATER_TIERS[tier_index("low").unwrap()];
        assert_eq!(
            tier_bits(low),
            (1 << 3) | (1 << 5) | (opacity_fifteenths(low.opacity) << 6)
        );
        let medium = &WATER_TIERS[tier_index("medium").unwrap()];
        assert_eq!(tier_bits(medium), 1 | 2 | (3 << 3) | (15 << 6));
        assert_eq!(opacity_fifteenths(0.55), 8);
        assert_eq!(tier_bits(&WATER_TIERS[cpu_tier()]) & 1, 0);
    }
}
