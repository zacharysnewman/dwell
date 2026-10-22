//! Between the faces (docs/BIFACIAL_WORLD.md §2, §6): the bedrock either side of the midplane and
//! the portals through it — a few round shafts per world, open from face A's ground to face B's,
//! a ladder down one wall the whole way and a low rim round each opening. Owner, 2026-10-21: "add
//! bedrock back in between the two faces. So the portals are holes through the bedrock leading up
//! to the surface on both sides."
//!
//! Everything is data (`shared/worldgen/midplane.json`, generated into `midplane.gen.rs`): the
//! bedrock's block and thickness, the portals' count, spacing, siting rules, radius, rim and ladder.
//! The sites are a pure function of the world seed and both faces' height fields (searched once
//! per seed and rules, then shared), so every chunk knows from the seed alone whether a portal
//! reaches it, in any order, on any thread, natively or in WASM (integer hashing and the height
//! field's own deterministic arithmetic, ADR 0010).

use std::sync::{Arc, Mutex};

use dwell_blocks::StateId;
use dwell_world::SEA_LEVEL_M;

use crate::noise::seed_word;
use crate::terrain::TerrainGenerator;

/// The bedrock and the portals (data: `shared/worldgen/midplane.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Midplane {
    /// The bedrock's block, by name (indestructible: the data's validation checks it).
    pub bedrock: &'static str,
    /// The bedrock's rows on each face, from the midplane (face-local rows 0 … `bedrock_rows` − 1).
    pub bedrock_rows: i32,
    pub portals: PortalRules,
}

/// How a world's portals are sited and shaped (`midplane.json` `portals`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalRules {
    /// How many portals a world has: the seed picks a number in `count_min ..= count_max` (fewer
    /// if the search finds fewer sites).
    pub count_min: u32,
    pub count_max: u32,
    /// The least distance between two portals' axes (m).
    pub spacing: f32,
    /// Portals lie within this distance of the disc's centre (m).
    pub within: f32,
    /// Seeded points tried for sites.
    pub attempts: u32,
    /// A site lies at least this far inland of both faces' coasts (m).
    pub inland: f32,
    /// A site's ground on both faces lies at most this far above sea level (m), over its whole
    /// footprint (the shaft and the rim).
    pub max_height: f32,
    /// The shaft's radius (m): columns whose centres lie within it are open.
    pub radius: f32,
    /// The rim round the opening: its block, its width (m beyond the shaft) and its height (rows
    /// over the ground).
    pub rim_block: &'static str,
    pub rim_width: f32,
    pub rim_rows: i32,
    /// The ladder's state (the block turned away from its wall) and the wall's direction (x, z).
    pub ladder: &'static str,
    pub wall: [i32; 2],
    /// No tree or decoration stands within this far of the rim (m).
    pub clear: f32,
    /// The look of the glowing motes drawn in the shaft round the midplane, by name
    /// (`shared/look/motes.json`; the client's, not the terrain's).
    pub motes: &'static str,
}

mod table {
    include!("midplane.gen.rs");
}
pub use table::MIDPLANE;

/// A portal: the column its shaft's axis runs through the centre of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Portal {
    pub x: i32,
    pub z: i32,
}

/// What a column is to the portals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Open from the ground to the midplane; `ladder`: the column the ladder runs up.
    Shaft { ladder: bool },
    /// The rim round the opening: flat ground with the rim on top.
    Rim,
    /// Bare ground round the rim: no tree or decoration.
    Clear,
    /// Not near a portal.
    None,
}

/// A world's portals, resolved: the sites and the states they are built of.
#[derive(Debug)]
pub struct Portals {
    pub rules: PortalRules,
    pub sites: Arc<Vec<Portal>>,
    pub rim: StateId,
    pub ladder: StateId,
    /// Squared radii (m²) of the shaft, the rim's outer edge and the clear ground's.
    shaft2: f64,
    rim2: f64,
    clear2: f64,
    /// How far (whole columns) the clear ground reaches from an axis.
    reach: i32,
    /// The ladder's column: this many columns from the axis toward the wall.
    ladder_k: i32,
}

