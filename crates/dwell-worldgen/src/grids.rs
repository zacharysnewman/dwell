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
use crate::ramps::{RampRule, Top};
use dwell_world::ramps::RampFacing;
use dwell_world::rows_for_m;

/// The build's grids file.
pub const GRIDS_JSON: &str = include_str!("../../../shared/worldgen/grids.json");

/// Where a grid sits and how it moves (MOON_WORLD.md §3): the home world `Rest`s (the frame the
/// others are placed against); `Orbit` follows the sky's moon path (docs/DAY_NIGHT.md); `SunPath`
/// sits on the sky's sun path (`sun.distanceM` out toward today's sun direction, its face A toward
/// home; docs/proposals/SUN_WORLD.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridFrameKind {
    Rest,
    Orbit,
    SunPath,
}

/// A grid's generator, by kind.
#[derive(Clone, Debug, PartialEq)]
pub enum GridGenerator {
    /// Today's home terrain (`TerrainGenerator`, both faces by `BifacialTerrain`); its far bake
    /// (`crate::home_far`) is `bake_size` texels a side per face, from the world map.
    Terrain { bake_size: usize },
    /// The moon rules ([`CraterRules`]).
    Cratered(Arc<CraterRules>),
    /// The sun's rules ([`crate::volcanic::VolcanicRules`], docs/proposals/SUN_WORLD.md).
    Volcanic(Arc<crate::volcanic::VolcanicRules>),
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
    /// Whether the grid has air (`atmosphere`, false when not given): the day sky is a layer over
    /// space that thins with height (docs/DAY_NIGHT.md §15); without, a viewer on it sees space,
    /// the night sky's, at every hour (the moon, the sun).
    pub atmosphere: bool,
    /// The coarse mesh of its surface drawn nearer than `far_m` and under its chunks while on it
    /// (`nearMesh`, MOON_WORLD.md §5 M-5); `None` draws none.
    pub near_mesh: Option<NearMesh>,
    /// Whether the sky draws it as a far disc past `far_m` (`skyDisc`, true when not given: the
    /// moon's and the sun's discs); false (home) draws its `near_mesh` from another grid at every
    /// distance instead (MOON_WORLD.md §5, M-6).
    pub sky_disc: bool,
    /// Its terrain past the chunks while on it is drawn by an LOD tree of its own (`lod`,
    /// BUGS.md #143, #159): home's `LodSystem` with sections from its generator
    /// (`crate::grid_lod`); false draws none past the chunks. Home's own LOD is home's.
    pub lod: bool,
    /// What lights its ground while on it (`light`, BUGS.md #145); `None`: home's keyframes as
    /// they are (home, and a grid that declares none).
    pub light: Option<LightRig>,
    pub generator: GridGenerator,
    /// The capability a client must say before the server captures it into this grid
    /// (`shared/protocol/capabilities.json`; the sun's `grid.sun`, docs/proposals/SUN_WORLD.md §5):
    /// a client that doesn't flies on through, and the grid stays a far body for it. `None`: any
    /// `grid.frames` client and older ones (the moon's, as built).
    pub capability: Option<String>,
}

impl GridDef {
    /// Whether a client with the capabilities `has` may be captured into this grid.
    pub fn open_to(&self, has: impl Fn(&str) -> bool) -> bool {
        self.capability.as_deref().is_none_or(has)
    }
}

/// A grid's coarse surface mesh (`nearMesh`): `cells` quads across its disc's diameter, each
/// vertex the lowest ground round it less `sinkM`, so the full-detail chunks cover it where
/// they are and it shows where they are not (MOON_WORLD.md §5, M-5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearMesh {
    pub cells: usize,
    pub sink_m: f64,
}

/// A body that lights a grid by the sunlight it reflects (`light.bodies`): another grid by name,
/// its colour (`albedo`, sRGB) and its light (`intensity`, linear) when seen full at `refDeg` in
/// angular radius; the light at a moment is that × the share of its disc lit as the viewer sees it
/// (its phase) × (its angular radius / `refDeg`)², toward its true direction.
#[derive(Clone, Debug, PartialEq)]
pub struct LitBy {
    pub grid: String,
    pub albedo: [f64; 3],
    pub intensity: f64,
    pub ref_deg: f64,
}

