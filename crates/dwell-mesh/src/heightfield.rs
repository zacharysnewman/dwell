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
    water_levels_with(Kinds::build(), padded, &[], mirror).0
}

/// A flood light (sky, block as shares of 15; the meshes' `light` stream) packed into one float:
/// sky × 16 + block, whole levels.
pub fn pack_light(l: [f32; 2]) -> f32 {
    (l[0] * 15.0).round() * 16.0 + (l[1] * 15.0).round()
}

/// A tile texel's second channel: the column's packed light ([`pack_light`], under
/// [`LIGHT_SPAN`]) plus [`LIGHT_SPAN`] × the face-local cell row of its water's floor (the run of
/// the same water under its surface, in the chunk): the skirts' bottom.
pub fn pack_column(l: [f32; 2], floor_cell: i32) -> f32 {
    pack_light(l) + LIGHT_SPAN * floor_cell as f32
}
/// The packed light's span in a texel's second channel ([`pack_column`]).
pub const LIGHT_SPAN: f32 = 256.0;
/// A texel's packed light ([`pack_light`]) from its second channel.
pub fn column_light(packed: f32) -> f32 {
    packed - LIGHT_SPAN * (packed / LIGHT_SPAN).floor()
}
/// A texel's water floor, face-local metres, from its second channel.
pub fn column_floor(packed: f32) -> f32 {
    (packed / LIGHT_SPAN).floor() * dwell_world::CELL_H as f32
}

/// A chunk's surface tile as the client's renderer takes it with the chunk's meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterTile {
    /// [`TILE`]² face-local levels ([`water_levels`]).
    pub levels: Levels,
    /// Each column's flood light at its surface ([`pack_light`]; open sky where dry).
    pub lights: Vec<f32>,
    /// The fluid drawn (its id in the registry's fluid table, as the meshes' `fluids` stream):
    /// the first wet column's in the chunk. A chunk holds one water today.
    pub fluid: f32,
    /// A face-B chunk (its tile face-local; the shader mirrors it back).
    pub face_b: bool,
}

/// Chunk `padded`'s tile with its flood light (`light`: the mesh job's; empty: open sky), as the
/// mesher makes it; none where no column inside the chunk is wet. The client's re-mesh skip: a
/// chunk whose only changes are its water's levels gets its tile from this, not a mesh job.
pub fn water_tile(padded: &[u16], light: &[u8], mirror: bool) -> Option<WaterTile> {
    tile_with(crate::mesher::Kinds::build(), padded, light, mirror)
}

