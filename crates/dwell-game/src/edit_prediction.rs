//! Edit prediction (ARCHITECTURE.md §6.5, BRUSHES.md §4): the client shows its own edits at once —
//! a break, a place, a region tool's edit, a brush stroke — and reconciles with the server.
//!
//! One mechanism serves every edit. Each edit the client sends has an id; the cells it is predicted
//! to change are written into the client's world at once, and an **overlay** keeps, for each such
//! cell, the server's state (what the cell held, then whatever the server's modifications say) and
//! how many predicted edits still wait on it. While a cell waits, the server's changes to it go to
//! the overlay instead of the world, so the prediction stays on screen. When the server's
//! `EditAck` says an edit is done (applied or refused), each of its cells waits on one edit fewer;
//! a cell no edit waits on any more shows the server's state again. The ack comes after the tick's
//! `VoxelModification` on the same ordered channel, so by then the overlay holds the server's
//! result: a confirmed edit shows no change, and a refused or different one is rolled back and
//! replaced with what the server did.

use std::collections::HashMap;

use dwell_blocks::StateId;
use dwell_protocol::ChunkChanges;
use dwell_world::CHUNK_SIZE;

/// A cell and the state an edit is predicted to put there.
pub type Change = ([i32; 3], StateId);

/// What prediction reads and writes: the client's world.
pub trait PredictionWorld {
    /// The state the client shows at `c`.
    fn state(&mut self, c: [i32; 3]) -> StateId;
    /// Shows these states (predicted, or the server's again), remeshing what they touch.
    fn show(&mut self, cells: &[Change]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Overlay {
    /// The server's state of the cell, as far as the client has heard.
    server: StateId,
    /// Predicted edits not yet acknowledged that change it.
    waiting: u32,
}

/// Counts for the F3 overlay and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PredictionStats {
    /// Edits predicted (with at least one cell).
    pub predicted: u32,
    /// Edits the server refused.
    pub refused: u32,
    /// Cells that showed something other than the server's state when their edits were done:
    /// rolled back or replaced.
    pub corrected: u32,
}

#[derive(Clone, Debug, Default)]
pub struct EditPredictor {
    next_id: u32,
    cells: HashMap<[i32; 3], Overlay>,
    /// Each edit not yet acknowledged → the cells it predicted.
    edits: HashMap<u32, Vec<[i32; 3]>>,
    pub stats: PredictionStats,
}

fn chunk_cell(coord: [i32; 3], index: u16) -> [i32; 3] {
    let i = i32::from(index);
    [
        coord[0] * CHUNK_SIZE + (i & 31),
        coord[1] * CHUNK_SIZE + ((i >> 5) & 31),
        coord[2] * CHUNK_SIZE + ((i >> 10) & 31),
    ]
}

impl EditPredictor {
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the next edit sent (every edit has one, predicted or not).
    pub fn next_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    /// Edits waiting for the server.
    pub fn pending(&self) -> usize {
        self.edits.len()
    }

    /// Cells showing a prediction.
    pub fn predicted_cells(&self) -> usize {
        self.cells.len()
    }

    /// The server's state of `c` while it shows a prediction.
    pub fn server_state(&self, c: [i32; 3]) -> Option<StateId> {
        self.cells.get(&c).map(|o| o.server)
    }

    /// Shows edit `id`'s predicted `changes` at once.
    pub fn predict(&mut self, id: u32, changes: &[Change], world: &mut impl PredictionWorld) {
        if changes.is_empty() {
            return;
        }
        let mut cells = Vec::with_capacity(changes.len());
        for &(c, _) in changes {
            let shown = world.state(c);
            let o = self.cells.entry(c).or_insert(Overlay {
                server: shown,
                waiting: 0,
            });
            o.waiting += 1;
            cells.push(c);
        }
        world.show(changes);
        self.stats.predicted += 1;
        // An id seen twice (it can't be) would leave cells waiting: add to its list.
        self.edits.entry(id).or_default().extend(cells);
    }

