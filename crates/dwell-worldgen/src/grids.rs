//! A world's voxel grids (docs/proposals/MOON_WORLD.md, slice M-1): every buildable two-faced disc
//! a world has, as data (`shared/worldgen/grids.json`). The home world is the grid at rest; the
//! moon is a second grid with its own frame, radius, seed and generator kind. Each grid's terrain is
//! a pure function of (world seed, grid, face, column), face-local (+y up from the grid's
//! midplane), so the same generator serves the far-LOD bake now and streamed chunks later.
//!
//! The `cratered` generator ([`CrateredTerrain`]): cratered plains, ridges and maria, a crater
//! field of size classes, and [`CrateredTerrain::bake_far`], the far-LOD normal/height and albedo
//! maps the sky's moon will draw (phase M-2).

use std::sync::{Arc, OnceLock};

use dwell_blocks::StateId;
use dwell_world::{CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, FaceFrame, M, local_index};
use serde_json::Value;

use crate::bifacial::face_seed;
use crate::noise::{fbm2, hash2, seed_word, smooth_step, unit};

/// The build's grids file.
pub const GRIDS_JSON: &str = include_str!("../../../shared/worldgen/grids.json");

/// Where a grid sits and how it moves (MOON_WORLD.md §3): the home world `Rest`s (the frame the
/// others are placed against); `Orbit` follows the sky's moon path (docs/DAY_NIGHT.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridFrameKind {
    Rest,
    Orbit,
}

/// A grid's generator, by kind.
#[derive(Clone, Debug, PartialEq)]
pub enum GridGenerator {
    /// Today's home terrain (`TerrainGenerator`, both faces by `BifacialTerrain`); its far bake
    /// (`crate::home_far`) is `bake_size` texels a side per face, from the world map.
    Terrain { bake_size: usize },
    /// The moon rules ([`CraterRules`]).
    Cratered(Arc<CraterRules>),
}

/// One grid of `grids.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridDef {
    /// Namespaced, `dwell:moon`.
    pub name: String,
    pub frame: GridFrameKind,
    /// The disc's radius (m); `None`: the world's own size (`WorldBounds::radius`).
    pub radius_m: Option<f64>,
    /// Mixed into the world seed for the grid's seed; `None` keeps the world seed (the home grid).
    pub seed_salt: Option<u32>,
    /// Its surface gravity, × the home world's (the moon's about ⅙).
    pub gravity_scale: f64,
    /// Inside this distance (m) of the grid's centre a flying player is captured into its frame
    /// (MOON_WORLD.md §6; `None` for the grid at rest, which holds everyone not captured).
    pub capture_m: Option<f64>,
    /// Past this distance (m) from the viewer the grid is drawn as a far body (its baked maps,
    /// MOON_WORLD.md §5), nearer as LOD terrain.
    pub far_m: f64,
    /// How high its ground reaches over its midplane (m, face-local): the slab far flight measures
    /// its distance from (MOON_WORLD.md §7).
    pub surface_m: f64,
    pub generator: GridGenerator,
}

