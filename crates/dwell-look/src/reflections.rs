//! The water's reflections per water quality (docs/WATER_RENDERING.md §8; W5's screen-space
//! reflections brought forward): how far and how finely the reflected ray is marched through the
//! scene's view depth for land, and how much of the cloud deck is drawn along it. The tunables are
//! data (`shared/look/reflections.json`, generated into `reflections.gen.rs`); the march itself is
//! the renderer's (`dwell-client` `render/reflection.rs` and its WGSL twin).

pub use crate::reflections_gen::REFLECTIONS;

/// What every tier shares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsrTunables {
    /// How far behind the scene's surface a ray step may land and still hit it (m).
    pub thickness_m: f64,
    /// Plus this share of the surface's view depth.
    pub thickness_rel: f64,
    /// Hits fade toward the sky within this share of the screen height from its edges.
    pub edge_fade: f64,
    /// Hits fade out over this last share of the tier's reach.
    pub distance_fade: f64,
}

/// One water quality's reflections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionTier {
    /// March steps on screen (0: no land reflected).
    pub ssr_steps: u32,
    /// Binary steps that pin a hit between the last two march steps.
    pub refine_steps: u32,
    /// How far the reflected ray reaches (m).
    pub max_distance_m: f64,
    /// The cloud deck's octaves in reflections (0: the sky alone).
    pub cloud_octaves: u32,
    /// The deck's self-shadow in reflections.
    pub cloud_shadow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionTunables {
    pub ssr: SsrTunables,
    /// Low, Medium, High.
    pub tiers: [ReflectionTier; 3],
}

impl ReflectionTunables {
    /// The tier for a water quality level (0 Low, 1 Medium, 2 High; higher is High).
    pub fn tier(&self, level: usize) -> &ReflectionTier {
        &self.tiers[level.min(self.tiers.len() - 1)]
    }
}

/// The frame uniform's two reflection vec4s for a tier, under the Clouds setting's octaves and
/// self-shadow (the reflection never draws more of the deck than the sky does): x march steps,
/// y refine steps, z reach (m), w the hit fade's start (m); then the cloud octaves, the
/// self-shadow (0 or 1), the edge fade and the thickness.
pub fn reflection_uniforms(
    t: &ReflectionTunables,
    level: usize,
    sky_octaves: u32,
    sky_shadow: bool,
) -> [[f32; 4]; 2] {
    let tier = t.tier(level);
    let reach = tier.max_distance_m;
    [
        [
            tier.ssr_steps as f32,
            tier.refine_steps as f32,
            reach as f32,
            (reach * (1.0 - t.ssr.distance_fade)) as f32,
        ],
        [
            tier.cloud_octaves.min(sky_octaves) as f32,
            if tier.cloud_shadow && sky_shadow {
                1.0
            } else {
                0.0
            },
            t.ssr.edge_fade as f32,
            t.ssr.thickness_m as f32,
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_reflects_the_sky_alone_and_medium_and_high_land_and_clouds() {
        let low = REFLECTIONS.tier(0);
        assert_eq!((low.ssr_steps, low.cloud_octaves), (0, 0));
        let (m, h) = (REFLECTIONS.tier(1), REFLECTIONS.tier(2));
        assert!(m.ssr_steps > 0 && m.cloud_octaves > 0);
        assert!(h.ssr_steps >= m.ssr_steps && h.max_distance_m >= m.max_distance_m);
        assert_eq!(REFLECTIONS.tier(9), h);
    }

    #[test]
    fn the_reflection_never_draws_more_of_the_deck_than_the_sky() {
        let u = reflection_uniforms(&REFLECTIONS, 2, 3, false);
        assert_eq!(u[1][0], 3.0_f32.min(REFLECTIONS.tier(2).cloud_octaves as f32));
        assert_eq!(u[1][1], 0.0, "no self-shadow while the sky draws none");
        let off = reflection_uniforms(&REFLECTIONS, 2, 0, true);
        assert_eq!(off[1][0], 0.0, "Clouds off: none reflected");
        let m = reflection_uniforms(&REFLECTIONS, 1, 8, true);
        assert_eq!(m[0][0], REFLECTIONS.tier(1).ssr_steps as f32);
        assert!(m[0][3] < m[0][2] && m[0][3] > 0.0, "the fade starts inside the reach");
    }

    #[test]
    fn a_tier_changed_by_data_changes_the_uniforms() {
        let mut t = REFLECTIONS;
        t.tiers[1].ssr_steps = 5;
        t.ssr.edge_fade = 0.3;
        let u = reflection_uniforms(&t, 1, 8, true);
        assert_eq!((u[0][0], u[1][2]), (5.0, 0.3));
    }
}
