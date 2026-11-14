//! Whole-map grids stored by their exceptions (BUGS.md #70: the starter world's biome set held
//! ~27 MB a face of grids, and a phone's tab ran out of memory). Most of a face's map is sea, where
//! the coast distance, the rain, the drainage distance and its line are all one value: a
//! [`SparseGrid`] keeps a default and stores only the cells whose value differs from it (by its
//! bits, so a read is the dense grid's value exactly and the terrain does not change).

/// A grid value compared by its bits (`f32` -0.0 and 0.0 differ, as the dense grid's would).
pub trait Bits: Copy {
    fn same(self, other: Self) -> bool;
}

impl Bits for f32 {
    fn same(self, other: Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Bits for u32 {
    fn same(self, other: Self) -> bool {
        self == other
    }
}

/// A grid of `len` cells holding `default` everywhere but the cells it stores.
#[derive(Clone, Debug)]
pub struct SparseGrid<T> {
    default: T,
    /// Bit `c % 64` of word `c / 64`: cell c is stored.
    stored: Vec<u64>,
    /// The stored cells before word w's (the index of its first value).
    rank: Vec<u32>,
    values: Vec<T>,
}

impl<T: Bits> SparseGrid<T> {
    /// `dense` stored as `default` and the cells whose value differs from it.
    pub fn from_dense(dense: &[T], default: T) -> Self {
        let words = dense.len().div_ceil(64);
        let mut stored = vec![0u64; words];
        let mut rank = Vec::with_capacity(words);
        let mut count = 0usize;
        for (w, bits) in stored.iter_mut().enumerate() {
            rank.push(u32::try_from(count).expect("a grid of under 2^32 cells"));
            let cells = &dense[w * 64..((w + 1) * 64).min(dense.len())];
            for (b, v) in cells.iter().enumerate() {
                if !v.same(default) {
                    *bits |= 1 << b;
                    count += 1;
                }
            }
        }
        let mut values = Vec::with_capacity(count);
        values.extend(dense.iter().copied().filter(|v| !v.same(default)));
        Self {
            default,
            stored,
            rank,
            values,
        }
    }

    /// The value at cell `c`, exactly the dense grid's.
    #[inline]
    pub fn get(&self, c: usize) -> T {
        let (w, b) = (c / 64, c % 64);
        let bits = self.stored[w];
        if bits & (1 << b) == 0 {
            return self.default;
        }
        let below = (bits & ((1u64 << b) - 1)).count_ones() as usize;
        self.values[self.rank[w] as usize + below]
    }

    /// The bytes its allocations hold.
    pub fn bytes(&self) -> usize {
        crate::vec_bytes(&self.stored)
            + crate::vec_bytes(&self.rank)
            + crate::vec_bytes(&self.values)
    }
}

/// The value most cells of `dense` hold (by bits; the first such on a tie), for a default.
pub fn most_common(dense: &[f32]) -> f32 {
    let mut counts: std::collections::HashMap<u32, (usize, usize)> =
        std::collections::HashMap::new();
    for (i, v) in dense.iter().enumerate() {
        counts.entry(v.to_bits()).or_insert((0, i)).0 += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)))
        .map_or(0.0, |(bits, _)| f32::from_bits(bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sparse_grid_reads_every_cell_as_the_dense_grid_bit_for_bit() {
        // A pseudo-random grid with runs of the default, -0.0 next to 0.0, and odd lengths.
        for len in [0usize, 1, 63, 64, 65, 1000, 4099] {
            let mut x = 0x9e37_79b9u32;
            let dense: Vec<f32> = (0..len)
                .map(|i| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    match x % 5 {
                        0 | 1 => 0.0,
                        2 => -0.0,
                        3 => f32::INFINITY,
                        _ => i as f32 * 0.37,
                    }
                })
                .collect();
            for default in [0.0f32, -0.0, most_common(&dense)] {
                let g = SparseGrid::from_dense(&dense, default);
                for (c, v) in dense.iter().enumerate() {
                    assert_eq!(g.get(c).to_bits(), v.to_bits(), "len {len} cell {c}");
                }
            }
        }
    }

    #[test]
    fn a_grid_mostly_of_its_default_stores_only_the_rest() {
        let mut dense = vec![7u32; 10_000];
        dense[42] = 1;
        dense[9_999] = 2;
        let g = SparseGrid::from_dense(&dense, 7);
        assert_eq!(g.values, vec![1, 2]);
        assert!(g.bytes() < 10_000 * 4 / 10, "{} bytes", g.bytes());
        assert_eq!((g.get(41), g.get(42), g.get(9_999)), (7, 1, 2));
    }
}