impl GridDef {
    /// The grid's seed in a world: the world seed for the home grid, else derived from it, so
    /// every world has its own moon.
    pub fn seed(&self, world_seed: u64) -> u64 {
        match self.seed_salt {
            None => world_seed,
            Some(salt) => {
                let lo = u64::from(seed_word(world_seed, salt));
                let hi = u64::from(seed_word(world_seed, salt ^ 0x6A09_E667));
                (hi << 32) | lo
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseRule {
    pub amplitude_m: f64,
    pub wavelength_m: f64,
    pub octaves: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RidgeRule {
    pub noise: NoiseRule,
    /// The ridge's crest sharpness (the ridged noise's power).
    pub sharpness: f64,
    /// The ridges' height inside a mare, × (wrinkle ridges are lower).
    pub in_maria: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MariaRule {
    /// A jittered grid of this cell (m), one candidate basin a cell.
    pub cell_m: f64,
    /// The share of cells that hold a mare.
    pub chance: f64,
    pub radius_m: [f64; 2],
    /// The basin floor's depth under the datum (m).
    pub depth_m: f64,
    /// The width of the basin's soft edge (m), inside its radius.
    pub edge_m: f64,
    /// The plains noise inside a mare, ×.
    pub flatten: f64,
    /// The share of craters kept inside a mare.
    pub crater_density: f64,
    /// The mare's albedo, × the block's colour.
    pub albedo: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CraterClass {
    pub radius_m: [f64; 2],
    pub per_km2: f64,
}

impl CraterClass {
    /// The class's jittered grid cell (m): one crater a cell gives `per_km2`.
    pub fn cell_m(&self) -> f64 {
        (1.0e6 / self.per_km2).sqrt()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CraterField {
    pub reach: f64,
    pub depth_ratio: f64,
    pub max_depth_m: f64,
    pub rim_ratio: f64,
    pub max_rim_m: f64,
    pub floor_fraction: f64,
    pub classes: Vec<CraterClass>,
}

/// The `cratered` generator's rules.
#[derive(Clone, Debug, PartialEq)]
pub struct CraterRules {
    /// The ground's block, by name.
    pub block: String,
    pub datum_m: f64,
    /// The lowest the ground goes (face-local m): overlapping craters stop there.
    pub min_m: f64,
    pub plains: NoiseRule,
    pub ridges: RidgeRule,
    pub maria: MariaRule,
    pub craters: CraterField,
    /// The far-LOD map's texels a side, per face.
    pub bake_size: usize,
}

fn get<'a>(v: &'a Value, path: &str, at: &str) -> Result<&'a Value, String> {
    let mut x = v;
    for k in path.split('.') {
        x = x
            .get(k)
            .ok_or_else(|| format!("{at}: `{path}` is missing"))?;
    }
    Ok(x)
}

fn num(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    get(v, path, at)?
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{at}: `{path}` must be a number"))
}

fn positive(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    let n = num(v, path, at)?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err(format!("{at}: `{path}` must be > 0 (is {n})"))
    }
}

fn share(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    let n = num(v, path, at)?;
    if (0.0..=1.0).contains(&n) {
        Ok(n)
    } else {
        Err(format!("{at}: `{path}` must be in 0..=1 (is {n})"))
    }
}

fn range(v: &Value, path: &str, at: &str) -> Result<[f64; 2], String> {
    let a = get(v, path, at)?
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| format!("{at}: `{path}` must be [min, max]"))?;
    let lo = a[0].as_f64().unwrap_or(f64::NAN);
    let hi = a[1].as_f64().unwrap_or(f64::NAN);
    if lo > 0.0 && hi >= lo {
        Ok([lo, hi])
    } else {
        Err(format!(
            "{at}: `{path}` must be [min, max] with 0 < min <= max"
        ))
    }
}

fn noise_rule(v: &Value, at: &str) -> Result<NoiseRule, String> {
    let octaves = num(v, "octaves", at)?;
    if !(1.0..=8.0).contains(&octaves) || octaves.fract() != 0.0 {
        return Err(format!("{at}: `octaves` must be a whole number in 1..=8"));
    }
    Ok(NoiseRule {
        amplitude_m: num(v, "amplitudeM", at)?.max(0.0),
        wavelength_m: positive(v, "wavelengthM", at)?,
        octaves: octaves as u32,
    })
}

fn crater_rules(g: &Value, at: &str) -> Result<CraterRules, String> {
    let block = get(g, "block", at)?
        .as_str()
        .ok_or_else(|| format!("{at}: `block` must be a block name"))?;
    if dwell_blocks::state(block).is_none() {
        return Err(format!("{at}: `block` names no block (`{block}`)"));
    }
    let r = get(g, "ridges", at)?;
    let m = get(g, "maria", at)?;
    let c = get(g, "craters", at)?;
    let classes = get(c, "classes", at)?
        .as_array()
        .ok_or_else(|| format!("{at}: `craters.classes` must be a list"))?
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let at = format!("{at} craters.classes[{i}]");
            Ok(CraterClass {
                radius_m: range(k, "radiusM", &at)?,
                per_km2: positive(k, "perKm2", &at)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let reach = num(c, "reach", at)?;
    if reach <= 1.0 {
        return Err(format!(
            "{at}: `craters.reach` must be > 1 (the rim is at 1)"
        ));
    }
    let maria = MariaRule {
        cell_m: positive(m, "cellM", at)?,
        chance: share(m, "chance", at)?,
        radius_m: range(m, "radiusM", at)?,
        depth_m: num(m, "depthM", at)?,
        edge_m: positive(m, "edgeM", at)?,
        flatten: share(m, "flatten", at)?,
        crater_density: share(m, "craterDensity", at)?,
        albedo: share(m, "albedo", at)?,
    };
    if maria.radius_m[1] >= maria.cell_m {
        return Err(format!(
            "{at}: `maria.radiusM` must stay under `maria.cellM`"
        ));
    }
    if maria.edge_m > maria.radius_m[0] {
        return Err(format!(
            "{at}: `maria.edgeM` must be at most the smallest radius"
        ));
    }
    let bake = bake_size(g, at)?;
    Ok(CraterRules {
        block: block.to_owned(),
        datum_m: num(g, "datumM", at)?,
        min_m: {
            let m = positive(g, "minM", at)?;
            if m >= num(g, "datumM", at)? {
                return Err(format!("{at}: `minM` must be under `datumM`"));
            }
            m
        },
        plains: noise_rule(get(g, "plains", at)?, &format!("{at} plains"))?,
        ridges: RidgeRule {
            noise: noise_rule(r, &format!("{at} ridges"))?,
            sharpness: positive(r, "sharpness", at)?,
            in_maria: share(r, "inMaria", at)?,
        },
        maria,
        craters: CraterField {
            reach,
            depth_ratio: positive(c, "depthRatio", at)?,
            max_depth_m: positive(c, "maxDepthM", at)?,
            rim_ratio: positive(c, "rimRatio", at)?,
            max_rim_m: positive(c, "maxRimM", at)?,
            floor_fraction: {
                let f = share(c, "floorFraction", at)?;
                if f >= 1.0 {
                    return Err(format!("{at}: `craters.floorFraction` must be < 1"));
                }
                f
            },
            classes,
        },
        bake_size: bake,
    })
}

/// A generator's `bake.size`: the far-LOD map's texels a side per face.
fn bake_size(g: &Value, at: &str) -> Result<usize, String> {
    let bake = num(g, "bake.size", at)?;
    if !(8.0..=2048.0).contains(&bake) || bake.fract() != 0.0 {
        return Err(format!(
            "{at}: `bake.size` must be a whole number in 8..=2048"
        ));
    }
    Ok(bake as usize)
}

/// Parses and validates a grids file: a bad entry fails with a message naming it.
pub fn parse_grids(text: &str) -> Result<Vec<GridDef>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("grids: {e}"))?;
    let list = v
        .get("grids")
        .and_then(Value::as_array)
        .ok_or("grids: `grids` must be a list")?;
    let mut out: Vec<GridDef> = Vec::new();
    for (i, g) in list.iter().enumerate() {
        let name = g
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| n.contains(':'))
            .ok_or_else(|| format!("grids[{i}]: `name` must be a namespaced name"))?;
        let at = format!("grid `{name}`");
        if out.iter().any(|d| d.name == name) {
            return Err(format!("{at}: named twice"));
        }
        let frame = match get(g, "frame", &at)?.as_str() {
            Some("rest") => GridFrameKind::Rest,
            Some("orbit") => GridFrameKind::Orbit,
            _ => return Err(format!("{at}: `frame` must be \"rest\" or \"orbit\"")),
        };
        let radius_m = match get(g, "radiusM", &at)? {
            Value::String(s) if s == "world" => None,
            _ => Some(positive(g, "radiusM", &at)?),
        };
        let seed_salt = match g.get("seedSalt") {
            None => None,
            Some(s) => Some(
                s.as_u64()
                    .and_then(|s| u32::try_from(s).ok())
                    .ok_or_else(|| format!("{at}: `seedSalt` must be a u32"))?,
            ),
        };
        let gen_ = get(g, "generator", &at)?;
        let generator = match gen_.get("kind").and_then(Value::as_str) {
            Some("terrain") => GridGenerator::Terrain {
                bake_size: bake_size(gen_, &at)?,
            },
            Some("cratered") => GridGenerator::Cratered(Arc::new(crater_rules(gen_, &at)?)),
            _ => {
                return Err(format!(
                    "{at}: `generator.kind` must be \"terrain\" or \"cratered\""
                ));
            }
        };
        let gravity_scale = positive(g, "gravityScale", &at)?;
        let capture_m = match g.get("captureM") {
            None => None,
            Some(_) => Some(positive(g, "captureM", &at)?),
        };
        let far_m = positive(g, "farM", &at)?;
        let surface_m = positive(g, "surfaceM", &at)?;
        if frame == GridFrameKind::Rest && capture_m.is_some() {
            return Err(format!(
                "{at}: the grid at rest captures nobody (no `captureM`)"
            ));
        }
        if frame != GridFrameKind::Rest && seed_salt.is_none() {
            return Err(format!("{at}: a moving grid needs its own `seedSalt`"));
        }
        if frame != GridFrameKind::Rest && capture_m.is_none() {
            return Err(format!("{at}: a moving grid needs a `captureM`"));
        }
        if let (Some(c), Some(r)) = (capture_m, radius_m)
            && c <= r
        {
            return Err(format!(
                "{at}: `captureM` must reach past the disc's radius"
            ));
        }
        if frame == GridFrameKind::Rest && seed_salt.is_some() {
            return Err(format!(
                "{at}: the grid at rest keeps the world seed (no `seedSalt`)"
            ));
        }
        out.push(GridDef {
            name: name.to_owned(),
            frame,
            radius_m,
            seed_salt,
            gravity_scale,
            capture_m,
            far_m,
            surface_m,
            generator,
        });
    }
    if out
        .iter()
        .filter(|d| d.frame == GridFrameKind::Rest)
        .count()
        != 1
    {
        return Err("grids: exactly one grid must be at rest (the home world)".into());
    }
    let mut salts: Vec<u32> = out.iter().filter_map(|d| d.seed_salt).collect();
    salts.sort_unstable();
    if salts.windows(2).any(|w| w[0] == w[1]) {
        return Err("grids: two grids share a `seedSalt` (their terrain would be one)".into());
    }
    Ok(out)
}

/// The build's grids (`grids.json`); a bad file panics with its message (CI's tests parse it).
pub fn grids() -> &'static [GridDef] {
    static GRIDS: OnceLock<Vec<GridDef>> = OnceLock::new();
    GRIDS.get_or_init(|| parse_grids(GRIDS_JSON).unwrap_or_else(|e| panic!("{e}")))
}

/// The build's grid named `name` (`dwell:moon`).
pub fn grid(name: &str) -> Option<&'static GridDef> {
    grids().iter().find(|g| g.name == name)
}

/// Travel between the grids (`grids.json` `travel`, MOON_WORLD.md §7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TravelRules {
    /// Far flight starts this far (m) from the nearest grid's slab.
    pub far_above_m: f64,
    /// Far flight's speed per metre of that distance (1/s): `v = perS × d`.
    pub far_per_s: f64,
    /// Far flight's top speed (m/s).
    pub far_max_mps: f64,
    /// A captured player is released past `captureM` × this (> 1: hysteresis).
    pub release_factor: f64,
}

impl TravelRules {
    /// The far-flight speed at `d` m from the nearest grid's slab: 0 nearer than `aboveM` (the
    /// slider's speed alone), else `perS × d` up to the top.
    pub fn far_speed(&self, d: f64) -> f64 {
        if d <= self.far_above_m {
            0.0
        } else {
            (self.far_per_s * d).min(self.far_max_mps)
        }
    }
}

/// Parses and validates `grids.json`'s `travel` block.
pub fn parse_travel(text: &str) -> Result<TravelRules, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("grids: {e}"))?;
    let t = v.get("travel").ok_or("grids: no `travel` block")?;
    let at = "grids `travel`";
    let far = get(t, "farFlight", at)?;
    let at_far = "grids `travel.farFlight`";
    let rules = TravelRules {
        far_above_m: positive(far, "aboveM", at_far)?,
        far_per_s: positive(far, "perS", at_far)?,
        far_max_mps: positive(far, "maxMps", at_far)?,
        release_factor: positive(t, "releaseFactor", at)?,
    };
    if rules.release_factor <= 1.0 {
        return Err(format!("{at}: `releaseFactor` must be over 1 (hysteresis)"));
    }
    Ok(rules)
}

