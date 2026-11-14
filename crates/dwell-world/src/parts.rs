//! Multi-cell blocks kept whole (docs/BLOCK_REGISTRY.md, Multi-cell blocks; BUGS.md #171): a block
//! whose data `extends.up` names the block in the cell above it (a torch's stick and its flame,
//! `partOf` naming the stick back) is in the world whole or not at all. One rule over the block
//! data, no block's own code:
//!
//! - a structure or the generator writes a multi-cell block's cells together or neither
//!   ([`Placed::cells_whole`]);
//! - a placed one needs every cell free (`edit::check_block_edit`) and puts them all
//!   (`edit::edit_writes`);
//! - any write that leaves one cell of it without the other clears the other
//!   ([`lone_parts`], run over every server tick's changed cells, whatever made them: a break, a
//!   blast, a fill, a falling piece, water);
//! - a lone cell in a saved chunk is removed when the chunk loads ([`repair_chunk`]).
//!
//! A lone cell is removed rather than completed: completing would write a cell the player never put
//! (perhaps over what they did), and removing is what breaking either cell already does.

use dwell_blocks::{StateId, info};

use crate::structure::Placed;
use crate::{CHUNK_SIZE, Chunk, ChunkCoord, M, VoxelWorld, edit, local_index};

/// The name of the block a state belongs to ("" for an id outside the table).
fn block_name(m: StateId) -> &'static str {
    if usize::from(m) < dwell_blocks::state_count() {
        dwell_blocks::block_of(m).name
    } else {
        ""
    }
}

/// Whether `m` is one cell of a multi-cell block (block data `extends` or `partOf`).
pub fn multi_cell(m: StateId) -> bool {
    dwell_blocks::try_info(m).is_some_and(|s| s.extends_up.is_some() || s.part_of.is_some())
}

/// The other cells a multi-cell block's state at `cell` needs, each with the block's name there:
/// above its face's ground for `extends.up`, below for `partOf`.
pub fn partners(cell: [i32; 3], m: StateId) -> Vec<([i32; 3], &'static str)> {
    let Some(s) = dwell_blocks::try_info(m) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(up) = s.extends_up {
        out.push((edit::above(cell), up));
    }
    if let Some(of) = s.part_of {
        out.push((edit::below(cell), of));
    }
    out
}

/// Whether `m` at `cell` is whole: every other cell it needs holds its block. `get` reads a cell,
/// `None` where it is not known (an unloaded chunk): taken as whole, never cleared on a guess.
pub fn whole(cell: [i32; 3], m: StateId, get: impl Fn([i32; 3]) -> Option<StateId>) -> bool {
    partners(cell, m)
        .into_iter()
        .all(|(p, name)| get(p).is_none_or(|there| block_name(there) == name))
}

/// What a lone cell leaves: water where it was flooded, else air.
fn left_by(m: StateId) -> StateId {
    if info(m).flooded { M.water } else { M.air }
}

/// The lone cells of multi-cell blocks at or beside (above, below) the `changed` cells, each with
/// what clearing it leaves: what a write that took or replaced one cell of a torch must also clear.
/// Read against `world` as it is (after the writes); unloaded cells are never judged.
pub fn lone_parts(world: &VoxelWorld, changed: &[[i32; 3]]) -> Vec<([i32; 3], StateId)> {
    let get = |p: [i32; 3]| world.voxel_if_held(p);
    let mut out: Vec<([i32; 3], StateId)> = Vec::new();
    for &c in changed {
        for p in [c, edit::above(c), edit::below(c)] {
            let Some(m) = get(p) else { continue };
            if multi_cell(m) && !whole(p, m, get) && !out.iter().any(|(o, _)| *o == p) {
                out.push((p, left_by(m)));
            }
        }
    }
    out
}

