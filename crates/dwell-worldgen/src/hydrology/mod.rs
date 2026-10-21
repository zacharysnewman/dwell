//! The world-scale hydrology prototype (docs/proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md): a coarse
//! map of one face of the disc, computed once per seed and face — the height field sampled on a
//! grid (`cell` m, data), eroded by stream power (`erosion.rs`), its remaining depressions made
//! lakes or removed (`lakes.rs`), the rain routed to the sea and accumulated, and the rivers traced
//! (`rivers.rs`). The world map (`worldmap`, ADR 0049) keeps what chunk generation reads of it.
//!
//! Face-local: a face's map is computed from that face's generator in its own frame (heights from
//! its midplane, up away from it), so face B's code is face A's.
//!
//! Deterministic (ADR 0010): one thread, a fixed order everywhere, strict IEEE arithmetic with
//! only +, −, ×, ÷, `sqrt` and `floor` (powers by `math.rs`), and ties broken by cell index, so a
//! seed's map is bit-identical natively and in WASM (`HydrologyMap::digest`, pinned by a test).
//!
//! The tunables are data: `shared/worldgen/hydrology.json`'s profiles, generated into
//! `profiles.gen.rs` by `shared/worldgen/gen.mjs`.

pub mod erosion;
pub mod flood;
pub mod lakes;
pub mod math;
pub mod rain;
pub mod rivers;
pub mod uplift;

use dwell_world::{FaceFrame, SEA_LEVEL_M, WORLD_RADIUS};

use crate::BifacialTerrain;
use crate::base::BaseField;
pub use flood::{Grid, NEIGHBOURS as NEIGHBOURS_D8, NONE};

/// A point of a river's width curve: at `discharge` (m³/s), `width` and `depth` (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverPoint {
    pub discharge: f64,
    pub width: f64,
    pub depth: f64,
}

/// How rain falls (a fixed vocabulary; data picks one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rain {
    /// The same everywhere.
    Uniform,
    /// Wet windward slopes and a dry lee (`rain.rs`).
    Orographic(Orographic),
}

/// Orographic rain's tunables (hydrology.json `rain`; `rain.rs` has the model).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orographic {
    /// The wind blows from this direction (degrees clockwise from −z), turned per seed by up to
    /// ± `spread` degrees.
    pub direction: f64,
    pub spread: f64,
    /// Extra rain per unit of windward slope (m per m, over a kilometre).
    pub upslope: f64,
    /// How far upwind (m) a barrier casts its rain shadow.
    pub fetch: f64,
    /// The share of the rain a barrier `shadow_height` m above the ground takes (in proportion
    /// below it).
    pub shadow: f64,
    pub shadow_height: f64,
    /// The relative rain is kept within [min, max].
    pub min: f64,
    pub max: f64,
}

/// The uplift layer (`uplift.rs`): a per-seed, per-face map of the tectonic strength (0 on the
/// coast's shelf, 1 on a range's spine) that sets the initial ground and the uplift rate, so
/// mountain ranges, highlands and plains come from data rather than from the height field's noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tectonics {
    /// Uplift (m/yr) where the strength is 1.
    pub rate: f64,
    /// The initial ground (m above sea level) where the strength is 1, added to the kept relief.
    pub base_height: f64,
    /// The share of the sampled height field's land relief kept under it.
    pub keep_relief: f64,
    /// The lowlands' strength: the floor everywhere inland.
    pub plains: f64,
    /// The strength fades to 0 at the coast over the shelf's width (m), which varies between
    /// `shelf_min` and `shelf_max` by noise of `shelf_wavelength`: a narrow shelf is a steep coast.
    pub shelf_min: f64,
    pub shelf_max: f64,
    pub shelf_wavelength: i32,
    /// The foothills around ranges and convergent boundaries (`uplift.rs` `Apron`): their
    /// strength at the spine (a share of the spine's) and their reach (m); 0: none.
    pub apron_strength: f64,
    pub apron_width: f64,
    pub ranges: Ranges,
    pub highlands: Highlands,
    pub plates: Plates,
}

/// Mountain ranges: `count` long curved spines per face (none when 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ranges {
    pub count: u32,
    /// Spine length (m), per range between min and max.
    pub length_min: f64,
    pub length_max: f64,
    /// Half-width (m) across the spine, where the strength falls to 0.
    pub width: f64,
    /// The strength on the spine (each range 75–100 % of it).
    pub strength: f64,
    /// How much a spine turns (radians per kilometre, a random walk).
    pub wander: f64,
    /// A domain warp of the strength (m, at `warp_wavelength` m) so the flanks are not smooth.
    pub warp: f64,
    pub warp_wavelength: i32,
    /// A spine's centre lies between these distances (m) from the coast.
    pub min_coast: f64,
    pub max_coast: f64,
    /// How far its heading turns from a random one toward the coast's direction (0–1).
    pub align: f64,
}

