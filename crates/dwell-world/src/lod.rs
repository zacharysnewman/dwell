//! Level of detail: the whole-world view (ARCHITECTURE.md §6.6, ADR 0012). A 3D octree of sections:
//! at level L a cell is a 2^L m cube and a section 32³ cells, so level 0 is the chunk grid.
//! Section coordinates count from (−2²³, −2²³, −2²³); one root at `MAX_LEVEL` holds the whole disc.
//! Section content is 34³ cells (the section and a one-cell apron) in layer order (x fastest, then
//! z, then y): the codec's order and the client mesher's padded layout.

use dwell_blocks::{StateId, info};
use dwell_protocol::lod;

use crate::voxel::*;

pub const SECTION_CELLS: i32 = lod::SECTION_CELLS as i32;
pub const MAX_LEVEL: i32 = lod::MAX_LEVEL as i32;
pub const INDEX_LEVEL: i32 = lod::INDEX_LEVEL as i32;
pub const PAD: i32 = SECTION_CELLS + 2;
pub const VOLUME: usize = (PAD * PAD * PAD) as usize;
pub const ORIGIN_X: i64 = -(1 << 23);
pub const ORIGIN_Y: i64 = -(1 << 23);
pub const ORIGIN_Z: i64 = -(1 << 23);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LodCoord {
    pub level: i32,
    pub i: i32,
    pub j: i32,
    pub k: i32,
}

impl LodCoord {
    pub const fn new(level: i32, i: i32, j: i32, k: i32) -> Self {
        Self { level, i, j, k }
    }
}

/// Index of cell (x, y, z) of a section's content, each in −1..32 (−1 and 32: the apron).
pub const fn cell_index(x: i32, y: i32, z: i32) -> usize {
    ((x + 1) + PAD * ((z + 1) + PAD * (y + 1))) as usize
}
/// Index of column (x, z) of a section's padded columns.
pub const fn column_index(x: i32, z: i32) -> usize {
    ((z + 1) * PAD + x + 1) as usize
}

pub const fn cell_size(level: i32) -> i64 {
    1 << level
}
pub const fn section_size(level: i32) -> i64 {
    (SECTION_CELLS as i64) << level
}
/// Sections of a level along x and z (the root level has one).
pub const fn sections_across(level: i32) -> i64 {
    1 << (MAX_LEVEL - level)
}
pub const fn floor_div64(a: i64, b: i64) -> i64 {
    a / b
        - if a % b != 0 && ((a < 0) != (b < 0)) {
            1
        } else {
            0
        }
}
/// The first and last section rows of a level that hold part of the world's height.
pub const fn first_row(level: i32) -> i32 {
    floor_div64(WORLD_BOTTOM_Y as i64 - ORIGIN_Y, section_size(level)) as i32
}
pub const fn last_row(level: i32) -> i32 {
    floor_div64(WORLD_MAX_Y as i64 - 1 - ORIGIN_Y, section_size(level)) as i32
}

/// World metres of a section's minimum corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodOrigin {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

pub fn section_origin(c: LodCoord) -> LodOrigin {
    let s = section_size(c.level);
    LodOrigin {
        x: ORIGIN_X + i64::from(c.i) * s,
        y: ORIGIN_Y + i64::from(c.j) * s,
        z: ORIGIN_Z + i64::from(c.k) * s,
    }
}

/// Octant bits: 1 = +x half, 2 = +y half, 4 = +z half.
pub fn child(c: LodCoord, octant: i32) -> LodCoord {
    LodCoord::new(
        c.level - 1,
        c.i * 2 + (octant & 1),
        c.j * 2 + ((octant >> 1) & 1),
        c.k * 2 + ((octant >> 2) & 1),
    )
}
pub fn parent(c: LodCoord) -> LodCoord {
    LodCoord::new(c.level + 1, c.i >> 1, c.j >> 1, c.k >> 1)
}
pub fn ancestor(c: LodCoord, level: i32) -> LodCoord {
    let d = level - c.level;
    LodCoord::new(level, c.i >> d, c.j >> d, c.k >> d)
}

const CHUNK_OFFSET_XZ: i32 = (-ORIGIN_X / CHUNK_SIZE as i64) as i32;
const CHUNK_OFFSET_Y: i32 = (-ORIGIN_Y / CHUNK_SIZE as i64) as i32;
const _: () = assert!(CHUNK_OFFSET_Y == 1 << 18);

