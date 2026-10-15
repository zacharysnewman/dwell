//! The client's estimate of world time (docs/DAY_NIGHT.md): the server's tick, which is the world's
//! time (saved in the world file, resumed on load), arrives in every snapshot; between snapshots
//! the clock runs on at real time. Small differences (jitter, a late datagram) are taken in a
//! little at a time so the sky never jumps; a large one (a stall, a resumed tab) snaps.

use dwell_protocol::SIM_HZ;

/// Differences beyond this (seconds) snap instead of being eased in.
pub const SNAP_S: f64 = 2.0;
/// The share of a difference taken in per snapshot.
pub const EASE: f64 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldClock {
    /// World seconds at `anchor_ms` on the page's clock; none before the first snapshot.
    anchor: Option<(f64, f64)>,
}

impl WorldClock {
    /// A snapshot stamped `server_tick` arrived at `now_ms`.
    pub fn observe(&mut self, server_tick: u32, now_ms: f64) {
        let server_s = f64::from(server_tick) / f64::from(SIM_HZ);
        self.anchor = Some(match self.seconds(now_ms) {
            Some(estimate) if (server_s - estimate).abs() <= SNAP_S => {
                (estimate + (server_s - estimate) * EASE, now_ms)
            }
            _ => (server_s, now_ms),
        });
    }

    /// World seconds at `now_ms`, or none before the first snapshot.
    pub fn seconds(&self, now_ms: f64) -> Option<f64> {
        self.anchor
            .map(|(s, at)| s + (now_ms - at).max(0.0) / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_servers_tick_and_runs_on_between_snapshots() {
        let mut c = WorldClock::default();
        assert_eq!(c.seconds(0.0), None);
        c.observe(600, 1000.0); // tick 600 = 10 s
        assert_eq!(c.seconds(1000.0), Some(10.0));
        assert_eq!(c.seconds(1500.0), Some(10.5));
        // Snapshots on time keep it exact.
        c.observe(630, 1500.0);
        assert!((c.seconds(1500.0).unwrap() - 10.5).abs() < 1e-12);
    }

    #[test]
    fn eases_in_jitter_without_a_jump_and_snaps_after_a_stall() {
        let mut c = WorldClock::default();
        c.observe(600, 0.0);
        // A snapshot 60 ms late: the clock moves back 6 ms, not 60.
        let estimate = c.seconds(1060.0).unwrap();
        assert!((estimate - 11.06).abs() < 1e-9);
        c.observe(660, 1060.0);
        assert!((c.seconds(1060.0).unwrap() - 11.054).abs() < 1e-9);
        // Steady snapshots converge on the server's time.
        for k in 1..60 {
            c.observe(660 + 3 * k, 1060.0 + 50.0 * f64::from(k));
        }
        let last = 1060.0 + 50.0 * 59.0;
        assert!((c.seconds(last).unwrap() - f64::from(660 + 3 * 59) / 60.0).abs() < 1e-3);
        // A 10 s stall (a hidden tab): snaps to the server.
        c.observe(60 * 100, 2_000_000.0);
        assert_eq!(c.seconds(2_000_000.0), Some(100.0));
    }

    #[test]
    fn a_resumed_world_resumes_its_time() {
        // A world saved at tick 1 000 000 resumes there (the server loads `world_tick`).
        let mut c = WorldClock::default();
        c.observe(1_000_000, 5.0);
        assert!((c.seconds(5.0).unwrap() - 1_000_000.0 / 60.0).abs() < 1e-9);
    }
}
