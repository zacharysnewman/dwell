//! Procedural terrain, generator version 1016 (ARCHITECTURE.md §6.3; on the half-height branch,
//! 1000 + main's version 16): the interim baseline of Phase
//! 15 — one island at the origin, a 2D height field, and the ground filled by depth — and since
//! version 13 (Phase 33) the decorations of `shared/worldgen/decorations.json` one cell above the
//! ground (tall grass; `decorations.rs`), and since version 14 (Phase 39, HALF_HEIGHT_GRID.md) ½ m
//! cells with natural tops shaped from stored cardinal ramps (`ramps.rs`). A chunk is a
//! pure function of (world seed, chunk coordinate), so chunks generate in any order, on any thread,
//! natively or in WASM, bit-identically (noise.rs, ADR 0010).
//!
//! Per chunk: (1) layout: the signed distance to the coast; (2) height: the sea floor falling from
//! the coast, or the land rising inland, plus hills, never below the core; (3) fill: the stone core
//! (`CORE_LAYERS` from the midplane, never carved), stone up to the surface under grass over dirt
//! on land, sand at the shore and under water, water in open space below sea level. 2D fields are
//! sampled every 4 columns and interpolated. Heights are face-local, from the midplane (Phase 16:
//! the midplane at 0, sea level `h` = 32). The height field's parameters are data
//! (`shared/worldgen/terrain.json`, `HeightField`); the island's layout and the fill depths are
//! still code constants. Phase 17 replaces this with an evaluator of terrain definitions (data,
//! ADR 0026). Nothing is generated outside the build volume.

use dwell_blocks::StateId;
use dwell_world::hash::HashMap;
use dwell_world::lod::{self, LodBounds, LodCells, LodCoord, LodKind, LodSurface, LodSurfaces};
use dwell_world::ramps::{self, RampFacing};
use dwell_world::{
    BUILD_VOLUME, CELL_H, CHUNK_SIZE, CORE_LAYERS, Chunk, ChunkCoord, DiscOverlap, M, MIDPLANE_Y,
    Overlap, SEA_LEVEL, chunk_disc_overlap, inside_world_disc, local_index, m_of_row, rows_for_m,
};
use std::sync::Arc;

use crate::decorations::{DECORATIONS, Placer};
use crate::island::{self, IslandLayout};
use crate::noise::{
    clamp01, fbm2, floor_div, floor_mod, floor_to_int, lerp, noise_offset, octaves_resolved,
    seed_word, smooth_step,
};
use crate::ramps as ramps_rule;
use crate::ramps::Top;

/// A generator works face-locally: its band is [FACE_MIN_Y, FACE_MAX_Y), from the midplane to the
/// build volume's top (the sphere is symmetric about the midplane, so both faces' bands are equal).
const FACE_MIN_Y: i32 = MIDPLANE_Y;
pub const FACE_MAX_Y: i32 = BUILD_VOLUME.y_range().1 as i32;
const _: () = assert!(
    BUILD_VOLUME.y_range().0 + BUILD_VOLUME.y_range().1 == 2 * MIDPLANE_Y as i64,
    "the build volume is symmetric about the midplane"
);
/// Lattice spacing of the interpolated 2D fields (columns, m).
const LATTICE: i32 = 4;
const LATTICE_STEP: f32 = 0.25;
/// The height field's parameters (data: `shared/worldgen/terrain.json`, generated into
/// `terrain.gen.rs` by `shared/worldgen/gen.mjs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightField {
    /// The land's rise inland (m, approached far inland) and the distance (m) at which it is half
    /// done.
    pub inland_rise: f32,
    pub rise_half: f32,
    /// The sea floor's depth offshore (m below sea level, negative), reached linearly over `shelf`
    /// (m).
    pub sea_floor: f32,
    pub shelf: f32,
    /// Hills on land (about ±`hill_height` m; a fractal sum of this wavelength in m and octaves),
    /// faded in from `hill_fade_from` to `hill_fade_to` m inland.
    pub hill_height: f32,
    pub hill_wavelength: i32,
    pub hill_octaves: u32,
    pub hill_fade_from: f32,
    pub hill_fade_to: f32,
    /// Small bumps everywhere (about ±`bump_height` m).
    pub bump_height: f32,
    pub bump_wavelength: i32,
    pub bump_octaves: u32,
    /// Half-height branch (HALF_HEIGHT_GRID.md §4): where a lattice cell's ground is at least this
    /// steep (m per m), heights snap to whole multiples of `step_height` (m; 0: off), so steep
    /// ground is whole steps that the generation rule tops with 45° ramps.
    pub step_from_slope: f32,
    pub step_height: f32,
}

