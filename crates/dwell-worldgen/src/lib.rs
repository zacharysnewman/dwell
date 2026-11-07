//! Dwell's terrain (ARCHITECTURE.md §6.3): deterministic noise, the interim island layout, the
//! height-field generator of Phase 15 and the bifacial world built from two of them, plus the
//! generator lookup by version that the server, the client and the tools share.

pub mod air_columns;
pub mod base;
pub mod bifacial;
pub mod decorations;
pub mod hydrology;
pub mod island;
pub mod noise;
pub mod placements;
pub mod portals;
pub mod ramps;
pub mod terrain;
pub mod volume;
pub mod worldmap;

use std::sync::Arc;

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
    if let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) {
        let terrain = Arc::new(terrain);
        return Arc::new(move |coord, chunk| terrain.generate(coord, chunk));
    }
    if version == GENERATOR_SANDBOX {
        return flat_generator_of(sandbox_layers());
    }
    flat_generator()
}

/// The generator's all-air test. The terrain's caches its sky floors per chunk column and face
/// (bounded: cleared when large); `AirColumns` gives the server its costly part apart.
pub fn air_test_for(version: u32, world_seed: u64) -> AirChunkTest {
    AirColumns::for_generator(version, world_seed).test()
}

/// Face B's spawn point for the terrain generator (`BifacialTerrain::spawn_point_b`); the face-A
/// one for the others (they have no face B).
pub fn spawn_point_b_for(version: u32, world_seed: u64) -> [f64; 3] {
    if let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) {
        return terrain.spawn_point_b();
    }
    spawn_point_for(version, world_seed)
}

/// The portals' axes (world x, z through each shaft's axis column's centre) and the look their
/// motes take (`midplane.json` `portals.motes`): none for a generator without portals. The client
/// draws the motes from these (BIFACIAL_WORLD.md §12.1).
pub fn portal_axes_for(version: u32, world_seed: u64) -> (Vec<[f64; 2]>, &'static str) {
    let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) else {
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

/// Feet position players spawn at: near the origin, on open level ground.
pub fn spawn_point_for(version: u32, world_seed: u64) -> [f64; 3] {
    if let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) {
        return terrain.spawn_point();
    }
    // The flat world's (or the sandbox's) surface: the top of its top layer (row FLAT_TOP_Y − 1),
    // as its shape gives it, so the spawn is on the ground, not inside it.
    let mut flat = dwell_world::VoxelWorld::new(
        generator_for(version, world_seed),
        Some(air_test_for(version, world_seed)),
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
    let terrain = Arc::new(BifacialTerrain::for_generator(version, world_seed)?);
    Some(Arc::new(move |face, x, z| {
        terrain.local(face).discharge_at(x, z)
    }))
}

pub fn lod_generator_for(version: u32, world_seed: u64) -> LodGenerator {
    if let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) {
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
    if let Some(terrain) = BifacialTerrain::for_generator(version, world_seed) {
        let terrain = Arc::new(terrain);
        return Arc::new(move |level, i, k| terrain.lod_bounds_at(level, i, k));
    }
    Arc::new(lod::flat_lod_bounds)
}
