//! The camera across the midplane (BIFACIAL_WORLD.md §6): the body turns over and the view keeps
//! pointing exactly the same way. The look continues on the other face
//! ([`FaceFrame::look_across`]: the heading kept, the pitch mirrored), whose world direction is the
//! same at every heading and pitch; only the camera's up differs, by half a turn about the forward
//! axis. The view is drawn rolled back by that half turn, easing in and out to 0 over
//! [`ROLL_SECONDS`] (data: `shared/look/motion.json` `crossingRollS`), so it starts exactly at the
//! old view and turns upright about its own forward axis, which never moves. Pure, so tests pin it;
//! the renderer applies the roll (`render/camera.rs` `aim`).
//!
//! The crossing's rule is this module's `tick` (what the look becomes) and `residual` (the view
//! turn that takes the new look's view back to the old one); `aim` draws any roll.
//!
//! An experiment switch (owner, 2026-10-21) sets the roll's duration per page: `?crossingroll=<s>`
//! ([`roll_seconds_from_query`]) or `__dwell.setCrossingRoll(s)`; 0 (`?crossingroll=snap`) snaps
//! upright on the crossing frame, with no frames in between.

use std::f64::consts::PI;

use dwell_world::FaceFrame;

/// How long the camera takes to roll upright after a crossing (s).
pub const ROLL_SECONDS: f64 = dwell_look::motion::MOTION.crossing_roll_s;

/// The ease the crossing's roll and the eye's swing share, gently in and out: 0 at the crossing,
/// 1 at rest ([`ROLL_SECONDS`] later).
pub fn ease(elapsed_seconds: f64) -> f64 {
    ease_over(elapsed_seconds, ROLL_SECONDS)
}

/// [`ease`] over `seconds` instead of [`ROLL_SECONDS`]; 0 seconds (a snap) is at rest at once.
pub fn ease_over(elapsed_seconds: f64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 1.0;
    }
    let x = (elapsed_seconds / seconds).clamp(0.0, 1.0);
    0.5 - 0.5 * (PI * x).cos()
}

/// The longest roll the experiment switch takes (s), as `motion.json`'s validation allows.
pub const MAX_ROLL_SECONDS: f64 = 5.0;

/// A roll duration asked for (`?crossingroll=`, `setCrossingRoll`): seconds in
/// 0..=[`MAX_ROLL_SECONDS`], 0 a snap; anything else is refused.
pub fn valid_roll_seconds(seconds: f64) -> Option<f64> {
    (seconds.is_finite() && (0.0..=MAX_ROLL_SECONDS).contains(&seconds)).then_some(seconds)
}

/// The roll duration from the page's query: `?crossingroll=<seconds>`, or `?crossingroll=snap`
/// for 0; none without it or for a value [`valid_roll_seconds`] refuses.
pub fn roll_seconds_from_query(search: &str) -> Option<f64> {
    let (_, v) = search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "crossingroll")?;
    if v.eq_ignore_ascii_case("snap") {
        return Some(0.0);
    }
    valid_roll_seconds(v.parse().ok()?)
}

/// The roll (radians about the camera's forward axis, as `aim`'s `roll`) that takes the view of
/// the look continued across the midplane back to the view before it: half a turn, whatever the
/// look (the forward is the same, the up reversed).
pub fn residual() -> f64 {
    PI
}

/// An angle wrapped into (−π, π].
fn wrap(a: f64) -> f64 {
    let w = a.rem_euclid(2.0 * PI);
    if w > PI { w - 2.0 * PI } else { w }
}

#[derive(Clone, Debug)]
pub struct FaceFlip {
    face: Option<FaceFrame>,
    /// The roll drawn at the last crossing (radians).
    start: f64,
    /// Seconds drawn since the last crossing (`seconds` or more: at rest).
    elapsed: f64,
    /// How long the roll takes (s; [`ROLL_SECONDS`] unless the experiment switch set it; 0 snaps).
    seconds: f64,
}

