//! The server's clock as a client draws snapshot-interpolated things on it (ARCHITECTURE.md §9.5):
//! remote players (`remotes`) and moving pieces (`dwell-client` `pieces`). A snapshot is timed by
//! the server tick it was taken at, not by when a frame read it; the local-minus-server offset is
//! the least-delayed snapshot's within [`WINDOW_MS`], and the offset drawn with moves toward it
//! continuously at [`SLEW`], so a better estimate never jumps the drawn time. One estimator, so
//! everything drawn from snapshots agrees on the time.

use std::collections::VecDeque;

/// How fast (ms a ms) the drawn clock is brought to a better estimate: 3 % faster or slower, no
/// visible change of speed; one off by more than [`SNAP_MS`] (the first snapshot, a restarted
/// server, a server that dropped time) is taken at once.
pub const SLEW: f64 = 0.03;
pub const SNAP_MS: f64 = 250.0;
/// How long (ms, local) a snapshot's delay counts toward the estimate: the estimate is the least
/// delay seen within it, so it follows a slower link, or a server that dropped time (a local
/// world's worker busy past its catch-up cap, `LocalCore::advance`), within this. It drifted up
/// 0.05 ms a snapshot before (BUGS.md #173, #174): after the server dropped ½ s, pieces and remote
/// players were drawn past their newest snapshots for minutes, moving only on frames one came.
pub const WINDOW_MS: f64 = 1000.0;

#[derive(Clone, Debug, Default)]
pub struct ServerClock {
    /// The offset (local − server, ms) estimated: the smallest within the window.
    target: Option<f64>,
    /// The offsets seen within the window: (local ms, offset).
    seen: VecDeque<(f64, f64)>,
    /// The offset drawn with at local time `anchor_ms`, moving toward `target` from there.
    anchor: f64,
    anchor_ms: f64,
}

impl ServerClock {
    /// Whether a snapshot has been seen.
    pub fn is_set(&self) -> bool {
        self.target.is_some()
    }

    /// The offset (local − server, ms) drawn with at local `now_ms`.
    pub fn offset(&self, now_ms: f64) -> Option<f64> {
        let target = self.target?;
        let most = (now_ms - self.anchor_ms).max(0.0) * SLEW;
        Some(self.anchor + (target - self.anchor).clamp(-most, most))
    }

    /// A snapshot of server time `server_ms` read at local `now_ms`.
    pub fn observe(&mut self, server_ms: f64, now_ms: f64) {
        let seen = now_ms - server_ms;
        let drawn = self.offset(now_ms);
        self.seen.push_back((now_ms, seen));
        while self
            .seen
            .front()
            .is_some_and(|&(at, _)| at < now_ms - WINDOW_MS)
        {
            self.seen.pop_front();
        }
        let target = self.seen.iter().map(|s| s.1).fold(f64::INFINITY, f64::min);
        let anchor = match drawn {
            Some(d) if (target - d).abs() <= SNAP_MS => d,
            _ => target,
        };
        self.target = Some(target);
        self.anchor = anchor;
        self.anchor_ms = now_ms;
    }

    /// The server time (ms) at local `now_ms`, if a snapshot has been seen.
    pub fn server_ms(&self, now_ms: f64) -> Option<f64> {
        Some(now_ms - self.offset(now_ms)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_least_delay_within_the_window_and_slews_to_a_small_change() {
        // face-a-only: a clock, no world.
        let mut c = ServerClock::default();
        assert!(!c.is_set() && c.server_ms(0.0).is_none());
        // Server 0 read at local 1030 (30 ms of delay on a 1000 ms skew).
        c.observe(0.0, 1030.0);
        assert_eq!(c.server_ms(1030.0), Some(0.0));
        assert_eq!(c.server_ms(1080.0), Some(50.0));
        // A later, slower one: ignored while the quicker one is within the window.
        c.observe(50.0, 1100.0);
        assert_eq!(c.server_ms(1100.0), Some(70.0));
        // 10 ms quicker: reached at 3 % (10 ms over 333 ms), not jumped.
        c.observe(100.0, 1120.0);
        let at = |c: &ServerClock, now: f64| c.server_ms(now).unwrap() - (now - 1020.0);
        assert!(at(&c, 1120.0).abs() > 9.9, "not jumped");
        assert!(at(&c, 1500.0).abs() < 1e-9, "reached");
        // Every snapshot 600 ms late from now on (the server dropped time): taken once the
        // quicker ones have left the window, at once (past `SNAP_MS`).
        for k in 0..30 {
            let s = 150.0 + 50.0 * f64::from(k);
            c.observe(s, s + 1000.0 + 620.0);
        }
        let now = 1650.0 + 50.0 * 29.0 + 1000.0;
        assert!((c.server_ms(now).unwrap() - (now - 1620.0)).abs() < 1e-9);
    }
}
