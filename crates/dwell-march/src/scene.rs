//! The CPU mirror of the GPU buffers: the clipmap levels and their slots, nodes, column grids and
//! the brick pool. Builds ([`crate::build`]) are placed here; [`Scene::gpu`] gives the words to
//! upload, and the reference marcher ([`crate::march`]) reads the same structures.

use std::collections::{BTreeMap, HashMap};

use dwell_blocks::StateId;
use dwell_world::{ChunkCoord, lod};

use crate::alloc::{BrickPool, IndexPool};
use crate::build::{NodeBuild, build_chunk, build_section};
use crate::layout::*;
use crate::state::StateTable;

pub const COLUMNS_NONE: u32 = 0xFFFF_FFFF;

#[derive(Clone, Debug)]
pub(crate) struct Level {
    pub info: LevelInfo,
    pub slots: Vec<u32>,
}

/// The words to upload, one `Vec` per buffer (crate::layout).
#[derive(Clone, Debug, Default)]
pub struct GpuBuffers {
    pub levels: Vec<u32>,
    pub slots: Vec<u32>,
    pub nodes: Vec<u32>,
    pub node_columns: Vec<u32>,
    pub columns: Vec<u32>,
    pub pool: Vec<u32>,
    pub states: Vec<u32>,
}

impl GpuBuffers {
    pub fn bytes(&self) -> usize {
        4 * (self.levels.len()
            + self.slots.len()
            + self.nodes.len()
            + self.node_columns.len()
            + self.columns.len()
            + self.pool.len()
            + self.states.len())
    }
}

/// What changed since the last [`Scene::take_dirty`]: the GPU mirror writes only these (one
/// `queue.writeBuffer` per range).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    /// A level's box changed: every level's `slot_base` may have moved, so the levels and all
    /// slots go up again.
    pub layout: bool,
    /// Slots by their index in the concatenated `slots` buffer (valid when `layout` is false).
    pub slots: Vec<u32>,
    /// Nodes (and their `node_columns` entry) by index.
    pub nodes: Vec<u32>,
    /// Column grids by word offset in `columns` (`COLUMN_WORDS` each).
    pub columns: Vec<u32>,
    /// Pool ranges: word offset and length.
    pub pool: Vec<(u32, u32)>,
    /// The nodes whose content changed (put or removed), as `(level, node)`: what a ray finds
    /// in each one's box may differ from before, at any level (a removed node lets a coarser
    /// level show through). The temporal start (A4c) trusts last frame's clear distances only
    /// outside these boxes.
    pub regions: Vec<(u32, [i32; 3])>,
}

impl Dirty {
    pub fn is_empty(&self) -> bool {
        !self.layout
            && self.slots.is_empty()
            && self.nodes.is_empty()
            && self.columns.is_empty()
            && self.pool.is_empty()
            && self.regions.is_empty()
    }

