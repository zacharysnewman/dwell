//! Procedural terrain, generator version 17 (ARCHITECTURE.md §6.3): the interim baseline of Phase
//! 15 — one island at the origin, a 2D height field, and the ground filled by depth — and since
//! version 13 (Phase 33) the decorations of `shared/worldgen/decorations.json` one cell above the
//! ground (tall grass; `decorations.rs`). A chunk is a
//! pure function of (world seed, chunk coordinate), so chunks generate in any order, on any thread,
//! natively or in WASM, bit-identically (noise.rs, ADR 0010).
//!
//! Per chunk: (1) layout: the signed distance to the coast; (2) height: the sea floor falling from
//! the coast, or the land rising inland, plus hills, never below the core; (3) fill: the stone core
//! (`CORE_LAYERS` from the midplane, never carved), stone up to the surface under grass over dirt
//! on land, sand at the shore and under water, water in open space below sea level — since version
//! 17 also the open part of a top block below sea level, which takes its `flooded` state
//! (docs/WATER_RENDERING.md §9). 2D fields are
//! sampled every 4 columns and interpolated. Heights are face-local, from the midplane (Phase 16:
//! the midplane at 0, sea level `h` = 32). The height field's parameters are data
//! (`shared/worldgen/terrain.json`, `HeightField`); the island's layout and the fill depths are
//! still code constants. Phase 17 replaces this with an evaluator of terrain definitions (data,
//! ADR 0026). Nothing is generated outside the build volume.

use dwell_blocks::StateId;
use dwell_world::lod::{self, LodBounds, LodCells, LodCoord, LodKind, LodSurface, LodSurfaces};
use dwell_world::{
    BUILD_VOLUME, CHUNK_SIZE, CORE_LAYERS, Chunk, ChunkCoord, DiscOverlap, M, MIDPLANE_Y, Overlap,
    SEA_LEVEL, chunk_disc_overlap, inside_world_disc, local_index,
};

use crate::decorations::{DECORATIONS, Placer};
use crate::island::{self, IslandLayout};
use crate::noise::{
    clamp01, fbm2, floor_div, floor_mod, floor_to_int, lerp, noise_offset, octaves_resolved,
    seed_word, smooth_step,
};
use crate::placements::{self, Ground, PLACEMENTS, Planter, Tree};

/// A generator works face-locally: its band is [FACE_MIN_Y, FACE_MAX_Y), from the midplane to the
/// build volume's top (the sphere is symmetric about the midplane, so both faces' bands are equal).
const FACE_MIN_Y: i32 = MIDPLANE_Y;
pub const FACE_MAX_Y: i32 = BUILD_VOLUME.y_range().1 as i32;
const _: () = assert!(
    BUILD_VOLUME.y_range().0 + BUILD_VOLUME.y_range().1 == 2 * MIDPLANE_Y as i64,
    "the build volume is symmetric about the midplane"
);
/// Lattice spacing of the interpolated 2D fields (voxels).
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
}

mod table {
    include!("terrain.gen.rs");
}
pub use table::HEIGHT;

const H: HeightField = HEIGHT;
/// Fill by depth (m): sand at the shore (top voxel below SHORE_HEIGHT above sea) to SAND_DEPTH,
/// else grass over dirt to DIRT_DEPTH; stone below.
const SHORE_HEIGHT: i32 = 2;
const SAND_DEPTH: i64 = 3;
const DIRT_DEPTH: i64 = 3;

/// The spawn point's level patch reaches this many columns either side.
pub const SPAWN_CLEAR: i32 = 4;

/// 2D fields of one column.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Signed distance to the coast (m): > 0 land, < 0 sea.
    pub coast: f32,
    /// Ground height (m, face-local from the midplane): the top voxel is ceil(height) − 1.
    pub height: f32,
    /// Beyond the world's disc: nothing is generated.
    pub outside: bool,
}

