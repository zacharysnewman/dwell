use super::*;
use crate::daynight::MoonOrbit;

const C: fn() -> Celestial = || Celestial::new(DAY_NIGHT, None);

fn day_s() -> f64 {
    DAY_NIGHT.day_length_s()
}

/// The world time at which the month's count reads `phase` in month `n` (0 new, 0.5 full).
fn at_phase(c: &Celestial, phase: f64, n: f64) -> f64 {
    let m = c.day.moon.cycle_days;
    let days = ((phase - c.start_phase).rem_euclid(1.0) + n) * m;
    (days - c.day.clock.start_hour / 24.0) * c.day.day_length_s()
}

/// A few viewers on a face: face-local offsets over the home midplane, through the face frame.
fn viewers(f: FaceFrame) -> Vec<V3> {
    [
        [0.0, 72.0, 0.0],
        [20_000.0, 60.0, -9_000.0],
        [-14_000.0, 140.0, 17_000.0],
    ]
    .into_iter()
    .map(|p| f.vector(p))
    .collect()
}

fn close(a: V3, b: V3, eps: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= eps)
}

#[test]
fn the_clock_and_the_suns_path_are_unchanged() {
    // Golden: the sun at these world times before the celestial model (0.54.43).
    let golden: [(f64, f64, V3); 5] = [
        (
            0.0,
            48.460652149512306,
            [0.6623344531684241, 0.6482204392542615, 0.3756638580953682],
        ),
        (
            300.0,
            96.03424527831947,
            [0.37206669489256805, 0.8612269664408299, -0.3462000676297542],
        ),
        (
            977.5,
            329.69604349726205,
            [0.2284057947119086, -0.4369853713209041, 0.8699853896437302],
        ),
        (
            5_000.0,
            83.96575472168063,
            [0.4803512490730835, 0.8612269664408301, -0.16598430585164947],
        ),
        (
            123_456.0,
            2.329247117248272,
            [0.5320309656096228, 0.03519686117386804, 0.8459930452409168],
        ),
    ];
    let c = C();
    assert_eq!(
        (
            DAY_NIGHT.clock.day_s,
            DAY_NIGHT.clock.night_s,
            DAY_NIGHT.clock.start_hour
        ),
        (800.0, 400.0, 7.0)
    );
    for (t, solar, sun) in golden {
        assert!((c.solar_deg(t) - solar).abs() < 1e-9, "solar angle at {t}");
        assert!(close(c.sun_direction(t), sun, 1e-12), "sun at {t}");
        assert_eq!(
            c.sun_direction(t),
            DAY_NIGHT.sun_direction(DAY_NIGHT.solar_deg(DAY_NIGHT.time_of_day(t)))
        );
    }
}

#[test]
fn the_moon_is_on_its_orbit_at_todays_size_in_the_sky() {
    let c = C();
    for k in 0..400 {
        let t = f64::from(k) * 37.3;
        let p = c.moon_position(t);
        let d = dot(p, p).sqrt();
        assert!((d - c.day.moon_orbit.distance_m).abs() < 1e-6, "{d}");
        // Within the inclination of the sun's path plane.
        let lat = dot(unit(p), c.day.sky_axis()).asin().to_degrees().abs();
        assert!(lat <= c.day.moon_orbit.inclination_deg + 1e-9, "{lat}");
    }
    // 12.5 km (grids.json's moon, checked by daynight.mjs) at the distance is moon.radiusDeg.
    let r = (12_500.0 / c.day.moon_orbit.distance_m).atan().to_degrees();
    assert!((r - c.day.moon.radius_deg).abs() < 0.01, "{r}");
}