impl Default for FaceFlip {
    fn default() -> Self {
        Self {
            face: None,
            start: 0.0,
            elapsed: ROLL_SECONDS,
            seconds: ROLL_SECONDS,
        }
    }
}

impl FaceFlip {
    pub fn new() -> Self {
        Self::default()
    }

    /// The player placed on `face` (joined, respawned): no roll, the view as it is.
    pub fn reset(&mut self, face: FaceFrame) {
        *self = Self {
            face: Some(face),
            elapsed: self.seconds,
            seconds: self.seconds,
            ..Self::default()
        };
    }

    /// How long the roll takes from the next crossing (s, [`valid_roll_seconds`]; 0 snaps). False
    /// (and unchanged) for a value it refuses.
    pub fn set_seconds(&mut self, seconds: f64) -> bool {
        let Some(s) = valid_roll_seconds(seconds) else {
            return false;
        };
        self.seconds = s;
        self.elapsed = self.elapsed.min(s);
        true
    }

    /// How long the roll takes (s).
    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    /// Per simulation tick with the predicted face and the look (face-local degrees): the look to
    /// go on with, continued across the midplane when the face changed. The drawn roll then starts
    /// from the view drawn now, so a body swinging back across mid-roll rolls on from where it is.
    pub fn tick(&mut self, face: FaceFrame, yaw: f64, pitch: f64) -> (f64, f64) {
        let crossed = self.face.is_some_and(|f| f != face);
        self.face = Some(face);
        if !crossed {
            return (yaw, pitch);
        }
        self.start = wrap(residual() + self.angle());
        self.elapsed = 0.0;
        FaceFrame::look_across(yaw, pitch)
    }

    /// The roll drawn now (radians about the camera's forward axis; 0 at rest).
    pub fn angle(&self) -> f64 {
        self.start * (1.0 - ease_over(self.elapsed, self.seconds))
    }