/// The top voxel of a column of height h: ceil(h) − 1.
fn top_voxel(h: f32) -> i32 {
    let f = floor_to_int(h);
    if f as f32 == h { f - 1 } else { f }
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

/// The state the generator writes at row `y` of a column whose top voxel is `top`, filled with
/// `m`: the top voxel below sea level holds the sea in its open part (its `flooded` state, when
/// its block has one; docs/WATER_RENDERING.md §9), so a natural top block shaped below the water
/// line is under the water, not dry beside it.
fn under_sea(m: StateId, top: i32, y: i32) -> StateId {
    if y != top || y >= SEA_LEVEL {
        return m;
    }
    static FLOODED: std::sync::LazyLock<Vec<StateId>> = std::sync::LazyLock::new(|| {
        (0..dwell_blocks::STATES.len() as StateId)
            .map(|id| dwell_blocks::with_property(id, "flooded", "true").unwrap_or(id))
            .collect()
    });
    FLOODED.get(usize::from(m)).copied().unwrap_or(m)
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
    /// The structure placements (data), resolved for this world.
    planters: Vec<Planter>,
    /// How far above the ground decorations and placed structures reach (m): the sky test leaves
    /// this much room over the highest ground before a chunk is all air.
    rise: f32,
    world_seed: u64,
}

/// How far above the ground decorations reach (one cell).
const DECORATION_RISE: i32 = 1;

/// The LOD draws placed structures as blobs of their voxels at levels of cells this size (m) or
/// finer, and as a canopy tint on the ground at coarser ones.
const LOD_TREE_CELL: i64 = 2;

impl TerrainGenerator {
    pub fn new(world_seed: u64) -> Self {
        Self {
            hills_seed: seed_word(world_seed, 1),
            bumps_seed: seed_word(world_seed, 2),
            offset_x: noise_offset(seed_word(world_seed, 3)),
            offset_z: noise_offset(seed_word(world_seed, 4)),
            island: IslandLayout::new(world_seed),
            decorations: Placer::resolve(&DECORATIONS, world_seed),
            planters: Vec::new(),
            rise: DECORATION_RISE as f32,
            world_seed,
        }
        .with_placements(&PLACEMENTS)
    }

    /// The generator with these structure placements in place of the build's (tests add one by
    /// data alone).
    pub fn with_placements(mut self, rules: &[placements::Placement]) -> Self {
        self.planters = Planter::resolve(rules, self.world_seed, (self.offset_x, self.offset_z));
        // A structure anchored beside the chunk on ground up to its reach higher still reaches in.
        let reach = self.planters.iter().map(Planter::reach).max().unwrap_or(0);
        let rise = if self.planters.is_empty() {
            0
        } else {
            placements::max_rise(&self.planters) + 1 + reach
        };
        self.rise = DECORATION_RISE.max(rise) as f32;
        self
    }

    /// The structure placements, resolved.
    pub fn planters(&self) -> &[Planter] {
        &self.planters
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
            height: (SEA_LEVEL as f32 + relief).max((MIDPLANE_Y + CORE_LAYERS) as f32),
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
        Self::finish(Corner {
            coast: bi(|c| c.coast),
            hills: bi(|c| c.hills),
            bumps: bi(|c| c.bumps),
        })
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
            under_sea(fill(top, i64::from(top - y)), top, y)
        } else if y < SEA_LEVEL {
            M.water
        } else {
            M.air
        }
    }

    /// The height of the ground's surface at (x, z) (face-local, inside the disc): the top voxel's
    /// shape as the adaptive rules derive it (docs/ADAPTIVE_BLOCKS.md), at the column's centre — on
    /// level ground half a block above the top voxel's floor.
    pub fn surface_y_at(&self, x: i32, z: i32) -> Option<f64> {
        let g = self.ground_y(x, z)?;
        let mut cols = [[None; 3]; 3];
        let mut get = |dx: i32, dy: i32, dz: i32| {
            let c = cols[(dz + 1) as usize][(dx + 1) as usize]
                .get_or_insert_with(|| self.column_at(x + dx, z + dz));
            Self::state_in(c, g + dy)
        };
        let shape = dwell_blocks::adaptive::shape_at(&mut get, 1);
        let top = dwell_blocks::shape::solid_span_at(shape, 0.5, 0.5).hi;
        Some(f64::from(g) + f64::from(top))
    }

    /// The island's coast distance at the point itself (no lattice).
    pub fn coast_at(&self, x: i32, z: i32) -> f32 {
        self.island.coast_at(i64::from(x), i64::from(z), None)
    }

    /// Columns of a chunk (S × S, row-major from (x0, z0)).
    fn chunk_columns(&self, x0: i32, z0: i32) -> Vec<Column> {
        self.rect_columns(x0, z0, CHUNK_SIZE, CHUNK_SIZE)
    }

    /// Columns of a w × l rectangle (row-major from (x0, z0)), with exactly `column_at`'s
    /// arithmetic: the lattice corners sampled once and interpolated.
    fn rect_columns(&self, x0: i32, z0: i32, w: i32, l: i32) -> Vec<Column> {
        let lx0 = floor_div(x0, LATTICE);
        let lz0 = floor_div(z0, LATTICE);
        let nx = floor_div(x0 + w - 1, LATTICE) - lx0 + 2;
        let nz = floor_div(z0 + l - 1, LATTICE) - lz0 + 2;
        let mut corners = Vec::with_capacity((nx * nz) as usize);
        for j in 0..nz {
            for i in 0..nx {
                corners.push(self.sample_corner(lx0 + i, lz0 + j));
            }
        }
        let at = |i: i32, j: i32| corners[(j * nx + i) as usize];
        let mut cols = Vec::with_capacity((w * l) as usize);
        for z in z0..z0 + l {
            for x in x0..x0 + w {
                let (i, j) = (floor_div(x, LATTICE) - lx0, floor_div(z, LATTICE) - lz0);
                let c = [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)];
                let mut col = Self::interp(&c, floor_mod(x, LATTICE), floor_mod(z, LATTICE));
                col.outside = !inside_world_disc(i64::from(x), i64::from(z));
                cols.push(col);
            }
        }
        cols
    }

    /// The highest ground of the columns inside the disc, plus what grows on it.
    fn sky_floor(&self, cols: &[Column]) -> f32 {
        cols.iter()
            .filter(|c| !c.outside)
            .fold(-1e9f32, |top, c| top.max(c.height))
            + self.rise
    }

    /// The sky test's height for a chunk column: chunks with y0 above it (and at or above sea
    /// level) are air. Depends only on (cx, cz), so callers cache it per column.
    pub fn sky_floor_at(&self, cx: i32, cz: i32) -> f32 {
        self.sky_floor(&self.chunk_columns(cx * CHUNK_SIZE, cz * CHUNK_SIZE))
    }

    /// A column's ground as placements see it (`None` beyond the rim).
    fn ground_of(col: &Column) -> Option<Ground> {
        if col.outside {
            return None;
        }
        let top = top_voxel(col.height);
        // The top block as generated: below sea level it takes its flooded state.
        let block = under_sea(fill(top, 0), top, top);
        Some(Ground {
            top,
            above_sea: top - SEA_LEVEL,
            height: col.height,
            block,
            dry: top >= SEA_LEVEL && !dwell_blocks::info(block).flooded,
        })
    }

    /// The structures placed reaching columns [x0, x1] × [z0, z1] (face-local), the ground they
    /// ask for computed once for the rectangle and its margin.
    pub fn trees_in(&self, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) -> Vec<Tree> {
        if self.planters.is_empty() {
            return Vec::new();
        }
        let m = placements::ground_margin(&self.planters);
        let (rx, rz) = (x0 - m, z0 - m);
        let (w, l) = (x1 - x0 + 1 + 2 * m, z1 - z0 + 1 + 2 * m);
        let cols = self.rect_columns(rx, rz, w, l);
        let ground = |x: i32, z: i32| {
            let (i, j) = (x - rx, z - rz);
            if (0..w).contains(&i) && (0..l).contains(&j) {
                Self::ground_of(&cols[(j * w + i) as usize])
            } else {
                Self::ground_of(&self.column_at(x, z))
            }
        };
        placements::trees_in(&self.planters, (x0, z0), (x1, z1), &ground)
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
        if y0 as f32 > self.sky_floor(&cols) && y0 >= SEA_LEVEL {
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
                    if y <= top {
                        *v = under_sea(fill(top, i64::from(top - y)), top, y);
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
        self.stamp_trees(coord, &cols, volume, chunk);
    }

    /// Stands the placed structures reaching this chunk in it, over air and replaceable cells
    /// only (decorations; never the ground or water). Only chunks holding a row between the lowest
    /// ground and the highest crown look.
    fn stamp_trees(&self, coord: ChunkCoord, cols: &[Column], volume: Overlap, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        if self.planters.is_empty() {
            return;
        }
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for c in cols.iter().filter(|c| !c.outside) {
            let top = top_voxel(c.height);
            lo = lo.min(top);
            hi = hi.max(top);
        }
        let reach = self.rise as i32;
        if lo == i32::MAX || y0 + S <= lo - reach || y0 > hi + reach {
            return;
        }
        let trees = self.trees_in((x0, z0), (x0 + S - 1, z0 + S - 1));
        let voxels = chunk.voxels_mut();
        for t in &trees {
            let placed = self.planters[t.planter as usize].place(t);
            if placed.misses(coord) {
                continue;
            }
            for (p, state) in placed.cells() {
                let (lx, ly, lz) = (p[0] - x0, p[1] - y0, p[2] - z0);
                let inside = (0..S).contains(&lx)
                    && (0..S).contains(&ly)
                    && (0..S).contains(&lz)
                    && (FACE_MIN_Y..FACE_MAX_Y).contains(&p[1]);
                if !inside
                    || (volume == Overlap::Partial
                        && !BUILD_VOLUME.contains_voxel(
                            i64::from(p[0]),
                            i64::from(p[1]),
                            i64::from(p[2]),
                        ))
                {
                    continue;
                }
                let v = &mut voxels[local_index(lx, ly, lz)];
                if *v == M.air || dwell_blocks::info(*v).replaceable {
                    *v = state;
                }
            }
        }
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

    /// How far above its ground a level's sections draw placed structures (m): their full rise
    /// at the levels that draw them as blobs, none where they only tint the ground.
    fn lod_rise(&self, level: i32) -> f64 {
        if lod::cell_size(level) <= LOD_TREE_CELL && !self.planters.is_empty() {
            f64::from(self.rise)
        } else {
            0.0
        }
    }

    fn lod_bounds_of(cols: &[Column], rise: f64) -> LodBounds {
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
            b.hi = b.hi.max(f64::from(col.height) + 1.0 + rise);
            b.lo = b.lo.min(f64::from(col.height) - 1.0);
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
        Self::lod_bounds_of(&self.lod_columns(o, level), self.lod_rise(level))
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
        let bounds = Self::lod_bounds_of(&cols, self.lod_rise(c.level));
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
                let is_sea = col.height < SEA_LEVEL as f32;
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
                            height: col.height.min((a + cell) as f32),
                            material: fill(top as i32, 0),
                            water: if is_sea { SEA_LEVEL as f32 } else { 0.0 },
                        };
                    }
                }
            }
        }
        self.lod_trees(c.level, o, &cols, cells, surface);
        (kind, bounds)
    }

    /// The placed structures in a section: at levels of cells up to `LOD_TREE_CELL`, each tree
    /// the chunks hold (`trees_in`, exactly) fills the air cells whose centre column and rows its
    /// voxels reach; at coarser levels a share of the columns as large as the trees' cover shows
    /// the canopy's block on its top cell.
    fn lod_trees(
        &self,
        level: i32,
        o: lod::LodOrigin,
        cols: &[Column],
        cells: &mut LodCells,
        mut surface: Option<&mut LodSurfaces>,
    ) {
        if self.planters.is_empty() {
            return;
        }
        let cell = lod::cell_size(level);
        let half = cell / 2;
        let n = lod::SECTION_CELLS;
        let centre = |o: i64, i: i32| (o + i64::from(i) * cell + half) as i32;
        if cell <= LOD_TREE_CELL {
            let (x0, z0) = (centre(o.x, -1), centre(o.z, -1));
            let (x1, z1) = (centre(o.x, n), centre(o.z, n));
            for t in self.trees_in((x0, z0), (x1, z1)) {
                let p = &self.planters[t.planter as usize];
                let placed = p.place(&t);
                let fp = p.footprint(&t);
                for zi in -1..=n {
                    let wz = centre(o.z, zi);
                    if wz < fp[1] || wz > fp[3] {
                        continue;
                    }
                    for xi in -1..=n {
                        let wx = centre(o.x, xi);
                        if wx < fp[0] || wx > fp[2] {
                            continue;
                        }
                        for yi in -1..=n {
                            let a = o.y + i64::from(yi) * cell;
                            if a + cell <= i64::from(placed.min[1]) || a > i64::from(placed.max[1])
                            {
                                continue;
                            }
                            let m = &mut cells[lod::cell_index(xi, yi, zi)];
                            if *m != M.air {
                                continue;
                            }
                            if let Some(s) =
                                (a..a + cell).find_map(|y| placed.at([wx, y as i32, wz]))
                            {
                                *m = s;
                            }
                        }
                    }
                }
            }
            return;
        }
        for zi in -1..=n {
            for xi in -1..=n {
                let col = cols[lod::column_index(xi, zi)];
                let Some(g) = Self::ground_of(&col) else {
                    continue;
                };
                let h = |dx: i32, dz: i32| {
                    let (x, z) = ((xi + dx).clamp(-1, n), (zi + dz).clamp(-1, n));
                    cols[lod::column_index(x, z)].height
                };
                let run = 2.0 * cell as f32;
                let gx = (h(1, 0) - h(-1, 0)) / run;
                let gz = (h(0, 1) - h(0, -1)) / run;
                let (wx, wz) = (centre(o.x, xi), centre(o.z, zi));
                let Some(p) = self
                    .planters
                    .iter()
                    .find(|p| p.canopy_at((wx, wz), level, g, gx * gx + gz * gz))
                else {
                    continue;
                };
                // The column's topmost solid cell in the section, if its top is the ground's.
                let Some(yi) = (-1..=n).rev().find(|&y| {
                    let m = cells[lod::cell_index(xi, y, zi)];
                    m != M.air && m != M.water
                }) else {
                    continue;
                };
                let a = o.y + i64::from(yi) * cell;
                if i64::from(g.top) >= a + cell {
                    continue; // buried: the ground goes on above this section
                }
                cells[lod::cell_index(xi, yi, zi)] = p.canopy();
                if let Some(s) = surface.as_deref_mut() {
                    let sc = &mut s[lod::column_index(xi, zi)];
                    if sc.valid && !sc.wet {
                        sc.material = p.canopy();
                    }
                }
            }
        }
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
                // On the shaped surface (a level patch's top blocks rest at half height).
                let feet = self.surface_y_at(x, z).unwrap_or(f64::from(g) + 1.0);
                let point = [f64::from(x) + 0.5, feet, f64::from(z) + 0.5];
                fallback.get_or_insert(point);
                let level = (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|pz| {
                    (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|px| self.ground_y(x + px, z + pz) == Some(g))
                });
                // Never in (or under) a placed structure: the patch is clear of their boxes.
                if level
                    && self
                        .trees_in(
                            (x - SPAWN_CLEAR, z - SPAWN_CLEAR),
                            (x + SPAWN_CLEAR, z + SPAWN_CLEAR),
                        )
                        .is_empty()
                {
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
        fallback.unwrap_or([0.5, f64::from(SEA_LEVEL) + 1.0, 0.5])
    }
}
