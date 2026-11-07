//! The streaming budget: how much of a frame's main-thread time the terrain's loading work (the
//! flood light of chunks that arrived, then the mesh job starts) may spend
//! (docs/ARCHITECTURE.md §7.4, docs/DEV_TOOLS.md "Flight loading (M1 Air)").
//!
//! One budget for the frame, not one per kind of work: the loading stops at
//! [`STREAM_DEADLINE_MS`] after the frame's start, whatever ran before it (the finished jobs taken
//! in and uploaded, the ticks), so `frame.cpu` stays near 9 ms with the LOD and the draw after it.
//! A frame that is already late still gets [`StreamBudget::minimum_ms`], so loading never stalls,
//! and a slow page's (CI's ~5 fps) minimum grows with its frame interval. Before 0.50.14 the light
//! and the mesh starts each had their own 4 ms, up to 8 ms a frame plus the last item's overshoot
//! (`mesh_dirty` 6–7.5 ms, up to 14, in flight on the M1 Air).

/// The loading's deadline, ms after the frame's start: `frame.cpu` comes to ~9 ms with the LOD
/// selection and the draw after it (9.0–9.6 ms mean in flight on the M1 Air; 7 ms gave 8.2–8.5).
pub const STREAM_DEADLINE_MS: f64 = 8.0;
/// The least a frame's loading gets (ms), however late the frame is.
pub const STREAM_MIN_MS: f64 = 2.0;
/// ... or this share of the last frame's interval on a slow page, ...
pub const STREAM_FRAME_SHARE: f64 = 0.15;
/// ... up to this (ms): a slow page's frames are long anyway, and a fixed few ms there started
/// one or two mesh jobs a frame (at CI's ~5 fps the view took 20 s to mesh, e2e/play.spec.ts).
pub const STREAM_MIN_MAX_MS: f64 = 50.0;
/// The share of the budget kept for the mesh starts while chunks wait to be meshed: the light
/// stops short of it, so lighting doesn't starve the meshing (0.22.9 started 2–4 mesh jobs a
/// frame at load where 16 had run).
pub const STREAM_MESH_SHARE: f64 = 0.4;

/// One frame's loading budget: `ms` from when the loading starts, of which the light leaves
/// `mesh_ms` to the mesh starts when there are chunks to mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamBudget {
    pub ms: f64,
    pub mesh_ms: f64,
}

impl StreamBudget {
    /// `ms` in all, `mesh_ms` of it kept for the mesh starts.
    pub fn new(ms: f64, mesh_ms: f64) -> Self {
        Self {
            ms,
            mesh_ms: mesh_ms.min(ms),
        }
    }

    /// The least a frame's loading gets after a frame interval of `frame_ms`.
    pub fn minimum_ms(frame_ms: f64) -> f64 {
        (frame_ms * STREAM_FRAME_SHARE).clamp(STREAM_MIN_MS, STREAM_MIN_MAX_MS)
    }

    /// The budget of a frame whose loading starts `elapsed_ms` after the frame's start, after a
    /// frame interval of `frame_ms`: what is left until [`STREAM_DEADLINE_MS`], at least the
    /// minimum.
    pub fn for_frame(elapsed_ms: f64, frame_ms: f64) -> Self {
        let ms = (STREAM_DEADLINE_MS - elapsed_ms).max(Self::minimum_ms(frame_ms));
        Self::new(ms, ms * STREAM_MESH_SHARE)
    }
}

/// A running estimate of one kind of item's cost (ms), to stop before an item that would run past
/// the deadline rather than after it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemCost {
    estimate: f64,
}

impl ItemCost {
    /// Weight of the newest item in the estimate.
    const WEIGHT: f64 = 0.25;

    pub fn estimate(&self) -> f64 {
        self.estimate
    }

    /// One item took `ms`.
    pub fn took(&mut self, ms: f64) {
        let ms = ms.max(0.0);
        self.estimate = if self.estimate == 0.0 {
            ms
        } else {
            self.estimate + (ms - self.estimate) * Self::WEIGHT
        };
    }

