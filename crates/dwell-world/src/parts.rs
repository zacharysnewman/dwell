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

/// Whether `m` is one cell of a multi-cell block (block data `extends`, `partOf` or `column`).
pub fn multi_cell(m: StateId) -> bool {
    dwell_blocks::try_info(m)
        .is_some_and(|s| s.extends_up.is_some() || s.part_of.is_some() || s.column.is_some())
}

/// `cell` moved `k` rows toward its face's sky (negative: toward its ground).
pub fn up_by(cell: [i32; 3], k: i32) -> [i32; 3] {
    let mut c = cell;
    for _ in 0..k.unsigned_abs() {
        c = if k > 0 {
            edit::above(c)
        } else {
            edit::below(c)
        };
    }
    c
}

/// The other cells a multi-cell block's state at `cell` needs, each with the state it puts there:
/// above its face's ground for `extends.up`, below for `partOf`; a column's other cells above and
/// below it, each its own value of the column's property (the rest of the state the same).
pub fn partners(cell: [i32; 3], m: StateId) -> Vec<([i32; 3], StateId)> {
    let Some(s) = dwell_blocks::try_info(m) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(up) = s.extends_up.and_then(dwell_blocks::state) {
        out.push((edit::above(cell), up));
    }
    if let Some(of) = s.part_of.and_then(dwell_blocks::state) {
        out.push((edit::below(cell), of));
    }
    if let Some(c) = s.column {
        for j in 0..c.count {
            if j == c.index {
                continue;
            }
            if let Some(want) = dwell_blocks::with_property_index(m, c.property, usize::from(j)) {
                out.push((up_by(cell, i32::from(j) - i32::from(c.index)), want));
            }
        }
    }
    out
}

/// Whether `there` holds what a partner wants: a column's cell its exact state (the same door,
/// its own part), any other part its block.
pub fn holds(there: StateId, want: StateId) -> bool {
    if dwell_blocks::try_info(want).is_some_and(|s| s.column.is_some()) {
        return there == want;
    }
    block_name(there) == block_name(want)
}

/// The state of every cell of the multi-cell block `m` belongs to, with `property` set to `value`
/// (a door opened): `m`'s own new state, or `m` when it has no such value.
pub fn with_all(m: StateId, property: &str, value: &str) -> StateId {
    dwell_blocks::with_property(m, property, value).unwrap_or(m)
}

/// Whether `m` at `cell` is whole: every other cell it needs holds its block. `get` reads a cell,
/// `None` where it is not known (an unloaded chunk): taken as whole, never cleared on a guess.
pub fn whole(cell: [i32; 3], m: StateId, get: impl Fn([i32; 3]) -> Option<StateId>) -> bool {
    partners(cell, m)
        .into_iter()
        .all(|(p, want)| get(p).is_none_or(|there| holds(there, want)))
}

/// `c` and the cells beside it above and below that a multi-cell block there could reach: the one
/// above and the one below, and on through every further cell `multi` holds (a door's four cells).
fn near(c: [i32; 3], mut multi: impl FnMut([i32; 3]) -> bool) -> Vec<[i32; 3]> {
    let mut out = vec![c];
    for dir in [1, -1] {
        let mut p = c;
        for _ in 0..8 {
            p = up_by(p, dir);
            out.push(p);
            if !multi(p) {
                break;
            }
        }
    }
    out
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
        for p in near(c, |p| get(p).is_some_and(multi_cell)) {
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

/// [`partners`] in face-local cells (+y up), for placements before they go through a face.
fn partners_local(p: [i32; 3], m: StateId) -> Vec<([i32; 3], StateId)> {
    let Some(s) = dwell_blocks::try_info(m) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(up) = s.extends_up.and_then(dwell_blocks::state) {
        out.push(([p[0], p[1] + 1, p[2]], up));
    }
    if let Some(of) = s.part_of.and_then(dwell_blocks::state) {
        out.push(([p[0], p[1] - 1, p[2]], of));
    }
    if let Some(c) = s.column {
        for j in 0..c.count {
            if j != c.index
                && let Some(want) = dwell_blocks::with_property_index(m, c.property, usize::from(j))
            {
                out.push(([p[0], p[1] + i32::from(j) - i32::from(c.index), p[2]], want));
            }
        }
    }
    out
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
            // Every other cell in the placement holding its part (face-local: +y up), and all of
            // them free.
            let others: Vec<([i32; 3], StateId)> = partners_local(p, m);
            !others.is_empty()
                && free(p)
                && others
                    .iter()
                    .all(|&(q, want)| self.at(q).is_some_and(|u| holds(u, want)) && free(q))
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
                        .any(|(p, want)| !holds(view(p), want))
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
        for p in near(c, |p| multi_cell(view(p))).into_iter().skip(1) {
            if after.contains_key(&p) || extra.iter().any(|(o, _)| *o == p) {
                continue;
            }
            let m = view(p);
            if multi_cell(m)
                && partners(p, m)
                    .into_iter()
                    .any(|(q, want)| !holds(view(q), want))
            {
                extra.push((p, left_by(m)));
            }
        }
    }
    changes.extend(extra);
}