/// The build's travel rules (`grids.json` `travel`); a bad file panics with its message.
pub fn travel() -> TravelRules {
    static T: OnceLock<TravelRules> = OnceLock::new();
    *T.get_or_init(|| parse_travel(GRIDS_JSON).unwrap_or_else(|e| panic!("{e}")))
}

/// One crater of the field, face-local (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crater {
    pub x: f64,
    pub z: f64,
    pub radius: f64,
    /// The floor under the surround (m).
    pub depth: f64,
    /// The rim over the surround (m).
    pub rim: f64,
    /// Its index in `craters.classes`.
    pub class: usize,
}

impl Crater {
    /// The crater's height offset at `d` m from its centre: the flat floor, the bowl up to the rim
    /// at the radius, the ejecta falling off to nothing at `reach` × the radius.
    pub fn profile(&self, d: f64, field: &CraterField) -> f64 {
        let x = d / self.radius;
        if x >= field.reach {
            return 0.0;
        }
        if x < 1.0 {
            let f = field.floor_fraction;
            let t = ((x - f) / (1.0 - f)).max(0.0);
            return -self.depth + (self.depth + self.rim) * t * t;
        }
        let tail = field.reach.powi(-3);
        self.rim * (x.powi(-3) - tail) / (1.0 - tail)
    }
}