    /// The changed regions as level-0 grid boxes, `(lo, hi)` inclusive, at most `max` of them:
    /// over that, each level's boxes are merged into their bounding box (still covering every
    /// change), and over that again, all of them into one.
    pub fn boxes(&self, max: usize) -> Vec<([i32; 3], [i32; 3])> {
        type Box3 = ([i32; 3], [i32; 3]);
        let of = |&(l, n): &(u32, [i32; 3])| -> Box3 {
            let s = l + 5;
            (n.map(|v| v << s), n.map(|v| ((v + 1) << s) - 1))
        };
        let join = |a: Box3, b: Box3| -> Box3 {
            (
                std::array::from_fn(|k| a.0[k].min(b.0[k])),
                std::array::from_fn(|k| a.1[k].max(b.1[k])),
            )
        };
        let mut out: Vec<Box3> = Vec::new();
        for r in &self.regions {
            let b = of(r);
            if !out.contains(&b) {
                out.push(b);
            }
        }
        if out.len() <= max {
            return out;
        }
        let mut per_level: Vec<Option<Box3>> = vec![None; LEVEL_COUNT];
        for r in &self.regions {
            let b = of(r);
            let e = &mut per_level[r.0 as usize];
            *e = Some(e.map_or(b, |a| join(a, b)));
        }
        out = per_level.into_iter().flatten().collect();
        if out.len() > max {
            out = vec![out.iter().copied().reduce(join).expect("non-empty")];
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub(crate) table: StateTable,
    pub(crate) levels: Vec<Option<Level>>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) node_columns: Vec<u32>,
    pub(crate) columns: Vec<u32>,
    pub(crate) pool: BrickPool,
    node_ids: IndexPool,
    column_ids: IndexPool,
    /// Per node: its bricks (offset, words), to free on replace.
    node_bricks: Vec<Vec<(u32, usize)>>,
    /// Per placed node: the grid rows its occupied cells span (lo, hi exclusive), and per level
    /// how many nodes reach each bottom and top: the level's occupied rows ([`Scene::rows`]).
    extents: HashMap<(u32, [i32; 3]), (i32, i32)>,
    bottoms: Vec<BTreeMap<i32, u32>>,
    tops: Vec<BTreeMap<i32, u32>>,
    dirty: Dirty,
}

/// Grid cell → the cell at level `l` (floor).
pub(crate) fn shr(v: [i32; 3], l: u32) -> [i32; 3] {
    v.map(|x| x >> l)
}

impl Scene {
    pub fn new(table: StateTable) -> Self {
        Self {
            table,
            levels: vec![None; LEVEL_COUNT],
            nodes: Vec::new(),
            node_columns: Vec::new(),
            columns: Vec::new(),
            pool: BrickPool::new(),
            node_ids: IndexPool::default(),
            column_ids: IndexPool::default(),
            node_bricks: Vec::new(),
            extents: HashMap::new(),
            bottoms: vec![BTreeMap::new(); LEVEL_COUNT],
            tops: vec![BTreeMap::new(); LEVEL_COUNT],
            dirty: Dirty {
                layout: true,
                ..Dirty::default()
            },
        }
    }

    /// What changed since the last call (a new scene: everything).
    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    /// Where level `l`'s slots start in the concatenated `slots` buffer (`gpu`'s order).
    pub fn slot_base(&self, level: u32) -> u32 {
        self.levels[..level as usize]
            .iter()
            .flatten()
            .map(|lv| lv.slots.len() as u32)
            .sum()
    }

    /// The `levels` buffer's words (`slot_base` filled in level order).
    pub fn level_words(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(LEVEL_COUNT * LEVEL_WORDS);
        let mut base = 0u32;
        for (l, lv) in self.levels.iter().enumerate() {
            let mut info = lv.as_ref().map_or(
                LevelInfo {
                    cell_log2: l as u32,
                    ..LevelInfo::default()
                },
                |lv| lv.info,
            );
            info.slot_base = base;
            let rows = self.rows(l as u32).unwrap_or((i32::MAX, i32::MIN));
            out.extend(info.to_words_with_rows(rows));
            if let Some(lv) = lv {
                base += lv.slots.len() as u32;
            }
        }
        out
    }

    /// The concatenated `slots` buffer.
    pub fn slot_words(&self) -> Vec<u32> {
        self.levels
            .iter()
            .flatten()
            .flat_map(|lv| lv.slots.iter().copied())
            .collect()
    }

    /// Slot word `i` of the concatenated buffer.
    pub fn slot_word(&self, i: u32) -> u32 {
        let mut i = i as usize;
        for lv in self.levels.iter().flatten() {
            if i < lv.slots.len() {
                return lv.slots[i];
            }
            i -= lv.slots.len();
        }
        SLOT_NONE
    }