#[test]
fn full_when_opposite_the_sun_new_when_between() {
    let c = C();
    let i = c.day.moon_orbit.inclination_deg.to_radians();
    for n in 0..6 {
        let n = f64::from(n);
        let full = at_phase(&c, 0.5, n);
        let (sun, moon) = (c.sun_direction(full), c.moon_direction(full));
        assert!(dot(sun, moon) <= -(i.cos()) + 1e-9, "opposite at full");
        let v = c.view(full, [0.0, 72.0, 0.0]);
        assert!(
            // (the orbit's tilt leaves full a few degrees off opposite: the regolith's law,
            // `moon.lunarLambert`, loses under 1% of its light there)
            v.lit >= i.cos() - 0.01 && (v.phase - 0.5).abs() < 0.02,
            "{v:?}"
        );
        let new = at_phase(&c, 0.0, n);
        assert!(
            dot(c.sun_direction(new), c.moon_direction(new)) >= i.cos() - 1e-9,
            "between at new"
        );
        let v = c.view(new, [0.0, 72.0, 0.0]);
        assert_eq!(v.lit, 0.0, "{v:?}");
        assert!(v.phase.min(1.0 - v.phase) < 0.02, "{v:?}");
    }
}

#[test]
fn the_lit_share_grows_from_new_to_full_and_wanes_after() {
    let c = C();
    let lit = |p: f64| c.view(at_phase(&c, p, 2.0), [0.0, 72.0, 0.0]).lit;
    let mut last = lit(0.0);
    for k in 1..=100 {
        let l = lit(0.5 * f64::from(k) / 100.0);
        assert!(l >= last - 1e-3, "waxing at {k}: {l} after {last}");
        last = l;
    }
    let mut last = lit(0.5);
    for k in 1..=100 {
        let l = lit(0.5 + 0.5 * f64::from(k) / 100.0);
        assert!(l <= last + 1e-3, "waning at {k}: {l} after {last}");
        last = l;
    }
    // The domed disc (owner: "maybe do that"): a crescent is part-lit, a quarter about half, a
    // gibbous most; the month is not half dark.
    let (crescent, quarter, gibbous) = (lit(0.125), lit(0.25), lit(0.375));
    // (Lambert: the lit share is the light the face sends home, a ball's ~1/π of full at a quarter.)
    assert!(crescent > 0.02 && crescent < quarter, "{crescent}");
    assert!(quarter > 0.25 && quarter < 0.5, "{quarter}");
    assert!(gibbous > 0.5 && gibbous < 0.97, "{gibbous}");
    assert!(lit(0.8) > 0.0);
    eprintln!("lit share: crescent {crescent:.3}, quarter {quarter:.3}, gibbous {gibbous:.3}");
    // The geometric phase follows the month's count (within the inclination).
    for k in 0..64 {
        let p = f64::from(k) / 64.0;
        let v = c.view(at_phase(&c, p, 1.0), [0.0, 72.0, 0.0]);
        let d = (v.phase - p).rem_euclid(1.0);
        assert!(d.min(1.0 - d) < 0.02, "{p}: {}", v.phase);
    }
}

#[test]
fn a_month_is_the_datas_days_and_the_moon_rises_later_each_day() {
    let c = C();
    let month = c.day.moon.cycle_days * day_s();
    for t in [0.0, 1_234.0, 50_000.0] {
        let (a, b) = (c.view(t, [0.0; 3]), c.view(t + month, [0.0; 3]));
        let d = (a.phase - b.phase).rem_euclid(1.0);
        assert!(d.min(1.0 - d) < 0.02, "{a:?} {b:?}");
    }
    // A day later the moon is behind where it was against the sky's turn: 1/month of a turn.
    let t = 4_000.0;
    let along = |t: f64| {
        let m = c.moon_direction(t);
        let f = c.sky_frame(t);
        dot(m, f[1]).atan2(dot(m, f[0])).to_degrees()
    };
    let lag = (along(t) - along(t + day_s())).rem_euclid(360.0);
    assert!((lag - 360.0 / c.day.moon.cycle_days).abs() < 1.5, "{lag}");
}