/// One mare, face-local (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mare {
    pub x: f64,
    pub z: f64,
    pub radius: f64,
}

/// A grid's far-LOD maps for one face (MOON_WORLD.md §5): `size` × `size` texels over the disc's
/// square, row-major from (−r, −r), +x along a row and +z down the rows, face-local.
#[derive(Clone, Debug, PartialEq)]
pub struct FarBake {
    pub size: usize,
    pub radius_m: f64,
    /// RGBA: the face-local normal (R = x, G = z, B = up; each `(n + 1) / 2 × 255`), A the height
    /// over `height_range` (0 at its low end, 255 at its high); outside the disc straight up, A 0.
    pub normal_height: Vec<u8>,
    /// RGBA: the ground's colour × its albedo (maria darker), A 255 inside the disc, 0 outside.
    /// Under water it is the ground under the water (the bed): the renderer lays the water's own
    /// look over it by `water`.
    pub albedo: Vec<u8>,
    /// One byte a texel: the share of it under water (0 dry, 255 all water; 0 outside the disc),
    /// for the far body's sea colour, specular glint and reflection (MOON_WORLD.md §5). All 0 on a
    /// grid without water (the moon).
    pub water: Vec<u8>,
    /// The heights (m, face-local) A spans.
    pub height_range: [f64; 2],
}

