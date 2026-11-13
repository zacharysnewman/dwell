//! The view on another grid (docs/proposals/MOON_WORLD.md §7.3, M-7c): while the player is on the
//! moon the game holds the moon's chunks in its own grid-local coordinates and draws in that frame,
//! so the sky's directions (the sun, the moon, the stars' axis) are turned into it, and the moon's
//! own far disc is not drawn (the viewer stands on it), nor home's clouds; a grid without air
//! (`grids.json` `atmosphere` false, the moon) sees space at its ground: black, the stars by day,
//! the sun white (the atmosphere's space path, docs/DAY_NIGHT.md §15, with no air at all).

use dwell_look::daynight::DayState;
use dwell_world::{FaceFrame, GridFrame};

/// The day as seen from grid `frame` (its local frame): the sun, the moon's direction and the
/// sky's axis turned into it; the moon's far disc and home's clouds hidden off home (the viewer is
/// on a grid that is not home, the only one with a moon disc and a cloud deck). Home's frame returns `day` as it is.
pub fn day_on_grid(day: &DayState, frame: &GridFrame, grid: u8) -> DayState {
    if grid == 0 {
        return *day;
    }
    let turn = |v: [f64; 3]| {
        let t = frame.vector_to_grid(v);
        let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-12);
        t.map(|x| x / l)
    };
    DayState {
        sun: turn(day.sun),
        moon: turn(day.moon),
        axis: turn(day.axis),
        moon_shown: false,
        clouds_shown: false,
        airless: !grid_has_air(grid),
        on_moon: dwell_worldgen::grids::grids()
            .get(usize::from(grid))
            .is_some_and(|g| g.name == crate::render::moon::MOON_GRID),
        on_sun: dwell_worldgen::grids::grids()
            .get(usize::from(grid))
            .is_some_and(|g| g.name == crate::render::moon::SUN_GRID),
        ..*day
    }
}

