//! Placed blocks held up by the ground (WORK_QUEUE first pass 3a; owner, 2026-11-25: "All blocks
//! should behave like that, falling when disconnected from their supporting (connecting to solid
//! ground) blocks", "Always one body", "natural terrain anchors", "I like the distance marks do
//! that"; docs/FELLING.md §8).
//!
//! Every placed block (one that differs from the cell as generated, of the `ground` support role,
//! solid, standing on no `needsSupport` rule of its own) keeps its **distance** to what anchors it:
//! the fewest face steps through placed blocks to an anchor — natural ground (a solid cell as
//! generated), or a block with its own support rule (a log, sand, leaves), which is distance 0.
//!
//! - **Placing** sets a block's distance to its least neighbour's + 1 and lowers any neighbour it
//!   gives a shorter way down.
//! - **Removing** (a break, a blast, a dug anchor) re-checks only the blocks whose distance may have
//!   run through it: one with a neighbour still one step nearer keeps its distance and the walk
//!   stops there; the rest lose theirs, are refilled from the blocks round them that kept theirs,
//!   and what no way to an anchor reaches is **cut off**. Each 6-connected group of cut-off blocks
//!   falls as one body, however large.
//! - A placed block with no distance yet (loaded from a save, or written before this rule) gets one
//!   when a change first reaches it: its placed component is walked once from the anchors round it.
//!
//! Face-agnostic: the six face neighbours, no up or down. Deterministic: the same world and changes
//! give the same groups in the same order.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use dwell_blocks::{Support, SupportRole, try_info};

use crate::hash::{HashMap, HashSet};
use crate::{Chunk, ChunkCoord, M, VoxelWorld};

/// The six face neighbours.
const DIRS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Generated chunks kept for reading cells as generated; past this many the cache starts over.
const GENERATED_CACHE_CHUNKS: usize = 64;

fn add(a: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    [a[0] + d[0], a[1] + d[1], a[2] + d[2]]
}

/// What a cell is to the rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A placed block the rule holds up (or lets fall).
    Placed,
    /// Holds up what touches it: natural solid ground, or a block with its own support rule.
    Anchor,
    /// Neither: air, liquids, plants and blocks hung on something (`needsSupport`).
    Other,
}

/// The distances of placed blocks to the ground (see the module).
#[derive(Default)]
pub struct Anchoring {
    dist: HashMap<[i32; 3], u32>,
    generated: HashMap<ChunkCoord, Box<Chunk>>,
    /// During an update: the blocks being re-checked (no distance until refilled), which working
    /// out an unknown block's distance must not walk into.
    pending: HashSet<[i32; 3]>,
    /// The blocks the last update re-checked (lost their distance and were refilled or cut off).
    walked: usize,
}

impl Anchoring {
    pub fn new() -> Self {
        Self::default()
    }

    /// The distance kept for `cell` (a placed block's), if any.
    pub fn distance(&self, cell: [i32; 3]) -> Option<u32> {
        self.dist.get(&cell).copied()
    }

    /// How many placed blocks have a distance.
    pub fn len(&self) -> usize {
        self.dist.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dist.is_empty()
    }

    /// How many blocks the last [`Self::update`] re-checked: the cost of a change, which stops at
    /// the blocks still held one step nearer the ground (a break in a big build walks a few).
    pub fn walked(&self) -> usize {
        self.walked
    }

    fn kind(&mut self, world: &mut VoxelWorld, c: [i32; 3]) -> Kind {
        let m = world.get_voxel(c[0], c[1], c[2]);
        if m == M.air {
            return Kind::Other;
        }
        let Some(s) = try_info(m) else {
            return Kind::Other;
        };
        if s.liquid || !s.solid || s.passable && s.support_role == SupportRole::Ground {
            return Kind::Other;
        }
        if s.support_role != SupportRole::Ground {
            // Logs, sand, leaves: their own rule holds them (docs/FELLING.md §2).
            return Kind::Anchor;
        }
        if s.needs_support != Support::None {
            return Kind::Other;
        }
        if self.generated.len() > GENERATED_CACHE_CHUNKS {
            self.generated.clear();
        }
        if world.generated_voxel(c, &mut self.generated) == m {
            Kind::Anchor
        } else {
            Kind::Placed
        }
    }

