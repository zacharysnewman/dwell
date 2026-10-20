//! Dwell's terrain (ARCHITECTURE.md §6.3): deterministic noise, the interim island layout, the
//! height-field generator of Phase 15 and the bifacial world built from two of them, plus the
//! generator lookup by version that the server, the client and the tools share.

pub mod bifacial;
pub mod decorations;
pub mod island;
pub mod noise;
pub mod ramps;
pub mod terrain;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dwell_world::lod::{self, LodBoundsFn, LodGenerator};
use dwell_world::{
    AirChunkTest, CHUNK_SIZE, ChunkCoord, ChunkGenerator, DiscOverlap, FlatLayers,
    GENERATOR_SANDBOX, GENERATOR_TERRAIN, SEA_LEVEL, WORLD_MAX_Y, chunk_disc_overlap,
    face_local_chunk, face_of_chunk_y, flat_generator, flat_generator_of, flat_is_air,
};

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
    if version == GENERATOR_TERRAIN {
        let terrain = Arc::new(BifacialTerrain::new(world_seed));
        return Arc::new(move |coord, chunk| terrain.generate(coord, chunk));
    }
    if version == GENERATOR_SANDBOX {
        return flat_generator_of(sandbox_layers());
    }
    flat_generator()
}

/// The generator's all-air test. The terrain's caches its sky floors per chunk column and face
/// (bounded: cleared when large).
pub fn air_test_for(version: u32, world_seed: u64) -> AirChunkTest {
    if version != GENERATOR_TERRAIN {
        return Arc::new(flat_is_air);
    }
    let terrain = BifacialTerrain::new(world_seed);
    let cache: Mutex<HashMap<(i32, i32, u8), f32>> = Mutex::new(HashMap::new());
    Arc::new(move |coord: ChunkCoord| {
        let face = face_of_chunk_y(coord.y);
        let y0 = face_local_chunk(coord).y * CHUNK_SIZE;
        if !(SEA_LEVEL..WORLD_MAX_Y).contains(&y0)
            || chunk_disc_overlap(coord.x, coord.z) == DiscOverlap::Outside
        {
            return terrain.is_air_chunk(coord);
        }
        let key = (coord.x, coord.z, face as u8);
        let mut cache = cache.lock().expect("air cache");
        if cache.len() >= 4096 && !cache.contains_key(&key) {
            cache.clear();
        }
        let floor = *cache
            .entry(key)
            .or_insert_with(|| terrain.sky_floor_at(coord.x, coord.z, face));
        BifacialTerrain::is_air_chunk_with(coord, floor)
    })
}

/// Feet position players spawn at: near the origin, on open level ground.
pub fn spawn_point_for(version: u32, world_seed: u64) -> [f64; 3] {
    if version == GENERATOR_TERRAIN {
        return BifacialTerrain::new(world_seed).spawn_point();
    }
    // The flat world's surface: its top layer (y = −1) as the adaptive rules shape it — a level
    // field of natural blocks rests at half height (ADAPTIVE_BLOCKS.md).
    let mut flat = dwell_world::VoxelWorld::flat();
    let top = flat.shape_at(0, -1, 0);
    let feet = dwell_world::m_of_row(-1)
        + f64::from(dwell_blocks::shape::solid_span_at(top, 0.5, 0.5).hi) * dwell_world::CELL_H;
    [0.5, feet, 0.5]
}

pub fn lod_generator_for(version: u32, world_seed: u64) -> LodGenerator {
    if version == GENERATOR_TERRAIN {
        let terrain = Arc::new(BifacialTerrain::new(world_seed));
        return Arc::new(move |c, cells| terrain.generate_lod(c, cells, None, None));
    }
    if version == GENERATOR_SANDBOX {
        let layers = sandbox_layers();
        return Arc::new(move |c, cells| lod::generate_flat_lod_of(&layers, c, cells));
    }
    Arc::new(lod::generate_flat_lod)
}

pub fn lod_bounds_for(version: u32, world_seed: u64) -> LodBoundsFn {
    if version == GENERATOR_TERRAIN {
        let terrain = Arc::new(BifacialTerrain::new(world_seed));
        return Arc::new(move |level, i, k| terrain.lod_bounds_at(level, i, k));
    }
    Arc::new(lod::flat_lod_bounds)
}
