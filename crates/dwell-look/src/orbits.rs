//! Orbits by seed (docs/proposals/REALISTIC_WORLD.md §8, O1–O2; owner 2026-11-12: "Orbits should
//! be seed determined."): a world's sun path, distance, tilt, year, clock and moon rolled from its
//! seed in a preset's ranges (`shared/look/orbits.json`, generated into `orbits.gen.rs`), kept as
//! the world's settings (ADR 0054: the clock in `CLOCK`, the moon's phase in `MOON_PHASE`, the rest
//! in `CELESTIAL`), and drawn by the sky ([`Orbit::day_night`], `celestial::Celestial::for_world`).
//!
//! - **Today's orbit** ([`Orbit::today`]) is read from `daynight.json`: the pole 90° − the noon
//!   elevation (30°: face A at 30° N, face B at 30° S), its yaw the sun's rising point, distance 1,
//!   no tilt, today's clock and moon. A world without `CELESTIAL` (every default and starter world)
//!   has it, and its sky is today's exactly.
//! - **The roll** ([`Orbit::roll`], pure): each value uniform in its preset's range from its own
//!   stream of the seed (so a range added later doesn't move the others), the tilt within
//!   `[min, min(max, maxPoleDeg − pole)]`, the day rounded to whole seconds.
//! - **Not yet read:** the tilt, the year and its phase are rolled and kept for the seasons (O4).

#[rustfmt::skip]
#[allow(clippy::all)]
mod data {
    include!("orbits.gen.rs");
}
pub use data::ORBITS;

use crate::daynight::{Clock, DAY_NIGHT, DayNight};
use dwell_protocol::world_settings::{WorldClock, WorldOrbit, WorldSettings};

/// A preset's ranges for the moon (`orbits.json` `roll.moon`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonRanges {
    pub cycle_days: [f64; 2],
    pub inclination_deg: [f64; 2],
    pub node_deg: [f64; 2],
    pub phase: [f64; 2],
}

/// A preset's ranges (`orbits.json` `presets.<name>.roll`), each [min, max].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitRanges {
    pub pole_deg: [f64; 2],
    pub pole_yaw_deg: [f64; 2],
    pub distance: [f64; 2],
    pub tilt_deg: [f64; 2],
    pub year_days: [f64; 2],
    pub year_phase: [f64; 2],
    pub day_length_s: [f64; 2],
    pub day_share: [f64; 2],
    pub moon: MoonRanges,
}

/// A Sky choice (`orbits.json` `presets`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitPreset {
    pub name: &'static str,
    pub label: &'static str,
    pub roll: OrbitRanges,
}

/// What every preset keeps for play (`orbits.json` `limits`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitLimits {
    /// The pole plus the tilt at most this (°): the noon sun high enough in every season.
    pub max_pole_deg: f64,
    /// Each face's daylight share within this.
    pub day_share: [f64; 2],
    /// The distance's temperature offset (°C) within this.
    pub offset_c: [f64; 2],
}

/// The presets and limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitRules {
    /// The Sky choices, `default` first.
    pub presets: &'static [OrbitPreset],
    pub limits: OrbitLimits,
}

impl OrbitRules {
    /// The preset named `name`.
    pub fn preset(&self, name: &str) -> Option<&OrbitPreset> {
        self.presets.iter().find(|p| p.name == name)
    }
}

/// A world's orbit (REALISTIC_WORLD.md §8.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    /// The celestial pole's height over face A's horizon (°): face A's latitude N, face B's S.
    pub pole_deg: f64,
    /// The pole's yaw: the sun's rising point (°, 0 = +Z, 90 = +X).
    pub pole_yaw_deg: f64,
    /// From the sun, relative to today's.
    pub distance: f64,
    /// The axial tilt (°; seasons, O4).
    pub tilt_deg: f64,
    /// The year (world days) and where in it day 0 is (0–1).
    pub year_days: f64,
    pub year_phase: f64,
    /// Face A's daylight and night (s).
    pub day_s: f64,
    pub night_s: f64,
    /// The moon's month (days, new to new), inclination, ascending node at day 0 (°) and phase at
    /// day 0 (0 new, 0.5 full).
    pub moon_cycle_days: f64,
    pub moon_inclination_deg: f64,
    pub moon_node_deg: f64,
    pub moon_phase: f64,
}

