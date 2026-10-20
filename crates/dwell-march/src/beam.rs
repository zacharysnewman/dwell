//! The beam prepass (RAY_MARCHED_RENDERER.md §4.2 pass 1, §4.3 item 5; Phase 25 step A4b): one
//! conservative ray per tile of `BEAM_TILE` × `BEAM_TILE` pixels finds a distance that no pixel
//! ray of the tile hits anything before, and every pixel ray starts there
//! ([`Scene::march_from`]).
//!
//! # Why it is conservative
//!
//! The tile's pixel rays leave the same eye, and each is within angle θ of the beam's centre ray;
//! at the same distance `t` along each, a pixel ray's point is within `t · |d − d_c|` of the
//! centre ray's, and `|d − d_c|` is largest at a corner of the tile (the cone's cross-section on
//! the image plane is an ellipse, so a convex tile's farthest point from its centre is a vertex).
//! That chord length is the beam's **spread** κ. So for `t` in `[t0, t1]` every pixel ray lies in
//! the box around the centre ray's segment widened by `κ · t1` (plus a slack for rounding).
//!
//! The prepass walks the centre ray with segments of growing length (doubled after a clear one,
//! halved after a blocked one) and moves on past a segment only when its widened box holds no
//! occupied cell at any level a pixel ray there could stop at ([`Scene::box_clear`]):
//!
//! - A ray at distance `t` asks first for the cone LOD's level `level_for(t)` and falls back to
//!   the first coarser level with data; the LOD-switch rule (march.rs) can keep it at a finer
//!   level it marched at before. Every level it can stop at is therefore at most the first level
//!   from `level_for(t1)` up with data there — `level_for` grows with `t` — and the box test
//!   checks every level from 0 to the first one from `level_for(t1)` up that has data all over
//!   the box (the top level if none).
//! - Per level: the level's box and occupied rows (`Scene::rows`), then each node the box meets
//!   (at most `BEAM_MAX_NODES` a side, else the segment is too long), its column grid at the mip
//!   whose regions the box fits in, then its bricks, and in a mixed brick the occupancy bits.
//!
//! The walk stops at the first segment it can't clear once segments are down to
//! `BEAM_MIN_STEP` cells of the cone level, or after `BEAM_ITERATIONS`: every pixel ray in the
//! tile then starts at the last distance cleared. A start never passes a hit (the tests:
//! `tests/beam.rs` on the CPU, the per-pixel GPU check in the client against a march from the
//! eye).
//!
//! The GPU's prepass (`render/march/wgsl.rs`, `beam_main`) is this module transcribed; the
//! per-pixel check reads the start each pixel used back from the GPU and marches the reference
//! from it, so the two need not agree to the bit.

use crate::layout::*;
use crate::march::{MarchParams, level_for, shape_hit};
use crate::scene::{COLUMNS_NONE, Scene};

/// Pixels a side of a beam's tile.
pub const BEAM_TILE: u32 = 8;
/// Segments a beam tries before it settles.
pub const BEAM_ITERATIONS: u32 = 48;
/// The first segment's length (metres).
pub const BEAM_FIRST_STEP: f32 = 1.0;
/// A blocked segment this short (in cells of the cone level at its start) ends the walk.
pub const BEAM_MIN_STEP: f32 = 1.0;
/// Nodes a side a box may meet at a level whose occupied box it reaches.
pub const BEAM_MAX_NODES: i32 = 3;
/// Slack added to the cone's radius (cells): absolute, and per metre of distance, for the
/// rounding of a ray's position (`frac + t · d` in `f32`).
pub const BEAM_SLACK: f32 = 1.0e-2;
pub const BEAM_SLACK_PER_M: f32 = 1.0e-5;
/// The spread is widened by this factor (the GPU's directions are rounded differently).
pub const BEAM_SPREAD_MARGIN: f32 = 1.001;

/// A tile's beam: the eye (grid cell and fraction, as a `Ray`), the centre direction (unit) and
/// the spread κ (the largest `|d − d_c|` of the tile's pixel rays).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    pub cell: [i32; 3],
    pub frac: [f32; 3],
    pub dir: [f32; 3],
    pub spread: f32,
}