impl FarBake {
    /// The decoded face-local normal of texel (i, j).
    pub fn normal(&self, i: usize, j: usize) -> [f64; 3] {
        let p = &self.normal_height[(j * self.size + i) * 4..];
        let d = |b: u8| f64::from(b) / 255.0 * 2.0 - 1.0;
        [d(p[0]), d(p[2]), d(p[1])]
    }
    /// The world (face-local) x or z of texel `i`'s centre.
    pub fn texel_centre(&self, i: usize) -> f64 {
        -self.radius_m + (i as f64 + 0.5) * self.texel_m()
    }
    pub fn texel_m(&self) -> f64 {
        2.0 * self.radius_m / self.size as f64
    }
    /// The bytes it holds (all three maps).
    pub fn bytes(&self) -> usize {
        self.normal_height.len() + self.albedo.len() + self.water.len()
    }
}

/// What a grid gives its far-body bake (MOON_WORLD.md §5): any grid's generator implements it,
/// the moon's now ([`CrateredTerrain`]) and the home world's later (from its world map), so one
/// bake and one far-body renderer serve every grid.
pub trait FarSource {
    /// The disc's radius (m).
    fn far_radius_m(&self) -> f64;
    /// The ground's height (face-local m) at (x, z), with the detail at least `texel_m` wide.
    fn far_height(&self, face: Face, x: f64, z: f64, texel_m: f64) -> f64;
    /// The ground's colour at (x, z), sRGB 0..=1, the texel `texel_m` wide.
    fn far_albedo(&self, face: Face, x: f64, z: f64) -> [f64; 3];
    /// The share (0..=1) of the texel `texel_m` wide centred on (x, z) that is under water. None by
    /// default (a dry grid).
    fn far_water(&self, _face: Face, _x: f64, _z: f64, _texel_m: f64) -> f64 {
        0.0
    }
}

