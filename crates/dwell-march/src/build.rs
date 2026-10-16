//! Builders (RAY_MARCHED_RENDERER.md §4.1): a chunk's voxels or a LOD section's cells → a node, its
//! bricks and, for sections, its column min-max grid. Pure and deterministic, so they run in the
//! workers that mesh today and are tested without a GPU. A build is position-free: the scene places
//! it.

use dwell_blocks::StateId;
use dwell_world::{CHUNK_VOLUME, local_index, lod};

use crate::layout::*;
use crate::state::{MarchClass, StateTable};

/// How mixed bricks are packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// `u16` per voxel, fixed size: level-0 bricks, edited in place.
    Raw,
    /// 1/2/4-bit indices into a palette (raw past 16 states): LOD bricks.
    Palette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildOptions {
    pub packing: Packing,
    /// Build the column min-max grid (§4.3 item 3).
    pub columns: bool,
}

impl BuildOptions {
    /// Chunks carry column grids too: the proposal had them for LOD sections only, but on
    /// generated terrain they cut horizon rays' p99 from 455 to 83 steps for 2.7 KiB a node
    /// (`examples/march_steps.rs`).
    pub const CHUNK: Self = Self {
        packing: Packing::Raw,
        columns: true,
    };
    pub const SECTION: Self = Self {
        packing: Packing::Palette,
        columns: true,
    };
}

/// A built node before upload: `node.brick[b]` of a mixed brick indexes `bricks`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeData {
    pub node: Node,
    pub bricks: Vec<Vec<u32>>,
    pub columns: Option<ColumnGrid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeBuild {
    /// Nothing occupied (a slot of ALL_AIR).
    Air,
    /// Every cell the one occupied state (ALL_SOLID).
    Solid(StateId),
    Node(Box<NodeData>),
}

impl NodeBuild {
    /// GPU bytes this build takes once uploaded: node, bricks and column grid (slots aside).
    pub fn gpu_bytes(&self) -> usize {
        match self {
            NodeBuild::Node(d) => {
                4 * (NODE_WORDS
                    + d.bricks.iter().map(Vec::len).sum::<usize>()
                    + d.columns.as_ref().map_or(0, |_| COLUMN_WORDS))
            }
            _ => 0,
        }
    }
}

/// A chunk's voxels (`local_index` order).
pub fn build_chunk(voxels: &[StateId], table: &StateTable) -> NodeBuild {
    assert_eq!(voxels.len(), CHUNK_VOLUME);
    build_node(
        |x, y, z| voxels[local_index(x, y, z)],
        table,
        BuildOptions::CHUNK,
    )
}

/// A LOD section's cells (34³ with the apron, `lod::cell_index` order; the apron is not read).
pub fn build_section(cells: &[StateId], table: &StateTable) -> NodeBuild {
    assert_eq!(cells.len(), lod::VOLUME);
    build_node(
        |x, y, z| cells[lod::cell_index(x, y, z)],
        table,
        BuildOptions::SECTION,
    )
}

/// Any 32³ source: `at(x, y, z)`, each 0..32.
pub fn build_node(
    at: impl Fn(i32, i32, i32) -> StateId,
    table: &StateTable,
    opts: BuildOptions,
) -> NodeBuild {
    let mut node = Node::default();
    let mut bricks = Vec::new();
    let mut empty = [false; BRICKS_PER_NODE];
    let mut uniform_state: Option<Option<StateId>> = None;
    let mut states = [0 as StateId; BRICK_VOXELS];
    for bz in 0..BRICKS_PER_AXIS {
        for by in 0..BRICKS_PER_AXIS {
            for bx in 0..BRICKS_PER_AXIS {
                let b = brick_index(bx, by, bz);
                let mut occ = [0u32; OCC_WORDS];
                for z in 0..BRICK_CELLS {
                    for y in 0..BRICK_CELLS {
                        for x in 0..BRICK_CELLS {
                            let v = voxel_index(x, y, z);
                            let s = at(
                                bx * BRICK_CELLS + x,
                                by * BRICK_CELLS + y,
                                bz * BRICK_CELLS + z,
                            );
                            states[v] = s;
                            if table.occupied(s) {
                                occ[v >> 5] |= 1 << (v & 31);
                            }
                        }
                    }
                }
                let count: u32 = occ.iter().map(|w| w.count_ones()).sum();
                let r = if count == 0 {
                    empty[b] = true;
                    BrickRef::Empty(0)
                } else if count as usize == BRICK_VOXELS && states.iter().all(|&s| s == states[0]) {
                    BrickRef::Uniform(states[0])
                } else {
                    bricks.push(pack_brick(&states, &occ, table, opts.packing));
                    BrickRef::Mixed(bricks.len() as u32 - 1)
                };
                uniform_state = Some(match (uniform_state, r) {
                    (None, BrickRef::Uniform(s)) => Some(s),
                    (Some(Some(u)), BrickRef::Uniform(s)) if u == s => Some(s),
                    _ => None,
                });
                node.set(b, r);
            }
        }
    }
    if empty.iter().all(|&e| e) {
        return NodeBuild::Air;
    }
    if let Some(Some(s)) = uniform_state {
        return NodeBuild::Solid(s);
    }
    for (b, d) in brick_distances(&empty).into_iter().enumerate() {
        if empty[b] {
            node.set(b, BrickRef::Empty(d));
        }
    }
    let columns = opts.columns.then(|| column_grid(&at, table));
    NodeBuild::Node(Box::new(NodeData {
        node,
        bricks,
        columns,
    }))
}