    /// The server's modification (§6.5) before it reaches the world: changes to cells showing a
    /// prediction go to their overlay and are taken out (the chunk's revision still applies).
    pub fn filter(&mut self, chunks: &mut [ChunkChanges]) {
        if self.cells.is_empty() {
            return;
        }
        for c in chunks {
            let coord = c.coord;
            c.changes.retain(|ch| {
                match self.cells.get_mut(&chunk_cell(coord, ch.index)) {
                    Some(o) => {
                        o.server = ch.material;
                        false
                    }
                    None => true,
                }
            });
        }
    }

    /// The server is done with these edits (`(id, refused)`, from its `EditAck`, or refused
    /// without being sent): their cells wait on one edit fewer, and those no edit waits on show
    /// the server's state.
    pub fn resolve(&mut self, results: &[(u32, bool)], world: &mut impl PredictionWorld) {
        let mut back = Vec::new();
        for &(id, refused) in results {
            if refused {
                self.stats.refused += 1;
            }
            for c in self.edits.remove(&id).unwrap_or_default() {
                let Some(o) = self.cells.get_mut(&c) else {
                    continue; // its chunk was replaced meanwhile
                };
                o.waiting -= 1;
                if o.waiting == 0 {
                    let server = o.server;
                    self.cells.remove(&c);
                    if world.state(c) != server {
                        self.stats.corrected += 1;
                        back.push((c, server));
                    }
                }
            }
        }
        if !back.is_empty() {
            world.show(&back);
        }
    }