/// The level-0 section that is this chunk.
pub fn lod_of_chunk(c: ChunkCoord) -> LodCoord {
    LodCoord::new(
        0,
        c.x + CHUNK_OFFSET_XZ,
        c.y + CHUNK_OFFSET_Y,
        c.z + CHUNK_OFFSET_XZ,
    )
}
/// Level 0 only.
pub fn chunk_of_lod(c: LodCoord) -> ChunkCoord {
    ChunkCoord::new(
        c.i - CHUNK_OFFSET_XZ,
        c.j - CHUNK_OFFSET_Y,
        c.k - CHUNK_OFFSET_XZ,
    )
}

/// Inside the octree, within the world's rows, and overlapping the world's disc.
pub fn in_world(c: LodCoord) -> bool {
    if c.level < 0 || c.level > MAX_LEVEL {
        return false;
    }
    let across = sections_across(c.level);
    if c.i < 0 || c.k < 0 || i64::from(c.i) >= across || i64::from(c.k) >= across {
        return false;
    }
    if c.j < first_row(c.level) || c.j > last_row(c.level) {
        return false;
    }
    let o = section_origin(c);
    let s = section_size(c.level);
    let nearest = |lo: i64, hi: i64| {
        if lo > 0 {
            lo
        } else if hi - 1 < 0 {
            hi - 1
        } else {
            0
        }
    };
    inside_world_disc(nearest(o.x, o.x + s), nearest(o.z, o.z + s))
}

/// `VOLUME` cells in `cell_index` order.
pub type LodCells = Vec<StateId>;

/// The surface of a column of a generated section, at full vertical precision (a cell far away is
/// kilometres tall, so the client draws the top at `height` instead of the top cell's top).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LodSurface {
    /// The column's topmost filled cell in the section holds its surface.
    pub valid: bool,
    /// Under water: `height` is the floor, `material` the floor's.
    pub wet: bool,
    pub height: f32,
    pub material: StateId,
    /// Where wet: the water's surface (the sea's).
    pub water: f32,
}

/// `PAD²` columns, `column_index` order.
pub type LodSurfaces = Vec<LodSurface>;

/// All air (nothing to draw), buried (solid with no exposed face) or content to mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodKind {
    Empty = 0,
    Buried = 1,
    Content = 2,
}

/// Not air, not liquid.
pub fn lod_solid(m: StateId) -> bool {
    m != M.air && !info(m).liquid
}

/// A 2×2×2 block → one cell of the next level (§6.6): filled if at least 4 of 8 are not air (liquid
/// counts, so seas keep their surface), else air. The material is the most common among the top
/// filled cell of each of the four columns, ties to the upper cells. Index: dx | dy << 1 | dz << 2.
pub fn downsample_block(cells: &[StateId; 8]) -> StateId {
    let air = M.air;
    let filled = cells.iter().filter(|&&m| m != air).count();
    if filled < 4 {
        return air;
    }
    let mut candidates = [0 as StateId; 4];
    let mut n = 0;
    for pass in 0..2 {
        for col in 0..4 {
            let (dx, dz) = (col & 1, col >> 1);
            let top = cells[dx | 2 | (dz << 2)];
            let bottom = cells[dx | (dz << 2)];
            let from_upper = top != air;
            if from_upper != (pass == 0) {
                continue;
            }
            if !from_upper && bottom == air {
                continue;
            }
            candidates[n] = if from_upper { top } else { bottom };
            n += 1;
        }
    }
    let mut best = candidates[0];
    let mut best_count = 0;
    for a in 0..n {
        let count = candidates[..n]
            .iter()
            .filter(|&&c| c == candidates[a])
            .count();
        if count > best_count {
            best = candidates[a];
            best_count = count;
        }
    }
    best
}

/// On face B the surface seen is the lowest filled cell, so the block is read upside down.
pub fn downsample_block_face(cells: &[StateId; 8], face: Face) -> StateId {
    match face {
        Face::A => downsample_block(cells),
        Face::B => downsample_block(&std::array::from_fn(|b| cells[b ^ 2])),
    }
}