mod table {
    include!("terrain.gen.rs");
}
pub use table::HEIGHT;

const H: HeightField = HEIGHT;
/// Fill by depth: sand at the shore (top voxel below SHORE_HEIGHT above sea) to SAND_DEPTH, else
/// grass over dirt to DIRT_DEPTH; stone below. In rows, from metres (2 m, 3 m, 3 m).
const SHORE_HEIGHT: i32 = (2.0 / CELL_H) as i32;
const SAND_DEPTH: i64 = (3.0 / CELL_H) as i64;
const DIRT_DEPTH: i64 = (3.0 / CELL_H) as i64;

/// The spawn point's level patch reaches this many columns either side.
pub const SPAWN_CLEAR: i32 = 4;

/// 2D fields of one column.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Signed distance to the coast (m): > 0 land, < 0 sea.
    pub coast: f32,
    /// Ground height (m, face-local from the midplane): the top voxel is the row ceil(height ÷
    /// cell height) − 1.
    pub height: f32,
    /// Beyond the world's disc: nothing is generated.
    pub outside: bool,
}

/// A height (m) in rows of cells (HALF_HEIGHT_GRID.md §2); exact at 1 m and ½ m cells.
fn rows(h: f32) -> f32 {
    h / CELL_H as f32
}

/// The steepest edge of a lattice cell whose corners' heights (m) are `h` (corner order as
/// `interp`'s: (0, 0), (1, 0), (0, 1), (1, 1)), in m per m.
fn lattice_slope(h: [f32; 4]) -> f32 {
    let d = [h[1] - h[0], h[3] - h[2], h[2] - h[0], h[3] - h[1]];
    d.iter().fold(0.0f32, |m, v| m.max(v.abs())) / LATTICE as f32
}

/// A height (m) snapped to the nearest whole step (`step_height`).
fn snap_to_step(h: f32) -> f32 {
    (h / H.step_height).round() * H.step_height
}

/// The top voxel (row) of a column of height h (m): ceil(h in rows) − 1.
fn top_voxel(h: f32) -> i32 {
    let r = rows(h);
    let f = floor_to_int(r);
    if f as f32 == r { f - 1 } else { f }
}

/// The material `depth` m below the surface of a column whose top voxel is `top`: the core's stone
/// below `CORE_LAYERS`.
fn fill(top: i32, depth: i64) -> StateId {
    if i64::from(top) - depth < i64::from(MIDPLANE_Y + CORE_LAYERS) {
        return M.stone;
    }
    if top < SEA_LEVEL + SHORE_HEIGHT {
        return if depth < SAND_DEPTH { M.sand } else { M.stone };
    }
    if depth == 0 {
        M.grass
    } else if depth <= DIRT_DEPTH {
        M.dirt
    } else {
        M.stone
    }
}

/// The noise fields of a point, before the height is derived from them.
#[derive(Clone, Copy, Debug, Default)]
struct Corner {
    coast: f32,
    hills: f32,
    bumps: f32,
}

#[derive(Clone, Debug)]
pub struct TerrainGenerator {
    hills_seed: u32,
    bumps_seed: u32,
    offset_x: i64,
    offset_z: i64,
    island: IslandLayout,
    /// The decorations (data), resolved for this world.
    decorations: Vec<Placer>,
    /// The stored ramps natural tops are shaped with (HALF_HEIGHT_GRID.md §4); empty (no shaping)
    /// on 1 m cells.
    ramps: Arc<RampPieces>,
}

/// The ramp pieces of the natural surface materials, resolved by name once: for each material,
/// facing and whether flooded, a ½ m ramp's cell and a 1 m ramp's cells from the bottom.
#[derive(Debug, Default)]
struct RampPieces {
    by: HashMap<(StateId, RampFacing, bool), (StateId, Vec<StateId>)>,
}

