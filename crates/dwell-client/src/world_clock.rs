//! The world's clock as a client sees it, for client-side visuals that clients should roughly agree
//! on (the sky's cloud drift, docs/CLOUDS.md): the server's tick, from its snapshots, carried
//! forward smoothly by the local clock between them. Network delay only ever makes a snapshot look
//! older, so the clock keeps the largest server-minus-local offset seen (the least delayed
//! snapshot's), and never jumps back for a late one. A server whose ticks start over (another
//! world, a restart) resets it.

use dwell_protocol::SIM_HZ;

/// A drop of more than this (s) in the offset is a new clock, not a late snapshot.
const RESET_S: f64 = 5.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldClock {
    /// Server seconds minus local seconds.
    offset_s: Option<f64>,
}

impl WorldClock {
    /// A snapshot of `server_tick` arrived at local time `now_ms`.
    pub fn observe(&mut self, server_tick: u32, now_ms: f64) {
        let offset = f64::from(server_tick) / f64::from(SIM_HZ) - now_ms / 1000.0;
        self.offset_s = Some(match self.offset_s {
            Some(o) if offset > o - RESET_S => o.max(offset),
            _ => offset,
        });
    }

    /// The world's time (s) at local time `now_ms`, once a snapshot has arrived.
    pub fn seconds(&self, now_ms: f64) -> Option<f64> {
        self.offset_s.map(|o| now_ms / 1000.0 + o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_server_tick_and_runs_on_between_snapshots() {
        let mut c = WorldClock::default();
        assert_eq!(c.seconds(1000.0), None);
        // Tick 600 (10 s of world time) arrives at local 2 s.
        c.observe(600, 2000.0);
        assert!((c.seconds(2000.0).unwrap() - 10.0).abs() < 1e-9);
        assert!((c.seconds(2500.0).unwrap() - 10.5).abs() < 1e-9);
    }

    #[test]
    fn a_late_snapshot_never_turns_it_back_and_a_quicker_one_catches_it_up() {
        let mut c = WorldClock::default();
        c.observe(600, 2000.0);
        // Tick 606 (0.1 s later) arrives 0.3 s later: 0.2 s of extra delay, ignored.
        c.observe(606, 2300.0);
        assert!((c.seconds(2300.0).unwrap() - 10.3).abs() < 1e-9);
        // One with less delay than the first moves it on.
        c.observe(630, 2400.0);
        assert!((c.seconds(2400.0).unwrap() - 10.5).abs() < 1e-9);
    }

    #[test]
    fn a_server_starting_over_resets_it() {
        let mut c = WorldClock::default();
        c.observe(60_000, 2000.0);
        c.observe(60, 3000.0);
        assert!((c.seconds(3000.0).unwrap() - 1.0).abs() < 1e-9);
    }
}
