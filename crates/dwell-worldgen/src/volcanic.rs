//! The sun's `volcanic` generator (docs/SUN_WORLD.md, slice S-2; WORK_QUEUE item 42): a volcano
//! world, all from `grids.json` data. Face-local heights (m over the grid's midplane), each face
//! from its own seed stream (`bifacial::face_seed`), as the moon's and home's are.
//!
//! - **Plains and basins:** `datumM` + fbm plains + broad fbm basins; wherever the ground lies under
//!   `lava.seaM` it is a **lava ocean** (lava up to the sea's level).
//! - **Rifts:** ridged noise's zero lines carved `rifts.depthM` deep (a smooth V across `width` of
//!   the noise); the bottom `fill` of each rift's depth holds a **lava river** (its surface follows
//!   the ground it is cut into, so it runs downhill with the land).
//! - **Cones:** one candidate a jittered `cones.cellM` cell (`chance`), a radius and height drawn in
//!   their ranges, its flank `(1 − d/R)^profile` over the ground at its centre; a summit crater
//!   `craterRatio` × R wide and `craterDepthM` deep holding a **lava lake** `lakeBelowRimM` under
//!   its rim.
//! - **Crusts:** the ground is `crust` (basalt); a dry column whose top is within `shore.bandM` over
//!   the lava it borders is `shore.block` (obsidian, the quench glass at the edge).
//! - **Lava** is `lava.fluid` (`dwell:lava`, the same block the lava fields of VOLCANOES.md write),
//!   generated still: the always-flowing simulation on the sun is a later slice.
//!
//! [`VolcanicTerrain::generate`] fills a grid chunk through `FaceFrame` (face B's flipped), and the
//! far-body bake ([`FarSource`]) shapes the sky's sun disc from the same columns (S-3).

use std::sync::Arc;

use dwell_blocks::StateId;
use dwell_world::{CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, FaceFrame, M, local_index};
use serde_json::Value;

use crate::bifacial::face_seed;
use crate::grids::{
    FarBake, FarSource, GridDef, GridGenerator, NoiseRule, bake_far, bake_size, block_name, get,
    grid, noise_rule, num, positive, range, share,
};
use crate::noise::{fbm2, hash2, seed_word, unit};

/// The lava: its block and the oceans' level.
#[derive(Clone, Debug, PartialEq)]
pub struct LavaRule {
    pub block: String,
    /// Ground under this (face-local m) is a lava ocean's floor, the lava up to it.
    pub sea_m: f64,
}

/// The obsidian edge round the lava.
#[derive(Clone, Debug, PartialEq)]
pub struct ShoreRule {
    pub block: String,
    /// A dry column at most this far (m) over the lava it borders is the shore's block.
    pub band_m: f64,
}

/// The volcanic cones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConeRule {
    /// A jittered grid of this cell (m), one candidate cone a cell.
    pub cell_m: f64,
    pub chance: f64,
    pub radius_m: [f64; 2],
    pub height_m: [f64; 2],
    /// The flank's shape: `(1 − d/R)^profile` (1 a straight cone, more a concave one).
    pub profile: f64,
    /// The summit crater's radius, × the cone's.
    pub crater_ratio: f64,
    pub crater_depth_m: f64,
    /// The crater's lava lake stands this far under its rim.
    pub lake_below_rim_m: f64,
}

/// The rifts and their lava rivers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiftRule {
    /// The ridged noise the rifts follow (its amplitude unused: `depthM` is the depth).
    pub noise: NoiseRule,
    /// The rift's width as a share of the noise's range round its zero line.
    pub width: f64,
    pub depth_m: f64,
    /// The share of the rift's depth, from its bottom, that holds lava.
    pub fill: f64,
}

/// The `volcanic` generator's rules.
#[derive(Clone, Debug, PartialEq)]
pub struct VolcanicRules {
    /// The crust's block (basalt), by name.
    pub crust: String,
    pub datum_m: f64,
    pub min_m: f64,
    pub plains: NoiseRule,
    pub basins: NoiseRule,
    pub lava: LavaRule,
    pub shore: ShoreRule,
    pub cones: ConeRule,
    pub rifts: RiftRule,
    pub bake_size: usize,
}

