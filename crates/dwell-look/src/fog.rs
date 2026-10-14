//! Height fog (ARCHITECTURE.md §6.6): a haze whose density falls off exponentially above sea level,
//! σ(y) = σ₀·e^(−(y − seaLevel)/H), integrated along each view ray, capped at `density`.
//! `haze_amount` is the reference for the renderer's shader.

use dwell_protocol::world;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSettings {
    /// At sea level, the distance at which the haze reaches half its maximum (m).
    pub distance_m: f64,
    /// Maximum haze, 0 (no fog) to 1.
    pub density: f64,
    /// Scale height (m).
    pub height_m: f64,
}

pub const DEFAULT_FOG: FogSettings = FogSettings {
    distance_m: 4000.0,
    density: 0.5,
    height_m: 1500.0,
};

/// Slider ranges (the settings menu): distance up to the world's diameter.
pub const DISTANCE_LIMITS: (f64, f64) = (1000.0, 2.0 * world::WORLD_RADIUS as f64);
pub const DENSITY_LIMITS: (f64, f64) = (0.0, 1.0);
pub const HEIGHT_LIMITS: (f64, f64) = (50.0, 50_000.0);

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
    let low = camera_y.min(point_y) - world::SEA_LEVEL as f64;
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

/// Clamps settings into the menu's ranges; anything missing or not a number takes the default.
pub fn sanitize_fog(
    distance_m: Option<f64>,
    density: Option<f64>,
    height_m: Option<f64>,
) -> FogSettings {
    let pick = |v: Option<f64>, (lo, hi): (f64, f64), d: f64| {
        v.filter(|x| x.is_finite()).map_or(d, |x| x.clamp(lo, hi))
    };
    FogSettings {
        distance_m: pick(distance_m, DISTANCE_LIMITS, DEFAULT_FOG.distance_m),
        density: pick(density, DENSITY_LIMITS, DEFAULT_FOG.density),
        height_m: pick(height_m, HEIGHT_LIMITS, DEFAULT_FOG.height_m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::players;

    const EYE: f64 = 1.6;
    const FOG: FogSettings = FogSettings {
        distance_m: 100_000.0,
        density: 0.6,
        height_m: 1500.0,
    };
    const RADIUS: f64 = world::WORLD_RADIUS as f64;
    const TOP: f64 = players::FLIGHT_CEILING as f64;

    #[test]
    fn hazes_the_horizon_lightly_never_past_the_density() {
        assert!(haze_amount(&FOG, EYE, 0.0, 1000.0) < 0.01);
        assert!(haze_amount(&FOG, EYE, 0.0, 400_000.0) > 0.5);
        assert!(haze_amount(&FOG, EYE, 0.0, 2.0 * RADIUS) <= FOG.density);
    }

    #[test]
    fn reaches_half_its_density_at_the_distance_at_sea_level() {
        let fog = FogSettings {
            distance_m: 250_000.0,
            ..FOG
        };
        assert!((haze_amount(&fog, 0.0, 0.0, 250_000.0) - fog.density / 2.0).abs() < 1e-6);
    }

    #[test]
    fn thins_with_altitude_and_leaves_mountain_tops_clearer() {
        let mut previous = f64::INFINITY;
        for height in [0.0, 1000.0, 5000.0, 20_000.0, 200_000.0, 2_000_000.0] {
            let haze = haze_amount(&FOG, height, 0.0, 300_000f64.hypot(height));
            assert!(haze < previous);
            previous = haze;
        }
        assert!(
            haze_amount(&FOG, 4000.0, 4000.0, 100_000.0)
                < haze_amount(&FOG, 0.0, 0.0, 100_000.0) / 10.0
        );
    }

    #[test]
    fn defaults_haze_the_ground_within_kilometres_and_clear_from_the_ceiling() {
        assert_eq!(
            DEFAULT_FOG,
            FogSettings {
                distance_m: 4000.0,
                density: 0.5,
                height_m: 1500.0
            }
        );
        assert!((haze_amount(&DEFAULT_FOG, EYE, 0.0, 4000.0) - 0.25).abs() < 0.05);
        assert!(haze_amount(&DEFAULT_FOG, EYE, 0.0, 50_000.0) > 0.49);
        for r in [0.0, RADIUS / 2.0, RADIUS] {
            assert!(haze_amount(&DEFAULT_FOG, TOP, 0.0, r.hypot(TOP)) < DEFAULT_FOG.density / 3.0);
            assert!(haze_amount(&FOG, TOP, 0.0, r.hypot(TOP)) < 0.05);
        }
    }

    #[test]
    fn stays_finite_at_the_extremes_and_is_off_at_zero_density() {
        let mid = world::MIDPLANE_Y as f64;
        for height_m in [HEIGHT_LIMITS.0, HEIGHT_LIMITS.1] {
            for distance_m in [DISTANCE_LIMITS.0, DISTANCE_LIMITS.1] {
                let fog = FogSettings {
                    distance_m,
                    density: 1.0,
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
                    ..FOG
                },
                0.0,
                0.0,
                1e7
            ),
            0.0
        );
        assert_eq!(DISTANCE_LIMITS.1, 2.0 * RADIUS);
    }

    #[test]
    fn sanitizes_stored_settings() {
        assert_eq!(sanitize_fog(None, None, None), DEFAULT_FOG);
        assert_eq!(
            sanitize_fog(Some(1e12), Some(-1.0), Some(f64::NAN)),
            FogSettings {
                distance_m: DISTANCE_LIMITS.1,
                density: 0.0,
                height_m: DEFAULT_FOG.height_m
            }
        );
    }
}
