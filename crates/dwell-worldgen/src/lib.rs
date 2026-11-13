//! Dwell's terrain (ARCHITECTURE.md §6.3): deterministic noise, the interim island layout, the
//! height-field generator of Phase 15 and the bifacial world built from two of them, plus the
//! generator lookup by version that the server, the client and the tools share.

pub mod air_columns;
pub mod arches;
pub mod base;
pub mod bifacial;
pub mod biomemap;
pub mod climate;
pub mod config;
pub mod decorations;
pub mod geology;
pub mod grass;
pub mod grid;
pub mod grids;
pub mod growth;
pub mod home_far;
pub mod hydrology;
pub mod impostor;
pub mod island;
pub mod map_layers;
pub mod noise;
pub mod placements;
pub mod plate_tectonics;
pub mod plates;
pub mod portals;
pub mod progress;
pub mod ramps;
pub mod shrines;
pub mod species;
pub mod tectonics;
pub mod terrain;
pub mod variety;
pub mod vegetation;
pub mod volcanic;
pub mod volume;
pub mod worldmap;

use std::sync::Arc;

use dwell_protocol::world_size::WorldSize;

use dwell_world::lod::{self, LodBoundsFn, LodGenerator};
use dwell_world::{
    AirChunkTest, ChunkGenerator, FlatLayers, GENERATOR_SANDBOX, flat_generator, flat_generator_of,
};

pub use air_columns::{AirColumns, SkyColumn};
pub use bifacial::BifacialTerrain;
pub use terrain::TerrainGenerator;

/// The block the structure sandbox (`GENERATOR_SANDBOX`) is made of, by name (block data
/// `"indestructible": true`, so its ground stays to build on).
pub const SANDBOX_FLOOR: &str = "dwell:sandbox_floor";

/// The sandbox's layers: the flat world, all floor block.
pub fn sandbox_layers() -> FlatLayers {
    FlatLayers::of(dwell_blocks::require(SANDBOX_FLOOR))
}

/// The chunk generator for a generator version; unknown versions fall back to the flat world. The
/// sandbox has the flat world's shape (its air test, spawn and LOD bounds) in its floor block.
pub fn generator_for(version: u32, world_seed: u64) -> ChunkGenerator {
    generator_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`generator_for`] in a world of `size` (BIG_MAP.md M2).
pub fn generator_for_sized(version: u32, world_seed: u64, size: WorldSize) -> ChunkGenerator {
    if let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) {
        let terrain = Arc::new(terrain);
        return Arc::new(move |coord, chunk| terrain.generate(coord, chunk));
    }
    if version == GENERATOR_SANDBOX {
        return flat_generator_of(sandbox_layers());
    }
    flat_generator()
}

/// A chunk's ground densities from its biomes (ADR 0074 B5; `dwell_world::ground::GroundDensity`).
pub type GroundDensityFn =
    Arc<dyn Fn(dwell_world::ChunkCoord) -> dwell_world::ground::GroundDensity + Send + Sync>;

/// The ground densities of a generator version's world (ADR 0074 B5): none where no biome of
/// either face's worldgen has a `ground` (the default world: nothing is built, and the scatter is
/// as before). The terrain is built only when one does.
pub fn ground_density_for(version: u32, world_seed: u64) -> Option<GroundDensityFn> {
    ground_density_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`ground_density_for`] in a world of `size` (BIG_MAP.md M2).
pub fn ground_density_for_sized(
    version: u32,
    world_seed: u64,
    size: WorldSize,
) -> Option<GroundDensityFn> {
    if version != dwell_world::GENERATOR_TERRAIN {
        return None;
    }
    let any = [dwell_world::Face::A, dwell_world::Face::B]
        .into_iter()
        .any(|f| {
            match &worldmap::cache::for_seed_sized(bifacial::face_seed(world_seed, f), size)
                .worldgen
            {
                Some(w) => w.edit.biomes.iter().any(|b| b.ground.is_some()),
                None => terrain::BIOMES.iter().any(|b| b.ground.is_some()),
            }
        });
    if !any {
        return None;
    }
    let t = Arc::new(BifacialTerrain::for_generator_sized(
        version, world_seed, size,
    )?);
    t.ground_varies()
        .then(|| Arc::new(move |c| t.ground_density(c)) as GroundDensityFn)
}

/// The generator's all-air test. The terrain's caches its sky floors per chunk column and face
/// (bounded: cleared when large); `AirColumns` gives the server its costly part apart.
pub fn air_test_for(version: u32, world_seed: u64) -> AirChunkTest {
    air_test_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`air_test_for`] in a world of `size` (BIG_MAP.md M2).
pub fn air_test_for_sized(version: u32, world_seed: u64, size: WorldSize) -> AirChunkTest {
    AirColumns::for_generator_sized(version, world_seed, size).test()
}