dwell_world::both_faces!(
    /// Tidally locked: from anywhere on either home face the moon shows its face A, at every
    /// phase, its up pointing at the home disc's centre.
    fn the_moon_shows_home_its_face_a_at_every_phase(f: FaceFrame) {
        let c = C();
        for k in 0..200 {
            let t = f64::from(k) * c.day.moon.cycle_days * day_s() / 197.0;
            let [x, y, z] = c.moon_axes(t);
            assert!((dot(x, x) - 1.0).abs() < 1e-9 && (dot(y, y) - 1.0).abs() < 1e-9);
            assert!(
                dot(x, y).abs() < 1e-9 && dot(cross(x, y), z) > 1.0 - 1e-9,
                "right-handed"
            );
            assert!(
                dot(y, scale(c.moon_direction(t), -1.0)) > 1.0 - 1e-12,
                "up toward home"
            );
            for v in viewers(f) {
                assert_eq!(c.view(t, v).face, FaceFrame::A, "at {t}");
            }
        }
    }
);

dwell_world::both_faces!(
    /// Both home faces see the same phase sequence as face A's.
    fn both_home_faces_see_the_same_phases(f: FaceFrame) {
        let c = C();
        for k in 0..300 {
            let t = f64::from(k) * 211.7;
            for (a, b) in viewers(FaceFrame::A).into_iter().zip(viewers(f)) {
                let (va, vb) = (c.view(t, a), c.view(t, b));
                assert_eq!(va.lit, vb.lit, "at {t}");
                let d = (va.phase - vb.phase).rem_euclid(1.0);
                assert!(d.min(1.0 - d) < 1e-4, "at {t}: {va:?} {vb:?}");
            }
        }
    }
);

dwell_world::both_faces!(
    /// Moonlight never falls under the night floor, is the floor on a moonless night or with the
    /// moon under the face's horizon, and today's whole moonlight from a full moon overhead.
    fn the_moonlight_keeps_its_floor(f: FaceFrame) {
        let c = C();
        let floor = c.day.moon.night_floor;
        assert!(
            floor > 0.0 && floor < DAY_NIGHT.moon.new_moon_light,
            "darker than today, not black"
        );
        let v = f.vector([0.0, 72.0, 0.0]);
        let (mut up, mut down) = (0, 0);
        for k in 0..2_000 {
            let t = f64::from(k) * 97.1;
            let m = c.moonlight(t, v, f);
            assert!((floor - 1e-12..=1.0 + 1e-12).contains(&m), "{m} at {t}");
            let view = c.view(t, v);
            let el = dot(view.direction, f.up());
            if el < -0.05 {
                assert_eq!(m, floor, "under the horizon at {t}");
                down += 1;
            } else if el > 0.05 {
                up += 1;
            }
            if view.lit == 0.0 {
                assert_eq!(m, floor, "moonless at {t}");
            }
        }
        assert!(up > 100 && down > 100, "{up} {down}");
        // The brightest moonlight near a full moon: today's, or nearly (in month 3 of the 8.25-day
        // month the full moon comes a quarter day off a face's midnight, at dusk or dawn).
        let month = c.day.moon.cycle_days * day_s();
        let best = (-100..=100)
            .map(|k| at_phase(&c, 0.5, 3.0) + f64::from(k) * 0.0008 * month)
            .max_by(|a, b| c.moonlight(*a, v, f).total_cmp(&c.moonlight(*b, v, f)))
            .unwrap();
        let view = c.view(best, v);
        assert!(c.moonlight(best, v, f) > 0.9, "{view:?}");
        // The halo is today's × the lit share; the lit part as bright as today's.
        let (bloom, glow) = c.glow(&view);
        assert_eq!(
            (bloom, glow),
            (c.day.moon.bloom * view.lit, c.day.moon.glow * view.lit)
        );
        let a_full = c.view(
            at_phase(&c, 0.5, 3.0),
            FaceFrame::A.vector([0.0, 72.0, 0.0]),
        );
        assert!(
            c.glow(&a_full).1 > 0.99 * c.day.moon.glow,
            "full: today's halo"
        );
        let new = c.view(at_phase(&c, 0.0, 3.0), v);
        assert_eq!(c.glow(&new), (0.0, 0.0), "no halo at new moon");
    }
);

