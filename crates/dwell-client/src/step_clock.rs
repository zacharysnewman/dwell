//! The stepped clock (a cinematic take, docs/THEATER_MODE.md §6.1): while held, the client's clock
//! (`telemetry::now_ms`, and so the frame loop's, the render clock's and every animation's) stands
//! still and moves only when the take sets it, by exactly 1 / fps a captured frame, however long the
//! frame took to render; released, it runs on from where it stood (no jump back), at the wall
//! clock's rate.

use std::cell::Cell;

/// Clock time from wall time: wall time plus an offset, or the held time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepClock {
    held: Option<f64>,
    offset_ms: f64,
}

impl StepClock {
    /// The clock (ms) at wall time `wall_ms`.
    pub fn at(&self, wall_ms: f64) -> f64 {
        self.held.unwrap_or(wall_ms + self.offset_ms)
    }

    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// Holds (or releases) the clock at wall time `wall_ms`; the same state again changes nothing.
    pub fn set_held(&mut self, on: bool, wall_ms: f64) {
        match (on, self.held) {
            (true, None) => self.held = Some(wall_ms + self.offset_ms),
            (false, Some(at)) => {
                self.offset_ms = at - wall_ms;
                self.held = None;
            }
            _ => {}
        }
    }

    /// Sets a held clock to `ms` (the take computes it from the frame count, so it never drifts);
    /// ignored while running.
    pub fn set(&mut self, ms: f64) {
        if let Some(h) = &mut self.held {
            *h = ms;
        }
    }
}

thread_local! {
    static CLOCK: Cell<StepClock> = const { Cell::new(StepClock { held: None, offset_ms: 0.0 }) };
}

/// The clock (ms) at wall time `wall_ms`.
pub fn at(wall_ms: f64) -> f64 {
    CLOCK.with(|c| c.get().at(wall_ms))
}

/// Holds or releases the clock now; returns the clock's time.
pub fn set_held(on: bool, wall_ms: f64) -> f64 {
    CLOCK.with(|c| {
        let mut k = c.get();
        k.set_held(on, wall_ms);
        c.set(k);
        k.at(wall_ms)
    })
}

/// Sets the held clock (ms).
pub fn set(ms: f64) {
    CLOCK.with(|c| {
        let mut k = c.get();
        k.set(ms);
        c.set(k);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_it_is_the_wall_clock() {
        let c = StepClock::default();
        assert_eq!(c.at(1234.5), 1234.5);
        assert!(!c.held());
    }

    #[test]
    fn held_it_moves_only_when_set_and_released_it_runs_on_without_a_jump() {
        let mut c = StepClock::default();
        c.set_held(true, 1000.0);
        assert_eq!(c.at(5000.0), 1000.0);
        // 9000 frames at 30 fps, each set from the frame count: exactly 300 s of clock, whatever the
        // wall clock did meanwhile.
        for i in 1..=9000 {
            c.set(1000.0 + f64::from(i) * 1000.0 / 30.0);
        }
        assert_eq!(c.at(2_000_000.0), 301_000.0);
        c.set_held(false, 2_000_000.0);
        assert_eq!(c.at(2_000_000.0), 301_000.0);
        assert_eq!(c.at(2_000_010.0), 301_010.0);
        // Running, a set does nothing.
        c.set(0.0);
        assert_eq!(c.at(2_000_010.0), 301_010.0);
    }
}