/// A grid's light rig (`light`): whether the sun lights it (`sunlit`: its light and direction are
/// the sky's keyframes', turned into the grid's frame), the bodies that light it by their reflected
/// light (at most [`MAX_LIT_BY`]), the least ambient light its ground gets (`ambient`: an sRGB
/// colour × `min`, linear), and a glow from its own ground (`glow`: an sRGB colour × `intensity`,
/// lighting surfaces from below and around: the sun's lava).
#[derive(Clone, Debug, PartialEq)]
pub struct LightRig {
    pub sunlit: bool,
    pub bodies: Vec<LitBy>,
    pub ambient: [f64; 3],
    pub ambient_min: f64,
    pub glow: Option<([f64; 3], f64)>,
}

/// The most bodies a grid's light rig lists (the shaders' rig lights).
pub const MAX_LIT_BY: usize = 2;

/// `#rrggbb` as sRGB 0–1.
fn colour(v: &Value, path: &str, at: &str) -> Result<[f64; 3], String> {
    let s = get(v, path, at)?
        .as_str()
        .and_then(|s| s.strip_prefix('#'))
        .filter(|s| s.len() == 6)
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .ok_or_else(|| format!("{at}: `{path}` must be a colour \"#rrggbb\""))?;
    Ok([(s >> 16) & 0xff, (s >> 8) & 0xff, s & 0xff].map(|c| f64::from(c) / 255.0))
}

fn light_rig(g: &Value, at: &str) -> Result<Option<LightRig>, String> {
    let Some(l) = g.get("light") else {
        return Ok(None);
    };
    let sunlit = get(l, "sunlit", at)?
        .as_bool()
        .ok_or_else(|| format!("{at}: `light.sunlit` must be true or false"))?;
    let list = get(l, "bodies", at)?
        .as_array()
        .ok_or_else(|| format!("{at}: `light.bodies` must be a list"))?;
    if list.len() > MAX_LIT_BY {
        return Err(format!(
            "{at}: `light.bodies` lists at most {MAX_LIT_BY} bodies"
        ));
    }
    let bodies = list
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let at = format!("{at} light.bodies[{i}]");
            Ok(LitBy {
                grid: get(b, "grid", &at)?
                    .as_str()
                    .filter(|n| n.contains(':'))
                    .ok_or_else(|| format!("{at}: `grid` must name a grid"))?
                    .to_owned(),
                albedo: colour(b, "albedo", &at)?,
                intensity: num(b, "intensity", &at).and_then(|v| {
                    (v >= 0.0)
                        .then_some(v)
                        .ok_or_else(|| format!("{at}: `intensity` must be ≥ 0"))
                })?,
                ref_deg: positive(b, "refDeg", &at)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let a = get(l, "ambient", at)?;
    let glow = match l.get("glow") {
        None => None,
        Some(gl) => Some((
            colour(gl, "color", &format!("{at} light.glow"))?,
            positive(gl, "intensity", &format!("{at} light.glow"))?,
        )),
    };
    Ok(Some(LightRig {
        sunlit,
        bodies,
        ambient: colour(a, "color", &format!("{at} light.ambient"))?,
        ambient_min: num(a, "min", &format!("{at} light.ambient"))?.max(0.0),
        glow,
    }))
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

#[derive(Clone, Debug, PartialEq)]
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
    /// The maria's ground block, by name (a darker dust): a column at least half in a mare
    /// ([`CrateredTerrain::mare_weight`] ≥ ½) is this block, and the far bake's colour there is
    /// its `color`.
    pub block: String,
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
    /// The rule that shapes its tops into stored ramps (`ramps`: `none`, `strict` or `notched`,
    /// HALF_HEIGHT_GRID.md §4 and §10); none when not given.
    pub ramps: crate::ramps::RampRule,
    /// The far-LOD map's texels a side, per face.
    pub bake_size: usize,
}

/// A generator's `ramps` (none when not given).
pub(crate) fn ramp_rule(g: &Value, at: &str) -> Result<crate::ramps::RampRule, String> {
    match g.get("ramps") {
        None => Ok(crate::ramps::RampRule::None),
        Some(v) => v
            .as_str()
            .and_then(crate::ramps::RampRule::parse)
            .ok_or_else(|| {
                format!("{at}: `generator.ramps` must be \"none\", \"strict\" or \"notched\"")
            }),
    }
}

pub(crate) fn get<'a>(v: &'a Value, path: &str, at: &str) -> Result<&'a Value, String> {
    let mut x = v;
    for k in path.split('.') {
        x = x
            .get(k)
            .ok_or_else(|| format!("{at}: `{path}` is missing"))?;
    }
    Ok(x)
}