/// The roll (radians about the forward, as the camera's `roll`) that takes the view of look
/// (`yaw`, `pitch`) on grid `to`'s face `tf` back to the view of look (`from_yaw`, `from_pitch`) on
/// grid `from`'s face `ff` drawn before a crossing, when both point the same way in the world
/// (`look_across`): the angle from the new up to the old, about the forward. The view then eases
/// from it upright (`FaceFlip::cross_grid`). 0 looking straight along either up.
#[allow(clippy::too_many_arguments)]
pub fn crossing_roll(
    from: &GridFrame,
    ff: FaceFrame,
    (from_yaw, from_pitch): (f64, f64),
    to: &GridFrame,
    tf: FaceFrame,
    (yaw, pitch): (f64, f64),
) -> f64 {
    use crate::render::camera::aim;
    let old = aim(from_yaw, from_pitch, ff, 0.0);
    let new = aim(yaw, pitch, tf, 0.0);
    let up_old = from.vector_to_world(old.up);
    let (up_new, right_new) = (to.vector_to_world(new.up), to.vector_to_world(new.right));
    let (c, s) = (dot(up_old, up_new), -dot(up_old, right_new));
    if c.hypot(s) < 1e-9 {
        return 0.0;
    }
    s.atan2(c)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The maps' note off home (WORK_QUEUE item 27): the maps show home's terrain only, so on another
/// grid they are hidden and this label shows instead, named by the grid (`dwell:moon` → "No map on
/// the moon"); `None` at home.
pub fn map_away_label(grid: u8) -> Option<String> {
    if grid == 0 {
        return None;
    }
    let name = dwell_worldgen::grids::grids()
        .get(usize::from(grid))
        .map_or("another world", |g| {
            g.name.split_once(':').map_or(g.name.as_str(), |(_, n)| n)
        })
        .replace('_', " ");
    Some(format!("No map on the {name}"))
}

/// Whether grid `grid` (its `grids.json` index) has air (`atmosphere`); an unknown index has.
pub fn grid_has_air(grid: u8) -> bool {
    grid_has_air_in(dwell_worldgen::grids::grids(), grid)
}

/// [`grid_has_air`] over `grids` (a list as `grids.json` parses).
pub fn grid_has_air_in(grids: &[dwell_worldgen::grids::GridDef], grid: u8) -> bool {
    grids.get(usize::from(grid)).is_none_or(|g| g.atmosphere)
}

#[cfg(test)]
mod tests {
    use super::*;

    use dwell_look::atmosphere::{ATMOSPHERE, Air, space_sky};
    use dwell_look::daynight::DAY_NIGHT;

    /// The sky straight up toward `zenith` (the keyframe's), away from the sun, in `air`: the
    /// CPU twin of `dwell_space` (the day layer over space).
    fn up_sky(zenith: [f64; 3], air: Air) -> [f64; 3] {
        space_sky(zenith, zenith, 1.0, -1.0, 1000.0, air)
    }

    fn gap(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
    }

    dwell_world::both_faces!(
        /// The owner (2026-11-13): "Sun and moon should be space, the same as night sky." On a
        /// grid without air (the moon; a sun added by data alone, no `atmosphere`) the day layer
        /// is gone at every hour: the sky straight up is space's colour, the stars out, never a
        /// day colour, on face `f`.
        fn on_a_grid_without_air_day_colours_never_show(f: FaceFrame) {
            let text = dwell_worldgen::grids::GRIDS_JSON;
            let end = text.rfind("\n  ]").unwrap();
            let sun = r#",
    { "name": "test:sun", "frame": "orbit", "radiusM": 9000, "seedSalt": 99, "gravityScale": 2,
      "captureM": 30000, "farM": 150000, "surfaceM": 200,
      "generator": { "kind": "terrain", "bake": { "size": 64 } } }"#;
            let list = dwell_worldgen::grids::parse_grids(&format!(
                "{}{}{}",
                &text[..end],
                sun,
                &text[end..]
            ))
            .expect("a sun by data");
            let sun_index = list.iter().position(|g| g.name == "test:sun").unwrap() as u8;
            assert!(
                !grid_has_air_in(&list, sun_index),
                "the sun declares no air"
            );
            assert!(!grid_has_air(1) && grid_has_air(0), "the moon none, home's");
            let all = dwell_worldgen::grids::grids();
            for name in ["dwell:moon", "dwell:sun"] {
                let i = all.iter().position(|g| g.name == name).expect(name) as u8;
                assert!(!grid_has_air(i), "{name} has no air");
            }
            for k in 0..48 {
                let t = f64::from(k) / 48.0;
                let day = DAY_NIGHT.state(0.0, Some(t), None);
                let on = day_on_grid(&day, &GridFrame::HOME, 1).at_eye(80.0, 0.0, 12_500.0);
                assert_eq!(on.air, Air::SPACE);
                let v = on.of(f);
                let sky = up_sky(v.sky_zenith, on.air);
                assert!(
                    gap(sky, ATMOSPHERE.space_color) < 1e-9,
                    "t {t}: {sky:?} is not space"
                );
                assert_eq!(on.air.stars(v.stars), 1.0, "the stars out at t {t}");
                assert!(on.air.fog() == 0.0, "no haze");
            }
        }
    );

    dwell_world::both_faces!(
        /// Flying up from home's ground on face `f` at noon, the day layer fades to space: the sky
        /// straight up moves steadily from the keyframe's day blue to space's colour at the top.
        fn flying_up_from_home_the_day_layer_fades_to_space(f: FaceFrame) {
            let noon = DAY_NIGHT.state(0.0, Some(if f.is_b() { 0.0 } else { 0.5 }), None);
            let zenith = noon.of(f).sky_zenith;
            let mut last = f64::INFINITY;
            for k in 0..=40 {
                let alt = ATMOSPHERE.top_m * f64::from(k) / 40.0;
                let day = day_on_grid(&noon, &GridFrame::HOME, 0).at_eye(alt, 0.0, 25_000.0);
                let to_space = gap(up_sky(zenith, day.air), ATMOSPHERE.space_color);
                assert!(to_space <= last + 1e-12, "{alt} m: {to_space} after {last}");
                last = to_space;
            }
            assert!(
                gap(zenith, ATMOSPHERE.space_color) > 0.2,
                "day is blue at the ground"
            );
            assert!(last < 1e-9, "space at the top: {last}");
        }
    );

    dwell_world::both_faces!(
        /// Home's day and night as before: at its ground the day layer is whole at every hour
        /// (the sky exactly the keyframes', packed bit for bit as the plain day), on face `f`.
        fn homes_day_and_night_look_as_before(f: FaceFrame) {
            for k in 0..48 {
                let t = f64::from(k) / 48.0;
                let day = DAY_NIGHT.state(0.0, Some(t), None);
                let home = day_on_grid(&day, &GridFrame::HOME, 0).at_eye(80.0, 10.0, 25_000.0);
                assert_eq!(home.air, Air::GROUND, "t {t}");
                assert!(!home.airless);
                assert_eq!(
                    home.pack(f, 1.0, 3.0, 0.001),
                    day.pack(f, 1.0, 3.0, 0.001),
                    "t {t}"
                );
            }
        }
    );

    /// The maps off home: hidden with a label named by the grid; at home none.
    // both-faces: the grid's name; no face enters.
    #[test]
    fn off_home_the_map_is_a_label() {
        assert_eq!(map_away_label(0), None);
        assert_eq!(map_away_label(1).as_deref(), Some("No map on the moon"));
    }

    dwell_world::both_faces!(
        /// Across a crossing from home's face A onto a turned grid's face `f`, the look continued
        /// with the same world direction and the roll of `crossing_roll` drawn: the first frame's
        /// view is the one before it (its forward and its up the same in the world); upright
        /// (roll 0) its up is the new face's.
        fn the_crossing_roll_draws_the_view_before_then_eases_to_the_new_up(f: FaceFrame) {
            use crate::render::camera::aim;
            let to = GridFrame {
                origin: [400_000.0, 300_000.0, -200_000.0],
                rotation: dwell_world::quat_axis_angle([0.6, 0.0, 0.8], 1.1),
                ..GridFrame::HOME
            };
            let from = GridFrame::HOME;
            for (yaw, pitch) in [(10.0, 5.0), (-120.0, -40.0), (75.0, 60.0)] {
                let (y2, p2) =
                    dwell_sim::travel::look_across(&from, FaceFrame::A, &to, f, yaw, pitch);
                let r = crossing_roll(&from, FaceFrame::A, (yaw, pitch), &to, f, (y2, p2));
                let before = aim(yaw, pitch, FaceFrame::A, 0.0);
                let after = aim(y2, p2, f, r);
                let w = |v| to.vector_to_world(v);
                let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6);
                assert!(close(w(after.back), before.back), "the same forward");
                assert!(
                    close(w(after.up), before.up),
                    "the same up: {:?} vs {:?} (roll {r})",
                    w(after.up),
                    before.up
                );
                assert!(r.abs() > 1e-3, "a turned grid's up differs");
                let upright = aim(y2, p2, f, 0.0);
                let face_up = to.up(f);
                assert!(dot(w(upright.up), face_up) > 0.0, "upright on the new face");
            }
        }
    );

    /// Home's view is the day as it is, bit for bit.
    // both-faces: the sky's directions are world vectors; no face enters.
    #[test]
    fn home_keeps_the_day() {
        let day = DayState::default();
        assert_eq!(day_on_grid(&day, &GridFrame::HOME, 0), day);
    }

    dwell_world::both_faces!(
        /// On a turned grid the sun keeps its world direction, now in the grid's frame: its height
        /// over the grid's face `f` is the world sun's over that face's up turned to the world; the
        /// moon's far disc is not drawn.
        fn on_the_moon_the_sun_is_turned_into_its_frame(f: FaceFrame) {
            let day = DayState::default();
            let frame = GridFrame {
                origin: [400_000.0, 300_000.0, -200_000.0],
                // An eighth of a turn about a tilted axis: not the identity.
                rotation: dwell_world::quat_axis_angle(
                    [0.6, 0.0, 0.8],
                    std::f64::consts::FRAC_PI_4,
                ),
                ..GridFrame::HOME
            };
            let on = day_on_grid(&day, &frame, 1);
            assert!(!on.moon_shown && day.moon_shown);
            assert!(
                !on.clouds_shown && day.clouds_shown,
                "no home clouds on the moon"
            );
            let world_up = frame.up(f);
            let local_up = f.vector([0.0, 1.0, 0.0]);
            let rise_world = dot(day.sun, world_up);
            let rise_local = dot(on.sun, local_up);
            assert!(
                (rise_world - rise_local).abs() < 1e-9,
                "{rise_world} {rise_local}"
            );
            assert!((dot(on.sun, on.sun) - 1.0).abs() < 1e-9);
            // The moon has no air: at its ground the sky is space, the stars out by day, the sun
            // white; home's ground sky is today's (BUGS.md #126's sky).
            assert!(on.airless && !day.airless);
            let noon = dwell_look::daynight::DAY_NIGHT.state(0.0, Some(0.5), None);
            let moon_noon = day_on_grid(&noon, &frame, 1).at_eye(80.0, 0.0, 12_500.0);
            assert_eq!(moon_noon.air, dwell_look::atmosphere::Air::SPACE);
            let u = moon_noon.pack(f, 1.0, 0.0, 0.001);
            let space = 23 * 4;
            assert_eq!(u[space], 1.0, "the sky thinned all the way to space");
            assert_eq!(u[space + 2], 0.0, "no limb: no air seen edge-on");
            assert_eq!(u[8 * 4], 1.0, "the stars by day");
            let home_noon = noon.at_eye(80.0, 0.0, 25_000.0);
            assert_eq!(home_noon.air, dwell_look::atmosphere::Air::GROUND);
            assert!((dot(on.sun, day.sun) - 1.0).abs() > 1e-3, "turned");
            assert_eq!(on.time_of_day, day.time_of_day);
        }
    );

    dwell_world::both_faces!(
        /// The moon is never in its own sky (owner, 0.61: "Is there a moon visible from the
        /// moon?"; BUGS.md #142): standing on the moon's ground on face `f` at every hour of a
        /// month's days and phases, the sky draws no moon disc (`space.w`), no moon halo (the
        /// viewer's and the other face's `disc.a`) and no moonlight (both faces' `moon_light`), and
        /// home's sky keeps all three.
        fn the_moon_is_never_in_its_own_sky(f: FaceFrame) {
            let moon = dwell_worldgen::grids::grids()
                .iter()
                .position(|g| g.name == "dwell:moon")
                .expect("the moon") as u8;
            let frame = GridFrame {
                origin: [400_000.0, 300_000.0, -200_000.0],
                rotation: dwell_world::quat_axis_angle([0.6, 0.0, 0.8], 0.7),
                ..GridFrame::HOME
            };
            let (mut home_halo, mut home_light) = (0usize, 0usize);
            for day_n in 0..9 {
                for k in 0..12 {
                    let world_s =
                        (f64::from(day_n) + f64::from(k) / 12.0) * DAY_NIGHT.day_length_s();
                    let day = DAY_NIGHT.state(world_s, None, None);
                    let on = day_on_grid(&day, &frame, moon).at_eye(80.0, 0.0, 12_500.0);
                    let u = on.pack(f, 1.0, 0.0, 0.001);
                    assert_eq!(u[23 * 4 + 3], 0.0, "no moon disc");
                    assert_eq!(u[7 * 4 + 3], 0.0, "no moon halo ({day_n}, {k})");
                    assert_eq!(u[21 * 4 + 3], 0.0, "no moon halo in the other face's sky");
                    assert_eq!(u[8 * 4 + 3], 0.0, "no moon glint on water");
                    for face in [10, 14] {
                        assert_eq!(&u[face * 4..face * 4 + 3], &[0.0; 3], "no moonlight");
                    }
                    let h = day.pack(f, 1.0, 0.0, 0.001);
                    assert_eq!(h[23 * 4 + 3], 1.0, "home draws the moon");
                    home_halo += usize::from(h[7 * 4 + 3] > 0.0);
                    home_light += usize::from(h[10 * 4..10 * 4 + 3].iter().any(|&c| c > 0.0));
                }
            }
            assert!(
                home_halo > 0 && home_light > 0,
                "home keeps the halo and moonlight"
            );
        }
    );
}