impl RampPieces {
    fn resolve() -> Self {
        let mut by = HashMap::default();
        for m in [M.grass, M.sand, M.dirt, M.stone] {
            let base = dwell_blocks::block_of(m).name;
            for facing in RampFacing::ALL {
                for flooded in [false, true] {
                    let gentle = ramps::gentle_cell_flooded(base, facing, flooded);
                    let steep = ramps::steep_cells_flooded(base, facing, flooded);
                    if let (Some(g), Some(s)) = (gentle, steep) {
                        by.insert((m, facing, flooded), (g, s));
                    }
                }
            }
        }
        Self { by }
    }

    /// The cells (from the bottom) that top a column of `material` shaped `top`; empty for a flat
    /// top, or where the material or the grid has no ramps.
    fn cells(&self, material: StateId, top: Top, flooded: bool) -> &[StateId] {
        let (facing, steep) = match top {
            Top::Flat => return &[],
            Top::Gentle(f) => (f, false),
            Top::Steep(f) => (f, true),
        };
        match self.by.get(&(material, facing, flooded)) {
            Some((g, _)) if !steep => std::slice::from_ref(g),
            Some((_, s)) => s,
            None => &[],
        }
    }
}

/// How far above the ground decorations reach (rows): the sky test leaves this much room over the
/// highest ground before a chunk is all air.
const DECORATION_RISE: f32 = 1.0;

impl TerrainGenerator {
    pub fn new(world_seed: u64) -> Self {
        Self {
            hills_seed: seed_word(world_seed, 1),
            bumps_seed: seed_word(world_seed, 2),
            offset_x: noise_offset(seed_word(world_seed, 3)),
            offset_z: noise_offset(seed_word(world_seed, 4)),
            island: IslandLayout::new(world_seed),
            decorations: Placer::resolve(&DECORATIONS, world_seed),
            ramps: Arc::new(RampPieces::resolve()),
        }
    }

    fn sample_corner(&self, lx: i32, lz: i32) -> Corner {
        let x = i64::from(lx) * i64::from(LATTICE);
        let z = i64::from(lz) * i64::from(LATTICE);
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        Corner {
            coast: self.island.coast_at(x, z, None),
            hills: fbm2(
                self.hills_seed,
                nx,
                nz,
                H.hill_wavelength,
                H.hill_octaves,
                None,
            ),
            bumps: fbm2(
                self.bumps_seed,
                nx,
                nz,
                H.bump_wavelength,
                H.bump_octaves,
                None,
            ),
        }
    }

    fn sample_lod(&self, x: i64, z: i64, cell: i64) -> Corner {
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        Corner {
            coast: self.island.coast_at(
                x,
                z,
                Some(octaves_resolved(
                    island::COAST_WAVELENGTH,
                    island::COAST_OCTAVES,
                    cell,
                )),
            ),
            hills: fbm2(
                self.hills_seed,
                nx,
                nz,
                H.hill_wavelength,
                H.hill_octaves,
                Some(octaves_resolved(H.hill_wavelength, H.hill_octaves, cell)),
            ),
            bumps: fbm2(
                self.bumps_seed,
                nx,
                nz,
                H.bump_wavelength,
                H.bump_octaves,
                Some(octaves_resolved(H.bump_wavelength, H.bump_octaves, cell)),
            ),
        }
    }

    fn finish(c: Corner) -> Column {
        // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
        let base = if c.coast > 0.0 {
            H.inland_rise * (c.coast / (c.coast + H.rise_half))
        } else {
            H.sea_floor * clamp01(-c.coast * (1.0 / H.shelf))
        };
        let land = smooth_step(H.hill_fade_from, H.hill_fade_to, c.coast);
        let relief = base + land * H.hill_height * 2.0 * c.hills + H.bump_height * 2.0 * c.bumps;
        Column {
            coast: c.coast,
            // Never below the core's top.
            height: (dwell_world::SEA_LEVEL_M as f32 + relief)
                .max((dwell_world::MIDPLANE_M + dwell_world::CORE_M) as f32),
            outside: false,
        }
    }