/// Broad highlands: low-frequency noise above a threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Highlands {
    /// 0: none.
    pub strength: f64,
    pub wavelength: i32,
    /// About the share of the land they cover.
    pub cover: f64,
}

/// Plate boundaries: `count` plates (Voronoi cells of random points over the land, each with a
/// random motion); convergent boundaries are uplifted into ranges, divergent ones sink (rifts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plates {
    pub count: u32,
    /// Half-width (m) of a boundary's band.
    pub width: f64,
    /// The strength at a head-on convergent boundary.
    pub strength: f64,
    /// The (negative) strength at a head-on divergent one.
    pub rift: f64,
    pub warp: f64,
    pub warp_wavelength: i32,
}

/// Hard and soft rock: parallel bands (a random strike per seed, warped) where the erodibility is
/// lower by `contrast`, so rivers cut gorges through them and they stand as cliffs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockBands {
    pub contrast: f64,
    /// Band spacing (m).
    pub wavelength: f64,
    /// The share of the land in hard bands.
    pub hard_share: f64,
    pub warp: f64,
    pub warp_wavelength: i32,
}

/// What a depression too small to be a lake becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallPits {
    Fill,
    Breach,
}

/// One profile's tunables (hydrology.json; units there).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HydrologyParams {
    pub name: &'static str,
    /// Grid cell size (m).
    pub cell: i32,
    pub iterations: u32,
    /// Years per iteration.
    pub time_step: f64,
    /// K, the exponents m (area) and n (slope).
    pub erodibility: f64,
    pub area_exponent: f64,
    pub slope_exponent: f64,
    /// Without `tectonics`: uplift (m/yr) where the original ground is `uplift_reference` m above
    /// sea level or higher, in proportion below it: the mountains stay where the height field put
    /// them.
    pub uplift: f64,
    pub uplift_reference: f64,
    /// The uplift layer; `None`: uplift from the height field (above).
    pub tectonics: Option<Tectonics>,
    /// Erodibility bands; `None`: one rock everywhere.
    pub rock_bands: Option<RockBands>,
    pub talus_slope: f64,
    pub talus_passes: u32,
    /// The share of a depression's depth that fills with sediment each iteration.
    pub lake_fill: f64,
    pub rain: Rain,
    /// m/yr.
    pub rainfall: f64,
    /// m², m.
    pub lake_min_area: f64,
    pub lake_min_depth: f64,
    pub small_pits: SmallPits,
    pub max_breach_depth: f64,
    /// m³/s.
    pub river_threshold: f64,
    pub river_geometry: &'static [RiverPoint],
    pub mouth_min_discharge: f64,
    /// How a mouth's slope is measured, and over how far (m).
    pub mouth_rule: MouthRule,
    pub mouth_reach: f64,
    pub delta_max_slope: f64,
}

mod table {
    include!("profiles.gen.rs");
}
pub use table::{DEFAULT, PROFILES};

/// A profile by name.
pub fn profile(name: &str) -> Option<&'static HydrologyParams> {
    PROFILES.iter().find(|p| p.name == name)
}

/// The profile tools use when none is named.
pub fn default_profile() -> &'static HydrologyParams {
    profile(DEFAULT).expect("hydrology.json's default names a profile")
}

/// A lake: a depression under one flat water surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Lake {
    /// Its water surface (m above sea level): its spill level.
    pub level: f64,
    /// The cell it drains to (dry land at its spill level, or the sea).
    pub outlet: u32,
    /// Its cell the outlet drains it from.
    pub entry: u32,
    pub cells: u32,
    /// m², m, m³.
    pub area: f64,
    pub max_depth: f64,
    pub volume: f64,
}

/// A point of a river's polyline (face-local m; `surface` above sea level).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub z: f64,
    pub surface: f64,
    pub discharge: f64,
    pub width: f64,
    pub depth: f64,
}

/// Where a river ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Sea,
    Lake(u32),
    /// It joins another river (its last vertex is on it).
    Joins,
}

