//! The render clock (docs/DEV_TOOLS.md "Pausing the look"): the one time every drawn animation
//! reads — the water's ripples, caustics and flow (`fog.z`), the plants' sway, the flames, motes and
//! flecks, the lights' flicker, the stars' twinkle, the clouds' drift and the day clock, the eye's
//! adaptation — so a debug pause (`?pause=1`, `__dwell.pause(true)`, Settings → Debug, F7) freezes
//! them all at one instant and the same moment can be drawn from any angle (the menu panoramas,
//! `npm run panoramas`). Only the look stops: the simulation, the controller and the network run on
//! the wall clock as before. Resuming carries on from the instant it stopped, without a jump.

use std::cell::Cell;

/// Render time from wall time: wall time less the time spent paused, or the instant it stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderClock {
    /// The render time it stopped at, while paused (ms).
    paused_at: Option<f64>,
    /// The wall time spent paused so far (ms).
    lag_ms: f64,
}

impl RenderClock {
    /// The render time (ms) at wall time `wall_ms`.
    pub fn at(&self, wall_ms: f64) -> f64 {
        self.paused_at.unwrap_or(wall_ms - self.lag_ms)
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Stops (or starts) render time at wall time `wall_ms`; the same state again changes nothing.
    pub fn set_paused(&mut self, on: bool, wall_ms: f64) {
        match (on, self.paused_at) {
            (true, None) => self.paused_at = Some(wall_ms - self.lag_ms),
            (false, Some(at)) => {
                self.lag_ms = wall_ms - at;
                self.paused_at = None;
            }
            _ => {}
        }
    }
}

thread_local! {
    static CLOCK: Cell<RenderClock> = const { Cell::new(RenderClock { paused_at: None, lag_ms: 0.0 }) };
}

/// The render time (ms) at wall time `wall_ms` (a `telemetry::now_ms` or a frame's `now`).
pub fn at(wall_ms: f64) -> f64 {
    CLOCK.with(|c| c.get().at(wall_ms))
}

/// The render time now (ms).
pub fn now_ms() -> f64 {
    at(crate::telemetry::now_ms())
}

pub fn paused() -> bool {
    CLOCK.with(|c| c.get().paused())
}

/// Pauses or resumes the look now.
pub fn set_paused(on: bool) {
    let wall = crate::telemetry::now_ms();
    CLOCK.with(|c| {
        let mut k = c.get();
        k.set_paused(on, wall);
        c.set(k);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_it_is_the_wall_clock() {
        let c = RenderClock::default();
        assert_eq!(c.at(1234.5), 1234.5);
        assert!(!c.paused());
    }

    #[test]
    fn paused_time_does_not_advance_and_resumes_from_the_same_instant() {
        let mut c = RenderClock::default();
        c.set_paused(true, 1000.0);
        assert!(c.paused());
        assert_eq!(c.at(1000.0), 1000.0);
        assert_eq!(c.at(5000.0), 1000.0, "frozen while paused");
        // Pausing again changes nothing.
        c.set_paused(true, 6000.0);
        assert_eq!(c.at(9000.0), 1000.0);
        c.set_paused(false, 9000.0);
        assert!(!c.paused());
        assert_eq!(c.at(9000.0), 1000.0, "no jump on resuming");
        assert_eq!(
            c.at(9250.0),
            1250.0,
            "then it runs at the wall clock's rate"
        );
        // A second pause and resume adds to the time left out.
        c.set_paused(true, 10_000.0);
        assert_eq!(c.at(12_000.0), 2000.0);
        c.set_paused(false, 12_000.0);
        assert_eq!(c.at(12_500.0), 2500.0);
        // Resuming while running changes nothing.
        c.set_paused(false, 13_000.0);
        assert_eq!(c.at(13_000.0), 3000.0);
    }

    #[test]
    fn the_global_clock_freezes_and_resumes() {
        set_paused(true);
        let a = now_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(now_ms(), a);
        set_paused(false);
        let b = now_ms();
        assert!(
            b >= a && b - a < 5.0,
            "resumes from where it stopped: {a} → {b}"
        );
    }
}