    /// Whether another item fits before `deadline` at `now`. The first of a frame always runs
    /// (`ran` = 0): the loading never stalls.
    pub fn fits(&self, ran: usize, now: f64, deadline: f64) -> bool {
        ran == 0 || now + self.estimate <= deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs light then mesh items of the given costs (ms) on a fake clock, as `mesh_dirty` does,
    /// with the frame's loading starting `elapsed` ms in; returns (lit, meshed, the frame's time
    /// at the end).
    fn run(elapsed: f64, light: &[f64], mesh: &[f64]) -> (usize, usize, f64) {
        let b = StreamBudget::for_frame(elapsed, 16.7);
        let mut t = elapsed;
        let deadline = t + b.ms;
        let light_deadline = deadline - b.mesh_ms;
        let (mut lc, mut mc) = (ItemCost::default(), ItemCost::default());
        let mut lit = 0;
        for &c in light {
            if !lc.fits(lit, t, light_deadline) {
                break;
            }
            t += c;
            lc.took(c);
            lit += 1;
        }
        let mut meshed = 0;
        for &c in mesh {
            if !mc.fits(meshed, t, deadline) {
                break;
            }
            t += c;
            mc.took(c);
            meshed += 1;
        }
        (lit, meshed, t)
    }

    #[test]
    fn one_budget_for_the_frame_ends_the_loading_near_the_deadline() {
        // The flight on the M1 Air: the frame's loading starts ~3 ms in, a chunk's light ~1 ms and
        // a mesh start ~0.5 ms, more of both waiting than any frame does. Before 0.50.14 the light
        // and the mesh starts had 4 ms each: the loading ran to ~11 ms and frame.cpu to ~13.
        let (lit, meshed, end) = run(3.0, &[1.0; 40], &[0.5; 40]);
        assert!(end <= STREAM_DEADLINE_MS + 1e-9, "ended at {end} ms");
        assert!(lit >= 2 && meshed >= 2, "lit {lit}, meshed {meshed}");
        // Each kind gets its share: the light leaves the mesh starts theirs.
        assert!(lit <= 3, "lit {lit}");
    }

    #[test]
    fn a_late_frame_still_loads_its_minimum() {
        // Already 12 ms in (a burst of ticks): the minimum, not nothing.
        let b = StreamBudget::for_frame(12.0, 16.7);
        assert_eq!(b.ms, StreamBudget::minimum_ms(16.7));
        let (lit, meshed, _) = run(12.0, &[1.0; 40], &[0.5; 40]);
        assert!(lit >= 1 && meshed >= 1);
        // An item over the whole budget still runs, one a frame.
        let (lit, meshed, _) = run(12.0, &[30.0; 4], &[30.0; 4]);
        assert_eq!((lit, meshed), (1, 1));
    }

    #[test]
    fn a_slow_pages_minimum_grows_with_its_frames() {
        assert_eq!(StreamBudget::minimum_ms(8.3), STREAM_MIN_MS);
        assert!((StreamBudget::minimum_ms(16.7) - 2.505).abs() < 1e-9);
        assert!((StreamBudget::minimum_ms(200.0) - 30.0).abs() < 1e-9);
        assert_eq!(StreamBudget::minimum_ms(1000.0), STREAM_MIN_MAX_MS);
        // At CI's 200 ms frames, 30 ms however late the frame.
        assert!((StreamBudget::for_frame(150.0, 200.0).ms - 30.0).abs() < 1e-9);
    }

    #[test]
    fn the_estimate_stops_before_an_item_that_would_overrun() {
        let mut c = ItemCost::default();
        assert!(c.fits(0, 100.0, 0.0), "the first always runs");
        c.took(2.0);
        assert!(c.fits(1, 4.0, 6.0));
        assert!(!c.fits(1, 4.5, 6.0));
    }
}