impl Portals {
    /// The portals of a world whose faces' generators are `a` and `b`, by `rules`. The sites are
    /// searched once per (seed, rules, faces) and shared.
    pub fn resolve(
        world_seed: u64,
        rules: PortalRules,
        a: &TerrainGenerator,
        b: &TerrainGenerator,
        mirrored: bool,
    ) -> Self {
        let sites = cached_sites(world_seed, rules, a.map().settings, mirrored, || {
            search(world_seed, &rules, a, b)
        });
        Self::with_sites(rules, sites)
    }

    /// The portals at given sites (tests and tools).
    pub fn with_sites(rules: PortalRules, sites: Arc<Vec<Portal>>) -> Self {
        let r = f64::from(rules.radius);
        let rim = r + f64::from(rules.rim_width);
        let clear = rim + f64::from(rules.clear);
        let ladder_k = (0..)
            .take_while(|&k: &i32| f64::from(k * k) <= r * r)
            .last()
            .unwrap_or(0);
        Self {
            rules,
            sites,
            rim: dwell_blocks::require(rules.rim_block),
            ladder: dwell_blocks::require(rules.ladder),
            shaft2: r * r,
            rim2: rim * rim,
            clear2: clear * clear,
            reach: clear.ceil() as i32,
            ladder_k,
        }
    }

    /// How far (columns) a portal's footprint and clear ground reach from its axis.
    pub fn reach(&self) -> i32 {
        self.reach
    }

    /// The portals whose clear ground reaches columns [x0, x1] × [z0, z1].
    pub fn near(&self, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) -> Vec<Portal> {
        let r = self.reach;
        self.sites
            .iter()
            .copied()
            .filter(|p| p.x + r >= x0 && p.x - r <= x1 && p.z + r >= z0 && p.z - r <= z1)
            .collect()
    }

    /// What column (x, z) is to `portal`.
    pub fn part_of(&self, portal: Portal, x: i32, z: i32) -> Part {
        let (dx, dz) = (i64::from(x - portal.x), i64::from(z - portal.z));
        let d2 = (dx * dx + dz * dz) as f64;
        if d2 <= self.shaft2 {
            let [wx, wz] = self.rules.wall;
            let ladder = (dx, dz) == (i64::from(wx * self.ladder_k), i64::from(wz * self.ladder_k));
            Part::Shaft { ladder }
        } else if d2 <= self.rim2 {
            Part::Rim
        } else if d2 <= self.clear2 {
            Part::Clear
        } else {
            Part::None
        }
    }

    /// What column (x, z) is to the portals in `near` (from [`Self::near`]), and to which.
    pub fn locate(&self, near: &[Portal], x: i32, z: i32) -> (Part, Option<Portal>) {
        near.iter()
            .map(|&p| (self.part_of(p, x, z), Some(p)))
            .find(|&(part, _)| part != Part::None)
            .unwrap_or((Part::None, None))
    }

    /// What column (x, z) is to the portals in `near`.
    pub fn part(&self, near: &[Portal], x: i32, z: i32) -> Part {
        self.locate(near, x, z).0
    }

    /// The column whose ground the ladder climbs out onto: the wall beyond its top.
    pub fn wall_column(&self, portal: Portal) -> (i32, i32) {
        let [wx, wz] = self.rules.wall;
        let k = self.ladder_k + 1;
        (portal.x + wx * k, portal.z + wz * k)
    }

    /// The ladder's column.
    pub fn ladder_column(&self, portal: Portal) -> (i32, i32) {
        let [wx, wz] = self.rules.wall;
        (portal.x + wx * self.ladder_k, portal.z + wz * self.ladder_k)
    }

