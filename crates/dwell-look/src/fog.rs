//! Height fog (ARCHITECTURE.md §6.6): a haze whose density falls off exponentially above sea level,
//! σ(y) = σ₀·e^(−(y − seaLevel)/H), integrated along each view ray, capped at `density`.
//! `haze_amount` is the reference for the renderer's shader. The fog is fixed look data
//! (`shared/look/fog.json`, generated into `fog.gen.rs`), the same for every client: not a setting.

use dwell_protocol::world;

#[path = "fog.gen.rs"]
mod generated;
pub use generated::{DENSITY_RANGE, DISTANCE_RANGE, FOG, HEIGHT_RANGE};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSettings {
    /// At sea level, the distance at which the haze reaches half its maximum (m).
    pub distance_m: f64,
    /// Maximum haze, 0 (no fog) to 1.
    pub density: f64,
    /// Scale height (m).
    pub height_m: f64,
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
        // Owner, 2026-10-21: a clear day's air (about 100 km visibility): half haze at 16 km, the
        // cap near 1 so far hills fade into the sky, and real low-lying haze (1.8 km scale height).
        // The 0.22.7 defaults, made the one fog (shared/look/fog.json) when the sliders went.
        assert_eq!(
            FOG,
            FogSettings {
                distance_m: 16_000.0,
                density: 0.95,
                height_m: 1800.0
            }
        );
        // A hill 2.5 mi off stays crisp; the far coast is clearly hazy; 40 mi (a Rainier) is mostly haze.
        assert!(haze_amount(&FOG, EYE, 0.0, 4000.0) < 0.2);
        assert!(haze_amount(&FOG, EYE, 0.0, 25_000.0) > 0.55);
        assert!(haze_amount(&FOG, EYE, 0.0, 64_000.0) > 0.85);
        for r in [0.0, RADIUS / 2.0, RADIUS] {
            assert!(haze_amount(&FOG, TOP, 0.0, r.hypot(TOP)) < FOG.density / 3.0);
            assert!(haze_amount(&THIN, TOP, 0.0, r.hypot(TOP)) < 0.05);
        }
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
