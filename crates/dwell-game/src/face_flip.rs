//! The camera across the midplane (BIFACIAL_WORLD.md §6): the body turns over, the camera keeps
//! pointing the same way. The look continues on the other face ([`FaceFrame::look_across`]: the
//! heading turned by 180°, the pitch mirrored), which in the world keeps the view's vertical part
//! and its right, so the new view differs from the old only by a turn about the camera's right
//! axis of 2 × (90° − |pitch|): none looking straight down or up, a 20° nod at 80°, half a turn
//! looking level. The view is drawn turned back by that angle, easing out to 0 over
//! [`FLIP_SECONDS`], so it starts exactly at the old view and never rolls. Pure, so tests pin it;
//! the renderer applies the angle (`render/camera.rs` `aim`).

use std::f64::consts::PI;

use dwell_world::FaceFrame;

/// How long the camera takes to settle after a crossing.
pub const FLIP_SECONDS: f64 = 0.5;

/// The ease the crossing's camera turn and the eye's swing share: 0 at the crossing, 1 at rest.
pub fn ease(elapsed_seconds: f64) -> f64 {
    let x = (elapsed_seconds / FLIP_SECONDS).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The turn (radians, about the camera's right axis, as `aim`'s `flip`) that takes the view of the
/// look continued across the midplane back to the view of a look at face-local `pitch_deg` before
/// it: π + 2p looking down or level, −π + 2p looking up (each the shorter way round, through
/// straight down or up), 0 at ±90°.
pub fn residual(pitch_deg: f64) -> f64 {
    let p = pitch_deg.to_radians();
    if p <= 0.0 {
        PI + 2.0 * p
    } else {
        -PI + 2.0 * p
    }
}

/// An angle wrapped into (−π, π].
fn wrap(a: f64) -> f64 {
    let w = a.rem_euclid(2.0 * PI);
    if w > PI { w - 2.0 * PI } else { w }
}

#[derive(Clone, Debug)]
pub struct FaceFlip {
    face: Option<FaceFrame>,
    /// The turn drawn at the last crossing (radians).
    start: f64,
    /// Seconds drawn since the last crossing (FLIP_SECONDS or more: at rest).
    elapsed: f64,
}

impl Default for FaceFlip {
    fn default() -> Self {
        Self {
            face: None,
            start: 0.0,
            elapsed: FLIP_SECONDS,
        }
    }
}

impl FaceFlip {
    pub fn new() -> Self {
        Self::default()
    }

    /// The player placed on `face` (joined, respawned): no turn, the view as it is.
    pub fn reset(&mut self, face: FaceFrame) {
        *self = Self {
            face: Some(face),
            ..Self::default()
        };
    }

    /// Per simulation tick with the predicted face and the look (face-local degrees): the look to
    /// go on with, continued across the midplane when the face changed. The drawn turn then starts
    /// from the view drawn now, so a body swinging back across mid-turn turns on from where it is.
    pub fn tick(&mut self, face: FaceFrame, yaw: f64, pitch: f64) -> (f64, f64) {
        let crossed = self.face.is_some_and(|f| f != face);
        self.face = Some(face);
        if !crossed {
            return (yaw, pitch);
        }
        self.start = wrap(residual(pitch) + self.angle());
        self.elapsed = 0.0;
        FaceFrame::look_across(yaw, pitch)
    }

    /// The turn drawn now (radians about the camera's right axis; 0 at rest).
    pub fn angle(&self) -> f64 {
        self.start * (1.0 - ease(self.elapsed))
    }

    /// Per drawn frame, `dt_seconds` after the last: the camera's turn about its right axis.
    pub fn draw(&mut self, dt_seconds: f64) -> f64 {
        self.elapsed = FLIP_SECONDS.min(self.elapsed + dt_seconds);
        self.angle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: FaceFrame = FaceFrame::A;
    const B: FaceFrame = FaceFrame::B;

    #[test]
    fn the_turn_is_twice_the_pitch_from_vertical_the_shorter_way() {
        for (pitch, turn) in [(-90.0, 0.0), (-80.0, 20.0), (0.0, 180.0), (80.0, -20.0)] {
            let r = residual(pitch).to_degrees();
            assert!((r - turn).abs() < 1e-9, "{pitch}: {r}");
        }
    }

    dwell_world::both_faces!(
        /// Crossing from face `f`: the look goes on across, and the turn eases from the residual
        /// to 0 over FLIP_SECONDS, monotonically.
        fn a_crossing_continues_the_look_and_eases_the_turn_out(f: FaceFrame) {
            let mut flip = FaceFlip::new();
            assert_eq!(flip.tick(f, 37.0, -80.0), (37.0, -80.0));
            assert_eq!(flip.draw(0.016), 0.0);
            let (yaw, pitch) = flip.tick(f.other(), 37.0, -80.0);
            assert_eq!((yaw, pitch), (217.0, 80.0));
            let first = flip.draw(0.0);
            assert!((first - 20f64.to_radians()).abs() < 1e-12, "{first}");
            let mut last = first;
            let mut t = 0.0;
            while t < FLIP_SECONDS {
                let r = flip.draw(0.016);
                assert!(r <= last + 1e-12 && r >= 0.0);
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
    fn a_swing_back_mid_turn_turns_on_from_the_drawn_view() {
        let mut flip = FaceFlip::new();
        flip.tick(A, 0.0, 0.0);
        let (yaw, pitch) = flip.tick(B, 0.0, 0.0);
        flip.draw(FLIP_SECONDS / 2.0); // halfway: a quarter turn left
        assert!((flip.angle() - PI / 2.0).abs() < 1e-9);
        // Back across: the look is the one before both crossings, and the drawn turn goes on from
        // the quarter turn (π + π/2 ≡ −π/2): the same view, with no jump.
        assert_eq!(flip.tick(A, yaw, pitch), (0.0, 0.0));
        assert!((flip.draw(0.0) + PI / 2.0).abs() < 1e-9);
    }

    // both-faces: placed on face A after face B.
    #[test]
    fn being_placed_does_not_turn() {
        let mut flip = FaceFlip::new();
        flip.tick(B, 10.0, -30.0);
        flip.reset(A);
        assert_eq!(flip.draw(0.0), 0.0);
        assert_eq!(flip.tick(A, 10.0, -30.0), (10.0, -30.0));
    }
}