/// Face B's spawn point for the terrain generator (`BifacialTerrain::spawn_point_b`); the face-A
/// one for the others (they have no face B).
pub fn spawn_point_b_for(version: u32, world_seed: u64) -> [f64; 3] {
    spawn_point_b_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`spawn_point_b_for`] in a world of `size` (BIG_MAP.md M2).
pub fn spawn_point_b_for_sized(version: u32, world_seed: u64, size: WorldSize) -> [f64; 3] {
    if let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) {
        return terrain.spawn_point_b();
    }
    spawn_point_for_sized(version, world_seed, size)
}

/// The portals' axes (world x, z through each shaft's axis column's centre) and the look their
/// motes take (`midplane.json` `portals.motes`): none for a generator without portals. The client
/// draws the motes from these (BIFACIAL_WORLD.md §12.1).
pub fn portal_axes_for(version: u32, world_seed: u64) -> (Vec<[f64; 2]>, &'static str) {
    portal_axes_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`portal_axes_for`] in a world of `size` (BIG_MAP.md M2).
pub fn portal_axes_for_sized(
    version: u32,
    world_seed: u64,
    size: WorldSize,
) -> (Vec<[f64; 2]>, &'static str) {
    let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) else {
        return (Vec::new(), "");
    };
    let p = terrain.portals();
    let axes = p
        .sites
        .iter()
        .map(|s| [f64::from(s.x) + 0.5, f64::from(s.z) + 0.5])
        .collect();
    (axes, p.rules.motes)
}

/// The world's portal shafts as the player controller sees them (BIFACIAL_WORLD.md §12.3): each
/// one's axis, radius and opening (the rim's top, face-local) on both faces, with the exit push
/// (`midplane.json` `portals.exit`). Empty for a generator without portals. The server and the
/// client's prediction both set it on their `VoxelWorld` (`portals`), so the push agrees.
pub fn portal_shafts_for_sized(
    version: u32,
    world_seed: u64,
    size: WorldSize,
) -> dwell_world::portal_shafts::PortalShafts {
    let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) else {
        return Default::default();
    };
    let p = terrain.portals();
    let top = |face, s| dwell_world::m_of_row(terrain.local(face).ladder_top(p, s) + 1);
    let shafts = p
        .sites
        .iter()
        .map(|&s| dwell_world::portal_shafts::Shaft {
            axis: [f64::from(s.x) + 0.5, f64::from(s.z) + 0.5],
            radius: f64::from(p.rules.radius),
            top: [top(dwell_world::Face::A, s), top(dwell_world::Face::B, s)],
        })
        .collect();
    dwell_world::portal_shafts::PortalShafts::new(shafts, p.rules.exit)
}

/// A dev spawn by a portal (`?spawn=portal`, BIFACIAL_WORLD.md §6): on face `face`'s rim over the
/// ladder of the portal nearest the disc's centre, the shaft in front. `None` for a generator
/// without portals.
pub fn portal_spawn_for(
    version: u32,
    world_seed: u64,
    face: dwell_world::Face,
) -> Option<[f64; 3]> {
    let terrain = BifacialTerrain::for_generator(version, world_seed)?;
    let p = terrain.portals();
    let portal = *p
        .sites
        .iter()
        .min_by_key(|s| i64::from(s.x).pow(2) + i64::from(s.z).pow(2))?;
    let (x, z) = p.wall_column(portal);
    let top = terrain.local(face).ladder_top(p, portal);
    let feet = dwell_world::m_of_row(top + 1);
    Some([
        f64::from(x) + 0.5,
        dwell_world::FaceFrame::of(face).y(feet),
        f64::from(z) + 0.5,
    ])
}

/// Where a player who died with its feet at a world point stands up again (docs/SHRINES.md): the
/// shrine nearest the death on the death's face, as world feet; `None` with none in the search.
pub type ShrineRespawn = Arc<dyn Fn([f64; 3]) -> Option<[f64; 3]> + Send + Sync>;

/// The shrine respawn of a generator version's world; `None` for a generator without shrines (the
/// flat world, the sandbox).
pub fn shrine_respawn_for_sized(
    version: u32,
    world_seed: u64,
    size: WorldSize,
) -> Option<ShrineRespawn> {
    let terrain = Arc::new(BifacialTerrain::for_generator_sized(
        version, world_seed, size,
    )?);
    Some(Arc::new(move |[x, y, z]: [f64; 3]| {
        let face = dwell_world::FaceFrame::at_height(y).face;
        terrain.nearest_shrine(face, x, z).map(|(_, feet)| feet)
    }))
}

/// How far a town's footprint keeps from a shrine's clear ground (m).
const TOWN_SHRINE_MARGIN_M: i32 = 8;