/// Parses a `volcanic` generator's rules; a bad entry fails with a message naming `at`.
pub(crate) fn volcanic_rules(g: &Value, at: &str) -> Result<VolcanicRules, String> {
    let l = get(g, "lava", at)?;
    let s = get(g, "shore", at)?;
    let c = get(g, "cones", at)?;
    let r = get(g, "rifts", at)?;
    let datum_m = num(g, "datumM", at)?;
    let min_m = positive(g, "minM", at)?;
    if min_m >= datum_m {
        return Err(format!("{at}: `minM` must be under `datumM`"));
    }
    let lava = LavaRule {
        block: block_name(l, "fluid", &format!("{at} lava"))?,
        sea_m: positive(l, "seaM", at)?,
    };
    if lava.sea_m <= min_m {
        return Err(format!("{at}: `lava.seaM` must be over `minM`"));
    }
    let cones = ConeRule {
        cell_m: positive(c, "cellM", at)?,
        chance: share(c, "chance", at)?,
        radius_m: range(c, "radiusM", at)?,
        height_m: range(c, "heightM", at)?,
        profile: positive(c, "profile", at)?,
        crater_ratio: share(c, "craterRatio", at)?,
        crater_depth_m: num(c, "craterDepthM", at)?.max(0.0),
        lake_below_rim_m: num(c, "lakeBelowRimM", at)?.max(0.0),
    };
    if cones.radius_m[1] * 2.0 > cones.cell_m {
        return Err(format!(
            "{at}: `cones.radiusM` must stay under half `cones.cellM`"
        ));
    }
    if cones.crater_ratio >= 1.0 {
        return Err(format!("{at}: `cones.craterRatio` must be < 1"));
    }
    let rifts = RiftRule {
        noise: noise_rule(r, &format!("{at} rifts"))?,
        width: share(r, "width", at)?,
        depth_m: num(r, "depthM", at)?.max(0.0),
        fill: share(r, "fill", at)?,
    };
    if rifts.width <= 0.0 {
        return Err(format!("{at}: `rifts.width` must be > 0"));
    }
    Ok(VolcanicRules {
        crust: block_name(g, "crust", at)?,
        datum_m,
        min_m,
        plains: noise_rule(get(g, "plains", at)?, &format!("{at} plains"))?,
        basins: noise_rule(get(g, "basins", at)?, &format!("{at} basins"))?,
        lava,
        shore: ShoreRule {
            block: block_name(s, "block", &format!("{at} shore"))?,
            band_m: num(s, "bandM", at)?.max(0.0),
        },
        cones,
        rifts,
        bake_size: bake_size(g, at)?,
    })
}

/// One cone, face-local (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    pub x: f64,
    pub z: f64,
    pub radius: f64,
    pub height: f64,
}

/// What a column holds: its ground's top and the lava it borders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// The ground's top (face-local m).
    pub ground: f64,
    /// The lava's surface this column borders (face-local m): the ocean's level, a river's or a
    /// crater lake's, the highest of those that reach it. Lava fills the column from the ground up
    /// to it where it is over the ground.
    pub lava: f64,
}

impl Column {
    /// Lava over the ground.
    pub fn molten(&self) -> bool {
        self.lava > self.ground
    }
    /// What a far viewer sees: the lava's surface or the ground's.
    pub fn surface(&self) -> f64 {
        self.ground.max(self.lava)
    }
}

/// The `volcanic` generator over one grid of one world, both faces.
#[derive(Clone, Debug)]
pub struct VolcanicTerrain {
    rules: Arc<VolcanicRules>,
    radius: f64,
    seeds: [u64; 2],
    crust: StateId,
    shore: StateId,
    lava: StateId,
}

const PLAINS: u32 = 0x53_0001;
const BASINS: u32 = 0x53_0002;
const RIFTS: u32 = 0x53_0003;
const CONES: u32 = 0x53_0004;

fn fi(f: Face) -> usize {
    match f {
        Face::A => 0,
        Face::B => 1,
    }
}

impl VolcanicTerrain {
    /// The terrain of a `volcanic` grid in the world of `world_seed`; `None` for another kind or a
    /// grid sized by the world.
    pub fn new(def: &GridDef, world_seed: u64) -> Option<Self> {
        let GridGenerator::Volcanic(rules) = &def.generator else {
            return None;
        };
        let seed = def.seed(world_seed);
        Some(Self {
            rules: rules.clone(),
            radius: def.radius_m?,
            seeds: [face_seed(seed, Face::A), face_seed(seed, Face::B)],
            crust: dwell_blocks::require(&rules.crust),
            shore: dwell_blocks::require(&rules.shore.block),
            lava: dwell_blocks::require(&rules.lava.block),
        })
    }