/// A grid's far-LOD maps for one face at `size` texels a side: heights at the texel centres
/// with the detail at least a texel wide, normals by central differences, the albedo and the water
/// share per texel.
pub fn bake_far<S: FarSource + ?Sized>(src: &S, face: Face, size: usize) -> FarBake {
    let size = size.max(2);
    let r = src.far_radius_m();
    let s = 2.0 * r / size as f64;
    let centre = |i: usize| -r + (i as f64 + 0.5) * s;
    let inside = |i: usize, j: usize| centre(i).hypot(centre(j)) <= r;
    let mut h = vec![0.0f64; size * size];
    for j in 0..size {
        for i in 0..size {
            h[j * size + i] = src.far_height(face, centre(i), centre(j), s);
        }
    }
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for j in 0..size {
        for i in 0..size {
            if inside(i, j) {
                lo = lo.min(h[j * size + i]);
                hi = hi.max(h[j * size + i]);
            }
        }
    }
    if lo > hi {
        (lo, hi) = (0.0, 0.0);
    }
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut normal_height = vec![0u8; size * size * 4];
    let mut albedo = vec![0u8; size * size * 4];
    let mut water = vec![0u8; size * size];
    let at = |i: usize, j: usize| h[j * size + i];
    for j in 0..size {
        for i in 0..size {
            let p = (j * size + i) * 4;
            if !inside(i, j) {
                normal_height[p..p + 4].copy_from_slice(&[128, 128, 255, 0]);
                continue;
            }
            let (i0, i1) = (i.saturating_sub(1), (i + 1).min(size - 1));
            let (j0, j1) = (j.saturating_sub(1), (j + 1).min(size - 1));
            let dx = (at(i1, j) - at(i0, j)) / ((i1 - i0) as f64 * s);
            let dz = (at(i, j1) - at(i, j0)) / ((j1 - j0) as f64 * s);
            let len = (dx * dx + dz * dz + 1.0).sqrt();
            let n = [-dx / len, -dz / len, 1.0 / len];
            let a = if hi > lo {
                (at(i, j) - lo) / (hi - lo)
            } else {
                0.0
            };
            normal_height[p..p + 4].copy_from_slice(&[
                byte(n[0] * 0.5 + 0.5),
                byte(n[1] * 0.5 + 0.5),
                byte(n[2] * 0.5 + 0.5),
                byte(a),
            ]);
            let c = src.far_albedo(face, centre(i), centre(j));
            albedo[p..p + 4].copy_from_slice(&[byte(c[0]), byte(c[1]), byte(c[2]), 255]);
            water[j * size + i] = byte(src.far_water(face, centre(i), centre(j), s));
        }
    }
    FarBake {
        size,
        radius_m: r,
        normal_height,
        albedo,
        water,
        height_range: [lo, hi],
    }
}

/// The `cratered` generator over one grid of one world, both faces (each from its own seed
/// stream, as the home world's faces are).
#[derive(Clone, Debug)]
pub struct CrateredTerrain {
    rules: Arc<CraterRules>,
    radius: f64,
    seeds: [u64; 2],
    block: StateId,
}

const PLAINS: u32 = 0x4D_0001;
const RIDGES: u32 = 0x4D_0002;
const MARIA: u32 = 0x4D_0003;
const CRATERS: u32 = 0x4D_0100;

fn fi(f: Face) -> usize {
    match f {
        Face::A => 0,
        Face::B => 1,
    }
}

impl CrateredTerrain {
    /// The terrain of a `cratered` grid in the world of `world_seed`; `None` for another kind or a
    /// grid sized by the world.
    pub fn new(def: &GridDef, world_seed: u64) -> Option<Self> {
        let GridGenerator::Cratered(rules) = &def.generator else {
            return None;
        };
        let seed = def.seed(world_seed);
        Some(Self {
            rules: rules.clone(),
            radius: def.radius_m?,
            seeds: [face_seed(seed, Face::A), face_seed(seed, Face::B)],
            block: dwell_blocks::require(&rules.block),
        })
    }

    /// The build's grid `name` in the world of `world_seed` (`dwell:moon`).
    pub fn for_grid(name: &str, world_seed: u64) -> Option<Self> {
        Self::new(grid(name)?, world_seed)
    }

    pub fn rules(&self) -> &CraterRules {
        &self.rules
    }
    pub fn radius_m(&self) -> f64 {
        self.radius
    }
    pub fn inside(&self, x: f64, z: f64) -> bool {
        x * x + z * z <= self.radius * self.radius
    }

    fn stream(&self, face: Face, stream: u32) -> u32 {
        seed_word(self.seeds[fi(face)], stream)
    }

    /// The maria whose basins reach within `within` m of (x, z).
    pub fn maria_near(&self, face: Face, x: f64, z: f64, within: f64) -> Vec<Mare> {
        let m = &self.rules.maria;
        let seed = self.stream(face, MARIA);
        let span = ((m.radius_m[1] + within) / m.cell_m).ceil() as i32 + 1;
        let (cx, cz) = ((x / m.cell_m).floor() as i32, (z / m.cell_m).floor() as i32);
        let mut out = Vec::new();
        for gz in cz - span..=cz + span {
            for gx in cx - span..=cx + span {
                let h = hash2(seed, gx, gz);
                if f64::from(unit(h)) >= m.chance {
                    continue;
                }
                let j = |k: u32| f64::from(unit(hash2(seed ^ k, gx, gz)));
                let mare = Mare {
                    x: (f64::from(gx) + 0.25 + 0.5 * j(0x11)) * m.cell_m,
                    z: (f64::from(gz) + 0.25 + 0.5 * j(0x22)) * m.cell_m,
                    radius: m.radius_m[0] + (m.radius_m[1] - m.radius_m[0]) * j(0x33),
                };
                if (mare.x - x).hypot(mare.z - z) <= mare.radius + within {
                    out.push(mare);
                }
            }
        }
        out
    }