pub(crate) fn num(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    get(v, path, at)?
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{at}: `{path}` must be a number"))
}

pub(crate) fn positive(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    let n = num(v, path, at)?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err(format!("{at}: `{path}` must be > 0 (is {n})"))
    }
}

pub(crate) fn share(v: &Value, path: &str, at: &str) -> Result<f64, String> {
    let n = num(v, path, at)?;
    if (0.0..=1.0).contains(&n) {
        Ok(n)
    } else {
        Err(format!("{at}: `{path}` must be in 0..=1 (is {n})"))
    }
}

pub(crate) fn range(v: &Value, path: &str, at: &str) -> Result<[f64; 2], String> {
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

pub(crate) fn noise_rule(v: &Value, at: &str) -> Result<NoiseRule, String> {
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

/// A block name at `path`, which must name a block of the registry.
pub(crate) fn block_name(v: &Value, path: &str, at: &str) -> Result<String, String> {
    let block = get(v, path, at)?
        .as_str()
        .ok_or_else(|| format!("{at}: `{path}` must be a block name"))?;
    if dwell_blocks::state(block).is_none() {
        return Err(format!("{at}: `{path}` names no block (`{block}`)"));
    }
    Ok(block.to_owned())
}

fn crater_rules(g: &Value, at: &str) -> Result<CraterRules, String> {
    let block = block_name(g, "block", at)?;
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
        block: block_name(m, "block", &format!("{at} maria"))?,
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
        block,
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
        ramps: ramp_rule(g, at)?,
        bake_size: bake,
    })
}