#[derive(Clone, Debug, PartialEq)]
pub struct River {
    /// The lake it flows out of, when it starts at an outlet.
    pub from_lake: Option<u32>,
    pub end: End,
    pub path: Vec<Vertex>,
    /// Its cells, source first (the last vertex, in the sea, lake or river it ends in, has none).
    pub cells: Vec<u32>,
}

/// How a river mouth's slope is measured (`rivers.rs`), against `delta_max_slope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouthRule {
    /// The river's mean fall over its last `mouth_reach` m.
    RiverSlope,
    /// The highest land within `mouth_reach` m of the mouth, over that distance.
    CoastRelief,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouthKind {
    Delta,
    Estuary,
}

/// Where a large river meets the sea.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mouth {
    pub x: f64,
    pub z: f64,
    pub discharge: f64,
    /// The slope `mouth_rule` measured (m per m).
    pub slope: f64,
    pub river: u32,
    pub kind: MouthKind,
}

/// A river system's main stem (`HydrologyMap::main_stem`).
#[derive(Clone, Debug, PartialEq)]
pub struct Stem {
    pub cells: Vec<u32>,
    pub length: f64,
    pub lakes: Vec<u32>,
}

/// One face's hydrology map. Grid cell `i` is column `i % n`, row `i / n`, centred on face-local
/// (x0 + col·cell, x0 + row·cell). Heights are metres above sea level (≤ 0: the sea).
#[derive(Clone, Debug)]
pub struct HydrologyMap {
    pub grid: Grid,
    pub cell: f64,
    /// The centre of cell 0, on both axes (m).
    pub x0: f64,
    /// The height field as sampled, and after erosion and the small pits' removal.
    pub original: Vec<f32>,
    pub height: Vec<f32>,
    pub sea: Vec<bool>,
    /// The water surface: the ground on dry land, a lake's level, 0 at sea.
    pub surface: Vec<f32>,
    pub lake_of: Vec<u32>,
    /// Where each land cell drains (`NONE` at sea).
    pub receiver: Vec<u32>,
    /// m³/s through each land cell.
    pub discharge: Vec<f32>,
    pub lakes: Vec<Lake>,
    pub rivers: Vec<River>,
    pub mouths: Vec<Mouth>,
    /// The inputs the erosion ran with, per cell: uplift (m/yr), erodibility K and relative rain.
    pub uplift: Vec<f32>,
    pub erodibility: Vec<f32>,
    pub rain: Vec<f32>,
    /// The uplift layer's range spines and plate points (empty without `tectonics`).
    pub spines: Vec<uplift::Spine>,
    pub plates: Vec<(f64, f64)>,
}

/// The grid for a cell size: square, covering the world's disc, with a border of sea.
pub fn grid_for(cell: i32) -> (Grid, f64) {
    let r = i64::from(WORLD_RADIUS);
    let c = i64::from(cell);
    let n = ((2 * r + c - 1) / c + 2) as usize;
    let x0 = -((n as i64 - 1) * c) as f64 * 0.5;
    (Grid { n }, x0)
}

/// One face's map for a world seed (the face's own generator, face-local).
pub fn generate(world_seed: u64, face: FaceFrame, p: &HydrologyParams) -> HydrologyMap {
    generate_seed(crate::bifacial::face_seed(world_seed, face.face), p)
}

/// One face's map of a terrain (the mirror world's too, for face parity).
pub fn generate_for(
    terrain: &BifacialTerrain,
    face: FaceFrame,
    p: &HydrologyParams,
) -> HydrologyMap {
    generate_seed(terrain.local(face.face).seed(), p)
}

/// The map of the face whose generator's seed is `seed`.
pub fn generate_seed(seed: u64, p: &HydrologyParams) -> HydrologyMap {
    let (grid, x0, h) = sample_seed(seed, p.cell);
    from_heights(grid, f64::from(p.cell), x0, &h, seed, p)
}

/// The face's base field on the grid of `cell` m (m above sea level; −∞ beyond the disc): each
/// cell's centre, with the octaves finer than a cell dropped (the LOD's sampling).
pub fn sample(terrain: &BifacialTerrain, face: FaceFrame, cell: i32) -> (Grid, f64, Vec<f64>) {
    sample_seed(terrain.local(face.face).seed(), cell)
}