    fn interp(c: &[Corner; 4], fx: i32, fz: i32) -> Column {
        let tx = fx as f32 * LATTICE_STEP;
        let tz = fz as f32 * LATTICE_STEP;
        let bi = |f: fn(&Corner) -> f32| {
            lerp(
                lerp(f(&c[0]), f(&c[1]), tx),
                lerp(f(&c[2]), f(&c[3]), tx),
                tz,
            )
        };
        let mut col = Self::finish(Corner {
            coast: bi(|c| c.coast),
            hills: bi(|c| c.hills),
            bumps: bi(|c| c.bumps),
        });
        if H.step_height > 0.0 {
            let h = c.map(|c| Self::finish(c).height);
            if lattice_slope(h) >= H.step_from_slope {
                col.height = snap_to_step(col.height);
            }
        }
        col
    }

    /// A column with exactly the chunk path's arithmetic (spawn, tools, tests).
    pub fn column_at(&self, x: i32, z: i32) -> Column {
        let lx = floor_div(x, LATTICE);
        let lz = floor_div(z, LATTICE);
        let c = [
            self.sample_corner(lx, lz),
            self.sample_corner(lx + 1, lz),
            self.sample_corner(lx, lz + 1),
            self.sample_corner(lx + 1, lz + 1),
        ];
        let mut col = Self::interp(&c, floor_mod(x, LATTICE), floor_mod(z, LATTICE));
        col.outside = !inside_world_disc(i64::from(x), i64::from(z));
        col
    }

    pub fn solid_at(&self, x: i32, y: i32, z: i32) -> bool {
        if !(FACE_MIN_Y..FACE_MAX_Y).contains(&y)
            || !BUILD_VOLUME.contains_voxel(i64::from(x), i64::from(y), i64::from(z))
        {
            return false;
        }
        let col = self.column_at(x, z);
        !col.outside && y <= top_voxel(col.height)
    }

    /// The top solid voxel of a column inside the disc.
    pub fn ground_y(&self, x: i32, z: i32) -> Option<i32> {
        let col = self.column_at(x, z);
        (!col.outside).then(|| top_voxel(col.height).clamp(FACE_MIN_Y, FACE_MAX_Y - 1))
    }

    /// The state `generate` puts at (x, y, z) (face-local), from the column alone.
    pub fn state_at(&self, x: i32, y: i32, z: i32) -> StateId {
        Self::state_in(&self.column_at(x, z), y)
    }

    fn state_in(col: &Column, y: i32) -> StateId {
        if col.outside || !(FACE_MIN_Y..FACE_MAX_Y).contains(&y) {
            return M.air;
        }
        let top = top_voxel(col.height);
        if y <= top {
            fill(top, i64::from(top - y))
        } else if y < SEA_LEVEL {
            M.water
        } else {
            M.air
        }
    }

    /// The height (m) of the ground's surface at the centre of column (x, z) (face-local, inside the
    /// disc): the top of a flat column, or the middle of the ramp the generation rule tops it with.
    pub fn surface_y_at(&self, x: i32, z: i32) -> Option<f64> {
        let g = self.ground_y(x, z)?;
        let pieces = self.pieces_with(|nx, nz| self.column_at(nx, nz), x, z, g);
        let first = g + 1 - pieces.len() as i32;
        let mut top = m_of_row(g + 1);
        for (k, &p) in pieces.iter().enumerate() {
            let span =
                dwell_blocks::shape::solid_span_at(dwell_blocks::shape::shape_of(p), 0.5, 0.5);
            if span.hi > span.lo {
                top = m_of_row(first + k as i32) + f64::from(span.hi) * CELL_H;
            }
        }
        Some(top)
    }

    /// The island's coast distance at the point itself (no lattice).
    pub fn coast_at(&self, x: i32, z: i32) -> f32 {
        self.island.coast_at(i64::from(x), i64::from(z), None)
    }