    /// True when the box [x0, x1] × [z0, z1] reaches any portal's clear ground.
    pub fn box_reaches_clear(&self, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) -> bool {
        self.sites.iter().any(|p| {
            let nx = i64::from(p.x.clamp(x0, x1) - p.x);
            let nz = i64::from(p.z.clamp(z0, z1) - p.z);
            ((nx * nx + nz * nz) as f64) <= self.clear2
        })
    }
}

/// The number of portals a world asks for (the seed's pick in the rules' range).
pub fn wanted(world_seed: u64, rules: &PortalRules) -> u32 {
    let span = rules.count_max - rules.count_min + 1;
    rules.count_min + seed_word(world_seed, 0x9047_A1C0) % span
}

/// The seeded search: points uniform over the `within` disc, each kept when both faces have
/// suitable ground over its footprint and it is `spacing` from the sites kept before.
pub fn search(
    world_seed: u64,
    rules: &PortalRules,
    a: &TerrainGenerator,
    b: &TerrainGenerator,
) -> Vec<Portal> {
    let want = wanted(world_seed, rules) as usize;
    let w = rules.within.floor() as i64;
    let w2 = w * w;
    let spacing = f64::from(rules.spacing);
    let span = (2 * w + 1) as u64;
    let footprint = f64::from(rules.radius) + f64::from(rules.rim_width);
    let f = footprint.ceil() as i32;
    let foot2 = footprint * footprint;
    let mut sites: Vec<Portal> = Vec::new();
    for i in 0..rules.attempts {
        if sites.len() >= want {
            break;
        }
        let hx = seed_word(world_seed, 0x9047_0000u32.wrapping_add(2 * i));
        let hz = seed_word(world_seed, 0x9047_0001u32.wrapping_add(2 * i));
        let x = (u64::from(hx) % span) as i64 - w;
        let z = (u64::from(hz) % span) as i64 - w;
        if x * x + z * z > w2 {
            continue;
        }
        let (x, z) = (x as i32, z as i32);
        let spaced = sites.iter().all(|s| {
            let (dx, dz) = (f64::from(s.x - x), f64::from(s.z - z));
            dx * dx + dz * dz >= spacing * spacing
        });
        if !spaced {
            continue;
        }
        let ground_ok = |g: &TerrainGenerator, px: i32, pz: i32| {
            let c = g.column_at(px, pz);
            !c.outside
                && c.height >= (SEA_LEVEL_M + 1.0) as f32
                && c.height <= SEA_LEVEL_M as f32 + rules.max_height
        };
        // The centre first (cheap rejections), then the coasts, then the whole footprint.
        if !(ground_ok(a, x, z) && ground_ok(b, x, z)) {
            continue;
        }
        if a.coast_at(x, z) < rules.inland || b.coast_at(x, z) < rules.inland {
            continue;
        }
        let all = (-f..=f).all(|dz| {
            (-f..=f).all(|dx| {
                f64::from(dx * dx + dz * dz) > foot2
                    || (ground_ok(a, x + dx, z + dz) && ground_ok(b, x + dx, z + dz))
            })
        });
        if all {
            sites.push(Portal { x, z });
        }
    }
    sites
}

type SiteKey = (u64, PortalRules, u64, bool);

/// The sites searched so far, by (seed, rules, mirror world): a generator is built often (per
/// chunk in some tools), the search once per world.
fn cached_sites(
    world_seed: u64,
    rules: PortalRules,
    map_settings: u64,
    mirrored: bool,
    search: impl FnOnce() -> Vec<Portal>,
) -> Arc<Vec<Portal>> {
    static CACHE: Mutex<Vec<(SiteKey, Arc<Vec<Portal>>)>> = Mutex::new(Vec::new());
    let key = (world_seed, rules, map_settings, mirrored);
    if let Some((_, s)) = CACHE
        .lock()
        .expect("portal sites")
        .iter()
        .find(|(k, _)| *k == key)
    {
        return s.clone();
    }
    let sites = Arc::new(search());
    let mut cache = CACHE.lock().expect("portal sites");
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.push((key, sites.clone()));
    sites
}
