//! The GPU data layout (RAY_MARCHED_RENDERER.md §4.1), as `u32` words. Every buffer the marcher
//! reads is a `array<u32>` (or a struct of `u32`s) in WGSL, so each type here has a `to_words` and
//! the shader decodes the same words with the same shifts and masks.
//!
//! # The grid frame
//!
//! All coordinates are integer **grid cells**: world metres offset by `−ORIGIN` of the LOD octree
//! (`dwell_world::lod::ORIGIN_*` = −2²³), so the whole world is `0..2²⁴` on every axis, a level-L
//! cell is `grid >> L` and a node (32³ cells) is `grid >> (L + 5)` — a level-L node coordinate is
//! the LOD section coordinate (i, j, k), and a level-0 node is `lod_of_chunk(chunk)`. Differences
//! of two grid coordinates are below 2²⁴, so they convert to `f32` exactly; that is what the
//! marcher's camera-relative arithmetic relies on.
//!
//! # Buffers
//!
//! ```wgsl
//! struct LevelInfo {            // 12 words (WGSL alignment: vec3 at 0 and 16)
//!   origin: vec3<i32>,          // first node of the level's box, node coordinates
//!   occ_hi: i32,                // top of the level's occupied cells, grid rows (exclusive)
//!   dims: vec3<u32>,            // nodes along x, y, z
//!   slot_base: u32,             // first slot of the level in `slots`
//!   cell_log2: u32,             // L: a cell is 2^L m
//!   occ_lo: i32,                // bottom of its occupied cells (none: MAX, MIN)
//! };
//! @binding(0) var<storage, read> levels: array<LevelInfo, 20>;
//! @binding(1) var<storage, read> slots: array<u32>;   // slot_base + x + dims.x * (y + dims.y * z)
//! struct Node { mixed: vec2<u32>, uniform: vec2<u32>, brick: array<u32, 64> };   // 68 words
//! @binding(2) var<storage, read> nodes: array<Node>;
//! @binding(3) var<storage, read> node_columns: array<u32>;  // per node: offset in `columns` or NONE
//! @binding(4) var<storage, read> columns: array<u32>;      // column min-max grids
//! @binding(5) var<storage, read> pool: array<u32>;         // bricks
//! @binding(6) var<storage, read> states: array<StateGpu>;  // crate::state
//! ```
//!
//! **Slot** ([`Slot`]): `0xFFFF_FFFF` NONE (no data at this level here: use a coarser one);
//! tag in bits 30–31 — `0` a node index (bits 0–29), `1` ALL_AIR, `2` ALL_SOLID with the state id
//! in bits 0–15.
//!
//! **Node** ([`Node`]): brick `b = bx + 4·by + 16·bz` (bricks of 8³ cells, 4×4×4 per node). Bit `b`
//! of `mixed` (word `b >> 5`, bit `b & 31`) marks a brick with a pool payload, of `uniform` one
//! wholly of one occupied state. `brick[b]` is then the payload's word offset in `pool` (mixed),
//! the state id (uniform), or — for an empty brick — the Chebyshev distance in bricks to the
//! nearest non-empty brick of the node (RAY_MARCHED_RENDERER.md §4.3 item 4; 1 = a neighbour is not
//! empty).
//!
//! **Brick** (in `pool`, at the offset): word 0 a header — bits 0–4 the index width (1, 2 or 4 for
//! a palette brick, 16 for raw states), bits 8–15 the palette length, bit 16 a shape plane follows.
//! Words 1–16: the 512-bit occupancy (voxel `v = x + 8·y + 64·z`, word `1 + (v >> 5)`, bit
//! `v & 31`). Then:
//! - raw (`width = 16`, level-0 bricks): 256 words of `u16` states, voxel `v` in word
//!   `17 + (v >> 1)`, low half for even `v` ([`RAW_BRICK_WORDS`]); with a shape plane, 128 more
//!   words of one `u8` per voxel (word `273 + (v >> 2)`, byte `v & 3`): the shape the cell is hit
//!   against (0 = its cube), resolved on the worker (adaptive shapes, Phase 18) —
//!   [`SHAPED_BRICK_WORDS`];
//! - palette (`width` 1/2/4, LOD bricks): `2^width / 2` words of `u16` palette entries (entry `i`
//!   in word `17 + (i >> 1)`), then `16 · width` words of indices (voxel `v`'s index at bit
//!   `v · width` of that run). Sizes: [`palette_brick_words`].
//!
//! **Columns** ([`ColumnGrid`]): per node, a min-max mip of the occupied rows of each column: 32²,
//! 16², 8², 4², 2², 1² regions, region `(rx, rz)` of mip `m` (2^m columns wide) at entry
//! `MIP_OFFSET[m] + rx + rz · (32 >> m)`; two `u16` entries per word (even entry low), each
//! `lo | hi << 8` in cells from the node's bottom (`hi` exclusive; an empty column is
//! `32 | 0 << 8`).