/// Removes the lone cells of multi-cell blocks in a chunk read from a world file (a saved torch's
/// flame without its stick, or a stick without its flame). A partner in another chunk is read with
/// `outside` (`None`: not loaded, left alone). Returns how many cells it cleared.
pub fn repair_chunk(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    outside: impl Fn([i32; 3]) -> Option<StateId>,
) -> usize {
    const S: i32 = CHUNK_SIZE;
    let o = [coord.x * S, coord.y * S, coord.z * S];
    let inside = |p: [i32; 3]| (0..3).all(|i| (o[i]..o[i] + S).contains(&p[i]));
    let mut lone = Vec::new();
    {
        let voxels = chunk.voxels();
        let get = |p: [i32; 3]| {
            if inside(p) {
                Some(voxels[local_index(p[0] - o[0], p[1] - o[1], p[2] - o[2])])
            } else {
                outside(p)
            }
        };
        for (i, &m) in voxels.iter().enumerate() {
            if !multi_cell(m) {
                continue;
            }
            let i = i as i32;
            let p = [o[0] + (i & 31), o[1] + ((i >> 5) & 31), o[2] + (i >> 10)];
            if !whole(p, m, get) {
                lone.push((i as usize, left_by(m)));
            }
        }
    }
    for &(i, m) in &lone {
        chunk.set_at(i, m);
    }
    lone.len()
}

impl Placed {
    /// [`Placed::cells`] with every multi-cell block kept whole: its cells are written only when the
    /// placement holds all of them and `free` says each may be written (over air or a replaceable
    /// block, as the generator stamps), else none of them. A placement is face-local (+y up).
    /// `free` is asked before any is written: it reads the cells as they are before the stamp.
    pub fn cells_whole(&self, free: impl Fn([i32; 3]) -> bool) -> Vec<([i32; 3], StateId)> {
        let kept = |p: [i32; 3], m: StateId| {
            if !multi_cell(m) {
                return true;
            }
            // The block's lowest cell: down through `partOf` (one part a block: parts.mjs).
            let s = info(m);
            let base = if s.part_of.is_some() {
                [p[0], p[1] - 1, p[2]]
            } else {
                p
            };
            let Some(lower) = self.at(base) else {
                return false;
            };
            let li = info(lower);
            let Some(up) = li.extends_up else {
                return false;
            };
            let top = [base[0], base[1] + 1, base[2]];
            let upper_ok = self.at(top).is_some_and(|u| block_name(u) == up);
            let lower_ok = s.part_of.is_none_or(|of| block_name(lower) == of);
            upper_ok && lower_ok && free(base) && free(top)
        };
        self.cells().filter(|&(p, m)| kept(p, m)).collect()
    }
}

/// A region edit's changes (a fill, a paste, a structure; `region::check_region_edit`) with every
/// multi-cell block kept whole, read against `get` (the world before the edit): a change that
/// would put one cell of a multi-cell block without its others is dropped (a fill of torches puts
/// none), and a change that takes or replaces one cell of a torch the box cuts through also clears
/// its other cell outside the box.
pub fn keep_whole(
    changes: &mut Vec<([i32; 3], StateId)>,
    mut get: impl FnMut([i32; 3]) -> StateId,
) {
    use std::collections::HashMap;
    // Drop what would stand lone, until nothing more does (dropping one may leave its partner's
    // change lone).
    loop {
        let after: HashMap<[i32; 3], StateId> = changes.iter().copied().collect();
        let mut cache: HashMap<[i32; 3], StateId> = HashMap::new();
        let mut view = |p: [i32; 3]| {
            after
                .get(&p)
                .copied()
                .unwrap_or_else(|| *cache.entry(p).or_insert_with(|| get(p)))
        };
        let lone: Vec<usize> = (0..changes.len())
            .filter(|&i| {
                let (c, m) = changes[i];
                multi_cell(m)
                    && partners(c, m)
                        .into_iter()
                        .any(|(p, name)| block_name(view(p)) != name)
            })
            .collect();
        if lone.is_empty() {
            break;
        }
        for i in lone.into_iter().rev() {
            changes.remove(i);
        }
    }
    // The other cells of what the changes took, beside the box.
    let after: HashMap<[i32; 3], StateId> = changes.iter().copied().collect();
    let mut view = |p: [i32; 3]| after.get(&p).copied().unwrap_or_else(|| get(p));
    let mut extra: Vec<([i32; 3], StateId)> = Vec::new();
    for &(c, _) in changes.iter() {
        for p in [edit::above(c), edit::below(c)] {
            if after.contains_key(&p) || extra.iter().any(|(o, _)| *o == p) {
                continue;
            }
            let m = view(p);
            if multi_cell(m)
                && partners(p, m)
                    .into_iter()
                    .any(|(q, name)| block_name(view(q)) != name)
            {
                extra.push((p, left_by(m)));
            }
        }
    }
    changes.extend(extra);
}