/// SplitMix64: one stream per value, from the seed and the value's salt.
fn unit_draw(seed: u64, salt: u64) -> f64 {
    let mut z = seed ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x6f72_6269_7473_0001;
    for _ in 0..2 {
        z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
    }
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn lerp([lo, hi]: [f64; 2], t: f64) -> f64 {
    lo + (hi - lo) * t
}

impl Orbit {
    /// Today's orbit: `daynight.json`'s path, clock and moon (the default and starter worlds').
    pub fn today() -> Self {
        let d = DAY_NIGHT;
        Self {
            pole_deg: 90.0 - d.sun_path.noon_elevation_deg,
            pole_yaw_deg: d.sun_path.rise_yaw_deg,
            distance: 1.0,
            tilt_deg: 0.0,
            year_days: 1.0,
            year_phase: 0.0,
            day_s: d.clock.day_s,
            night_s: d.clock.night_s,
            moon_cycle_days: d.moon.cycle_days,
            moon_inclination_deg: d.moon_orbit.inclination_deg,
            moon_node_deg: d.moon_orbit.node_deg,
            moon_phase: d.moon.start_phase,
        }
    }

    /// The orbit `world_seed` rolls in `preset`'s ranges under `limits` (pure: the same seed and
    /// preset give the same orbit).
    pub fn roll(world_seed: u64, preset: &OrbitPreset, limits: &OrbitLimits) -> Self {
        let r = &preset.roll;
        let at = |salt: u64, range: [f64; 2]| lerp(range, unit_draw(world_seed, salt));
        let pole_deg = at(1, r.pole_deg).min(limits.max_pole_deg);
        let tilt_hi = r.tilt_deg[1].min(limits.max_pole_deg - pole_deg).max(0.0);
        let tilt_lo = r.tilt_deg[0].min(tilt_hi);
        let length = at(7, r.day_length_s).round();
        let share = at(8, r.day_share).clamp(limits.day_share[0], limits.day_share[1]);
        let day_s = (length * share).round().clamp(1.0, length - 1.0);
        Self {
            pole_deg,
            pole_yaw_deg: at(2, r.pole_yaw_deg).rem_euclid(360.0),
            distance: at(3, r.distance),
            tilt_deg: at(4, [tilt_lo, tilt_hi]),
            year_days: at(5, r.year_days),
            year_phase: at(6, r.year_phase).rem_euclid(1.0),
            day_s,
            night_s: length - day_s,
            moon_cycle_days: at(9, r.moon.cycle_days),
            moon_inclination_deg: at(10, r.moon.inclination_deg),
            moon_node_deg: at(11, r.moon.node_deg).rem_euclid(360.0),
            moon_phase: at(12, r.moon.phase).rem_euclid(1.0),
        }
    }

    /// The orbit a world created with Sky preset `name` rolls (None: no such preset).
    pub fn roll_preset(world_seed: u64, name: &str) -> Option<Self> {
        Some(Self::roll(world_seed, ORBITS.preset(name)?, &ORBITS.limits))
    }

    /// The world's orbit: its `CELESTIAL` record with its clock and moon's phase, or today's orbit
    /// with them (a world without one).
    pub fn for_world(s: &WorldSettings) -> Self {
        let today = Self::today();
        let mut o = match s.orbit {
            Some(w) => Self {
                pole_deg: f64::from(w.pole_deg),
                pole_yaw_deg: f64::from(w.pole_yaw_deg),
                distance: f64::from(w.distance),
                tilt_deg: f64::from(w.tilt_deg),
                year_days: f64::from(w.year_days),
                year_phase: f64::from(w.year_phase),
                moon_cycle_days: f64::from(w.moon_cycle_days),
                moon_inclination_deg: f64::from(w.moon_inclination_deg),
                moon_node_deg: f64::from(w.moon_node_deg),
                ..today
            },
            None => today,
        };
        o.day_s = f64::from(s.clock.day_s);
        o.night_s = f64::from(s.clock.night_s);
        if let Some(p) = s.moon_phase {
            o.moon_phase = f64::from(p);
        }
        o
    }

    /// `s` with this orbit written in: its clock's day and night, its moon's phase and its
    /// `CELESTIAL` record (a world creating its sky).
    pub fn into_settings(&self, s: WorldSettings) -> WorldSettings {
        WorldSettings {
            clock: WorldClock {
                day_s: self.day_s as f32,
                night_s: self.night_s as f32,
                ..s.clock
            },
            moon_phase: Some(self.moon_phase as f32),
            orbit: Some(WorldOrbit {
                pole_deg: self.pole_deg as f32,
                pole_yaw_deg: self.pole_yaw_deg as f32,
                distance: self.distance as f32,
                tilt_deg: self.tilt_deg as f32,
                year_days: self.year_days as f32,
                year_phase: self.year_phase as f32,
                moon_cycle_days: self.moon_cycle_days as f32,
                moon_inclination_deg: self.moon_inclination_deg as f32,
                moon_node_deg: self.moon_node_deg as f32,
            }),
            ..s
        }
    }

    /// The day/night cycle drawn for this orbit over `base` (this build's data, its looks kept):
    /// the sun's path from the pole (noon 90° − pole, rising at the pole's yaw), the clock's day
    /// and night, the moon's month, phase and orbit. The sun's size by distance is not drawn yet
    /// (the sky shader's `SUN_RADIUS` is a constant): a later look.
    pub fn day_night(&self, base: DayNight) -> DayNight {
        let mut d = base;
        d.sun_path.noon_elevation_deg = 90.0 - self.pole_deg;
        d.sun_path.rise_yaw_deg = self.pole_yaw_deg;
        d.clock = Clock {
            day_s: self.day_s,
            night_s: self.night_s,
            ..base.clock
        };
        d.moon.cycle_days = self.moon_cycle_days;
        d.moon.start_phase = self.moon_phase;
        d.moon_orbit.inclination_deg = self.moon_inclination_deg;
        d.moon_orbit.node_deg = self.moon_node_deg;
        d
    }

    /// Face A's daylight share of a whole day (face B has the rest).
    pub fn day_share(&self) -> f64 {
        self.day_s / (self.day_s + self.night_s)
    }

    /// The distance's temperature offset (°C, REALISTIC_WORLD.md §8.2): radiative balance,
    /// 288 K × (distance^−½ − 1), clamped to `limits.offsetC`.
    pub fn offset_c(&self, limits: &OrbitLimits) -> f64 {
        (288.0 * (self.distance.powf(-0.5) - 1.0)).clamp(limits.offset_c[0], limits.offset_c[1])
    }
}

/// The world's day/night cycle (its clock, its orbit's sky; this build's looks): what the
/// server's celestial model and every client's sky draw.
pub fn day_night_for(s: &WorldSettings) -> DayNight {
    let start_hour = f64::from(s.clock.start_hour);
    let mut d = Orbit::for_world(s).day_night(DAY_NIGHT);
    d.clock.start_hour = start_hour;
    d
}

#[cfg(test)]
mod tests;
