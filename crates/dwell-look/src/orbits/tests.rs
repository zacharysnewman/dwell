use super::*;
use crate::celestial::Celestial;
use dwell_protocol::world_settings::{SettingsFields, WorldClock};
use dwell_world::both_faces;

const SEEDS: u64 = 10_000;

fn today_settings() -> WorldSettings {
    let c = DAY_NIGHT.clock;
    WorldSettings {
        clock: WorldClock {
            day_s: c.day_s as f32,
            night_s: c.night_s as f32,
            start_hour: c.start_hour as f32,
        },
        water_speed: 1.0,
        mode: "creative",
        size: dwell_protocol::world_size::WorldSize::DEFAULT,
        moon_phase: None,
        orbit: None,
        theater_guests: true,
    }
}

fn within(v: f64, [lo, hi]: [f64; 2], what: &str) {
    assert!(
        v >= lo - 1e-9 && v <= hi + 1e-9,
        "{what} {v} outside [{lo}, {hi}]"
    );
}

#[test]
fn todays_orbit_draws_todays_sky_exactly() {
    // The golden: no orbit (every default and starter world) is today's cycle, field for field.
    let s = today_settings();
    assert_eq!(day_night_for(&s), DAY_NIGHT);
    assert_eq!(Orbit::today().day_night(DAY_NIGHT), DAY_NIGHT);
    // The celestial model, sun and moon over a month, as before orbits.
    let before = Celestial::new(
        DayNight {
            clock: Clock {
                day_s: 800.0,
                night_s: 400.0,
                start_hour: f64::from(s.clock.start_hour),
            },
            ..DAY_NIGHT
        },
        None,
    );
    let now = Celestial::for_world(&s);
    for i in 0..400 {
        let t = f64::from(i) * 97.3;
        assert_eq!(now.sun_direction(t), before.sun_direction(t));
        assert_eq!(now.moon_position(t), before.moon_position(t));
    }
}

#[test]
fn the_same_seed_rolls_the_same_orbit_and_different_seeds_differ() {
    for p in ORBITS.presets {
        let a = Orbit::roll(7, p, &ORBITS.limits);
        assert_eq!(a, Orbit::roll(7, p, &ORBITS.limits), "{}", p.name);
        assert_ne!(a.pole_deg, Orbit::roll(8, p, &ORBITS.limits).pole_deg);
        assert_ne!(
            a.moon_node_deg,
            Orbit::roll(8, p, &ORBITS.limits).moon_node_deg
        );
    }
    assert_eq!(Orbit::roll_preset(7, "nope"), None);
}

#[test]
fn every_value_is_within_its_presets_range_and_the_limits_hold_over_10000_seeds() {
    let l = ORBITS.limits;
    for p in ORBITS.presets {
        let r = p.roll;
        for seed in 0..SEEDS {
            let o = Orbit::roll(seed.wrapping_mul(0x2545_f491_4f6c_dd1d), p, &l);
            within(o.pole_deg, r.pole_deg, "pole");
            within(o.distance, r.distance, "distance");
            within(o.year_days, r.year_days, "year");
            within(o.moon_cycle_days, r.moon.cycle_days, "month");
            within(
                o.moon_inclination_deg,
                r.moon.inclination_deg,
                "inclination",
            );
            within(o.tilt_deg, [0.0, r.tilt_deg[1]], "tilt");
            within(o.day_s + o.night_s, r.day_length_s, "day length");
            assert!((0.0..360.0).contains(&o.pole_yaw_deg));
            assert!((0.0..360.0).contains(&o.moon_node_deg));
            assert!((0.0..1.0).contains(&o.moon_phase));
            // The noon sun high enough at the winter solstice.
            assert!(
                o.pole_deg + o.tilt_deg <= l.max_pole_deg + 1e-9,
                "{}",
                p.name
            );
            // Both faces get days and nights (seasons not drawn yet: both solstices share this).
            within(o.day_share(), l.day_share, "face A's share");
            within(1.0 - o.day_share(), l.day_share, "face B's share");
            assert_eq!(o.day_s, o.day_s.round(), "whole seconds");
            // What the world keeps is readable back.
            assert!(o.into_settings(today_settings()).orbit.unwrap().valid());
        }
    }
}

