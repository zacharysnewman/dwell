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
use dwell_world::tree_tint::{Tint, TreeTint, tint_at};
use dwell_world::{
    BUILD_VOLUME, CELL_H, CHUNK_SIZE, CORE_LAYERS, Chunk, ChunkCoord, DiscOverlap, M, MIDPLANE_Y,
    Overlap, SEA_LEVEL, SEA_LEVEL_M, local_index, m_of_row, rows_for_m,
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
    /// Tectonics at realistic scale (`terrain.json` `tectonics`, systemic terrain T5): one plate
    /// boundary, its volcanic arc and a hotspot chain, added to the field; off by default.
    pub tectonics: crate::tectonics::Tectonics,
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
    /// The biome map's 0..1-ish temperature (the starter's and Chisel's tuning).
    pub temperature: WhereRange,
    pub wetness: WhereRange,
    pub elevation: WhereRange,
    pub coast: WhereRange,
    /// °C (R4, the presets' climate envelope): the climate's in a world with one
    /// (`tectonics.climate`), else the 0..1 temperature in °C by `vegetation.json` `climate`.
    pub celsius: WhereRange,
    /// Annual rain, mm (R4, Whittaker-style): read only in a world with a climate; fits
    /// everywhere in a world without one.
    pub rain: WhereRange,
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
    /// The rain shadow (`biomemap::rainfall`): where the prevailing wind blows from (degrees,
    /// face-local, from +x toward +z).
    pub wind_direction: f64,
    /// Metres of full rain that dry the air out.
    pub wind_reach: f64,
    /// The share of the air's moisture rained per km on flat land.
    pub base_rain: f64,
    /// More rain per 10 % grade the ground rises along the wind.
    pub uplift_rain: f64,
    /// Less rain per 10 % grade the ground falls along the wind (the lee).
    pub lee_dry: f64,
    /// The rainfall's share of the wetness (0: the river/lake wetness alone, the default).
    pub rain_share: f64,
    /// The wind sees the ground low-passed over about this many metres (0: the map's own), so the
    /// rain follows whole ranges, not each hillside.
    pub wind_smooth_m: f64,
    /// The pick's exponent on each weight (`biomemap::BiomeField::mix`; 1: the weights as they are).
    pub sharpen: f64,
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
    /// Its density for each of those placements (ADR 0074 B5): a multiplier on the placement's
    /// chance where the biome has weight; a placement not named here takes 1. A placement the
    /// biome does not list does not stand where it has weight.
    pub trees: &'static [(&'static str, f64)],
    /// Its spacing for some of those placements (0.52.65): the blocks kept clear between two of
    /// them anchored where it is the column's biome, in place of the placement's own `spacing`.
    pub spacing: &'static [(&'static str, i32)],
    /// Its ground items (ADR 0074 B5): kind (`shared/ground` `kinds`) → a multiplier on that kind's
    /// rules where the biome has weight. None: every kind 1 (the default biome); given, a kind it
    /// does not name lies nowhere it rules.
    pub ground: Option<&'static [(&'static str, f64)]>,
    /// The fluid block the biome's water is drawn as.
    pub water: &'static str,
    /// Its water's colour and clarity over the fluid's look (none: the fluid's own).
    pub water_look: Option<WaterLook>,
    /// The strata stack (`rocks.json`, by name) its ground is made of, in place of the rock
    /// province's (systemic terrain T2); none: the province's.
    pub rock_stack: Option<&'static str>,
    /// How much the climate rules its placements (`vegetation.json`'s kinds; systemic terrain T3):
    /// 0 none (the default biome), 1 fully.
    pub vegetation: f64,
    /// How far its scarps have retreated from the drainage (`rocks.json` `erosion`; systemic
    /// terrain T4): 0 none (the default biome), 1 fully: isolated mesas and buttes on a floor.
    pub erosion: f64,
    /// Its rills (T4, the badlands'): 0 none, 1 the erosion's `gullyDepth`.
    pub gullies: f64,
    /// Its arches (ADR 0074 B8, `arches.rs`): openings through narrow fins; none in the default.
    pub arches: Option<crate::arches::Arches>,
    /// Its own snow line from temperature (systemic terrain T3b); none: the world's (the
    /// tectonics' `snowBelowC`), if on.
    pub snow: Option<BiomeSnow>,
    /// Whether its ground shows its soil type's blocks (`vegetation.json` `soilTypes`' `ground`
    /// and `under`; T3b): false (the default biome) the bands' own.
    pub soil_blocks: bool,
    /// How much the climate rules its decorations and ground items (`vegetation.json`'s kinds
    /// with a `decoration` or `ground`; T3b): 0 none (the default biome), 1 fully.
    pub undergrowth: f64,
}

/// A biome's snow line (T3b; data: a biome's `snow`): where the biome map's temperature with its
/// lapse rate is below `below_c` °C and the ground no steeper than `max_slope` (rise per run) the
/// top is `block`; colder steeper ground is bare rock; with `ice`, gentle ground colder than its
/// °C is that block (a glacier).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeSnow {
    pub below_c: f64,
    pub max_slope: f64,
    pub block: &'static str,
    pub ice: Option<(&'static str, f64)>,
}

mod table {
    include!("terrain.gen.rs");
}
pub use table::{
    BIOME_MAP, BIOMES, FIXTURE_BIOMES, FIXTURE_WORLD_EDITS, FIXTURE_WORLDS, HEIGHT, PRESET_BIOMES,
    SURFACE,
};

/// Biome `b`'s density for the placement `name` (ADR 0074 B5): 0 when it does not list it, its
/// `trees` entry when given, else 1.
pub fn tree_density_of(b: &Biome, name: &str) -> f32 {
    if !b.placements.contains(&name) {
        return 0.0;
    }
    b.trees
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(1.0, |&(_, d)| d as f32)
}

/// Biome `b`'s density for the ground item kind `name` (ADR 0074 B5): 1 without a `ground`, else
/// its entry, 0 when it does not name the kind.
pub fn ground_density_of(b: &Biome, name: &str) -> f32 {
    b.ground.map_or(1.0, |g| {
        g.iter()
            .find(|(n, _)| *n == name)
            .map_or(0.0, |&(_, d)| d as f32)
    })
}

/// The biome named `name` (`shared/worldgen/biomes`), if the build has it.
pub fn find_biome(name: &str) -> Option<&'static Biome> {
    BIOMES.iter().find(|b| b.name == name)
}

const S: Surface = SURFACE;
/// Sand at the shore and under water to SAND_DEPTH (rows, from metres).
/// The arches' site draws' seed stream (ADR 0074 B8).
const ARCH_STREAM: u32 = 0xA2C4_0001;

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
    /// The rock's wall shows at its top (an erosion mesa's scarp, a hard bed's wall): its edge
    /// takes notched ramps where rocks.json `shaping.notched` is `mesaEdges`.
    pub wall: bool,
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
    /// The biome map's geography it was built with.
    params: BiomeMap,
    /// The geology (systemic terrain T2): the stacks' profiles and the provinces.
    pub geology: crate::geology::Resolved,
    /// Each biome's own stack (its `rockStack`), by index into the geology's stacks.
    biome_stack: Vec<Option<usize>>,
    /// The rocks' palettes from here (the first rock's).
    rock_first: u16,
    /// Rock r's scree palette (ground and under its scree, rock its face); its cap palette at
    /// `+ 1` (ground and under its cap). The rocks of the noise's stacks (weight above 0) from
    /// `rock_first` in order; a rock only in stacks of weight 0 (R5's plate provinces) after
    /// every other palette, so adding one leaves the others' palettes where they were.
    rock_palette: Vec<u16>,
    /// Per rock, the palette of its first `capBy` tread (the rest follow in order; BUGS.md #134).
    cap_by_palette: Vec<u16>,
    /// Per blend (rocks.json `blends`) and palette: the first of that palette's blended variants
    /// (one per tone, in order), `u16::MAX` where neither its ground nor its under is blended.
    blend_first: Vec<Vec<u16>>,
    /// Per stack, the blend whose `stacks` name it.
    stack_blend: Vec<Option<usize>>,
    /// The palette each blended variant was made from (variants from `blend_start` on), so a
    /// variant reads as its band's biome.
    blend_start: u16,
    blend_origin: Vec<u16>,
    /// The drainage seen from above (T4), built only where a biome erodes.
    drainage: Option<crate::geology::DrainageField>,
    /// Each stack's lowest soft layer's top (m within its period; T4's base level).
    soft_tops: Vec<f64>,
    /// The snow line (`terrain.json` `tectonics.snowBelowC`; `rocks.json` `tectonic.snow`);
    /// None when it is off.
    snow: Option<SnowLine>,
    /// Each biome's own snow line (its `snow`, T3b), in place of the world's where it is the
    /// column's biome.
    biome_snow: Vec<Option<SnowLine>>,
    /// Each biome's soil palettes (its `soilBlocks`, T3b): band k's with soil type t's blocks at
    /// `first + k × soil_types + t`; None for a biome without.
    soil_first: Vec<Option<u16>>,
    /// The soil types the soil palettes were built for (`vegetation.json` `soilTypes`).
    soil_types: usize,
    soil_list: &'static [crate::vegetation::SoilType],
    /// Which soil types change a block (the rest keep the band's palette).
    soil_has_blocks: Vec<bool>,
}

/// The snow line's covers by palette: the snow below `below_c` °C on ground no steeper than
/// `max_slope`, and there the glacier's ice (`tectonics.iceBelowC`) where colder than its °C.
#[derive(Clone, Copy, Debug)]
struct SnowLine {
    snow: u16,
    ice: Option<(u16, f64)>,
    below_c: f64,
    max_slope: f64,
}

impl BiomeSet {
    /// The set's configuration as text: everything but its whole-map grids (the biome field and
    /// the drainage), which are derived from the map and this (`TerrainGenerator::spawn_key`;
    /// BUGS.md #132: formatting the grids too took ~134 ms a face on the Realistic world).
    pub fn config_text(&self) -> String {
        format!(
            "{:?}",
            (
                &self.list,
                &self.palettes,
                &self.first,
                self.field.is_some(),
                &self.params,
                &self.geology,
                &self.biome_stack,
                (
                    self.rock_first,
                    &self.rock_palette,
                    &self.cap_by_palette,
                    (&self.blend_first, &self.stack_blend, self.blend_start)
                ),
                self.drainage.is_some(),
                &self.soft_tops,
                (&self.snow, &self.biome_snow),
                (
                    &self.soil_first,
                    self.soil_types,
                    self.soil_list,
                    &self.soil_has_blocks
                ),
            )
        )
    }

    /// The bytes each of its whole-map grids holds (BUGS.md #70; `tests/starter_memory.rs`).
    pub fn grid_bytes(&self) -> Vec<(&'static str, usize)> {
        let mut out = Vec::new();
        if let Some(f) = &self.field {
            out.extend(f.grid_bytes());
        }
        if let Some(d) = &self.drainage {
            out.extend(d.grid_bytes());
        }
        out
    }

    /// The biome whose band palette `p` is, its soil palettes too (None for the rocks' and the
    /// snow's palettes).
    pub fn biome_of_palette(&self, p: u16) -> Option<usize> {
        let p = if p >= self.blend_start {
            *self.blend_origin.get(usize::from(p - self.blend_start))?
        } else {
            p
        };
        if p < self.rock_first {
            return Some(self.first.partition_point(|&f| f <= p) - 1);
        }
        let n = self.soil_types as u16;
        self.soil_first.iter().enumerate().find_map(|(b, f)| {
            let f = (*f)?;
            let len = self.list[b].bands.len() as u16 * n;
            (f..f + len).contains(&p).then_some(b)
        })
    }

    /// `list` (the world's default first) over `map`.
    pub fn new(list: &[Biome], map: &WorldMap, world_seed: u64) -> Self {
        Self::with_map(list, map, world_seed, BIOME_MAP)
    }

    /// [`BiomeSet::new`] with the biome map's geography `params` (`terrain.json` `biomeMap`).
    pub fn with_map(list: &[Biome], map: &WorldMap, world_seed: u64, params: BiomeMap) -> Self {
        Self::with_geology(list, map, world_seed, params, crate::geology::GEOLOGY)
    }

    /// [`BiomeSet::with_map`] with `geology` (`rocks.json`) in place of the build's.
    pub fn with_geology(
        list: &[Biome],
        map: &WorldMap,
        world_seed: u64,
        params: BiomeMap,
        geology: crate::geology::Geology,
    ) -> Self {
        Self::with_soils(
            list,
            map,
            world_seed,
            params,
            geology,
            crate::vegetation::VEGETATION.soil_types,
        )
    }