    /// Per drawn frame, `dt_seconds` after the last: the camera's roll about its forward axis.
    pub fn draw(&mut self, dt_seconds: f64) -> f64 {
        self.elapsed = self.seconds.min(self.elapsed + dt_seconds);
        self.angle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: FaceFrame = FaceFrame::A;
    const B: FaceFrame = FaceFrame::B;

    /// The owner (2026-10-21): "perhaps slow down the transition a little" — 0.8 s, from the data.
    #[test]
    fn the_roll_takes_0_8_seconds_from_the_data_eased_in_and_out() {
        assert_eq!(ROLL_SECONDS, 0.8);
        let json = include_str!("../../../shared/look/motion.json");
        assert!(json.contains("\"crossingRollS\": 0.8"));
        // Gentle at both ends, half way at half time.
        assert!(ease(0.0) == 0.0 && ease(ROLL_SECONDS) == 1.0);
        assert!((ease(ROLL_SECONDS / 2.0) - 0.5).abs() < 1e-12);
        let dt = 0.001;
        assert!(ease(dt) / dt < 0.01 && (1.0 - ease(ROLL_SECONDS - dt)) / dt < 0.01);
    }

    dwell_world::both_faces!(
        /// Crossing from face `f`: the heading is kept, the pitch mirrored, and the roll eases from
        /// half a turn to 0 over ROLL_SECONDS, monotonically.
        fn a_crossing_continues_the_look_and_eases_the_roll_out(f: FaceFrame) {
            let mut flip = FaceFlip::new();
            assert_eq!(flip.tick(f, 37.0, -80.0), (37.0, -80.0));
            assert_eq!(flip.draw(0.016), 0.0);
            let (yaw, pitch) = flip.tick(f.other(), 37.0, -80.0);
            assert_eq!((yaw, pitch), (37.0, 80.0));
            let first = flip.draw(0.0);
            assert!((first - PI).abs() < 1e-12, "{first}");
            let mut last = first;
            let mut t = 0.0;
            while t + 0.016 < ROLL_SECONDS {
                let r = flip.draw(0.016);
                assert!(r < last && r > 0.0);
                last = r;
                t += 0.016;
            }
            assert_eq!(flip.draw(0.016), 0.0);
            // On the same face nothing changes.
            assert_eq!(flip.tick(f.other(), yaw, pitch), (yaw, pitch));
            assert_eq!(flip.draw(5.0), 0.0);
        }
    );

    // both-faces: crosses A → B → A itself.
    #[test]
    fn a_swing_back_mid_roll_rolls_on_from_the_drawn_view() {
        let mut flip = FaceFlip::new();
        flip.tick(A, 0.0, 10.0);
        let (yaw, pitch) = flip.tick(B, 0.0, 10.0);
        flip.draw(ROLL_SECONDS / 2.0); // halfway: a quarter turn left
        assert!((flip.angle() - PI / 2.0).abs() < 1e-9);
        // Back across: the look before both crossings, and the roll goes on from the quarter turn
        // (π + π/2 ≡ −π/2): the same view, with no jump.
        assert_eq!(flip.tick(A, yaw, pitch), (0.0, 10.0));
        assert!((flip.draw(0.0) + PI / 2.0).abs() < 1e-9);
    }

    dwell_world::both_faces!(
        /// The owner's experiment (2026-10-21): "an instant snap roll rotation on the mid plane
        /// transition". With 0 s the first frame drawn after crossing from `f` is already upright
        /// (no roll at all), and so is every frame after.
        fn a_zero_second_roll_snaps_upright_on_the_crossing_frame(f: FaceFrame) {
            let mut flip = FaceFlip::new();
            assert!(flip.set_seconds(0.0));
            flip.reset(f);
            flip.tick(f, 37.0, -80.0);
            assert_eq!(flip.draw(0.016), 0.0);
            assert_eq!(flip.tick(f.other(), 37.0, -80.0), (37.0, 80.0));
            assert_eq!(flip.angle(), 0.0);
            assert_eq!(flip.draw(0.0), 0.0);
            assert_eq!(flip.draw(0.016), 0.0);
            // And back across at once.
            assert_eq!(flip.tick(f, 37.0, 80.0), (37.0, -80.0));
            assert_eq!(flip.draw(0.0), 0.0);
        }
    );

    #[test]
    fn the_crossing_roll_is_read_from_the_query() {
        assert_eq!(roll_seconds_from_query(""), None);
        assert_eq!(roll_seconds_from_query("?local=1"), None);
        assert_eq!(roll_seconds_from_query("?crossingroll=0"), Some(0.0));
        assert_eq!(roll_seconds_from_query("?crossingroll=snap"), Some(0.0));
        assert_eq!(
            roll_seconds_from_query("?local=1&crossingroll=SNAP"),
            Some(0.0)
        );
        assert_eq!(
            roll_seconds_from_query("?crossingroll=0.3&world=flat"),
            Some(0.3)
        );
        assert_eq!(roll_seconds_from_query("crossingroll=2"), Some(2.0));
        for bad in ["-1", "6", "NaN", "inf", "", "fast"] {
            assert_eq!(
                roll_seconds_from_query(&format!("?crossingroll={bad}")),
                None,
                "{bad}"
            );
        }
        let mut flip = FaceFlip::new();
        assert!(!flip.set_seconds(f64::NAN) && !flip.set_seconds(-0.1));
        assert_eq!(flip.seconds(), ROLL_SECONDS);
        // The setting outlives a respawn.
        assert!(flip.set_seconds(0.25));
        flip.reset(A);
        assert_eq!(flip.seconds(), 0.25);
    }

    // both-faces: placed on face A after face B.
    #[test]
    fn being_placed_does_not_roll() {
        let mut flip = FaceFlip::new();
        flip.tick(B, 10.0, -30.0);
        flip.reset(A);
        assert_eq!(flip.draw(0.0), 0.0);
        assert_eq!(flip.tick(A, 10.0, -30.0), (10.0, -30.0));
    }
}