    /// The distance of `c` as things stand: 0 an anchor, a placed block's kept one (worked out now
    /// if it has none: `cut` gets what that finds cut off), none for anything else or a block
    /// found cut off.
    fn known(
        &mut self,
        world: &mut VoxelWorld,
        c: [i32; 3],
        cut: &mut HashSet<[i32; 3]>,
    ) -> Option<u32> {
        match self.kind(world, c) {
            Kind::Anchor => Some(0),
            Kind::Other => None,
            Kind::Placed => {
                if let Some(&d) = self.dist.get(&c) {
                    return Some(d);
                }
                if cut.contains(&c) || self.pending.contains(&c) {
                    return None;
                }
                self.ensure(world, c, cut);
                self.dist.get(&c).copied()
            }
        }
    }

    /// Gives every placed block of `start`'s component without a distance one: the component is
    /// walked once and filled outward from the anchors and the kept distances round it; what it
    /// doesn't reach is cut off (into `cut`).
    fn ensure(&mut self, world: &mut VoxelWorld, start: [i32; 3], cut: &mut HashSet<[i32; 3]>) {
        let mut comp: HashSet<[i32; 3]> = HashSet::default();
        let mut order = vec![start];
        comp.insert(start);
        let mut heap: BinaryHeap<Reverse<(u32, [i32; 3])>> = BinaryHeap::new();
        let mut i = 0;
        while i < order.len() {
            let x = order[i];
            i += 1;
            for d in DIRS {
                let n = add(x, d);
                if comp.contains(&n) || self.pending.contains(&n) {
                    continue;
                }
                match self.kind(world, n) {
                    Kind::Anchor => heap.push(Reverse((1, x))),
                    Kind::Placed => {
                        if let Some(&k) = self.dist.get(&n) {
                            heap.push(Reverse((k + 1, x)));
                        } else if !cut.contains(&n) {
                            comp.insert(n);
                            order.push(n);
                        }
                    }
                    Kind::Other => {}
                }
            }
        }
        while let Some(Reverse((d, x))) = heap.pop() {
            if self.dist.get(&x).is_some_and(|&k| k <= d) {
                continue;
            }
            self.dist.insert(x, d);
            for dir in DIRS {
                let n = add(x, dir);
                if comp.contains(&n) && self.dist.get(&n).is_none_or(|&k| k > d + 1) {
                    heap.push(Reverse((d + 1, n)));
                }
            }
        }
        for c in order {
            if !self.dist.contains_key(&c) {
                cut.insert(c);
            }
        }
    }