    /// The build's grid `name` in the world of `world_seed` (`dwell:sun`).
    pub fn for_grid(name: &str, world_seed: u64) -> Option<Self> {
        Self::new(grid(name)?, world_seed)
    }

    pub fn rules(&self) -> &VolcanicRules {
        &self.rules
    }
    pub fn radius_m(&self) -> f64 {
        self.radius
    }
    pub fn inside(&self, x: f64, z: f64) -> bool {
        x * x + z * z <= self.radius * self.radius
    }
    /// The lava's block (`lava.fluid`).
    pub fn lava_block(&self) -> StateId {
        self.lava
    }

    fn stream(&self, face: Face, stream: u32) -> u32 {
        seed_word(self.seeds[fi(face)], stream)
    }

    fn noise(&self, face: Face, stream: u32, rule: &NoiseRule, x: f64, z: f64) -> f64 {
        f64::from(fbm2(
            self.stream(face, stream),
            x.floor() as i64,
            z.floor() as i64,
            rule.wavelength_m as i32,
            rule.octaves,
            None,
        ))
    }

    /// The plains and basins at (x, z), before the rifts and cones.
    pub fn base_height(&self, face: Face, x: f64, z: f64) -> f64 {
        let r = &*self.rules;
        r.datum_m
            + self.noise(face, PLAINS, &r.plains, x, z) * r.plains.amplitude_m
            + self.noise(face, BASINS, &r.basins, x, z) * r.basins.amplitude_m
    }

    /// How deep in a rift (x, z) is: 1 on its zero line, easing to 0 at its edge.
    pub fn rift_weight(&self, face: Face, x: f64, z: f64) -> f64 {
        let r = &self.rules.rifts;
        let n = self.noise(face, RIFTS, &r.noise, x, z).abs();
        let v = (1.0 - n / r.width).clamp(0.0, 1.0);
        v * v * (3.0 - 2.0 * v)
    }

    /// The cones whose flanks reach within `within` m of (x, z).
    pub fn cones_near(&self, face: Face, x: f64, z: f64, within: f64) -> Vec<Cone> {
        let c = &self.rules.cones;
        let seed = self.stream(face, CONES);
        let span = ((c.radius_m[1] + within) / c.cell_m).ceil() as i32;
        let (cx, cz) = ((x / c.cell_m).floor() as i32, (z / c.cell_m).floor() as i32);
        let mut out = Vec::new();
        for gz in cz - span..=cz + span {
            for gx in cx - span..=cx + span {
                if f64::from(unit(hash2(seed, gx, gz))) >= c.chance {
                    continue;
                }
                let j = |k: u32| f64::from(unit(hash2(seed ^ k, gx, gz)));
                let lerp = |r: [f64; 2], t: f64| r[0] + (r[1] - r[0]) * t;
                // Inside its cell with room for its flank: no cone is cut by its cell's edge.
                let radius = lerp(c.radius_m, j(0x33));
                let room = (c.cell_m - 2.0 * radius) / c.cell_m;
                let cone = Cone {
                    x: (f64::from(gx) + radius / c.cell_m + room * j(0x11)) * c.cell_m,
                    z: (f64::from(gz) + radius / c.cell_m + room * j(0x22)) * c.cell_m,
                    radius,
                    height: lerp(c.height_m, j(0x44)),
                };
                if (cone.x - x).hypot(cone.z - z) <= cone.radius + within {
                    out.push(cone);
                }
            }
        }
        out
    }

    /// A cone's ground and its crater lake's surface at `d` m from its centre, over the ground
    /// `foot` at its centre: the flank, and inside the crater a bowl down from the rim.
    fn cone_at(&self, cone: &Cone, foot: f64, d: f64) -> (f64, Option<f64>) {
        let c = &self.rules.cones;
        let flank = |d: f64| foot + cone.height * (1.0 - d / cone.radius).max(0.0).powf(c.profile);
        let crater = c.crater_ratio * cone.radius;
        if d >= crater {
            return (flank(d), None);
        }
        let rim = flank(crater);
        let t = d / crater;
        let ground = rim - c.crater_depth_m * (1.0 - t * t);
        (ground, Some(rim - c.lake_below_rim_m))
    }

