//! Procedural terrain, generator version 21 (ARCHITECTURE.md §6.3, ADR 0049): mountains, rivers and
//! lakes from the world map. Each face's map (`worldmap`: the base field sampled on a 100 m grid,
//! mountain ranges lifted by the uplift layer, eroded, its lakes and rivers found) is computed once
//! per face seed; every column is shaped from it here:
//!
//! 1. **Height**: the map's eroded height, Catmull-Rom bicubic between its cells (C1, no creases),
//!    plus fine detail noise (`terrain.json` `detail`), stronger on mountains and faded out near
//!    water.
//! 2. **Lakes**: inside a lake's shore (the map's lake cells, a smooth signed distance) water stands
//!    flat at its level over the ground below it; outside, the ground stands at least `bank` over
//!    the level nearby, so the water never stands open beside lower ground.
//! 3. **Rivers**: the nearest channel (the map's polylines, smoothed, `worldmap::channels`) cuts its
//!    bed below its water and a valley down to its banks; the water's surface falls along the river
//!    in ½ m terraces, small falls where it is steep.
//! 4. **The sea**: open space below sea level is water, as always.
//! 5. **Materials** by height and slope (`bands`): snow over the snow line, bare rock on steep
//!    ground, sand at the shore and under water, grass over dirt elsewhere.
//!
//! Then, as before (since version 13–20): decorations one cell above the ground
//! (`decorations.rs`), ½ m cells with natural tops shaped from stored cardinal ramps (`ramps.rs`,
//! HALF_HEIGHT_GRID.md §4), the stone core and the bedrock under each face, structures
//! (`placements.rs`) and the portals between the faces (`portals.rs`). A chunk is a pure function of
//! (world seed, chunk coordinate) given the map, which is a pure function of the seed, so chunks
//! generate in any order, on any thread, natively or in WASM, bit-identically (ADR 0010). Heights
//! are face-local, from the midplane (sea level `h` = 32 m). Nothing is generated outside the build
//! volume.

use dwell_blocks::StateId;
use dwell_world::hash::HashMap;
use dwell_world::lod::{self, LodBounds, LodCells, LodCoord, LodKind, LodSurface, LodSurfaces};
use dwell_world::ramps::{self, RampFacing};
use dwell_world::{
    BUILD_VOLUME, CELL_H, CHUNK_SIZE, CORE_LAYERS, Chunk, ChunkCoord, DiscOverlap, M, MIDPLANE_Y,
    Overlap, SEA_LEVEL, SEA_LEVEL_M, chunk_disc_overlap, inside_world_disc, local_index, m_of_row,
    rows_for_m,
};
use std::sync::Arc;

use crate::base::BaseField;
use crate::decorations::{DECORATIONS, Placer};
use crate::noise::{fbm2, floor_to_int, octaves_resolved, seed_word, smooth_step};
use crate::placements::{self, Ground, PLACEMENTS, Planter, Tree};
use crate::portals::{MIDPLANE, Part, Portal, Portals};
use crate::ramps as ramps_rule;
use crate::ramps::Top;
use crate::volume::{ColumnInput, Volume, VolumeDef};
use crate::worldmap::{self, WorldMap};
use dwell_world::structure::Placed;

/// A generator works face-locally: its band is [FACE_MIN_Y, FACE_MAX_Y), from the midplane to the
/// build volume's top (the sphere is symmetric about the midplane, so both faces' bands are equal).
const FACE_MIN_Y: i32 = MIDPLANE_Y;
pub const FACE_MAX_Y: i32 = BUILD_VOLUME.y_range().1 as i32;
const _: () = assert!(
    // face-boundary: the build volume is the frame's own mirror image
    BUILD_VOLUME.y_range().0 + BUILD_VOLUME.y_range().1 == 2 * MIDPLANE_Y as i64,
    "the build volume is symmetric about the midplane"
);

/// The base height field's parameters: the world map's input (data: `shared/worldgen/terrain.json`,
/// generated into `terrain.gen.rs` by `shared/worldgen/gen.mjs`; `base.rs`).
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

/// How columns are shaped from the world map (data: `terrain.json`; its comments give the units
/// and meanings, the generator's validation the ranges).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub detail_amplitude: f64,
    pub detail_wavelength: i32,
    pub detail_octaves: u32,
    pub detail_mountain_amplitude: f64,
    pub detail_mountain_from: f64,
    pub detail_mountain_full: f64,
    pub detail_water_fade: f64,
    pub river_smooth: u32,
    pub river_min_depth: f64,
    pub river_bank: f64,
    pub river_valley_slope: f64,
    /// A joining river is at the joined one's level this far (and its own half-width) beyond the
    /// joined one's channel (m).
    pub river_join_margin: f64,
    /// How fast a levee falls away from the bank to the lower land (m per m).
    pub river_levee_fall: f64,
    /// The steepest a joining river falls to the river it joins (m per m).
    pub river_join_slope: f64,
    pub river_reach: f64,
    pub river_reach_per_width: f64,
    pub river_meander: f64,
    pub river_meander_wavelength: i32,
    pub river_lod_width: f64,
    pub river_lod_min_share: f64,
    pub river_lod_max_cell: i64,
    pub mouth_reach: f64,
    pub estuary_widen: f64,
    pub estuary_deepen: f64,
    pub delta_widen: f64,
    pub delta_branches: u32,
    pub delta_split: f64,
    pub delta_spread: f64,
    pub delta_branch_width: f64,
    pub lake_bank: f64,
    pub lake_levee: f64,
    pub lake_shore_noise: f64,
    pub lake_shore_wavelength: i32,
    pub snow_line: f64,
    pub snow_noise: f64,
    pub snow_wavelength: i32,
    pub snow_slope_lift: f64,
    pub rock_slope: f64,
    /// The rock band's slope threshold at the snow line (soil thins with height): it eases from
    /// `rock_slope` at sea level to this.
    pub rock_slope_high: f64,
    /// The landform's slope for the rock band is taken over ± this many metres (a hillside, not
    /// a map cell), so rock comes in whole faces, not along the map's grid.
    pub rock_window: f64,
    pub shore: f64,
    pub under_depth: f64,
    pub ground: &'static str,
    pub under: &'static str,
    pub snow: &'static str,
    pub rock: &'static str,
    pub shore_block: &'static str,
    pub spawn_min_height: f64,
    pub spawn_max_height: f64,
    pub spawn_near_river: f64,
}

mod table {
    include!("terrain.gen.rs");
}
pub use table::{HEIGHT, SURFACE};

const S: Surface = SURFACE;
/// Sand at the shore and under water to SAND_DEPTH (rows, from metres).
const SAND_DEPTH: i64 = (3.0 / CELL_H) as i64;

/// The deepest a lake's bed is forced below its level near its shore (m): deeper beds are the map's.
const LAKE_DEPTH: f64 = 6.0;

/// The spawn point's level patch reaches this many columns either side.
pub const SPAWN_CLEAR: i32 = 4;