    /// The finest and coarsest levels with a box (what the marcher's uniforms carry).
    pub fn levels_in_use(&self) -> Option<(u32, u32)> {
        self.level_range()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn node_words(&self, i: u32) -> [u32; NODE_WORDS] {
        self.nodes[i as usize].to_words()
    }
    pub fn node_columns(&self) -> &[u32] {
        &self.node_columns
    }
    pub fn column_words(&self) -> &[u32] {
        &self.columns
    }
    pub fn pool_words(&self) -> &[u32] {
        self.pool.words()
    }
    /// Pool words in live bricks.
    pub fn pool_live_words(&self) -> usize {
        self.pool.live_words()
    }

    pub fn table(&self) -> &StateTable {
        &self.table
    }

    /// Makes level `level`'s box cover nodes `lo..=hi` (and what it covered), slots kept.
    pub fn reserve(&mut self, level: u32, lo: [i32; 3], hi: [i32; 3]) {
        let l = level as usize;
        let (olo, ohi) = match &self.levels[l] {
            Some(lv) => {
                let o = lv.info.origin;
                let e: [i32; 3] = std::array::from_fn(|k| o[k] + lv.info.dims[k] as i32 - 1);
                if (0..3).all(|k| lo[k] >= o[k] && hi[k] <= e[k]) {
                    return;
                }
                (
                    std::array::from_fn(|k| o[k].min(lo[k])),
                    std::array::from_fn(|k| e[k].max(hi[k])),
                )
            }
            None => (lo, hi),
        };
        let dims: [u32; 3] = std::array::from_fn(|k| (ohi[k] - olo[k] + 1) as u32);
        let info = LevelInfo {
            origin: olo,
            dims,
            slot_base: 0,
            cell_log2: level,
        };
        let mut slots = vec![SLOT_NONE; dims.iter().map(|&d| d as usize).product()];
        if let Some(old) = &self.levels[l] {
            let [dx, dy, dz] = old.info.dims.map(|d| d as i32);
            for z in 0..dz {
                for y in 0..dy {
                    for x in 0..dx {
                        let n = [
                            old.info.origin[0] + x,
                            old.info.origin[1] + y,
                            old.info.origin[2] + z,
                        ];
                        let (Some(from), Some(to)) = (old.info.slot_of(n), info.slot_of(n)) else {
                            continue;
                        };
                        slots[to] = old.slots[from];
                    }
                }
            }
        }
        self.levels[l] = Some(Level { info, slots });
        self.dirty.layout = true;
    }

    pub fn slot(&self, level: u32, n: [i32; 3]) -> Slot {
        self.levels[level as usize]
            .as_ref()
            .and_then(|lv| lv.info.slot_of(n).map(|i| Slot::from_word(lv.slots[i])))
            .unwrap_or(Slot::None)
    }

    fn release(&mut self, level: u32, n: [i32; 3]) {
        if let Slot::Node(i) = self.slot(level, n) {
            for (at, size) in std::mem::take(&mut self.node_bricks[i as usize]) {
                self.pool.free(at, size);
            }
            let c = self.node_columns[i as usize];
            if c != COLUMNS_NONE {
                self.column_ids.free(c / COLUMN_WORDS as u32);
                self.node_columns[i as usize] = COLUMNS_NONE;
            }
            self.node_ids.free(i);
        }
    }

    /// Places a build at node `n` of `level` (replacing what was there).
    pub fn put(&mut self, level: u32, n: [i32; 3], build: &NodeBuild) {
        self.reserve(level, n, n);
        self.release(level, n);
        // The rows its occupied cells span (in this level's cells from the node's bottom).
        let rows = match build {
            NodeBuild::Air => None,
            NodeBuild::Solid(_) => Some((0, NODE_CELLS)),
            NodeBuild::Node(d) => match &d.columns {
                Some(cols) => {
                    let (lo, hi) = cols.span(COLUMN_MIPS - 1, 0, 0);
                    (lo < hi).then_some((i32::from(lo), i32::from(hi)))
                }
                None => Some((0, NODE_CELLS)),
            },
        };
        let y0 = n[1] * NODE_CELLS;
        self.set_extent(
            level,
            n,
            rows.map(|(lo, hi)| ((y0 + lo) << level, (y0 + hi) << level)),
        );
        let slot = match build {
            NodeBuild::Air => Slot::AllAir,
            NodeBuild::Solid(s) => Slot::AllSolid(*s),
            NodeBuild::Node(d) => {
                let i = self.node_ids.alloc();
                let iu = i as usize;
                if iu == self.nodes.len() {
                    self.nodes.push(Node::default());
                    self.node_columns.push(COLUMNS_NONE);
                    self.node_bricks.push(Vec::new());
                }
                let mut node = d.node;
                let mut owned = Vec::with_capacity(d.bricks.len());
                for b in 0..BRICKS_PER_NODE {
                    if let BrickRef::Mixed(local) = node.get(b) {
                        let words = &d.bricks[local as usize];
                        let at = self.pool.insert(words);
                        self.dirty.pool.push((at, words.len() as u32));
                        owned.push((at, words.len()));
                        node.set(b, BrickRef::Mixed(at));
                    }
                }
                self.nodes[iu] = node;
                self.node_bricks[iu] = owned;
                if let Some(cols) = &d.columns {
                    let c = self.column_ids.alloc() as usize * COLUMN_WORDS;
                    if c + COLUMN_WORDS > self.columns.len() {
                        self.columns.resize(c + COLUMN_WORDS, 0);
                    }
                    self.columns[c..c + COLUMN_WORDS].copy_from_slice(&cols.to_words());
                    self.node_columns[iu] = c as u32;
                    self.dirty.columns.push(c as u32);
                }
                self.dirty.nodes.push(i);
                Slot::Node(i)
            }
        };
        let base = self.slot_base(level);
        let lv = self.levels[level as usize].as_mut().expect("reserved");
        let i = lv.info.slot_of(n).expect("inside");
        lv.slots[i] = slot.to_word();
        self.dirty.slots.push(base + i as u32);
        self.dirty.regions.push((level, n));
    }

    /// Back to NONE (no data at this level).
    pub fn remove(&mut self, level: u32, n: [i32; 3]) {
        self.release(level, n);
        self.set_extent(level, n, None);
        let base = self.slot_base(level);
        if let Some(lv) = self.levels[level as usize].as_mut()
            && let Some(i) = lv.info.slot_of(n)
        {
            lv.slots[i] = SLOT_NONE;
            self.dirty.slots.push(base + i as u32);
            self.dirty.regions.push((level, n));
        }
    }

    /// A chunk at level 0 (its node is the chunk's level-0 LOD section).
    pub fn put_chunk(&mut self, c: ChunkCoord, voxels: &[StateId]) -> NodeBuild {
        let b = build_chunk(voxels, &self.table);
        self.put(0, chunk_node(c), &b);
        b
    }

    /// A LOD section at its level.
    pub fn put_section(&mut self, c: lod::LodCoord, cells: &[StateId]) -> NodeBuild {
        let b = build_section(cells, &self.table);
        self.put(c.level as u32, [c.i, c.j, c.k], &b);
        b
    }

    fn set_extent(&mut self, level: u32, n: [i32; 3], rows: Option<(i32, i32)>) {
        let l = level as usize;
        let dec = |m: &mut BTreeMap<i32, u32>, k: i32| {
            if let Some(c) = m.get_mut(&k) {
                *c -= 1;
                if *c == 0 {
                    m.remove(&k);
                }
            }
        };
        if let Some((lo, hi)) = self.extents.remove(&(level, n)) {
            dec(&mut self.bottoms[l], lo);
            dec(&mut self.tops[l], hi);
        }
        if let Some((lo, hi)) = rows {
            self.extents.insert((level, n), (lo, hi));
            *self.bottoms[l].entry(lo).or_default() += 1;
            *self.tops[l].entry(hi).or_default() += 1;
        }
    }

    /// The grid rows level `level`'s occupied cells span (lo, hi exclusive): above `hi` and below
    /// `lo` its whole box is air (RAY_MARCHED_RENDERER.md §4.3 item 3, the level's own max height).
    /// `None`: nothing occupied.
    pub fn rows(&self, level: u32) -> Option<(i32, i32)> {
        let l = level as usize;
        Some((
            *self.bottoms[l].keys().next()?,
            *self.tops[l].keys().next_back()?,
        ))
    }

    /// The finest level with a box, and the coarsest.
    pub(crate) fn level_range(&self) -> Option<(u32, u32)> {
        let lo = self.levels.iter().position(Option::is_some)?;
        let hi = self.levels.iter().rposition(Option::is_some)?;
        Some((lo as u32, hi as u32))
    }

    /// The union of the levels' boxes in grid cells (lo inclusive, hi exclusive).
    pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let mut out: Option<([i32; 3], [i32; 3])> = None;
        for lv in self.levels.iter().flatten() {
            let s = 5 + lv.info.cell_log2;
            let lo = lv.info.origin.map(|o| o << s);
            let hi = std::array::from_fn(|k| (lv.info.origin[k] + lv.info.dims[k] as i32) << s);
            out = Some(match out {
                None => (lo, hi),
                Some((a, b)) => (
                    std::array::from_fn(|k| a[k].min(lo[k])),
                    std::array::from_fn(|k| b[k].max(hi[k])),
                ),
            });
        }
        out
    }

