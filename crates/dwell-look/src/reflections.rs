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
    /// How much of the ripples' slope steers the land's reflected ray (the sky takes all of it).
    pub ripple_scale: f64,
    /// The land is marched only where the surface reflects more than this share (Schlick's
    /// Fresnel), its reflection faded in up to twice it.
    pub min_reflectance: f64,
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
    /// One per water tier (`crate::water_tiers::WATER_TIERS`, in its order).
    pub tiers: &'static [ReflectionTier],
    /// The same on a phone (the data's `phone` overrides over each tier).
    pub phone_tiers: &'static [ReflectionTier],
}

impl ReflectionTunables {
    /// The tier for a water tier's index (`WATER_TIERS`; past the last, the last).
    pub fn tier(&self, level: usize) -> &ReflectionTier {
        &self.tiers[level.min(self.tiers.len() - 1)]
    }

    /// The tier as a device draws it: a phone's own (`phone_tiers`), else `tier`'s.
    pub fn tier_for(&self, level: usize, phone: bool) -> &ReflectionTier {
        if phone {
            &self.phone_tiers[level.min(self.phone_tiers.len() - 1)]
        } else {
            self.tier(level)
        }
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
    phone: bool,
) -> [[f32; 4]; 2] {
    let tier = t.tier_for(level, phone);
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
    use crate::water_tiers::tier_index;

    fn at(name: &str) -> usize {
        tier_index(name).expect("a water tier")
    }

    #[test]
    fn the_sceneless_tier_and_low_march_nothing_and_medium_reflects_land_and_clouds() {
        let opaque = REFLECTIONS.tier(at("opaque"));
        assert_eq!((opaque.ssr_steps, opaque.cloud_octaves), (0, 0));
        let low = REFLECTIONS.tier(at("low"));
        assert_eq!(
            low.ssr_steps, 0,
            "Low reflects the bank by the band, not a march"
        );
        let m = REFLECTIONS.tier(at("medium"));
        assert!(m.ssr_steps > 0 && m.cloud_octaves > 0);
        assert_eq!(
            REFLECTIONS.tiers.len(),
            crate::water_tiers::WATER_TIERS.len()
        );
        assert_eq!(REFLECTIONS.tier(99), REFLECTIONS.tiers.last().unwrap());
    }

    #[test]
    fn the_reflection_never_draws_more_of_the_deck_than_the_sky() {
        let m = at("medium");
        let u = reflection_uniforms(&REFLECTIONS, m, 2, false, false);
        assert_eq!(
            u[1][0],
            2.0_f32.min(REFLECTIONS.tier(m).cloud_octaves as f32)
        );
        assert_eq!(u[1][1], 0.0, "no self-shadow while the sky draws none");
        let off = reflection_uniforms(&REFLECTIONS, m, 0, true, false);
        assert_eq!(off[1][0], 0.0, "Clouds off: none reflected");
        let u = reflection_uniforms(&REFLECTIONS, m, 8, true, false);
        assert_eq!(u[0][0], REFLECTIONS.tier(m).ssr_steps as f32);
        assert!(
            u[0][3] < u[0][2] && u[0][3] > 0.0,
            "the fade starts inside the reach"
        );
    }

    #[test]
    fn a_tier_changed_by_data_changes_the_uniforms() {
        let m = at("medium");
        let mut tiers = REFLECTIONS.tiers.to_vec();
        tiers[m].ssr_steps = 5;
        let mut t = REFLECTIONS;
        t.tiers = Box::leak(tiers.into_boxed_slice());
        t.ssr.edge_fade = 0.3;
        let u = reflection_uniforms(&t, m, 8, true, false);
        assert_eq!((u[0][0], u[1][2]), (5.0, 0.3));
    }

    /// A phone's Medium marches fewer steps and reflects fewer cloud octaves (owner, 2026-11-06:
    /// Medium slowed a phone close to the water); a desktop's is the tier's.
    #[test]
    fn a_phone_draws_a_cheaper_medium() {
        let m = at("medium");
        let (desk, phone) = (
            REFLECTIONS.tier_for(m, false),
            REFLECTIONS.tier_for(m, true),
        );
        assert_eq!(desk, REFLECTIONS.tier(m));
        assert!(phone.ssr_steps < desk.ssr_steps && phone.cloud_octaves < desk.cloud_octaves);
        assert_eq!(
            REFLECTIONS.tier_for(at("low"), true),
            REFLECTIONS.tier(at("low"))
        );
        let u = reflection_uniforms(&REFLECTIONS, m, 8, true, true);
        assert_eq!(u[0][0], phone.ssr_steps as f32);
        assert_eq!(u[1][0], phone.cloud_octaves as f32);
    }
}