pub const NODE_CELLS: i32 = 32;
pub const BRICK_CELLS: i32 = 8;
pub const BRICKS_PER_AXIS: i32 = NODE_CELLS / BRICK_CELLS;
pub const BRICKS_PER_NODE: usize = 64;
pub const BRICK_VOXELS: usize = 512;
/// The LOD octree's root level: the coarsest level the marcher knows.
pub const MAX_LEVEL: u32 = dwell_world::lod::MAX_LEVEL as u32;
pub const LEVEL_COUNT: usize = MAX_LEVEL as usize + 1;

/// World metres → grid cells: add this (2²³).
pub const GRID_OFFSET: i64 = -dwell_world::lod::ORIGIN_X;
const _: () = assert!(dwell_world::lod::ORIGIN_X == dwell_world::lod::ORIGIN_Y);
const _: () = assert!(dwell_world::lod::ORIGIN_X == dwell_world::lod::ORIGIN_Z);

/// Brick index of brick coordinates (each 0..4).
pub const fn brick_index(bx: i32, by: i32, bz: i32) -> usize {
    (bx + BRICKS_PER_AXIS * (by + BRICKS_PER_AXIS * bz)) as usize
}
/// Voxel index within a brick (each 0..8).
pub const fn voxel_index(x: i32, y: i32, z: i32) -> usize {
    (x + BRICK_CELLS * (y + BRICK_CELLS * z)) as usize
}

// --- slots ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    None,
    AllAir,
    AllSolid(u16),
    Node(u32),
}

pub const SLOT_NONE: u32 = 0xFFFF_FFFF;
pub const TAG_SHIFT: u32 = 30;
pub const TAG_NODE: u32 = 0;
pub const TAG_AIR: u32 = 1;
pub const TAG_SOLID: u32 = 2;
/// Node indices fit below the tag.
pub const MAX_NODES: u32 = 1 << TAG_SHIFT;

impl Slot {
    pub fn to_word(self) -> u32 {
        match self {
            Slot::None => SLOT_NONE,
            Slot::AllAir => TAG_AIR << TAG_SHIFT,
            Slot::AllSolid(s) => (TAG_SOLID << TAG_SHIFT) | u32::from(s),
            Slot::Node(i) => {
                assert!(i < MAX_NODES, "node index {i} past the slot encoding");
                (TAG_NODE << TAG_SHIFT) | i
            }
        }
    }
    pub fn from_word(w: u32) -> Self {
        if w == SLOT_NONE {
            return Slot::None;
        }
        match w >> TAG_SHIFT {
            TAG_NODE => Slot::Node(w & (MAX_NODES - 1)),
            TAG_AIR => Slot::AllAir,
            _ => Slot::AllSolid((w & 0xFFFF) as u16),
        }
    }
}

// --- levels --------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelInfo {
    pub origin: [i32; 3],
    pub dims: [u32; 3],
    pub slot_base: u32,
    pub cell_log2: u32,
}

pub const LEVEL_WORDS: usize = 12;