    /// [`BiomeSet::with_geology`] with `soil_types` (`vegetation.json` `soilTypes`: the blocks a
    /// biome with `soilBlocks` shows) in place of the build's.
    pub fn with_soils(
        list: &[Biome],
        map: &WorldMap,
        world_seed: u64,
        params: BiomeMap,
        geology: crate::geology::Geology,
        soil_types: &'static [crate::vegetation::SoilType],
    ) -> Self {
        assert!(!list.is_empty(), "a world has at least one biome");
        BIOME_SETS_BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut palettes = Vec::new();
        let mut first = Vec::new();
        for b in list {
            first.push(palettes.len() as u16);
            palettes.extend(b.bands.iter().map(Bands::of));
        }
        let rock_first = palettes.len() as u16;
        let push_rock = |palettes: &mut Vec<Bands>, r: &crate::geology::Rock| {
            let (face, cap, scree) = (
                dwell_blocks::require(r.face),
                dwell_blocks::require(r.cap),
                dwell_blocks::require(r.scree),
            );
            palettes.push(Bands {
                ground: scree,
                under: scree,
                rock: face,
                shore: scree,
            });
            palettes.push(Bands {
                ground: cap,
                under: cap,
                rock: face,
                shore: scree,
            });
            palettes.len() as u16 - 2
        };
        // A rock only in stacks of weight 0 (R5) takes its palettes last (below).
        let late: Vec<bool> = geology
            .rocks
            .iter()
            .map(|r| {
                let in_stack = |w: bool| {
                    geology
                        .stacks
                        .iter()
                        .any(|s| (s.weight > 0.0) == w && s.layers.iter().any(|l| l.rock == r.name))
                };
                !in_stack(true) && in_stack(false)
            })
            .collect();
        let mut rock_palette = vec![0u16; geology.rocks.len()];
        for (i, r) in geology.rocks.iter().enumerate() {
            if !late[i] {
                rock_palette[i] = push_rock(&mut palettes, r);
            }
        }
        let biome_stack = list
            .iter()
            .map(|b| {
                b.rock_stack.map(|n| {
                    geology
                        .stack(n)
                        .unwrap_or_else(|| panic!("biome {}: no rock stack {n}", b.name))
                })
            })
            .collect();
        let field = (list.len() > 1)
            .then(|| crate::biomemap::BiomeField::with_params(map, world_seed, params));
        // The snow line (0.52.40): its cover's palette after the rocks', only when it is on.
        let snow = map.tectonics.snow_line().then(|| {
            let mut cover = |name: &str| {
                let b = dwell_blocks::require(name);
                palettes.push(Bands {
                    ground: b,
                    under: b,
                    rock: b,
                    shore: b,
                });
                palettes.len() as u16 - 1
            };
            let snow = cover(geology.snow_block);
            // The glacier (0.52.45): its cover's palette after the snow's, only when it is on.
            let ice = map
                .tectonics
                .glacier()
                .then(|| (cover(geology.ice_block), map.tectonics.ice_below_c));
            SnowLine {
                snow,
                ice,
                below_c: map.tectonics.snow_below_c,
                max_slope: map.tectonics.snow_max_slope,
            }
        });
        // T3b: each biome's own snow line, its covers' palettes after the world's, only for a
        // biome with one (none in the default world).
        let biome_snow = list
            .iter()
            .map(|b| {
                let s = b.snow?;
                let mut cover = |name: &str| {
                    let c = dwell_blocks::require(name);
                    palettes.push(Bands {
                        ground: c,
                        under: c,
                        rock: c,
                        shore: c,
                    });
                    palettes.len() as u16 - 1
                };
                let snow = cover(s.block);
                let ice = s.ice.map(|(name, c)| (cover(name), c));
                Some(SnowLine {
                    snow,
                    ice,
                    below_c: s.below_c,
                    max_slope: s.max_slope,
                })
            })
            .collect();
        // T3b: each `soilBlocks` biome's bands with each soil type's blocks, after those.
        let soil_first = list
            .iter()
            .map(|b| {
                if !b.soil_blocks {
                    return None;
                }
                let f = palettes.len() as u16;
                for band in b.bands {
                    let base = Bands::of(band);
                    for t in soil_types {
                        let or = |name: &str, own| {
                            if name.is_empty() {
                                own
                            } else {
                                dwell_blocks::require(name)
                            }
                        };
                        palettes.push(Bands {
                            ground: or(t.ground, base.ground),
                            under: or(t.under, base.under),
                            ..base
                        });
                    }
                }
                Some(f)
            })
            .collect();
        // The rocks only in stacks of weight 0 (R5), last.
        for (i, r) in geology.rocks.iter().enumerate() {
            if late[i] {
                rock_palette[i] = push_rock(&mut palettes, r);
            }
        }
        // The rocks' treads by the climate (`capBy`, BUGS.md #134), after all those.
        let cap_by_palette = geology
            .rocks
            .iter()
            .map(|r| {
                let first = palettes.len() as u16;
                for c in r.cap_by {
                    let cap = dwell_blocks::require(c.cap);
                    palettes.push(Bands {
                        ground: cap,
                        under: cap,
                        rock: dwell_blocks::require(r.face),
                        shore: dwell_blocks::require(r.scree),
                    });
                }
                first
            })
            .collect();
        // The blends' variants of every palette they reach (0.64), after all those.
        let blend_start = palettes.len() as u16;
        let mut blend_origin = Vec::new();
        let blend_first = geology
            .blends
            .iter()
            .map(|b| {
                let tones: Vec<StateId> =
                    b.blocks.iter().map(|n| dwell_blocks::require(n)).collect();
                let n = palettes.len();
                (0..n)
                    .map(|p| {
                        let base = palettes[p];
                        let (g, u) = (tones.contains(&base.ground), tones.contains(&base.under));
                        if !(g || u) {
                            return u16::MAX;
                        }
                        let f = palettes.len() as u16;
                        for &t in &tones {
                            blend_origin.push(p as u16);
                            palettes.push(Bands {
                                ground: if g { t } else { base.ground },
                                under: if u { t } else { base.under },
                                ..base
                            });
                        }
                        f
                    })
                    .collect()
            })
            .collect();
        let stack_blend = geology
            .stacks
            .iter()
            .map(|s| {
                geology
                    .blends
                    .iter()
                    .position(|b| b.stacks.contains(&s.name))
            })
            .collect();
        Self {
            list: list.to_vec(),
            palettes,
            first,
            blend_first,
            stack_blend,
            blend_start,
            blend_origin,
            field,
            params,
            geology: crate::geology::Resolved::new(geology, world_seed)
                .with_tectonics(map.tectonics),
            biome_stack,
            rock_first,
            rock_palette,
            cap_by_palette,
            drainage: (geology.erosion.strength > 0.0
                && list.iter().any(|b| b.erosion > 0.0 || b.gullies > 0.0))
            .then(|| crate::geology::DrainageField::of(map, geology.erosion.threshold)),
            soft_tops: geology
                .stacks
                .iter()
                .map(|s| crate::geology::soft_top(&geology, s))
                .collect(),
            snow,
            biome_snow,
            soil_first,
            soil_types: soil_types.len(),
            soil_list: soil_types,
            soil_has_blocks: soil_types.iter().map(|t| t.has_blocks()).collect(),
        }
    }

    /// Biome `biome`'s band `band` palette with soil type `soil`'s blocks (T3b): None where the
    /// biome has no `soilBlocks` or the soil type changes no block.
    pub fn soil_palette(&self, biome: usize, band: usize, soil: u8) -> Option<u16> {
        let f = self.soil_first.get(biome).copied().flatten()?;
        let t = usize::from(soil);
        (self.soil_has_blocks.get(t) == Some(&true))
            .then(|| f + (band * self.soil_types + t) as u16)
    }

    /// The biome map's temperature (0..1-ish, less the lapse rate) at (x, z) over `elevation` m:
    /// the map's with more than one biome, else the climate gradient's middle (0.5) less the lapse
    /// rate (so a one-biome world still has a snow line from temperature).
    pub fn temperature(&self, x: i64, z: i64, elevation: f64) -> f64 {
        match &self.field {
            Some(f) => f.geography(x, z, elevation).temperature,
            None => 0.5 - self.params.lapse_rate * elevation.max(0.0) / 1000.0,
        }
    }

