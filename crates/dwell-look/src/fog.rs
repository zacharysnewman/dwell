//! Height fog (ARCHITECTURE.md §6.6): a haze whose density falls off exponentially above sea level,
//! σ(y) = σ₀·e^(−(y − seaLevel)/H), integrated along each view ray, capped at `density`.
//! `haze_amount` is the reference for the renderer's shader. The fog is fixed look data
//! (`shared/look/fog.json`, generated into `fog.gen.rs`), the same for every client: not a setting.

use dwell_protocol::world;

#[path = "fog.gen.rs"]
mod generated;
pub use generated::{DENSITY_RANGE, DISTANCE_RANGE, FOG, HEIGHT_RANGE, PRESETS, VEIL_HALF_RANGE};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSettings {
    /// At sea level, the distance at which the haze reaches half its maximum (m).
    pub distance_m: f64,
    /// Maximum haze, 0 (no fog) to 1.
    pub density: f64,
    /// Scale height (m).
    pub height_m: f64,
    /// The air colour is the sky's at least this many degrees above the horizon (0: the horizon's).
    pub air_elevation_deg: f64,
    /// How far the air colour is multiplied toward `warm` facing the sun, `cool` away (0: none).
    pub sun_tint: f64,
    pub warm: [f64; 3],
    pub cool: [f64; 3],
    /// The distance (m) at which half the veil's most is lost.
    pub veil_half_m: f64,
    /// The most saturation far land loses before the haze hides it (0: none).
    pub veil_saturation: f64,
    /// The most its darks fill with the air (0: none).
    pub veil_contrast: f64,
}

impl FogSettings {
    /// Whether the air colour or the veil differs from the plain fade to the horizon's colour
    /// (false for today's data: the shader then draws the plain fade, unchanged).
    pub fn aerial(&self) -> bool {
        self.air_elevation_deg > 0.0
            || self.sun_tint > 0.0
            || self.veil_saturation > 0.0
            || self.veil_contrast > 0.0
    }
}

static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Picks the fog the renderer draws by its preset's name (`?haze=<name>`, a dev tool to compare
/// fog.json's `presets`; "" or an unknown name: [`FOG`]). Set before the renderer is built (its
/// shaders bake the fog in). Returns whether the name was a preset.
pub fn select(name: &str) -> bool {
    let at = PRESETS.iter().position(|(n, _)| *n == name);
    ACTIVE.store(
        at.map_or(0, |i| i + 1),
        std::sync::atomic::Ordering::Relaxed,
    );
    at.is_some()
}

/// The fog the renderer draws: [`FOG`], or the preset [`select`] picked.
pub fn active() -> &'static FogSettings {
    match ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => &FOG,
        i => &PRESETS[i - 1].1,
    }
}

/// The direction whose sky colour a ray `dir` (unit, face-local +y up) fades toward: lifted to at
/// least `air_elevation_deg` above the horizon in its azimuth (unchanged for 0 or a ray above it).
/// `haze_air` in the composite.
pub fn air_direction(fog: &FogSettings, dir: [f64; 3]) -> [f64; 3] {
    let lift = fog.air_elevation_deg.to_radians().sin();
    if dir[1] >= lift {
        return dir;
    }
    let len = dir[0].hypot(dir[2]);
    if len < 1e-9 {
        return dir;
    }
    let c = (1.0 - lift * lift).sqrt() / len;
    [dir[0] * c, lift, dir[2] * c]
}

/// The air colour's multiplier for a ray whose level direction makes cosine `toward` (−1 to 1)
/// with the sun's: warm toward it, cool away, by `sun_tint` (1 for 0).
pub fn air_tint(fog: &FogSettings, toward: f64) -> [f64; 3] {
    let t = (0.5 + 0.5 * toward).clamp(0.0, 1.0);
    std::array::from_fn(|i| {
        1.0 + fog.sun_tint * (fog.cool[i] + (fog.warm[i] - fog.cool[i]) * t - 1.0)
    })
}

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// A colour `c` (output colour) `distance` m off, veiled before the haze over it: toward its grey
/// by `veil_saturation`, its darks toward the air colour `air` by `veil_contrast`, each reaching
/// half at `veil_half_m`. `c` unchanged for 0s. `haze_veiled` in the composite.
pub fn veiled(fog: &FogSettings, c: [f64; 3], air: [f64; 3], distance: f64) -> [f64; 3] {
    let k = 1.0 - (-distance / fog.veil_half_m).exp2();
    let grey = luma(c);
    let s = fog.veil_saturation * k;
    let d: [f64; 3] = std::array::from_fn(|i| c[i] + (grey - c[i]) * s);
    let dark = (1.0 - luma(d) / luma(air).max(1e-3)).clamp(0.0, 1.0);
    let w = fog.veil_contrast * k * dark;
    std::array::from_fn(|i| d[i] + (air[i] - d[i]) * w)
}