    pub fn level_info(&self, level: u32) -> Option<LevelInfo> {
        self.levels[level as usize].as_ref().map(|l| l.info)
    }

    pub fn live_nodes(&self) -> usize {
        self.node_ids.live()
    }

    /// The upload: every buffer of crate::layout, `slot_base` filled in level order.
    pub fn gpu(&self) -> GpuBuffers {
        let mut g = GpuBuffers {
            levels: self.level_words(),
            slots: self.slot_words(),
            ..GpuBuffers::default()
        };
        g.nodes = self.nodes.iter().flat_map(|n| n.to_words()).collect();
        g.node_columns = self.node_columns.clone();
        g.columns = self.columns.clone();
        g.pool = self.pool.words().to_vec();
        g.states = self.table.to_words();
        g
    }
}

/// A chunk's level-0 node in grid coordinates.
pub fn chunk_node(c: ChunkCoord) -> [i32; 3] {
    let l = lod::lod_of_chunk(c);
    [l.i, l.j, l.k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build_chunk;
    use dwell_world::CHUNK_VOLUME;

    #[test]
    fn changed_nodes_are_named_as_regions_and_boxes_cover_them_within_the_limit() {
        let t = StateTable::registry();
        let mut s = Scene::new(t);
        s.take_dirty();
        s.put(0, [10, 10, 10], &NodeBuild::Solid(1));
        s.put(2, [-1, 3, 0], &NodeBuild::Air);
        s.remove(0, [10, 10, 10]);
        let d = s.take_dirty();
        assert_eq!(d.regions, vec![(0, [10, 10, 10]), (2, [-1, 3, 0]), (0, [10, 10, 10])]);
        // A node at level l spans 2^(l+5) level-0 cells a side.
        assert_eq!(
            d.boxes(8),
            vec![([320; 3], [351; 3]), ([-128, 384, 0], [-1, 511, 127])]
        );
        // Over the limit: merged per level, then into one; every change stays covered.
        let mut many = Dirty::default();
        for i in 0..20 {
            many.regions.push((0, [i, 0, 0]));
        }
        many.regions.push((1, [5, 5, 5]));
        let b = many.boxes(4);
        assert_eq!(b, vec![([0; 3], [639, 31, 31]), ([320; 3], [383; 3])]);
        assert_eq!(many.boxes(1), vec![([0; 3], [639, 383, 383])]);
        assert!(s.take_dirty().regions.is_empty());
    }

    #[test]
    fn put_replace_and_remove_recycle_storage() {
        let t = StateTable::registry();
        let mut s = Scene::new(t.clone());
        let mut v = vec![0u16; CHUNK_VOLUME];
        v[0] = 1;
        v[5000] = 2;
        let b = build_chunk(&v, &t);
        s.put(0, [10, 10, 10], &b);
        let used = s.pool.capacity_words();
        assert!(matches!(s.slot(0, [10, 10, 10]), Slot::Node(0)));
        s.put(0, [10, 10, 10], &b);
        assert_eq!(
            s.pool.capacity_words(),
            used,
            "replacing reuses the freed bricks"
        );
        assert_eq!(s.live_nodes(), 1);
        s.put(0, [12, 10, 10], &NodeBuild::Solid(1));
        assert_eq!(
            s.slot(0, [11, 10, 10]),
            Slot::None,
            "the box grew; the gap has no data"
        );
        assert_eq!(s.slot(0, [12, 10, 10]), Slot::AllSolid(1));
        assert!(
            matches!(s.slot(0, [10, 10, 10]), Slot::Node(_)),
            "kept on growth"
        );
        s.remove(0, [10, 10, 10]);
        assert_eq!(s.slot(0, [10, 10, 10]), Slot::None);
        assert_eq!(s.live_nodes(), 0);
        assert_eq!(s.pool.live_words(), 0);
        let g = s.gpu();
        assert_eq!(g.levels.len(), LEVEL_COUNT * LEVEL_WORDS);
        assert_eq!(g.slots.len(), 3);
        assert_eq!(s.bounds(), Some(([320; 3], [416, 352, 352])));
        // Occupied rows: the solid node at y = 10 fills its node's rows.
        assert_eq!(s.rows(0), Some((320, 352)));
        s.remove(0, [12, 10, 10]);
        assert_eq!(s.rows(0), None);
    }

    #[test]
    fn dirty_ranges_replayed_onto_a_copy_reproduce_the_upload() {
        // The GPU mirror applies only what `take_dirty` names; applying it to a copy of the last
        // upload must give the full upload where any slot can reach, edit after edit.
        let t = StateTable::registry();
        let mut s = Scene::new(t.clone());
        let mut v = vec![0u16; CHUNK_VOLUME];
        v[7] = 1;
        v[9000] = 2;
        let b = build_chunk(&v, &t);
        let mut mirror = GpuBuffers::default();
        type Step<'a> = Box<dyn Fn(&mut Scene) + 'a>;
        let steps: Vec<Step> = vec![
            Box::new(|s| s.put(0, [10, 10, 10], &b)),
            Box::new(|s| s.put(0, [10, 10, 11], &b)),
            Box::new(|s| s.put(0, [10, 10, 10], &NodeBuild::Solid(1))),
            Box::new(|s| s.put(0, [10, 10, 10], &b)),
            Box::new(|s| s.remove(0, [10, 10, 11])),
            Box::new(|s| s.put(2, [3, 3, 3], &b)),
            Box::new(|s| s.put(0, [10, 10, 11], &b)),
        ];
        for step in steps {
            step(&mut s);
            let d = s.take_dirty();
            let full = s.gpu();
            // The levels' words (with their occupied rows) go up every frame in the uniforms.
            mirror.levels = full.levels.clone();
            if d.layout {
                mirror.slots = full.slots.clone();
            } else {
                for &i in &d.slots {
                    mirror.slots[i as usize] = s.slot_word(i);
                }
            }
            mirror.nodes.resize(full.nodes.len(), 0);
            mirror.node_columns.resize(full.node_columns.len(), 0);
            for &i in &d.nodes {
                let at = i as usize * NODE_WORDS;
                mirror.nodes[at..at + NODE_WORDS].copy_from_slice(&s.node_words(i));
                mirror.node_columns[i as usize] = s.node_columns()[i as usize];
            }
            mirror.columns.resize(full.columns.len(), 0);
            for &c in &d.columns {
                let c = c as usize;
                mirror.columns[c..c + COLUMN_WORDS]
                    .copy_from_slice(&s.column_words()[c..c + COLUMN_WORDS]);
            }
            mirror.pool.resize(full.pool.len(), 0);
            for &(at, n) in &d.pool {
                let (at, n) = (at as usize, n as usize);
                mirror.pool[at..at + n].copy_from_slice(&s.pool_words()[at..at + n]);
            }
            assert_eq!(mirror.levels, full.levels);
            assert_eq!(mirror.slots, full.slots);
            // Freed nodes and bricks keep stale words on the GPU; no slot reaches them.
            for &w in &full.slots {
                if let Slot::Node(n) = Slot::from_word(w) {
                    let at = n as usize * NODE_WORDS;
                    assert_eq!(
                        mirror.nodes[at..at + NODE_WORDS],
                        full.nodes[at..at + NODE_WORDS]
                    );
                    assert_eq!(
                        mirror.node_columns[n as usize],
                        full.node_columns[n as usize]
                    );
                    let c = full.node_columns[n as usize];
                    if c != COLUMNS_NONE {
                        let c = c as usize;
                        assert_eq!(
                            mirror.columns[c..c + COLUMN_WORDS],
                            full.columns[c..c + COLUMN_WORDS]
                        );
                    }
                    let node = Node::from_words(&full.nodes[at..at + NODE_WORDS]);
                    for b in 0..BRICKS_PER_NODE {
                        if let BrickRef::Mixed(o) = node.get(b) {
                            let o = o as usize;
                            assert_eq!(
                                mirror.pool[o..o + RAW_BRICK_WORDS],
                                full.pool[o..o + RAW_BRICK_WORDS]
                            );
                        }
                    }
                }
            }
        }
        assert!(s.take_dirty().is_empty());
    }
}