    /// The column at (x, z): its ground and the lava it borders.
    pub fn column(&self, face: Face, x: f64, z: f64) -> Column {
        let r = &*self.rules;
        let base = self.base_height(face, x, z);
        let w = self.rift_weight(face, x, z);
        let mut ground = base - r.rifts.depth_m * w;
        let mut lava = r.lava.sea_m;
        if w > 0.0 {
            lava = lava.max(base - r.rifts.depth_m * (1.0 - r.rifts.fill));
        }
        for cone in self.cones_near(face, x, z, 0.0) {
            let foot = self.base_height(face, cone.x, cone.z);
            let (g, lake) = self.cone_at(&cone, foot, (cone.x - x).hypot(cone.z - z));
            if let Some(lake) = lake {
                // Inside the crater the bowl is the ground, whatever the land round it.
                ground = g;
                lava = lava.max(lake);
            } else {
                ground = ground.max(g);
            }
        }
        Column {
            ground: ground.max(r.min_m),
            lava,
        }
    }

    /// The ground's height (face-local m) at (x, z): what a player stands on (lava has no floor).
    pub fn height(&self, face: Face, x: f64, z: f64) -> f64 {
        self.column(face, x, z).ground
    }

    /// The column's face-local rows: its ground's top cell and its lava's top cell (`None` when
    /// it holds no lava), or `None` outside the disc.
    pub fn rows(&self, face: Face, x: i64, z: i64) -> Option<(i64, Option<i64>)> {
        let (cx, cz) = (x as f64 + 0.5, z as f64 + 0.5);
        if !self.inside(cx, cz) {
            return None;
        }
        let col = self.column(face, cx, cz);
        let top = (col.ground / CELL_H).round() as i64 - 1;
        let lava = (col.lava / CELL_H).round() as i64 - 1;
        Some((top, (lava > top).then_some(lava)))
    }

    /// The ground's block of a column: the shore's glass where it is dry and at most `shore.bandM`
    /// over the lava it borders, the crust elsewhere (under lava too).
    pub fn ground_block(&self, col: &Column) -> StateId {
        let over = col.ground - col.lava;
        if over >= 0.0 && over <= self.rules.shore.band_m {
            self.shore
        } else {
            self.crust
        }
    }

    /// A chunk of the grid (in the grid's own coordinates): the ground's block from the midplane
    /// up to the surface, then lava up to its level, both faces through `FaceFrame`.
    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        const S: i32 = CHUNK_SIZE;
        let f = FaceFrame::at_chunk_y(coord.y);
        let local = f.chunk(coord);
        let lava = f.state(self.lava);
        let cells = chunk.voxels_mut();
        for z in 0..S {
            for x in 0..S {
                let wx = i64::from(local.x * S + x);
                let wz = i64::from(local.z * S + z);
                let (cx, cz) = (wx as f64 + 0.5, wz as f64 + 0.5);
                let rows = self.rows(f.face, wx, wz);
                let ground = match rows {
                    Some(_) => f.state(self.ground_block(&self.column(f.face, cx, cz))),
                    None => M.air,
                };
                for y in 0..S {
                    let row = i64::from(local.y * S + y);
                    let ly = if f.is_b() { S - 1 - y } else { y };
                    cells[local_index(x, ly, z)] = match rows {
                        Some((top, _)) if row >= 0 && row <= top => ground,
                        Some((top, Some(l))) if row > top && row <= l => lava,
                        _ => M.air,
                    };
                }
            }
        }
    }

    /// The far-LOD maps of one face at `size` texels a side ([`bake_far`]).
    pub fn bake_far(&self, face: Face, size: usize) -> FarBake {
        bake_far(self, face, size)
    }

    /// [`VolcanicTerrain::bake_far`] at the grid's data size (`bake.size`).
    pub fn bake_far_default(&self, face: Face) -> FarBake {
        self.bake_far(face, self.rules.bake_size)
    }
}

fn rgb(s: StateId) -> [f64; 3] {
    let c = dwell_blocks::info(s).color;
    [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0)
}

impl FarSource for VolcanicTerrain {
    fn far_radius_m(&self) -> f64 {
        self.radius
    }
    /// The lava's surface where it stands over the ground (flat oceans and lakes), else the ground.
    fn far_height(&self, face: Face, x: f64, z: f64, _texel_m: f64) -> f64 {
        self.column(face, x, z).surface()
    }
    /// Each block's own colour (blocks.json `color`): the lava's where it is molten, the shore's
    /// glass or the crust's elsewhere.
    fn far_albedo(&self, face: Face, x: f64, z: f64) -> [f64; 3] {
        let col = self.column(face, x, z);
        rgb(if col.molten() {
            self.lava
        } else {
            self.ground_block(&col)
        })
    }
}