impl LevelInfo {
    /// The WGSL struct's words (vec3 alignment: padding at words 10–11), with no occupied rows. A
    /// level with no data has zero dims.
    pub fn to_words(&self) -> [u32; LEVEL_WORDS] {
        self.to_words_with_rows((i32::MAX, i32::MIN))
    }
    /// The words with the level's occupied grid rows `(lo, hi)` (`Scene::rows`; `(i32::MAX,
    /// i32::MIN)` for none): `occ_hi` in word 3, `occ_lo` in word 9.
    pub fn to_words_with_rows(&self, (lo, hi): (i32, i32)) -> [u32; LEVEL_WORDS] {
        let o = self.origin.map(|v| v as u32);
        [
            o[0],
            o[1],
            o[2],
            hi as u32,
            self.dims[0],
            self.dims[1],
            self.dims[2],
            self.slot_base,
            self.cell_log2,
            lo as u32,
            0,
            0,
        ]
    }
    /// The slot index of node `n`, if inside the level's box.
    pub fn slot_of(&self, n: [i32; 3]) -> Option<usize> {
        let mut idx = [0usize; 3];
        for k in 0..3 {
            let d = i64::from(n[k]) - i64::from(self.origin[k]);
            if d < 0 || d >= i64::from(self.dims[k]) {
                return None;
            }
            idx[k] = d as usize;
        }
        let [dx, dy, _] = self.dims.map(|d| d as usize);
        Some(self.slot_base as usize + idx[0] + dx * (idx[1] + dy * idx[2]))
    }
}

// --- nodes ---------------------------------------------------------------------------------------

/// A 64-tree node: one chunk (level 0) or one LOD section (level L). 272 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub mixed: [u32; 2],
    pub uniform: [u32; 2],
    pub brick: [u32; BRICKS_PER_NODE],
}

pub const NODE_WORDS: usize = 4 + BRICKS_PER_NODE;

impl Default for Node {
    fn default() -> Self {
        Self {
            mixed: [0; 2],
            uniform: [0; 2],
            brick: [0; BRICKS_PER_NODE],
        }
    }
}

pub(crate) fn bit(mask: [u32; 2], b: usize) -> bool {
    mask[b >> 5] >> (b & 31) & 1 != 0
}
pub(crate) fn set_bit(mask: &mut [u32; 2], b: usize) {
    mask[b >> 5] |= 1 << (b & 31);
}

/// What a node says about one brick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrickRef {
    /// Nothing occupied; the Chebyshev distance (bricks) to the nearest non-empty brick.
    Empty(u32),
    Uniform(u16),
    /// Word offset of the payload in the pool.
    Mixed(u32),
}

impl Node {
    pub fn get(&self, b: usize) -> BrickRef {
        if bit(self.mixed, b) {
            BrickRef::Mixed(self.brick[b])
        } else if bit(self.uniform, b) {
            BrickRef::Uniform(self.brick[b] as u16)
        } else {
            BrickRef::Empty(self.brick[b])
        }
    }
    pub fn set(&mut self, b: usize, r: BrickRef) {
        let clear = !(1u32 << (b & 31));
        self.mixed[b >> 5] &= clear;
        self.uniform[b >> 5] &= clear;
        self.brick[b] = match r {
            BrickRef::Empty(d) => d,
            BrickRef::Uniform(s) => {
                set_bit(&mut self.uniform, b);
                u32::from(s)
            }
            BrickRef::Mixed(o) => {
                set_bit(&mut self.mixed, b);
                o
            }
        };
    }
    pub fn to_words(&self) -> [u32; NODE_WORDS] {
        let mut w = [0u32; NODE_WORDS];
        w[0..2].copy_from_slice(&self.mixed);
        w[2..4].copy_from_slice(&self.uniform);
        w[4..].copy_from_slice(&self.brick);
        w
    }
    pub fn from_words(w: &[u32]) -> Self {
        let mut n = Self::default();
        n.mixed.copy_from_slice(&w[0..2]);
        n.uniform.copy_from_slice(&w[2..4]);
        n.brick.copy_from_slice(&w[4..NODE_WORDS]);
        n
    }
}