/// The face of cell row r (−1..32) of a section: its centre's side of the midplane.
pub fn row_face(c: LodCoord, row: i32) -> Face {
    let cell = cell_size(c.level);
    let centre2 =
        2 * (ORIGIN_Y + i64::from(c.j) * section_size(c.level) + i64::from(row) * cell) + cell;
    if centre2 >= 2 * i64::from(MIDPLANE_Y) {
        Face::A
    } else {
        Face::B
    }
}

/// The face-local height of the bottom voxel of a face-B row's cell: the mirror of the cell's top.
pub const fn mirror_row_bottom(bottom: i64, cell: i64) -> i64 {
    MIRROR_SUM as i64 + 1 - bottom - cell
}

/// Downsamples a child's 32³ interior (`child_at(x, y, z)`, each 0..31) into the octant of the
/// parent section it covers; each parent cell takes the face of its centre.
pub fn downsample_octant(
    child_at: impl Fn(i32, i32, i32) -> StateId,
    parent_coord: LodCoord,
    octant: i32,
    parent: &mut LodCells,
) {
    let h = SECTION_CELLS / 2;
    let (ox, oy, oz) = (
        (octant & 1) * h,
        ((octant >> 1) & 1) * h,
        ((octant >> 2) & 1) * h,
    );
    for y in 0..h {
        let face = row_face(parent_coord, oy + y);
        for z in 0..h {
            for x in 0..h {
                let block = std::array::from_fn(|b| {
                    let b = b as i32;
                    child_at(
                        2 * x + (b & 1),
                        2 * y + ((b >> 1) & 1),
                        2 * z + ((b >> 2) & 1),
                    )
                });
                parent[cell_index(ox + x, oy + y, oz + z)] = downsample_block_face(&block, face);
            }
        }
    }
}

pub fn downsample_chunk_octant(
    chunk: &Chunk,
    parent_coord: LodCoord,
    octant: i32,
    parent: &mut LodCells,
) {
    let v = chunk.voxels();
    downsample_octant(
        |x, y, z| v[local_index(x, y, z)],
        parent_coord,
        octant,
        parent,
    );
}

pub fn downsample_section_octant(
    child: &LodCells,
    parent_coord: LodCoord,
    octant: i32,
    parent: &mut LodCells,
) {
    downsample_octant(
        |x, y, z| child[cell_index(x, y, z)],
        parent_coord,
        octant,
        parent,
    );
}

/// FNV-1a 64 over the kind and the cells (u16 little-endian): the golden test.
pub fn lod_hash(kind: LodKind, cells: &[StateId]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut byte = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    byte(kind as u8);
    for &m in cells {
        byte(m as u8);
        byte((m >> 8) as u8);
    }
    h
}

/// Height bounds of a column of sections (L, i, ·, k) from the generator's columns: cells sampling
/// above `hi` are air (or sea), below `lo` solid. Face B's are face-local.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodBounds {
    pub lo: f64,
    pub hi: f64,
    pub lo_b: f64,
    pub hi_b: f64,
    /// Some column of the section (with its apron) is inside the disc.
    pub any_inside: bool,
    /// False for the flat test world: one terrain from lo to hi in world terms.
    pub bifacial: bool,
}

impl Default for LodBounds {
    fn default() -> Self {
        Self {
            lo: 0.0,
            hi: 0.0,
            lo_b: 0.0,
            hi_b: 0.0,
            any_inside: true,
            bifacial: true,
        }
    }
}

/// One face's part of a section, face-local: rows starting at `origin_y`, judged against (lo, hi).
pub fn kind_of_face(level: i32, origin_y: i64, lo: f64, hi: f64) -> LodKind {
    let cell = cell_size(level);
    let lowest = (origin_y - cell) as f64;
    let highest = (origin_y + i64::from(SECTION_CELLS) * cell) as f64;
    if lowest > hi {
        LodKind::Empty
    } else if highest < lo {
        LodKind::Buried
    } else {
        LodKind::Content
    }
}

/// The lowest row of face A in a section (33: none).
pub fn first_face_a_row(c: LodCoord) -> i32 {
    (-1..=SECTION_CELLS)
        .find(|&r| row_face(c, r) == Face::A)
        .unwrap_or(SECTION_CELLS + 1)
}