/// [`sample`] for the face whose generator's seed is `seed`.
pub fn sample_seed(seed: u64, cell: i32) -> (Grid, f64, Vec<f64>) {
    let g = BaseField::new(seed);
    let (grid, x0) = grid_for(cell);
    let n = grid.n;
    let cell = i64::from(cell);
    let half = (n as i64 - 1) * cell / 2;
    let mut h = Vec::with_capacity(grid.len());
    for row in 0..n {
        for col in 0..n {
            let x = col as i64 * cell - half;
            let z = row as i64 * cell - half;
            h.push(match g.height_lod(x, z, cell) {
                None => f64::NEG_INFINITY,
                Some(height) => f64::from(height) - SEA_LEVEL_M,
            });
        }
    }
    (grid, x0, h)
}

/// The map of a sampled height field (m above sea level; −∞ beyond the disc). The sea is the
/// grid's border and every cell at or below sea level connected to it; a hollow below sea level
/// cut off from it is land (a depression: a lake, or filled). `seed` (the face generator's) seeds
/// the uplift layer, the rock bands and the wind.
pub fn from_heights(
    grid: Grid,
    cell: f64,
    x0: f64,
    h0: &[f64],
    seed: u64,
    p: &HydrologyParams,
) -> HydrologyMap {
    assert_eq!(h0.len(), grid.len());
    let len = grid.len();
    let sea = open_sea(grid, h0);
    let original: Vec<f32> = h0.iter().map(|&v| v.max(-1e4) as f32).collect();
    let mut h: Vec<f64> = (0..len)
        .map(|i| {
            if sea[i] {
                h0[i].clamp(-1e4, 0.0)
            } else {
                h0[i]
            }
        })
        .collect();
    let (uplift, spines, plates) = match &p.tectonics {
        None => (
            h.iter()
                .map(|&v| p.uplift * (v / p.uplift_reference).clamp(0.0, 1.0))
                .collect::<Vec<f64>>(),
            Vec::new(),
            Vec::new(),
        ),
        Some(t) => {
            let coast = uplift::coast_distance(grid, cell, &sea);
            let u = uplift::generate(grid, cell, x0, &sea, &coast, seed, t);
            for (i, v) in h.iter_mut().enumerate() {
                if !sea[i] {
                    *v = t.keep_relief * *v + t.base_height * u.strength[i];
                }
            }
            let rate = u.strength.iter().map(|&s| t.rate * s).collect();
            (rate, u.spines, u.plates)
        }
    };
    let erodibility: Vec<f64> = match &p.rock_bands {
        None => vec![p.erodibility; len],
        Some(r) => uplift::rock_factor(grid, cell, x0, &sea, seed, r)
            .iter()
            .map(|&f| p.erodibility * f)
            .collect(),
    };
    let rain: Vec<f64> = match &p.rain {
        Rain::Uniform => vec![1.0; len],
        Rain::Orographic(o) => rain::orographic(grid, cell, &h, &sea, seed, o),
    };
    erosion::erode(grid, cell, &mut h, &uplift, &erodibility, &rain, &sea, p);
    lakes::remove_small_pits(grid, cell, &mut h, &sea, p);
    let d = lakes::drainage(grid, cell, &h, &sea);
    // Discharge (m³/s): rain over the drainage area, donors before receivers.
    let order = lakes::stack(grid, &d.receiver, &sea);
    debug_assert_eq!(order.len(), sea.iter().filter(|s| !**s).count());
    let per_cell = cell * cell * p.rainfall / SECONDS_PER_YEAR;
    let mut q = vec![0.0f64; len];
    for &c in &order {
        q[c as usize] = rain[c as usize] * per_cell;
    }
    for &c in order.iter().rev() {
        let r = d.receiver[c as usize];
        if !sea[r as usize] {
            q[r as usize] += q[c as usize];
        }
    }
    let outlets: Vec<u32> = d.lakes.iter().map(|l| l.outlet).collect();
    let (rivers, mouths) = rivers::trace(
        &rivers::Inputs {
            grid,
            cell,
            x0,
            sea: &sea,
            lake_of: &d.lake_of,
            outlets: &outlets,
            receiver: &d.receiver,
            discharge: &q,
            surface: &d.surface,
        },
        p,
    );
    HydrologyMap {
        grid,
        cell,
        x0,
        original,
        height: h.iter().map(|&v| v as f32).collect(),
        sea,
        surface: d.surface.iter().map(|&v| v as f32).collect(),
        lake_of: d.lake_of,
        receiver: d.receiver,
        discharge: q.iter().map(|&v| v as f32).collect(),
        lakes: d.lakes,
        rivers,
        mouths,
        uplift: uplift.iter().map(|&v| v as f32).collect(),
        erodibility: erodibility.iter().map(|&v| v as f32).collect(),
        rain: rain.iter().map(|&v| v as f32).collect(),
        spines,
        plates,
    }
}

