//! The water's surface as a heightfield (proposals/WATER_GPU_SURFACE.md §3, step C6a): each near
//! water chunk is a tile of levels, filled from the eighths a client already has (no message), on
//! one shared grid mesh whose vertex shader (`dwell-client` `render/heightfield.rs`) reads them. A
//! water change rewrites the tile, not the chunk's mesh, once the chunk mesher leaves the liquid's
//! tops to it ([`crate::ChunkFlags`]`::water_tops`).
//!
//! Heights are face-local (+y up from the chunk's face-local floor, BIFACIAL_WORLD.md §11), so a
//! face-B chunk's tile matches its face-A twin's and the shader crosses into the world through
//! `face_y`.

use crate::mesher::{Kinds, PAD, water_levels_with};

/// A tile's side in texels: the chunk's columns and a one-column apron (neighbours' edges), so
/// corners on a chunk's border are shared with its neighbour's tile.
pub const TILE: usize = PAD;
/// A tile's levels (`TILE` × `TILE`, x fastest, then z).
pub type Levels = Vec<f32>;
/// The level of a column with no surface in the chunk (far below any chunk-local height; the
/// shader tests `< DRY / 2`).
pub const DRY: f32 = -1.0e4;
/// The grid's columns per side (a chunk's).
pub const COLUMNS: usize = TILE - 2;
/// The grid's corners per side.
pub const CORNERS: usize = COLUMNS + 1;

/// The chunk's surface levels from its padded voxels ([`crate::padded_index`] order), face-local
/// (`mirror` for face B).
pub fn water_levels(padded: &[u16], mirror: bool) -> Levels {
    water_levels_with(Kinds::build(), padded, mirror).0
}

/// A chunk's surface tile as the client's renderer takes it with the chunk's meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterTile {
    /// [`TILE`]² face-local levels ([`water_levels`]).
    pub levels: Levels,
    /// The fluid drawn (its id in the registry's fluid table, as the meshes' `fluids` stream):
    /// the first wet column's in the chunk. A chunk holds one water today.
    pub fluid: f32,
    /// A face-B chunk (its tile face-local; the shader mirrors it back).
    pub face_b: bool,
}

/// Chunk `padded`'s tile over `kinds`; none where no column inside the chunk is wet.
pub(crate) fn tile_with(kinds: &Kinds, padded: &[u16], mirror: bool) -> Option<WaterTile> {
    let (levels, fluid) = water_levels_with(kinds, padded, mirror);
    (fluid != crate::NO_FLUID).then_some(WaterTile {
        levels,
        fluid,
        face_b: mirror,
    })
}

/// The shared grid's indices for both faces: [`grid_indices`] (face A), then the same
/// triangles re-wound (face B: the mirrored grid's up side is −y), each `COLUMNS² × 6` long.
pub fn grid_indices_both() -> Vec<u32> {
    let a = grid_indices();
    let b: Vec<u32> = a.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
    [a, b].concat()
}

/// Whether column level `h` is wet.
pub fn wet(h: f32) -> bool {
    h > DRY / 2.0
}

/// A grid corner's height from the four columns around it: the mean of the wet ones within
/// `join_m` of the highest (the lower ones are a real drop, C6b's skirts); `DRY` if none is wet.
pub fn corner_height(columns: [f32; 4], join_m: f32) -> f32 {
    let top = columns
        .iter()
        .copied()
        .filter(|h| wet(*h))
        .fold(DRY, f32::max);
    if !wet(top) {
        return DRY;
    }
    let (sum, n) = columns
        .iter()
        .filter(|h| wet(**h) && top - **h <= join_m)
        .fold((0.0, 0.0), |(s, n), h| (s + h, n + 1.0));
    sum / n
}