/// What water a column holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Water {
    /// Dry.
    #[default]
    None,
    /// The sea (below sea level).
    Sea,
    River,
    Lake,
}

/// The block bands of a column's top (resolved from `bands`' names).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Band {
    #[default]
    Ground,
    Snow,
    Rock,
    Shore,
}

/// One column, from the map.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Ground height (m, face-local from the midplane): the top voxel is the row ceil(height ÷
    /// cell height) − 1.
    pub height: f32,
    /// The water's surface (m, face-local), when `water` is not `None`: the rows below
    /// `water_row` over the ground are water.
    pub level: f32,
    pub water: Water,
    /// The surface's band.
    pub band: Band,
    /// Beyond the world's disc: nothing is generated.
    pub outside: bool,
}

impl Column {
    /// The first row above the water (rows below it over the ground are water); the ground's top
    /// + 1 when dry.
    pub fn water_row(&self) -> i32 {
        let top = top_voxel(self.height);
        if self.water == Water::None {
            return top + 1;
        }
        surface_row(self.level).max(top + 1)
    }

    /// The row the water's surface lies on (rows below it are under water, the top cell's open
    /// part too); none when dry.
    pub fn flood_row(&self) -> i32 {
        if self.water == Water::None {
            i32::MIN
        } else {
            surface_row(self.level)
        }
    }

    /// Whether water stands over the ground (cells of water above its top).
    pub fn wet(&self) -> bool {
        self.water_row() > top_voxel(self.height) + 1
    }
}

/// The row a water surface (m, face-local) lies on: the rows below it are water.
fn surface_row(level: f32) -> i32 {
    floor_to_int(level / CELL_H as f32 + 0.5)
}

/// A height (m) in rows of cells (HALF_HEIGHT_GRID.md §2); exact at 1 m and ½ m cells.
fn rows(h: f32) -> f32 {
    h / CELL_H as f32
}

/// The top voxel (row) of a column of height h (m): ceil(h in rows) − 1.
fn top_voxel(h: f32) -> i32 {
    let r = rows(h);
    let f = floor_to_int(r);
    if f as f32 == r { f - 1 } else { f }
}

/// The bedrock's state (`shared/worldgen/midplane.json`), resolved by name once.
fn bedrock() -> StateId {
    static BEDROCK: std::sync::LazyLock<StateId> =
        std::sync::LazyLock::new(|| dwell_blocks::require(MIDPLANE.bedrock));
    *BEDROCK
}

/// The bands' blocks, resolved by name once.
#[derive(Clone, Copy, Debug)]
pub struct Bands {
    pub ground: StateId,
    pub under: StateId,
    pub snow: StateId,
    pub rock: StateId,
    pub shore: StateId,
}

pub fn bands() -> &'static Bands {
    static BANDS: std::sync::LazyLock<Bands> = std::sync::LazyLock::new(|| Bands {
        ground: dwell_blocks::require(S.ground),
        under: dwell_blocks::require(S.under),
        snow: dwell_blocks::require(S.snow),
        rock: dwell_blocks::require(S.rock),
        shore: dwell_blocks::require(S.shore_block),
    });
    &BANDS
}

/// The material `depth` rows below the surface of a column whose top voxel is `top` and band
/// `band`: the bedrock in the face's first `bedrock_rows` from the midplane, the core's stone over
/// it up to `CORE_LAYERS`, then the band's layers.
fn fill(band: Band, top: i32, depth: i64) -> StateId {
    fill_soil(band, top, depth, under_rows())
}

/// The `under` block's least depth under a top block (rows): `underDepth` m.
fn under_rows() -> i64 {
    (S.under_depth / CELL_H) as i64
}

/// The soil under a column's top (rows): `underDepth`, or down to its lowest horizontal
/// neighbour's top, whichever is deeper, so the side a terrace's step shows is soil, never stone:
/// on grass and snow the soil mantles the hillside, and bare stone shows only where the rock band
/// is (steep faces), as whole areas, not as stray risers speckling a slope (owner, 2026-10-26).
pub fn soil_rows(top: i32, lowest_neighbour_top: i32) -> i64 {
    under_rows().max(i64::from(top - lowest_neighbour_top))
}

/// [`fill`] with `soil` rows of the `under` block under a ground or snow top.
fn fill_soil(band: Band, top: i32, depth: i64, soil: i64) -> StateId {
    let y = i64::from(top) - depth;
    if y < i64::from(MIDPLANE_Y + MIDPLANE.bedrock_rows) {
        return bedrock();
    }
    if y < i64::from(MIDPLANE_Y + CORE_LAYERS) {
        return M.stone;
    }
    let b = bands();
    match band {
        Band::Shore => {
            if depth < SAND_DEPTH {
                b.shore
            } else {
                M.stone
            }
        }
        Band::Rock => b.rock,
        Band::Ground | Band::Snow => {
            if depth == 0 {
                if band == Band::Snow { b.snow } else { b.ground }
            } else if depth <= soil {
                b.under
            } else {
                M.stone
            }
        }
    }
}

/// The state the generator writes for `m` at row `y` of a column's top (its top voxel, or a stored
/// ramp piece over it): below the column's water it holds the water in its open part (its
/// `flooded` state, when its block has one; docs/WATER_RENDERING.md §9), so a ramp in the water's
/// rows is under the water, not dry beside it.
fn under_water(m: StateId, y: i32, water_row: i32) -> StateId {
    if y >= water_row {
        return m;
    }
    static FLOODED: std::sync::LazyLock<Vec<StateId>> = std::sync::LazyLock::new(|| {
        (0..dwell_blocks::STATES.len() as StateId)
            .map(|id| dwell_blocks::with_property(id, "flooded", "true").unwrap_or(id))
            .collect()
    });
    FLOODED.get(usize::from(m)).copied().unwrap_or(m)
}

