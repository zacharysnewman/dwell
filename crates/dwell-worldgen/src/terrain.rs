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
use crate::volume::{ColumnEval, ColumnInput, Volume, VolumeDef};
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
    /// The coast broken into islands (`terrain.json` `islands`, ADR 0074 B4, the archipelago's
    /// knob): with `island_amplitude` > 0 the coast distance is capped at `island_cap` m and offset
    /// by a fractal noise of about ±`island_amplitude` m (`island_wavelength` m, `island_octaves`);
    /// 0 leaves the one island as it was.
    pub island_amplitude: f32,
    pub island_wavelength: i32,
    pub island_octaves: u32,
    pub island_cap: f32,
}

/// A biome's water look over its fluid's (`waterLook`, ADR 0074 B4): its colour (`scatter`, sRGB
/// 0xRRGGBB) and clarity (`absorption`, per metre of red, green, blue); a field left out keeps the
/// fluid's (`shared/blocks/dwell.json` `fluids`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterLook {
    pub scatter: Option<u32>,
    pub absorption: Option<[f32; 3]>,
}

/// The water's look resolved at a column: its scatter colour (linear 0..1) and absorption (per
/// m), blended by the biomes' weights there (`TerrainGenerator::water_look_at`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterTint {
    pub scatter: [f32; 3],
    pub absorption: [f32; 3],
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
    /// The ground round a lake stands at least `lake_bank` m over its water, falling
    /// `lake_bank_slope` m per m from 2 m out (the biome's `banks`).
    pub lake_bank: f64,
    pub lake_bank_slope: f64,
    /// Dry ground within this many m of a river's or lake's water takes the shore band (a beach;
    /// the biome's `banks.beach`, 0 none).
    pub beach: f64,
    pub lake_levee: f64,
    pub lake_shore_noise: f64,
    pub lake_shore_wavelength: i32,
    /// An elevation band's lower edge is moved ± this (m) by a noise of `edge_wavelength` m, and
    /// raised `edge_slope_lift` m per unit of slope.
    pub edge_noise: f64,
    pub edge_wavelength: i32,
    pub edge_slope_lift: f64,
    pub rock_slope: f64,
    /// The rock band's slope threshold at `rock_high` m (soil thins with height): it eases from
    /// `rock_slope` at sea level to this.
    pub rock_slope_high: f64,
    pub rock_high: f64,
    /// The landform's slope for the rock band is taken over ± this many metres (a hillside, not
    /// a map cell), so rock comes in whole faces, not along the map's grid.
    pub rock_window: f64,
    pub shore: f64,
    pub under_depth: f64,
    pub spawn_min_height: f64,
    pub spawn_max_height: f64,
    pub spawn_near_river: f64,
}

/// One of a biome's elevation bands (ADR 0074 B2): from `from` m above the sea (its edge moved by
/// the biome's `edge_*` noise and slope lift) to the next band's, its surface blocks by slope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevationBand {
    pub name: &'static str,
    /// The band's lower edge (m above the sea); the lowest band's is `-inf`.
    pub from: f64,
    pub ground: &'static str,
    pub under: &'static str,
    pub rock: &'static str,
    pub shore_block: &'static str,
}

/// A range of one geography field a biome belongs in (`where`), eased over ± `blend` at each
/// finite bound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhereRange {
    pub min: f64,
    pub max: f64,
    pub blend: f64,
}

impl WhereRange {
    /// No bound: fits everywhere.
    pub const ANY: Self = Self {
        min: f64::NEG_INFINITY,
        max: f64::INFINITY,
        blend: 0.0,
    };
}

/// Where a biome belongs in the world's geography (`biomemap.rs`): its weight and ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Where {
    pub weight: f64,
    pub temperature: WhereRange,
    pub wetness: WhereRange,
    pub elevation: WhereRange,
    pub coast: WhereRange,
}

/// The biome map's geography (`terrain.json` `biomeMap`; `biomemap.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeMap {
    pub climate_wavelength: i32,
    pub climate_noise: f64,
    pub lapse_rate: f64,
    pub wet_reach: f64,
    pub dither_wavelength: i32,
    pub dither: f64,
}

/// A biome (data: `shared/worldgen/biomes/<name>.json`, ADR 0074): what is layered on the world's
/// elevation (`terrain.json`, one world-scale system: [`HEIGHT`] and the rivers' and lakes'
/// shaping) to make a landscape read as one place: where it belongs (`place`, read by the biome
/// map), its bounded fine relief and surface shaping (merged in `surface` with the world's water
/// shaping) and its elevation bands. A generator's biomes ([`TerrainGenerator::with_biomes`]; the
/// build's [`BIOMES`] by default) are weighted per column by the biome map (`biomemap.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biome {
    /// The biome's name (`namespace:name`).
    pub name: &'static str,
    pub place: Where,
    pub surface: Surface,
    /// The elevation bands, lowest first.
    pub bands: &'static [ElevationBand],
    /// The decoration rules (`decorations.json`, by name) that may grow here.
    pub decorations: &'static [&'static str],
    /// The structure placement rules (`placements.json`, by name) that may stand here.
    pub placements: &'static [&'static str],
    /// The fluid block the biome's water is drawn as.
    pub water: &'static str,
    /// Its water's colour and clarity over the fluid's look (none: the fluid's own).
    pub water_look: Option<WaterLook>,
}

mod table {
    include!("terrain.gen.rs");
}
pub use table::{BIOME_MAP, BIOMES, FIXTURE_BIOMES, FIXTURE_WORLDS, HEIGHT, SURFACE};

/// The biome named `name` (`shared/worldgen/biomes`), if the build has it.
pub fn find_biome(name: &str) -> Option<&'static Biome> {
    BIOMES.iter().find(|b| b.name == name)
}

const S: Surface = SURFACE;
/// Sand at the shore and under water to SAND_DEPTH (rows, from metres).
const SAND_DEPTH: i64 = (3.0 / CELL_H) as i64;

/// The deepest a lake's bed is forced below its level near its shore (m): deeper beds are the map's.
const LAKE_DEPTH: f64 = 6.0;

/// The spawn point's level patch reaches this many columns either side.
pub const SPAWN_CLEAR: i32 = 4;

/// The spawn's wider search (WORLD_SCALE.md §3.2): the origin's spiral reaches this many legs of
/// 8 m (20 km out, 12.5 M patches, probed over the cores; the 50 km disc's seeds spawn within it)...
const SPAWN_ORIGIN_LEGS: i32 = 5000;
/// A spiral's patches probed at once (natively shared over the cores).
const SPAWN_BATCH: usize = 1 << 14;
/// ... then up to this many suitable map cells, one per block this many cells a side, are each
/// searched 12 legs round (~80 patches a cell, three times over as the level relaxes).
const SPAWN_SPREAD: usize = 256;
const SPAWN_BLOCK: i32 = 8;

/// How many reshaped columns each thread keeps (`TerrainGenerator::column`): a few chunk columns'
/// worth with their trees' margins; all are dropped when it fills.
const SHAPED_KEPT: usize = 1 << 16;

thread_local! {
    /// Reshaped columns by generator (`cache_id`), x, z and cell: a cache only, the same values
    /// either way.
    static SHAPED: std::cell::RefCell<FastMap<(u64, i64, i64, i64), Shaped>> =
        std::cell::RefCell::new(FastMap::default());
    /// Height-field columns by generator, x, z and cell, where a terrain volume reads them (its
    /// own and its slope's neighbours, each read by several columns): a cache only.
    static BASE: std::cell::RefCell<FastMap<(u64, i64, i64, i64), Column>> =
        std::cell::RefCell::new(FastMap::default());
}

/// A hash map keyed by small integer tuples, with a cheap multiplicative hash (the default
/// SipHash was a twentieth of a cliff chunk's time): a cache's lookups, never iterated.
type FastMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FastHash>>;

#[derive(Default)]
struct FastHash(u64);

impl FastHash {
    fn mix(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for FastHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.mix(u64::from(b));
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.mix(v);
    }
    fn write_i64(&mut self, v: i64) {
        self.mix(v as u64);
    }
}

/// A column as the terrain volume left it, with which of its rows within the volume's reach of
/// the height field's top are solid (`Rows`), where the volume reshapes it: what the column's
/// fill, the carving and the LOD all read, worked out once.
#[derive(Clone, Copy, Debug)]
struct Shaped {
    col: Column,
    rows: Option<Rows>,
}

/// The solid rows of a reshaped column: row `base_top − reach + i` is bit i, for i in
/// 0..=2 × reach, the density's where it is over the core (the core over the midplane is never
/// reshaped); beyond them the height field stands.
#[derive(Clone, Copy, Debug)]
struct Rows {
    base_top: i32,
    reach: i32,
    bits: [u64; ROWS_WORDS],
}

/// Words of `Rows::bits`: a reach of 64 m is 128 rows each side, 257 rows.
const ROWS_WORDS: usize = 5;

impl Rows {
    fn solid(&self, y: i32) -> bool {
        let i = y - (self.base_top - self.reach);
        if i < 0 || i > 2 * self.reach {
            y <= self.base_top
        } else {
            self.bits[(i / 64) as usize] >> (i % 64) & 1 == 1
        }
    }
}

/// A key no generator has had (`TerrainGenerator::cache_id`).
fn next_cache_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

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
    /// The generator's surface palette the column takes its blocks from (its biome's elevation
    /// band, `BiomeSet::palettes`).
    pub palette: u16,
    /// Beyond the world's disc: nothing is generated.
    pub outside: bool,
    /// How far (m) the nearest river channel's edge or lake shore lies: 0 in the water, `FAR_WATER`
    /// with none near (the terrain volume's `water` input: cliffs keep off the rivers).
    pub shore: f32,
    /// The height field's ground height (m, as `height`), before a terrain volume reshaped it.
    pub base: f32,
    /// Part of a river's lip (its crest, wall, or a rock at its foot; ADR 0058): its top stays
    /// flat (no ramp pieces), so no ramp's open part lets the water by.
    pub lip: bool,
    /// In a chute's stream (ADR 0059): running water over its top, no still water.
    pub stream: bool,
}

/// A column's bank shaping (a biome's `banks`, or the biomes' blend at a column).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Banks {
    river_slope: f64,
    lake: f64,
    lake_slope: f64,
    beach: f64,
}

impl Banks {
    fn of(s: &Surface) -> Self {
        Self {
            river_slope: s.river_valley_slope,
            lake: s.lake_bank,
            lake_slope: s.lake_bank_slope,
            beach: s.beach,
        }
    }
}

