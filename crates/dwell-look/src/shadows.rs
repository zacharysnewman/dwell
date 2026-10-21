//! Sun and moon shadows (docs/DAY_NIGHT.md §10): the tunables of the cascaded shadow maps, and
//! which light casts at a time of day and how strongly. The tunables are data
//! (`shared/look/shadows.json`, generated into `shadows.gen.rs`); the cascades' fit and the maps
//! themselves are the renderer's (`dwell-client` `render/shadows.rs` and its WGSL).

use crate::Rgb;
use crate::daynight::DayState;

pub use crate::shadows_gen::SHADOWS;

/// What every tier's cascades share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CascadeTunables {
    /// Where the first slice starts (m of view depth): the log split's near.
    pub near_m: f64,
    /// Each cascade's sphere's radius is rounded up to a multiple of this (m).
    pub radius_step_m: f64,
    /// How far toward the light from a cascade's sphere casters still cast into it (m).
    pub caster_reach_m: f64,
    /// Shadows fade to the stand-in over this last share of the shadow distance.
    pub edge_fade: f64,
    /// The most the spyglass stretches the shadow distance to (m).
    pub spyglass_max_m: f64,
}

/// Against acne and peter-panning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiasTunables {
    /// The receiver's lookup moves this many of its cascade's texels out along its normal.
    pub normal_offset_texels: f64,
    /// The compare's constant bias (m along the light).
    pub depth_bias_m: f64,
    /// The casters' slope-scaled depth bias (the pipeline's).
    pub slope_scale: f64,
}

/// How a light's shadows follow its elevation over the viewer's face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightTunables {
    pub fade_start_deg: f64,
    pub fade_end_deg: f64,
    /// The shadow's full strength (1: the direct term wholly shaded).
    pub strength: f64,
}

/// One Shadows setting's maps (Low, High).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowTier {
    /// Cascades (1–4).
    pub cascades: u32,
    /// Each cascade's map, texels a side (a power of two).
    pub resolution: u32,
    /// The shadow distance (m of view depth).
    pub distance_m: f64,
    /// The slices' split: 0 even, 1 logarithmic.
    pub split_lambda: f64,
    /// The PCF's radius (texels).
    pub filter_texels: f64,
    /// Its taps a side.
    pub filter_taps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowTunables {
    pub cascades: CascadeTunables,
    pub bias: BiasTunables,
    pub sun: LightTunables,
    pub moon: LightTunables,
    /// Low, High.
    pub tiers: [ShadowTier; 2],
}

/// The most cascades a tier has (the shader's array; shared/look/shadows.mjs `MAX_CASCADES`).
pub const MAX_CASCADES: usize = 4;

/// The Shadows setting (the settings menu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShadowQuality {
    Off,
    Low,
    #[default]
    High,
}

impl ShadowQuality {
    /// From the settings' level: 0 Off, 1 Low, 2 (or more) High.
    pub fn from_level(level: u32) -> Self {
        match level {
            0 => Self::Off,
            1 => Self::Low,
            _ => Self::High,
        }
    }

    pub fn level(self) -> u32 {
        self as u32
    }
}

impl ShadowTunables {
    /// The maps a setting draws (none for Off).
    pub fn tier(&self, q: ShadowQuality) -> Option<&ShadowTier> {
        match q {
            ShadowQuality::Off => None,
            ShadowQuality::Low => Some(&self.tiers[0]),
            ShadowQuality::High => Some(&self.tiers[1]),
        }
    }
}

/// The light that casts this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caster {
    /// Toward the light (unit, world).
    pub dir: Rgb,
    /// True for the moon.
    pub moon: bool,
    /// How strongly its shadows shade (0–1): the time of day's fade times the light's strength.
    pub strength: f64,
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The light whose shadows a viewer on face `face` (+1 A, −1 B) sees at `day`: the sun while it
/// is over that face's horizon, else the moon while it is; none when neither is up. Its strength
/// fades in with its elevation (`LightTunables`), so the sun's shadows fade out at dusk and the
/// moon's fade in.
pub fn caster(t: &ShadowTunables, day: &DayState, face: f64) -> Option<Caster> {
    let elevation = |d: Rgb| (d[1] * face).clamp(-1.0, 1.0).asin().to_degrees();
    let of = |dir: Rgb, l: &LightTunables, moon: bool| {
        let e = elevation(dir);
        (e > 0.0).then(|| Caster {
            dir,
            moon,
            strength: l.strength * smoothstep(l.fade_start_deg, l.fade_end_deg, e),
        })
    };
    of(day.sun, &t.sun, false).or_else(|| of(day.moon, &t.moon, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daynight::DAY_NIGHT;

    fn day_at(time: f64) -> DayState {
        DAY_NIGHT.state(0.0, Some(time), Some(0.5))
    }

    #[test]
    fn the_settings_levels_round_trip_and_off_draws_no_maps() {
        for q in [ShadowQuality::Off, ShadowQuality::Low, ShadowQuality::High] {
            assert_eq!(ShadowQuality::from_level(q.level()), q);
        }
        assert_eq!(ShadowQuality::from_level(9), ShadowQuality::High);
        assert!(SHADOWS.tier(ShadowQuality::Off).is_none());
        let (low, high) = (
            SHADOWS.tier(ShadowQuality::Low).unwrap(),
            SHADOWS.tier(ShadowQuality::High).unwrap(),
        );
        assert!(low.cascades <= high.cascades && low.resolution <= high.resolution);
        assert!(low.distance_m <= high.distance_m);
        assert!(high.cascades as usize <= MAX_CASCADES);
    }

    #[test]
    fn the_sun_casts_at_noon_the_moon_at_midnight_and_both_faces_see_their_own() {
        let noon = day_at(0.5);
        let c = caster(&SHADOWS, &noon, 1.0).expect("the sun is up on face A");
        assert!(!c.moon && c.strength > 0.99, "{c:?}");
        // Face B's noon is face A's midnight: there the sun is under face A, over face B.
        let midnight = day_at(0.0);
        let b = caster(&SHADOWS, &midnight, -1.0).expect("the sun is up on face B");
        assert!(!b.moon && b.strength > 0.99, "{b:?}");
        let a = caster(&SHADOWS, &midnight, 1.0).expect("the moon is up on face A");
        assert!(a.moon, "{a:?}");
    }

    #[test]
    fn the_suns_shadows_fade_out_toward_the_horizon() {
        // Through the evening the sun's strength only falls, to none once it has set.
        let mut last = f64::INFINITY;
        let mut faded = false;
        for k in 0..=500 {
            let t = 0.5 + k as f64 * 0.001;
            let Some(c) = caster(&SHADOWS, &day_at(t), 1.0) else {
                continue;
            };
            if c.moon {
                faded = true;
                continue;
            }
            assert!(c.strength <= last + 1e-9, "t {t}: {} after {last}", c.strength);
            last = c.strength;
        }
        assert!(last < 0.05 || faded, "the sun's shadows fade by sunset ({last})");
    }

    #[test]
    fn a_light_tuned_by_data_changes_the_strength() {
        let mut t = SHADOWS;
        t.sun.strength = 0.5;
        let c = caster(&t, &day_at(0.5), 1.0).unwrap();
        assert!((c.strength - 0.5).abs() < 1e-9);
        // Neither up: none.
        let mut d = day_at(0.5);
        d.sun = [0.0, -1.0, 0.0];
        d.moon = [0.0, -1.0, 0.0];
        assert_eq!(caster(&t, &d, 1.0), None);
    }
}