#[derive(Clone, Debug)]
pub struct TerrainGenerator {
    base: BaseField,
    map: Arc<WorldMap>,
    detail_seed: u32,
    meander_seeds: [u32; 2],
    shore_seeds: [u32; 2],
    snow_seed: u32,
    offset_x: i64,
    offset_z: i64,
    /// The decorations (data), resolved for this world.
    decorations: Vec<Placer>,
    /// The stored ramps natural tops are shaped with (HALF_HEIGHT_GRID.md §4); empty (no shaping)
    /// on 1 m cells.
    ramps: Arc<RampPieces>,
    /// The structure placements (data), resolved for this world.
    planters: Vec<Planter>,
    /// The portals between the faces (`portals.rs`), shared by both faces' generators; none for a
    /// face's generator on its own.
    portals: Option<Arc<Portals>>,
    /// How far above the ground decorations and placed structures reach (rows): the sky test
    /// leaves this much room over the highest ground before a chunk is all air.
    rise: f32,
    /// The LOD colour map's river valleys (a preview, `?lodmap=on`; ARCHITECTURE.md §6.6 "Colour
    /// map"): where a river crosses an LOD cell too coarse to hold its water, the cell's ground is
    /// lowered this far (m) under the river's level (`lod_valley`). Off by default; never in
    /// chunks.
    lod_valleys: Option<f64>,
    /// The terrain volume over the height field (`volume.rs`, a prototype: world type `cliffs`);
    /// none for the plain terrain. Chunks only: the LOD draws the height field's top.
    volume: Option<Arc<Volume>>,
    world_seed: u64,
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
        let b = bands();
        for m in [b.ground, b.shore, b.under, b.rock, b.snow] {
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

/// How far above the ground decorations reach (one cell).
const DECORATION_RISE: i32 = 1;

/// The LOD draws placed structures as blobs of their voxels at levels of cells this size (m) or
/// finer, and as a canopy tint on the ground at coarser ones.
const LOD_TREE_CELL: i64 = 2;

impl TerrainGenerator {
    /// The face generator for face seed `world_seed`, its world map computed now or taken from
    /// the process's maps (`worldmap::for_seed`).
    pub fn new(world_seed: u64) -> Self {
        Self::with_map(world_seed, worldmap::for_seed(world_seed))
    }

    /// The face generator over a given map (tests: a fixture map).
    pub fn with_map(world_seed: u64, map: Arc<WorldMap>) -> Self {
        let base = BaseField::new(world_seed);
        let (offset_x, offset_z) = base.offset();
        Self {
            base,
            map,
            detail_seed: seed_word(world_seed, 0xD7A1_0001),
            meander_seeds: [
                seed_word(world_seed, 0xD7A1_0002),
                seed_word(world_seed, 0xD7A1_0003),
            ],
            shore_seeds: [
                seed_word(world_seed, 0xD7A1_0004),
                seed_word(world_seed, 0xD7A1_0005),
            ],
            snow_seed: seed_word(world_seed, 0xD7A1_0006),
            offset_x,
            offset_z,
            decorations: Placer::resolve(&DECORATIONS, world_seed),
            ramps: Arc::new(RampPieces::resolve()),
            planters: Vec::new(),
            portals: None,
            rise: DECORATION_RISE as f32,
            lod_valleys: None,
            volume: None,
            world_seed,
        }
        .with_placements(&PLACEMENTS)
    }

    /// The world seed this generator was made from (a face's own seed, `bifacial::face_seed`).
    pub fn seed(&self) -> u64 {
        self.world_seed
    }

    /// This face's world map.
    pub fn map(&self) -> &Arc<WorldMap> {
        &self.map
    }

    /// The generator with these structure placements in place of the build's (tests add one by
    /// data alone).
    pub fn with_placements(mut self, rules: &[placements::Placement]) -> Self {
        self.planters = Planter::resolve(rules, self.world_seed, (self.offset_x, self.offset_z));
        // A structure anchored beside the chunk on ground up to its reach higher still reaches in
        // (rows: a structure's rise counts cells; ground climbs up to 1 m per column of reach).
        let reach = self.planters.iter().map(Planter::reach).max().unwrap_or(0);
        let rise = if self.planters.is_empty() {
            0
        } else {
            placements::max_rise(&self.planters) + 1 + reach * rows_for_m(1.0)
        };
        self.rise = DECORATION_RISE.max(rise) as f32;
        self.raise_for_portals();
        self
    }

    /// The generator with the world's portals cut into it (`BifacialTerrain` gives both faces the
    /// same ones).
    pub fn with_portals(mut self, portals: Arc<Portals>) -> Self {
        self.portals = Some(portals);
        self.raise_for_portals();
        self
    }

    /// The generator with a terrain volume shaping its chunks (`volume.rs`); the sky test leaves
    /// room for what it adds over the ground.
    pub fn with_volume(mut self, def: &'static VolumeDef) -> Self {
        let v = Volume::resolve(def, self.world_seed);
        self.rise = self.rise.max((v.reach_rows + 1) as f32);
        self.volume = Some(Arc::new(v));
        self
    }

    /// The portals, if this generator has them.
    pub fn portals(&self) -> Option<&Arc<Portals>> {
        self.portals.as_ref()
    }

    /// The sky test leaves room for the rims over the ground too.
    fn raise_for_portals(&mut self) {
        if let Some(p) = &self.portals {
            self.rise = self.rise.max((p.rules.rim_rows + 1) as f32);
        }
    }

    /// The portals reaching columns [x0, x1] × [z0, z1], with what resolves them.
    fn portals_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Option<(&Portals, Vec<Portal>)> {
        let p = self.portals.as_deref()?;
        let near = p.near(lo, hi);
        (!near.is_empty()).then_some((p, near))
    }

    /// The top row of a portal's ladder on this face: the ground (and rim) of the wall column it
    /// climbs out onto.
    pub fn ladder_top(&self, portals: &Portals, portal: Portal) -> i32 {
        let (wx, wz) = portals.wall_column(portal);
        let top = top_voxel(self.column_at(wx, wz).height);
        match portals.part_of(portal, wx, wz) {
            Part::Rim => top + portals.rules.rim_rows,
            _ => top,
        }
    }

    /// The structure placements, resolved.
    pub fn planters(&self) -> &[Planter] {
        &self.planters
    }

    /// A domain warp of about ±`amplitude` m at `wavelength` (two octaves).
    fn warp(&self, seeds: [u32; 2], x: i64, z: i64, amplitude: f64, wavelength: i32) -> (f64, f64) {
        if amplitude <= 0.0 {
            return (x as f64, z as f64);
        }
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        let wx = f64::from(fbm2(seeds[0], nx, nz, wavelength, 2, None));
        let wz = f64::from(fbm2(seeds[1], nx, nz, wavelength, 2, None));
        (
            x as f64 + 2.0 * amplitude * wx,
            z as f64 + 2.0 * amplitude * wz,
        )
    }

    /// Whether the ground at (x, z), `h` m over the sea, is bare rock: the landform's slope over ±
    /// `rock_window` m (central differences of the map's smooth height: a hillside's, not a map
    /// cell's) past a threshold easing from `rock_slope` at the sea to `rock_slope_high` at the
    /// snow line, as the soil thins with height (owner, 2026-10-26: structured, rooted in realism).
    /// A cell of `cell` m (the LOD's coarser ones) takes the slope over at least its own width,
    /// so a rock face narrower than a cell is not drawn as a dotted line of single cells.
    fn rock_at(&self, map: &crate::worldmap::WorldMap, x: f64, z: f64, h: f64, cell: i64) -> bool {
        let w = S.rock_window.max(cell as f64);
        let hx = map.height_at(x + w, z).0 - map.height_at(x - w, z).0;
        let hz = map.height_at(x, z + w).0 - map.height_at(x, z - w).0;
        let slope = hx.hypot(hz) / (2.0 * w);
        let t = (h / S.snow_line).clamp(0.0, 1.0);
        let threshold = S.rock_slope + (S.rock_slope_high - S.rock_slope) * t * t * (3.0 - 2.0 * t);
        slope >= threshold
    }

    /// The column at (x, z) as seen at a cell of `cell` m: 1 for chunks; the LOD's coarser cells
    /// drop the detail octaves finer than a cell and widen rivers to show from afar.
    fn column(&self, x: i64, z: i64, cell: i64) -> Column {
        if !inside_world_disc(x, z) {
            return Column {
                outside: true,
                ..Column::default()
            };
        }
        let map = &*self.map;
        let (base, gx, gz) = map.height_at(x as f64, z as f64);
        // The fine detail, stronger on mountains.
        let amplitude = S.detail_amplitude
            + (S.detail_mountain_amplitude - S.detail_amplitude)
                * f64::from(smooth_step(
                    S.detail_mountain_from as f32,
                    S.detail_mountain_full as f32,
                    base as f32,
                ));
        let kept = octaves_resolved(S.detail_wavelength, S.detail_octaves, cell);
        let detail = if kept == 0 {
            0.0
        } else {
            2.0 * amplitude
                * f64::from(fbm2(
                    self.detail_seed,
                    x + self.offset_x,
                    z + self.offset_z,
                    S.detail_wavelength,
                    S.detail_octaves,
                    Some(kept),
                ))
        };
        // The nearest river channel, through the meander warp (none far from every river).
        let (min_half, min_width) = if cell > 1 && cell <= S.river_lod_max_cell {
            (
                0.5 * S.river_lod_width * cell as f64,
                S.river_lod_min_share * cell as f64,
            )
        } else {
            (0.0, f64::INFINITY)
        };
        let river = if map.channels.near(x as f64, z as f64, min_half) {
            let (wx, wz) = self.warp(
                self.meander_seeds,
                x,
                z,
                S.river_meander,
                S.river_meander_wavelength,
            );
            map.channels.query(wx, wz, min_half, min_width)
        } else {
            None
        };
        // The lake whose shore field covers it, through the shore warp.
        let lake = if map.shores.fields.is_empty() {
            None
        } else {
            let (sx, sz) = self.warp(
                self.shore_seeds,
                x,
                z,
                S.lake_shore_noise,
                S.lake_shore_wavelength,
            );
            map.shores.query(map, sx, sz)
        };
        let mut fade: f64 = 1.0;
        if let Some(r) = &river {
            fade = fade.min(smooth01((r.d - r.half) / S.detail_water_fade.max(1e-6)));
        }
        if let Some(l) = &lake {
            fade = fade.min(smooth01(-l.sdf / S.detail_water_fade.max(1e-6)));
        }
        let mut h = base + detail * fade;
        let mut water = Water::None;
        let mut level = 0.0f64;
        if let Some(l) = &lake {
            if l.sdf > 0.0 {
                // Inside the shore: a bed below the water, deepening away from the shore.
                h = h.min(l.level - (0.5 + 0.15 * l.sdf).min(LAKE_DEPTH));
                water = Water::Lake;
                level = l.level;
            } else {
                // Round the shore: never lower than the bank over the water near it.
                let out = -l.sdf;
                let required = l.level + S.lake_bank - (out - 2.0).max(0.0) * 0.25;
                h = h.max(required);
            }
        }
        if let Some(r) = &river {
            let edge = r.d - r.half;
            if edge < 0.0 {
                // The channel: its bed below the water, deepest on the centreline.
                let f = r.d / r.half;
                let depth = (r.depth * (1.0 - f * f)).max(S.river_min_depth);
                // Below the surface as drawn (its row), so a channel always holds water cells.
                let drawn =
                    f64::from(surface_row((SEA_LEVEL_M + r.level) as f32)) * CELL_H - SEA_LEVEL_M;
                h = h.min(drawn.min(r.level) - depth);
                // In a lake the lake's flat surface wins (a river running into it ends there).
                if water != Water::Lake {
                    water = Water::River;
                    level = r.level;
                }
            } else if water != Water::Lake {
                // The valley down to the banks, blended back into the land at its reach.
                let bank = r.level + S.river_bank;
                let valley = bank + edge * S.river_valley_slope;
                let carved = smooth_min(h, valley, 2.0);
                let t = smooth01(edge / r.valley.max(1e-6));
                h = carved + (h - carved) * t;
                // Where the land is lower than the water, a levee `bank` over it so the water
                // never stands open beside lower ground, falling away gently (`leveeFall`) so it
                // is a broad bank, never a ridge; none where the river is at the sea's level (its
                // water is the sea's: the land beside it is a shore, open to both).
                if r.level > CELL_H {
                    let levee = bank - (edge - 1.0).max(0.0) * S.river_levee_fall;
                    h = h.max(levee);
                }
            }
        }
        if h < 0.0 && (water == Water::None || level < 0.0) {
            water = Water::Sea;
            level = 0.0;
        }
        let height = ((SEA_LEVEL_M + h).max(dwell_world::MIDPLANE_M + dwell_world::CORE_M)) as f32;
        let level = (SEA_LEVEL_M + level) as f32;
        // Water only where its surface's row lies above the top cell's bottom (rows, as drawn).
        let wet = water != Water::None && surface_row(level) > top_voxel(height);
        if !wet {
            water = Water::None;
        }
        // The band: shore (and every bed under water), rock where steep, snow over the snow line.
        let slope = (gx * gx + gz * gz).sqrt();
        let band = if wet || h < S.shore {
            Band::Shore
        } else if self.rock_at(map, x as f64, z as f64, h, cell) {
            Band::Rock
        } else if h + S.snow_noise >= S.snow_line {
            let noise = 2.0
                * S.snow_noise
                * f64::from(fbm2(
                    self.snow_seed,
                    x + self.offset_x,
                    z + self.offset_z,
                    S.snow_wavelength,
                    3,
                    Some(octaves_resolved(S.snow_wavelength, 3, cell)),
                ));
            if h >= S.snow_line + noise + S.snow_slope_lift * slope {
                Band::Snow
            } else {
                Band::Ground
            }
        } else {
            Band::Ground
        };
        Column {
            height,
            level,
            water,
            band,
            outside: false,
        }
    }

    /// A column with exactly the chunk path's arithmetic (spawn, tools, tests).
    pub fn column_at(&self, x: i32, z: i32) -> Column {
        self.column(i64::from(x), i64::from(z), 1)
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

    /// The state `generate` puts at (x, y, z) (face-local), from the column alone (no ramps,
    /// decorations, structures or portals).
    pub fn state_at(&self, x: i32, y: i32, z: i32) -> StateId {
        let col = self.column_at(x, z);
        let soil = self.soil_at(x, z, &col, |nx, nz| self.column_at(nx, nz));
        Self::state_in(&col, y, soil)
    }

    /// [`soil_rows`] for column (x, z), its neighbours' columns from `column`.
    fn soil_at(&self, x: i32, z: i32, col: &Column, column: impl Fn(i32, i32) -> Column) -> i64 {
        let top = top_voxel(col.height);
        let lowest = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .map(|(dx, dz)| column(x + dx, z + dz))
            .filter(|c| !c.outside)
            .map(|c| top_voxel(c.height))
            .min()
            .unwrap_or(top);
        soil_rows(top, lowest)
    }

    fn state_in(col: &Column, y: i32, soil: i64) -> StateId {
        if col.outside || !(FACE_MIN_Y..FACE_MAX_Y).contains(&y) {
            return M.air;
        }
        let top = top_voxel(col.height);
        let water = col.water_row();
        if y <= top {
            let m = fill_soil(col.band, top, i64::from(top - y), soil);
            if y == top {
                under_water(m, y, col.flood_row())
            } else {
                m
            }
        } else if y < water {
            M.water
        } else {
            M.air
        }
    }

    /// The height (m) of the ground's surface at the centre of column (x, z) (face-local, inside the
    /// disc): the top of a flat column, or the middle of the ramp the generation rule tops it with.
    pub fn surface_y_at(&self, x: i32, z: i32) -> Option<f64> {
        let g = self.ground_y(x, z)?;
        let col = self.column_at(x, z);
        let pieces = self.pieces_with(|nx, nz| self.column_at(nx, nz), x, z, &col, g);
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

    /// The island's coast distance at the point itself (the base field's: > 0 on its land).
    pub fn coast_at(&self, x: i32, z: i32) -> f32 {
        self.base.coast_at(i64::from(x), i64::from(z))
    }

    /// Columns of a chunk (S × S, row-major from (x0, z0)).
    fn chunk_columns(&self, x0: i32, z0: i32) -> Vec<Column> {
        self.rect_columns(x0, z0, CHUNK_SIZE, CHUNK_SIZE)
    }

    /// Columns of a w × l rectangle (row-major from (x0, z0)), with exactly `column_at`'s
    /// arithmetic.
    fn rect_columns(&self, x0: i32, z0: i32, w: i32, l: i32) -> Vec<Column> {
        let mut cols = Vec::with_capacity((w * l) as usize);
        for z in z0..z0 + l {
            for x in x0..x0 + w {
                cols.push(self.column_at(x, z));
            }
        }
        cols
    }

    /// The highest ground or water of the columns inside the disc, in rows, plus what grows on it
    /// (rows).
    fn sky_floor(&self, cols: &[Column], (x0, z0): (i32, i32)) -> f32 {
        let ground = cols.iter().filter(|c| !c.outside).fold(-1e9f32, |top, c| {
            top.max(rows(c.height)).max(c.water_row() as f32)
        }) + self.rise;
        let s = CHUNK_SIZE;
        ground.max(self.monuments_top((x0, z0), (x0 + s - 1, z0 + s - 1)) as f32)
    }

    /// A portal's monument on this face: its anchor on the axis, its bottom row on the lowest
    /// ground under its bottom layer (`None` without one).
    pub fn monument(&self, portals: &Portals, portal: Portal) -> Option<Placed> {
        let base = portals
            .monument_feet(portal)
            .into_iter()
            .map(|(x, z)| top_voxel(self.column_at(x, z).height))
            .min()?;
        portals.monument_at(portal, base + 1)
    }

    /// The monuments reaching columns [x0, x1] × [z0, z1], placed.
    fn monuments_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<Placed> {
        let Some((portals, near)) = self.portals_near(lo, hi) else {
            return Vec::new();
        };
        near.into_iter()
            .filter_map(|p| self.monument(portals, p))
            .filter(|m| {
                m.max[0] >= lo.0 && m.min[0] <= hi.0 && m.max[2] >= lo.1 && m.min[2] <= hi.1
            })
            .collect()
    }

    /// The row over the highest monument cell reaching the columns (a very low row for none).
    fn monuments_top(&self, lo: (i32, i32), hi: (i32, i32)) -> i32 {
        self.monuments_near(lo, hi)
            .iter()
            .map(|m| m.max[1] + 1)
            .max()
            .unwrap_or(i32::MIN / 2)
    }

    /// The sky test's height (rows) for a chunk column: chunks with y0 above it (and at or above
    /// sea level) are air. Depends only on (cx, cz), so callers cache it per column.
    pub fn sky_floor_at(&self, cx: i32, cz: i32) -> f32 {
        let (x0, z0) = (cx * CHUNK_SIZE, cz * CHUNK_SIZE);
        self.sky_floor(&self.chunk_columns(x0, z0), (x0, z0))
    }

    /// A column's ground as placements see it (`None` beyond the rim).
    fn ground_of(col: &Column) -> Option<Ground> {
        if col.outside {
            return None;
        }
        let top = top_voxel(col.height);
        // The top block as generated: under water it takes its flooded state. Rows of cells for
        // the top; metres for the height band (placements.json's) and the slope.
        let water = col.water_row();
        let block = under_water(fill(col.band, top, 0), top, col.flood_row());
        Some(Ground {
            top,
            above_sea: (m_of_row(top) - SEA_LEVEL_M).floor() as i32,
            height: col.height,
            block,
            dry: water <= top + 1 && !dwell_blocks::info(block).flooded,
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
        let mut trees = placements::trees_in(&self.planters, (x0, z0), (x1, z1), &ground);
        // No structure stands over a portal's opening or its clear ground round the rim.
        if let Some(p) = &self.portals {
            trees.retain(|t| {
                let fp = self.planters[t.planter as usize].footprint(t);
                !p.box_reaches_clear((fp[0], fp[1]), (fp[2], fp[3]))
            });
        }
        trees
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
        if y0 as f32 > self.sky_floor(&cols, (x0, z0)) && y0 >= SEA_LEVEL {
            return;
        }
        let near = self.portals_near((x0, z0), (x0 + S - 1, z0 + S - 1));
        let voxels = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside {
                    continue;
                }
                let top = top_voxel(col.height);
                let water = col.water_row();
                let flood = col.flood_row();
                let (part, portal) = near
                    .as_ref()
                    .map_or((Part::None, None), |(p, n)| p.locate(n, x0 + x, z0 + z));
                let in_volume = |y: i32| {
                    (FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                        && (volume != Overlap::Partial
                            || BUILD_VOLUME.contains_voxel(
                                i64::from(x0 + x),
                                i64::from(y),
                                i64::from(z0 + z),
                            ))
                };
                // A portal's shaft: open from the ground to the midplane, the bedrock and core
                // included, and its ladder up the wall the whole way (portals.rs).
                if let (Part::Shaft { ladder }, Some((portals, _)), Some(portal)) =
                    (part, near.as_ref(), portal)
                {
                    let ladder_top = if ladder {
                        self.ladder_top(portals, portal)
                    } else {
                        i32::MIN
                    };
                    for ly in 0..S {
                        let y = y0 + ly;
                        if in_volume(y) {
                            voxels[local_index(x, ly, z)] = if y <= ladder_top {
                                portals.ladder
                            } else {
                                M.air
                            };
                        }
                    }
                    continue;
                }
                let rim = match (part, near.as_ref()) {
                    (Part::Rim, Some((portals, _))) => Some((portals.rim, portals.rules.rim_rows)),
                    _ => None,
                };
                // The stored ramp pieces over the top (HALF_HEIGHT_GRID.md §4), from the bottom;
                // a portal's rim stands on flat ground.
                let pieces = if rim.is_some() {
                    &[]
                } else {
                    self.top_pieces(&cols, x0, z0, x, z, top)
                };
                let first_piece = top + 1 - pieces.len() as i32;
                // The soil down to the lowest neighbour's top (neighbours past the chunk from
                // `column_at`, so the answer doesn't depend on the chunk asking).
                let soil = self.soil_at(x0 + x, z0 + z, &col, |nx, nz| {
                    let (ix, iz) = (nx - x0, nz - z0);
                    if (0..S).contains(&ix) && (0..S).contains(&iz) {
                        cols[(iz * S + ix) as usize]
                    } else {
                        self.column_at(nx, nz)
                    }
                });
                for ly in 0..S {
                    let y = y0 + ly;
                    if !in_volume(y) {
                        continue;
                    }
                    let v = &mut voxels[local_index(x, ly, z)];
                    if let Some((m, rows)) = rim
                        && y > top
                        && y <= top + rows
                    {
                        *v = m;
                    } else if y <= top && y >= first_piece {
                        *v = under_water(pieces[(y - first_piece) as usize], y, flood);
                    } else if y == top {
                        *v = under_water(fill(col.band, top, 0), y, flood);
                    } else if y < top {
                        *v = fill_soil(col.band, top, i64::from(top - y), soil);
                    } else if y < water {
                        *v = M.water;
                    }
                }
                // A decoration in the air cell over the ground, if this chunk holds that cell
                // (none near a portal).
                let above = top + 1;
                if part == Part::None
                    && (y0..y0 + S).contains(&above)
                    && above >= water
                    && (FACE_MIN_Y..FACE_MAX_Y).contains(&above)
                    && let Some(m) = self.decoration_at(&cols, x0, z0, x, z, top)
                {
                    voxels[local_index(x, above - y0, z)] = m;
                }
            }
        }
        if let Some(v) = &self.volume {
            self.shape_volume(v, coord, &cols, volume, near.as_ref(), chunk);
        }
        self.stamp_trees(coord, &cols, volume, chunk);
        self.stamp_monuments(coord, volume, chunk);
    }

    /// Reshapes a generated chunk by the terrain volume: within its reach of the height field's
    /// surface a cell is solid where the density is above zero. Cells it adds are its material
    /// (over air, water and decorations); cells it removes become air, or water below the column's
    /// water. A decoration left over air goes. Portals' columns and the core are left alone.
    fn shape_volume(
        &self,
        v: &Volume,
        coord: ChunkCoord,
        cols: &[Column],
        overlap: Overlap,
        near: Option<&(&Portals, Vec<Portal>)>,
        chunk: &mut Chunk,
    ) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        let reach = v.reach_rows;
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for c in cols.iter().filter(|c| !c.outside) {
            let top = top_voxel(c.height);
            lo = lo.min(top);
            hi = hi.max(top);
        }
        // One row more below: the cell under the chunk's bottom row decides its decorations.
        if lo == i32::MAX || y0 + S <= lo - reach - 1 || y0 > hi + reach {
            return;
        }
        // The columns around the chunk out to the widest slope radius.
        let m = v.margin();
        let w = S + 2 * m;
        let ext = if m == 0 {
            cols.to_vec()
        } else {
            self.rect_columns(x0 - m, z0 - m, w, w)
        };
        let height_at = |lx: i32, lz: i32, own: f32| {
            let c = ext[((lz + m) * w + lx + m) as usize];
            if c.outside { own } else { c.height }
        };
        let floor = MIDPLANE_Y + CORE_LAYERS;
        let voxels = chunk.voxels_mut();
        let mut slopes = vec![0.0f32; v.radii().len()];
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside {
                    continue;
                }
                if let Some((p, n)) = near
                    && p.locate(n, x0 + x, z0 + z).0 != Part::None
                {
                    continue;
                }
                let top = top_voxel(col.height);
                if y0 + S <= top - reach - 1 || y0 > top + reach {
                    continue;
                }
                for (k, &r) in v.radii().iter().enumerate() {
                    let r = i32::from(r);
                    let h = col.height;
                    let sx = (height_at(x + r, z, h) - height_at(x - r, z, h)) / (2 * r) as f32;
                    let sz = (height_at(x, z + r, h) - height_at(x, z - r, h)) / (2 * r) as f32;
                    slopes[k] = (sx * sx + sz * sz).sqrt();
                }
                let eval = v.column(&ColumnInput {
                    x: x0 + x,
                    z: z0 + z,
                    surface: m_of_row(top + 1) as f32,
                    slopes: &slopes,
                });
                let solid = |y: i32| {
                    if y < floor || (y - top).abs() > reach {
                        y <= top
                    } else {
                        v.density(&eval, y) > 0.0
                    }
                };
                // Carved cells under the column's water are water; in a dry column the water row
                // is just over the ground, so there they are air.
                let water = if col.water == Water::None {
                    i32::MIN
                } else {
                    col.water_row()
                };
                let in_volume = |y: i32| {
                    (FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                        && (overlap != Overlap::Partial
                            || BUILD_VOLUME.contains_voxel(
                                i64::from(x0 + x),
                                i64::from(y),
                                i64::from(z0 + z),
                            ))
                };
                let mut below = solid(y0 - 1);
                for ly in 0..S {
                    let y = y0 + ly;
                    let here = solid(y);
                    if in_volume(y) {
                        let cell = &mut voxels[local_index(x, ly, z)];
                        if here != (y <= top) {
                            *cell = if here {
                                v.material
                            } else if y < water {
                                M.water
                            } else {
                                M.air
                            };
                        } else if !here
                            && !below
                            && *cell != M.air
                            && *cell != M.water
                            && dwell_blocks::info(*cell).replaceable
                        {
                            *cell = M.air;
                        }
                    }
                    below = here;
                }
            }
        }
    }

    /// Stands the portals' monuments reaching this chunk in it, over air and replaceable cells
    /// only (the ground they stand on stays; a higher ground under a pillar holds it).
    fn stamp_monuments(&self, coord: ChunkCoord, volume: Overlap, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        let monuments = self.monuments_near((x0, z0), (x0 + S - 1, z0 + S - 1));
        if monuments.is_empty() {
            return;
        }
        let voxels = chunk.voxels_mut();
        for m in monuments.iter().filter(|m| !m.misses(coord)) {
            for (p, state) in m.cells() {
                let (lx, ly, lz) = (p[0] - x0, p[1] - y0, p[2] - z0);
                let inside = (0..S).contains(&lx)
                    && (0..S).contains(&ly)
                    && (0..S).contains(&lz)
                    && (FACE_MIN_Y..FACE_MAX_Y).contains(&p[1])
                    && (volume != Overlap::Partial
                        || BUILD_VOLUME.contains_voxel(
                            i64::from(p[0]),
                            i64::from(p[1]),
                            i64::from(p[2]),
                        ));
                if !inside {
                    continue;
                }
                let v = &mut voxels[local_index(lx, ly, lz)];
                if *v == M.air || dwell_blocks::info(*v).replaceable {
                    *v = state;
                }
            }
        }
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
        let col = cols[(lz * S + lx) as usize];
        self.pieces_with(column, x0 + lx, z0 + lz, &col, top)
    }

    /// [`Self::top_pieces`] for column (x, z), its neighbours' columns from `column`.
    fn pieces_with(
        &self,
        column: impl Fn(i32, i32) -> Column,
        x: i32,
        z: i32,
        col: &Column,
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
        // Dry pieces: `generate` floods each one under the water (`under_water`).
        let cells = self.ramps.cells(fill(col.band, top, 0), shape, false);
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
        let ground = fill(cols[(lz * S + lx) as usize].band, top, 0);
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

    /// A column at a coarse cell's resolution, as the LOD samples it: the detail octaves finer
    /// than `cell` m dropped, rivers widened to show (`rivers.lodWidth`), and with the LOD colour
    /// map's valleys on, lowered under a river crossing the cell (`lod_valley`).
    pub fn column_lod(&self, x: i64, z: i64, cell: i64) -> Column {
        let col = self.column(x, z, cell);
        if let Some(margin) = self.lod_valleys.filter(|_| cell > 1) {
            return self.lod_valley(col, x, z, cell, margin);
        }
        col
    }

    /// Turns the LOD colour map's river valleys on, their ground `margin` m under the river's
    /// level, or off (`column_lod`; LOD sections only).
    pub fn set_lod_valleys(&mut self, margin: Option<f64>) {
        self.lod_valleys = margin;
    }

    /// A dry LOD column whose cell a river crosses (its channel, widened to the cell, within
    /// 0.6 of a cell of the centre), lowered `valleyMarginM` under the river's water level, so a
    /// river painted on it sits in a valley rather than on a slope. A margin, not a tie: the
    /// lowered ground never lies in the plane of a water surface beside it (ARCHITECTURE.md §6.6
    /// "known bug class").
    fn lod_valley(&self, mut col: Column, x: i64, z: i64, cell: i64, margin: f64) -> Column {
        if col.outside || col.water != Water::None {
            return col;
        }
        let Some(r) = self.river_hit(x, z, 0.6 * cell as f64) else {
            return col;
        };
        if r.d > r.half || r.level < 0.0 {
            return col;
        }
        let floor = (SEA_LEVEL_M + r.level - margin) as f32;
        col.height = col.height.min(floor);
        col
    }

    /// The nearest river channel to face-local column (x, z), through the terrain's meander warp
    /// (what the chunks carve), its half-width at least `min_half` m; `None` far from every river.
    pub fn river_hit(&self, x: i64, z: i64, min_half: f64) -> Option<worldmap::Hit> {
        let map = &*self.map;
        if !map.channels.near(x as f64, z as f64, min_half) {
            return None;
        }
        let (wx, wz) = self.warp(
            self.meander_seeds,
            x,
            z,
            S.river_meander,
            S.river_meander_wavelength,
        );
        map.channels.query(wx, wz, min_half, 0.0)
    }

    /// The block on top of a column (dry, as the LOD and the colour map show it).
    pub fn top_state(col: &Column) -> StateId {
        fill(col.band, top_voxel(col.height), 0)
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

    /// How far above its ground a level's sections draw placed structures (rows): their full rise
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
            b.hi =
                b.hi.max(f64::from(rows(col.height)) + 1.0 + rise)
                    .max(f64::from(col.water_row()));
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
        let mut b = Self::lod_bounds_of(&self.lod_columns(o, level), self.lod_rise(level));
        self.raise_for_monuments(&mut b, o, level);
        b
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
        let mut bounds = Self::lod_bounds_of(&cols, self.lod_rise(c.level));
        self.raise_for_monuments(&mut bounds, o, c.level);
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
        // The portals' shafts, at levels whose cells are no wider than a shaft: a column whose
        // centre lies in one is open down to the midplane, as the chunks are.
        let half = cell / 2;
        let shafts = (cell as f32 <= self.portals.as_ref().map_or(0.0, |p| p.rules.radius))
            .then(|| {
                let n = i64::from(lod::SECTION_CELLS);
                let at = |o: i64, i: i64| (o + i * cell + half) as i32;
                self.portals_near((at(o.x, -1), at(o.z, -1)), (at(o.x, n), at(o.z, n)))
            })
            .flatten();
        for z in -1..=lod::SECTION_CELLS {
            for x in -1..=lod::SECTION_CELLS {
                let col = cols[lod::column_index(x, z)];
                if col.outside {
                    continue; // beyond the rim: the void
                }
                if let Some((p, near)) = &shafts {
                    let wx = (o.x + i64::from(x) * cell + half) as i32;
                    let wz = (o.z + i64::from(z) * cell + half) as i32;
                    if matches!(p.part(near, wx, wz), Part::Shaft { .. }) {
                        for y in -1..=lod::SECTION_CELLS {
                            if o.y + i64::from(y) * cell + cell <= face_min {
                                cells[lod::cell_index(x, y, z)] = M.stone; // the apron
                            }
                        }
                        continue;
                    }
                }
                let top = i64::from(top_voxel(col.height));
                let water = i64::from(col.water_row());
                let wet = water > top + 1;
                // The soil as the chunks mantle it: down to the lowest neighbouring cell's top.
                let lod_soil = |x: i32, z: i32| {
                    let n = lod::SECTION_CELLS;
                    let lowest = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .map(|(dx, dz)| ((x + dx).clamp(-1, n), (z + dz).clamp(-1, n)))
                        .map(|(nx, nz)| cols[lod::column_index(nx, nz)])
                        .filter(|c| !c.outside)
                        .map(|c| top_voxel(c.height))
                        .min()
                        .unwrap_or(top as i32);
                    soil_rows(top as i32, lowest)
                };
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
                        if at < water {
                            *m = M.water;
                        }
                        continue;
                    }
                    let depth = (top - (a + cell - 1)).max(0);
                    if depth > 0 {
                        *m = fill_soil(col.band, top as i32, depth.min(1000), lod_soil(x, z));
                        continue;
                    }
                    // The column's top: its surface seen from above. Water no cell sampled shows
                    // as the top.
                    let water_above = a + cell < water;
                    *m = if wet && !water_above {
                        M.water
                    } else {
                        fill(col.band, top as i32, 0)
                    };
                    if let Some(s) = surface.as_deref_mut() {
                        s[lod::column_index(x, z)] = LodSurface {
                            valid: true,
                            wet,
                            height: rows(col.height).min((a + cell) as f32),
                            material: fill(col.band, top as i32, 0),
                            water: if wet { water as f32 } else { 0.0 },
                        };
                    }
                }
            }
        }
        self.lod_trees(c.level, o, &cols, cells, surface);
        self.lod_monuments(c.level, o, cells);
        (kind, bounds)
    }

    /// The monuments the LOD draws at `level` reaching a section from `o`: at levels whose cells
    /// are at most `monument_lod_cell` m.
    fn lod_monuments_near(&self, o: lod::LodOrigin, level: i32) -> Vec<Placed> {
        let cell = lod::cell_size(level);
        match &self.portals {
            Some(p) if p.monument.is_some() && cell <= p.rules.monument_lod_cell => {}
            _ => return Vec::new(),
        }
        let n = i64::from(lod::SECTION_CELLS);
        let lo = ((o.x - cell) as i32, (o.z - cell) as i32);
        let hi = ((o.x + (n + 1) * cell) as i32, (o.z + (n + 1) * cell) as i32);
        self.monuments_near(lo, hi)
    }

    /// Lifts a section's open bound over the monuments it draws, so a section holding only a
    /// monument's top isn't sky.
    fn raise_for_monuments(&self, b: &mut LodBounds, o: lod::LodOrigin, level: i32) {
        if !b.any_inside {
            return;
        }
        for m in self.lod_monuments_near(o, level) {
            b.hi = b.hi.max(f64::from(m.max[1] + 1));
        }
    }

    /// The monuments in a section: a cell holding any of a monument's cells shows it (the first
    /// one met), over air only, so a pillar narrower than a cell still reads from afar.
    fn lod_monuments(&self, level: i32, o: lod::LodOrigin, cells: &mut LodCells) {
        let cell = lod::cell_size(level);
        let n = i64::from(lod::SECTION_CELLS);
        for m in self.lod_monuments_near(o, level) {
            for (p, state) in m.cells() {
                let at = |p: i32, o: i64| (i64::from(p) - o).div_euclid(cell);
                let (xi, yi, zi) = (at(p[0], o.x), at(p[1], o.y), at(p[2], o.z));
                if [xi, yi, zi].iter().any(|&i| !(-1..=n).contains(&i)) {
                    continue;
                }
                let c = &mut cells[lod::cell_index(xi as i32, yi as i32, zi as i32)];
                if *c == M.air {
                    *c = state;
                }
            }
        }
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

    /// Feet position for players (`terrain.json` `spawn`): on level dry land between the spawn
    /// heights, by a river when one is near: the map's suitable cells nearest the disc's centre
    /// first, each searched round its centre for a level patch clear of structures and portals.
    /// Falls back to the level dry ground nearest the origin.
    pub fn spawn_point(&self) -> [f64; 3] {
        for (x, z) in self.spawn_candidates(48) {
            if let Some(p) = self.spawn_near(x, z, 12) {
                return p;
            }
        }
        let mut fallback = None;
        self.spawn_near(0, 0, 5000)
            .or_else(|| {
                fallback = self.ground_y(0, 0).map(|g| [0.5, m_of_row(g + 1), 0.5]);
                fallback
            })
            .unwrap_or([0.5, m_of_row(SEA_LEVEL + 1), 0.5])
    }

    /// Up to `most` map cells to look for a spawn in: dry land between the spawn heights, not in
    /// a lake, within `spawn.nearRiver` m of a river vertex; nearest the disc's centre first.
    fn spawn_candidates(&self, most: usize) -> Vec<(i32, i32)> {
        let map = &*self.map;
        let n = map.n as i64;
        let reach = (S.spawn_near_river / map.cell).ceil() as i64;
        let mut near = vec![false; map.n * map.n];
        for r in &map.rivers {
            for v in &r.path {
                let c = map.cell_at(f64::from(v.x), f64::from(v.z));
                let (ci, cj) = ((c % map.n) as i64, (c / map.n) as i64);
                for dj in -reach..=reach {
                    for di in -reach..=reach {
                        let (i, j) = (ci + di, cj + dj);
                        if (0..n).contains(&i)
                            && (0..n).contains(&j)
                            && ((di * di + dj * dj) as f64) * map.cell * map.cell
                                <= S.spawn_near_river * S.spawn_near_river
                        {
                            near[(j * n + i) as usize] = true;
                        }
                    }
                }
            }
        }
        let mut cells: Vec<(i64, usize)> = (0..map.n * map.n)
            .filter(|&c| {
                let h = f64::from(map.height[c]);
                near[c]
                    && !map.is_sea(c)
                    && map.lake[c] == worldmap::NO_LAKE
                    && (S.spawn_min_height..=S.spawn_max_height).contains(&h)
            })
            .map(|c| {
                let (x, z) = map.centre(c);
                ((x * x + z * z) as i64, c)
            })
            .collect();
        cells.sort_unstable();
        cells
            .into_iter()
            .take(most)
            .map(|(_, c)| {
                let (x, z) = map.centre(c);
                (x as i32, z as i32)
            })
            .collect()
    }

    /// A spawn on a level dry patch found by a square spiral of 8 m steps round (cx, cz), at most
    /// `steps` legs out.
    fn spawn_near(&self, cx: i32, cz: i32, steps: i32) -> Option<[f64; 3]> {
        let (mut x, mut z, mut dx, mut dz) = (cx, cz, 1i32, 0i32);
        let (mut leg, mut walked, mut turns) = (1, 0, 0);
        while leg <= steps {
            if let Some(p) = self.spawn_patch(x, z) {
                return Some(p);
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
        None
    }

    /// The feet on column (x, z) when it is the centre of a level dry patch SPAWN_CLEAR columns
    /// either side, between the spawn heights, clear of structures and portals.
    fn spawn_patch(&self, x: i32, z: i32) -> Option<[f64; 3]> {
        let col = self.column_at(x, z);
        if col.outside || col.wet() {
            return None;
        }
        let g = top_voxel(col.height);
        let above = m_of_row(g + 1) - SEA_LEVEL_M;
        if above < S.spawn_min_height.min(1.0) || above > S.spawn_max_height * 2.0 {
            return None;
        }
        let level = (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|pz| {
            (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|px| {
                let c = self.column_at(x + px, z + pz);
                !c.outside && !c.wet() && top_voxel(c.height) == g
            })
        });
        if !level {
            return None;
        }
        // Never in (or under) a placed structure: the patch is clear of their boxes; nor by a
        // portal's opening.
        let lo = (x - SPAWN_CLEAR, z - SPAWN_CLEAR);
        let hi = (x + SPAWN_CLEAR, z + SPAWN_CLEAR);
        if self
            .portals
            .as_ref()
            .is_some_and(|p| p.box_reaches_clear(lo, hi))
            || !self.trees_in(lo, hi).is_empty()
        {
            return None;
        }
        let feet = self.surface_y_at(x, z).unwrap_or(m_of_row(g + 1));
        Some([f64::from(x) + 0.5, feet, f64::from(z) + 0.5])
    }
}

/// 0 below 0, 1 above 1, smooth (C1) between.
fn smooth01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A smooth minimum of a and b, rounding the corner over about `k` m.
fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
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
        for m in [M.grass, M.sand, M.dirt, M.stone, bands().snow] {
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