/// Extinction coefficient at sea level (per m): haze before the cap reaches ½ at `distance_m`.
pub fn fog_sigma(distance_m: f64) -> f64 {
    std::f64::consts::LN_2 / distance_m
}

/// Largest exponent used: keeps e^x finite in float32 (the shader) deep below sea level.
pub const MAX_EXPONENT: f64 = 80.0;

/// Haze (0 to `density`) over a point `distance` m from a camera at height `camera_y`, the point
/// at `point_y`: the optical depth of the exponential atmosphere along the straight ray.
pub fn haze_amount(fog: &FogSettings, camera_y: f64, point_y: f64, distance: f64) -> f64 {
    let h = fog.height_m;
    let low = camera_y.min(point_y) - world::SEA_LEVEL_M;
    let k = (point_y - camera_y).abs() / h;
    let base = (-low / h).min(MAX_EXPONENT).exp();
    let along = if k < 1e-4 {
        1.0 - k / 2.0
    } else {
        -(-k).exp_m1() / k
    };
    let depth = fog_sigma(fog.distance_m) * distance * base * along;
    fog.density * -(-depth).exp_m1()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: f64 = 1.6;
    const THIN: FogSettings = FogSettings {
        distance_m: 100_000.0,
        density: 0.6,
        height_m: 1500.0,
        ..FOG
    };
    const RADIUS: f64 = world::WORLD_RADIUS as f64;
    /// High above the world: flight has no ceiling (ADR 0025 D3).
    const TOP: f64 = 200_000.0;

    #[test]
    fn hazes_the_horizon_lightly_never_past_the_density() {
        assert!(haze_amount(&THIN, EYE, 0.0, 1000.0) < 0.01);
        assert!(haze_amount(&THIN, EYE, 0.0, 400_000.0) > 0.5);
        assert!(haze_amount(&THIN, EYE, 0.0, 2.0 * RADIUS) <= THIN.density);
    }

    #[test]
    fn reaches_half_its_density_at_the_distance_at_sea_level() {
        let fog = FogSettings {
            distance_m: 250_000.0,
            ..THIN
        };
        let sea = world::SEA_LEVEL_M;
        assert!((haze_amount(&fog, sea, sea, 250_000.0) - fog.density / 2.0).abs() < 1e-6);
    }

    #[test]
    fn thins_with_altitude_and_leaves_mountain_tops_clearer() {
        let mut previous = f64::INFINITY;
        for height in [0.0, 1000.0, 5000.0, 20_000.0, 200_000.0, 2_000_000.0] {
            let haze = haze_amount(&THIN, height, 0.0, 300_000f64.hypot(height));
            assert!(haze < previous);
            previous = haze;
        }
        assert!(
            haze_amount(&THIN, 4000.0, 4000.0, 100_000.0)
                < haze_amount(&THIN, 0.0, 0.0, 100_000.0) / 10.0
        );
    }

    #[test]
    fn the_fixed_fog_keeps_nearby_hills_crisp_and_softens_the_far_island() {
        // Owner, 2026-11-12 (WORK_QUEUE item 29): "Yeah let's go with D on haze" — half haze at 6 km
        // and a 1 km scale height (was 16 km / 1.8 km since 0.22.7), the cap near 1 so far hills fade
        // into the sky; the haze thins with height, so peaks stay crisper than the plains below them.
        assert_eq!(
            FOG,
            FogSettings {
                distance_m: 6_000.0,
                density: 0.95,
                height_m: 1000.0,
                ..FOG
            }
        );
        // Aerial perspective is off in the data every client draws (WORK_QUEUE item 29: the owner
        // picks a preset); the composite then draws the plain fade, unchanged.
        assert!(!FOG.aerial());
        // A hill 1 km off stays crisp; one 4 km off is softened; the far coast is mostly haze.
        assert!(haze_amount(&FOG, EYE, 0.0, 1000.0) < 0.15);
        assert!(haze_amount(&FOG, EYE, 0.0, 4000.0) > 0.3);
        assert!(haze_amount(&FOG, EYE, 0.0, 25_000.0) > 0.85);
        assert!(haze_amount(&FOG, EYE, 0.0, 64_000.0) > 0.9);
        for r in [0.0, RADIUS / 2.0, RADIUS] {
            assert!(haze_amount(&FOG, TOP, 0.0, r.hypot(TOP)) < FOG.density / 3.0);
            assert!(haze_amount(&THIN, TOP, 0.0, r.hypot(TOP)) < 0.05);
        }
    }

    #[test]
    fn todays_fog_output_is_pinned() {
        // Golden values of today's haze: the owner's pick D (WORK_QUEUE item 29, 2026-11-12: 6 km half
        // distance, 1 km scale height); was 0.041/0.154/0.481/0.893 at 16 km / 1.8 km.
        for (d, want) in [
            (1000.0, 0.106_739),
            (4000.0, 0.360_238),
            (16_000.0, 0.808_897),
            (64_000.0, 0.949_538),
        ] {
            let got = haze_amount(&FOG, EYE, 0.0, d);
            assert!((got - want).abs() < 1e-5, "{d} m: {got}");
        }
        // With today's zeros the air is the horizon's and the veil leaves colours alone.
        let c = [0.2, 0.5, 0.1];
        assert_eq!(veiled(&FOG, c, [0.8, 0.85, 0.9], 50_000.0), c);
        // (below the horizon the sky reads the horizon in that azimuth, as `dwell_sky` does)
        assert_eq!(air_direction(&FOG, [0.6, -0.8, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(air_tint(&FOG, 1.0), [1.0; 3]);
    }

    #[test]
    fn presets_by_data_alone_reach_the_aerial_look() {
        // Every preset is a full fog over the data; picking one by name makes it the active one.
        assert!(!PRESETS.is_empty());
        for (name, p) in PRESETS {
            assert!(select(name));
            assert_eq!(active(), p);
        }
        assert!(!select("no-such-haze"));
        assert_eq!(active(), &FOG);
        let fog = FogSettings {
            air_elevation_deg: 4.0,
            sun_tint: 1.0,
            veil_saturation: 0.5,
            veil_contrast: 0.5,
            ..FOG
        };
        assert!(fog.aerial());
        // The air colour is the sky's 4° up for a ray at or below that, the azimuth kept.
        let lifted = air_direction(&fog, [1.0, 0.0, 0.0]);
        assert!((lifted[1] - 4f64.to_radians().sin()).abs() < 1e-12 && lifted[2] == 0.0);
        assert!((lifted[0].hypot(lifted[1]) - 1.0).abs() < 1e-12);
        assert_eq!(air_direction(&fog, [0.0, 0.5, 0.866]), [0.0, 0.5, 0.866]);
        // Warm toward the sun (more red than blue), cool away.
        let (toward, away) = (air_tint(&fog, 1.0), air_tint(&fog, -1.0));
        assert!(toward[0] > toward[2] && away[2] > away[0]);
        // The veil greys far colour and fills its darks with the air; half of it by halfM.
        let green = [0.1, 0.4, 0.05];
        let air = [0.7, 0.8, 0.95];
        let near = veiled(&fog, green, air, 10.0);
        let far = veiled(&fog, green, air, 40_000.0);
        let spread = |c: [f64; 3]| {
            c.iter().cloned().fold(0.0, f64::max) - c.iter().cloned().fold(1.0, f64::min)
        };
        assert!(spread(far) < spread(near) && spread(near) <= spread(green) + 1e-12);
        assert!(luma(far) > luma(near));
    }

    // face-a-only: a numeric-range check of the fog data at face A's heights (TOP, the
    // midplane); it tests the formula's finiteness, not any face-dependent behaviour.
    #[test]
    fn stays_finite_over_the_data_ranges_and_is_off_at_zero_density() {
        let mid = world::MIDPLANE_M;
        assert!((DISTANCE_RANGE.0..=DISTANCE_RANGE.1).contains(&FOG.distance_m));
        assert!((DENSITY_RANGE.0..=DENSITY_RANGE.1).contains(&FOG.density));
        assert!((HEIGHT_RANGE.0..=HEIGHT_RANGE.1).contains(&FOG.height_m));
        for height_m in [HEIGHT_RANGE.0, HEIGHT_RANGE.1] {
            for distance_m in [DISTANCE_RANGE.0, DISTANCE_RANGE.1] {
                let fog = FogSettings {
                    distance_m,
                    density: DENSITY_RANGE.1,
                    height_m,
                    ..FOG
                };
                for (cam, point) in [(TOP, mid), (mid, mid), (mid, TOP)] {
                    let haze = haze_amount(&fog, cam, point, 2.0 * RADIUS);
                    assert!(haze.is_finite() && (0.0..=1.0).contains(&haze));
                }
            }
        }
        assert_eq!(
            haze_amount(
                &FogSettings {
                    density: 0.0,
                    ..THIN
                },
                0.0,
                0.0,
                1e7
            ),
            0.0
        );
        assert_eq!(DISTANCE_RANGE.1, 2.0 * RADIUS);
    }
}