/// Chunk `padded`'s tile over `kinds`; none where no column inside the chunk is wet.
pub(crate) fn tile_with(
    kinds: &Kinds,
    padded: &[u16],
    light: &[u8],
    mirror: bool,
) -> Option<WaterTile> {
    let (levels, lights, fluid) = water_levels_with(kinds, padded, light, mirror);
    (fluid != crate::NO_FLUID).then_some(WaterTile {
        levels,
        lights,
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

/// The grid's vertices per column: its two triangles, unshared, so a dry column's six collapse
/// to one point and draw nothing (the vertex shader decides per column, not per corner).
pub const COLUMN_VERTICES: usize = 6;
/// The grid's vertices in one draw: every column's [`COLUMN_VERTICES`].
pub const GRID_VERTICES: usize = COLUMNS * COLUMNS * COLUMN_VERTICES;

/// The corner offsets (x, z from the column's own corner) of a column's six vertices, in
/// [`grid_indices`]' order (face A) or re-wound as [`grid_indices_both`]' second half (face B).
pub fn column_corners(face_b: bool) -> [[usize; 2]; COLUMN_VERTICES] {
    let both = grid_indices_both();
    let first = if face_b { COLUMNS * COLUMNS * 6 } else { 0 };
    std::array::from_fn(|k| {
        let i = both[first + k] as usize;
        [i % CORNERS, i / CORNERS]
    })
}

/// Grid vertex `vertex` of a tile, face-local (x, height, z in chunk-local metres), as
/// `heightfield_vs` places it: none (collapsed to one point, no pixels) where its column is dry,
/// else its corner at [`corner_height`]. A wet column's corners are all wet, so the surface ends
/// at the shoreline's column edge as the meshed water's tops do. The shader's reference.
pub fn grid_vertex(levels: &[f32], join_m: f32, vertex: usize, face_b: bool) -> Option<[f32; 3]> {
    let column = vertex / COLUMN_VERTICES;
    let (cx, cz) = (column % COLUMNS, column / COLUMNS);
    let l = |x: usize, z: usize| levels[x + TILE * z];
    if !wet(l(cx + 1, cz + 1)) {
        return None;
    }
    let [ox, oz] = column_corners(face_b)[vertex % COLUMN_VERTICES];
    let (i, j) = (cx + ox, cz + oz);
    let h = corner_height([l(i, j), l(i + 1, j), l(i, j + 1), l(i + 1, j + 1)], join_m);
    Some([i as f32, h, j as f32])
}

/// A column's sides (W2): −x, +x, −z, +z, each a skirt quad of six vertices.
pub const SIDES: [[i32; 2]; 4] = [[-1, 0], [1, 0], [0, -1], [0, 1]];
/// The skirts' vertices per column: four sides of [`COLUMN_VERTICES`].
pub const SKIRT_COLUMN_VERTICES: usize = SIDES.len() * COLUMN_VERTICES;
/// The skirts' vertices in one draw, after the grid's [`GRID_VERTICES`].
pub const SKIRT_VERTICES: usize = COLUMNS * COLUMNS * SKIRT_COLUMN_VERTICES;
/// One heightfield draw's vertices: the grid's, then the skirts'.
pub const DRAW_VERTICES: usize = GRID_VERTICES + SKIRT_VERTICES;

/// The bottom (face-local metres) of the skirt of a column at `level` with water floor `floor`
/// beside a neighbour column at `beside` (its level, or [`DRY`]): none where the neighbour's water
/// joins this one (within `join_m`, the shared corners draw the step) or none shows. Down to the
/// neighbour's lower water, else to the column's own floor (a bank in front hides the rest).
pub fn skirt_bottom(level: f32, floor: f32, beside: f32, join_m: f32) -> Option<f32> {
    if !wet(level) || (wet(beside) && level - beside <= join_m) {
        return None;
    }
    let bottom = if wet(beside) {
        beside.max(floor)
    } else {
        floor
    };
    (bottom < level).then_some(bottom)
}

/// Skirt vertex `vertex` (of [`SKIRT_VERTICES`]) of a tile, face-local (x, height, z in
/// chunk-local metres), as `heightfield_vs` places it past the grid's vertices: the vertical
/// quad from the column's surface edge (its two corners' [`corner_height`]s) down to its
/// [`skirt_bottom`] on that side; none (collapsed) where the side has no skirt. `floors` is each
/// texel's [`column_floor`]. The shader's reference.
pub fn skirt_vertex(
    levels: &[f32],
    floors: &[f32],
    join_m: f32,
    vertex: usize,
) -> Option<[f32; 3]> {
    let column = vertex / SKIRT_COLUMN_VERTICES;
    let side = (vertex / COLUMN_VERTICES) % SIDES.len();
    let (cx, cz) = ((column % COLUMNS) as i32, (column / COLUMNS) as i32);
    let texel = |x: i32, z: i32| (x + 1) as usize + TILE * (z + 1) as usize;
    let own = texel(cx, cz);
    let [dx, dz] = SIDES[side];
    let bottom = skirt_bottom(
        levels[own],
        floors[own],
        levels[texel(cx + dx, cz + dz)],
        join_m,
    )?;
    let [a, b] = skirt_edge(side);
    let corner = [a, b, b, a, b, a][vertex % COLUMN_VERTICES];
    let up = [true, true, false, true, false, false][vertex % COLUMN_VERTICES];
    let (i, j) = (cx as usize + corner[0], cz as usize + corner[1]);
    let l = |x: usize, z: usize| levels[x + TILE * z];
    let top = corner_height([l(i, j), l(i + 1, j), l(i, j + 1), l(i + 1, j + 1)], join_m);
    Some([i as f32, if up { top } else { bottom.min(top) }, j as f32])
}

/// The two corners (offsets from the column's own corner) of side `side`'s edge.
pub fn skirt_edge(side: usize) -> [[usize; 2]; 2] {
    match side {
        0 => [[0, 0], [0, 1]],
        1 => [[1, 0], [1, 1]],
        2 => [[0, 0], [1, 0]],
        _ => [[0, 1], [1, 1]],
    }
}

/// Whether the skirts draw the side face of liquid cell `cell` (face-local, chunk-local) toward
/// neighbour column `beside` (padded x, z), so the chunk mesher leaves it out while the
/// heightfield draws: the neighbour cell is open air (`open`), the cell is in its column's top
/// water ([`crate::mesher::water_runs`]), and it lies wholly above that side's skirt bottom.
pub(crate) fn skirt_covers(
    runs: &[Option<crate::mesher::WaterRun>],
    [x, y, z]: [i32; 3],
    [bx, bz]: [i32; 2],
    open: bool,
) -> bool {
    let run = |x: i32, z: i32| runs[(x + 1) as usize + TILE * (z + 1) as usize];
    let Some(own) = run(x, z) else { return false };
    if !open || y < own.floor || y > own.top {
        return false;
    }
    let beside = run(bx, bz).map_or(DRY, |r| r.level);
    let floor = own.floor as f32 * dwell_world::CELL_H as f32;
    let join = dwell_look::water_tiers::HEIGHTFIELD.join_m as f32;
    skirt_bottom(own.level, floor, beside, join)
        .is_some_and(|b| y as f32 * dwell_world::CELL_H as f32 >= b - 1e-4)
}

#[cfg(test)]
#[path = "heightfield_tests.rs"]
mod tests;