/// Per empty brick, the Chebyshev distance (bricks) to the nearest non-empty brick of the node:
/// every brick nearer than that is empty. Non-empty bricks get 0. A node with no non-empty brick
/// gets 4 everywhere (it is ALL_AIR anyway).
pub fn brick_distances(empty: &[bool; BRICKS_PER_NODE]) -> [u32; BRICKS_PER_NODE] {
    let coord = |b: usize| [(b % 4) as i32, (b / 4 % 4) as i32, (b / 16) as i32];
    std::array::from_fn(|b| {
        if !empty[b] {
            return 0;
        }
        let p = coord(b);
        (0..BRICKS_PER_NODE)
            .filter(|&o| !empty[o])
            .map(|o| {
                let q = coord(o);
                (0..3)
                    .map(|k| (p[k] - q[k]).unsigned_abs())
                    .max()
                    .unwrap_or(0)
            })
            .min()
            .unwrap_or(4)
    })
}

fn pack_brick(
    states: &[StateId; BRICK_VOXELS],
    occ: &[u32; OCC_WORDS],
    table: &StateTable,
    packing: Packing,
) -> Vec<u32> {
    if packing == Packing::Palette {
        let mut palette: Vec<StateId> = Vec::new();
        for &s in states {
            if !palette.contains(&s) {
                palette.push(s);
                if palette.len() > 16 {
                    break;
                }
            }
        }
        let width = match palette.len() {
            0..=2 => 1u32,
            3..=4 => 2,
            5..=16 => 4,
            _ => 0,
        };
        if width != 0 {
            let mut w = vec![0u32; palette_brick_words(width)];
            w[0] = BrickHeader {
                width,
                palette_len: palette.len() as u32,
                shape_plane: false,
            }
            .to_word();
            w[1..1 + OCC_WORDS].copy_from_slice(occ);
            let base = 1 + OCC_WORDS;
            for (i, &s) in palette.iter().enumerate() {
                w[base + (i >> 1)] |= u32::from(s) << ((i & 1) * 16);
            }
            let idx = base + (1usize << width).div_ceil(2);
            for (v, s) in states.iter().enumerate() {
                let i = palette.iter().position(|p| p == s).unwrap_or(0) as u32;
                let bitpos = v * width as usize;
                w[idx + (bitpos >> 5)] |= i << (bitpos & 31);
            }
            return w;
        }
    }
    let shaped = states.iter().any(|&s| table.class(s) == MarchClass::Shaped);
    let header = BrickHeader {
        width: RAW_WIDTH,
        palette_len: 0,
        shape_plane: shaped,
    };
    let mut w = vec![0u32; header.words()];
    w[0] = header.to_word();
    w[1..1 + OCC_WORDS].copy_from_slice(occ);
    for (v, &s) in states.iter().enumerate() {
        w[1 + OCC_WORDS + (v >> 1)] |= u32::from(s) << ((v & 1) * 16);
    }
    if shaped {
        for (v, &s) in states.iter().enumerate() {
            if table.class(s) == MarchClass::Shaped {
                let shape = table.shape_index(s).min(255) as u32;
                w[RAW_BRICK_WORDS + (v >> 2)] |= shape << ((v & 3) * 8);
            }
        }
    }
    w
}