/// A generator's `bake.size`: the far-LOD map's texels a side per face.
pub(crate) fn bake_size(g: &Value, at: &str) -> Result<usize, String> {
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
            Some("sunPath") => GridFrameKind::SunPath,
            _ => {
                return Err(format!(
                    "{at}: `frame` must be \"rest\", \"orbit\" or \"sunPath\""
                ));
            }
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
            Some("volcanic") => {
                GridGenerator::Volcanic(Arc::new(crate::volcanic::volcanic_rules(gen_, &at)?))
            }
            _ => {
                return Err(format!(
                    "{at}: `generator.kind` must be \"terrain\", \"cratered\" or \"volcanic\""
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
        // No air unless declared (the moon, the sun): its sky is space, the night sky's.
        let atmosphere = match g.get("atmosphere") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("{at}: `atmosphere` must be true or false"))?,
        };
        let near_mesh = match g.get("nearMesh") {
            None => None,
            Some(_) => {
                let cells = num(g, "nearMesh.cells", &at)?;
                if !(4.0..=512.0).contains(&cells) || cells.fract() != 0.0 {
                    return Err(format!(
                        "{at}: `nearMesh.cells` must be a whole number in 4..=512"
                    ));
                }
                let sink_m = num(g, "nearMesh.sinkM", &at)?;
                if !(0.0..=200.0).contains(&sink_m) {
                    return Err(format!("{at}: `nearMesh.sinkM` must be in 0..=200"));
                }
                Some(NearMesh {
                    cells: cells as usize,
                    sink_m,
                })
            }
        };
        let sky_disc = match g.get("skyDisc") {
            None => true,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("{at}: `skyDisc` must be true or false"))?,
        };
        if !sky_disc && near_mesh.is_none() {
            return Err(format!(
                "{at}: a grid with no sky disc (`skyDisc` false) needs a `nearMesh` to be seen by"
            ));
        }
        let lod = match g.get("lod") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("{at}: `lod` must be true or false"))?,
        };
        if lod && (radius_m.is_none() || matches!(generator, GridGenerator::Terrain { .. })) {
            return Err(format!(
                "{at}: `lod` needs a disc (`radiusM`) and a generator whose columns its sections \
                 come from (`cratered`, `volcanic`); home's terrain has its own LOD"
            ));
        }
        let light = light_rig(g, &at)?;
        if frame == GridFrameKind::Rest && light.is_some() {
            return Err(format!(
                "{at}: home is lit by its sky's keyframes (no `light`)"
            ));
        }
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
        let capability = match g.get("capability") {
            None => None,
            Some(c) => {
                let c = c
                    .as_str()
                    .filter(|c| dwell_protocol::capability_names::ALL.contains(c))
                    .ok_or_else(|| {
                        format!(
                            "{at}: `capability` must name a capability of shared/protocol/capabilities.json"
                        )
                    })?;
                if frame == GridFrameKind::Rest {
                    return Err(format!(
                        "{at}: the grid at rest is everyone's (no `capability`)"
                    ));
                }
                Some(c.to_owned())
            }
        };
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
            atmosphere,
            near_mesh,
            sky_disc,
            lod,
            light,
            generator,
            capability,
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
    for d in &out {
        for b in d.light.iter().flat_map(|l| &l.bodies) {
            if b.grid == d.name || !out.iter().any(|o| o.name == b.grid) {
                return Err(format!(
                    "grid `{}`: `light.bodies` names `{}`, not another grid of the list",
                    d.name, b.grid
                ));
            }
        }
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

/// A spawn on another grid (MOON_WORLD.md §8, M-8; owner 2026-11-13: "Im not seeing a moon spawn or
/// sun spawn in my new world settings"): the grid and face a world's players start on, chosen by
/// its key (the grid's name after its namespace, `-b` for face B: `moon`, `moon-b`, `sun`, `sun-b`),
/// the New world form's Spawn and the `?spawn=` link alike. Every grid of the list but the home
/// grid (index 0) that has a generator of its own offers both faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpawn {
    /// The grid's index in its list (the wire's grid byte).
    pub index: usize,
    pub def: &'static GridDef,
    pub face: Face,
}

/// A grid face's spawn key (`moon`, `moon-b`).
pub fn spawn_key(def: &GridDef, face: Face) -> String {
    let short = def
        .name
        .split_once(':')
        .map_or(def.name.as_str(), |(_, n)| n);
    match face {
        Face::A => short.to_owned(),
        Face::B => format!("{short}-b"),
    }
}

/// The spawn keys a grid list offers, in its order (both faces of every grid but the home one with
/// a generator a stand point is found on).
pub fn spawn_keys(list: &[GridDef]) -> Vec<String> {
    list.iter()
        .skip(1)
        .filter(|g| g.radius_m.is_some() && !matches!(g.generator, GridGenerator::Terrain { .. }))
        .flat_map(|g| [spawn_key(g, Face::A), spawn_key(g, Face::B)])
        .collect()
}

/// The build's grid spawn of key `key` (`sun-b`), if any.
pub fn grid_spawn(key: &str) -> Option<GridSpawn> {
    let all = grids();
    all.iter().enumerate().skip(1).find_map(|(index, def)| {
        [Face::A, Face::B]
            .into_iter()
            .find(|&f| spawn_key(def, f) == key && spawn_keys(all).contains(&spawn_key(def, f)))
            .map(|face| GridSpawn { index, def, face })
    })
}

/// Where a player stands on grid `def` of the world of `world_seed` on `face`: its generator's
/// spawn point (the moon's flat ground clear of crater walls; the sun's dry crust away from lava),
/// face-local feet (x, ground, z). `None` for a grid without such a generator.
pub fn stand_point(def: &GridDef, world_seed: u64, face: Face) -> Option<[f64; 3]> {
    match &def.generator {
        GridGenerator::Cratered(_) => {
            Some(CrateredTerrain::new(def, world_seed)?.spawn_point(face))
        }
        GridGenerator::Volcanic(_) => {
            Some(crate::volcanic::VolcanicTerrain::new(def, world_seed)?.spawn_point(face))
        }
        GridGenerator::Terrain { .. } => None,
    }
}

