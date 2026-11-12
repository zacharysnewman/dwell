//! The upload budget: how many bytes of finished chunk meshes a frame hands to the GPU
//! (docs/ARCHITECTURE.md §7.4, docs/BUGS.md #72; the budgets are data,
//! `shared/render/streaming.json`).
//!
//! Finished meshes arrive from the workers in bursts (2–8 in a frame while moving), and taking
//! them all in one frame interleaved and wrote up to 3.6 MB at once: `jobs.apply` reached 15 ms on
//! the phone page with the CPU throttled 4×, a dropped frame on its own. The queue keeps them in
//! arrival order and lets a frame take only what fits its byte budget (the first always, so a
//! mesh larger than the budget still goes, one a frame); the rest wait for the next frame.

use std::collections::VecDeque;

/// Finished meshes waiting for their upload, oldest first, with their sizes in bytes.
#[derive(Debug)]
pub struct UploadQueue<T> {
    queue: VecDeque<(T, u64)>,
    bytes: u64,
}

impl<T> Default for UploadQueue<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            bytes: 0,
        }
    }
}

impl<T> UploadQueue<T> {
    /// Queues `item` of `bytes` behind the ones already waiting.
    pub fn push(&mut self, item: T, bytes: u64) {
        self.bytes += bytes;
        self.queue.push_back((item, bytes));
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Bytes waiting.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// This frame's items: in arrival order while their bytes fit in `budget` (the first always,
    /// so the queue never stalls).
    pub fn take(&mut self, budget: u64) -> Vec<T> {
        let mut out = Vec::new();
        let mut used = 0u64;
        while let Some((_, b)) = self.queue.front() {
            if !out.is_empty() && used.saturating_add(*b) > budget {
                break;
            }
            let (item, b) = self.queue.pop_front().expect("peeked");
            used += b;
            self.bytes -= b;
            out.push(item);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frame_uploads_more_than_its_budget_and_the_queue_drains_in_order() {
        // A burst like the phone page's worst while flying: 2.6 MB of meshes in one frame.
        let sizes = [400_000u64, 900_000, 120_000, 300_000, 700_000, 50_000, 160_000];
        let mut q = UploadQueue::default();
        for (i, &b) in sizes.iter().enumerate() {
            q.push(i, b);
        }
        let budget = 1_000_000;
        let mut order = Vec::new();
        let mut frames = 0;
        while !q.is_empty() {
            let took = q.take(budget);
            assert!(!took.is_empty(), "a frame always takes one");
            let bytes: u64 = took.iter().map(|&i| sizes[i]).sum();
            assert!(bytes <= budget, "frame {frames} uploaded {bytes} B");
            order.extend(took);
            frames += 1;
        }
        assert_eq!(order, (0..sizes.len()).collect::<Vec<_>>(), "arrival order");
        assert_eq!(q.bytes(), 0);
        // 2.63 MB at 1 MB a frame: three or four frames, not one.
        assert!((3..=4).contains(&frames), "{frames} frames");
    }

    #[test]
    fn a_mesh_over_the_budget_still_goes_alone() {
        let mut q = UploadQueue::default();
        q.push('a', 5_000_000);
        q.push('b', 10);
        assert_eq!(q.take(1_000), vec!['a']);
        assert_eq!(q.take(1_000), vec!['b']);
        assert!(q.take(1_000).is_empty());
    }

    #[test]
    fn an_unlimited_budget_takes_everything() {
        let mut q = UploadQueue::default();
        for i in 0..10 {
            q.push(i, 1 << 20);
        }
        assert_eq!(q.take(u64::MAX).len(), 10);
    }
}