    /// The server replaced or unloaded this chunk: its data is the server's, so its cells' overlays
    /// go (their edits still count down; nothing is written back).
    pub fn forget_chunk(&mut self, coord: [i32; 3]) {
        if self.cells.is_empty() {
            return;
        }
        self.cells
            .retain(|c, _| c.map(|v| v.div_euclid(CHUNK_SIZE)) != coord);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::VoxelChange;

    /// A world of a few cells; `shown` counts writes.
    #[derive(Default)]
    struct Cells {
        map: HashMap<[i32; 3], StateId>,
        writes: usize,
    }

    impl PredictionWorld for Cells {
        fn state(&mut self, c: [i32; 3]) -> StateId {
            self.map.get(&c).copied().unwrap_or(0)
        }
        fn show(&mut self, cells: &[Change]) {
            for &(c, m) in cells {
                self.map.insert(c, m);
                self.writes += 1;
            }
        }
    }

    impl Cells {
        /// What the client would apply of the server's modification, after the predictor.
        fn modification(&mut self, p: &mut EditPredictor, changes: &[Change]) {
            let mut chunks: Vec<ChunkChanges> = changes
                .iter()
                .map(|&(c, m)| {
                    let coord = c.map(|v| v.div_euclid(CHUNK_SIZE));
                    let l = c.map(|v| v.rem_euclid(CHUNK_SIZE));
                    ChunkChanges {
                        coord,
                        revision: 1,
                        changes: vec![VoxelChange {
                            index: (l[0] | l[1] << 5 | l[2] << 10) as u16,
                            material: m,
                        }],
                    }
                })
                .collect();
            p.filter(&mut chunks);
            for ch in chunks {
                for v in ch.changes {
                    let c = chunk_cell(ch.coord, v.index);
                    self.map.insert(c, v.material);
                }
            }
        }
    }

    const STONE: StateId = 1;
    const SAND: StateId = 6;

    #[test]
    fn a_prediction_shows_at_once_and_a_confirmation_changes_nothing() {
        let (mut w, mut p) = (Cells::default(), EditPredictor::new());
        let id = p.next_id();
        p.predict(id, &[([1, 2, 3], STONE)], &mut w);
        assert_eq!(w.state([1, 2, 3]), STONE, "before the server answers");
        assert_eq!(p.pending(), 1);
        // The server applies it: its change goes to the overlay, then the ack.
        w.modification(&mut p, &[([1, 2, 3], STONE)]);
        let writes = w.writes;
        p.resolve(&[(id, false)], &mut w);
        assert_eq!(w.state([1, 2, 3]), STONE);
        assert_eq!(w.writes, writes, "nothing to write back");
        assert_eq!((p.pending(), p.predicted_cells(), p.stats.corrected), (0, 0, 0));
    }

    #[test]
    fn a_refused_prediction_rolls_back() {
        let (mut w, mut p) = (Cells::default(), EditPredictor::new());
        w.map.insert([0, 0, 0], SAND);
        let id = p.next_id();
        p.predict(id, &[([0, 0, 0], 0), ([1, 0, 0], STONE)], &mut w);
        assert_eq!((w.state([0, 0, 0]), w.state([1, 0, 0])), (0, STONE));
        p.resolve(&[(id, true)], &mut w);
        assert_eq!((w.state([0, 0, 0]), w.state([1, 0, 0])), (SAND, 0));
        assert_eq!((p.stats.refused, p.stats.corrected), (1, 2));
    }

    #[test]
    fn a_different_result_replaces_the_prediction() {
        let (mut w, mut p) = (Cells::default(), EditPredictor::new());
        let id = p.next_id();
        p.predict(id, &[([0, 0, 0], STONE)], &mut w);
        // Another player's edit landed first: the server put sand there.
        w.modification(&mut p, &[([0, 0, 0], SAND)]);
        assert_eq!(w.state([0, 0, 0]), STONE, "the prediction stays until the ack");
        p.resolve(&[(id, false)], &mut w);
        assert_eq!(w.state([0, 0, 0]), SAND);
    }

    #[test]
    fn rapid_edits_and_a_refusal_reconcile_to_the_server() {
        let (mut w, mut p) = (Cells::default(), EditPredictor::new());
        let c = [4, 4, 4];
        // Place, break, place again on one cell; the server refuses the break (too fast).
        let (a, b, d) = (p.next_id(), p.next_id(), p.next_id());
        p.predict(a, &[(c, STONE)], &mut w);
        p.predict(b, &[(c, 0)], &mut w);
        p.predict(d, &[(c, SAND)], &mut w);
        assert_eq!(w.state(c), SAND);
        // The server: a applied, b refused, d refused (the cell was full); one ack covers a and b.
        w.modification(&mut p, &[(c, STONE)]);
        p.resolve(&[(a, false), (b, true)], &mut w);
        assert_eq!(w.state(c), SAND, "d still waits");
        p.resolve(&[(d, true)], &mut w);
        assert_eq!(w.state(c), STONE, "the server's");
        assert_eq!(p.predicted_cells(), 0);
        // Acks in another order than the edits were made (a stroke's queued edits go out after a
        // break) count down the same.
        let (e, f) = (p.next_id(), p.next_id());
        p.predict(e, &[([9, 9, 9], STONE)], &mut w);
        p.predict(f, &[([9, 9, 9], SAND)], &mut w);
        w.modification(&mut p, &[([9, 9, 9], SAND)]);
        p.resolve(&[(f, false)], &mut w);
        p.resolve(&[(e, false)], &mut w);
        assert_eq!(w.state([9, 9, 9]), SAND);
    }

    #[test]
    fn a_replaced_chunk_drops_its_overlays() {
        let (mut w, mut p) = (Cells::default(), EditPredictor::new());
        let id = p.next_id();
        p.predict(id, &[([1, 1, 1], STONE), ([40, 1, 1], STONE)], &mut w);
        p.forget_chunk([0, 0, 0]);
        // The chunk's new data (the server's) arrived; the ack must not write the old state back.
        w.map.insert([1, 1, 1], SAND);
        p.resolve(&[(id, false)], &mut w);
        assert_eq!(w.state([1, 1, 1]), SAND);
        assert_eq!(w.state([40, 1, 1]), 0, "the other chunk's cell rolls back");
    }
}
