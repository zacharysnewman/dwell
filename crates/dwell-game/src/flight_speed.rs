//! The flight speed slider (PLAYER_CONTROLLER.md §6.7): a level carried in every input frame
//! (`input_buttons::FLY_SPEED`). Level 0 flies at the height-based speed alone; level L flies at
//! least fly.speed × 2^(L/2) — also below WORLD_MAX_Y, where only the height-based speed is capped.
//! The controller applies it (`dwell-physics`); this module only mirrors it for display. Keeping the
//! chosen level between sessions is the shell's job: [`fly_speed_level_from_stored`] and
//! [`fly_speed_level_to_stored`] turn it to and from the stored text.

use std::f64::consts::SQRT_2;

use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::players;

use crate::js_round;

pub const MAX_FLY_SPEED_LEVEL: u8 = players::FLY_SPEED_MAX_LEVEL as u8;
/// The storage key the browser shell keeps the level under.
pub const FLY_SPEED_STORAGE_KEY: &str = "dwell.flySpeed";

/// fly.speed in the controller config (m/s), for showing speeds.
pub fn fly_base_speed() -> f64 {
    f64::from(PlayerControllerConfig::default().fly.speed)
}

/// fly.terrainSpeed: the height-based speed's cap below WORLD_MAX_Y (m/s).
pub fn fly_terrain_speed() -> f64 {
    f64::from(PlayerControllerConfig::default().fly.terrain_speed)
}

/// Rounds to the nearest level in range; NaN and infinities are level 0.
pub fn clamp_fly_speed_level(level: f64) -> u8 {
    if level.is_finite() {
        js_round(level).clamp(0.0, f64::from(MAX_FLY_SPEED_LEVEL)) as u8
    } else {
        0
    }
}

/// 2^(level / 2), as the controller's fly speed factor.
pub fn fly_speed_factor(level: f64) -> f64 {
    let l = i32::from(clamp_fly_speed_level(level));
    (if l % 2 == 1 { SQRT_2 } else { 1.0 }) * 2f64.powi(l / 2)
}

/// The slowest the chosen level flies (without Run), in m/s.
pub fn fly_speed_floor(level: f64) -> f64 {
    fly_base_speed() * fly_speed_factor(level)
}

/// JavaScript's `Number.prototype.toFixed` for non-negative `x` (exact ties round up).
fn to_fixed(x: f64, digits: usize) -> String {
    // The exact decimal expansion (an f64's is finite), then rounded half up by hand.
    let exact = format!("{x:.1100}");
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut digits_str: Vec<u8> = int.bytes().chain(frac.bytes().take(digits)).collect();
    let rest = &frac.as_bytes()[digits.min(frac.len())..];
    if rest.first().is_some_and(|&d| d >= b'5') {
        let mut i = digits_str.len();
        loop {
            if i == 0 {
                digits_str.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits_str[i] == b'9' {
                digits_str[i] = b'0';
            } else {
                digits_str[i] += 1;
                break;
            }
        }
    }
    let split = digits_str.len() - digits;
    let s = String::from_utf8(digits_str).unwrap_or_default();
    if digits == 0 {
        s
    } else {
        format!("{}.{}", &s[..split], &s[split..])
    }
}

/// An integer with en-US thousands separators (`toLocaleString('en-US')`).
fn with_separators(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A speed for the HUD: "45 m/s", "1.2 km/s", "740 km/s", "8,160 km/s".
pub fn format_speed(mps: f64) -> String {
    if mps < 1000.0 {
        return format!("{} m/s", js_round(mps));
    }
    let km = mps / 1000.0;
    if km < 10.0 {
        return format!("{} km/s", to_fixed(km, 1));
    }
    format!("{} km/s", with_separators(js_round(km) as u64))
}

/// The slider's caption for a level.
pub fn fly_speed_label(level: f64) -> String {
    let l = clamp_fly_speed_level(level);
    if l == 0 {
        return "Normal (faster with height)".into();
    }
    format!("At least {}", format_speed(fly_speed_floor(f64::from(l))))
}

/// The level from its stored text (0 if none or unreadable), as `Number(text)` reads it.
pub fn fly_speed_level_from_stored(stored: Option<&str>) -> u8 {
    let text = stored.unwrap_or("0").trim();
    let n = if text.is_empty() {
        0.0
    } else {
        text.parse::<f64>().unwrap_or(f64::NAN)
    };
    clamp_fly_speed_level(n)
}

/// The text to store for a level.
pub fn fly_speed_level_to_stored(level: f64) -> String {
    clamp_fly_speed_level(level).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_close;

    const MAX: f64 = MAX_FLY_SPEED_LEVEL as f64;

    #[test]
    fn double_every_two_levels() {
        assert_eq!(fly_speed_factor(0.0), 1.0);
        assert_eq!(fly_speed_factor(1.0), SQRT_2);
        assert_eq!(fly_speed_factor(2.0), 2.0);
        assert_eq!(fly_speed_factor(20.0), 1024.0);
        assert_eq!(fly_speed_factor(MAX), SQRT_2 * 2f64.powi(19));
    }

    #[test]
    fn reach_about_the_speed_near_the_flight_ceiling_at_the_top() {
        // The height-based factor at 24,000 km is 1 + 24e6 / 32 ≈ 750,000.
        assert!(fly_speed_factor(MAX) <= 1.0 + 24e6 / 32.0);
        assert!(fly_speed_factor(MAX) > 0.95 * (1.0 + 24e6 / 32.0));
        assert_close(fly_speed_floor(MAX), fly_base_speed() * 741455.2, -2);
    }

    #[test]
    fn clamp_levels() {
        assert_eq!(clamp_fly_speed_level(-3.0), 0);
        assert_eq!(clamp_fly_speed_level(2.6), 3);
        assert_eq!(clamp_fly_speed_level(500.0), MAX_FLY_SPEED_LEVEL);
        assert_eq!(clamp_fly_speed_level(f64::NAN), 0);
    }

    #[test]
    fn describe_speeds() {
        assert_eq!(format_speed(44.6), "45 m/s");
        assert_eq!(format_speed(1234.0), "1.2 km/s");
        assert_eq!(format_speed(8_156_007.0), "8,156 km/s");
        assert_eq!(fly_speed_label(0.0), "Normal (faster with height)");
        assert_eq!(fly_speed_label(8.0), "At least 176 m/s");
    }

    // Beyond the TS suite: the JS number formatting this mirrors, and the stored level.
    #[test]
    fn formats_like_javascript() {
        assert_eq!(to_fixed(1.25, 1), "1.3"); // an exact tie rounds up, as toFixed does
        assert_eq!(to_fixed(9.96, 1), "10.0");
        assert_eq!(format_speed(1_000_000_000.0), "1,000,000 km/s");
        assert_eq!(fly_base_speed(), 11.0);
        assert_eq!(fly_terrain_speed(), 400.0);
    }

    #[test]
    fn reads_and_writes_the_stored_level() {
        assert_eq!(fly_speed_level_from_stored(None), 0);
        assert_eq!(fly_speed_level_from_stored(Some("7")), 7);
        assert_eq!(fly_speed_level_from_stored(Some("junk")), 0);
        assert_eq!(fly_speed_level_from_stored(Some("99")), MAX_FLY_SPEED_LEVEL);
        assert_eq!(fly_speed_level_to_stored(5.4), "5");
    }
}