/// An sRGB 0xRRGGBB colour, linear 0..1.
fn linear_rgb(c: u32) -> [f32; 3] {
    let ch = |v: u32| {
        let s = (v & 0xff) as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [ch(c >> 16), ch(c >> 8), ch(c)]
}

/// A biome's water look: its `waterLook` over its fluid block's look (the registry's `fluids`).
pub fn water_tint_of(b: &Biome) -> WaterTint {
    let fluid = dwell_blocks::state(b.water)
        .and_then(|id| dwell_blocks::STATES[usize::from(id)].fluid)
        .map(|f| &dwell_blocks::FLUIDS[usize::from(f)])
        .unwrap_or_else(|| dwell_blocks::require_fluid("dwell:water"));
    let (scatter, absorption) = (fluid.scatter, fluid.absorption);
    let l = b.water_look.unwrap_or(WaterLook {
        scatter: None,
        absorption: None,
    });
    WaterTint {
        scatter: linear_rgb(l.scatter.unwrap_or(scatter)),
        absorption: l.absorption.unwrap_or(absorption),
    }
}

/// A column's `shore` with no river valley or lake shore reaching it.
pub const FAR_WATER: f32 = 1000.0;

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

/// The cells across a river's lip that pass `q` m³/s with the water a full row over the sill
/// (water's weir, `dwell_world::flow`): at least one (docs/WATER_FLOW.md §17).
pub fn lip_cells(q: f64) -> u32 {
    let per_cell = dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, CELL_H);
    if per_cell <= 0.0 {
        return 1;
    }
    ((q / per_cell).round() as u32).max(1)
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

/// Water's running block (a chute's stream, ADR 0059), resolved by name once.
fn running_water() -> StateId {
    static RUNNING: std::sync::LazyLock<StateId> =
        std::sync::LazyLock::new(|| dwell_blocks::require("dwell:running_water"));
    *RUNNING
}

/// The bedrock's state (`shared/worldgen/midplane.json`), resolved by name once.
fn bedrock() -> StateId {
    static BEDROCK: std::sync::LazyLock<StateId> =
        std::sync::LazyLock::new(|| dwell_blocks::require(MIDPLANE.bedrock));
    *BEDROCK
}

/// An elevation band's blocks, resolved by name once.
#[derive(Clone, Copy, Debug)]
pub struct Bands {
    pub ground: StateId,
    pub under: StateId,
    pub rock: StateId,
    pub shore: StateId,
}

impl Bands {
    /// A band's blocks, resolved by name (panics on a name the registry lacks: the generator's
    /// validation checks the checked-in biomes).
    pub fn of(b: &ElevationBand) -> Self {
        Self {
            ground: dwell_blocks::require(b.ground),
            under: dwell_blocks::require(b.under),
            rock: dwell_blocks::require(b.rock),
            shore: dwell_blocks::require(b.shore_block),
        }
    }
}

/// A generator's biomes: each one's elevation bands resolved as palettes, and the biome map
/// weighting them (none with one biome: it is everywhere).
#[derive(Debug)]
pub struct BiomeSet {
    pub list: Vec<Biome>,
    /// Every biome's bands' blocks, biome by biome (`first[i]` is biome i's lowest band).
    pub palettes: Vec<Bands>,
    first: Vec<u16>,
    field: Option<crate::biomemap::BiomeField>,
}

impl BiomeSet {
    /// `list` (the world's default first) over `map`.
    pub fn new(list: &[Biome], map: &WorldMap, world_seed: u64) -> Self {
        Self::with_map(list, map, world_seed, BIOME_MAP)
    }

    /// [`BiomeSet::new`] with the biome map's geography `params` (`terrain.json` `biomeMap`).
    pub fn with_map(list: &[Biome], map: &WorldMap, world_seed: u64, params: BiomeMap) -> Self {
        assert!(!list.is_empty(), "a world has at least one biome");
        let mut palettes = Vec::new();
        let mut first = Vec::new();
        for b in list {
            first.push(palettes.len() as u16);
            palettes.extend(b.bands.iter().map(Bands::of));
        }
        let field = (list.len() > 1)
            .then(|| crate::biomemap::BiomeField::with_params(map, world_seed, params));
        Self {
            list: list.to_vec(),
            palettes,
            first,
            field,
        }
    }

    /// The geography at (x, z) over the world height field's `elevation` m (None with one biome:
    /// no map is built).
    pub fn geography(&self, x: i64, z: i64, elevation: f64) -> Option<crate::biomemap::Geography> {
        self.field.as_ref().map(|f| f.geography(x, z, elevation))
    }

    /// The biomes' weights at (x, z) over the world height field's `elevation` m.
    pub fn mix(&self, x: i64, z: i64, elevation: f64) -> crate::biomemap::Mix {
        match &self.field {
            None => crate::biomemap::Mix::only(0),
            Some(f) => f.mix(&self.list, x, z, elevation),
        }
    }
}

/// The material `depth` rows below the surface of a column whose top voxel is `top` and band
/// `band`: the bedrock in the face's first `bedrock_rows` from the midplane, the core's stone over
/// it up to `CORE_LAYERS`, then the band's layers.
fn fill(b: &Bands, band: Band, top: i32, depth: i64) -> StateId {
    fill_soil(b, band, top, depth, under_rows())
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
fn fill_soil(b: &Bands, band: Band, top: i32, depth: i64, soil: i64) -> StateId {
    let y = i64::from(top) - depth;
    if y < i64::from(MIDPLANE_Y + MIDPLANE.bedrock_rows) {
        return bedrock();
    }
    if y < i64::from(MIDPLANE_Y + CORE_LAYERS) {
        return M.stone;
    }
    match band {
        Band::Shore => {
            if depth < SAND_DEPTH {
                b.shore
            } else {
                M.stone
            }
        }
        Band::Rock => b.rock,
        Band::Ground => {
            if depth == 0 {
                b.ground
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
    /// This generator's biomes (the build's unless `with_biomes`) and their surface palettes.
    biomes: Arc<BiomeSet>,
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
    /// This generator's key in the reshaped columns' cache (`SHAPED`): new whenever it is built or
    /// changed, so no cached column outlives what made it.
    cache_id: u64,
    /// The terrain volume over the height field (`volume.rs`, a prototype: world type `cliffs`);
    /// none for the plain terrain. Chunks only: the LOD draws the height field's top.
    volume: Option<Arc<Volume>>,
    world_seed: u64,
    /// Each river's rows (`RiverRows`: which are a chute's), built on demand.
    rows: Arc<std::sync::RwLock<HashMap<u32, Arc<RiverRows>>>>,
}

/// The ramp pieces of the natural surface materials, resolved by name once: for each material,
/// facing and whether flooded, a ½ m ramp's cell and a 1 m ramp's cells from the bottom.
#[derive(Debug, Default)]
struct RampPieces {
    by: HashMap<(StateId, RampFacing, bool), (StateId, Vec<StateId>)>,
}

impl RampPieces {
    fn resolve(palettes: &[Bands]) -> Self {
        let mut by = HashMap::default();
        let mut ms: Vec<StateId> = palettes
            .iter()
            .flat_map(|b| [b.ground, b.shore, b.under, b.rock])
            .collect();
        ms.sort_unstable();
        ms.dedup();
        for m in ms {
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
        let biomes = Arc::new(BiomeSet::new(BIOMES, &map, world_seed));
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
            ramps: Arc::new(RampPieces::resolve(&biomes.palettes)),
            planters: Vec::new(),
            biomes,
            portals: None,
            rise: DECORATION_RISE as f32,
            lod_valleys: None,
            volume: None,
            cache_id: next_cache_id(),
            world_seed,
            rows: Arc::default(),
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

    /// The generator with `biome` alone in place of the build's biomes (everywhere).
    pub fn with_biome(self, biome: &Biome) -> Self {
        self.with_biomes(std::slice::from_ref(biome))
    }

    /// The generator with `biomes` (the first the world's default, taking a column no biome
    /// fits) in place of the build's, weighted per column by the biome map (ADR 0074 B2).
    pub fn with_biomes(self, biomes: &[Biome]) -> Self {
        self.with_biomes_on(biomes, BIOME_MAP)
    }

    /// [`TerrainGenerator::with_biomes`] with the biome map's geography `params` in place of the
    /// build's (`terrain.json` `biomeMap`; Chisel's edits).
    pub fn with_biomes_on(mut self, biomes: &[Biome], params: BiomeMap) -> Self {
        let set = BiomeSet::with_map(biomes, &self.map, self.world_seed, params);
        self.ramps = Arc::new(RampPieces::resolve(&set.palettes));
        self.biomes = Arc::new(set);
        self.cache_id = next_cache_id();
        self
    }

    /// This generator's biomes.
    pub fn biomes(&self) -> &[Biome] {
        &self.biomes.list
    }

    /// The biomes' weights at face-local (x, z) (biome index, weight; summing to 1) and the one
    /// whose surface the column takes.
    pub fn biome_mix(&self, x: i64, z: i64) -> crate::biomemap::Mix {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        self.biomes.mix(x, z, base)
    }

    /// The geography at face-local (x, z) the biome map reads (None with one biome).
    pub fn biome_geography(&self, x: i64, z: i64) -> Option<crate::biomemap::Geography> {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        self.biomes.geography(x, z, base)
    }

    /// A column's surface palette.
    fn palette(&self, col: impl std::borrow::Borrow<Column>) -> &Bands {
        &self.biomes.palettes[usize::from(col.borrow().palette)]
    }

    /// The fine relief of `s` at (x, z) over the height field's `base` m, at a cell of `cell` m.
    fn detail_of(&self, s: &Surface, x: i64, z: i64, base: f64, cell: i64) -> f64 {
        let amplitude = s.detail_amplitude
            + (s.detail_mountain_amplitude - s.detail_amplitude)
                * f64::from(smooth_step(
                    s.detail_mountain_from as f32,
                    s.detail_mountain_full as f32,
                    base as f32,
                ));
        let kept = octaves_resolved(s.detail_wavelength, s.detail_octaves, cell);
        if kept == 0 {
            0.0
        } else {
            2.0 * amplitude
                * f64::from(fbm2(
                    self.detail_seed,
                    x + self.offset_x,
                    z + self.offset_z,
                    s.detail_wavelength,
                    s.detail_octaves,
                    Some(kept),
                ))
        }
    }

    /// The fine relief at (x, z) blended by the biomes' weights (`mix`): each biome's own,
    /// weighted; one biome's alone where it is the only one.
    fn blended_detail(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        base: f64,
        cell: i64,
    ) -> f64 {
        if mix.len == 1 {
            let s = &self.biomes.list[usize::from(mix.weights[0].0)].surface;
            return self.detail_of(s, x, z, base, cell);
        }
        mix.iter()
            .map(|(i, w)| w * self.detail_of(&self.biomes.list[i].surface, x, z, base, cell))
            .sum()
    }

    /// The biomes' bank shaping at a column, blended by their weights (`mix`): exactly the
    /// biome's own where one biome is everywhere.
    fn blended_banks(&self, mix: &crate::biomemap::Mix) -> Banks {
        let of = |i: usize| Banks::of(&self.biomes.list[i].surface);
        if mix.len == 1 {
            return of(usize::from(mix.weights[0].0));
        }
        let mut b = Banks::default();
        for (i, w) in mix.iter() {
            let o = of(i);
            b.river_slope += w * o.river_slope;
            b.lake += w * o.lake;
            b.lake_slope += w * o.lake_slope;
            b.beach += w * o.beach;
        }
        b
    }

    /// The water's look at face-local (x, z) (ADR 0074 B4): each biome's `waterLook` over its
    /// fluid's look, blended by the biomes' weights there (so it eases across a border). Clients
    /// compute it from the build's biome data as they generate terrain; nothing is sent.
    pub fn water_look_at(&self, x: i64, z: i64) -> WaterTint {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let mix = self.biomes.mix(x, z, base);
        let mut t = WaterTint {
            scatter: [0.0; 3],
            absorption: [0.0; 3],
        };
        for (i, w) in mix.iter() {
            let o = water_tint_of(&self.biomes.list[i]);
            for k in 0..3 {
                t.scatter[k] += w as f32 * o.scatter[k];
                t.absorption[k] += w as f32 * o.absorption[k];
            }
        }
        t
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
        self.cache_id = next_cache_id();
        self
    }

    /// The generator with a terrain volume shaping its chunks (`volume.rs`); the sky test leaves
    /// room for what it adds over the ground.
    pub fn with_volume(mut self, def: &'static VolumeDef) -> Self {
        let v = Volume::resolve(def, self.world_seed);
        self.rise = self.rise.max((v.reach_rows + 1) as f32);
        self.volume = Some(Arc::new(v));
        self.cache_id = next_cache_id();
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
    fn rock_at(
        &self,
        s: &Surface,
        map: &crate::worldmap::WorldMap,
        x: f64,
        z: f64,
        h: f64,
        cell: i64,
    ) -> bool {
        let w = s.rock_window.max(cell as f64);
        let hx = map.height_at(x + w, z).0 - map.height_at(x - w, z).0;
        let hz = map.height_at(x, z + w).0 - map.height_at(x, z - w).0;
        let slope = hx.hypot(hz) / (2.0 * w);
        let t = (h / s.rock_high).clamp(0.0, 1.0);
        let threshold = s.rock_slope + (s.rock_slope_high - s.rock_slope) * t * t * (3.0 - 2.0 * t);
        slope >= threshold
    }

    /// The column at (x, z) as seen at a cell of `cell` m: 1 for chunks; the LOD's coarser cells
    /// drop the detail octaves finer than a cell and widen rivers to show from afar. With a terrain
    /// volume its top is the reshaped ground's topmost solid cell (WORLD_GENERATION.md §5), so the
    /// fill, the ramps, the decorations, the trees and the LOD all stand on the final surface.
    fn column(&self, x: i64, z: i64, cell: i64) -> Column {
        match &self.volume {
            None => self.base_column(x, z, cell),
            Some(_) => self.shaped_column(x, z, cell).col,
        }
    }

    /// The column at (x, z) under the terrain volume, with its solid rows where the volume
    /// reshapes it (none without a volume, or where it leaves the column alone).
    fn shaped_column(&self, x: i64, z: i64, cell: i64) -> Shaped {
        let Some(v) = &self.volume else {
            return Shaped {
                col: self.base_column(x, z, cell),
                rows: None,
            };
        };
        // A reshaped column costs a scan of the density: each thread keeps the latest ones, as
        // the chunks stacked over a column, their neighbours, the carving and the trees all ask.
        let key = (self.cache_id, x, z, cell);
        if let Some(c) = SHAPED.with(|m| m.borrow().get(&key).copied()) {
            return c;
        }
        let base = self.base_cached(x, z, cell);
        let c = if base.outside {
            Shaped {
                col: base,
                rows: None,
            }
        } else {
            self.shaped(v, x, z, cell, base)
        };
        SHAPED.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= SHAPED_KEPT {
                m.clear();
            }
            m.insert(key, c);
        });
        c
    }

    /// `base` (the height field's column at (x, z)) with its top raised or lowered to the volume's
    /// topmost solid cell, and its rows within the reach.
    fn shaped(&self, v: &Volume, x: i64, z: i64, cell: i64, base: Column) -> Shaped {
        let plain = Shaped {
            col: base,
            rows: None,
        };
        let Some(eval) = self.volume_eval(v, x, z, cell, &base) else {
            return plain;
        };
        if eval.plain {
            return plain;
        }
        let base_top = top_voxel(base.height);
        let reach = v.reach_rows;
        let mut rows = Rows {
            base_top,
            reach,
            bits: [0; ROWS_WORDS],
        };
        // Within the reach and over the core the density decides (`volume_solid`), elsewhere the
        // height field.
        let core = MIDPLANE_Y + CORE_LAYERS;
        let mut highest = None;
        let (lo, hi) = (base_top - reach, base_top + reach);
        let mut set = |y: i32, solid: bool| {
            if solid {
                let i = y - lo;
                rows.bits[(i / 64) as usize] |= 1 << (i % 64);
                highest = Some(y);
            }
        };
        for y in lo..core.min(hi + 1) {
            set(y, y <= base_top);
        }
        v.densities(&eval, lo.max(core)..=hi, |y, d| set(y, d > 0.0));
        // Dry ground above the sea is never lowered under its surface (it would stand open beside
        // the sea's water).
        let top = highest
            .unwrap_or(base_top - reach - 1)
            .max(base_top.min(Self::sea_top()));
        let col = if top == base_top {
            base
        } else {
            Column {
                height: ((f64::from(top) + 1.0) * CELL_H) as f32,
                ..base
            }
        };
        Shaped {
            col,
            rows: Some(rows),
        }
    }

    /// The volume's column-constant part at (x, z) over the height field's column `base`; none in a
    /// portal's columns (its shaft and rim stand on the height field).
    fn volume_eval(
        &self,
        v: &Volume,
        x: i64,
        z: i64,
        cell: i64,
        base: &Column,
    ) -> Option<ColumnEval> {
        let (xi, zi) = (x as i32, z as i32);
        // Water is never reshaped: a lake, a river or the sea keeps its bed.
        if base.water != Water::None {
            return None;
        }
        if let Some((p, near)) = self.portals_near((xi, zi), (xi, zi))
            && p.locate(&near, xi, zi).0 != Part::None
        {
            return None;
        }
        let h = base.height;
        let at = |dx: i64, dz: i64| {
            let c = self.base_cached(x + dx, z + dz, cell);
            if c.outside { h } else { c.height }
        };
        // The height field's slope over ±r m, measured only where the density asks for it.
        let slope = |r: u8| {
            let r = i64::from(r);
            let sx = (at(r, 0) - at(-r, 0)) / (2 * r) as f32;
            let sz = (at(0, r) - at(0, -r)) / (2 * r) as f32;
            (sx * sx + sz * sz).sqrt()
        };
        Some(v.column(&ColumnInput {
            x: xi,
            z: zi,
            surface: m_of_row(top_voxel(h) + 1) as f32,
            slope: &slope,
            water: base.shore,
        }))
    }

    /// The row under the sea's surface (face-local): the lowest a volume leaves dry ground's top.
    fn sea_top() -> i32 {
        top_voxel(SEA_LEVEL_M as f32)
    }

    /// [`TerrainGenerator::base_column`] through this thread's cache (`BASE`).
    fn base_cached(&self, x: i64, z: i64, cell: i64) -> Column {
        let key = (self.cache_id, x, z, cell);
        if let Some(c) = BASE.with(|m| m.borrow().get(&key).copied()) {
            return c;
        }
        let c = self.base_column(x, z, cell);
        BASE.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() >= SHAPED_KEPT {
                m.clear();
            }
            m.insert(key, c);
        });
        c
    }

    /// The height field's column at (x, z) (`column` without the volume).
    /// The row a river's or lake's water surface lies on at full-detail column (x, z) (the rows
    /// below it water); none where no river channel or lake is. A light lookup for the waterline
    /// (docs/WATER_FLOW.md §17).
    /// The river whose channel holds full-detail column (x, z), and its surface row (none off a
    /// channel, or at the sea's level).
    fn river_row_at(&self, x: i64, z: i64) -> Option<(u32, i32)> {
        if !inside_world_disc(x, z) || !self.map.channels.near(x as f64, z as f64, 0.0) {
            return None;
        }
        let (wx, wz) = self.warp(
            self.meander_seeds,
            x,
            z,
            S.river_meander,
            S.river_meander_wavelength,
        );
        let r = self.map.channels.query(wx, wz, 0.0, f64::INFINITY)?;
        (r.d < r.half && r.level > CELL_H)
            .then(|| (r.river, surface_row((SEA_LEVEL_M + r.level) as f32)))
    }

    fn water_row_at(&self, x: i64, z: i64) -> Option<i32> {
        if !inside_world_disc(x, z) {
            return None;
        }
        let map = &*self.map;
        if !map.shores.fields.is_empty() {
            let (sx, sz) = self.warp(
                self.shore_seeds,
                x,
                z,
                S.lake_shore_noise,
                S.lake_shore_wavelength,
            );
            if let Some(l) = map.shores.query(map, sx, sz)
                && l.sdf > 0.0
            {
                return Some(surface_row((SEA_LEVEL_M + l.level) as f32));
            }
        }
        if !map.channels.near(x as f64, z as f64, 0.0) {
            return None;
        }
        let (wx, wz) = self.warp(
            self.meander_seeds,
            x,
            z,
            S.river_meander,
            S.river_meander_wavelength,
        );
        let r = map.channels.query(wx, wz, 0.0, f64::INFINITY)?;
        (r.d < r.half && r.level > CELL_H).then(|| surface_row((SEA_LEVEL_M + r.level) as f32))
    }

    fn base_column(&self, x: i64, z: i64, cell: i64) -> Column {
        self.base_column_with(x, z, cell, true)
    }

    /// `base_column`, or with `beds` false the ground with the rivers' valleys but no beds: a
    /// channel's columns take the valley floor at its banks' height (no bed cut, no lips, no water
    /// drawn), for the water surface grid's prototype (Phase 53 G0, ADR 0060).
    fn base_column_with(&self, x: i64, z: i64, cell: i64, beds: bool) -> Column {
        if !inside_world_disc(x, z) {
            return Column {
                outside: true,
                ..Column::default()
            };
        }
        let map = &*self.map;
        let (base, gx, gz) = map.height_at(x as f64, z as f64);
        // The biomes here (by the world height field, which they never change beyond their fine
        // relief) and the one whose surface the column takes.
        let mix = self.biomes.mix(x, z, base);
        let biome = &self.biomes.list[usize::from(mix.chosen)];
        let bs = &biome.surface;
        // The fine detail, stronger on mountains, blended by the biomes' weights.
        let detail = self.blended_detail(&mix, x, z, base, cell);
        // How the banks look here (the biomes' `banks`, blended by weight; the elevation placed
        // the water).
        let banks = self.blended_banks(&mix);
        // The nearest river channel, through the meander warp (none far from every river).
        let (min_half, min_width) = if cell > 1 && cell <= S.river_lod_max_cell {
            (
                0.5 * S.river_lod_width * cell as f64,
                S.river_lod_min_share * cell as f64,
            )
        } else {
            (0.0, f64::INFINITY)
        };
        let (river, warped) = if map.channels.near(x as f64, z as f64, min_half) {
            let (wx, wz) = self.warp(
                self.meander_seeds,
                x,
                z,
                S.river_meander,
                S.river_meander_wavelength,
            );
            (map.channels.query(wx, wz, min_half, min_width), (wx, wz))
        } else {
            (None, (0.0, 0.0))
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
            fade = fade.min(smooth01((r.d - r.half) / bs.detail_water_fade.max(1e-6)));
        }
        if let Some(l) = &lake {
            fade = fade.min(smooth01(-l.sdf / bs.detail_water_fade.max(1e-6)));
        }
        let mut h = base + detail * fade;
        let mut on_lip = false;
        let mut stream = false;
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
                let required = l.level + banks.lake - (out - 2.0).max(0.0) * banks.lake_slope;
                h = h.max(required);
            }
        }
        if let Some(r) = &river {
            let edge = r.d - r.half;
            if edge < 0.0 && beds {
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
                    // No lips (BUGS.md #29, Phase 53 G1a): a step is the water falling a row from
                    // one pool to the next, the bed under each at its own depth; the water grid
                    // rests what is drawn (WATER_SURFACE_GRID.md §2b), so nothing holds a pool up.
                    // A chute (G2, ADR 0059): steps too close for pools; its bed a row under its
                    // surface, dry but for a stream running down its middle as wide as its river's
                    // flow's cells (on any heading).
                    // So is a column of a river standing two rows or more over another's water
                    // beside it (where a river joins one lower than it reaches): no wall of water.
                    let row = surface_row((SEA_LEVEL_M + r.level) as f32);
                    let over_another = || {
                        [(1, 0), (-1, 0), (0, 1), (0, -1)]
                            .into_iter()
                            .any(|(dx, dz)| {
                                self.river_row_at(x + dx, z + dz)
                                    .is_some_and(|(n, k)| n != r.river && k <= row - 2)
                            })
                    };
                    if cell == 1
                        && (self.river_rows(r.river).steep.contains(&row) || over_another())
                    {
                        h = drawn - CELL_H;
                        water = Water::None;
                        level = 0.0;
                        on_lip = true;
                        let faces = (r.dir.0.abs() + r.dir.1.abs()).max(1.0);
                        let half = (0.5 * f64::from(lip_cells(r.q)) / faces).max(0.71);
                        stream = r.d < half;
                    }
                }
            } else if water != Water::Lake {
                // The valley down to the banks, blended back into the land at its reach: by every
                // channel whose valley reaches here, the lowest, so where two rivers' valleys meet
                // the ground never steps where the nearest changes (docs/BUGS.md 14).
                let valley_of = |r: &worldmap::Hit, h: f64| {
                    let edge = (r.d - r.half).max(0.0);
                    let valley = r.level + S.river_bank + edge * banks.river_slope;
                    let carved = smooth_min(h, valley, 2.0);
                    carved + (h - carved) * smooth01(edge / r.valley.max(1e-6))
                };
                let land = h;
                h = valley_of(r, land);
                map.channels
                    .each_near(warped.0, warped.1, min_half, min_width, |_, o| {
                        h = h.min(valley_of(&o, land));
                    });
                let bank = r.level + S.river_bank;
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
        // The waterline: dry ground beside a river's or lake's water stands a row over its top,
        // so no open cell lies below the water beside it, nor a ramp's open part in it (ADR 0057).
        let near_water = river.as_ref().is_some_and(|r| r.d - r.half < 2.0)
            || lake.as_ref().is_some_and(|l| l.sdf > -2.0);
        if beds && cell == 1 && water == Water::None && near_water {
            let top = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .filter_map(|(dx, dz)| self.water_row_at(x + dx, z + dz))
                .max();
            if let Some(k) = top {
                h = h.max(f64::from(k + 1) * CELL_H - SEA_LEVEL_M);
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
        // The band: shore (and every bed under water), rock where steep, else the ground; its
        // blocks those of the biome's elevation band at h (each band's edge moved by a noise and
        // raised on slopes, so the snow line, say, is no flat contour).
        let slope = (gx * gx + gz * gz).sqrt();
        // A beach: dry ground within the biomes' `beach` m of a river's or lake's water.
        let on_beach = banks.beach > 0.0 && {
            let edge = river
                .as_ref()
                .map_or(f64::INFINITY, |r| r.d - r.half)
                .min(lake.as_ref().map_or(f64::INFINITY, |l| -l.sdf));
            edge < banks.beach
        };
        let band = if wet || h < bs.shore || on_beach {
            Band::Shore
        } else if self.rock_at(bs, map, x as f64, z as f64, h, cell) {
            Band::Rock
        } else {
            Band::Ground
        };
        let mut level_band = 0usize;
        if biome.bands.len() > 1 && h + bs.edge_noise >= biome.bands[1].from {
            let noise = 2.0
                * bs.edge_noise
                * f64::from(fbm2(
                    self.snow_seed,
                    x + self.offset_x,
                    z + self.offset_z,
                    bs.edge_wavelength,
                    3,
                    Some(octaves_resolved(bs.edge_wavelength, 3, cell)),
                ));
            for (k, b) in biome.bands.iter().enumerate().skip(1) {
                if h >= b.from + noise + bs.edge_slope_lift * slope {
                    level_band = k;
                }
            }
        }
        let palette = self.biomes.first[usize::from(mix.chosen)] + level_band as u16;
        let mut shore = FAR_WATER;
        if let Some(r) = &river {
            shore = shore.min((r.d - r.half).max(0.0) as f32);
        }
        if let Some(l) = &lake {
            shore = shore.min((-l.sdf).max(0.0) as f32);
        }
        if water != Water::None {
            shore = 0.0;
        }
        Column {
            height,
            level,
            water,
            band,
            palette,
            outside: false,
            shore,
            base: height,
            lip: on_lip,
            stream,
        }
    }

    /// The flow (m³/s) through face-local column (x, z) by the world map (docs/WATER_FLOW.md §2.1):
    /// a river's where its channel lies, the flow its lips are sized for (its head's discharge
    /// in whole lip cells, more below each river joining it; ADR 0058), a lake's outflow (what
    /// its outlet river starts with) inside its shore; 0 elsewhere. The lookups are the column's
    /// own (`base_column`): the same warps, the same nearest channel.
    pub fn discharge_at(&self, x: i32, z: i32) -> f64 {
        let (x, z) = (i64::from(x), i64::from(z));
        let map = &self.map;
        if !map.shores.fields.is_empty() {
            let (sx, sz) = self.warp(
                self.shore_seeds,
                x,
                z,
                S.lake_shore_noise,
                S.lake_shore_wavelength,
            );
            if let Some(l) = map.shores.query(map, sx, sz)
                && l.sdf > 0.0
            {
                let id = l.lake + 1;
                return map
                    .rivers
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.from_lake == id)
                    .filter_map(|(i, _)| map.channels.courses.get(i)?.first())
                    .map(|p| p.q)
                    .fold(0.0, f64::max);
            }
        }
        // The flow its lips are sized for (stepped between junctions, ADR 0058): the water's
        // engine brings in at the loaded world's edge what a river's lips pass.
        if !map.channels.near(x as f64, z as f64, 0.0) {
            return 0.0;
        }
        let (wx, wz) = self.warp(
            self.meander_seeds,
            x,
            z,
            S.river_meander,
            S.river_meander_wavelength,
        );
        map.channels
            .query(wx, wz, 0.0, f64::INFINITY)
            .filter(|h| h.d <= h.half)
            .map_or(0.0, |h| h.q)
    }

    /// The current (m/s along x and z) of the river at face-local column (x, z) whose water there
    /// is `depth` m deep (Phase 53 G3b, docs/WATER_SURFACE_GRID.md §2c): downstream along its
    /// channel as the column sees it (the meander's warp undone: the map's direction through the
    /// warp's inverse, found from its neighbours), at the speed Manning's law gives that depth on the
    /// river's fall (its segment's, or its course's mean over `riverFallReachM` up and down stream
    /// where steeper: BUGS.md #42) with the drawn liquid's roughness (`flow.json`), the grid's own
    /// law, or that its
    /// flow (`river_flow`) gives over that depth where faster, so a level reach and a shallow one
    /// run too (owner, 2026-11-06: "dead rivers"); times `riverCurrentScale` (2: owner, 2026-11-09),
    /// at most `riverCurrentMaxMs`.
    /// None off a river's channel, under `currentDepthM` or slower than `currentStillMs`.
    pub fn river_current(&self, x: i32, z: i32, depth: f64) -> Option<[f64; 2]> {
        let flow = &dwell_world::flow::FLOW;
        if depth < flow.current_depth_m {
            return None;
        }
        let (hit, [u, v]) = self.river_heading(x, z)?;
        // Manning's on the river's fall (its own, or its course's mean round it where steeper: a
        // reach held level where the land dips runs as the river round it does, BUGS.md #42), or
        // its flow over the column's depth where that is faster; times `riverCurrentScale`.
        let fall = hit.fall.max(hit.mean_fall).max(0.0);
        let speed = flow.river_speed(fall, Self::unit_flow(&hit), depth);
        (speed >= flow.current_still_ms).then_some([u * speed, v * speed])
    }

    /// A channel's discharge across its width (m²/s).
    fn unit_flow(hit: &crate::worldmap::channels::Hit) -> f64 {
        hit.q.max(0.0) / (2.0 * hit.half).max(1.0)
    }

    /// The river's flow through face-local column (x, z) per metre of width (m²/s along x and z,
    /// docs/WATER_SURFACE_GRID.md §2b): its discharge spread across its channel's width, downstream
    /// as the column sees it (`river_current`'s direction). The water's grid carries it through
    /// the water as generated, so a dam backs the river up by what it carries, level reaches
    /// too. None off a river's channel.
    pub fn river_flow(&self, x: i32, z: i32) -> Option<[f64; 2]> {
        let (hit, [u, v]) = self.river_heading(x, z)?;
        let q = Self::unit_flow(&hit);
        (q > 0.0).then_some([u * q, v * q])
    }

    /// The channel face-local column (x, z) lies in and its downstream direction as the column
    /// sees it (a unit vector: the map's direction through the meander warp's inverse, found from
    /// its neighbours).
    fn river_heading(&self, x: i32, z: i32) -> Option<(crate::worldmap::channels::Hit, [f64; 2])> {
        let (x, z) = (i64::from(x), i64::from(z));
        let map = &self.map;
        if !map.channels.near(x as f64, z as f64, 0.0) {
            return None;
        }
        let warp = |x: i64, z: i64| {
            self.warp(
                self.meander_seeds,
                x,
                z,
                S.river_meander,
                S.river_meander_wavelength,
            )
        };
        let (wx, wz) = warp(x, z);
        let hit = map
            .channels
            .query(wx, wz, 0.0, f64::INFINITY)
            .filter(|h| h.d <= h.half)?;
        // The warp's Jacobian by central differences; the world's direction solves J v = dir.
        let (ex, ez) = (warp(x + 1, z), warp(x - 1, z));
        let (nx, nz) = (warp(x, z + 1), warp(x, z - 1));
        let (a, c) = ((ex.0 - ez.0) * 0.5, (ex.1 - ez.1) * 0.5);
        let (b, d) = ((nx.0 - nz.0) * 0.5, (nx.1 - nz.1) * 0.5);
        let det = a * d - b * c;
        let (u, v) = if det.abs() > 1e-6 {
            (
                (d * hit.dir.0 - b * hit.dir.1) / det,
                (-c * hit.dir.0 + a * hit.dir.1) / det,
            )
        } else {
            hit.dir
        };
        let len = u.hypot(v);
        if len < 1e-9 {
            return None;
        }
        Some((hit, [u / len, v / len]))
    }

    /// The river whose channel face-local column (x, z) lies in, by the world map
    /// (docs/WATER_FLOW.md §6): its index, how far along its course the column projects (m from
    /// its source) and its discharge there. The column's own lookups (`base_column`).
    fn river_point(&self, x: i64, z: i64) -> Option<(u32, f64, f64)> {
        let map = &self.map;
        if !map.channels.near(x as f64, z as f64, 0.0) {
            return None;
        }
        let (wx, wz) = self.warp(
            self.meander_seeds,
            x,
            z,
            S.river_meander,
            S.river_meander_wavelength,
        );
        let hit = map.channels.query(wx, wz, 0.0, f64::INFINITY)?;
        if hit.d > hit.half {
            return None;
        }
        let river = map.rivers.get(hit.river as usize)?;
        // Between the course's two vertices nearest (the segment it projects onto).
        let mut best = (f64::INFINITY, 0.0, 0.0);
        let mut along = 0.0;
        for w in river.path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (ax, az) = (f64::from(a.x), f64::from(a.z));
            let (dx, dz) = (f64::from(b.x) - ax, f64::from(b.z) - az);
            let len2 = dx * dx + dz * dz;
            let t = if len2 > 0.0 {
                (((wx - ax) * dx + (wz - az) * dz) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, pz) = (ax + dx * t, az + dz * t);
            let d2 = (wx - px).powi(2) + (wz - pz).powi(2);
            if d2 < best.0 {
                let q =
                    f64::from(a.discharge) + (f64::from(b.discharge) - f64::from(a.discharge)) * t;
                best = (d2, q, along + len2.sqrt() * t);
            }
            along += len2.sqrt();
        }
        if best.0.is_infinite() {
            return Some((
                hit.river,
                0.0,
                river.path.first().map_or(0.0, |v| f64::from(v.discharge)),
            ));
        }
        Some((hit.river, best.2, best.1))
    }

    /// [`Self::river_at`] for tests.
    #[cfg(test)]
    fn river_point_at(&self, x: i32, z: i32) -> Option<(u32, f64)> {
        self.river_at(x, z)
    }

    /// The river at face-local column (x, z) and how far along it (m from its source), by the
    /// world map: where a change to its flow there goes on from (docs/WATER_FLOW.md §6).
    pub fn river_at(&self, x: i32, z: i32) -> Option<(u32, f64)> {
        self.river_point(i64::from(x), i64::from(z))
            .map(|(r, s, _)| (r, s))
    }

    /// A river's course length (m).
    pub fn river_length(&self, river: u32) -> f64 {
        self.map.rivers.get(river as usize).map_or(0.0, |r| {
            r.path
                .windows(2)
                .map(|w| (f64::from(w[1].x - w[0].x)).hypot(f64::from(w[1].z - w[0].z)))
                .sum()
        })
    }

    /// Where a river goes at its end, by the world map: into another river (how far along it),
    /// into a lake (its area, its outflow and the rivers out of it), or into the sea.
    pub fn river_next(&self, river: u32) -> RiverNext {
        let map = &self.map;
        let Some(r) = map.rivers.get(river as usize) else {
            return RiverNext::End;
        };
        match r.end {
            crate::worldmap::RiverEnd::Sea => RiverNext::End,
            crate::worldmap::RiverEnd::Joins(j) => {
                let Some((last, other)) = r.path.last().zip(map.rivers.get(j as usize)) else {
                    return RiverNext::End;
                };
                let (px, pz) = (f64::from(last.x), f64::from(last.z));
                let mut best = (f64::INFINITY, 0.0);
                let mut along = 0.0;
                for w in other.path.windows(2) {
                    let (ax, az) = (f64::from(w[0].x), f64::from(w[0].z));
                    let (dx, dz) = (f64::from(w[1].x) - ax, f64::from(w[1].z) - az);
                    let len2 = dx * dx + dz * dz;
                    let t = if len2 > 0.0 {
                        (((px - ax) * dx + (pz - az) * dz) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d2 = (px - ax - dx * t).powi(2) + (pz - az - dz * t).powi(2);
                    if d2 < best.0 {
                        best = (d2, along + len2.sqrt() * t);
                    }
                    along += len2.sqrt();
                }
                RiverNext::River(j, best.1)
            }
            crate::worldmap::RiverEnd::Lake(l) => {
                let id = l + 1;
                let out: Vec<(u32, f64)> = map
                    .rivers
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.from_lake == id)
                    .map(|(i, o)| {
                        (
                            i as u32,
                            o.path.first().map_or(0.0, |v| f64::from(v.discharge)),
                        )
                    })
                    .collect();
                let cells = map.lake.iter().filter(|&&k| k == id).count();
                RiverNext::Lake {
                    lake: u32::from(l),
                    area: cells as f64 * map.cell * map.cell,
                    out,
                }
            }
        }
    }

    /// The ground at face-local column (x, z) with no water shaped into it: the map's eroded height
    /// and the fine detail, without the river cut, valley, levees or lake shore (`base_column`'s first
    /// two layers); none outside the world's disc. For the water surface grid's prototype (Phase 53
    /// G0: rivers without beds, ADR 0060); the chunk path doesn't use it.
    pub fn ground_without_water(&self, x: i32, z: i32) -> Option<f32> {
        let (x, z) = (i64::from(x), i64::from(z));
        if !inside_world_disc(x, z) {
            return None;
        }
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let mix = self.biomes.mix(x, z, base);
        let detail = self.blended_detail(&mix, x, z, base, 1);
        let h = base + detail;
        Some(((SEA_LEVEL_M + h).max(dwell_world::MIDPLANE_M + dwell_world::CORE_M)) as f32)
    }

    /// The ground at face-local column (x, z) with the rivers' valleys but no beds (no cut, lips or
    /// drawn river water; lake beds as generated): the valley floor runs through where the channel
    /// was, at its banks' height. For the water surface grid's prototype (Phase 53 G0, ADR 0060).
    pub fn ground_with_valleys(&self, x: i32, z: i32) -> Option<f32> {
        let c = self.base_column_with(i64::from(x), i64::from(z), 1, false);
        (!c.outside).then_some(c.height)
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
        self.state_in(&col, y, soil)
    }

    /// [`soil_rows`] for column (x, z), its neighbours' columns from `column`.
    fn soil_at(&self, x: i32, z: i32, col: &Column, column: impl Fn(i32, i32) -> Column) -> i64 {
        let top = top_voxel(col.height);
        let around: Vec<Column> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .map(|(dx, dz)| column(x + dx, z + dz))
            .filter(|c| !c.outside)
            .collect();
        let lowest = around
            .iter()
            .map(|c| top_voxel(c.height))
            .min()
            .unwrap_or(top);
        let rows = soil_rows(top, lowest);
        // A cliff face the volume made shows rock below its soil: the soil is at most the volume's
        // `soilDepth`, or the height field's own mantle there where that is deeper (so where the
        // volume reshapes nothing the soil is the height field's).
        match &self.volume {
            Some(v) => {
                let base_top = top_voxel(col.base);
                let base_lowest = around
                    .iter()
                    .map(|c| top_voxel(c.base))
                    .min()
                    .unwrap_or(base_top);
                rows.min(v.soil_rows.max(soil_rows(base_top, base_lowest)))
            }
            None => rows,
        }
    }

    fn state_in(&self, col: &Column, y: i32, soil: i64) -> StateId {
        if col.outside || !(FACE_MIN_Y..FACE_MAX_Y).contains(&y) {
            return M.air;
        }
        let top = top_voxel(col.height);
        let water = col.water_row();
        if y <= top {
            let m = fill_soil(self.palette(col), col.band, top, i64::from(top - y), soil);
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
    fn ground_of(&self, col: &Column) -> Option<Ground> {
        if col.outside {
            return None;
        }
        let top = top_voxel(col.height);
        // The top block as generated: under water it takes its flooded state. Rows of cells for
        // the top; metres for the height band (placements.json's) and the slope.
        let water = col.water_row();
        let block = under_water(
            fill(self.palette(col), col.band, top, 0),
            top,
            col.flood_row(),
        );
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
                self.ground_of(&cols[(j * w + i) as usize])
            } else {
                self.ground_of(&self.column_at(x, z))
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
                // The rows of water beside the column (its four neighbours' water over their
                // ground): a ramp piece in them holds the water in its open part, as one under the
                // column's own water does, so the water never ends in a wall at a dry bank's ramp
                // (owner, 2026-11-03; docs/BUGS.md 18).
                let beside: [(i32, i32); 4] = if pieces.is_empty() {
                    [(0, 0); 4]
                } else {
                    [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dz)| {
                        let (ix, iz) = (x + dx, z + dz);
                        let n = if (0..S).contains(&ix) && (0..S).contains(&iz) {
                            cols[(iz * S + ix) as usize]
                        } else {
                            self.column_at(x0 + ix, z0 + iz)
                        };
                        if n.outside || n.water == Water::None {
                            (0, 0)
                        } else {
                            (top_voxel(n.height) + 1, n.water_row())
                        }
                    })
                };
                let flood_at = |y: i32| {
                    if beside.iter().any(|&(lo, hi)| (lo..hi).contains(&y)) {
                        y + 1
                    } else {
                        flood
                    }
                };
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
                        *v = under_water(pieces[(y - first_piece) as usize], y, flood_at(y));
                    } else if y == top {
                        *v = under_water(fill(self.palette(col), col.band, top, 0), y, flood);
                    } else if y < top {
                        *v = fill_soil(self.palette(col), col.band, top, i64::from(top - y), soil);
                    } else if y < water {
                        *v = M.water;
                    } else if col.stream && y == top + 1 {
                        *v = running_water();
                    }
                }
                // A decoration in the air cell over the ground, if this chunk holds that cell
                // (none near a portal).
                let above = top + 1;
                if part == Part::None
                    && (y0..y0 + S).contains(&above)
                    && above >= water
                    && !col.stream
                    && (FACE_MIN_Y..FACE_MAX_Y).contains(&above)
                    && let Some(m) = self.decoration_at(&cols, x0, z0, x, z, top)
                {
                    voxels[local_index(x, above - y0, z)] = m;
                }
            }
        }
        if let Some(v) = &self.volume {
            self.carve_volume(v, coord, &cols, volume, chunk);
        }
        self.stamp_trees(coord, &cols, volume, chunk);
        self.stamp_monuments(coord, volume, chunk);
    }

    /// Carves a chunk filled to its columns' reshaped tops by the terrain volume: under each top the
    /// cells the density leaves empty become air, or water under the column's own water (a lake,
    /// river or the sea; never in a dry column). A floor left under rock is the column's top block
    /// where open sky lies within the volume's `floorGrass` of it (an overhang's mouth), else the
    /// volume's material, as is a ceiling (WORLD_GENERATION.md §5).
    fn carve_volume(
        &self,
        v: &Volume,
        coord: ChunkCoord,
        cols: &[Column],
        overlap: Overlap,
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
        // Carving reaches at most `reach` under the height field's top, which lies at most
        // `reach` under the reshaped one.
        if lo == i32::MAX || y0 + S <= lo - 2 * reach - 1 || y0 >= hi {
            return;
        }
        // The reshaped tops round the chunk out to the floors' reach of open sky.
        let r = v.floor_grass_cells;
        let w = S + 2 * r;
        let mut ext = Vec::with_capacity((w * w) as usize);
        for lz in -r..S + r {
            for lx in -r..S + r {
                ext.push(if (0..S).contains(&lx) && (0..S).contains(&lz) {
                    cols[(lz * S + lx) as usize]
                } else {
                    self.column_at(x0 + lx, z0 + lz)
                });
            }
        }
        let top_at = |lx: i32, lz: i32| {
            let c = ext[((lz + r) * w + lx + r) as usize];
            if c.outside {
                i32::MIN
            } else {
                top_voxel(c.height)
            }
        };
        let voxels = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside {
                    continue;
                }
                let (wx, wz) = (i64::from(x0 + x), i64::from(z0 + z));
                let Some(rows) = self.shaped_column(wx, wz, 1).rows else {
                    continue;
                };
                let base_top = rows.base_top;
                let top = top_voxel(col.height);
                // Nor is dry ground carved under the sea's surface.
                let floor = (base_top - reach).max(Self::sea_top() + 1).min(top);
                if y0 + S <= floor || y0 >= top {
                    continue;
                }
                let solid = |y: i32| rows.solid(y);
                let water = if col.water == Water::None {
                    i32::MIN
                } else {
                    col.water_row()
                };
                // Open sky reaches row y + 1 of a column within the floors' reach.
                let sky_near = |y: i32| {
                    (-r..=r).any(|dz| {
                        (-r..=r).any(|dx| dx * dx + dz * dz <= r * r && top_at(x + dx, z + dz) <= y)
                    })
                };
                for ly in 0..S {
                    let y = y0 + ly;
                    if y < floor
                        || y >= top
                        || !(FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                        || (overlap == Overlap::Partial
                            && !BUILD_VOLUME.contains_voxel(wx, i64::from(y), wz))
                    {
                        continue;
                    }
                    let cell = &mut voxels[local_index(x, ly, z)];
                    // A ramp piece the fill put under the top stays (a steep ramp's lower half
                    // over an overhang's mouth): only full cells take a floor or a ceiling.
                    let full = dwell_blocks::info(*cell).shape_index
                        == dwell_blocks::info(M.stone).shape_index;
                    if !solid(y) {
                        *cell = if y < water { M.water } else { M.air };
                    } else if full && !solid(y + 1) {
                        *cell = if sky_near(y) {
                            under_water(fill(self.palette(col), col.band, y, 0), y, col.flood_row())
                        } else {
                            v.material
                        };
                    } else if full && !solid(y - 1) {
                        *cell = v.material;
                    }
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
        if self.ramps.by.is_empty() || col.lip {
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
        let cells = self
            .ramps
            .cells(fill(self.palette(col), col.band, top, 0), shape, false);
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
        let here = &cols[(lz * S + lx) as usize];
        let ground = fill(self.palette(here), here.band, top, 0);
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
        self.cache_id = next_cache_id();
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
    pub fn top_state(&self, col: &Column) -> StateId {
        fill(self.palette(col), col.band, top_voxel(col.height), 0)
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
        let mut b = self.lod_bounds_with(&self.lod_columns(o, level), level);
        self.raise_for_monuments(&mut b, o, level);
        b
    }

    /// [`Self::lod_bounds_of`] for this generator at `level`: under a terrain volume a section below
    /// every top may still hold carved air (an overhang's underside), so it is buried only below
    /// the volume's reach under them.
    fn lod_bounds_with(&self, cols: &[Column], level: i32) -> LodBounds {
        let mut b = Self::lod_bounds_of(cols, self.lod_rise(level));
        if let Some(v) = &self.volume {
            b.lo -= f64::from(2 * v.reach_rows);
        }
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
        let mut bounds = self.lod_bounds_with(&cols, c.level);
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
                // The terrain volume's density at this column's sample point, where it reshapes:
                // the cells it leaves empty under the top are air, as in the chunks, so the
                // LOD draws an overhang's underside (WORLD_GENERATION.md §5).
                let carved = self.volume.as_deref().and_then(|_| {
                    let wx = o.x + i64::from(x) * cell + half;
                    let wz = o.z + i64::from(z) * cell + half;
                    self.shaped_column(wx, wz, cell).rows
                });
                // Whether the volume leaves the cell from row `a` empty (sampled at its middle row).
                let empty = |a: i64| {
                    carved
                        .as_ref()
                        .is_some_and(|r| !r.solid((a + cell / 2) as i32))
                };
                // The soil as the chunks mantle it: down to the lowest neighbouring cell's top.
                // Under a terrain volume, as `soil_at`: at most its `soilDepth` where it changed the
                // neighbours, so a cliff face reads as rock from afar too.
                let lod_soil = |x: i32, z: i32| {
                    let n = lod::SECTION_CELLS;
                    let around: Vec<Column> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .map(|(dx, dz)| ((x + dx).clamp(-1, n), (z + dz).clamp(-1, n)))
                        .map(|(nx, nz)| cols[lod::column_index(nx, nz)])
                        .filter(|c| !c.outside)
                        .collect();
                    let lowest = around
                        .iter()
                        .map(|c| top_voxel(c.height))
                        .min()
                        .unwrap_or(top as i32);
                    let rows = soil_rows(top as i32, lowest);
                    match &self.volume {
                        Some(v) => {
                            let base_top = top_voxel(col.base);
                            let base_lowest = around
                                .iter()
                                .map(|c| top_voxel(c.base))
                                .min()
                                .unwrap_or(base_top);
                            rows.min(v.soil_rows.max(soil_rows(base_top, base_lowest)))
                        }
                        None => rows,
                    }
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
                        if empty(a) {
                            continue;
                        }
                        // A ceiling over carved air is rock, as in the chunks.
                        *m = if empty(a - cell) {
                            M.stone
                        } else {
                            fill_soil(
                                self.palette(col),
                                col.band,
                                top as i32,
                                depth.min(1000),
                                lod_soil(x, z),
                            )
                        };
                        continue;
                    }
                    // The column's top: its surface seen from above. Water no cell sampled shows
                    // as the top.
                    let water_above = a + cell < water;
                    *m = if wet && !water_above {
                        M.water
                    } else {
                        fill(self.palette(col), col.band, top as i32, 0)
                    };
                    if let Some(s) = surface.as_deref_mut() {
                        s[lod::column_index(x, z)] = LodSurface {
                            valid: true,
                            wet,
                            height: rows(col.height).min((a + cell) as f32),
                            material: fill(self.palette(col), col.band, top as i32, 0),
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
                let Some(g) = self.ground_of(&col) else {
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
    /// When none of those holds one (a wide island, WORLD_SCALE.md §3.2), a bounded search
    /// (`spawn_wider`); never a mountain top.
    pub fn spawn_point(&self) -> [f64; 3] {
        let candidates = self.spawn_cells(true);
        for &(x, z) in candidates.iter().take(48) {
            if let Some(p) = self.spawn_near(x, z, 12, 0) {
                return p;
            }
        }
        self.spawn_wider(&candidates)
    }

    /// The search past the river-side cells nearest the centre, in a bounded number of probes:
    /// the origin's spiral to `SPAWN_ORIGIN_LEGS` legs, then up to `SPAWN_SPREAD` cells (river-side
    /// first, then any suitable dry cell) one per block of `SPAWN_BLOCK` map cells, nearest the
    /// centre first, each searched 12 legs round, with a level patch's rise 0, then 1 and 2 rows.
    /// Failing all, the nearest suitable cell's ground (low dry land, not a peak), else the
    /// origin's.
    fn spawn_wider(&self, river_side: &[(i32, i32)]) -> [f64; 3] {
        if let Some(p) = self.spawn_near(0, 0, SPAWN_ORIGIN_LEGS, 0) {
            return p;
        }
        let spread = self.spawn_spread(river_side, &self.spawn_cells(false));
        for rise in 0..=2 {
            for &(x, z) in &spread {
                if let Some(p) = self.spawn_near(x, z, 12, rise) {
                    return p;
                }
            }
        }
        let ground = |(x, z): (i32, i32)| {
            self.ground_y(x, z)
                .map(|g| [f64::from(x) + 0.5, m_of_row(g + 1), f64::from(z) + 0.5])
        };
        spread
            .first()
            .and_then(|&c| ground(c))
            .or_else(|| ground((0, 0)))
            .unwrap_or([0.5, m_of_row(SEA_LEVEL + 1), 0.5])
    }

    /// One cell per block of `SPAWN_BLOCK` map cells from `first` then `then` (each nearest the
    /// centre first), at most `SPAWN_SPREAD`: candidates spread over the island, not one valley.
    fn spawn_spread(&self, first: &[(i32, i32)], then: &[(i32, i32)]) -> Vec<(i32, i32)> {
        let block = f64::from(SPAWN_BLOCK) * self.map.cell;
        let mut seen = std::collections::HashSet::new();
        first
            .iter()
            .chain(then)
            .filter(|&&(x, z)| {
                seen.insert((
                    (f64::from(x) / block).floor() as i64,
                    (f64::from(z) / block).floor() as i64,
                ))
            })
            .take(SPAWN_SPREAD)
            .copied()
            .collect()
    }

    /// The map cells to look for a spawn in: dry land between the spawn heights, not in a lake,
    /// within `spawn.nearRiver` m of a river vertex when `river` is set; nearest the disc's centre
    /// first.
    fn spawn_cells(&self, river: bool) -> Vec<(i32, i32)> {
        let map = &*self.map;
        let n = map.n as i64;
        let reach = (S.spawn_near_river / map.cell).ceil() as i64;
        let mut near = vec![!river; map.n * map.n];
        for r in map.rivers.iter().filter(|_| river) {
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
            .map(|(_, c)| {
                let (x, z) = map.centre(c);
                (x as i32, z as i32)
            })
            .collect()
    }

    /// A spawn on a level dry patch (its tops within `rise` rows of its centre's) found by a
    /// square spiral of 8 m steps round (cx, cz), at most `steps` legs out.
    /// Natively a long spiral is probed in batches over the cores, the first patch in spiral
    /// order kept, so the answer is the one a probe at a time gives.
    fn spawn_near(&self, cx: i32, cz: i32, steps: i32, rise: i32) -> Option<[f64; 3]> {
        let (mut x, mut z, mut dx, mut dz) = (cx, cz, 1i32, 0i32);
        let (mut leg, mut walked, mut turns) = (1, 0, 0);
        let mut batch = Vec::with_capacity(SPAWN_BATCH);
        while leg <= steps {
            batch.push((x, z));
            if batch.len() == SPAWN_BATCH {
                if let Some(p) = self.first_patch(&batch, rise) {
                    return Some(p);
                }
                batch.clear();
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
        self.first_patch(&batch, rise)
    }

    /// The first of `at` (in order) holding a spawn patch: natively each core takes a share,
    /// the earliest found wins.
    fn first_patch(&self, at: &[(i32, i32)], rise: i32) -> Option<[f64; 3]> {
        let one = |at: &[(i32, i32)]| {
            at.iter()
                .enumerate()
                .find_map(|(k, &(x, z))| self.spawn_patch(x, z, rise).map(|p| (k, p)))
        };
        #[cfg(not(target_family = "wasm"))]
        {
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
            if cores > 1 && at.len() >= 256 {
                let share = at.len().div_ceil(cores);
                let found = std::thread::scope(|s| {
                    let jobs: Vec<_> = at
                        .chunks(share)
                        .enumerate()
                        .map(|(i, part)| {
                            s.spawn(move || one(part).map(|(k, p)| (i * share + k, p)))
                        })
                        .collect();
                    jobs.into_iter()
                        .filter_map(|j| j.join().expect("spawn search"))
                        .min_by_key(|&(k, _)| k)
                });
                return found.map(|(_, p)| p);
            }
        }
        one(at).map(|(_, p)| p)
    }

    /// The feet on column (x, z) when it is the centre of a level dry patch SPAWN_CLEAR columns
    /// either side (its tops within `rise` rows of the centre's), between the spawn heights, clear
    /// of structures and portals.
    fn spawn_patch(&self, x: i32, z: i32, rise: i32) -> Option<[f64; 3]> {
        // First on the ground before the cliffs reshape it (a column's volume pass is most of its
        // cost): a patch that is not a level dry one there is passed over without it.
        if self.volume.is_some() && !self.level_patch(x, z, rise, |x, z| self.base_column(x, z, 1))
        {
            return None;
        }
        if !self.level_patch(x, z, rise, |x, z| self.column(x, z, 1)) {
            return None;
        }
        self.spawn_clear(x, z)
    }

    /// Whether (x, z) is the centre of a level dry patch between the spawn heights, by `column`:
    /// every column's top within `rise` rows of the centre's (0: all one row).
    fn level_patch(&self, x: i32, z: i32, rise: i32, column: impl Fn(i64, i64) -> Column) -> bool {
        let col = column(i64::from(x), i64::from(z));
        if col.outside || col.wet() {
            return false;
        }
        let g = top_voxel(col.height);
        let above = m_of_row(g + 1) - SEA_LEVEL_M;
        if above < S.spawn_min_height.min(1.0) || above > S.spawn_max_height * 2.0 {
            return false;
        }
        (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|pz| {
            (-SPAWN_CLEAR..=SPAWN_CLEAR).all(|px| {
                let c = column(i64::from(x + px), i64::from(z + pz));
                !c.outside && !c.wet() && (top_voxel(c.height) - g).abs() <= rise
            })
        })
    }

    /// The feet on a level patch's centre (x, z) when the patch is clear of structures and portals.
    fn spawn_clear(&self, x: i32, z: i32) -> Option<[f64; 3]> {
        let g = top_voxel(self.column_at(x, z).height);
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

#[path = "river_rows.rs"]
mod river_rows;
pub use river_rows::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 53 G3b: an untouched river's current runs downstream as the column sees the river: a
    /// couple of metres along it, the river's course is further on, near the rivers of seed 0.
    #[test]
    fn a_rivers_current_runs_downstream_along_its_meanders() {
        // face-a-only: the generator's columns are face-local; face B is generated by the same
        // code through its mirror.
        let g = TerrainGenerator::new(0);
        let (mut down, mut checked) = (0, 0);
        let mut picks = Vec::new();
        for r in &g.map.rivers {
            for (i, v) in r.path.iter().enumerate() {
                if i % 25 == 12 && v.surface > 3.0 {
                    picks.push((v.x as i32, v.z as i32));
                }
            }
        }
        picks.truncate(120);
        for (px, pz) in picks {
            for dx in (-8..=8).step_by(4) {
                for dz in (-8..=8).step_by(4) {
                    let (x, z) = (px + dx, pz + dz);
                    let (Some(c), Some((river, along))) =
                        (g.river_current(x, z, 0.5), g.river_point_at(x, z))
                    else {
                        continue;
                    };
                    let speed = c[0].hypot(c[1]);
                    assert!(
                        speed > 0.0 && speed <= dwell_world::flow::FLOW.river_current_max_ms + 1e-9,
                        "{speed} m/s at ({x}, {z})"
                    );
                    let step = |k: f64| {
                        (
                            x + (c[0] / speed * k).round() as i32,
                            z + (c[1] / speed * k).round() as i32,
                        )
                    };
                    let (ax, az) = step(3.0);
                    let Some((r2, a2)) = g.river_point_at(ax, az) else {
                        continue;
                    };
                    if r2 != river {
                        continue;
                    }
                    checked += 1;
                    down += usize::from(a2 > along);
                }
            }
        }
        assert!(checked > 100, "{checked} columns checked");
        assert!(
            down * 10 >= checked * 9,
            "{down} of {checked} currents ran downstream"
        );
    }

    /// BUGS.md #38 (owner, 2026-11-06: "dead rivers ... the shallow ones missing currents"): a
    /// river runs through its level reaches too. Every channel column near seed 0's and 1's rivers,
    /// level (no fall) or not, has a current and a flow at a cell's depth, and seed 0's dam site
    /// (a level reach of a 0.6 m³/s creek) runs. Red before: a level reach was still.
    #[test]
    fn every_reach_of_a_river_runs_level_ones_too() {
        // face-a-only: the generator's columns are face-local; face B is generated by the same
        // code through its mirror.
        for seed in [0u64, 1] {
            let g = TerrainGenerator::new(seed);
            let (mut level, mut checked) = (0, 0);
            for r in &g.map.rivers {
                for (i, v) in r.path.iter().enumerate() {
                    if i % 5 != 2 || v.surface <= 3.0 {
                        continue;
                    }
                    let (x, z) = (v.x as i32, v.z as i32);
                    let Some((hit, _)) = g.river_heading(x, z) else {
                        continue;
                    };
                    checked += 1;
                    level += usize::from(hit.fall <= 0.0);
                    for depth in [0.5, 0.875] {
                        assert!(
                            g.river_current(x, z, depth).is_some(),
                            "seed {seed}: ({x}, {z}) at {depth} m (fall {}) is still",
                            hit.fall
                        );
                    }
                    assert!(
                        g.river_flow(x, z).is_some(),
                        "seed {seed}: ({x}, {z}) carries no flow"
                    );
                }
            }
            assert!(
                checked > 100 && level > 0,
                "seed {seed}: {checked} columns checked, {level} level"
            );
            eprintln!("seed {seed}: {checked} channel columns, {level} in level reaches");
        }
        let g = TerrainGenerator::new(0);
        let c = g
            .river_current(2364, 7448, 0.875)
            .expect("the dam site runs");
        assert!(c[1] > 0.05, "downstream (+z) at the dam site: {c:?}");
    }

    /// BUGS.md #42 (owner, 2026-11-06: "in this spot the river stops flowing too"). Seed
    /// 3974502447's creek below (12850, -3300): its course is held level for 167 m where the land
    /// dips (the map's levels never rise), so on its own fall it ran at its flow over its depth,
    /// 0.11 m/s, just below a reach at 1.5. On its course's mean fall round it, it runs as the
    /// river round it does. Red before: 0.11 m/s.
    #[test]
    fn the_owners_level_creek_runs_as_the_river_round_it() {
        // face-a-only: the owner's spot is on face A; `a_level_reach_runs_on_its_courses_fall`
        // checks both faces.
        let g = TerrainGenerator::new(3974502447);
        let mut checked = 0;
        for z in -3347..=-3316 {
            for x in 12840..=12870 {
                if g.column_at(x, z).water != Water::River {
                    continue;
                }
                let c = g.river_current(x, z, 1.0).expect("the creek runs");
                assert!(
                    c[0].hypot(c[1]) >= 0.5 && c[1] < 0.0,
                    "({x}, {z}) runs {c:?} (downstream is -z)"
                );
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked} creek columns");
    }

    dwell_world::both_faces!(
        /// BUGS.md #42: a river's reach held level where the land dips (fall 0) runs on its
        /// course's mean fall round it, not at its flow over its depth alone, on either face's
        /// map. Red before: every such reach of a creek at ~0.1 m/s.
        fn a_level_reach_runs_on_its_courses_fall(f: dwell_world::FaceFrame) {
            let t = crate::BifacialTerrain::height_field(0);
            let g = t.local(f.face);
            let channels = &g.map.channels;
            // Each segment's course falling over 256 m up and down from it (m per m).
            let mut round: HashMap<(i64, i64, i64, i64), f64> = HashMap::default();
            for c in &channels.courses {
                let falls = crate::worldmap::channels::mean_falls(c, 256.0);
                for (w, f) in c.windows(2).zip(falls) {
                    let k = |v: f64| (v * 1000.0).round() as i64;
                    round.insert((k(w[0].x), k(w[0].z), k(w[1].x), k(w[1].z)), f);
                }
            }
            let (mut level, mut slow) = (0, Vec::new());
            for r in &g.map.rivers {
                for (i, v) in r.path.iter().enumerate() {
                    if i % 3 != 1 || v.surface <= 3.0 {
                        continue;
                    }
                    let (x, z) = (v.x as i32, v.z as i32);
                    let Some((hit, _)) = g.river_heading(x, z) else {
                        continue;
                    };
                    // The segment the column found, by its ends.
                    let (wx, wz) = g.warp(
                        g.meander_seeds,
                        i64::from(x),
                        i64::from(z),
                        S.river_meander,
                        S.river_meander_wavelength,
                    );
                    let mut fall_round = 0.0;
                    channels.each_near(wx, wz, 0.0, f64::INFINITY, |id, h| {
                        if h == hit {
                            let s = &channels.segs[id as usize];
                            let k = |v: f64| (v * 1000.0).round() as i64;
                            fall_round = round
                                .get(&(k(s.a.x), k(s.a.z), k(s.b.x), k(s.b.z)))
                                .copied()
                                .unwrap_or(0.0);
                        }
                    });
                    if hit.fall > 0.0 || fall_round < 2e-4 {
                        continue;
                    }
                    level += 1;
                    let c = g.river_current(x, z, 1.0).expect("a level reach runs");
                    if c[0].hypot(c[1]) < 0.4 {
                        slow.push((x, z, c, hit.q));
                    }
                }
            }
            assert!(level >= 10, "{level} level reaches checked");
            assert!(
                slow.is_empty(),
                "{} of {level} level reaches crawl: {:?}",
                slow.len(),
                &slow[..slow.len().min(5)]
            );
        }
    );

    dwell_world::both_faces!(
        /// Owner, 2026-11-09: "double the river current speeds generally". On either face's map
        /// every sampled river column 1 m deep runs twice its speed by the law (Manning's on its
        /// fall, or its flow over its depth where faster), at most 3 m/s, and some reaches run
        /// past the old 1.5 m/s cap. Red before: the law's speed, at most 1.5.
        fn a_rivers_current_runs_twice_its_law(f: dwell_world::FaceFrame) {
            let t = crate::BifacialTerrain::height_field(0);
            let g = t.local(f.face);
            let (mut checked, mut past_old_cap) = (0, 0);
            for r in &g.map.rivers {
                for (i, v) in r.path.iter().enumerate() {
                    if i % 5 != 2 || v.surface <= 3.0 {
                        continue;
                    }
                    let (x, z) = (v.x as i32, v.z as i32);
                    let Some((hit, _)) = g.river_heading(x, z) else {
                        continue;
                    };
                    let fall = hit.fall.max(hit.mean_fall).max(0.0);
                    let law = (fall.sqrt() / 0.03).max(TerrainGenerator::unit_flow(&hit));
                    let want = (2.0 * law).min(3.0);
                    let got = g.river_current(x, z, 1.0).map_or(0.0, |c| c[0].hypot(c[1]));
                    if want < dwell_world::flow::FLOW.current_still_ms {
                        continue;
                    }
                    assert!(
                        (got - want).abs() < 1e-9,
                        "({x}, {z}) runs {got} m/s, the law {law}"
                    );
                    checked += 1;
                    past_old_cap += usize::from(got > 1.5 + 1e-9);
                }
            }
            assert!(
                checked >= 50 && past_old_cap > 0,
                "{checked} columns, {past_old_cap} past 1.5 m/s"
            );
        }
    );

    /// BUGS.md #29 (owner, 2026-11-05: "dirt walls extending into the river, usually at step
    /// downs"): the lips stood the channel's columns at a step `lipRise` rows over the water. Near
    /// rivers of seeds 0 and 1 no column inside a river's channel stands dry over its water but a
    /// chute's (a stream down a dry bed a row under its surface). Red before: the lips' walls.
    #[test]
    fn no_dry_wall_stands_in_a_rivers_channel() {
        // face-a-only: the generator's columns are face-local; face B is generated by the same
        // code through its mirror.
        let mut checked = 0;
        for seed in [0u64, 1] {
            let g = TerrainGenerator::new(seed);
            let mut picks = Vec::new();
            for r in &g.map.rivers {
                for (i, v) in r.path.iter().enumerate() {
                    if i % 25 == 12 && v.surface > 3.0 {
                        picks.push((v.x as i64, v.z as i64));
                    }
                }
            }
            picks.truncate(60);
            for (px, pz) in picks {
                for dx in -12..=12 {
                    for dz in -12..=12 {
                        let (x, z) = (px + dx, pz + dz);
                        // the river's channel here (as the column finds it), off the sea's level
                        let (wx, wz) = g.warp(
                            g.meander_seeds,
                            x,
                            z,
                            S.river_meander,
                            S.river_meander_wavelength,
                        );
                        let Some(r) = g.map.channels.query(wx, wz, 0.0, f64::INFINITY) else {
                            continue;
                        };
                        if r.d >= r.half || r.level <= CELL_H {
                            continue;
                        }
                        let row = surface_row((SEA_LEVEL_M + r.level) as f32);
                        if g.river_rows(r.river).steep.contains(&row) {
                            continue; // a chute's dry bed, a row under its surface
                        }
                        let col = g.base_column(x, z, 1);
                        if col.water == Water::Lake {
                            continue; // in a lake, the lake's surface
                        }
                        checked += 1;
                        assert!(
                            top_voxel(col.height) < row,
                            "seed {seed}: ({x}, {z}) stands at row {} over its river's water at row {row}",
                            top_voxel(col.height)
                        );
                    }
                }
            }
        }
        assert!(checked >= 1000, "channel columns checked: {checked}");
    }

    #[test]
    fn natural_tops_are_shaped_with_stored_ramps_only_on_half_height_cells() {
        // HALF_HEIGHT_GRID.md §4: on 1 m cells the generator writes what it always has (its
        // goldens are unchanged); on ½ m cells every natural surface material has its pieces, a
        // ½ m ramp in one cell and a 45° ramp in two.
        let palettes: Vec<Bands> = BIOMES
            .iter()
            .flat_map(|b| b.bands.iter().map(Bands::of))
            .collect();
        let pieces = RampPieces::resolve(&palettes);
        if CELL_H == 1.0 {
            assert!(pieces.by.is_empty());
            return;
        }
        for m in [M.grass, M.sand, M.dirt, M.stone, palettes[1].ground] {
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

/// Where a river goes at its end (`TerrainGenerator::river_next`).
#[derive(Clone, Debug, PartialEq)]
pub enum RiverNext {
    /// Into the sea, or off the map: it ends.
    End,
    /// Into another river, this far along it (m).
    River(u32, f64),
    /// Into a lake: its id, area (m²) and the rivers out of it with each one's discharge (m³/s).
    Lake {
        lake: u32,
        area: f64,
        out: Vec<(u32, f64)>,
    },
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    dwell_world::both_faces!(
        /// WORLD_SCALE.md §3.2: past the origin's spiral, the spawn search's spread cells hold a
        /// level dry patch, found in a bounded search (at most `SPAWN_SPREAD` cells, each 12 legs
        /// round), so a world whose spiral finds none still spawns on level low ground.
        fn spread_spawn_search_finds_level_ground(f: dwell_world::FaceFrame) {
            let t = crate::BifacialTerrain::new(0);
            let g = t.local(f.face);
            let spread = g.spawn_spread(&g.spawn_cells(true), &g.spawn_cells(false));
            assert!(!spread.is_empty() && spread.len() <= SPAWN_SPREAD);
            let p = spread
                .iter()
                .find_map(|&(x, z)| g.spawn_near(x, z, 12, 0))
                .expect("a level patch among the spread cells");
            let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
            assert!(g.level_patch(x, z, 0, |x, z| g.column(x, z, 1)), "{p:?}");
            let above = p[1] - SEA_LEVEL_M;
            assert!(
                (S.spawn_min_height.min(1.0)..=2.0 * S.spawn_max_height).contains(&above),
                "{p:?}: low ground, not a peak"
            );
        }
    );

    dwell_world::both_faces!(
        /// A patch's rise: 0 asks every top on one row, 1 or 2 within that many rows of the
        /// centre's; a patch level at 0 is level at any rise.
        fn a_patchs_rise_relaxes_its_level(f: dwell_world::FaceFrame) {
            let t = crate::BifacialTerrain::height_field(0);
            let g = t.local(f.face);
            let (mut level, mut relaxed) = (0, 0);
            for k in 0..4000 {
                let (x, z) = (-8000 + (k % 64) * 250, -8000 + (k / 64) * 250);
                let l = [0, 1, 2].map(|r| g.level_patch(x, z, r, |x, z| g.column(x, z, 1)));
                assert!(!l[0] || l[1], "({x}, {z})");
                assert!(!l[1] || l[2], "({x}, {z})");
                level += usize::from(l[0]);
                relaxed += usize::from(l[2]);
            }
            assert!(relaxed > level, "{level} level, {relaxed} within 2 rows");
        }
    );
}
