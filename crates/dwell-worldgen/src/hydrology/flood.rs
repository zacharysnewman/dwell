//! Priority-flood depression filling (Barnes, Lehman & Mulla 2014): every land cell's water level
//! (its height, or the spill level of the depression it lies in) and a drainage tree rooted at the
//! sea. The heap pops cells in order of (level, index), a total order, so the result is the same on
//! every target.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

/// No cell (a sea cell's receiver, an empty slot).
pub const NONE: u32 = u32::MAX;

/// A square grid of `n` × `n` cells, row-major: index `z * n + x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub n: usize,
}

/// The eight neighbours, in a fixed order: (dx, dz, diagonal).
pub const NEIGHBOURS: [(i32, i32, bool); 8] = [
    (1, 0, false),
    (0, 1, false),
    (-1, 0, false),
    (0, -1, false),
    (1, 1, true),
    (-1, 1, true),
    (-1, -1, true),
    (1, -1, true),
];

impl Grid {
    pub const fn len(self) -> usize {
        self.n * self.n
    }

    pub const fn is_empty(self) -> bool {
        self.n == 0
    }

    /// The neighbours of cell `i` inside the grid: (index, diagonal).
    pub fn neighbours(self, i: usize) -> impl Iterator<Item = (usize, bool)> {
        let n = self.n as i32;
        let (x, z) = ((i % self.n) as i32, (i / self.n) as i32);
        NEIGHBOURS.iter().filter_map(move |&(dx, dz, diag)| {
            let (nx, nz) = (x + dx, z + dz);
            (nx >= 0 && nz >= 0 && nx < n && nz < n).then_some(((nz * n + nx) as usize, diag))
        })
    }

    /// Whether cell `i` is on the grid's border.
    pub fn on_border(self, i: usize) -> bool {
        let (x, z) = (i % self.n, i / self.n);
        x == 0 || z == 0 || x + 1 == self.n || z + 1 == self.n
    }
}

/// A flooded surface.
pub struct Flood {
    /// The water level (m above sea level): a land cell's height, or the spill level of the
    /// depression it lies in; 0 at sea.
    pub level: Vec<f64>,
    /// The cell each land cell was flooded from: a neighbour no higher than its level, on the way
    /// to the sea. `NONE` at sea.
    pub parent: Vec<u32>,
    /// Land cells in the order they were flooded: every cell after its parent.
    pub order: Vec<u32>,
}

/// The land cells next to the sea, each with its first sea neighbour: where the flood starts.
pub fn shore(grid: Grid, sea: &[bool]) -> Vec<(u32, u32)> {
    let mut queued: Vec<bool> = sea.to_vec();
    let mut out = Vec::new();
    for (i, &s) in sea.iter().enumerate() {
        if !s {
            continue;
        }
        for (j, _) in grid.neighbours(i) {
            if !queued[j] {
                queued[j] = true;
                out.push((j as u32, i as u32));
            }
        }
    }
    out
}

/// Floods the land (`!sea`) from the sea, whose level is 0, starting at `shore` (`shore(grid,
/// sea)`). Every land cell must be reachable from a sea cell (callers make the grid's border sea).
/// Land may lie below sea level (a hollow cut off from the sea): it is then under its depression's
/// water. Barnes et al.'s improved variant: a cell flooded at the current level (in a depression)
/// goes to a first-in-first-out queue served before the heap, so levels still come out in order.
pub fn flood(grid: Grid, height: &[f64], sea: &[bool], shore: &[(u32, u32)]) -> Flood {
    let len = grid.len();
    let mut level = vec![0.0f64; len];
    let mut parent = vec![NONE; len];
    let mut queued: Vec<bool> = sea.to_vec();
    let mut order = Vec::with_capacity(len);
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(shore.len() * 2);
    let mut pit: VecDeque<u32> = VecDeque::new();
    // The bits of an f64 in an order that sorts like the value (sign flipped; negatives inverted).
    let key = |v: f64| {
        let b = v.to_bits();
        if b >> 63 == 1 { !b } else { b | 1 << 63 }
    };
    for &(j, i) in shore {
        let j = j as usize;
        queued[j] = true;
        parent[j] = i;
        level[j] = height[j].max(0.0);
        heap.push(Reverse((key(level[j]), j as u32)));
    }
    loop {
        let c = if let Some(c) = pit.pop_front() {
            c
        } else if let Some(Reverse((_, c))) = heap.pop() {
            c
        } else {
            break;
        };
        let c = c as usize;
        order.push(c as u32);
        let lc = level[c];
        for (j, _) in grid.neighbours(c) {
            if !queued[j] {
                queued[j] = true;
                parent[j] = c as u32;
                if height[j] <= lc {
                    level[j] = lc;
                    pit.push_back(j as u32);
                } else {
                    level[j] = height[j];
                    heap.push(Reverse((key(level[j]), j as u32)));
                }
            }
        }
    }
    debug_assert_eq!(order.len(), sea.iter().filter(|s| !**s).count());
    Flood {
        level,
        parent,
        order,
    }
}