    /// How much of a mare (x, z) is in: 1 on a basin's floor, easing to 0 across its edge.
    pub fn mare_weight(&self, face: Face, x: f64, z: f64) -> f64 {
        let edge = self.rules.maria.edge_m;
        self.maria_near(face, x, z, 0.0)
            .iter()
            .map(|m| {
                let d = (m.x - x).hypot(m.z - z) as f32;
                1.0 - f64::from(smooth_step((m.radius - edge) as f32, m.radius as f32, d))
            })
            .fold(0.0, f64::max)
    }

    /// The craters at least `min_radius` m wide whose reach comes within `within` m of (x, z).
    pub fn craters_near(
        &self,
        face: Face,
        x: f64,
        z: f64,
        within: f64,
        min_radius: f64,
    ) -> Vec<Crater> {
        let f = &self.rules.craters;
        let mut out = Vec::new();
        for (k, class) in f.classes.iter().enumerate() {
            if class.radius_m[1] < min_radius {
                continue;
            }
            let cell = class.cell_m();
            let seed = self.stream(face, CRATERS + k as u32);
            let span = ((class.radius_m[1] * f.reach + within) / cell).ceil() as i32;
            let (cx, cz) = ((x / cell).floor() as i32, (z / cell).floor() as i32);
            for gz in cz - span..=cz + span {
                for gx in cx - span..=cx + span {
                    let j = |s: u32| f64::from(unit(hash2(seed ^ s, gx, gz)));
                    let u = j(0x33);
                    // Smaller craters are commoner inside a class too (u² toward its small end).
                    let radius =
                        class.radius_m[0] + (class.radius_m[1] - class.radius_m[0]) * u * u;
                    if radius < min_radius {
                        continue;
                    }
                    let c = Crater {
                        x: (f64::from(gx) + j(0x11)) * cell,
                        z: (f64::from(gz) + j(0x22)) * cell,
                        radius,
                        depth: (radius * f.depth_ratio).min(f.max_depth_m),
                        rim: (radius * f.rim_ratio).min(f.max_rim_m),
                        class: k,
                    };
                    if (c.x - x).hypot(c.z - z) > radius * f.reach + within {
                        continue;
                    }
                    // A mare's younger floor keeps only some of the craters.
                    let w = self.mare_weight(face, c.x, c.z);
                    let keep = 1.0 - w * (1.0 - self.rules.maria.crater_density);
                    if j(0x44) >= keep {
                        continue;
                    }
                    out.push(c);
                }
            }
        }
        out
    }

    /// The ground's height (face-local m) at (x, z) without the craters: the datum, the plains,
    /// the ridges and the maria's basins.
    pub fn base_height(&self, face: Face, x: f64, z: f64) -> f64 {
        let r = &*self.rules;
        let w = self.mare_weight(face, x, z);
        let (xi, zi) = (x.floor() as i64, z.floor() as i64);
        let n = |s: u32, rule: &NoiseRule| {
            f64::from(fbm2(
                self.stream(face, s),
                xi,
                zi,
                rule.wavelength_m as i32,
                rule.octaves,
                None,
            ))
        };
        let plains = n(PLAINS, &r.plains) * r.plains.amplitude_m;
        let ridge = (1.0 - n(RIDGES, &r.ridges.noise).abs())
            .clamp(0.0, 1.0)
            .powf(r.ridges.sharpness)
            * r.ridges.noise.amplitude_m;
        let lerp = |a: f64, b: f64| a + (b - a) * w;
        r.datum_m + plains * lerp(1.0, r.maria.flatten) + ridge * lerp(1.0, r.ridges.in_maria)
            - w * r.maria.depth_m
    }