/// Empty or Buried when the bounds decide the section (with its apron), else Content. A section
/// straddling the midplane is empty or buried only when both parts are.
pub fn kind_from_bounds(c: LodCoord, b: &LodBounds) -> LodKind {
    if !in_world(c) || !b.any_inside {
        return LodKind::Empty;
    }
    if !b.bifacial {
        return kind_of_face(c.level, section_origin(c).y, b.lo, b.hi);
    }
    let cell = cell_size(c.level);
    let first_a = first_face_a_row(c);
    let y0 = section_origin(c).y;
    let judge = |lowest: f64, highest: f64, lo: f64, hi: f64| {
        if lowest > hi {
            LodKind::Empty
        } else if highest < lo {
            LodKind::Buried
        } else {
            LodKind::Content
        }
    };
    let mut kinds = Vec::with_capacity(2);
    if first_a <= SECTION_CELLS {
        let lowest = (y0 + i64::from(first_a) * cell) as f64;
        let highest = (y0 + i64::from(SECTION_CELLS) * cell) as f64;
        kinds.push(judge(lowest, highest, b.lo, b.hi));
    }
    if first_a >= 0 {
        let top_row = (first_a - 1).min(SECTION_CELLS);
        let lowest = mirror_row_bottom(y0 + i64::from(top_row) * cell, cell) as f64;
        let highest = mirror_row_bottom(y0 - cell, cell) as f64;
        kinds.push(judge(lowest, highest, b.lo_b, b.hi_b));
    }
    if kinds.iter().all(|&k| k == LodKind::Empty) {
        LodKind::Empty
    } else if kinds.iter().all(|&k| k == LodKind::Buried) {
        LodKind::Buried
    } else {
        LodKind::Content
    }
}

/// Generates a section's cells (resized to `VOLUME`): a pure function of the coordinate.
pub type LodGenerator = std::sync::Arc<dyn Fn(LodCoord, &mut LodCells) -> LodKind + Send + Sync>;
pub type LodBoundsFn = std::sync::Arc<dyn Fn(i32, i32, i32) -> LodBounds + Send + Sync>;

pub fn flat_lod_bounds(level: i32, i: i32, k: i32) -> LodBounds {
    let c = LodCoord::new(level, i, first_row(level), k);
    let o = section_origin(c);
    let cell = cell_size(level);
    let s = section_size(level);
    let mut all_inside = true;
    for x in [o.x - cell, o.x + s + cell] {
        for z in [o.z - cell, o.z + s + cell] {
            all_inside &= inside_world_disc(x, z);
        }
    }
    LodBounds {
        hi: -1.0,
        lo: if all_inside { 0.0 } else { f64::NEG_INFINITY },
        bifacial: false,
        any_inside: in_world(c),
        ..LodBounds::default()
    }
}

/// The flat test world's sections.
pub fn generate_flat_lod(c: LodCoord, cells: &mut LodCells) -> LodKind {
    generate_flat_lod_of(&FlatLayers::standard(), c, cells)
}

/// The sections of a flat world of `layers` (the voxels' [`generate_flat_chunk_of`]).
pub fn generate_flat_lod_of(layers: &FlatLayers, c: LodCoord, cells: &mut LodCells) -> LodKind {
    cells.clear();
    cells.resize(VOLUME, M.air);
    let kind = kind_from_bounds(c, &flat_lod_bounds(c.level, c.i, c.k));
    match kind {
        LodKind::Empty => return kind,
        LodKind::Buried => {
            cells.fill(layers.base);
            return kind;
        }
        LodKind::Content => {}
    }
    let o = section_origin(c);
    let cell = cell_size(c.level);
    for z in -1..=SECTION_CELLS {
        for x in -1..=SECTION_CELLS {
            if !inside_world_disc(
                o.x + i64::from(x) * cell + cell / 2,
                o.z + i64::from(z) * cell + cell / 2,
            ) {
                continue;
            }
            for y in -1..=SECTION_CELLS {
                let a = o.y + i64::from(y) * cell;
                let m = if a < i64::from(MIDPLANE_Y) {
                    layers.base // below the world: the apron reads solid, so the floor is not drawn
                } else if a < 0 {
                    layers.at_depth(-cell - a)
                } else {
                    M.air
                };
                cells[cell_index(x, y, z)] = m;
            }
        }
    }
    kind
}
