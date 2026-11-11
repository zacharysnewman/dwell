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
    /// The sun and moon reflected as their discs and a glitter path (`BodyTunables`); false: the
    /// GGX glint of their light only (a simple highlight).
    pub body_path: bool,
}

/// One body's glitter path on the water (docs/WATER_RENDERING.md §15).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPath {
    /// The path's light × the disc's colour (the moon's × its lit share).
    pub strength: f64,
    /// The path lobe's GGX α (larger: longer, wider, dimmer).
    pub path_alpha: f64,
    /// Whole while the body is lower than this over the face's horizon (degrees).
    pub full_below_deg: f64,
    /// Gone while it is higher than this (degrees; at least `full_below_deg`).
    pub gone_above_deg: f64,
}

impl BodyPath {
    /// How much of the path shows with the body `elevation_deg` over the face's horizon:
    /// 1 under `full_below_deg`, 0 over `gone_above_deg`, smoothly between (the WGSL `path_share`).
    pub fn share(&self, elevation_deg: f64) -> f64 {
        if elevation_deg <= self.full_below_deg {
            return 1.0;
        }
        if elevation_deg >= self.gone_above_deg {
            return 0.0;
        }
        let t = (elevation_deg - self.full_below_deg) / (self.gone_above_deg - self.full_below_deg);
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}

/// The sun's and moon's paths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyTunables {
    pub sun: BodyPath,
    pub moon: BodyPath,
    /// The share of the path the cloud cover takes away while clouds are drawn.
    pub cloud_fade: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionTunables {
    pub ssr: SsrTunables,
    /// One per water tier (`crate::water_tiers::WATER_TIERS`, in its order).
    pub tiers: &'static [ReflectionTier],
    /// The same on a phone (the data's `phone` overrides over each tier).
    pub phone_tiers: &'static [ReflectionTier],
    pub bodies: BodyTunables,
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
/// self-shadow (bit 0) and the bodies' path (bit 1), the edge fade and the thickness.
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
            (u32::from(tier.cloud_shadow && sky_shadow) | (u32::from(tier.body_path) << 1)) as f32,
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
    fn the_cpu_tier_draws_a_highlight_only_and_the_others_the_bodies_path() {
        let cpu = crate::water_tiers::cpu_tier();
        for (level, tier) in REFLECTIONS.tiers.iter().enumerate() {
            let bits = reflection_uniforms(&REFLECTIONS, level, 8, false, false)[1][1] as u32;
            assert_eq!(bits & 2 != 0, tier.body_path);
            assert_eq!(tier.body_path, level != cpu, "tier {level}");
        }
        // The shadow bit stays its own.
        let m = at("medium");
        let mut tiers = REFLECTIONS.tiers.to_vec();
        tiers[m].cloud_shadow = true;
        let t = ReflectionTunables {
            tiers: Box::leak(tiers.into_boxed_slice()),
            ..REFLECTIONS
        };
        assert_eq!(reflection_uniforms(&t, m, 8, true, false)[1][1], 3.0);
    }

    #[test]
    fn the_suns_path_is_a_low_sun_s_and_the_moons_shows_at_any_height() {
        let b = REFLECTIONS.bodies;
        assert_eq!(b.sun.share(2.0), 1.0, "a setting sun lays a path");
        assert_eq!(b.sun.share(60.0), 0.0, "a high sun is a glint only");
        let mid = b.sun.share((b.sun.full_below_deg + b.sun.gone_above_deg) / 2.0);
        assert!((mid - 0.5).abs() < 1e-9);
        for e in [1.0, 30.0, 80.0] {
            assert_eq!(b.moon.share(e), 1.0);
        }
        assert!(b.moon.strength > 0.0 && b.sun.strength > 0.0);
    }

    #[test]
    fn the_reflection_never_draws_more_of_the_deck_than_the_sky() {
        let m = at("medium");
        let u = reflection_uniforms(&REFLECTIONS, m, 2, false, false);
        assert_eq!(
            u[1][0],
            2.0_f32.min(REFLECTIONS.tier(m).cloud_octaves as f32)
        );
        assert_eq!(u[1][1] as u32 & 1, 0, "no self-shadow while the sky draws none");
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