#[test]
fn the_default_preset_keeps_the_20_minute_day_and_todays_split() {
    for seed in 0..200 {
        let o = Orbit::roll_preset(seed, "default").unwrap();
        assert_eq!((o.day_s, o.night_s), (800.0, 400.0));
    }
    let varies: Vec<_> = (0..200)
        .map(|s| Orbit::roll_preset(s, "day_varies").unwrap().day_s)
        .collect();
    assert!(
        varies.iter().any(|d| *d != 800.0),
        "Day varies rolls the day"
    );
}

#[test]
fn a_fixture_preset_added_by_data_alone_is_rolled() {
    let base = ORBITS.presets[0];
    let tropic = OrbitPreset {
        name: "tropic",
        label: "Tropic",
        roll: OrbitRanges {
            pole_deg: [10.0, 12.0],
            ..base.roll
        },
    };
    let presets: &'static [OrbitPreset] = Box::leak(Box::new([base, tropic]));
    let rules = OrbitRules { presets, ..ORBITS };
    let p = rules.preset("tropic").unwrap();
    for seed in 0..100 {
        within(
            Orbit::roll(seed, p, &rules.limits).pole_deg,
            [10.0, 12.0],
            "pole",
        );
    }
}

#[test]
fn a_rolled_orbit_round_trips_through_the_world_settings() {
    let o = Orbit::roll_preset(42, "extreme").unwrap();
    let s = o.into_settings(today_settings());
    let back = SettingsFields::default()
        .with(s)
        .settings(today_settings())
        .unwrap();
    let r = Orbit::for_world(&back);
    let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-4, "{a} vs {b}");
    close(r.pole_deg, o.pole_deg);
    close(r.pole_yaw_deg, o.pole_yaw_deg);
    close(r.distance, o.distance);
    close(r.moon_node_deg, o.moon_node_deg);
    close(r.moon_phase, o.moon_phase);
    assert_eq!((r.day_s, r.night_s), (o.day_s, o.night_s));
}

both_faces!(
    fn a_rolled_orbits_noon_is_90_less_the_pole_on_both_faces(f: FaceFrame) {
        for seed in [1_u64, 99, 12_345] {
            let o = Orbit::roll_preset(seed, "extreme").unwrap();
            let d = o.day_night(DAY_NIGHT);
            // The face's noon: the sun's highest over its horizon around the whole path.
            let up = f.up();
            let el = (0..36_000)
                .map(|i| {
                    let sun = d.sun_direction(f64::from(i) * 0.01);
                    (sun[0] * up[0] + sun[1] * up[1] + sun[2] * up[2])
                        .asin()
                        .to_degrees()
                })
                .fold(f64::MIN, f64::max);
            assert!(
                (el - (90.0 - o.pole_deg)).abs() < 1e-6,
                "{el} vs pole {}",
                o.pole_deg
            );
            // The sky turns about the pole: the axis stands the pole's height over face A's
            // horizon and as high over face B's (its other end).
            let axis = d.sky_axis();
            let pole = (axis[0] * up[0] + axis[1] * up[1] + axis[2] * up[2])
                .abs()
                .asin()
                .to_degrees();
            assert!((pole - o.pole_deg).abs() < 1e-9);
        }
    }
);

#[test]
fn a_farther_orbit_is_colder() {
    let near = Orbit {
        distance: 0.97,
        ..Orbit::today()
    };
    let far = Orbit {
        distance: 1.03,
        ..Orbit::today()
    };
    assert!(near.offset_c(&ORBITS.limits) > 0.0);
    assert!(far.offset_c(&ORBITS.limits) < 0.0);
    assert_eq!(Orbit::today().offset_c(&ORBITS.limits), 0.0);
}