#[test]
fn the_worlds_phase_moves_its_month_and_a_world_without_one_takes_the_datas() {
    use dwell_protocol::world_settings::{WorldClock, WorldSettings};
    let s = WorldSettings {
        clock: WorldClock {
            day_s: 800.0,
            night_s: 400.0,
            start_hour: 7.0,
        },
        water_speed: 1.0,
        mode: "creative",
        size: dwell_protocol::world_size::WorldSize::DEFAULT,
        moon_phase: None,
        orbit: None,
        theater_guests: true,
    };
    let older = Celestial::for_world(&s);
    assert_eq!(older.start_phase, DAY_NIGHT.moon.start_phase);
    assert_eq!(
        older.month_phase(0.0),
        DAY_NIGHT.moon_phase(DAY_NIGHT.days(0.0)),
        "today's count"
    );
    let own = Celestial::for_world(&WorldSettings {
        moon_phase: Some(0.0),
        ..s
    });
    assert_eq!(own.month_phase(at_phase(&own, 0.0, 0.0)), 0.0);
    assert!(own.view(at_phase(&own, 0.0, 0.0), [0.0; 3]).lit == 0.0);
    assert!(older.view(at_phase(&older, 0.5, 0.0), [0.0; 3]).lit > 0.99);
}

#[test]
fn an_orbit_from_data_alone_moves_the_moon() {
    // Fixture data: no tilt, a fixed node, a nearer moon.
    let day = DayNight {
        moon_orbit: MoonOrbit {
            inclination_deg: 0.0,
            node_deg: 0.0,
            node_turn_days: 0.0,
            distance_m: 100_000.0,
        },
        ..DAY_NIGHT
    };
    let c = Celestial::new(day, None);
    for k in 0..50 {
        let t = f64::from(k) * 401.0;
        let p = c.moon_position(t);
        assert!((dot(p, p).sqrt() - 100_000.0).abs() < 1e-6);
        assert!(
            dot(unit(p), day.sky_axis()).abs() < 1e-12,
            "on the sun's path"
        );
    }
    let full = c.view(at_phase(&c, 0.5, 0.0), [0.0; 3]);
    assert!(full.lit > 1.0 - 1e-9, "exactly opposite with no tilt");
}

#[test]
fn the_disc_lit_share_spans_flat_to_ball() {
    use super::disc_lit;
    for c in [-1.0, -0.5, 0.0, 0.3, 0.7, 1.0] {
        assert!(
            (disc_lit(c, 0.0, 0.0) - f64::max(c, 0.0)).abs() < 1e-9,
            "flat at {c}"
        );
    }
    // A ball's light: growing with the phase angle's cosine, a third of full at a quarter.
    let ball = [-0.9, -0.5, 0.0, 0.5, 1.0].map(|c| disc_lit(c, 1.0, 0.0));
    assert!(ball.windows(2).all(|w| w[0] < w[1]), "{ball:?}");
    assert!(ball[2] > 0.2 && ball[2] < 0.5, "{ball:?}");
    assert_eq!(disc_lit(-1.0, 0.6, 0.0), 0.0);
    assert!((disc_lit(1.0, 0.6, 1.0) - 1.0).abs() < 1e-9);
}

/// The radius (of the disc's) where a full moon's light along a radius first falls below half its
/// centre's: the bright disc's size as the sky draws it (`moon_shade` is `dwell_moon`'s).
fn bright_radius(dome: f64, lunar: f64) -> f64 {
    use super::{moon_shade, moon_shape};
    let full = [0.0, 0.0, 1.0];
    let centre = moon_shade(
        moon_shape(0.0, 0.0, dome),
        moon_shape(0.0, 0.0, dome),
        full,
        lunar,
    );
    (0..=1000)
        .map(|k| f64::from(k) / 1000.0)
        .find(|&r| {
            let s = moon_shape(r, 0.0, dome);
            moon_shade(s, s, full, lunar) < 0.5 * centre
        })
        .unwrap_or(1.0)
}