    /// The ground's height (face-local m) at (x, z) with every crater at least `min_radius` m
    /// wide (the bake leaves out the ones smaller than its texel, which would only alias).
    pub fn height_at_scale(&self, face: Face, x: f64, z: f64, min_radius: f64) -> f64 {
        let f = &self.rules.craters;
        let h = self.base_height(face, x, z)
            + self
                .craters_near(face, x, z, 0.0, min_radius)
                .iter()
                .map(|c| c.profile((c.x - x).hypot(c.z - z), f))
                .sum::<f64>();
        h.max(self.rules.min_m)
    }

    /// The ground's height (face-local m) at (x, z): what a player would stand on.
    pub fn height(&self, face: Face, x: f64, z: f64) -> f64 {
        self.height_at_scale(face, x, z, 0.0)
    }

    /// The face-local row of the column's top cell (`None` outside the disc).
    pub fn top_row(&self, face: Face, x: i64, z: i64) -> Option<i64> {
        let (cx, cz) = (x as f64 + 0.5, z as f64 + 0.5);
        self.inside(cx, cz)
            .then(|| (self.height(face, cx, cz) / CELL_H).round() as i64 - 1)
    }

    /// A spawn column on `face` (MOON_WORLD.md §7): the first point of a spiral out from the
    /// disc's centre whose ground is flat (heights within 1 m over 4 m round it) and clear of every
    /// crater wall (not within 1.2 radii of a crater 10 m or wider). Face-local (x, top of the
    /// ground, z), deterministic per seed; the centre if nothing is found.
    pub fn spawn_point(&self, face: Face) -> [f64; 3] {
        let ok = |x: f64, z: f64| {
            if !self.inside(x, z) {
                return false;
            }
            let h = self.height(face, x, z);
            let flat = [(4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0)]
                .iter()
                .all(|(dx, dz)| (self.height(face, x + dx, z + dz) - h).abs() <= 1.0);
            flat && self
                .craters_near(face, x, z, 0.0, 10.0)
                .iter()
                .all(|c| (c.x - x).hypot(c.z - z) > 1.2 * c.radius)
        };
        let column = |x: f64, z: f64| {
            let top = self
                .top_row(face, x.floor() as i64, z.floor() as i64)
                .unwrap_or(0);
            [x.floor() + 0.5, (top + 1) as f64 * CELL_H, z.floor() + 0.5]
        };
        for k in 0..2000u32 {
            // A golden-angle spiral, 6 m apart.
            let r = 6.0 * f64::from(k).sqrt() * 2.0;
            let th = f64::from(k) * 2.399_963_229_728_653;
            let (x, z) = (r * th.cos(), r * th.sin());
            if ok(x, z) {
                return column(x, z);
            }
        }
        column(0.0, 0.0)
    }

    /// A chunk of the grid (in the grid's own coordinates): the ground's block from the midplane
    /// up to the surface, both faces through `FaceFrame` (face B's chunks are face-local ones
    /// flipped, as the home world's are).
    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let f = FaceFrame::at_chunk_y(coord.y);
        let local = f.chunk(coord);
        let cells = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let wx = i64::from(local.x * S + x);
                let wz = i64::from(local.z * S + z);
                let top = self.top_row(f.face, wx, wz);
                for y in 0..S {
                    let row = i64::from(local.y * S + y);
                    let solid = top.is_some_and(|t| row >= 0 && row <= t);
                    let ly = if f.is_b() { S - 1 - y } else { y };
                    cells[local_index(x, ly, z)] = if solid { f.state(self.block) } else { M.air };
                }
            }
        }
    }

    /// The far-LOD maps of one face (MOON_WORLD.md §5) at `size` texels a side ([`bake_far`]).
    pub fn bake_far(&self, face: Face, size: usize) -> FarBake {
        bake_far(self, face, size)
    }

    /// [`CrateredTerrain::bake_far`] at the grid's data size (`bake.size`).
    pub fn bake_far_default(&self, face: Face) -> FarBake {
        self.bake_far(face, self.rules.bake_size)
    }
}

impl FarSource for CrateredTerrain {
    fn far_radius_m(&self) -> f64 {
        self.radius
    }
    fn far_height(&self, face: Face, x: f64, z: f64, texel_m: f64) -> f64 {
        self.height_at_scale(face, x, z, texel_m)
    }
    fn far_albedo(&self, face: Face, x: f64, z: f64) -> [f64; 3] {
        let c = dwell_blocks::info(self.block).color;
        let k = 1.0 - self.mare_weight(face, x, z) * (1.0 - self.rules.maria.albedo);
        [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0 * k)
    }
}
