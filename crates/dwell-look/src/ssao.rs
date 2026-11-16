//! Screen-space ambient occlusion (docs/SSAO.md, ADR 0080): soft contact shadows on the hemisphere
//! and block light where things meet, from the scene's view depth. Drawn by the Ambient occlusion
//! setting's tier (`shared/look/ssao.json`, generated into `ssao.gen.rs`) at the Settings strength.

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("ssao.gen.rs");
}
pub use generated::{HIGH, LEVELS, LOW, MAX_BLUR, MAX_SAMPLES, SSAO};

/// The look: the sampling radius (m), the estimator's intensity and bias (m), the darkening's
/// strength, how much the baked corner shading weakens it, the widest the spiral may be (drawing
/// buffer pixels), the near and far fades (m) and the blur's taps each way and depth sharpness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoLook {
    pub radius: f64,
    pub intensity: f64,
    pub bias: f64,
    pub strength: f64,
    pub baked: f64,
    pub max_pixels: f64,
    pub near_from: f64,
    pub near_to: f64,
    pub far_from: f64,
    pub far_to: f64,
    pub blur_radius: u32,
    pub blur_sharpness: f64,
}

/// A Settings look (`levels` in shared/look/ssao.json; owner, 2026-11-15): the strength, intensity
/// and radius (m) it draws with in place of the file's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoLevel {
    pub strength: f64,
    pub intensity: f64,
    pub radius: f64,
    /// The least share of a pixel's light it darkens (0: the scene's share alone).
    pub floor: f64,
}

/// The look named `name` (`weak`, `normal`, `strong`); anything else: normal.
pub fn level(name: &str) -> SsaoLevel {
    LEVELS
        .iter()
        .find(|(n, _)| *n == name)
        .or_else(|| LEVELS.iter().find(|(n, _)| *n == "normal"))
        .map(|(_, l)| *l)
        .expect("the data has normal")
}

/// A tier of the setting: the buffer against the drawing buffer (each axis) and the samples a texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoTier {
    pub scale: f64,
    pub samples: u32,
}

/// The Ambient occlusion setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsaoQuality {
    Off,
    Low,
    High,
}

impl SsaoQuality {
    /// From the settings store's name (`off`, `low`, `high`); anything else is None.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The tier it draws (None: no passes).
    pub fn tier(self) -> Option<SsaoTier> {
        match self {
            Self::Off => None,
            Self::Low => Some(LOW),
            Self::High => Some(HIGH),
        }
    }
}

/// The occlusion buffer for a drawing buffer of `size` at `tier` (each axis scaled, rounded up).
pub fn buffer_size((w, h): (u32, u32), tier: SsaoTier) -> (u32, u32) {
    let s = |v: u32| ((f64::from(v) * tier.scale).ceil() as u32).max(1);
    (s(w), s(h))
}

/// The share of the occlusion a pixel `depth` m from the eye keeps: none inside `near_from` (your
/// arms, the lens), all from `near_to` to `far_from`, none past `far_to` (smoothly between).
pub fn fade(look: &SsaoLook, depth: f64) -> f64 {
    let smooth = |a: f64, b: f64, x: f64| {
        let t = ((x - a) / (b - a).max(1e-9)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    smooth(look.near_from, look.near_to, depth) * (1.0 - smooth(look.far_from, look.far_to, depth))
}

/// The share of a pixel's light the occlusion darkens (the scene's `frame_ao`): the hemisphere and
/// block light's share of its irradiance, `occludable` of `total`, weakened by the baked corner
/// shading (`ambient`, the baked hemisphere level 0–1: at 0 it keeps 1 − `baked` of it).
pub fn share(look: &SsaoLook, occludable: f64, total: f64, ambient: f64) -> f64 {
    let s = (occludable / total.max(1e-6)).clamp(0.0, 1.0);
    s * (1.0 - look.baked * (1.0 - ambient.clamp(0.0, 1.0)))
}

/// A pixel's colour factor for an occlusion `ao` (1: open) at its share `share` and the setting's
/// strength `strength` (× the data's): 1 − (1 − AO) × strength × share.
pub fn factor(ao: f64, strength: f64, share: f64) -> f64 {
    1.0 - (1.0 - ao.clamp(0.0, 1.0)) * strength.clamp(0.0, 4.0) * share.clamp(0.0, 1.0)
}

/// The share as the stored view depth carries it: `SHARE_LEVELS` + 1 steps (render/shaders.rs
/// `frame_depth`, three bits).
pub const SHARE_LEVELS: u32 = 7;

/// A share rounded to its stored step.
pub fn stored_share(share: f64) -> u32 {
    (share.clamp(0.0, 1.0) * f64::from(SHARE_LEVELS)).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiers_pin_their_cost() {
        // Low (the High preset) at half resolution and fewer samples; High (Ultra) at full.
        assert_eq!(LOW.scale, 0.5);
        assert_eq!(HIGH.scale, 1.0);
        const { assert!(LOW.samples < HIGH.samples) };
        assert_eq!(SsaoQuality::Off.tier(), None);
        assert_eq!(SsaoQuality::Low.tier(), Some(LOW));
        assert_eq!(SsaoQuality::High.tier(), Some(HIGH));
        assert_eq!(SsaoQuality::from_name("low"), Some(SsaoQuality::Low));
        assert_eq!(SsaoQuality::from_name("ultra"), None);
        const { assert!(HIGH.samples <= MAX_SAMPLES && SSAO.blur_radius <= MAX_BLUR) };
    }

    #[test]
    fn the_buffer_scales_each_axis_and_rounds_up() {
        assert_eq!(buffer_size((1280, 720), LOW), (640, 360));
        assert_eq!(buffer_size((1281, 721), LOW), (641, 361));
        assert_eq!(buffer_size((1280, 720), HIGH), (1280, 720));
        assert_eq!(buffer_size((1, 1), LOW), (1, 1));
    }

    #[test]
    fn it_fades_in_past_the_arms_and_out_with_distance() {
        let l = SSAO;
        assert_eq!(fade(&l, 0.2), 0.0, "your arms take none");
        assert_eq!(fade(&l, (l.near_to + l.far_from) / 2.0), 1.0);
        assert_eq!(fade(&l, l.far_to + 1.0), 0.0, "the far terrain takes none");
        let mid = fade(&l, (l.far_from + l.far_to) / 2.0);
        assert!(mid > 0.4 && mid < 0.6, "{mid}");
    }

    #[test]
    fn only_the_ambient_share_darkens_and_the_baked_shading_weakens_it() {
        let l = SSAO;
        // In full sun the hemisphere is a small share of the light: the AO darkens little.
        let sunlit = share(&l, 0.3, 1.0, 1.0);
        let shaded = share(&l, 1.0, 1.0, 1.0);
        assert!((sunlit - 0.3).abs() < 1e-9 && shaded == 1.0);
        // A baked corner (ambient 0) keeps 1 − baked of it, so the two don't double.
        assert!((share(&l, 1.0, 1.0, 0.0) - (1.0 - l.baked)).abs() < 1e-9);
        // An occluded pixel darkens by strength × share; an open one not at all.
        assert_eq!(factor(1.0, 1.0, 1.0), 1.0);
        assert!((factor(0.5, 1.0, 0.5) - 0.75).abs() < 1e-9);
        assert_eq!(factor(0.0, 0.0, 1.0), 1.0, "strength 0 draws none");
        assert_eq!(stored_share(1.0), SHARE_LEVELS);
        assert_eq!(stored_share(0.0), 0);
        assert_eq!(stored_share(0.5), 4);
    }
}
