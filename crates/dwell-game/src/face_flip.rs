//! The camera turning over when the player crosses the midplane (BIFACIAL_WORLD.md §6): the heading
//! turns by 180° at once and the view is drawn rotated about its right axis by π, easing out to 0
//! over [`FLIP_SECONDS`], so it starts exactly at the old view and turns over nose first. Pure, so
//! tests pin it; the renderer applies the angle.

use std::f64::consts::PI;

use crate::FaceSign;

/// How long the camera takes to turn over.
pub const FLIP_SECONDS: f64 = 0.5;

fn smoothstep(t: f64) -> f64 {
    let x = t.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[derive(Clone, Debug)]
pub struct FaceFlip {
    face: Option<FaceSign>,
    draw_face: Option<FaceSign>,
    /// Seconds since the drawn face last changed (FLIP_SECONDS or more: at rest).
    elapsed: f64,
}

impl Default for FaceFlip {
    fn default() -> Self {
        Self {
            face: None,
            draw_face: None,
            elapsed: FLIP_SECONDS,
        }
    }
}

impl FaceFlip {
    pub fn new() -> Self {
        Self::default()
    }

    /// Per simulation tick with the predicted face: degrees to add to the heading (0 or 180).
    pub fn tick(&mut self, face: FaceSign) -> f64 {
        let turn = if self.face.is_some_and(|f| f != face) {
            180.0
        } else {
            0.0
        };
        self.face = Some(face);
        turn
    }

    /// Per drawn frame: the camera's pitch-over angle (radians, π … 0) about its right axis.
    pub fn draw(&mut self, face: FaceSign, dt_seconds: f64) -> f64 {
        if self.draw_face.is_some_and(|f| f != face) {
            self.elapsed = 0.0;
        }
        self.draw_face = Some(face);
        self.elapsed = FLIP_SECONDS.min(self.elapsed + dt_seconds);
        PI * (1.0 - smoothstep(self.elapsed / FLIP_SECONDS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_rests_at_0_and_starts_at_a_half_turn_when_the_face_changes() {
        let mut flip = FaceFlip::new();
        assert_eq!(flip.draw(1, 0.016), 0.0);
        assert_eq!(flip.draw(1, 0.016), 0.0);
        // The first frame on the other face: nearly the old orientation (almost π).
        let first = flip.draw(-1, 0.016);
        assert!(first > PI * 0.99);
        // Monotone, reaching 0 after FLIP_SECONDS and staying there.
        let mut last = first;
        let mut t = 0.016;
        while t < FLIP_SECONDS {
            let r = flip.draw(-1, 0.016);
            assert!(r <= last + 1e-12);
            last = r;
            t += 0.016;
        }
        assert_eq!(flip.draw(-1, 0.016), 0.0);
        assert_eq!(flip.draw(-1, 5.0), 0.0);
    }

    #[test]
    fn draw_turns_over_once_per_crossing_either_way_and_midway_is_a_quarter_turn() {
        let mut flip = FaceFlip::new();
        flip.draw(-1, 1.0);
        let half = flip.draw(1, FLIP_SECONDS / 2.0);
        assert!((half - PI / 2.0).abs() < 5e-7);
        assert_eq!(flip.draw(1, FLIP_SECONDS), 0.0);
        assert!(flip.draw(-1, 0.01) > PI * 0.99);
    }

    #[test]
    fn tick_turns_the_heading_by_180_on_each_face_change_and_not_on_the_first_call() {
        let mut flip = FaceFlip::new();
        assert_eq!(flip.tick(-1), 0.0);
        assert_eq!(flip.tick(-1), 0.0);
        assert_eq!(flip.tick(1), 180.0);
        assert_eq!(flip.tick(1), 0.0);
        assert_eq!(flip.tick(-1), 180.0);
    }
}