fn sub_len(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|k| a[k] - b[k]);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

impl Beam {
    /// The beam of a tile from its centre direction and its corner pixels' directions (unit).
    pub fn new(cell: [i32; 3], frac: [f32; 3], dir: [f32; 3], corners: &[[f32; 3]]) -> Self {
        let spread = corners
            .iter()
            .map(|&c| sub_len(c, dir))
            .fold(0.0f32, f32::max);
        Self {
            cell,
            frac,
            dir,
            spread: spread * BEAM_SPREAD_MARGIN,
        }
    }

    /// The cone's half-width (cells) at distance `t`, with the slack.
    pub fn radius(&self, t: f32) -> f32 {
        self.spread * t + BEAM_SLACK + BEAM_SLACK_PER_M * t
    }

    /// The level-0 grid cells (inclusive) every pixel ray of the tile is in for `t` in
    /// `[t0, t1]`.
    pub fn segment_box(&self, t0: f32, t1: f32) -> ([i32; 3], [i32; 3]) {
        let r = self.radius(t1);
        let mut lo = [0; 3];
        let mut hi = [0; 3];
        for k in 0..3 {
            let a = self.frac[k] + t0 * self.dir[k];
            let b = self.frac[k] + t1 * self.dir[k];
            lo[k] = self.cell[k] + (a.min(b) - r).floor() as i32;
            hi[k] = self.cell[k] + (a.max(b) + r).floor() as i32;
        }
        (lo, hi)
    }
}

impl Scene {
    /// The distance (metres) the tile's pixel rays can start from (see the module docs).
    pub fn beam_start(&self, beam: &Beam, p: &MarchParams) -> f32 {
        self.beam_walk(beam, p).0
    }

    /// The start and the segments tried.
    pub fn beam_walk(&self, beam: &Beam, p: &MarchParams) -> (f32, u32) {
        let Some((_, top)) = self.level_range() else {
            return (p.max_t, 0);
        };
        let mut t = 0.0f32;
        let mut dt = BEAM_FIRST_STEP;
        // Segments double until one is blocked, then the walk bisects: each later segment is
        // half the last, cleared or not, down to the minimum.
        let mut grow = 2.0f32;
        let mut tried = 0;
        while tried < BEAM_ITERATIONS && t < p.max_t {
            tried += 1;
            let t1 = (t + dt).min(p.max_t);
            let (lo, hi) = beam.segment_box(t, t1);
            if self.box_clear(lo, hi, level_for(t1, p).min(top), top) {
                t = t1;
            } else {
                grow = 0.5;
            }
            if grow < 1.0 && dt <= BEAM_MIN_STEP * (1u32 << level_for(t, p).min(top)) as f32 {
                break;
            }
            dt *= grow;
        }
        (t, tried)
    }

    /// Whether level-0 grid box `[lo, hi]` (inclusive) holds no cell a ray asking for level
    /// `want` or finer could stop at, at any level (see the module docs).
    pub fn box_clear(&self, lo: [i32; 3], hi: [i32; 3], want: u32, top: u32) -> bool {
        let coarsest = (want..=top)
            .find(|&l| self.level_covers(l, lo, hi))
            .unwrap_or(top);
        (0..=coarsest).all(|l| self.level_clear(l, lo, hi))
    }

    /// Whether level `l` has data (a slot) in every node the box meets.
    fn level_covers(&self, l: u32, lo: [i32; 3], hi: [i32; 3]) -> bool {
        let Some(lv) = &self.levels[l as usize] else {
            return false;
        };
        let s = l + 5;
        let (na, nb) = (lo.map(|v| v >> s), hi.map(|v| v >> s));
        if (0..3).any(|k| nb[k] - na[k] >= BEAM_MAX_NODES) {
            return false;
        }
        for z in na[2]..=nb[2] {
            for y in na[1]..=nb[1] {
                for x in na[0]..=nb[0] {
                    match lv.info.slot_of([x, y, z]) {
                        Some(i) if lv.slots[i] != SLOT_NONE => {}
                        _ => return false,
                    }
                }
            }
        }
        true
    }