// --- bricks --------------------------------------------------------------------------------------

pub const OCC_WORDS: usize = BRICK_VOXELS / 32;
/// Header + occupancy + 512 `u16` states.
pub const RAW_BRICK_WORDS: usize = 1 + OCC_WORDS + BRICK_VOXELS / 2;
/// A raw brick and its shape plane (512 `u8`).
pub const SHAPED_BRICK_WORDS: usize = RAW_BRICK_WORDS + BRICK_VOXELS / 4;
pub const RAW_WIDTH: u32 = 16;
pub const HEADER_SHAPE_PLANE: u32 = 1 << 16;

/// Words of a palette brick of index width 1, 2 or 4 (palette capacity 2^width).
pub const fn palette_brick_words(width: u32) -> usize {
    1 + OCC_WORDS + (1usize << width).div_ceil(2) + 16 * width as usize
}

/// Decoded header word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrickHeader {
    pub width: u32,
    pub palette_len: u32,
    pub shape_plane: bool,
}

impl BrickHeader {
    pub fn to_word(self) -> u32 {
        self.width
            | (self.palette_len << 8)
            | if self.shape_plane {
                HEADER_SHAPE_PLANE
            } else {
                0
            }
    }
    pub fn from_word(w: u32) -> Self {
        Self {
            width: w & 0x1F,
            palette_len: (w >> 8) & 0xFF,
            shape_plane: w & HEADER_SHAPE_PLANE != 0,
        }
    }
    /// The brick's size in words.
    pub fn words(self) -> usize {
        if self.width == RAW_WIDTH {
            if self.shape_plane {
                SHAPED_BRICK_WORDS
            } else {
                RAW_BRICK_WORDS
            }
        } else {
            palette_brick_words(self.width)
        }
    }
}

/// Whether voxel `v` of the brick at `b` is occupied.
pub fn brick_occupied(b: &[u32], v: usize) -> bool {
    b[1 + (v >> 5)] >> (v & 31) & 1 != 0
}

/// The state of voxel `v` of the brick at `b` (raw or palette).
pub fn brick_state(b: &[u32], v: usize) -> u16 {
    let h = BrickHeader::from_word(b[0]);
    let base = 1 + OCC_WORDS;
    if h.width == RAW_WIDTH {
        return (b[base + (v >> 1)] >> ((v & 1) * 16)) as u16;
    }
    let palette_words = (1usize << h.width).div_ceil(2);
    let w = h.width as usize;
    let bitpos = v * w;
    let i = (b[base + palette_words + (bitpos >> 5)] >> (bitpos & 31)) & ((1 << w) - 1);
    let i = i as usize;
    (b[base + (i >> 1)] >> ((i & 1) * 16)) as u16
}

/// The shape plane entry of voxel `v` (0 without a plane).
pub fn brick_shape(b: &[u32], v: usize) -> u8 {
    let h = BrickHeader::from_word(b[0]);
    if !h.shape_plane {
        return 0;
    }
    (b[RAW_BRICK_WORDS + (v >> 2)] >> ((v & 3) * 8)) as u8
}

// --- columns -------------------------------------------------------------------------------------

/// Mip levels of a column grid: 2^m columns per region, m = 0..=5.
pub const COLUMN_MIPS: usize = 6;
pub const MIP_OFFSET: [usize; COLUMN_MIPS] = [0, 1024, 1280, 1344, 1360, 1364];
pub const COLUMN_ENTRIES: usize = 1365;
pub const COLUMN_WORDS: usize = COLUMN_ENTRIES.div_ceil(2);
/// An empty column or region: lo above hi, so every ray passes it.
pub const EMPTY_SPAN: (u8, u8) = (32, 0);

/// Occupied rows per column, with its min-max mip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnGrid {
    /// `COLUMN_ENTRIES` (lo, hi) spans.
    pub spans: Vec<(u8, u8)>,
}

