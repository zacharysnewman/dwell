//! The view on another grid (docs/proposals/MOON_WORLD.md §7.3, M-7c): while the player is on the
//! moon the game holds the moon's chunks in its own grid-local coordinates and draws in that frame,
//! so the sky's directions (the sun, the moon, the stars' axis) are turned into it, and the moon's
//! own far disc is not drawn (the viewer stands on it).

use dwell_look::daynight::DayState;
use dwell_world::GridFrame;

/// The day as seen from grid `frame` (its local frame): the sun, the moon's direction and the
/// sky's axis turned into it; the moon's far disc hidden off home (the viewer is on a grid that is
/// not home, the only one with a moon disc). Home's frame returns `day` as it is.
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
        ..*day
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

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
            let world_up = frame.up(f);
            let local_up = f.vector([0.0, 1.0, 0.0]);
            let rise_world = dot(day.sun, world_up);
            let rise_local = dot(on.sun, local_up);
            assert!(
                (rise_world - rise_local).abs() < 1e-9,
                "{rise_world} {rise_local}"
            );
            assert!((dot(on.sun, on.sun) - 1.0).abs() < 1e-9);
            assert!((dot(on.sun, day.sun) - 1.0).abs() > 1e-3, "turned");
            assert_eq!(on.time_of_day, day.time_of_day);
        }
    );
}
