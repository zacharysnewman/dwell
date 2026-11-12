//! Staged mesh writes (docs/BUGS.md #72): a chunk mesh's bytes written to its reserved place in
//! the arena's pages a slice a frame, under the frame's upload budget
//! (`shared/render/streaming.json`), and the chunk drawn only once all of it is there (its old
//! mesh until then). A near chunk's mesh is 1.2–2 MB, over a phone's 512 KiB a frame on its own:
//! written at once it was 5–8 ms of one frame on the phone page at CPU 4×.

use std::collections::VecDeque;

/// One write: `data` to `offset` in page `page` of vertex format `format`.
#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    pub format: usize,
    pub page: usize,
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Debug)]
struct Item<K> {
    key: K,
    writes: VecDeque<Write>,
    /// Bytes of the front write already written.
    done: usize,
}

/// Each key's writes, oldest key first.
#[derive(Debug)]
pub struct WriteQueue<K> {
    items: VecDeque<Item<K>>,
}

impl<K> Default for WriteQueue<K> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }
}

impl<K: PartialEq + Copy> WriteQueue<K> {
    /// Queues `key`'s writes (each a whole number of 4-byte words, as the GPU's writes are).
    pub fn push(&mut self, key: K, writes: Vec<Write>) {
        debug_assert!(writes.iter().all(|w| w.data.len() % 4 == 0 && w.offset % 4 == 0));
        self.items.push_back(Item {
            key,
            writes: writes.into(),
            done: 0,
        });
    }

    /// Drops `key`'s writes not yet done (its mesh replaced or removed meanwhile).
    pub fn cancel(&mut self, key: K) {
        self.items.retain(|i| i.key != key);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Bytes waiting.
    pub fn bytes(&self) -> u64 {
        self.items
            .iter()
            .map(|i| i.writes.iter().map(|w| w.data.len()).sum::<usize>() - i.done)
            .sum::<usize>() as u64
    }

    /// This frame's writes, oldest first, at most `budget` bytes in all (at least one 4-byte word,
    /// so the queue never stalls), handed to `write` as (format, page, offset, bytes); returns the
    /// keys whose writes are now all done, in order.
    pub fn pump(
        &mut self,
        budget: u64,
        mut write: impl FnMut(usize, usize, u64, &[u8]),
    ) -> Vec<K> {
        let mut left = (budget.min(usize::MAX as u64) as usize / 4 * 4).max(4);
        let mut finished = Vec::new();
        while left > 0 {
            let Some(item) = self.items.front_mut() else {
                break;
            };
            match item.writes.front() {
                None => {
                    finished.push(item.key);
                    self.items.pop_front();
                }
                Some(w) => {
                    let rest = w.data.len() - item.done;
                    let n = rest.min(left);
                    if n > 0 {
                        write(
                            w.format,
                            w.page,
                            w.offset + item.done as u64,
                            &w.data[item.done..item.done + n],
                        );
                    }
                    left -= n;
                    item.done += n;
                    if item.done == w.data.len() {
                        item.writes.pop_front();
                        item.done = 0;
                    }
                }
            }
        }
        // A key whose last write ended exactly on the budget is done too.
        while let Some(item) = self.items.front() {
            if !item.writes.is_empty() {
                break;
            }
            finished.push(item.key);
            self.items.pop_front();
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(offset: u64, len: usize, byte: u8) -> Write {
        Write {
            format: 0,
            page: 0,
            offset,
            data: vec![byte; len],
        }
    }

    #[test]
    fn a_mesh_over_the_budget_is_written_across_frames_and_shown_once_whole() {
        // face-a-only: bytes, no up in it.
        let mut q = WriteQueue::default();
        // A 1.6 MB chunk (vertices and indices) and a small one after it, at 512 KiB a frame.
        q.push('a', vec![w(0, 1_400_000, 1), w(2_000_000, 200_000, 2)]);
        q.push('b', vec![w(3_000_000, 40_000, 3)]);
        let budget = 512 * 1024;
        let mut page = vec![0u8; 3_100_000];
        let mut frames = Vec::new();
        while !q.is_empty() {
            let mut bytes = 0;
            let done = q.pump(budget, |_, _, off, data| {
                bytes += data.len();
                page[off as usize..off as usize + data.len()].copy_from_slice(data);
            });
            assert!(bytes as u64 <= budget, "a frame wrote {bytes} B");
            frames.push(done);
        }
        // 'a' done only after its last byte: on the fourth frame (1.64 MB at 0.52 MB a frame), with
        // 'b' finishing in the same frame's rest.
        assert_eq!(frames.len(), 4, "{frames:?}");
        assert!(frames[..3].iter().all(Vec::is_empty));
        assert_eq!(frames[3], vec!['a', 'b']);
        assert!(page[..1_400_000].iter().all(|&b| b == 1));
        assert!(page[2_000_000..2_200_000].iter().all(|&b| b == 2));
        assert!(page[3_000_000..3_040_000].iter().all(|&b| b == 3));
    }

    #[test]
    fn a_replaced_mesh_is_never_finished_and_an_unlimited_budget_writes_all_at_once() {
        // face-a-only: bytes, no up in it.
        let mut q = WriteQueue::default();
        q.push(1, vec![w(0, 4096, 1)]);
        q.push(2, vec![w(8192, 4096, 2)]);
        assert!(q.pump(1024, |_, _, _, _| {}).is_empty());
        q.cancel(1);
        assert_eq!(q.bytes(), 4096);
        assert_eq!(q.pump(u64::MAX, |_, _, _, _| {}), vec![2]);
        assert!(q.is_empty());
    }
}