/// Travel between the grids (`grids.json` `travel`, MOON_WORLD.md §7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TravelRules {
    /// Far flight starts this far (m) from the nearest grid's slab.
    pub far_above_m: f64,
    /// Far flight's speed per metre of that distance (1/s): `v = perS × d`.
    pub far_per_s: f64,
    /// Far flight's space speed (m/s): its top, reached only in space (`space` 1 in
    /// [`TravelRules::far_speed`]).
    pub far_max_mps: f64,
    /// A captured player is released past `captureM` × this (> 1: hysteresis).
    pub release_factor: f64,
}

impl TravelRules {
    /// The far-flight speed (the least fly speed, m/s) at `d` m from the nearest grid's slab and
    /// `space` (0–1) of the way into space, the sky's value (the atmosphere's thinning, docs/
    /// DAY_NIGHT.md §15; owner, 2026-11-13: "a much faster fly speed for creative. But only in
    /// space."): 0 nearer than `aboveM` or in the air (the slider's speed alone), else
    /// `perS × d` (easing down toward every body's surface) up to `maxMps × space`.
    pub fn far_speed(&self, d: f64, space: f64) -> f64 {
        if d <= self.far_above_m {
            0.0
        } else {
            (self.far_per_s * d).min(self.far_max_mps * space.clamp(0.0, 1.0))
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
    /// RGBA: the ground's colour (its blocks' `color`, the maria's darker block), A 255 inside the disc, 0 outside.
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

/// Grid `def`'s far source in the world of `world_seed` (its generator, read at any scale: the far
/// bake's, the near mesh's and the grid LOD's): the `cratered` and `volcanic` kinds; `None` for
/// home's `terrain`, whose source is its world map (`crate::home_far::HomeFar`).
pub fn far_source(def: &GridDef, world_seed: u64) -> Option<Arc<dyn FarSource + Send + Sync>> {
    match &def.generator {
        GridGenerator::Cratered(_) => Some(Arc::new(CrateredTerrain::new(def, world_seed)?)),
        GridGenerator::Volcanic(_) => Some(Arc::new(crate::volcanic::VolcanicTerrain::new(
            def, world_seed,
        )?)),
        GridGenerator::Terrain { .. } => None,
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
    /// The maria's ground.
    mare_block: StateId,
    /// Each ground block's ramp pieces by facing: the ½ m ramp's cell and the 1 m ramp's cells
    /// from the bottom (empty when the rules shape no ramps).
    pieces: Vec<(StateId, RampFacing, StateId, Vec<StateId>)>,
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
        let block = dwell_blocks::require(&rules.block);
        let mare_block = dwell_blocks::require(&rules.maria.block);
        let mut pieces = Vec::new();
        if rules.ramps != RampRule::None {
            for (m, name) in [(block, &rules.block), (mare_block, &rules.maria.block)] {
                for facing in RampFacing::ALL {
                    let g = dwell_world::ramps::gentle_cell_flooded(name, facing, false);
                    let st = dwell_world::ramps::steep_cells_flooded(name, facing, false);
                    let (Some(g), Some(st)) = (g, st) else {
                        panic!(
                            "grid `{}`: `generator.ramps` shapes ramps but `{name}` has no ramp family (blocks `rampFamilies`)",
                            def.name
                        );
                    };
                    pieces.push((m, facing, g, st));
                }
            }
        }
        Some(Self {
            rules: rules.clone(),
            radius: def.radius_m?,
            seeds: [face_seed(seed, Face::A), face_seed(seed, Face::B)],
            block,
            mare_block,
            pieces,
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

    /// The cells (from the bottom) topping column (x, z) of `face`, by the rules' `ramps`
    /// (HALF_HEIGHT_GRID.md §4): empty for a flat top. `top` gives a column's top row (`None` off
    /// the disc, lower than any ground), the column's own included.
    pub fn top_pieces(
        &self,
        face: Face,
        x: i64,
        z: i64,
        top: impl Fn(i64, i64) -> Option<i64>,
    ) -> &[StateId] {
        if self.pieces.is_empty() {
            return &[];
        }
        let Some(t) = top(x, z) else {
            return &[];
        };
        let around: crate::ramps::Around = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                top(x + c as i64 - 1, z + r as i64 - 1).map_or(i32::MIN / 2, |t| (t + 1) as i32)
            })
        });
        let (facing, steep) = match self.rules.ramps.top(&around, rows_for_m(1.0)) {
            Top::Flat => return &[],
            Top::Gentle(f) => (f, false),
            Top::Steep(f) => (f, true),
        };
        let m = self.surface_block(face, x, z);
        let Some((_, _, g, st)) = self.pieces.iter().find(|p| p.0 == m && p.1 == facing) else {
            return &[];
        };
        let cells: &[StateId] = if steep { st } else { std::slice::from_ref(g) };
        // A 1 m ramp needs two cells of ground.
        if cells.len() as i64 > t + 1 {
            return &[];
        }
        cells
    }

    /// The ground block of column (x, z) on `face`: the maria's block where the column's centre
    /// is at least half in a mare, the grid's block elsewhere.
    pub fn surface_block(&self, face: Face, x: i64, z: i64) -> StateId {
        if self.mare_weight(face, x as f64 + 0.5, z as f64 + 0.5) >= 0.5 {
            self.mare_block
        } else {
            self.block
        }
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
        let (x0, z0) = (i64::from(local.x * S), i64::from(local.z * S));
        // The tops of the chunk's columns and a ring round them, for the ramp rule's neighbours.
        let w = (S + 2) as usize;
        let tops: Vec<Option<i64>> = (0..w * w)
            .map(|k| self.top_row(f.face, x0 - 1 + (k % w) as i64, z0 - 1 + (k / w) as i64))
            .collect();
        let top_at = |x: i64, z: i64| tops[((z - z0 + 1) as usize) * w + (x - x0 + 1) as usize];
        let cells = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let wx = x0 + i64::from(x);
                let wz = z0 + i64::from(z);
                let top = top_at(wx, wz);
                let ground = f.state(self.surface_block(f.face, wx, wz));
                let pieces = self.top_pieces(f.face, wx, wz, top_at);
                let first = top.map_or(0, |t| t + 1 - pieces.len() as i64);
                for y in 0..S {
                    let row = i64::from(local.y * S + y);
                    let solid = top.is_some_and(|t| row >= 0 && row <= t);
                    let ly = if f.is_b() { S - 1 - y } else { y };
                    cells[local_index(x, ly, z)] = if !solid {
                        M.air
                    } else if row >= first && !pieces.is_empty() {
                        f.state(pieces[(row - first) as usize])
                    } else {
                        ground
                    };
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
        // Each block's own colour (blocks.json `color`), blended across a mare's soft edge.
        let rgb = |s: StateId| {
            let c = dwell_blocks::info(s).color;
            [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0)
        };
        let (high, mare) = (rgb(self.block), rgb(self.mare_block));
        let w = self.mare_weight(face, x, z);
        std::array::from_fn(|i| high[i] + (mare[i] - high[i]) * w)
    }
}

impl crate::grid_lod::GridColumns for CrateredTerrain {
    fn radius_m(&self) -> f64 {
        self.radius
    }
    /// The ground with every crater at least half a cell wide, and the block the chunks give a
    /// column there: the maria's where the point is at least half in a mare, else the grid's.
    fn lod_column(
        &self,
        face: Face,
        x: f64,
        z: f64,
        cell_m: f64,
    ) -> Option<crate::grid_lod::GridColumn> {
        if !self.inside(x, z) {
            return None;
        }
        let ground = if self.mare_weight(face, x, z) >= 0.5 {
            self.mare_block
        } else {
            self.block
        };
        Some(crate::grid_lod::GridColumn {
            ground_m: self.height_at_scale(face, x, z, cell_m / 2.0),
            ground,
            fluid_m: f64::NEG_INFINITY,
            fluid: M.air,
        })
    }
}
