//! The sky frame (BIFACIAL_WORLD.md §6, ARCHITECTURE.md §6.3): the day sky's zenith, the sun and
//! the moon, which turn together about one axis. At angle 0 they are the static sky; π turns day
//! and night over.

use crate::{Rgb, moon_direction_world, normalize, sun_direction};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    pub day_pole: Rgb,
    pub sun: Rgb,
    pub moon: Rgb,
}

/// Horizontal, perpendicular to the sun's azimuth: the sun's elevation changes with the angle.
pub fn sky_axis() -> Rgb {
    let s = sun_direction();
    normalize([s[2], 0.0, -s[0]])
}

/// `v` turned by `angle` about the unit axis `k` (Rodrigues' formula, right-handed).
fn rotate(v: Rgb, k: Rgb, angle: f64) -> Rgb {
    let (c, s) = (angle.cos(), angle.sin());
    let dot = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
    let cross = [
        k[1] * v[2] - k[2] * v[1],
        k[2] * v[0] - k[0] * v[2],
        k[0] * v[1] - k[1] * v[0],
    ];
    std::array::from_fn(|i| v[i] * c + cross[i] * s + k[i] * dot * (1.0 - c))
}

/// The frame turned by `angle` radians about the sky axis.
pub fn sky_frame(angle: f64) -> SkyFrame {
    let k = sky_axis();
    SkyFrame {
        day_pole: rotate([0.0, 1.0, 0.0], k, angle),
        sun: rotate(sun_direction(), k, angle),
        moon: rotate(moon_direction_world(), k, angle),
    }
}

/// The static sky: the frame at angle 0.
pub fn sky_rest() -> SkyFrame {
    SkyFrame {
        day_pole: [0.0, 1.0, 0.0],
        sun: sun_direction(),
        moon: moon_direction_world(),
    }
}

/// The debug switches `?skyrot=<degrees>` and `?skyspin=<degrees per second>`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkySwitches {
    pub rotate_deg: f64,
    pub spin_deg_per_second: f64,
}

/// Reads the switches from a query string; anything missing or not a number is 0.
pub fn sky_switches(search: &str) -> SkySwitches {
    let read = |key: &str| {
        search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    };
    SkySwitches {
        rotate_deg: read("skyrot"),
        spin_deg_per_second: read("skyspin"),
    }
}

/// The sky's angle (radians, 0 up to 2π): the settings slider plus `?skyrot` plus the spin over
/// `seconds`. Client-side only.
pub fn sky_angle(setting_deg: f64, s: SkySwitches, seconds: f64) -> f64 {
    let deg = setting_deg + s.rotate_deg + s.spin_deg_per_second * seconds;
    (deg % 360.0 + 360.0) % 360.0 * std::f64::consts::PI / 180.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn dot(a: Rgb, b: Rgb) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn near(a: Rgb, b: Rgb) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-9, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn is_the_static_sky_at_angle_0() {
        let f = sky_frame(0.0);
        near(f.day_pole, [0.0, 1.0, 0.0]);
        near(f.sun, sun_direction());
        near(f.moon, moon_direction_world());
        near(sky_rest().sun, sun_direction());
        near(sky_rest().moon, moon_direction_world());
        assert_eq!(sky_frame(0.0), sky_rest());
    }

    #[test]
    fn turns_day_and_night_over_at_pi() {
        let f = sky_frame(PI);
        near(f.day_pole, [0.0, -1.0, 0.0]);
        near(f.sun, moon_direction_world());
        near(f.moon, sun_direction());
    }

    #[test]
    fn keeps_unit_vectors_and_angles_with_a_horizontal_axis() {
        let s = sun_direction();
        assert_eq!(sky_axis()[1], 0.0);
        assert!(dot(sky_axis(), [s[0], 0.0, s[2]]).abs() < 1e-12);
        for angle in [0.3, 1.0, 2.5, 4.0, 6.0] {
            let f = sky_frame(angle);
            for v in [f.day_pole, f.sun, f.moon] {
                assert!((dot(v, v).sqrt() - 1.0).abs() < 1e-12);
            }
            assert!((dot(f.day_pole, f.sun) - dot([0.0, 1.0, 0.0], s)).abs() < 1e-12);
            assert!((dot(f.sun, f.moon) + 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn reads_the_switches_and_wraps_the_angle() {
        assert_eq!(
            sky_switches("?skyrot=90&skyspin=-12.5"),
            SkySwitches {
                rotate_deg: 90.0,
                spin_deg_per_second: -12.5
            }
        );
        assert_eq!(sky_switches("?skyrot=abc"), SkySwitches::default());
        assert_eq!(sky_switches(""), SkySwitches::default());
        let none = SkySwitches::default();
        assert_eq!(sky_angle(0.0, none, 100.0), 0.0);
        assert!(
            (sky_angle(
                90.0,
                SkySwitches {
                    rotate_deg: 90.0,
                    ..none
                },
                0.0
            ) - PI)
                .abs()
                < 1e-12
        );
        assert!(
            (sky_angle(
                0.0,
                SkySwitches {
                    spin_deg_per_second: 90.0,
                    ..none
                },
                2.0
            ) - PI)
                .abs()
                < 1e-12
        );
        assert!(
            (sky_angle(
                0.0,
                SkySwitches {
                    rotate_deg: -90.0,
                    ..none
                },
                0.0
            ) - 1.5 * PI)
                .abs()
                < 1e-12
        );
        assert_eq!(sky_angle(360.0, none, 0.0), 0.0);
    }
}
