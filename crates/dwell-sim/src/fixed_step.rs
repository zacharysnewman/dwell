//! Fixed-timestep accumulator (ARCHITECTURE.md §4.2). The simulation always advances in whole steps
//! of `1 / hz`; real time that doesn't fill a step carries over to the next call.

pub struct FixedStep {
    dt: f64,
    max_steps: u32,
    accumulator: f64,
}

impl FixedStep {
    pub fn new(hz: u32) -> Self {
        Self::with_cap(hz, 8)
    }
    pub fn with_cap(hz: u32, max_steps_per_advance: u32) -> Self {
        Self {
            dt: 1.0 / f64::from(hz),
            max_steps: max_steps_per_advance,
            accumulator: 0.0,
        }
    }

    /// Adds `elapsed_seconds` of real time and returns how many steps to run now. If the host fell
    /// far behind (e.g. a stall), at most `max_steps_per_advance` are returned and the rest of the
    /// backlog is dropped, so a slow frame can't snowball.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        self.accumulator += elapsed_seconds.max(0.0);
        let steps = (self.accumulator / self.dt) as u32;
        if steps > self.max_steps {
            self.accumulator = 0.0;
            return self.max_steps;
        }
        self.accumulator -= f64::from(steps) * self.dt;
        steps
    }

    pub fn dt(&self) -> f64 {
        self.dt
    }
    /// Seconds until the next step is due.
    pub fn time_until_next_step(&self) -> f64 {
        (self.dt - self.accumulator).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_halves_make_one_step_as_in_the_cpp_suite() {
        let mut step = FixedStep::with_cap(60, 5);
        assert_eq!(step.advance(1.0 / 120.0), 0);
        assert_eq!(step.advance(1.0 / 120.0), 1); // two halves make one step
        assert_eq!(step.advance(3.0 / 60.0), 3);
        assert_eq!(step.advance(10.0), 5); // capped, backlog dropped
        assert_eq!(step.advance(0.0), 0);
    }

    #[test]
    fn carries_remainders_and_caps_backlogs() {
        let mut f = FixedStep::new(60);
        assert_eq!(f.advance(0.010), 0);
        assert_eq!(f.advance(0.010), 1); // 20 ms: one step, 3.3 ms carried
        assert_eq!(f.advance(1.0 / 60.0), 1);
        assert_eq!(f.advance(10.0), 8); // capped; the backlog is dropped
        assert_eq!(f.advance(0.0), 0);
        assert!(f.time_until_next_step() > 0.016);
    }
}
