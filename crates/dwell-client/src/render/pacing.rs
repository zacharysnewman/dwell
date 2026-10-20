//! Frame pacing (docs/WATER_RENDERING.md §8, "Stalls"): at most `MAX_IN_FLIGHT` frames submitted
//! and not yet done on the GPU. When the GPU falls behind (a heavy frame, or another process
//! sharing it), a frame that would queue more work is skipped instead: in a browser the commands
//! of a frame recorded while the GPU process is backed up block the main thread until it catches
//! up (seen as 60–340 ms long tasks inside `draw.raster`), which stops input, the network and the
//! simulation with it. Skipping keeps them running; the canvas shows the last frame meanwhile.
//!
//! Completion is counted by `Queue::on_submitted_work_done`. If it stops answering (a lost device),
//! the pacer gives up waiting after `GIVE_UP_MS` and draws, forgetting what was in flight.
//! (The counts are per frame, so they stay far from overflowing: 2³² frames is two years at 60 Hz.)

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Frames allowed in flight on the GPU.
pub const MAX_IN_FLIGHT: u32 = 2;
/// How long a frame waits for the GPU before drawing anyway (ms).
pub const GIVE_UP_MS: f64 = 1000.0;

#[derive(Debug, Default)]
pub struct FramePacer {
    submitted: u32,
    done: Arc<AtomicU32>,
    waiting_since: Option<f64>,
}

impl FramePacer {
    /// Frames submitted and not yet done.
    pub fn in_flight(&self) -> u32 {
        self.submitted
            .saturating_sub(self.done.load(Ordering::Acquire))
    }

    /// Whether a frame may be drawn at `now_ms`.
    pub fn ready(&mut self, now_ms: f64) -> bool {
        if self.in_flight() < MAX_IN_FLIGHT {
            self.waiting_since = None;
            return true;
        }
        let since = *self.waiting_since.get_or_insert(now_ms);
        if now_ms - since >= GIVE_UP_MS {
            // The GPU stopped answering: forget what was in flight (a new count, so late answers
            // for those frames go to the old one).
            self.waiting_since = None;
            self.submitted = 0;
            self.done = Arc::new(AtomicU32::new(0));
            return true;
        }
        false
    }

    /// A frame was submitted; returns the callback that marks it done (for
    /// `Queue::on_submitted_work_done`).
    pub fn submitted(&mut self) -> impl FnOnce() + Send + 'static {
        self.submitted += 1;
        let done = self.done.clone();
        move || {
            done.fetch_add(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_waits_while_two_are_in_flight_and_draws_once_one_is_done() {
        let mut p = FramePacer::default();
        assert!(p.ready(0.0));
        let a = p.submitted();
        assert!(p.ready(16.0), "one in flight");
        let b = p.submitted();
        assert_eq!(p.in_flight(), 2);
        assert!(!p.ready(32.0), "two in flight: skip, don't queue a third");
        assert!(!p.ready(48.0));
        a();
        assert!(p.ready(64.0));
        b();
        assert_eq!(p.in_flight(), 0);
    }

    #[test]
    fn a_gpu_that_stops_answering_is_given_up_on() {
        let mut p = FramePacer::default();
        let a = p.submitted();
        let b = p.submitted();
        assert!(!p.ready(100.0));
        assert!(!p.ready(100.0 + GIVE_UP_MS - 1.0));
        assert!(p.ready(100.0 + GIVE_UP_MS), "draws anyway");
        assert_eq!(p.in_flight(), 0);
        // The late answers don't wedge it.
        a();
        b();
        assert_eq!(p.in_flight(), 0);
        let c = p.submitted();
        let d = p.submitted();
        assert!(!p.ready(2000.0), "paced again");
        c();
        d();
        assert!(p.ready(2016.0));
    }
}