    /// The snow line at (x, z) on ground `elevation` m high and `slope` steep (rise per run),
    /// given °C from the biome map's temperature: None when the line is off or the ground warmer
    /// than it; else Some(the cover's palette) on ground no steeper than its slope, Some(None) on
    /// a steeper face (bare rock: ridges poke through the snow).
    /// `biome` is the column's: its own line (`snow`, T3b) in place of the world's.
    pub fn snow_cover(
        &self,
        biome: usize,
        x: i64,
        z: i64,
        elevation: f64,
        slope: f64,
        celsius: impl Fn(f64) -> f64,
    ) -> Option<Option<u16>> {
        let line = self
            .biome_snow
            .get(biome)
            .copied()
            .flatten()
            .or(self.snow)?;
        let c = celsius(self.temperature(x, z, elevation));
        let palette = match line.ice {
            Some((ice, ice_c)) if c < ice_c => ice,
            _ => line.snow,
        };
        (c < line.below_c).then_some((slope <= line.max_slope).then_some(palette))
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
    /// Each planter's density per biome (ADR 0074 B5; `Biome::trees`), and whether any active one
    /// varies over the world (else trees ask nothing of the biomes: the default world).
    tree_density: Arc<(Vec<Vec<f32>>, bool)>,
    /// Each planter's spacing per biome (the biomes' `spacing`, else its own), and the most of
    /// them when some biome sets one (0.52.65; None: the placements' own, the default world).
    tree_spacing: Arc<Option<(Vec<Vec<i32>>, i32)>>,
    /// Soil and vegetation from climate (`vegetation.json`; systemic terrain T3): read only where a
    /// biome's `vegetation` is above 0.
    vegetation: crate::vegetation::Vegetation,
    /// This generator's biomes (the build's unless `with_biomes`) and their surface palettes.
    biomes: Arc<BiomeSet>,
    /// The portals between the faces (`portals.rs`), shared by both faces' generators; none for a
    /// face's generator on its own.
    portals: Option<Arc<Portals>>,
    /// This face's resurrection shrines (`shrines.rs`); none for a face's generator on its own.
    shrines: Option<Arc<crate::shrines::Shrines>>,
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
    /// The arc cones' column-level detail (`tectonics.cleaverDepth`, the rim's notches, crags,
    /// cirques, Rainier detail), placed once over the map; empty when off (the default world).
    peaks: Arc<Vec<crate::tectonics::PeakDetail>>,
    world_seed: u64,
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

/// How many biome sets this process has built (`BiomeSet::with_geology`; `shared_biomes`'s test).
static BIOME_SETS_BUILT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The biome sets this process has built so far (each holds whole-map grids: see `shared_biomes`).
pub fn biome_sets_built() -> usize {
    BIOME_SETS_BUILT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The biomes of `map` for face seed `world_seed`: its worldgen config's (biomes, biome map and
/// geology) when it carries one, else the build's. Built once per map and seed in a process and
/// shared: a process makes several generators of the same world (a job worker's chunk, LOD and
/// bounds generators, the server's, the spawn search's), and a many-biome world's set holds grids
/// of the whole map (the biome field and the drainage, ~60 MB a face), once per generator before
/// (BUGS.md: the starter world reloaded iOS Safari's tab while loading).
pub fn shared_biomes(map: &Arc<WorldMap>, world_seed: u64) -> Arc<BiomeSet> {
    type Entry = (std::sync::Weak<WorldMap>, u64, Arc<BiomeSet>);
    static SETS: std::sync::Mutex<Vec<Entry>> = std::sync::Mutex::new(Vec::new());
    let mut sets = SETS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A map no longer held drops its set (the weak keeps its address from being reused meanwhile).
    sets.retain(|(m, _, _)| m.strong_count() > 0);
    if let Some((_, _, set)) = sets
        .iter()
        .find(|(m, seed, _)| *seed == world_seed && std::ptr::eq(m.as_ptr(), Arc::as_ptr(map)))
    {
        return set.clone();
    }
    let set = Arc::new(match &map.worldgen {
        Some(w) => BiomeSet::with_soils(
            &w.edit.biomes,
            map,
            world_seed,
            w.edit.map,
            w.edit.geology,
            w.edit.vegetation.soil_types,
        ),
        None => BiomeSet::new(BIOMES, map, world_seed),
    });
    sets.push((Arc::downgrade(map), world_seed, set.clone()));
    set
}

impl TerrainGenerator {
    /// The face generator for face seed `world_seed`, its world map computed now or taken from
    /// the process's maps (`worldmap::for_seed`).
    pub fn new(world_seed: u64) -> Self {
        Self::with_map(world_seed, worldmap::for_seed(world_seed))
    }

    /// [`TerrainGenerator::new`] in a world of `size` (BIG_MAP.md M2): its map is the size's
    /// (`worldmap::for_seed_sized`), and so are its disc, build volume and base field.
    pub fn sized(world_seed: u64, size: dwell_protocol::world_size::WorldSize) -> Self {
        Self::with_map(world_seed, worldmap::for_seed_sized(world_seed, size))
    }

    /// The face generator over a given map (tests: a fixture map).
    pub fn with_map(world_seed: u64, map: Arc<WorldMap>) -> Self {
        let base = BaseField::sized(world_seed, map.size);
        let (offset_x, offset_z) = base.offset();
        // The map's biomes (a test world's map carries its worldgen config, ADR 0074 B7: Chisel's
        // edit, whole), shared by every generator of this map in the process (`shared_biomes`).
        let biomes = shared_biomes(&map, world_seed);
        let worldgen = map.worldgen.clone();
        let peaks = Arc::new(map.tectonics.peak_details(|x, z| map.height_at(x, z).0));
        let g = Self {
            base,
            peaks,
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
            tree_density: Arc::new((Vec::new(), false)),
            tree_spacing: Arc::new(None),
            vegetation: crate::vegetation::VEGETATION,
            biomes,
            portals: None,
            shrines: None,
            rise: DECORATION_RISE as f32,
            lod_valleys: None,
            volume: None,
            cache_id: next_cache_id(),
            world_seed,
        }
        .with_placements(&PLACEMENTS);
        match worldgen {
            Some(w) => g.with_vegetation(w.edit.vegetation),
            None => g,
        }
    }

    /// The world seed this generator was made from (a face's own seed, `bifacial::face_seed`).
    pub fn seed(&self) -> u64 {
        self.world_seed
    }

    /// This face's world map.
    pub fn map(&self) -> &Arc<WorldMap> {
        &self.map
    }

    /// The world's disc and build volume: its map's size's (BIG_MAP.md M2).
    pub fn world(&self) -> dwell_world::WorldBounds {
        dwell_world::WorldBounds::of(self.map.size)
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
    pub fn with_biomes_on(self, biomes: &[Biome], params: BiomeMap) -> Self {
        let geology = self.biomes.geology.geology;
        self.with_biomes_and_geology(biomes, params, geology)
    }

    /// [`TerrainGenerator::with_biomes_on`] with `geology` (`rocks.json`) in place of the build's.
    pub fn with_biomes_and_geology(
        mut self,
        biomes: &[Biome],
        params: BiomeMap,
        geology: crate::geology::Geology,
    ) -> Self {
        let set = BiomeSet::with_geology(biomes, &self.map, self.world_seed, params, geology);
        self.ramps = Arc::new(RampPieces::resolve(&set.palettes));
        self.biomes = Arc::new(set);
        self.cache_id = next_cache_id();
        self.refresh_trees()
    }

    /// The generator with `geology` (`rocks.json`: the rocks, the stacks, the provinces and the
    /// shaping; systemic terrain T2) in place of the build's.
    pub fn with_geology(mut self, geology: crate::geology::Geology) -> Self {
        let b = &self.biomes;
        let set = BiomeSet::with_geology(&b.list, &self.map, self.world_seed, b.params, geology);
        self.ramps = Arc::new(RampPieces::resolve(&set.palettes));
        self.biomes = Arc::new(set);
        self.cache_id = next_cache_id();
        self
    }

    /// The ground at `h` m over the sea shaped by its rock (systemic terrain T2): each biome's
    /// stack (its own, or the province's, fading out at a province's border) mapped through its
    /// profile, blended by the biomes' weights, `strength` of the way (none at 0, the default
    /// world's) and none under the sea or at the water (`fade`). With it, the chosen biome's rock
    /// and part where the shaping is at least half strength (its blocks show there).
    fn rock_shaping(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        h: f64,
        fade: f64,
    ) -> (f64, Option<(usize, crate::geology::Part)>) {
        let geo = &self.biomes.geology;
        let strength = geo.geology.strength;
        if strength <= 0.0 || h <= 0.0 {
            return (h, None);
        }
        let reach = strength * smooth01(h / 8.0) * fade;
        let mut province = None;
        let mut dh = 0.0;
        let mut chosen = None;
        for (i, w) in mix.iter() {
            let (stack, f) = match self.biomes.biome_stack[i] {
                Some(k) => (k, 1.0),
                None => *province.get_or_insert_with(|| geo.province(x, z)),
            };
            let (v, rock, part) = geo.profiles[stack].map(h);
            dh += w * f * (v - h);
            if i == usize::from(mix.chosen) && reach * f >= 0.5 {
                chosen = Some((rock, part));
            }
        }
        (h + reach * dh, chosen)
    }

    /// The cut-back at face-local (x, z) (systemic terrain T4): the biomes' erosion strength there
    /// and the margin (m) past the retreated scarp (below 0: cut down to the floor), with the
    /// floor's height and the stack. None where nothing erodes.
    pub fn cut_back(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        h: f64,
    ) -> Option<(f64, f64, f64, usize)> {
        let field = self.biomes.drainage.as_ref()?;
        let geo = &self.biomes.geology;
        let e = &geo.geology.erosion;
        let s: f64 = mix
            .iter()
            .map(|(i, w)| w * self.biomes.list[i].erosion)
            .sum::<f64>()
            * e.strength;
        if s <= 0.0 {
            return None;
        }
        let stack = match self.biomes.biome_stack[usize::from(mix.chosen)] {
            Some(k) => k,
            None => geo.province(x, z).0,
        };
        let st = &geo.geology.stacks[stack];
        let cap = geo
            .geology
            .rock(st.layers[0].rock)
            .map_or(0.5, |r| geo.geology.rocks[r].hardness);
        let (dist, line, acc) = field.at(&self.map, x as f64, z as f64);
        let period = geo.profiles[stack].period().max(1.0);
        let top = self.biomes.soft_tops[stack];
        let snapped = ((line - top) / period).floor() * period + top;
        let floor = line + e.snap * (snapped.min(line) - line);
        let hill = (h - floor).max(0.0);
        let n = crate::noise::fbm2(
            crate::noise::seed_word(self.world_seed, 0x6E01_0004),
            x,
            z,
            e.outline_wavelength.max(1.0) as i32,
            3,
            None,
        );
        let retreat = e.reach
            * (1.0 - e.resist * cap).max(0.0)
            * (1.0 + e.valley * (acc / e.threshold.max(1.0)).max(1.0).log2())
            * (1.0 + e.outline * 2.0 * f64::from(n)).max(0.0)
            - e.relief * hill;
        let mut margin = dist - retreat;
        // The fin rule (0.52.66, the chosen biome's `arches.fins`): where the fronts from lines on
        // either side meet, a fin stands on the crest between them.
        if let Some(f) = self.biomes.list[usize::from(mix.chosen)]
            .arches
            .and_then(|a| a.fins)
            && let Some((d1, d2)) = field.fronts(x as f64, z as f64)
            && let Some(m) = f.margin(d1, d2, retreat)
        {
            margin = margin.max(m);
        }
        Some((s.min(1.0), margin, floor, stack))
    }

    /// The ground at `h` m eroded from the plan view (systemic terrain T4): cut down to the floor
    /// nearer the drainage than the retreated scarp, a wall and a talus skirt at the edge, the
    /// badlands' rills by the biomes' `gullies`, all `strength` of the way and faded at the water.
    /// Its walls show the cap rock's face, its talus the soft rock's scree.
    fn erosion_shaping(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        h: f64,
        fade: f64,
        rock: Option<(usize, crate::geology::Part)>,
    ) -> (f64, Option<(usize, crate::geology::Part)>) {
        if h <= 0.0 {
            return (h, rock);
        }
        let Some((s, margin, floor, stack)) = self.cut_back(mix, x, z, h) else {
            return (h, rock);
        };
        let geo = &self.biomes.geology;
        let e = &geo.geology.erosion;
        // Relief on the mesas (rocks.json `mesaRelief`): the standing ground undulates, tilts and
        // carries knobs, the floor slopes to its drainage line with mounds, fans and washes; the
        // wall and the talus between them follow both, so the edge stays a wall.
        let (top, low) = self.mesa_relief(x, z, margin);
        let (h_top, floor) = (h + top, (floor + low).min(h + top));
        let (mut g, cut) = crate::geology::eroded(e, h_top, floor, margin);
        let gullies: f64 = mix
            .iter()
            .map(|(i, w)| w * self.biomes.list[i].gullies)
            .sum();
        if gullies > 0.0 && e.gully_depth > 0.0 {
            let v = crate::noise::fbm2(
                crate::noise::seed_word(self.world_seed, 0x6E01_0005),
                x,
                z,
                e.gully_wavelength.max(1.0) as i32,
                3,
                None,
            );
            let rill = 1.0 - smooth01(f64::from(v).abs() / 0.12);
            g = (g - gullies.min(1.0) * e.gully_depth * rill).max(floor.min(g) - 1.0);
        }
        let reach = s * fade * smooth01(h / 8.0);
        let out = h + reach * (g - h);
        if reach < 0.5 {
            return (out, rock);
        }
        use crate::geology::{Cut, Part};
        let st = &geo.geology.stacks[stack];
        let cap = geo.geology.rock(st.layers[0].rock);
        let soft = st.layers.iter().rev().find_map(|l| {
            geo.geology
                .rock(l.rock)
                .filter(|&r| geo.geology.rocks[r].hardness < 0.5)
        });
        // A biome with its own stack (`rockStack`) shows the layer at the ground's height on its
        // walls and its standing tops, whatever the shaping's strength (0.52.66: the mesa's beds
        // at every height, not the band's stone over the cut); else the cap rock's face.
        let own = self.biomes.biome_stack[usize::from(mix.chosen)]
            .map(|k| geo.profiles[k].map(out.max(0.0)).1);
        let rock = match cut {
            Cut::Wall => own.or(cap).map(|r| (r, Part::Wall)),
            Cut::Talus => soft.or(cap).map(|r| (r, Part::Talus)),
            Cut::Floor => soft.or(cap).map(|r| (r, Part::Tread)),
            Cut::Standing => rock.or(own.map(|r| (r, Part::Tread))),
        };
        (out, rock)
    }

    /// `palette` with its ground and under blocks taken by the blend of the column's stack (its
    /// biome's `rockStack`, else the province's), at the tone the blend picks there (`slope` m per
    /// m, at most 1, adds to it); `palette` unchanged where no blend holds.
    fn blended(&self, mix: &crate::biomemap::Mix, x: i64, z: i64, palette: u16, slope: f64) -> u16 {
        let set = &self.biomes;
        if set.blend_first.is_empty() {
            return palette;
        }
        let stack = match set.biome_stack[usize::from(mix.chosen)] {
            Some(k) => k,
            None => set.geology.province(x, z).0,
        };
        let Some(b) = set.stack_blend[stack] else {
            return palette;
        };
        let Some(&first) = set.blend_first[b].get(usize::from(palette)) else {
            return palette;
        };
        if first == u16::MAX {
            return palette;
        }
        let blend = &set.geology.geology.blends[b];
        first + blend.tone(self.world_seed as u32, x, z, slope.min(1.0)) as u16
    }

    /// The tone index (into its blend's blocks) the column at face-local (x, z) shows, if its
    /// ground is blended (rocks.json `blends`): for tests and the map's views.
    pub fn blend_tone_at(&self, x: i32, z: i32) -> Option<(usize, &'static str)> {
        let col = self.column_at(x, z);
        let set = &self.biomes;
        for (b, firsts) in set.blend_first.iter().enumerate() {
            for (p, &f) in firsts.iter().enumerate() {
                let n = set.geology.geology.blends[b].blocks.len() as u16;
                if f != u16::MAX && (f..f + n).contains(&col.palette) {
                    let _ = p;
                    let k = usize::from(col.palette - f);
                    return Some((k, set.geology.geology.blends[b].blocks[k]));
                }
            }
        }
        None
    }

    /// The mesa relief at face-local (x, z) whose cut-back margin is `margin` (rocks.json
    /// `mesaRelief`): (m added to the standing ground, m added to the floor).
    fn mesa_relief(&self, x: i64, z: i64, margin: f64) -> (f64, f64) {
        use crate::noise::{fbm2, mix32, seed_word};
        let r = &self.biomes.geology.geology.mesa_relief;
        let w = self.world_seed;
        let noise = |stream: u32, wavelength: f64, octaves: u32| {
            f64::from(fbm2(
                seed_word(w, 0x6E01_0100 + stream),
                x,
                z,
                wavelength.max(1.0) as i32,
                octaves,
                None,
            ))
        };
        let mut top = 0.0;
        if r.top_amplitude > 0.0 {
            top += r.top_amplitude * 2.0 * noise(0, r.top_wavelength, 3);
        }
        if r.dip > 0.0 {
            // A tilt of about `dip` m per m: a long noise whose slope is near the dip.
            top += r.dip * r.dip_wavelength / std::f64::consts::PI
                * 2.0
                * noise(1, r.dip_wavelength, 1);
        }
        if r.knob_height > 0.0 && r.knob_chance > 0.0 {
            let cell = r.knob_spacing.max(1.0);
            let (cx, cz) = ((x as f64 / cell).floor(), (z as f64 / cell).floor());
            let mut best = 0.0f64;
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let (i, k) = (cx as i64 + dx, cz as i64 + dz);
                    let hsh = mix32(seed_word(w, 0x6E01_0110) ^ mix32(i as u32 ^ mix32(k as u32)));
                    let u = |s: u32| f64::from(mix32(hsh ^ s)) / 4_294_967_296.0;
                    if u(1) >= r.knob_chance {
                        continue;
                    }
                    let (px, pz) = ((i as f64 + u(2)) * cell, (k as f64 + u(3)) * cell);
                    let d = (x as f64 - px).hypot(z as f64 - pz) / r.knob_radius.max(1.0);
                    if d < 1.0 {
                        // A butte: steep sides, a flat-ish top.
                        let t = (1.0 - d) / 0.35;
                        best = best.max(r.knob_height * (0.5 + 0.5 * u(4)) * t.min(1.0).powf(0.7));
                    }
                }
            }
            top += best;
        }
        // The floor: rising from its drainage line, low mounds, talus fans, washes.
        let mut low = 0.0;
        if let Some(field) = self.biomes.drainage.as_ref() {
            let (dist, _, _) = field.at(&self.map, x as f64, z as f64);
            low += (r.floor_slope * dist).min(r.floor_rise);
        }
        if r.mound_height > 0.0 {
            low += r.mound_height * (2.0 * noise(2, r.mound_wavelength, 2)).max(0.0);
        }
        if r.fan_height > 0.0 && r.fan_width > 0.0 {
            let e = &self.biomes.geology.geology.erosion;
            let beyond = -margin - e.talus_width;
            let t = (1.0 - beyond / r.fan_width).clamp(0.0, 1.0);
            let lobes =
                0.5 + 0.5 * (2.0 * noise(3, (r.fan_width * 0.8).max(16.0), 1)).clamp(-1.0, 1.0);
            low += r.fan_height * t * t * lobes;
        }
        if r.wash_depth > 0.0 {
            let v = noise(4, r.wash_wavelength, 2).abs();
            low -= r.wash_depth * (1.0 - smooth01(v / 0.06));
        }
        (top, low)
    }

    /// The palette rock `r`'s tread shows at face-local (x, z) over `base` m: its first `capBy`
    /// entry whose climate holds there (BUGS.md #134), else its `cap`'s. A world with one biome has
    /// no geography: the `cap`.
    fn tread_palette(&self, r: usize, x: i64, z: i64, base: f64) -> u16 {
        let set = &self.biomes;
        let cap = set.rock_palette[r] + 1;
        let rules = set.geology.geology.rocks[r].cap_by;
        if rules.is_empty() {
            return cap;
        }
        let Some(geo) = set.geography(x, z, base) else {
            return cap;
        };
        let arid = &set.geology.geology.arid;
        rules
            .iter()
            .position(|c| c.climate.holds(arid, geo.celsius, geo.wetness, geo.rain))
            .map_or(cap, |k| set.cap_by_palette[r] + k as u16)
    }

    /// Whether ground at face-local (x, z) is arid by the geology's `arid` (rocks.json; BUGS.md
    /// #134): None in a world with one biome (no geography).
    pub fn arid_at(&self, x: i64, z: i64) -> Option<bool> {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let geo = self.biomes.geography(x, z, base)?;
        Some(
            self.biomes
                .geology
                .geology
                .arid
                .holds(geo.celsius, geo.wetness, geo.rain),
        )
    }

    /// Whether column `col`'s top shows a rock's blocks (the stack's wall, tread or talus: the
    /// rock shaping's or the erosion's), not its biome band's.
    pub fn is_rock_palette(&self, col: &Column) -> bool {
        self.biomes.biome_of_palette(col.palette).is_none()
    }

    /// The soil type `rock` weathers to at face-local (x, z) over `base` m: by the climate there
    /// where the rock's soils name one (R5), else the rock's own.
    fn soil_type_of(&self, rock: &str, x: i64, z: i64, base: f64) -> u8 {
        let v = &self.vegetation;
        if v.climate_dependent(rock)
            && let Some(geo) = self.biomes.geography(x, z, base)
        {
            return v
                .rock_soil(rock, v.celsius(geo.temperature), geo.wetness)
                .soil_type;
        }
        v.rock_soil(rock, f64::NAN, f64::NAN).soil_type
    }

    /// This generator's geology (`rocks.json`).
    pub fn geology(&self) -> &crate::geology::Geology {
        &self.biomes.geology.geology
    }

    /// The face's plate tectonics, in a world whose tectonics come from plates (R5; the big map's
    /// layers, `map_layers.rs`).
    pub fn plate_tectonics(&self) -> Option<&crate::plate_tectonics::PlateTectonics> {
        self.biomes.geology.plate_tectonics().map(|t| &**t)
    }

    /// The rock province's stack at face-local (x, z): the noise's, or the tectonics' (T5: a
    /// volcano's, the old core's).
    pub fn province_at(&self, x: i64, z: i64) -> &'static str {
        let geo = &self.biomes.geology;
        geo.geology.stacks[geo.province(x, z).0].name
    }

    /// The erosion's strength, cut-back margin (m past the retreated scarp; below 0 cut down to
    /// the floor) and floor (m over the sea) at face-local (x, z), by the height field's ground
    /// (Chisel's erosion overlay; systemic terrain T4). None where nothing erodes.
    pub fn erosion_at(&self, x: i64, z: i64) -> Option<(f64, f64, f64)> {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let mix = self.biomes.mix(x, z, base);
        self.cut_back(&mix, x, z, base)
            .map(|(s, margin, floor, _)| (s, margin, floor))
    }

    /// The rock at the surface of face-local (x, z) and the part of its stack's profile there,
    /// by the height field's ground (whatever the shaping's strength; Chisel's rock overlay).
    pub fn rock_at_surface(&self, x: i64, z: i64) -> (crate::geology::Rock, crate::geology::Part) {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let mix = self.biomes.mix(x, z, base);
        self.surface_rock_part(&mix, x, z, base)
    }

    /// [`TerrainGenerator::rock_at_surface`]'s rock, given the column's biome `mix` over `base` m.
    fn surface_rock(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        base: f64,
    ) -> crate::geology::Rock {
        self.surface_rock_part(mix, x, z, base).0
    }

    fn surface_rock_part(
        &self,
        mix: &crate::biomemap::Mix,
        x: i64,
        z: i64,
        base: f64,
    ) -> (crate::geology::Rock, crate::geology::Part) {
        let geo = &self.biomes.geology;
        let stack = match self.biomes.biome_stack[usize::from(mix.chosen)] {
            Some(k) => k,
            None => geo.province(x, z).0,
        };
        let (_, rock, part) = geo.profiles[stack].map(base.max(0.0));
        (geo.geology.rocks[rock], part)
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

    /// The rain shadow's rainfall at face-local (x, z), 0..1 (None with one biome: no map).
    pub fn biome_rainfall(&self, x: i64, z: i64) -> Option<f64> {
        self.biomes
            .field
            .as_ref()
            .map(|f| f.rainfall(x as f64, z as f64))
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
        self.refresh_trees()
    }

    /// The planters' densities from the biomes' `trees` (ADR 0074 B5) and the rise they need: a
    /// placement no biome lists stands nowhere.
    fn refresh_trees(mut self) -> Self {
        // With one biome (no biome map) only the first biome's weight counts.
        let biomes = if self.biomes.field.is_none() {
            &self.biomes.list[..1.min(self.biomes.list.len())]
        } else {
            &self.biomes.list[..]
        };
        let per: Vec<Vec<f32>> = self
            .planters
            .iter()
            .map(|p| {
                let name = p.rule().name;
                biomes.iter().map(|b| tree_density_of(b, name)).collect()
            })
            .collect();
        let mut varies = false;
        for (p, d) in self.planters.iter_mut().zip(&per) {
            let most = d.iter().copied().fold(0.0f32, f32::max);
            p.set_boost(most);
            varies |= most > 0.0 && d.iter().any(|&v| v != 1.0);
        }
        self.tree_density = Arc::new((per, varies));
        // The biomes' spacing, only when one sets any.
        let spacing: Vec<Vec<i32>> = self
            .planters
            .iter()
            .map(|p| {
                let rule = p.rule();
                biomes
                    .iter()
                    .map(|b| {
                        b.spacing
                            .iter()
                            .find(|(n, _)| *n == rule.name)
                            .map_or(rule.spacing, |&(_, s)| s)
                    })
                    .collect()
            })
            .collect();
        let set = biomes.iter().any(|b| !b.spacing.is_empty());
        let most = spacing.iter().flatten().copied().max().unwrap_or(0);
        self.tree_spacing = Arc::new(set.then_some((spacing, most)));
        self.cache_id = next_cache_id();
        // A structure anchored beside the chunk on ground up to its reach higher still reaches in
        // (rows: a structure's rise counts cells; ground climbs up to 1 m per column of reach).
        let on = || self.planters.iter().filter(|p| p.active());
        let reach = on().map(Planter::reach).max().unwrap_or(0);
        let rise = if on().next().is_none() {
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
        let mut v = Volume::resolve(def, self.world_seed);
        // A test world's `cliffs` (0.52.64): its strength, 0 none.
        if let Some(c) = self.map.worldgen.as_ref().and_then(|w| w.edit.cliffs) {
            if c <= 0.0 {
                return self;
            }
            v.strength = c;
        }
        self.rise = self.rise.max((v.reach_rows + 1) as f32);
        self.volume = Some(Arc::new(v));
        self.cache_id = next_cache_id();
        self
    }

    /// The generator with this face's resurrection shrines built on it (`BifacialTerrain` gives
    /// each face its own, sited clear of the portals).
    pub fn with_shrines(mut self, shrines: Arc<crate::shrines::Shrines>) -> Self {
        self.shrines = Some(shrines);
        self.cache_id = next_cache_id();
        self
    }

    /// This face's shrines, if this generator has them.
    pub fn shrines(&self) -> Option<&Arc<crate::shrines::Shrines>> {
        self.shrines.as_ref()
    }

    /// A shrine's structure on this face: its anchor on the site, its bottom row on the lowest
    /// ground under its bottom layer (as a portal's monument stands).
    pub fn shrine(&self, shrines: &crate::shrines::Shrines, s: crate::shrines::Shrine) -> Placed {
        let base = shrines
            .feet(s)
            .map(|(x, z)| top_voxel(self.column_at(x, z).height))
            .min()
            .unwrap_or(0);
        shrines.placed_at(s, base + 1)
    }

    /// Where a player stands up at shrine `s` (face-local feet, m): its `stand` column's centre,
    /// on the floor built there.
    pub fn shrine_stand(
        &self,
        shrines: &crate::shrines::Shrines,
        s: crate::shrines::Shrine,
    ) -> [f64; 3] {
        let placed = self.shrine(shrines, s);
        let (x, z) = shrines.stand_column(s);
        let floor = placed
            .cells()
            .filter(|(p, _)| p[0] == x && p[2] == z)
            .map(|(p, _)| p[1])
            .max()
            .unwrap_or(placed.min[1] - 1);
        // The floor stands on the ground; where the ground under the stand column is higher than
        // the structure's base, a player stands on that ground.
        let ground = top_voxel(self.column_at(x, z).height);
        let top = floor.max(ground);
        [
            f64::from(x) + 0.5,
            dwell_world::m_of_row(top + 1),
            f64::from(z) + 0.5,
        ]
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

    /// The river whose channel holds full-detail column (x, z), and its surface row (none off a
    /// channel, or at the sea's level).
    fn river_row_at(&self, x: i64, z: i64) -> Option<(u32, i32)> {
        if !self.world().inside_disc(x, z) || !self.map.channels.near(x as f64, z as f64, 0.0) {
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

    /// The row a river's or lake's water surface lies on at full-detail column (x, z) (the rows
    /// below it water); none where no river channel or lake is. A light lookup for the waterline
    /// (docs/WATER_FLOW.md §17).
    fn water_row_at(&self, x: i64, z: i64) -> Option<i32> {
        if !self.world().inside_disc(x, z) {
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

    /// The arc cones' column-level detail on ground `h` m high with the map's gradient (`gx`,
    /// `gz`) at (x, z): the ground and its gradient with the detail (by central differences over
    /// half a `cell`), unchanged away from the cones and when it is off.
    fn peak_relief(&self, x: i64, z: i64, h: f64, gx: f64, gz: f64, cell: i64) -> (f64, f64, f64) {
        if self.peaks.is_empty() {
            return (h, gx, gz);
        }
        let t = &self.map.tectonics;
        let (fx, fz) = (x as f64, z as f64);
        let at = |px: f64, pz: f64, ph: f64| t.peak_relief(&self.peaks, px, pz, ph);
        let shaped = at(fx, fz, h);
        if shaped == h {
            return (h, gx, gz);
        }
        let e = 0.5 * cell.max(1) as f64;
        (
            shaped,
            (at(fx + e, fz, h + gx * e) - at(fx - e, fz, h - gx * e)) / (2.0 * e),
            (at(fx, fz + e, h + gz * e) - at(fx, fz - e, h - gz * e)) / (2.0 * e),
        )
    }

    /// The landform's slope at full-detail column (x, z) as the snow line reads it (rise per run):
    /// the map's, with the arc cones' column-level detail (Rainier detail) over the biomes' fine relief
    /// (away from the water, whose banks fade that relief).
    pub fn cover_slope_at(&self, x: i64, z: i64) -> f64 {
        let (base, gx, gz) = self.map.height_at(x as f64, z as f64);
        let mix = self.biomes.mix(x, z, base);
        let h = base + self.blended_detail(&mix, x, z, base, 1);
        let (_, gx, gz) = self.peak_relief(x, z, h, gx, gz, 1);
        (gx * gx + gz * gz).sqrt()
    }

    /// The arc cones' column-level detail as placed for this map (empty when off).
    pub fn peak_details(&self) -> &[crate::tectonics::PeakDetail] {
        &self.peaks
    }

    fn base_column(&self, x: i64, z: i64, cell: i64) -> Column {
        self.base_column_with(x, z, cell, true)
    }

    /// `base_column`, or with `beds` false the ground with the rivers' valleys but no beds: a
    /// channel's columns take the valley floor at its banks' height (no bed cut, no lips, no water
    /// drawn), for the water surface grid's prototype (Phase 53 G0, ADR 0060).
    fn base_column_with(&self, x: i64, z: i64, cell: i64, beds: bool) -> Column {
        if !self.world().inside_disc(x, z) {
            return Column {
                outside: true,
                ..Column::default()
            };
        }
        let map = &*self.map;
        let (base, mut gx, mut gz) = map.height_at(x as f64, z as f64);
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
        // The arc cones' column-level detail (Rainier detail: cleavers, the broken rim, crags, cirques),
        // finer than the map's cells; its slope joins the map's (the snow line reads it).
        (h, gx, gz) = self.peak_relief(x, z, h, gx, gz, cell);
        // The rock's shaping (systemic terrain T2; none at strength 0).
        let (shaped, rock) = self.rock_shaping(&mix, x, z, h, fade);
        h = shaped;
        // The erosion's plan view (systemic terrain T4; none where no biome erodes).
        let (eroded, rock) = self.erosion_shaping(&mix, x, z, h, fade, rock);
        h = eroded;
        let on_lip = false;
        let stream = false;
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
                // Where a river joins one lower than it reaches (another river's water two rows
                // or more under it within two cells along an axis), its surface steps down to
                // that water a row a cell, as water: no wall of water and no dry bed (BUGS.md #62).
                let mut lvl = r.level;
                if cell == 1 {
                    let row = surface_row((SEA_LEVEL_M + r.level) as f32);
                    let mut cap = row;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        for d in 1..=2 {
                            if let Some((n, k)) = self.river_row_at(x + d * dx, z + d * dz)
                                && n != r.river
                            {
                                cap = cap.min(k + d as i32);
                            }
                        }
                    }
                    if cap < row {
                        lvl = f64::from(cap) * CELL_H - SEA_LEVEL_M - 0.25 * CELL_H;
                    }
                }
                // Below the surface as drawn (its row), so a channel always holds water cells.
                let drawn =
                    f64::from(surface_row((SEA_LEVEL_M + lvl) as f32)) * CELL_H - SEA_LEVEL_M;
                h = h.min(drawn.min(lvl) - depth);
                // In a lake the lake's flat surface wins (a river running into it ends there).
                if water != Water::Lake {
                    water = Water::River;
                    level = lvl;
                    // No lips (BUGS.md #29, Phase 53 G1a) and no dry chutes (BUGS.md #62): a step
                    // is the water falling a row from one reach to the next, the bed under each
                    // at its own depth, so the bed only falls downstream however steep the river;
                    // the water grid rests what is drawn (WATER_SURFACE_GRID.md §2b).
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
        let mut palette = self.biomes.first[usize::from(mix.chosen)] + level_band as u16;
        let mut band = band;
        // The snow line from temperature (0.52.40, opt-in by the world): snow on cold, gentle
        // ground over the band's and the rock's blocks; cold steep faces bare rock.
        let snow = if band == Band::Shore {
            None
        } else {
            self.biomes
                .snow_cover(usize::from(mix.chosen), x, z, h, slope, |t| {
                    self.vegetation.celsius(t)
                })
        };
        if snow == Some(None) {
            band = Band::Rock;
        }
        // Where the rock shapes the ground, its blocks show: a wall its face, talus its scree, a
        // tread its cap, a massive rock's steep faces its face.
        let wall = matches!(rock, Some((_, crate::geology::Part::Wall))) && band != Band::Shore;
        if let Some((r, part)) = rock
            && band != Band::Shore
        {
            use crate::geology::Part;
            let scree = self.biomes.rock_palette[r];
            match part {
                Part::Wall => (palette, band) = (scree, Band::Rock),
                Part::Talus => (palette, band) = (scree, Band::Ground),
                Part::Tread => (palette, band) = (self.tread_palette(r, x, z, base), Band::Ground),
                Part::Plain if band == Band::Rock => palette = scree,
                Part::Plain => {}
            }
        }
        if let Some(Some(p)) = snow {
            (palette, band) = (p, Band::Ground);
        } else if biome.soil_blocks
            && band == Band::Ground
            && palette == self.biomes.first[usize::from(mix.chosen)] + level_band as u16
        {
            // T3b: the soil type's blocks (sand under the grass on sandstone, say) on the band's
            // own ground, by the rock at the surface.
            let rock = self.surface_rock(&mix, x, z, base);
            let soil = self.soil_type_of(rock.name, x, z, base);
            if let Some(p) = self
                .biomes
                .soil_palette(usize::from(mix.chosen), level_band, soil)
            {
                palette = p;
            }
        }
        // The ground's ordered blend by stack (rocks.json `blends`, 0.64): mesa country's red
        // dirt, red sand and tan in order, no hard bands.
        if snow != Some(None) {
            palette = self.blended(&mix, x, z, palette, gx.hypot(gz));
        }
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
            wall,
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
        if !self.world().inside_disc(x, z) {
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
            || !self
                .world()
                .volume()
                .contains_voxel(i64::from(x), i64::from(y), i64::from(z))
        {
            return false;
        }
        let col = self.column_at(x, z);
        !col.outside && y <= top_voxel(col.height)
    }

    /// How many ramp pieces top column (x, z) (0: a flat top), by the generation rule over its
    /// neighbours (`generate`'s, from the columns alone).
    pub fn ramp_pieces_at(&self, x: i32, z: i32) -> usize {
        let col = self.column_at(x, z);
        if col.outside {
            return 0;
        }
        let top = top_voxel(col.height).clamp(FACE_MIN_Y, FACE_MAX_Y - 1);
        self.pieces_with(|nx, nz| self.column_at(nx, nz), x, z, &col, top)
            .len()
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

    /// The monuments reaching columns [x0, x1] × [z0, z1], placed: the portals' and the shrines'.
    fn monuments_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<Placed> {
        let mut out = self.portal_monuments_near(lo, hi);
        out.extend(self.shrines_placed_near(lo, hi));
        out
    }

    /// The portals' monuments reaching columns [x0, x1] × [z0, z1], placed.
    fn portal_monuments_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<Placed> {
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

    /// The shrines' structures reaching columns [x0, x1] × [z0, z1], placed.
    fn shrines_placed_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<Placed> {
        let Some(sh) = self.shrines.as_deref() else {
            return Vec::new();
        };
        sh.near(self, lo, hi)
            .into_iter()
            .map(|s| self.shrine(sh, s))
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

    /// The generator with `vegetation` (`vegetation.json`) in place of the build's.
    /// A biome with `soilBlocks` (T3b) shows its soil types' blocks: when they differ from the
    /// biomes' set's, the set is rebuilt with them.
    pub fn with_vegetation(mut self, vegetation: crate::vegetation::Vegetation) -> Self {
        self.vegetation = vegetation;
        self.cache_id = next_cache_id();
        let b = &self.biomes;
        if b.list.iter().any(|x| x.soil_blocks) && b.soil_list != vegetation.soil_types {
            let set = BiomeSet::with_soils(
                &b.list,
                &self.map,
                self.world_seed,
                b.params,
                b.geology.geology,
                vegetation.soil_types,
            );
            self.ramps = Arc::new(RampPieces::resolve(&set.palettes));
            self.biomes = Arc::new(set);
        }
        self
    }

    /// °C at face-local (x, z) over `elevation` m (the biome map's temperature with its lapse
    /// rate, as the snow line reads it; T3b).
    pub fn celsius_at(&self, x: i64, z: i64, elevation: f64) -> f64 {
        self.vegetation
            .celsius(self.biomes.temperature(x, z, elevation))
    }

    /// This generator's soil and vegetation.
    pub fn vegetation(&self) -> &crate::vegetation::Vegetation {
        &self.vegetation
    }

    /// How much the climate rules placement at (x, z): the biomes' `vegetation` by their weights
    /// (0 with one biome: no biome map, no climate).
    fn vegetation_strength(&self, x: i64, z: i64, base: f64) -> f64 {
        if self.biomes.field.is_none() || self.biomes.list.iter().all(|b| b.vegetation <= 0.0) {
            return 0.0;
        }
        self.biomes
            .mix(x, z, base)
            .iter()
            .map(|(i, w)| self.biomes.list[i].vegetation * w)
            .sum()
    }

    /// The climate and soil at face-local (x, z) (systemic terrain T3), the ground's slope from
    /// `height` (m, any column; central differences over ±2 m); None with one biome (no biome map).
    pub fn climate_with(
        &self,
        x: i64,
        z: i64,
        height: &dyn Fn(i64, i64) -> Option<f32>,
    ) -> Option<crate::vegetation::Climate> {
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        let geo = self.biomes.geography(x, z, base)?;
        let at = |dx: i64, dz: i64| height(x + dx, z + dz);
        let slope_deg = match (at(2, 0), at(-2, 0), at(0, 2), at(0, -2)) {
            (Some(e), Some(w), Some(s), Some(n)) => {
                let (gx, gz) = (f64::from(e - w) * 0.25, f64::from(s - n) * 0.25);
                (gx * gx + gz * gz).sqrt().atan().to_degrees()
            }
            _ => 90.0,
        };
        let v = &self.vegetation;
        let (rock, _) = self.rock_at_surface(x, z);
        let (soil_depth, soil_type) = v.soil(
            rock.name,
            slope_deg,
            geo.wetness,
            v.celsius(geo.temperature),
        );
        Some(crate::vegetation::Climate {
            temperature_c: v.celsius(geo.temperature),
            moisture: geo.wetness,
            soil_depth,
            soil_type,
            slope_deg,
            strength: self.vegetation_strength(x, z, base),
        })
    }

    /// [`TerrainGenerator::climate_with`] over the generated columns' heights (Chisel's overlays, tests).
    pub fn climate_at(&self, x: i64, z: i64) -> Option<crate::vegetation::Climate> {
        let h = |x: i64, z: i64| {
            let c = self.column_at(x as i32, z as i32);
            (!c.outside).then_some(c.height)
        };
        self.climate_with(x, z, &h)
    }

    /// The climate's factor on a placement's chance at (x, z) (1 for a placement no kind governs).
    fn climate_factor(
        &self,
        kind: Option<usize>,
        x: i64,
        z: i64,
        height: &dyn Fn(i64, i64) -> Option<f32>,
    ) -> f32 {
        let Some(k) = kind else { return 1.0 };
        self.climate_with(x, z, height)
            .map_or(1.0, |c| self.vegetation.factor(k, &c) as f32)
    }

    /// The vegetation's density at (x, z) for the placements of the biome the column takes (the
    /// most of its kinds': 0 where none grows, 1 where the climate doesn't rule; Chisel's overlay).
    pub fn vegetation_at(&self, x: i64, z: i64) -> Option<f64> {
        let c = self.climate_at(x, z)?;
        let b = &self.biomes.list[usize::from(self.biome_mix(x, z).chosen)];
        Some(
            b.placements
                .iter()
                .filter_map(|p| self.vegetation.kind_for(p))
                .map(|k| self.vegetation.factor(k, &c))
                .fold(0.0, f64::max),
        )
    }

    /// Planter `i`'s density at (x, z) from the biomes' `trees` by their weights (ADR 0074 B5; 1
    /// where it does not vary).
    pub fn tree_density_at(&self, i: usize, x: i64, z: i64) -> f32 {
        let (per, varies) = &*self.tree_density;
        if !varies {
            return 1.0;
        }
        let d = &per[i];
        if self.biomes.field.is_none() {
            return d[0];
        }
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        self.biomes
            .mix(x, z, base)
            .iter()
            .map(|(b, w)| d[b] * w as f32)
            .sum()
    }

    /// Whether the biomes' `ground` varies the ground items (ADR 0074 B5; else they ask nothing of
    /// the biomes: the default world).
    pub fn ground_varies(&self) -> bool {
        self.ground_biomes().iter().any(|b| b.ground.is_some()) || self.undergrowth_rules_ground()
    }

    /// Whether a biome's `undergrowth` scales a ground item kind by the climate (T3b): a biome
    /// map, a biome with it above 0 and a kind governing a ground item.
    fn undergrowth_rules_ground(&self) -> bool {
        self.biomes.field.is_some()
            && self.biomes.list.iter().any(|b| b.undergrowth > 0.0)
            && self.vegetation.kinds.iter().any(|k| !k.ground.is_empty())
    }

    /// The climate's factor on biome `b`'s decoration or ground item governed by `kind` (T3b): 1
    /// where it does not rule (no kind, `undergrowth` 0 or no climate), else 1 − u + u × the
    /// kind's density.
    fn undergrowth_factor(
        &self,
        b: &Biome,
        kind: Option<usize>,
        climate: impl FnOnce() -> Option<crate::vegetation::Climate>,
    ) -> f64 {
        let Some(k) = kind.filter(|_| b.undergrowth > 0.0) else {
            return 1.0;
        };
        climate().map_or(1.0, |c| {
            self.vegetation.factor(
                k,
                &crate::vegetation::Climate {
                    strength: b.undergrowth,
                    ..c
                },
            )
        })
    }

    /// Grass growth's envelope at face-local column (x, z) (ADR 0075 T3c): decoration `name`'s own
    /// per-cell roll and the climate's share of it there — 0 where the column's biome doesn't list
    /// it (with more than one biome), else its biome's undergrowth factor (1 without a climate).
    /// None when no decoration is named so.
    pub fn grass_envelope(&self, name: &str, x: i32, z: i32) -> Option<(f32, f64)> {
        let i = DECORATIONS.iter().position(|d| d.name == name)?;
        let roll = self.decorations.get(i)?.roll(x, z);
        if self.biomes.list.len() <= 1 {
            return Some((roll, 1.0));
        }
        let col = self.column_at(x, z);
        let Some(b) = self
            .biomes
            .biome_of_palette(col.palette)
            .map(|b| &self.biomes.list[b])
        else {
            return Some((roll, 1.0));
        };
        if !b.decorations.contains(&name) {
            return Some((roll, 0.0));
        }
        let kind = self.vegetation.kind_for_decoration(name);
        let share = self.undergrowth_factor(b, kind, || self.climate_at(x.into(), z.into()));
        Some((roll, share))
    }

    /// The biomes whose weights count (one biome without a biome map).
    fn ground_biomes(&self) -> &[Biome] {
        if self.biomes.field.is_none() {
            &self.biomes.list[..1.min(self.biomes.list.len())]
        } else {
            &self.biomes.list[..]
        }
    }

    /// Ground item kind `name`'s density at face-local column (x, z) from the biomes' `ground` by
    /// their weights (ADR 0074 B5; 1 where it does not vary).
    /// With a biome's `undergrowth` (T3b) its share is scaled by the climate's kind for `name`.
    pub fn ground_density_at(&self, name: &str, x: i64, z: i64) -> f32 {
        let list = self.ground_biomes();
        let climate_rules = self.undergrowth_rules_ground();
        if !climate_rules && !list.iter().any(|b| b.ground.is_some()) {
            return 1.0;
        }
        if self.biomes.field.is_none() {
            return ground_density_of(&list[0], name);
        }
        let kind = self.vegetation.kind_for_ground(name);
        let climate = std::cell::OnceCell::new();
        let (base, _, _) = self.map.height_at(x as f64, z as f64);
        self.biomes
            .mix(x, z, base)
            .iter()
            .map(|(b, w)| {
                let b = &self.biomes.list[b];
                let f = self
                    .undergrowth_factor(b, kind, || *climate.get_or_init(|| self.climate_at(x, z)));
                ground_density_of(b, name) * (w * f) as f32
            })
            .sum()
    }

    /// Chunk column (cx, cz)'s ground densities (face-local), each kind's at its four corners
    /// (`dwell_world::ground::GroundDensity`); empty where they do not vary.
    pub fn ground_density_chunk(&self, cx: i32, cz: i32) -> dwell_world::ground::GroundDensity {
        if !self.ground_varies() {
            return Default::default();
        }
        let n = i64::from(dwell_world::CHUNK_SIZE);
        let (x0, z0) = (i64::from(cx) * n, i64::from(cz) * n);
        let corners = dwell_world::ground::KINDS
            .iter()
            .map(|k| {
                [(x0, z0), (x0 + n, z0), (x0, z0 + n), (x0 + n, z0 + n)]
                    .map(|(x, z)| self.ground_density_at(k.name, x, z))
            })
            .collect();
        dwell_world::ground::GroundDensity { corners }
    }

    /// The structures expected per hectare at (x, z) by the placement rules, the biomes' `trees`
    /// and the climate, before the slope, footprint and spacing tests (Chisel's placement overlay,
    /// ADR 0074 B5); None beyond the rim. `boulders` and every other rule count alike.
    pub fn placement_density_at(&self, x: i64, z: i64) -> Option<f64> {
        self.placement_densities_at(x, z).map(|d| d.iter().sum())
    }

    /// [`TerrainGenerator::placement_density_at`] per planter (in [`TerrainGenerator::planters`]'
    /// order): Chisel's overlay colours each by its structure's canopy block.
    pub fn placement_densities_at(&self, x: i64, z: i64) -> Option<Vec<f64>> {
        let col = self.column_at(x as i32, z as i32);
        let g = self.ground_of(&col)?;
        let mut out = vec![0.0; self.planters.len()];
        if !g.dry {
            return Some(out);
        }
        let climate = self.climate_at(x, z);
        for (i, p) in self.planters.iter().enumerate() {
            if !p.active() || !p.stands_on(g.block) {
                continue;
            }
            let mut c = f64::from(p.chance(x as i32, z as i32, g.above_sea))
                * f64::from(self.tree_density_at(i, x, z));
            if let (Some(k), Some(cl)) = (self.vegetation.kind_for(p.rule().name), &climate)
                && self.climate_rules()
            {
                c *= self.vegetation.factor(k, cl);
            }
            let grid = f64::from(p.rule().grid);
            out[i] = c.min(1.0) * 10_000.0 / (grid * grid);
        }
        Some(out)
    }

    /// Whether any biome lets the climate rule its placements (else `trees_in` asks nothing of it).
    fn climate_rules(&self) -> bool {
        self.biomes.field.is_some()
            && self.biomes.list.iter().any(|b| b.vegetation > 0.0)
            && !self.vegetation.kinds.is_empty()
    }

    /// The structures placed reaching columns [x0, x1] × [z0, z1] (face-local), the ground they
    /// ask for computed at most once per column of the rectangle and its margin, and only the
    /// columns asked (a species' wide crown makes the margin large and its candidates sparse;
    /// ADR 0079).
    /// The name of the placement rule a placed tree's `planter` is (`placements.json`).
    pub fn planter_name(&self, planter: u8) -> Option<&'static str> {
        self.planters
            .get(usize::from(planter))
            .map(|p| p.rule().name)
    }

    pub fn trees_in(&self, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) -> Vec<Tree> {
        if self.planters.is_empty() {
            return Vec::new();
        }
        let m = placements::ground_margin(&self.planters)
            + self
                .tree_spacing
                .as_ref()
                .as_ref()
                .map_or(0, |(_, most)| *most);
        let (rx, rz) = (x0 - m, z0 - m);
        let (w, l) = (x1 - x0 + 1 + 2 * m, z1 - z0 + 1 + 2 * m);
        let cache: std::cell::RefCell<Vec<Option<Option<Ground>>>> =
            std::cell::RefCell::new(vec![None; (w * l) as usize]);
        let ground = |x: i32, z: i32| {
            let (i, j) = (x - rx, z - rz);
            if (0..w).contains(&i) && (0..l).contains(&j) {
                let k = (j * w + i) as usize;
                if let Some(g) = cache.borrow()[k] {
                    return g;
                }
                let g = self.ground_of(&self.column_at(x, z));
                cache.borrow_mut()[k] = Some(g);
                g
            } else {
                self.ground_of(&self.column_at(x, z))
            }
        };
        self.trees_over((x0, z0), (x1, z1), &ground)
    }

    /// The species trees reaching columns [x0, x1] × [z0, z1] (face-local) with their leaf tints
    /// (ADR 0079: each tree's seed drawn through its species' `variety`), for the meshers
    /// (`dwell_world::tree_tint`). Empty when no placement stands a species (the default world's
    /// hand-built pines carry none) or a species' variety draws no tint.
    pub fn tree_tints(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<TreeTint> {
        if !self.planters.iter().any(|p| p.active() && p.grows()) {
            return Vec::new();
        }
        self.trees_in(lo, hi)
            .into_iter()
            .filter_map(|t| {
                let p = &self.planters[t.planter as usize];
                let (i, seed, _) = p.grown(&t)?;
                let tint = crate::species::tint(crate::species::SPECIES.get(usize::from(i))?, seed);
                if tint.is_none() {
                    return None;
                }
                let placed = p.place(&t);
                Some(TreeTint {
                    anchor: t.anchor,
                    min: placed.min,
                    max: placed.max,
                    tint,
                })
            })
            .collect()
    }

    /// The species trees anchored in columns [x0, x1] × [z0, z1] (face-local), each listed once
    /// (by its anchor, so neighbouring sections never share a tree), sorted: the far tree list an
    /// impostor section draws (docs/proposals/TREE_IMPOSTORS.md I-2). Placements that are not
    /// species trees (hand-built structures) have no impostor and are left out.
    pub fn far_trees(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<crate::impostor::FarTree> {
        if !self.planters.iter().any(|p| p.active() && p.grows()) {
            return Vec::new();
        }
        let mut out: Vec<_> = self
            .trees_in(lo, hi)
            .into_iter()
            .filter(|t| {
                (lo.0..=hi.0).contains(&t.anchor[0]) && (lo.1..=hi.1).contains(&t.anchor[2])
            })
            .filter_map(|t| {
                let p = &self.planters[t.planter as usize];
                let (species, seed, stage) = p.grown(&t)?;
                let (stem, stem_cells) = p.stem(&t);
                Some(crate::impostor::FarTree {
                    anchor: t.anchor,
                    species,
                    seed,
                    stage,
                    stem,
                    stem_cells,
                })
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The leaf tints of an LOD section's tree cells (ADR 0079), at the levels whose cells hold
    /// the trees' own (`lod_trees`): for each non-air cell of `rows` (rows of `cells`, the
    /// section's world rows; `row_of` the face-local row each was generated at, from `origin_y`),
    /// its `lod::cell_index` and the tint of the tree owning the cell's centre — the voxel the
    /// chunks would ask of the same tree, so near and far leaves agree.
    #[allow(clippy::too_many_arguments)]
    pub fn lod_tints_at(
        &self,
        c: lod::LodCoord,
        origin_y: i64,
        rows: std::ops::RangeInclusive<i32>,
        row_of: &dyn Fn(i32) -> i32,
        cells: &[u16],
        out: &mut Vec<(u32, Tint)>,
    ) {
        let cell = lod::cell_size(c.level);
        if cell > LOD_TREE_CELL || cells.len() < lod::VOLUME {
            return;
        }
        let mut o = lod::section_origin(c);
        o.y = origin_y;
        let n = lod::SECTION_CELLS;
        let half = cell / 2;
        let centre = |o: i64, i: i32| (o + i64::from(i) * cell + half) as i32;
        let trees = self.tree_tints(
            (centre(o.x, -1), centre(o.z, -1)),
            (centre(o.x, n), centre(o.z, n)),
        );
        if trees.is_empty() {
            return;
        }
        for r in rows {
            let y = (o.y + i64::from(row_of(r)) * cell + half) as i32;
            for z in -1..=n {
                for x in -1..=n {
                    let i = lod::cell_index(x, r, z);
                    if cells[i] == M.air {
                        continue;
                    }
                    let t = tint_at(&trees, [centre(o.x, x), y, centre(o.z, z)]);
                    if !t.is_none() {
                        out.push((i as u32, t));
                    }
                }
            }
        }
    }

    /// [`TerrainGenerator::trees_in`] asking each column only as a candidate needs it (no
    /// rectangle computed up front): the same trees, cheaper over a large square where few cells
    /// pass their rolls (Chisel's 3D section, ADR 0074 B5).
    pub fn trees_sparse(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<Tree> {
        if self.planters.is_empty() {
            return Vec::new();
        }
        let ground = |x: i32, z: i32| self.ground_of(&self.column_at(x, z));
        self.trees_over(lo, hi, &ground)
    }

    /// The structures placed reaching [x0, x1] × [z0, z1], `ground` answering any column.
    fn trees_over(
        &self,
        (x0, z0): (i32, i32),
        (x1, z1): (i32, i32),
        ground: &dyn Fn(i32, i32) -> Option<Ground>,
    ) -> Vec<Tree> {
        // The climate's factor on each candidate (systemic terrain T3): none unless a biome opts in,
        // so the default world asks nothing of it.
        let kinds: Vec<Option<usize>> = self
            .planters
            .iter()
            .map(|p| self.vegetation.kind_for(p.rule().name))
            .collect();
        let height = |x: i64, z: i64| ground(x as i32, z as i32).map(|g| g.height);
        let climate = self.climate_rules();
        let factor = |i: usize, x: i32, z: i32| -> f32 {
            let (x, z) = (i64::from(x), i64::from(z));
            let c = if climate {
                self.climate_factor(kinds[i], x, z, &height)
            } else {
                1.0
            };
            c * self.tree_density_at(i, x, z)
        };
        let factor: Option<&placements::Factor<'_>> =
            (climate || self.tree_density.1).then_some(&factor);
        // The biomes' spacing at an anchor: its column's biome's (0.52.65).
        let spacing_at = |i: usize, x: i32, z: i32| -> i32 {
            let Some((per, _)) = self.tree_spacing.as_ref() else {
                return self.planters[i].rule().spacing;
            };
            let b = if self.biomes.field.is_none() {
                0
            } else {
                usize::from(self.biome_mix(i64::from(x), i64::from(z)).chosen)
            };
            per[i][b.min(per[i].len() - 1)]
        };
        let spacing = self
            .tree_spacing
            .as_ref()
            .as_ref()
            .map(|(_, most)| placements::Spacing {
                at: &spacing_at,
                most: *most,
            });
        let mut trees = placements::trees_in_with(
            &self.planters,
            (x0, z0),
            (x1, z1),
            ground,
            factor,
            spacing.as_ref(),
        );
        // No structure stands over a portal's opening or its clear ground round the rim.
        if let Some(p) = &self.portals {
            trees.retain(|t| {
                let fp = self.planters[t.planter as usize].footprint(t);
                !p.box_reaches_clear((fp[0], fp[1]), (fp[2], fp[3]))
            });
        }
        // Nor over a shrine or its clear ground.
        if let Some(sh) = self.shrines.as_deref() {
            trees.retain(|t| {
                let fp = self.planters[t.planter as usize].footprint(t);
                !sh.box_reaches_clear(self, (fp[0], fp[1]), (fp[2], fp[3]))
            });
        }
        trees
    }

    /// `is_air_chunk` given `sky_floor_at(coord.x, coord.z)`.
    pub fn is_air_chunk_with(
        world: &dwell_world::WorldBounds,
        coord: ChunkCoord,
        sky_floor: f32,
    ) -> bool {
        let y0 = coord.y * CHUNK_SIZE;
        if y0 + CHUNK_SIZE <= FACE_MIN_Y || y0 >= FACE_MAX_Y {
            return true;
        }
        if world.chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
            || !world.volume().chunk_may_hold(coord)
        {
            return true;
        }
        y0 >= SEA_LEVEL && y0 as f32 > sky_floor
    }

    /// True when `generate` would leave the chunk all air: exactly the generator's own shortcut.
    pub fn is_air_chunk(&self, coord: ChunkCoord) -> bool {
        let y0 = coord.y * CHUNK_SIZE;
        if !(SEA_LEVEL..FACE_MAX_Y).contains(&y0)
            || self.world().chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
            || !self.world().volume().chunk_may_hold(coord)
        {
            return Self::is_air_chunk_with(&self.world(), coord, 0.0);
        }
        Self::is_air_chunk_with(&self.world(), coord, self.sky_floor_at(coord.x, coord.z))
    }

    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        if y0 + S <= FACE_MIN_Y || y0 >= FACE_MAX_Y {
            return;
        }
        // Beyond the rim of the world's disc or outside the build volume: nothing.
        let volume = self.world().volume().chunk_overlap(coord);
        if self.world().chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
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
        // The placed structures reaching this chunk, and the pads under their trunks (WORK_QUEUE
        // item 44): each such column's ground levelled up to the row under its trunk's base.
        let trees = self.chunk_trees(coord, &cols);
        let pads = if trees.is_empty() {
            Vec::new()
        } else {
            placements::pads_over(&self.planters, &trees, (x0, z0), (x0 + S - 1, z0 + S - 1))
        };
        let voxels = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside {
                    continue;
                }
                let pad = pads.get((z * S + x) as usize).copied().flatten();
                let top = top_voxel(col.height).max(pad.map_or(i32::MIN, |b| b - 1));
                let water = col.water_row();
                let flood = col.flood_row();
                let (part, portal) = near
                    .as_ref()
                    .map_or((Part::None, None), |(p, n)| p.locate(n, x0 + x, z0 + z));
                let in_volume = |y: i32| {
                    (FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                        && (volume != Overlap::Partial
                            || self.world().volume().contains_voxel(
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
                // A trunk's pad is flat: full cells to its top, no ramp under the trunk.
                let pieces = if rim.is_some() || pad.is_some() {
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
        self.carve_arches(coord, &cols, volume, chunk);
        self.stamp_trees(coord, &trees, volume, chunk);
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
                            && !self.world().volume().contains_voxel(wx, i64::from(y), wz))
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

    /// The rock (its rocks.json name) at height `h` m in biome `biome`'s stack at face-local
    /// (x, z): its own `rockStack`, else the province's (an arch's `rock` filter).
    fn rock_name_at(&self, biome: usize, x: i64, z: i64, h: f64) -> &'static str {
        let geo = &self.biomes.geology;
        let stack = self.biomes.biome_stack[biome].unwrap_or_else(|| geo.province(x, z).0);
        geo.geology.rocks[geo.profiles[stack].map(h.max(0.0)).1].name
    }

    /// Whether a biome of this generator has arches (ADR 0074 B8; none in the default world).
    fn has_arches(&self) -> bool {
        self.biomes
            .list
            .iter()
            .any(|b| b.arches.is_some_and(|a| a.density > 0.0))
    }

    /// The arches (ADR 0074 B8, `arches.rs`) whose openings may reach columns `lo..=hi` (x, z):
    /// each site square's draw under the biome its centre's column chose, kept where the height
    /// field stands a fin there. A function of the world position alone, so every chunk agrees.
    pub fn arches_near(&self, lo: (i32, i32), hi: (i32, i32)) -> Vec<crate::arches::Site> {
        use crate::arches::{CELL, SLIDE, draw, site_near};
        if !self.has_arches() {
            return Vec::new();
        }
        let reach = self
            .biomes
            .list
            .iter()
            .filter_map(|b| b.arches)
            .map(|a| {
                (a.fin * a.fin + a.span[1] * a.span[1]).sqrt() / 2.0
                    + if a.fins.is_some() { SLIDE } else { 0.0 }
            })
            .fold(0.0_f64, f64::max);
        let r = reach.ceil() as i32 + CELL;
        let seed = seed_word(self.world_seed, ARCH_STREAM);
        let ground = |x: f64, z: f64| {
            let c = self.column_at(x.floor() as i32, z.floor() as i32);
            (!c.outside && c.water == Water::None).then_some(f64::from(c.height))
        };
        let mut out = Vec::new();
        for gz in (lo.1 - r).div_euclid(CELL)..=(hi.1 + r).div_euclid(CELL) {
            for gx in (lo.0 - r).div_euclid(CELL)..=(hi.0 + r).div_euclid(CELL) {
                let d = draw(seed, gx, gz);
                let (x, z) = (d.centre[0].floor() as i64, d.centre[1].floor() as i64);
                let biome = if self.biomes.field.is_none() {
                    0
                } else {
                    let (base, _, _) = self.map.height_at(x as f64, z as f64);
                    usize::from(self.biomes.mix(x, z, base).chosen)
                };
                let Some(a) = self.biomes.list.get(biome).and_then(|b| b.arches) else {
                    continue;
                };
                if let Some(s) = site_near(&a, &d, ground)
                    && a.rock
                        .is_none_or(|r| self.rock_name_at(biome, x, z, s.floor + s.rise / 2.0) == r)
                {
                    out.push(s);
                }
            }
        }
        out
    }

    /// Carves the arches' openings (ADR 0074 B8) in a chunk filled to its columns' tops
    /// (`Site::carves`: under the roof, or the whole column down to the floor at a mouth; dry
    /// columns only).
    fn carve_arches(
        &self,
        coord: ChunkCoord,
        cols: &[Column],
        overlap: Overlap,
        chunk: &mut Chunk,
    ) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        let sites = self.arches_near((x0, z0), (x0 + S - 1, z0 + S - 1));
        if sites.is_empty() {
            return;
        }
        let ch = CELL_H;
        let voxels = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let col = cols[(z * S + x) as usize];
                if col.outside || col.water != Water::None {
                    continue;
                }
                let (wx, wz) = (x0 + x, z0 + z);
                let (cx, cz) = (f64::from(wx) + 0.5, f64::from(wz) + 0.5);
                let height = f64::from(col.height);
                for s in &sites {
                    if s.roof(cx, cz).is_none() {
                        continue;
                    }
                    for ly in 0..S {
                        let y = y0 + ly;
                        let mid = (f64::from(y) + 0.5) * ch;
                        if !s.carves(cx, cz, height, mid)
                            || !(FACE_MIN_Y..FACE_MAX_Y).contains(&y)
                            || (overlap == Overlap::Partial
                                && !self.world().volume().contains_voxel(
                                    i64::from(wx),
                                    i64::from(y),
                                    i64::from(wz),
                                ))
                        {
                            continue;
                        }
                        voxels[local_index(x, ly, z)] = M.air;
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
                        || self.world().volume().contains_voxel(
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

    /// The placed structures reaching this chunk: none unless it holds a row between the lowest
    /// ground and the highest crown (their trunks' pads lie in those rows too).
    fn chunk_trees(&self, coord: ChunkCoord, cols: &[Column]) -> Vec<Tree> {
        const S: i32 = CHUNK_SIZE;
        if self.planters.is_empty() {
            return Vec::new();
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
            return Vec::new();
        }
        self.trees_in((x0, z0), (x0 + S - 1, z0 + S - 1))
    }

    /// Stands the placed structures reaching this chunk (`chunk_trees`) in it, over air and
    /// replaceable cells only (decorations; never the ground or water).
    fn stamp_trees(&self, coord: ChunkCoord, trees: &[Tree], volume: Overlap, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let (x0, y0, z0) = (coord.x * S, coord.y * S, coord.z * S);
        let voxels = chunk.voxels_mut();
        for t in trees {
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
                        && !self.world().volume().contains_voxel(
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

    /// The stored ramp topping column (x, z) by the generation rule (HALF_HEIGHT_GRID.md §4),
    /// every ramp backed (BUGS.md #157), its neighbours' columns from `column`.
    fn shape_with(
        &self,
        column: &impl Fn(i32, i32) -> Column,
        x: i32,
        z: i32,
        col: &Column,
    ) -> Top {
        // The 5 × 5 window: the column's 3 × 3 and, for the backed rule (BUGS.md #157), the column
        // behind each ramp's high side with its own neighbours.
        let cols: [[Column; 5]; 5] = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                if (r, c) == (2, 2) {
                    *col
                } else {
                    column(x + c as i32 - 2, z + r as i32 - 2)
                }
            })
        });
        let around5: ramps_rule::Around5 = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                let col = &cols[r][c];
                // Beyond the rim, nothing: lower than any ground.
                if col.outside {
                    i32::MIN / 2
                } else {
                    top_voxel(col.height) + 1
                }
            })
        });
        // Where the rock shapes the ground (rocks.json `shaping`), notched ramps on its diagonal
        // contours too (owner, 2026-11-13): everywhere (`all`, 0.59–0.61), or only at a rock
        // wall's edge (`mesaEdges`: the column or a neighbour shows a wall; owner, 2026-11-13: "I'd
        // like the old rule to apply except for mesa edges").
        // The rule of the column at (dx, dz) (within ±1): its own 3 × 3 decides `mesaEdges`.
        let geo = &self.biomes.geology.geology;
        let rule = |(dx, dz): (i32, i32)| {
            let notched = match geo.notched {
                crate::geology::Notched::None => false,
                crate::geology::Notched::All => true,
                crate::geology::Notched::MesaEdges => (dz + 1..=dz + 3)
                    .any(|r| (dx + 1..=dx + 3).any(|c| cols[r as usize][c as usize].wall)),
            };
            if notched && geo.strength > 0.0 {
                ramps_rule::RampRule::Notched
            } else {
                ramps_rule::RampRule::Strict
            }
        };
        ramps_rule::top_backed_by(&around5, rows_for_m(1.0), rule)
    }

    /// The stored ramp the generation rule tops column (x, z) with (Flat: a full block; a lip's or
    /// a column without ramp pieces may still be drawn flat), from the columns alone.
    pub fn ramp_top_at(&self, x: i32, z: i32) -> Top {
        let col = self.column_at(x, z);
        if col.outside {
            return Top::Flat;
        }
        self.shape_with(&|nx, nz| self.column_at(nx, nz), x, z, &col)
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
        let shape = self.shape_with(&column, x, z, col);
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
        // Plants grow on flat tops, and a rule with `onRamps` over a ½ m ramp (one piece) too, its
        // foot drawn down to the ramp's surface; never over a 1 m ramp (HALF_HEIGHT_GRID.md §5).
        let ramp = match self.top_pieces(cols, x0, z0, lx, lz, top).len() {
            0 => false,
            1 => true,
            _ => return None,
        };
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
        // With more than one biome, only the rules the column's biome lists (its `decorations`,
        // 0.52.63); a world of one biome asks nothing (the default world as before).
        let biome = (self.biomes.list.len() > 1)
            .then(|| self.biomes.biome_of_palette(here.palette))
            .flatten()
            .map(|b| &self.biomes.list[b]);
        // A world of one biome takes that biome's list (its rules only: a rule another biome lists,
        // e.g. the old-growth undergrowth, stays out of the default world).
        let listed = biome
            .or_else(|| self.biomes.list.first())
            .map(|b| b.decorations);
        // T3b: a biome's `undergrowth` scales each rule a vegetation kind governs by the climate
        // (heights from this chunk's columns where they lie in it).
        let height = |hx: i64, hz: i64| {
            let (ix, iz) = (hx as i32 - x0, hz as i32 - z0);
            let col = if (0..S).contains(&ix) && (0..S).contains(&iz) {
                cols[(iz * S + ix) as usize]
            } else {
                self.column_at(hx as i32, hz as i32)
            };
            (!col.outside).then_some(col.height)
        };
        self.decorations
            .iter()
            .enumerate()
            // A column with no biome (a rock province's palette, or a world of one biome) takes the
            // decorations some biome of the world lists: one no biome lists stands nowhere (as a
            // placement no biome lists, `refresh_trees`), so adding one changes no other world.
            .filter(|(i, _)| {
                let name = &DECORATIONS[*i].name;
                match listed {
                    Some(d) => d.contains(name),
                    None => self
                        .biomes
                        .list
                        .iter()
                        .any(|b| b.decorations.contains(name)),
                }
            })
            .filter(|(_, p)| !ramp || p.on_ramps())
            // The rules' heights are metres above sea level.
            .find_map(|(i, p)| {
                let factor = || match biome {
                    Some(b) => {
                        let kind = self.vegetation.kind_for_decoration(DECORATIONS[i].name);
                        self.undergrowth_factor(b, kind, || {
                            self.climate_with(x.into(), z.into(), &height)
                        }) as f32
                    }
                    None => 1.0,
                };
                p.places_scaled(
                    (x, z),
                    noise,
                    ground,
                    top - SEA_LEVEL,
                    |nx, nz| neighbour_top(nx, nz) - SEA_LEVEL,
                    factor,
                )
                // Tall grass stands at its climate's top stage (docs/ECOLOGY.md §10).
                .then(|| {
                    let name = DECORATIONS[i].name;
                    if name == crate::grass::GRASS.decoration {
                        crate::grass::generated_tuft(name, p.state(), f64::from(factor()))
                    } else {
                        p.state()
                    }
                })
            })
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

    /// The block column (x, z)'s top shows from afar, at a coarse cell of `cell` m (as
    /// [`TerrainGenerator::column_lod`] samples it): its palette's top for its band (no ramps,
    /// decorations or snow pieces), air outside the disc. The home far bake's colour (`home_far`).
    pub fn far_surface(&self, x: i64, z: i64, cell: i64) -> StateId {
        let col = self.column_lod(x, z, cell);
        if col.outside {
            return M.air;
        }
        fill_soil(
            self.palette(col),
            col.band,
            top_voxel(col.height),
            0,
            under_rows(),
        )
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
        if !lod::in_world_of(c, &self.world()) {
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
        let portals = matches!(&self.portals,
            Some(p) if p.monument.is_some() && cell <= p.rules.monument_lod_cell);
        let shrines = matches!(&self.shrines, Some(s) if cell <= s.rules.lod_cell);
        if !portals && !shrines {
            return Vec::new();
        }
        let n = i64::from(lod::SECTION_CELLS);
        let lo = ((o.x - cell) as i32, (o.z - cell) as i32);
        let hi = ((o.x + (n + 1) * cell) as i32, (o.z + (n + 1) * cell) as i32);
        let mut out = Vec::new();
        if portals {
            out.extend(self.portal_monuments_near(lo, hi));
        }
        if shrines {
            out.extend(self.shrines_placed_near(lo, hi));
        }
        out
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
                // The trunk's pad (WORK_QUEUE item 44): a cell column whose centre is a trunk
                // column fills its air cells under the base down to its ground with the ground's
                // top block, so a far tree has no hole under it.
                let (pad, base) = p.pad(&t);
                for (wx, wz) in pad {
                    let (dx, dz) = (i64::from(wx) - o.x, i64::from(wz) - o.z);
                    if dx.rem_euclid(cell) != half || dz.rem_euclid(cell) != half {
                        continue;
                    }
                    let (xi, zi) = (dx.div_euclid(cell) as i32, dz.div_euclid(cell) as i32);
                    if !(-1..=n).contains(&xi) || !(-1..=n).contains(&zi) {
                        continue;
                    }
                    let col = cols[lod::column_index(xi, zi)];
                    if col.outside {
                        continue;
                    }
                    let state = self.top_state(&col);
                    for yi in (-1..=n).rev() {
                        let a = o.y + i64::from(yi) * cell;
                        if a >= i64::from(base) {
                            continue;
                        }
                        let m = &mut cells[lod::cell_index(xi, yi, zi)];
                        if *m != M.air {
                            break;
                        }
                        *m = state;
                    }
                }
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
        let climate = self.climate_rules();
        let varies = self.tree_density.1;
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
                // The climate (systemic terrain T3) on the canopy's share, the slope from the LOD's
                // own gradient.
                let plane = |x: i64, z: i64| {
                    Some(
                        col.height
                            + gx * (x - i64::from(wx)) as f32
                            + gz * (z - i64::from(wz)) as f32,
                    )
                };
                let Some((_, p)) = self.planters.iter().enumerate().find(|(i, p)| {
                    let f = || {
                        let c = if climate {
                            let k = self.vegetation.kind_for(p.rule().name);
                            self.climate_factor(k, wx.into(), wz.into(), &plane)
                        } else {
                            1.0
                        };
                        c * self.tree_density_at(*i, wx.into(), wz.into())
                    };
                    let f: Option<&dyn Fn() -> f32> = (climate || varies).then_some(&f);
                    p.canopy_at_with((wx, wz), level, g, gx * gx + gz * gz, f)
                }) else {
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
        // With shrines, every spawn is at one (owner, 2026-11-12: "target the nearest shrine to that
        // point for initial spawn, so all spawns are at shrines"): the search's point's nearest.
        if let Some(sh) = self.shrines.as_deref() {
            return self.shrine_stand(sh, sh.spawn_shrine(self));
        }
        self.spawn_search_point()
    }

    /// What identifies this generator's spawn search beyond the seed (the spawn shrine's cache):
    /// its worldgen config, its world's bounds and its biomes.
    pub fn spawn_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.biomes.config_text().hash(&mut h);
        format!(
            "{}|{}|{:?}|{:016x}",
            self.world_seed,
            self.map().worldgen_text(),
            self.world(),
            h.finish()
        )
    }

    /// The spawn search's point: feet on open level ground near the origin (before the shrines
    /// move the spawn to the nearest one).
    pub fn spawn_search_point(&self) -> [f64; 3] {
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

/// The flow (m³/s) one lip cell passes: water's weir at a full row's head (`lip_cells`).
pub fn lip_cell_flow() -> f64 {
    dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, CELL_H)
}

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
    /// rivers of seeds 0 and 1 no column inside a river's channel stands dry over its water (no
    /// chute's dry bed either since BUGS.md #57). Red before: the lips' walls.
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

    /// BUGS.md #62 (owner, 2026-11-10: "grass sections a few blocks higher than the river bed,
    /// water kinda waterfalls over them"): a chute's bed stood dry a row under its surface, over
    /// the pool's bed above it, and its first column was raised a row over that pool's water (the
    /// waterline). Along rivers of seeds 0 and 7, on both faces, the owner's site among them, a
    /// channel's bed (its columns near the centreline) never rises downstream by more than
    /// 0.25 m, and every column in a channel holds water. Red before: the chutes' dry humps.
    #[test]
    fn a_rivers_bed_never_rises_downstream() {
        // both-faces: each face's own generator (face-local), the faces' seeds.
        let mut checked = 0;
        for seed in [0u64, 7] {
            for face in [dwell_world::Face::A, dwell_world::Face::B] {
                let g = TerrainGenerator::new(crate::bifacial::face_seed(seed, face));
                let mut picks = Vec::new();
                if seed == 0 && face == dwell_world::Face::A {
                    picks.push((-3850, 20040)); // the owner's site (river 42)
                }
                for r in &g.map.rivers {
                    for (i, v) in r.path.iter().enumerate() {
                        if i % 40 == 20 && v.surface > 3.0 {
                            picks.push((v.x as i64, v.z as i64));
                        }
                    }
                }
                picks.truncate(30);
                for (px, pz) in picks {
                    // (−level, height) of the centre columns, by river
                    let mut beds: dwell_world::hash::HashMap<u32, Vec<(f64, f32)>> =
                        Default::default();
                    for dx in -20..=20 {
                        for dz in -20..=20 {
                            let (x, z) = (px + dx, pz + dz);
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
                            let col = g.base_column(x, z, 1);
                            if col.water == Water::Lake || col.water == Water::Sea {
                                continue;
                            }
                            checked += 1;
                            assert!(
                                col.water == Water::River,
                                "seed {seed} {face:?}: ({x}, {z}) in river {}'s channel is dry at {}",
                                r.river,
                                col.height
                            );
                            // Downstream by the river's own level (it only falls along its
                            // course; the projection's distance along wanders across a bend).
                            if r.d < 0.1 * r.half {
                                beds.entry(r.river)
                                    .or_default()
                                    .push((-r.level, col.height));
                            }
                        }
                    }
                    for (river, mut bed) in beds {
                        bed.sort_by(|a, b| a.0.total_cmp(&b.0));
                        let mut lowest = f32::INFINITY;
                        for (s, h) in bed {
                            assert!(
                                h <= lowest + 0.25,
                                "seed {seed} {face:?}: river {river}'s bed rises to {h} under its level \
                                 {:.2}, over {lowest} upstream (near {px}, {pz})",
                                -s
                            );
                            lowest = lowest.min(h);
                        }
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