/// The heights of a tile's [`CORNERS`]² grid corners (x fastest): corner (i, j) lies between
/// columns i − 1 and i (chunk-local), so its four columns are tile texels (i, j) to (i + 1, j + 1).
/// The vertex shader computes the same (`render/heightfield.rs`); this is its reference.
pub fn corner_heights(levels: &[f32], join_m: f32) -> Vec<f32> {
    let l = |x: usize, z: usize| levels[x + TILE * z];
    let mut out = Vec::with_capacity(CORNERS * CORNERS);
    for j in 0..CORNERS {
        for i in 0..CORNERS {
            out.push(corner_height(
                [l(i, j), l(i + 1, j), l(i, j + 1), l(i + 1, j + 1)],
                join_m,
            ));
        }
    }
    out
}

/// The shared grid mesh's triangles over [`CORNERS`]² corners (corner `i + CORNERS × j`),
/// counter-clockwise seen from face-local up, two per column. A column's triangles collapse in the
/// shader when it is dry.
pub fn grid_indices() -> Vec<u32> {
    let c = CORNERS as u32;
    let mut out = Vec::with_capacity(COLUMNS * COLUMNS * 6);
    for j in 0..COLUMNS as u32 {
        for i in 0..COLUMNS as u32 {
            let a = i + c * j;
            let (b, d, e) = (a + 1, a + c, a + c + 1);
            // Seen from +y with x right and z toward the viewer: a → d → e, a → e → b.
            out.extend_from_slice(&[a, d, e, a, e, b]);
        }
    }
    out
}

/// The client's atlas of tiles: one slot per near water chunk, its levels kept so an unchanged
/// rewrite uploads nothing. Keys are the caller's (chunk coordinates).
#[derive(Debug, Default)]
pub struct HeightAtlas<K: std::hash::Hash + Eq + Copy> {
    slots: std::collections::HashMap<K, (usize, Levels)>,
    free: Vec<usize>,
    next: usize,
    dirty: Vec<usize>,
}

impl<K: std::hash::Hash + Eq + Copy> HeightAtlas<K> {
    pub fn new() -> Self {
        Self {
            slots: std::collections::HashMap::new(),
            free: Vec::new(),
            next: 0,
            dirty: Vec::new(),
        }
    }

    /// Sets chunk `key`'s levels: a chunk with no wet column frees its slot. Returns whether the
    /// tile changed (and is queued for upload).
    pub fn set(&mut self, key: K, levels: Levels) -> bool {
        if !levels.iter().any(|h| wet(*h)) {
            return self.remove(key);
        }
        if let Some((slot, old)) = self.slots.get_mut(&key) {
            if *old == levels {
                return false;
            }
            *old = levels;
            let slot = *slot;
            self.dirty.push(slot);
            return true;
        }
        let slot = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });
        self.slots.insert(key, (slot, levels));
        self.dirty.push(slot);
        true
    }

    /// Drops chunk `key`'s tile; whether it had one.
    pub fn remove(&mut self, key: K) -> bool {
        match self.slots.remove(&key) {
            Some((slot, _)) => {
                self.free.push(slot);
                self.dirty.retain(|s| *s != slot);
                true
            }
            None => false,
        }
    }

    /// Chunk `key`'s slot.
    pub fn slot(&self, key: K) -> Option<usize> {
        self.slots.get(&key).map(|(s, _)| *s)
    }

    /// The slots most ever used (the atlas texture's size).
    pub fn capacity(&self) -> usize {
        self.next
    }

    /// The tiles changed since the last call, each slot once, with their levels.
    pub fn take_dirty(&mut self) -> Vec<(usize, &[f32])> {
        let mut dirty = std::mem::take(&mut self.dirty);
        dirty.sort_unstable();
        dirty.dedup();
        let by_slot: std::collections::HashMap<usize, &[f32]> =
            self.slots.values().map(|(s, l)| (*s, &l[..])).collect();
        dirty
            .into_iter()
            .filter_map(|s| by_slot.get(&s).map(|l| (s, *l)))
            .collect()
    }
}

#[cfg(test)]
#[path = "heightfield_tests.rs"]
mod tests;