    /// Updates the distances after the cells `changed` were written (whatever wrote them), and
    /// returns the groups of placed blocks cut off from every anchor: each 6-connected group, its
    /// cells sorted, the groups by their first cell. Their distances are dropped (they are to fall).
    pub fn update(&mut self, world: &mut VoxelWorld, changed: &[[i32; 3]]) -> Vec<Vec<[i32; 3]>> {
        let mut cut: HashSet<[i32; 3]> = HashSet::default();
        self.pending.clear();
        self.walked = 0;
        // What went (a removed placed block, a dug anchor, a block whose kind changed): the
        // neighbours whose distance may have run through it are re-checked. What was placed is
        // pending until the refill gives it a distance.
        let mut removed: Vec<([i32; 3], u32)> = Vec::new();
        let mut sources: Vec<[i32; 3]> = Vec::new();
        for &c in changed {
            let old = self.dist.remove(&c);
            match self.kind(world, c) {
                Kind::Placed => {
                    if let Some(d) = old {
                        // Still a placed block (a door opened, a state turned): as it was.
                        self.dist.insert(c, d);
                    } else {
                        // New here, or an anchor replaced by a placed block: what leaned on it as
                        // an anchor is re-checked, and it takes its own distance in the refill.
                        removed.push((c, 0));
                        self.pending.insert(c);
                    }
                }
                Kind::Anchor => sources.push(c),
                Kind::Other => removed.push((c, old.unwrap_or(0))),
            }
        }
        // Removal: the blocks a gone cell may have held, then theirs, while none of them has a
        // neighbour still one step nearer the ground.
        let mut head = 0;
        let mut queue = removed;
        while head < queue.len() {
            let (x, dx) = queue[head];
            head += 1;
            for d in DIRS {
                let n = add(x, d);
                if self.pending.contains(&n) {
                    continue;
                }
                let Some(dn) = self.known(world, n, &mut cut) else {
                    continue;
                };
                if dn == 0 || dn <= dx || self.held(world, n, dn, &mut cut) {
                    sources.push(n);
                    continue;
                }
                self.pending.insert(n);
                self.dist.remove(&n);
                self.walked += 1;
                queue.push((n, dn));
            }
        }
        // Refill: outward from every block round the pending ones that kept its distance (and from
        // new anchors), into the pending blocks and lowering any placed block a new one gives a
        // shorter way down.
        let pending: Vec<[i32; 3]> = self.pending.iter().copied().collect();
        for &c in &pending {
            for d in DIRS {
                let n = add(c, d);
                if !self.pending.contains(&n) {
                    sources.push(n);
                }
            }
        }
        let mut heap: BinaryHeap<Reverse<(u32, [i32; 3])>> = BinaryHeap::new();
        for c in sources {
            if let Some(d) = self.known(world, c, &mut cut) {
                heap.push(Reverse((d, c)));
            }
        }
        let mut done: HashSet<[i32; 3]> = HashSet::default();
        while let Some(Reverse((d, x))) = heap.pop() {
            if !done.insert(x) {
                continue;
            }
            if self.pending.remove(&x) {
                self.dist.insert(x, d);
            } else if self.known(world, x, &mut cut) != Some(d) {
                done.remove(&x);
                continue;
            }
            for dir in DIRS {
                let n = add(x, dir);
                if done.contains(&n) {
                    continue;
                }
                let lower = if self.pending.contains(&n) {
                    true
                } else {
                    self.dist.get(&n).is_some_and(|&k| k > d + 1)
                };
                if lower {
                    self.dist.remove(&n);
                    self.pending.insert(n);
                    heap.push(Reverse((d + 1, n)));
                }
            }
        }
        // What the refill never reached is cut off; only what is still a placed block falls.
        let left: Vec<[i32; 3]> = self.pending.drain().collect();
        cut.extend(left);
        let mut cut: Vec<[i32; 3]> = cut
            .into_iter()
            .filter(|&c| {
                self.dist.remove(&c);
                self.kind(world, c) == Kind::Placed
            })
            .collect();
        cut.sort_unstable();
        groups(&cut)
    }

    /// Whether placed block `n` (distance `dn`) still has a neighbour one step nearer the ground
    /// that is not being re-checked.
    fn held(
        &mut self,
        world: &mut VoxelWorld,
        n: [i32; 3],
        dn: u32,
        cut: &mut HashSet<[i32; 3]>,
    ) -> bool {
        DIRS.iter().any(|&d| {
            let m = add(n, d);
            !self.pending.contains(&m) && self.known(world, m, cut) == Some(dn - 1)
        })
    }
}

/// The 6-connected groups of `cells` (sorted), each sorted, in the order of their first cell.
fn groups(cells: &[[i32; 3]]) -> Vec<Vec<[i32; 3]>> {
    let all: HashSet<[i32; 3]> = cells.iter().copied().collect();
    let mut seen: HashSet<[i32; 3]> = HashSet::default();
    let mut out = Vec::new();
    for &start in cells {
        if !seen.insert(start) {
            continue;
        }
        let mut group = vec![start];
        let mut i = 0;
        while i < group.len() {
            let x = group[i];
            i += 1;
            for d in DIRS {
                let n = add(x, d);
                if all.contains(&n) && seen.insert(n) {
                    group.push(n);
                }
            }
        }
        group.sort_unstable();
        out.push(group);
    }
    out
}

#[cfg(test)]
mod tests;
