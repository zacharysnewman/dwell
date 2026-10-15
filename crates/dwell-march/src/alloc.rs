//! The brick pool's allocator (RAY_MARCHED_RENDERER.md §4.1): one growable `u32` buffer, carved
//! into fixed-size slots per **size class**, each class with its own free-slot stack — so
//! allocating and freeing are O(1). Level-0 bricks are one class (raw, or raw with a shape plane);
//! LOD palette bricks are a class per index width. A freed slot is reused only by its class; new
//! slots are appended at the end, so the GPU buffer only grows (by `copy_buffer_to_buffer`) and a
//! slot's offset never moves. Pure: tested without a GPU.

/// A pool of `u32` words with per-class free stacks.
#[derive(Clone, Debug, Default)]
pub struct BrickPool {
    words: Vec<u32>,
    /// Free slot offsets per class size (words), most recently freed last.
    free: Vec<(usize, Vec<u32>)>,
    live_words: usize,
}

impl BrickPool {
    pub fn new() -> Self {
        Self::default()
    }

    fn stack(&mut self, size: usize) -> &mut Vec<u32> {
        let i = match self.free.iter().position(|(s, _)| *s == size) {
            Some(i) => i,
            None => {
                self.free.push((size, Vec::new()));
                self.free.len() - 1
            }
        };
        &mut self.free[i].1
    }

    /// A slot of `size` words (a class size), its offset; the slot's previous content is kept
    /// until written.
    pub fn alloc(&mut self, size: usize) -> u32 {
        assert!(size > 0);
        self.live_words += size;
        if let Some(at) = self.stack(size).pop() {
            return at;
        }
        let at = self.words.len();
        assert!(at + size <= u32::MAX as usize, "pool past u32 offsets");
        self.words.resize(at + size, 0);
        at as u32
    }

    /// Returns a slot (offset and the size it was allocated with) to its class.
    pub fn free(&mut self, at: u32, size: usize) {
        debug_assert!(at as usize + size <= self.words.len());
        self.live_words -= size;
        self.stack(size).push(at);
    }

    /// Allocates and writes a brick.
    pub fn insert(&mut self, brick: &[u32]) -> u32 {
        let at = self.alloc(brick.len());
        self.words[at as usize..at as usize + brick.len()].copy_from_slice(brick);
        at
    }

    pub fn words(&self) -> &[u32] {
        &self.words
    }
    pub fn slice(&self, at: u32, len: usize) -> &[u32] {
        &self.words[at as usize..at as usize + len]
    }
    /// The words from `at` to the end (a brick decodes its own size from its header).
    pub fn from(&self, at: u32) -> &[u32] {
        &self.words[at as usize..]
    }
    /// Words in live slots.
    pub fn live_words(&self) -> usize {
        self.live_words
    }
    /// The buffer's size in words (live and free slots).
    pub fn capacity_words(&self) -> usize {
        self.words.len()
    }
}

/// O(1) index allocation for fixed-size records (nodes, column grids): a free stack over
/// `0..high_water`.
#[derive(Clone, Debug, Default)]
pub struct IndexPool {
    free: Vec<u32>,
    next: u32,
}

impl IndexPool {
    pub fn alloc(&mut self) -> u32 {
        self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        })
    }
    pub fn free(&mut self, i: u32) {
        debug_assert!(i < self.next);
        self.free.push(i);
    }
    pub fn high_water(&self) -> u32 {
        self.next
    }
    pub fn live(&self) -> usize {
        self.next as usize - self.free.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_freed_slots_of_the_same_class_only() {
        let mut p = BrickPool::new();
        let a = p.alloc(273);
        let b = p.alloc(34);
        assert_eq!((a, b), (0, 273));
        p.free(a, 273);
        assert_eq!(
            p.alloc(34),
            307,
            "a 34-word brick does not take a 273-word slot"
        );
        assert_eq!(p.alloc(273), 0, "the freed raw slot is reused");
        assert_eq!(p.live_words(), 273 + 34 + 34);
        assert_eq!(p.capacity_words(), 273 + 34 + 34);
    }

    #[test]
    fn random_allocations_never_overlap() {
        let sizes = [273usize, 401, 34, 51, 89];
        let mut p = BrickPool::new();
        let mut live: Vec<(u32, usize)> = Vec::new();
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..5000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            if live.len() > 10 && s.is_multiple_of(3) {
                let i = (s >> 8) as usize % live.len();
                let (at, size) = live.swap_remove(i);
                p.free(at, size);
            } else {
                let size = sizes[(s >> 20) as usize % sizes.len()];
                let at = p.alloc(size);
                assert!(at as usize + size <= p.capacity_words());
                live.push((at, size));
            }
        }
        live.sort();
        for w in live.windows(2) {
            assert!(w[0].0 as usize + w[0].1 <= w[1].0 as usize, "{w:?} overlap");
        }
        assert_eq!(p.live_words(), live.iter().map(|l| l.1).sum::<usize>());
    }

    #[test]
    fn index_pool_reuses() {
        let mut p = IndexPool::default();
        assert_eq!((p.alloc(), p.alloc(), p.alloc()), (0, 1, 2));
        p.free(1);
        assert_eq!(p.alloc(), 1);
        assert_eq!(p.alloc(), 3);
        assert_eq!(p.live(), 4);
    }
}