// Owner, 2026-11-12: "The moon looks smaller I think compared to the old moon?" The old moon was a
// flat disc lit evenly to its edge; the domed Lambert ball's bright part ended at 0.87 of it.
#[test]
fn the_full_moon_is_bright_to_its_edge_as_the_old_flat_disc() {
    let old = bright_radius(0.0, 0.0);
    assert_eq!(old, 1.0, "the old flat disc");
    let m = crate::daynight::DAY_NIGHT.moon;
    let now = bright_radius(m.dome_bias, m.lunar_lambert);
    assert!(
        now >= 0.97 * old,
        "the full moon's bright disc: {now} of the old's {old}"
    );
    // And a crescent still reads: at a quarter the half toward the sun lit, the far half dark.
    use super::{moon_shade, moon_shape};
    let sun = [1.0, 0.0, 0.0];
    let at = |x: f64| {
        let s = moon_shape(x, 0.0, m.dome_bias);
        moon_shade(s, s, sun, m.lunar_lambert)
    };
    assert!(at(0.6) > 0.5 && at(-0.3) == 0.0, "{} {}", at(0.6), at(-0.3));
}

#[test]
fn the_regolith_law_keeps_the_phases_ordered() {
    use super::disc_lit;
    let m = crate::daynight::DAY_NIGHT.moon;
    let lit = [-1.0, -0.5, 0.0, 0.5, 1.0].map(|c| disc_lit(c, m.dome_bias, m.lunar_lambert));
    assert!(lit.windows(2).all(|w| w[0] <= w[1]), "{lit:?}");
    assert_eq!(lit[0], 0.0);
    assert!((lit[4] - 1.0).abs() < 1e-9);
    assert!(lit[2] > 0.2 && lit[2] < 0.5, "a quarter: {lit:?}");
}

#[test]
fn the_full_moon_comes_round_the_clock_and_both_faces_get_one_at_midnight() {
    // both-faces: the month's full moons compared between face A's and face B's midnights.
    // Owner, 2026-11-12: "Sounds good" to a fractional month (8.25 days), so the full moon falls
    // at a different clock time each month and each face's nights get full moons in turn.
    let c = C();
    let month = c.day.moon.cycle_days * day_s();
    // Each month's full moon: the brightest disc (geometry) near the count's full.
    let full = |n: f64| {
        let t0 = at_phase(&c, 0.5, n);
        (-200..=200)
            .map(|k| t0 + f64::from(k) * 0.0002 * month)
            .max_by(|a, b| {
                let lit = |t: f64| c.view(t, [0.0; 3]).lit;
                lit(*a).total_cmp(&lit(*b))
            })
            .unwrap()
    };
    let hours: Vec<f64> = (0..4)
        .map(|n| c.day.time_of_day(full(f64::from(n))) * 24.0)
        .collect();
    let apart = |a: f64, b: f64| {
        let d = (a - b).rem_euclid(24.0);
        d.min(24.0 - d)
    };
    for i in 0..4 {
        for j in 0..i {
            assert!(
                apart(hours[i], hours[j]) > 3.0,
                "months {j} and {i} at the same hour: {hours:?}"
            );
        }
    }
    // Face A's midnight is time of day 0, face B's 0.5 (its solar angle is A's + 180°).
    for (f, midnight) in [(FaceFrame::A, 0.0), (FaceFrame::B, 12.0)] {
        let n = (0..4)
            .min_by(|a, b| apart(hours[*a], midnight).total_cmp(&apart(hours[*b], midnight)))
            .unwrap();
        assert!(
            apart(hours[n], midnight) < 2.0,
            "{f:?}: no full moon near its midnight: {hours:?}"
        );
        let t = full(n as f64);
        let v = f.vector([0.0, 72.0, 0.0]);
        let view = c.view(t, v);
        assert!(view.lit > 0.97, "{f:?} month {n}: {view:?}");
        assert!(
            dot(view.direction, f.up()) > 0.5,
            "{f:?}: high in its night sky"
        );
        assert!(c.moonlight(t, v, f) > 0.95, "{f:?}: a full night's light");
    }
}