impl ColumnGrid {
    /// From the 32 × 32 finest spans (index x + 32 z): builds the mip.
    pub fn from_columns(cols: &[(u8, u8)]) -> Self {
        assert_eq!(cols.len(), 1024);
        let mut spans = vec![EMPTY_SPAN; COLUMN_ENTRIES];
        spans[..1024].copy_from_slice(cols);
        for m in 1..COLUMN_MIPS {
            let n = 32usize >> m;
            let prev = 32usize >> (m - 1);
            for rz in 0..n {
                for rx in 0..n {
                    let mut s = EMPTY_SPAN;
                    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let c = spans[MIP_OFFSET[m - 1] + (2 * rx + dx) + prev * (2 * rz + dz)];
                        s = (s.0.min(c.0), s.1.max(c.1));
                    }
                    spans[MIP_OFFSET[m] + rx + n * rz] = s;
                }
            }
        }
        Self { spans }
    }
    /// The span of region (rx, rz) of mip m.
    pub fn span(&self, m: usize, rx: usize, rz: usize) -> (u8, u8) {
        self.spans[MIP_OFFSET[m] + rx + (32 >> m) * rz]
    }
    pub fn to_words(&self) -> Vec<u32> {
        let mut w = vec![0u32; COLUMN_WORDS];
        for (i, &(lo, hi)) in self.spans.iter().enumerate() {
            let e = u32::from(lo) | (u32::from(hi) << 8);
            w[i >> 1] |= e << ((i & 1) * 16);
        }
        w
    }
}

impl ColumnGrid {
    /// From `to_words`'s words (`None` if too short).
    pub fn from_words(w: &[u32]) -> Option<Self> {
        if w.len() < COLUMN_WORDS {
            return None;
        }
        let spans = (0..COLUMN_ENTRIES)
            .map(|i| {
                let e = w[i >> 1] >> ((i & 1) * 16);
                ((e & 0xFF) as u8, ((e >> 8) & 0xFF) as u8)
            })
            .collect();
        Some(Self { spans })
    }
}

/// The span of region (rx, rz) of mip m read from a grid's words.
pub fn column_span(words: &[u32], m: usize, rx: usize, rz: usize) -> (u8, u8) {
    let i = MIP_OFFSET[m] + rx + (32 >> m) * rz;
    let e = words[i >> 1] >> ((i & 1) * 16);
    ((e & 0xFF) as u8, ((e >> 8) & 0xFF) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_round_trip() {
        for s in [
            Slot::None,
            Slot::AllAir,
            Slot::AllSolid(0),
            Slot::AllSolid(65535),
            Slot::Node(0),
            Slot::Node(MAX_NODES - 1),
        ] {
            assert_eq!(Slot::from_word(s.to_word()), s);
        }
    }

    #[test]
    fn mip_offsets_follow_the_region_counts() {
        let mut at = 0;
        for (m, &off) in MIP_OFFSET.iter().enumerate() {
            assert_eq!(off, at);
            at += (32 >> m) * (32 >> m);
        }
        assert_eq!(at, COLUMN_ENTRIES);
    }

    #[test]
    fn node_words_round_trip() {
        let mut n = Node::default();
        n.set(0, BrickRef::Mixed(1234));
        n.set(33, BrickRef::Uniform(7));
        n.set(63, BrickRef::Empty(3));
        assert_eq!(Node::from_words(&n.to_words()), n);
        assert_eq!(n.get(0), BrickRef::Mixed(1234));
        assert_eq!(n.get(33), BrickRef::Uniform(7));
        assert_eq!(n.get(63), BrickRef::Empty(3));
        n.set(33, BrickRef::Empty(1));
        assert_eq!(n.get(33), BrickRef::Empty(1));
        assert_eq!(n.uniform, [0, 0]);
    }

    #[test]
    fn sizes_match_the_proposal() {
        assert_eq!(NODE_WORDS * 4, 272);
        assert_eq!(palette_brick_words(1), 34);
        assert_eq!(palette_brick_words(2), 51);
        assert_eq!(palette_brick_words(4), 89);
        assert_eq!(RAW_BRICK_WORDS, 273);
        assert_eq!(SHAPED_BRICK_WORDS, 401);
    }
}