/// The ground a town site needs (docs/BLUEPRINTS.md §6): at a face's face-local column, the ground's
/// height (face-local m) where it is dry land (no sea, lake, river or stream, inside the world) within
/// `blueprints.json` `towns.heightM` above sea level, clear of the shrines and the portals, else None.
pub type TownGround = Arc<dyn Fn(dwell_world::Face, i32, i32) -> Option<f64> + Send + Sync>;

/// The town ground of a generator version's world; `None` for one without a height field (the flat
/// world, the sandbox): it has no towns.
pub fn town_ground_for_sized(version: u32, world_seed: u64, size: WorldSize) -> Option<TownGround> {
    let terrain = Arc::new(BifacialTerrain::for_generator_sized(
        version, world_seed, size,
    )?);
    let [lo, hi] = dwell_world::blueprint::SHIPPED.towns.height_m;
    let sea = dwell_world::SEA_LEVEL_M;
    Some(Arc::new(move |face, x, z| {
        let g = terrain.local(face);
        let c = g.column_at(x, z);
        let h = f64::from(c.height);
        let dry = !c.outside
            && c.water == terrain::Water::None
            && !c.stream
            && h >= sea + lo
            && h <= sea + hi;
        // Clear of the resurrection shrines (their clear ground, and a margin) and the portals.
        (dry && !g
            .shrines()
            .is_some_and(|sh| sh.crowds(g, x, z, TOWN_SHRINE_MARGIN_M)))
        .then_some(h)
    }))
}

/// Feet position players spawn at: near the origin, on open level ground.
pub fn spawn_point_for(version: u32, world_seed: u64) -> [f64; 3] {
    spawn_point_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`spawn_point_for`] in a world of `size` (BIG_MAP.md M2).
pub fn spawn_point_for_sized(version: u32, world_seed: u64, size: WorldSize) -> [f64; 3] {
    if let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) {
        return terrain.spawn_point();
    }
    // The flat world's (or the sandbox's) surface: the top of its top layer (row FLAT_TOP_Y − 1),
    // as its shape gives it, so the spawn is on the ground, not inside it.
    let mut flat = dwell_world::VoxelWorld::new(
        generator_for_sized(version, world_seed, size),
        Some(air_test_for_sized(version, world_seed, size)),
    );
    let top_y = dwell_world::FLAT_TOP_Y - 1;
    let top = flat.shape_at(0, top_y, 0);
    let feet = dwell_world::m_of_row(top_y)
        + f64::from(dwell_blocks::shape::solid_span_at(top, 0.5, 0.5).hi) * dwell_world::CELL_H;
    [0.5, feet, 0.5]
}

/// The world map's flow through a column (m³/s; docs/WATER_FLOW.md §2.1), by face and face-local
/// x, z: what feeds the water flow's basins. `None` for a generator without a world map (the flat
/// world, the sandbox): their water has no flow of its own.
pub type Runoff = Arc<dyn Fn(dwell_world::Face, i32, i32) -> f64 + Send + Sync>;

pub fn runoff_for(version: u32, world_seed: u64) -> Option<Runoff> {
    runoff_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`runoff_for`] in a world of `size` (BIG_MAP.md M2).
pub fn runoff_for_sized(version: u32, world_seed: u64, size: WorldSize) -> Option<Runoff> {
    let terrain = Arc::new(BifacialTerrain::for_generator_sized(
        version, world_seed, size,
    )?);
    Some(Arc::new(move |face, x, z| {
        terrain.local(face).discharge_at(x, z)
    }))
}

pub fn lod_generator_for(version: u32, world_seed: u64) -> LodGenerator {
    lod_generator_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`lod_generator_for`] in a world of `size` (BIG_MAP.md M2).
pub fn lod_generator_for_sized(version: u32, world_seed: u64, size: WorldSize) -> LodGenerator {
    if let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) {
        let terrain = Arc::new(terrain);
        return Arc::new(move |c, cells| terrain.generate_lod(c, cells, None, None));
    }
    if version == GENERATOR_SANDBOX {
        let layers = sandbox_layers();
        return Arc::new(move |c, cells| lod::generate_flat_lod_of(&layers, c, cells));
    }
    Arc::new(lod::generate_flat_lod)
}

pub fn lod_bounds_for(version: u32, world_seed: u64) -> LodBoundsFn {
    lod_bounds_for_sized(version, world_seed, WorldSize::DEFAULT)
}

/// [`lod_bounds_for`] in a world of `size` (BIG_MAP.md M2).
pub fn lod_bounds_for_sized(version: u32, world_seed: u64, size: WorldSize) -> LodBoundsFn {
    if let Some(terrain) = BifacialTerrain::for_generator_sized(version, world_seed, size) {
        let terrain = Arc::new(terrain);
        return Arc::new(move |level, i, k| terrain.lod_bounds_at(level, i, k));
    }
    Arc::new(lod::flat_lod_bounds)
}

/// The bytes a vector's allocation holds (`BiomeSet::grid_bytes`).
pub(crate) fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * std::mem::size_of::<T>()
}