fn column_grid(at: &impl Fn(i32, i32, i32) -> StateId, table: &StateTable) -> ColumnGrid {
    let mut cols = vec![EMPTY_SPAN; 1024];
    for z in 0..NODE_CELLS {
        for x in 0..NODE_CELLS {
            let occupied = |y: i32| table.occupied(at(x, y, z));
            let lo = (0..NODE_CELLS).find(|&y| occupied(y));
            let hi = (0..NODE_CELLS).rev().find(|&y| occupied(y));
            if let (Some(lo), Some(hi)) = (lo, hi) {
                cols[(x + NODE_CELLS * z) as usize] = (lo as u8, hi as u8 + 1);
            }
        }
    }
    ColumnGrid::from_columns(&cols)
}

/// Reads a build back as a 32³ grid of states (`x + 32 (y + 32 z)`): occupied cells exactly;
/// cells of empty bricks read as `air`.
pub fn decode(build: &NodeBuild, air: StateId) -> Vec<StateId> {
    let n = NODE_CELLS as usize;
    let mut out = vec![air; n * n * n];
    let idx = |x: i32, y: i32, z: i32| (x + NODE_CELLS * (y + NODE_CELLS * z)) as usize;
    match build {
        NodeBuild::Air => {}
        NodeBuild::Solid(s) => out.fill(*s),
        NodeBuild::Node(d) => {
            for bz in 0..4 {
                for by in 0..4 {
                    for bx in 0..4 {
                        let r = d.node.get(brick_index(bx, by, bz));
                        for z in 0..8 {
                            for y in 0..8 {
                                for x in 0..8 {
                                    let s = match r {
                                        BrickRef::Empty(_) => continue,
                                        BrickRef::Uniform(s) => s,
                                        BrickRef::Mixed(i) => {
                                            brick_state(&d.bricks[i as usize], voxel_index(x, y, z))
                                        }
                                    };
                                    out[idx(bx * 8 + x, by * 8 + y, bz * 8 + z)] = s;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    fn rng(seed: u64) -> impl FnMut() -> u64 {
        let mut s = seed | 1;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    }

    #[test]
    fn classifies_air_uniform_and_mixed() {
        let t = StateTable::registry();
        let stone = require("dwell:stone");
        let water = require("dwell:water");
        assert_eq!(build_chunk(&vec![0; CHUNK_VOLUME], &t), NodeBuild::Air);
        assert_eq!(build_chunk(&vec![water; CHUNK_VOLUME], &t), NodeBuild::Air);
        assert_eq!(
            build_chunk(&vec![stone; CHUNK_VOLUME], &t),
            NodeBuild::Solid(stone)
        );
        // The bottom brick row stone, one voxel of grass on top of brick 0.
        let mut v = vec![0; CHUNK_VOLUME];
        for z in 0..32 {
            for x in 0..32 {
                for y in 0..8 {
                    v[local_index(x, y, z)] = stone;
                }
            }
        }
        v[local_index(1, 8, 1)] = require("dwell:grass");
        let NodeBuild::Node(d) = build_chunk(&v, &t) else {
            panic!("a node");
        };
        assert_eq!(d.node.get(brick_index(1, 0, 0)), BrickRef::Uniform(stone));
        assert_eq!(d.node.get(brick_index(0, 1, 0)), BrickRef::Mixed(0));
        assert_eq!(d.node.get(brick_index(1, 1, 0)), BrickRef::Empty(1));
        assert_eq!(d.node.get(brick_index(3, 3, 3)), BrickRef::Empty(3));
        assert_eq!(d.node.get(brick_index(2, 2, 0)), BrickRef::Empty(2));
        assert_eq!(d.bricks.len(), 1);
        assert_eq!(d.bricks[0].len(), RAW_BRICK_WORDS);
        let cols = d.columns.as_ref().expect("chunks have column grids");
        assert_eq!(cols.span(0, 1, 1), (0, 9));
        assert_eq!(cols.span(0, 2, 2), (0, 8));
    }

    #[test]
    fn chunks_round_trip() {
        // The registry's ladders are cutout (a raster pass, not occupied); drawn opaque here they
        // are the shape hook's case: a thin look without a shape.
        let mut states = dwell_blocks::STATES.to_vec();
        for st in states.iter_mut().filter(|st| st.look == "ladder") {
            st.render = dwell_blocks::RenderMode::Opaque;
        }
        let t = StateTable::from_states(&states);
        let ids = [0u16, 1, 2, 3, 13, 4];
        let mut r = rng(7);
        let mut v = vec![0; CHUNK_VOLUME];
        for z in 0..32 {
            for y in 0..32 {
                for x in 0..32 {
                    // Sparse noise with whole uniform regions.
                    let s = if y < 6 {
                        1
                    } else if r().is_multiple_of(9) {
                        ids[(r() % ids.len() as u64) as usize]
                    } else {
                        0
                    };
                    v[local_index(x, y, z)] = s;
                }
            }
        }
        let b = build_chunk(&v, &t);
        let back = decode(&b, 0);
        for z in 0..32 {
            for y in 0..32 {
                for x in 0..32 {
                    assert_eq!(
                        back[(x + 32 * (y + 32 * z)) as usize],
                        v[local_index(x, y, z)]
                    );
                }
            }
        }
        // The (opaque) ladder cells give their bricks a shape plane.
        let NodeBuild::Node(d) = &b else { panic!() };
        assert!(d.bricks.iter().any(|w| w.len() == SHAPED_BRICK_WORDS));
    }

    #[test]
    fn palette_bricks_round_trip_at_every_width() {
        let mut t_states = dwell_blocks::STATES.to_vec();
        // Enough solid states for a raw fallback: fixture rows copied from stone.
        for _ in 0..40 {
            t_states.push(dwell_blocks::STATES[1]);
        }
        let t = StateTable::from_states(&t_states);
        // The solid states drawn from: the copies (the registry's own may not all be solid).
        let first = dwell_blocks::STATES.len() as u16;
        let mut r = rng(11);
        for distinct in [1usize, 2, 3, 4, 5, 16, 17, 40] {
            let mut cells = vec![0u16; lod::VOLUME];
            for y in 0..32 {
                for z in 0..32 {
                    for x in 0..32 {
                        cells[lod::cell_index(x, y, z)] = if y < 16 {
                            first + (r() % distinct as u64) as u16
                        } else {
                            0
                        };
                    }
                }
            }
            let b = build_section(&cells, &t);
            let back = decode(&b, 0);
            for y in 0..32 {
                for z in 0..32 {
                    for x in 0..32 {
                        assert_eq!(
                            back[(x + 32 * (y + 32 * z)) as usize],
                            cells[lod::cell_index(x, y, z)],
                            "{distinct} states"
                        );
                    }
                }
            }
            if let NodeBuild::Node(d) = &b {
                let want = match distinct {
                    1 => None,
                    2 => Some(palette_brick_words(1)),
                    3 | 4 => Some(palette_brick_words(2)),
                    5..=16 => Some(palette_brick_words(4)),
                    _ => Some(RAW_BRICK_WORDS),
                };
                if let Some(want) = want {
                    assert!(d.bricks.iter().all(|w| w.len() == want), "{distinct}");
                }
                let cols = d.columns.as_ref().expect("sections have columns");
                assert_eq!(cols.span(0, 3, 3), (0, 16));
                assert_eq!(cols.span(5, 0, 0), (0, 16));
            }
        }
    }

    #[test]
    fn brick_distances_are_chebyshev() {
        let mut empty = [true; 64];
        empty[brick_index(0, 0, 0)] = false;
        let d = brick_distances(&empty);
        assert_eq!(d[brick_index(0, 0, 0)], 0);
        assert_eq!(d[brick_index(1, 1, 0)], 1);
        assert_eq!(d[brick_index(2, 1, 0)], 2);
        assert_eq!(d[brick_index(3, 3, 3)], 3);
        assert_eq!(brick_distances(&[true; 64]), [4; 64]);
    }

    #[test]
    fn column_mip_takes_min_and_max() {
        let mut cols = vec![EMPTY_SPAN; 1024];
        cols[0] = (3, 5);
        cols[1] = (1, 2);
        cols[33] = (10, 20);
        let g = ColumnGrid::from_columns(&cols);
        assert_eq!(g.span(1, 0, 0), (1, 20));
        assert_eq!(g.span(1, 1, 0), EMPTY_SPAN);
        assert_eq!(g.span(5, 0, 0), (1, 20));
        let w = g.to_words();
        assert_eq!(column_span(&w, 1, 0, 0), (1, 20));
        assert_eq!(column_span(&w, 0, 1, 1), (10, 20));
        assert_eq!(column_span(&w, 0, 5, 5), EMPTY_SPAN);
    }

    #[test]
    fn builds_are_deterministic() {
        let t = StateTable::registry();
        let mut r = rng(3);
        let v: Vec<u16> = (0..CHUNK_VOLUME).map(|_| (r() % 4) as u16).collect();
        assert_eq!(build_chunk(&v, &t), build_chunk(&v, &t));
    }
}
