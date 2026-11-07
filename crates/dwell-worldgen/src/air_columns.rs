//! The generator's all-air test with its costly part apart (ARCHITECTURE.md §6.3): whether a chunk
//! is all air is cheap once its chunk column's sky floor is known, and the sky floor costs a
//! chunk's column heights (~1 ms natively, a few in the browser). The server asks `known` on its
//! tick, and has the columns it lacks computed elsewhere (`compute`, off the tick or within a
//! budget) and `insert`ed; [`AirColumns::test`] is the plain test that computes what it needs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dwell_world::{
    AirChunkTest, CHUNK_SIZE, ChunkCoord, DiscOverlap, Face, SEA_LEVEL, chunk_disc_overlap,
    face_local_chunk, face_of_chunk_y, flat_is_air,
};

use crate::BifacialTerrain;
use crate::terrain;

/// A chunk column of a face: what one sky floor is computed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkyColumn {
    pub x: i32,
    pub z: i32,
    pub face: Face,
}

/// Sky floors kept (one f32 each); past this many the cache starts again.
const MAX_CACHED: usize = 16384;

/// The air test of a generator, its sky floors cached per chunk column and face.
pub struct AirColumns {
    terrain: Option<Arc<BifacialTerrain>>,
    cache: Mutex<HashMap<SkyColumn, f32>>,
}

impl AirColumns {
    /// For a generator version: the terrain's (sky floors), or the flat world's (none needed).
    pub fn for_generator(version: u32, world_seed: u64) -> Arc<Self> {
        Arc::new(Self {
            terrain: BifacialTerrain::for_generator(version, world_seed).map(Arc::new),
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// The column whose sky floor chunk `c`'s test needs (None: it needs none).
    fn column_of(&self, c: ChunkCoord) -> Option<SkyColumn> {
        self.terrain.as_ref()?;
        let y0 = face_local_chunk(c).y * CHUNK_SIZE;
        if !(SEA_LEVEL..terrain::FACE_MAX_Y).contains(&y0)
            || chunk_disc_overlap(c.x, c.z) == DiscOverlap::Outside
        {
            return None;
        }
        Some(SkyColumn {
            x: c.x,
            z: c.z,
            face: face_of_chunk_y(c.y),
        })
    }

    /// Whether chunk `c` is all air, without computing: `Err` with the column whose sky floor it
    /// waits for.
    pub fn known(&self, c: ChunkCoord) -> Result<bool, SkyColumn> {
        let Some(terrain) = &self.terrain else {
            return Ok(flat_is_air(c));
        };
        let Some(col) = self.column_of(c) else {
            return Ok(terrain.is_air_chunk(c));
        };
        match self.cache.lock().expect("air cache").get(&col) {
            Some(&floor) => Ok(BifacialTerrain::is_air_chunk_with(c, floor)),
            None => Err(col),
        }
    }

    /// A column's sky floor (the costly part; the cache untouched).
    pub fn compute(&self, col: SkyColumn) -> f32 {
        self.terrain
            .as_ref()
            .map_or(0.0, |t| t.sky_floor_at(col.x, col.z, col.face))
    }

    /// A sky floor computed elsewhere, kept.
    pub fn insert(&self, col: SkyColumn, floor: f32) {
        let mut cache = self.cache.lock().expect("air cache");
        if cache.len() >= MAX_CACHED && !cache.contains_key(&col) {
            cache.clear();
        }
        cache.insert(col, floor);
    }

    /// Whether chunk `c` is all air, computing its column's sky floor if it isn't known.
    pub fn is_air(&self, c: ChunkCoord) -> bool {
        match self.known(c) {
            Ok(air) => air,
            Err(col) => {
                let floor = self.compute(col);
                self.insert(col, floor);
                BifacialTerrain::is_air_chunk_with(c, floor)
            }
        }
    }

    /// The plain air test, sharing this cache.
    pub fn test(self: &Arc<Self>) -> AirChunkTest {
        let me = self.clone();
        Arc::new(move |c| me.is_air(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::GENERATOR_TERRAIN;

    dwell_world::both_faces!(
        /// A chunk's answer is known once its column's sky floor is inserted, and it is the plain
        /// test's.
        fn a_column_computed_elsewhere_answers_as_the_plain_test(f: FaceFrame) {
            let air = AirColumns::for_generator(GENERATOR_TERRAIN, 0);
            let plain = AirColumns::for_generator(GENERATOR_TERRAIN, 0);
            let cx = 2364 / CHUNK_SIZE;
            let cz = 7443 / CHUNK_SIZE;
            let mut asked = 0;
            for local_cy in (SEA_LEVEL / CHUNK_SIZE)..(terrain::FACE_MAX_Y / CHUNK_SIZE) {
                let c = f.chunk(ChunkCoord::new(cx, local_cy, cz));
                match air.known(c) {
                    Ok(a) => assert_eq!(a, plain.is_air(c), "{c:?}"),
                    Err(col) => {
                        asked += 1;
                        assert_eq!(col.face, f.face);
                        air.insert(col, air.compute(col));
                        assert_eq!(air.known(c), Ok(plain.is_air(c)), "{c:?}");
                    }
                }
            }
            assert_eq!(asked, 1, "one column's sky floor for the whole column");
        }
    );
}