    /// Whether level `l` holds no occupied cell in the box.
    pub fn level_clear(&self, l: u32, lo: [i32; 3], hi: [i32; 3]) -> bool {
        let Some(lv) = &self.levels[l as usize] else {
            return true;
        };
        let Some((olo, ohi)) = self.rows(l) else {
            return true;
        };
        let s = l + 5;
        let mut a: [i32; 3] = std::array::from_fn(|k| lo[k].max(lv.info.origin[k] << s));
        let mut b: [i32; 3] = std::array::from_fn(|k| {
            hi[k].min(((lv.info.origin[k] + lv.info.dims[k] as i32) << s) - 1)
        });
        a[1] = a[1].max(olo);
        b[1] = b[1].min(ohi - 1);
        if (0..3).any(|k| a[k] > b[k]) {
            return true;
        }
        let (na, nb) = (a.map(|v| v >> s), b.map(|v| v >> s));
        if (0..3).any(|k| nb[k] - na[k] >= BEAM_MAX_NODES) {
            return false;
        }
        for z in na[2]..=nb[2] {
            for y in na[1]..=nb[1] {
                for x in na[0]..=nb[0] {
                    let n = [x, y, z];
                    let Some(i) = lv.info.slot_of(n) else {
                        continue;
                    };
                    match Slot::from_word(lv.slots[i]) {
                        Slot::None | Slot::AllAir => {}
                        Slot::AllSolid(_) => return false,
                        Slot::Node(i) => {
                            if !self.node_clear(l, i as usize, n, a, b) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Whether node `i` (at `n` of level `l`) holds no occupied cell in the level-0 box `[a, b]`.
    fn node_clear(&self, l: u32, i: usize, n: [i32; 3], a: [i32; 3], b: [i32; 3]) -> bool {
        let base = n.map(|v| v << 5);
        let ca: [i32; 3] = std::array::from_fn(|k| ((a[k] >> l) - base[k]).max(0));
        let cb: [i32; 3] = std::array::from_fn(|k| ((b[k] >> l) - base[k]).min(NODE_CELLS - 1));
        let cols = self.node_columns[i];
        if cols != COLUMNS_NONE {
            let words = &self.columns[cols as usize..cols as usize + COLUMN_WORDS];
            // The finest mip whose regions the box's columns fit two a side.
            let mut m = 0;
            while m < COLUMN_MIPS - 1
                && ((cb[0] >> m) - (ca[0] >> m) > 1 || (cb[2] >> m) - (ca[2] >> m) > 1)
            {
                m += 1;
            }
            let mut clear = true;
            for rz in (ca[2] >> m)..=(cb[2] >> m) {
                for rx in (ca[0] >> m)..=(cb[0] >> m) {
                    let (clo, chi) = column_span(words, m, rx as usize, rz as usize);
                    let (clo, chi) = (i32::from(clo), i32::from(chi));
                    clear &= clo >= chi || cb[1] < clo || ca[1] >= chi;
                }
            }
            if clear {
                return true;
            }
        }
        let node = &self.nodes[i];
        for bz in (ca[2] >> 3)..=(cb[2] >> 3) {
            for by in (ca[1] >> 3)..=(cb[1] >> 3) {
                for bx in (ca[0] >> 3)..=(cb[0] >> 3) {
                    match node.get(brick_index(bx, by, bz)) {
                        BrickRef::Empty(_) => {}
                        BrickRef::Uniform(s) => {
                            if shape_hit(self.table.class(s), 0) {
                                return false;
                            }
                        }
                        BrickRef::Mixed(off) => {
                            let brick = self.pool.from(off);
                            let b0 = [bx, by, bz].map(|v| v * BRICK_CELLS);
                            let lo: [i32; 3] = std::array::from_fn(|k| (ca[k] - b0[k]).max(0));
                            let hi: [i32; 3] =
                                std::array::from_fn(|k| (cb[k] - b0[k]).min(BRICK_CELLS - 1));
                            let xmask = (0xFFu32 >> (7 - hi[0])) & (0xFFu32 << lo[0]);
                            for z in lo[2]..=hi[2] {
                                for y in lo[1]..=hi[1] {
                                    let v = voxel_index(0, y, z);
                                    if (brick[1 + (v >> 5)] >> (v & 31)) & xmask != 0 {
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        true
    }
}