    /// Columns of a chunk (S × S, row-major from (x0, z0)).
    fn chunk_columns(&self, x0: i32, z0: i32) -> Vec<Column> {
        const S: i32 = CHUNK_SIZE;
        const NL: i32 = S / LATTICE + 1;
        let lx0 = floor_div(x0, LATTICE);
        let lz0 = floor_div(z0, LATTICE);
        let mut corners = Vec::with_capacity((NL * NL) as usize);
        for j in 0..NL {
            for i in 0..NL {
                corners.push(self.sample_corner(lx0 + i, lz0 + j));
            }
        }
        let at = |i: i32, j: i32| corners[(j * NL + i) as usize];
        let mut cols = Vec::with_capacity((S * S) as usize);
        for z in 0..S {
            for x in 0..S {
                let (i, j) = (x / LATTICE, z / LATTICE);
                let c = [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)];
                let mut col = Self::interp(&c, x % LATTICE, z % LATTICE);
                col.outside = !inside_world_disc(i64::from(x0 + x), i64::from(z0 + z));
                cols.push(col);
            }
        }
        cols
    }

    /// The highest ground of the columns inside the disc, in rows, with the decorations' room.
    fn sky_floor(cols: &[Column]) -> f32 {
        rows(
            cols.iter()
                .filter(|c| !c.outside)
                .fold(-1e9f32, |top, c| top.max(c.height)),
        ) + DECORATION_RISE
    }

    /// The sky test's height (rows) for a chunk column: chunks with y0 above it (and at or above
    /// sea level) are air. Depends only on (cx, cz), so callers cache it per column.
    pub fn sky_floor_at(&self, cx: i32, cz: i32) -> f32 {
        Self::sky_floor(&self.chunk_columns(cx * CHUNK_SIZE, cz * CHUNK_SIZE))
    }

    /// `is_air_chunk` given `sky_floor_at(coord.x, coord.z)`.
    pub fn is_air_chunk_with(coord: ChunkCoord, sky_floor: f32) -> bool {
        let y0 = coord.y * CHUNK_SIZE;
        if y0 + CHUNK_SIZE <= FACE_MIN_Y || y0 >= FACE_MAX_Y {
            return true;
        }
        if chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
            || !BUILD_VOLUME.chunk_may_hold(coord)
        {
            return true;
        }
        y0 >= SEA_LEVEL && y0 as f32 > sky_floor
    }

    /// True when `generate` would leave the chunk all air: exactly the generator's own shortcut.
    pub fn is_air_chunk(&self, coord: ChunkCoord) -> bool {
        let y0 = coord.y * CHUNK_SIZE;
        if !(SEA_LEVEL..FACE_MAX_Y).contains(&y0)
            || chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
            || !BUILD_VOLUME.chunk_may_hold(coord)
        {
            return Self::is_air_chunk_with(coord, 0.0);
        }
        Self::is_air_chunk_with(coord, self.sky_floor_at(coord.x, coord.z))
    }

    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        if y0 + S <= FACE_MIN_Y || y0 >= FACE_MAX_Y {
            return;
        }
        // Beyond the rim of the world's disc or outside the build volume: nothing.
        let volume = BUILD_VOLUME.chunk_overlap(coord);
        if chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
            || volume == Overlap::Outside
        {
            return;
        }
        let cols = self.chunk_columns(x0, z0);
        // Sky above all ground and sea: all air (`is_air_chunk` is this same test).
        if y0 as f32 > Self::sky_floor(&cols) && y0 >= SEA_LEVEL {
            return;
        }
        let voxels = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside {
                    continue;
                }
                let top = top_voxel(col.height);
                // The stored ramp pieces over the top (HALF_HEIGHT_GRID.md §4), from the bottom.
                let pieces = self.top_pieces(&cols, x0, z0, x, z, top);
                let first_piece = top + 1 - pieces.len() as i32;
                for ly in 0..S {
                    let y = y0 + ly;
                    if !(FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                        || (volume == Overlap::Partial
                            && !BUILD_VOLUME.contains_voxel(
                                i64::from(x0 + x),
                                i64::from(y),
                                i64::from(z0 + z),
                            ))
                    {
                        continue;
                    }
                    let v = &mut voxels[local_index(x, ly, z)];
                    if y <= top && y >= first_piece {
                        *v = pieces[(y - first_piece) as usize];
                    } else if y <= top {
                        *v = fill(top, i64::from(top - y));
                    } else if y < SEA_LEVEL {
                        *v = M.water;
                    }
                }
                // A decoration in the air cell over the ground, if this chunk holds that cell.
                let above = top + 1;
                if (y0..y0 + S).contains(&above)
                    && above >= SEA_LEVEL
                    && (FACE_MIN_Y..FACE_MAX_Y).contains(&above)
                    && let Some(m) = self.decoration_at(&cols, x0, z0, x, z, top)
                {
                    voxels[local_index(x, above - y0, z)] = m;
                }
            }
        }
    }

    /// The ramp pieces (from the bottom) that top column (x0 + lx, z0 + lz), whose top is `top`, by
    /// the generation rule over its neighbours' tops; empty for a flat top. Neighbours beyond the
    /// chunk's columns come from `column_at`, so the answer doesn't depend on the chunk asking.
    fn top_pieces(
        &self,
        cols: &[Column],
        x0: i32,
        z0: i32,
        lx: i32,
        lz: i32,
        top: i32,
    ) -> &[StateId] {
        const S: i32 = CHUNK_SIZE;
        let column = |nx: i32, nz: i32| {
            let (ix, iz) = (nx - x0, nz - z0);
            if (0..S).contains(&ix) && (0..S).contains(&iz) {
                cols[(iz * S + ix) as usize]
            } else {
                self.column_at(nx, nz)
            }
        };
        self.pieces_with(column, x0 + lx, z0 + lz, top)
    }

    /// [`Self::top_pieces`] for column (x, z), its neighbours' columns from `column`.
    fn pieces_with(
        &self,
        column: impl Fn(i32, i32) -> Column,
        x: i32,
        z: i32,
        top: i32,
    ) -> &[StateId] {
        if self.ramps.by.is_empty() {
            return &[];
        }
        let height = |nx: i32, nz: i32| {
            let col = column(nx, nz);
            // Beyond the rim, nothing: lower than any ground.
            if col.outside {
                i32::MIN / 2
            } else {
                top_voxel(col.height) + 1
            }
        };
        let around: ramps_rule::Around = std::array::from_fn(|r| {
            std::array::from_fn(|c| height(x + c as i32 - 1, z + r as i32 - 1))
        });
        let shape = ramps_rule::top_of(&around, rows_for_m(1.0));
        // Under water (the cell above the top is sea), the pieces hold it in their open part.
        let flooded = top + 1 < SEA_LEVEL;
        let cells = self.ramps.cells(fill(top, 0), shape, flooded);
        // A 1 m ramp needs two cells of ground; a column one cell deep takes none.
        if (cells.len() as i32) > top + 1 - FACE_MIN_Y {
            return &[];
        }
        cells
    }

    /// The decoration (if any) over the ground of column (x0 + lx, z0 + lz), whose top is `top`;
    /// `cols` are the chunk's columns, and columns beyond them come from `column_at` (the same
    /// arithmetic), so the answer doesn't depend on the chunk asking.
    fn decoration_at(
        &self,
        cols: &[Column],
        x0: i32,
        z0: i32,
        lx: i32,
        lz: i32,
        top: i32,
    ) -> Option<StateId> {
        const S: i32 = CHUNK_SIZE;
        // Plants grow on flat tops only (HALF_HEIGHT_GRID.md §5).
        if !self.top_pieces(cols, x0, z0, lx, lz, top).is_empty() {
            return None;
        }
        let (x, z) = (x0 + lx, z0 + lz);
        let ground = fill(top, 0);
        let neighbour_top = |nx: i32, nz: i32| {
            let (ix, iz) = (nx - x0, nz - z0);
            let col = if (0..S).contains(&ix) && (0..S).contains(&iz) {
                cols[(iz * S + ix) as usize]
            } else {
                self.column_at(nx, nz)
            };
            if col.outside {
                i32::MIN / 2
            } else {
                top_voxel(col.height)
            }
        };
        let noise = (i64::from(x) + self.offset_x, i64::from(z) + self.offset_z);
        self.decorations
            .iter()
            // The rules' heights are metres above sea level.
            .find(|p| {
                p.places((x, z), noise, ground, top - SEA_LEVEL, |nx, nz| {
                    neighbour_top(nx, nz) - SEA_LEVEL
                })
            })
            .map(Placer::state)
    }

    // --- level of detail (§6.6) -------------------------------------------------------------------

    fn column_lod(&self, x: i64, z: i64, cell: i64) -> Column {
        let mut col = Self::finish(self.sample_lod(x, z, cell));
        col.outside = !inside_world_disc(x, z);
        col
    }

    fn lod_columns(&self, o: lod::LodOrigin, level: i32) -> Vec<Column> {
        let cell = lod::cell_size(level);
        let half = cell / 2;
        let mut cols = vec![Column::default(); (lod::PAD * lod::PAD) as usize];
        for z in -1..=lod::SECTION_CELLS {
            for x in -1..=lod::SECTION_CELLS {
                cols[lod::column_index(x, z)] = self.column_lod(
                    o.x + i64::from(x) * cell + half,
                    o.z + i64::from(z) * cell + half,
                    cell,
                );
            }
        }
        cols
    }

    fn lod_bounds_of(cols: &[Column]) -> LodBounds {
        let mut b = LodBounds {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
            any_inside: false,
            ..LodBounds::default()
        };
        let mut all_inside = true;
        for col in cols {
            if col.outside {
                all_inside = false;
                continue;
            }
            b.any_inside = true;
            // A cell is solid when its bottom voxel lies below the column's height: samples below
            // `lo` are solid, at or above `hi` open.
            b.hi = b.hi.max(f64::from(rows(col.height)) + 1.0);
            b.lo = b.lo.min(f64::from(rows(col.height)) - 1.0);
        }
        if !b.any_inside {
            return b;
        }
        b.hi = b.hi.max(f64::from(SEA_LEVEL - 1));
        if !all_inside {
            b.lo = f64::NEG_INFINITY;
        }
        b
    }

    /// The column bounds sections are classified by (`lod::kind_from_bounds`).
    pub fn lod_bounds_at(&self, level: i32, i: i32, k: i32) -> LodBounds {
        let o = lod::section_origin(LodCoord::new(level, i, 0, k));
        Self::lod_bounds_of(&self.lod_columns(o, level))
    }

    /// A section evaluated at its cells' resolution: each cell samples the height field at its
    /// column's centre with octaves finer than a cell dropped, solid when its bottom voxel is. The
    /// apron below the face reads as stone.
    pub fn generate_lod(
        &self,
        c: LodCoord,
        cells: &mut LodCells,
        surface: Option<&mut LodSurfaces>,
    ) -> LodKind {
        if !lod::in_world(c) {
            cells.clear();
            cells.resize(lod::VOLUME, M.air);
            if let Some(s) = surface {
                s.clear();
                s.resize((lod::PAD * lod::PAD) as usize, LodSurface::default());
            }
            return LodKind::Empty;
        }
        self.generate_lod_at(c, lod::section_origin(c).y, cells, surface)
            .0
    }

    /// The same for the face-local section whose rows start at height `origin_y` (x, z, level from
    /// `c`). Returns the kind (this face's) and the section's column bounds.
    pub fn generate_lod_at(
        &self,
        c: LodCoord,
        origin_y: i64,
        cells: &mut LodCells,
        mut surface: Option<&mut LodSurfaces>,
    ) -> (LodKind, LodBounds) {
        cells.clear();
        cells.resize(lod::VOLUME, M.air);
        if let Some(s) = surface.as_deref_mut() {
            s.clear();
            s.resize((lod::PAD * lod::PAD) as usize, LodSurface::default());
        }
        let mut o = lod::section_origin(c);
        o.y = origin_y;
        let cell = lod::cell_size(c.level);
        let cols = self.lod_columns(o, c.level);
        let bounds = Self::lod_bounds_of(&cols);
        let kind = if bounds.any_inside {
            lod::kind_of_face(c.level, origin_y, bounds.lo, bounds.hi)
        } else {
            LodKind::Empty
        };
        match kind {
            LodKind::Empty => return (kind, bounds),
            LodKind::Buried => {
                cells.fill(M.stone);
                return (kind, bounds);
            }
            LodKind::Content => {}
        }
        let face_min = i64::from(FACE_MIN_Y);
        let sea = i64::from(SEA_LEVEL);
        for z in -1..=lod::SECTION_CELLS {
            for x in -1..=lod::SECTION_CELLS {
                let col = cols[lod::column_index(x, z)];
                if col.outside {
                    continue; // beyond the rim: the void
                }
                let top = i64::from(top_voxel(col.height));
                let is_sea = rows(col.height) < SEA_LEVEL as f32;
                for y in -1..=lod::SECTION_CELLS {
                    let a = o.y + i64::from(y) * cell;
                    let m = &mut cells[lod::cell_index(x, y, z)];
                    if a + cell <= face_min {
                        *m = M.stone; // the apron under the face
                        continue;
                    }
                    // A cell reaching below the face samples at the face's lowest voxel.
                    let at = a.max(face_min);
                    if at > top {
                        if at < sea {
                            *m = M.water;
                        }
                        continue;
                    }
                    let depth = (top - (a + cell - 1)).max(0);
                    if depth > 0 {
                        *m = fill(top as i32, depth.min(1000));
                        continue;
                    }
                    // The column's top: its surface seen from above. A sea whose water no cell
                    // sampled shows its water.
                    let water_above = a + cell < sea;
                    *m = if is_sea && !water_above {
                        M.water
                    } else {
                        fill(top as i32, 0)
                    };
                    if let Some(s) = surface.as_deref_mut() {
                        s[lod::column_index(x, z)] = LodSurface {
                            valid: true,
                            wet: is_sea,
                            height: rows(col.height).min((a + cell) as f32),
                            material: fill(top as i32, 0),
                            water: if is_sea { SEA_LEVEL as f32 } else { 0.0 },
                        };
                    }
                }
            }
        }
        (kind, bounds)
    }

    /// Feet position for players: the level dry ground nearest the origin (a square spiral in 8 m
    /// steps for a patch SPAWN_CLEAR columns either side at one height above the shore).
    pub fn spawn_point(&self) -> [f64; 3] {
        let mut fallback = None;
        let (mut x, mut z, mut dx, mut dz) = (0i32, 0i32, 1i32, 0i32);
        let (mut leg, mut walked, mut turns) = (1, 0, 0);
        for _ in 0..40_000 {
            if let Some(g) = self.ground_y(x, z)
                && g >= SEA_LEVEL + SHORE_HEIGHT
            {
                let feet = self.surface_y_at(x, z).unwrap_or(m_of_row(g + 1));
                let point = [f64::from(x) + 0.5, feet, f64::from(z) + 0.5];
                fallback.get_or_insert(point);
                let level = (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|pz| {
                    (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|px| self.ground_y(x + px, z + pz) == Some(g))
                });
                if level {
                    return point;
                }
            }
            x += dx * 8;
            z += dz * 8;
            walked += 1;
            if walked == leg {
                walked = 0;
                let t = dx;
                dx = -dz;
                dz = t;
                turns += 1;
                if turns % 2 == 0 {
                    leg += 1;
                }
            }
        }
        fallback.unwrap_or([0.5, m_of_row(SEA_LEVEL + 1), 0.5])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_tops_are_shaped_with_stored_ramps_only_on_half_height_cells() {
        // HALF_HEIGHT_GRID.md §4: on 1 m cells the generator writes what it always has (its
        // goldens are unchanged); on ½ m cells every natural surface material has its pieces, a
        // ½ m ramp in one cell and a 45° ramp in two.
        let pieces = RampPieces::resolve();
        if CELL_H == 1.0 {
            assert!(pieces.by.is_empty());
            return;
        }
        for m in [M.grass, M.sand, M.dirt, M.stone] {
            for facing in RampFacing::ALL {
                for flooded in [false, true] {
                    assert_eq!(pieces.cells(m, Top::Gentle(facing), flooded).len(), 1);
                    assert_eq!(pieces.cells(m, Top::Steep(facing), flooded).len(), 2);
                    assert!(pieces.cells(m, Top::Flat, flooded).is_empty());
                }
            }
        }
    }
}