const SECONDS_PER_YEAR: f64 = 365.25 * 86_400.0;

/// The open sea: the border, and the cells at or below sea level connected to it (8-connected).
fn open_sea(grid: Grid, h: &[f64]) -> Vec<bool> {
    let mut sea = vec![false; grid.len()];
    let mut todo = Vec::new();
    for (i, s) in sea.iter_mut().enumerate() {
        if grid.on_border(i) {
            *s = true;
            todo.push(i);
        }
    }
    while let Some(c) = todo.pop() {
        for (j, _) in grid.neighbours(c) {
            if !sea[j] && h[j] <= 0.0 {
                sea[j] = true;
                todo.push(j);
            }
        }
    }
    sea
}

impl HydrologyMap {
    /// A 64-bit FNV-1a digest of everything the map holds: equal digests on two targets mean
    /// bit-identical maps.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.u64(self.grid.n as u64);
        for v in self
            .original
            .iter()
            .chain(&self.height)
            .chain(&self.surface)
        {
            h.u64(u64::from(v.to_bits()));
        }
        for v in &self.discharge {
            h.u64(u64::from(v.to_bits()));
        }
        for (&l, &r) in self.lake_of.iter().zip(&self.receiver) {
            h.u64(u64::from(l) << 32 | u64::from(r));
        }
        for l in &self.lakes {
            for v in [l.level, l.area, l.max_depth, l.volume] {
                h.u64(v.to_bits());
            }
            h.u64(u64::from(l.outlet) << 32 | u64::from(l.entry));
        }
        for r in &self.rivers {
            h.u64(match r.end {
                End::Sea => 1,
                End::Lake(id) => 2 | u64::from(id) << 8,
                End::Joins => 3,
            });
            h.u64(r.from_lake.map_or(u64::MAX, u64::from));
            for v in &r.path {
                for x in [v.x, v.z, v.surface, v.discharge, v.width, v.depth] {
                    h.u64(x.to_bits());
                }
            }
        }
        for m in &self.mouths {
            for x in [m.x, m.z, m.discharge] {
                h.u64(x.to_bits());
            }
            h.u64(u64::from(m.river) << 1 | u64::from(m.kind == MouthKind::Delta));
        }
        h.0
    }

    /// Where the land drains into the sea: each land cell whose receiver is at sea, with its
    /// discharge (m³/s), largest first (ties by cell). Each is one river system's outlet.
    pub fn outlets(&self) -> Vec<(u32, f64)> {
        let mut out: Vec<(u32, f64)> = (0..self.grid.len())
            .filter(|&i| !self.sea[i] && self.sea[self.receiver[i] as usize])
            .map(|i| (i as u32, f64::from(self.discharge[i])))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The main stem upstream of a cell (an outlet, usually): from it, the donor with the most
    /// discharge each step, through lakes, to a source. Its cells source first, its length (m,
    /// cell centre to cell centre, plus the half cell to the sea when it ends there) and the lakes
    /// it passes through, in flow order.
    pub fn main_stem(&self, from: u32) -> Stem {
        let len = self.grid.len();
        let mut best = vec![NONE; len];
        for c in 0..len {
            if self.sea[c] {
                continue;
            }
            let r = self.receiver[c] as usize;
            if self.sea[r] {
                continue;
            }
            let b = best[r];
            if b == NONE || self.discharge[c] > self.discharge[b as usize] {
                best[r] = c as u32;
            }
        }
        let mut cells = vec![from];
        let mut c = from;
        while best[c as usize] != NONE {
            c = best[c as usize];
            cells.push(c);
        }
        cells.reverse();
        let mut length = 0.0;
        for w in cells.windows(2) {
            let (a, b) = (self.position(w[0] as usize), self.position(w[1] as usize));
            length += ((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1)).sqrt();
        }
        if self.sea[self.receiver[from as usize] as usize] {
            length += 0.5 * self.cell;
        }
        let mut lakes = Vec::new();
        for &c in &cells {
            let l = self.lake_of[c as usize];
            if l != NONE && lakes.last() != Some(&l) {
                lakes.push(l);
            }
        }
        Stem {
            cells,
            length,
            lakes,
        }
    }

    /// The centre of cell `i` (face-local m).
    pub fn position(&self, i: usize) -> (f64, f64) {
        let n = self.grid.n;
        (
            self.x0 + (i % n) as f64 * self.cell,
            self.x0 + (i / n) as f64 * self.cell,
        )
    }
}

struct Fnv(u64);
impl Fnv {
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}
