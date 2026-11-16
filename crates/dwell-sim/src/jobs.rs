//! The server's jobs run elsewhere (docs/WATER_SURFACE_GRID.md §2d, the follow-up; ARCHITECTURE.md
//! §5.1): with `ServerConfig::jobs_remote` (the browser's local mode) the chunks the server wants
//! generated (`worldgen_pool.rs`) and its LOD sections to compute (`lod_propagation.rs`) are run by
//! a [`JobRunner`] in the server-jobs worker, not on the tick. A job is bytes (`Server::take_job`),
//! and so is its answer (`Server::deliver_job`).
//!
//! A chunk job is "CJ1" and the chunk's coordinate (x, y, z: i32, little-endian); its answer "CD1",
//! the coordinate and the chunk's 32³ states (u16 each). A sky-floor job (the air test's costly
//! part, `sky_columns.rs`) is "SJ1", its answer "SD1". LOD jobs are `lod_propagation`'s.

use std::sync::Arc;

use dwell_world::{CHUNK_VOLUME, Chunk, ChunkCoord, ChunkGenerator};
use dwell_worldgen::AirColumns;

use crate::lod_propagation::LodRunner;

/// A chunk to generate, as bytes.
pub fn encode_chunk_job(c: ChunkCoord) -> Vec<u8> {
    let mut out = b"CJ1".to_vec();
    for v in c.to_array() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn coord(b: &[u8]) -> Option<ChunkCoord> {
    let mut v = [0i32; 3];
    for (k, x) in v.iter_mut().enumerate() {
        *x = i32::from_le_bytes(b.get(3 + 4 * k..7 + 4 * k)?.try_into().ok()?);
    }
    Some(ChunkCoord::from_array(v))
}

fn decode_chunk_job(b: &[u8]) -> Option<ChunkCoord> {
    (b.len() == 15 && b.starts_with(b"CJ1")).then(|| coord(b))?
}

fn encode_chunk_done(c: ChunkCoord, chunk: &Chunk) -> Vec<u8> {
    let mut out = b"CD1".to_vec();
    for v in c.to_array() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.reserve(CHUNK_VOLUME * 2);
    for m in chunk.voxels().iter() {
        out.extend_from_slice(&m.to_le_bytes());
    }
    out
}

/// A generated chunk's answer: its coordinate and the chunk (None if it isn't one).
pub fn decode_chunk_done(b: &[u8]) -> Option<(ChunkCoord, Box<Chunk>)> {
    if b.len() != 15 + CHUNK_VOLUME * 2 || !b.starts_with(b"CD1") {
        return None;
    }
    let c = coord(b)?;
    let voxels: Vec<u16> = b[15..]
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    Some((c, Box::new(Chunk::from_voxels(&voxels, 0)?)))
}

/// A tree's stage to grow (species index, seed, stage), as bytes: "TJ1" and those (u16, u32, u8,
/// little-endian). Growing a giant's stage takes ~2 ms natively, ~20 in the browser: off the tick
/// (priority 0, the owner's "the server never drops below 60fps").
pub fn encode_tree_job(species: u16, seed: u32, stage: u8) -> Vec<u8> {
    let mut out = b"TJ1".to_vec();
    out.extend(species.to_le_bytes());
    out.extend(seed.to_le_bytes());
    out.push(stage);
    out
}

fn tree_key(b: &[u8]) -> Option<(u16, u32, u8)> {
    Some((
        u16::from_le_bytes(b.get(3..5)?.try_into().ok()?),
        u32::from_le_bytes(b.get(5..9)?.try_into().ok()?),
        *b.get(9)?,
    ))
}

fn decode_tree_job(b: &[u8]) -> Option<(u16, u32, u8)> {
    (b.len() == 10 && b.starts_with(b"TJ1")).then(|| tree_key(b))?
}

fn encode_tree_done(
    species: u16,
    seed: u32,
    stage: u8,
    grown: &dwell_worldgen::species::Grown,
) -> Vec<u8> {
    let mut out = b"TD1".to_vec();
    out.extend(species.to_le_bytes());
    out.extend(seed.to_le_bytes());
    out.push(stage);
    out.extend(grown.to_bytes());
    out
}

/// A grown tree stage's answer: its key and what grew (None if it isn't one).
pub fn decode_tree_done(b: &[u8]) -> Option<((u16, u32, u8), dwell_worldgen::species::Grown)> {
    if !b.starts_with(b"TD1") {
        return None;
    }
    Some((
        tree_key(b)?,
        dwell_worldgen::species::Grown::from_bytes(b.get(10..)?)?,
    ))
}

/// Runs the server's jobs (the server-jobs worker): chunks generated, sky floors and LOD sections
/// computed.
pub struct JobRunner {
    generator: ChunkGenerator,
    air: Arc<AirColumns>,
    lod: LodRunner,
}

impl JobRunner {
    /// For the world of generator `generator_version` and `world_seed`.
    pub fn for_world(generator_version: u32, world_seed: u64) -> Self {
        Self::for_world_sized(
            generator_version,
            world_seed,
            dwell_protocol::world_size::WorldSize::DEFAULT,
        )
    }

    /// [`JobRunner::for_world`] in a world of `size` (BIG_MAP.md M2).
    pub fn for_world_sized(
        generator_version: u32,
        world_seed: u64,
        size: dwell_protocol::world_size::WorldSize,
    ) -> Self {
        Self {
            air: AirColumns::for_generator_sized(generator_version, world_seed, size),
            ..Self::new(
                dwell_worldgen::generator_for_sized(generator_version, world_seed, size),
                dwell_worldgen::lod_generator_for_sized(generator_version, world_seed, size),
            )
        }
    }

    /// For a generator of its own (a server with `generator_override`, which wants no sky floors).
    pub fn new(generator: ChunkGenerator, lod: dwell_world::lod::LodGenerator) -> Self {
        Self {
            generator,
            air: AirColumns::for_generator(dwell_world::GENERATOR_FLAT, 0),
            lod: LodRunner::new(lod),
        }
    }

    /// A job's bytes run; its answer's (None if the job can't be read).
    pub fn run(&mut self, job: &[u8]) -> Option<Vec<u8>> {
        if let Some(c) = decode_chunk_job(job) {
            let mut chunk = Chunk::default();
            (self.generator)(c, &mut chunk);
            return Some(encode_chunk_done(c, &chunk));
        }
        if let Some(col) = crate::sky_columns::decode_job(job) {
            return Some(crate::sky_columns::encode_done(col, self.air.compute(col)));
        }
        if let Some((species, seed, stage)) = decode_tree_job(job) {
            let s = dwell_worldgen::species::SPECIES.get(usize::from(species))?;
            let grown = dwell_worldgen::species::grow(s, seed, u32::from(stage));
            return Some(encode_tree_done(species, seed, stage, &grown));
        }
        self.lod.run(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_job_round_trips_to_the_same_grown_stage() {
        // face-a-only: an encoding; a tree's cells are face-local.
        let species = dwell_worldgen::species::index("dwell:oak").expect("oak") as u16;
        let s = &dwell_worldgen::species::SPECIES[usize::from(species)];
        let mut r = JobRunner::new(
            dwell_world::flat_generator(),
            std::sync::Arc::new(dwell_world::lod::generate_flat_lod),
        );
        let done = r.run(&encode_tree_job(species, 77, 12)).expect("an answer");
        let (key, grown) = decode_tree_done(&done).expect("a tree");
        assert_eq!(key, (species, 77, 12));
        assert_eq!(grown, dwell_worldgen::species::grow(s, 77, 12));
    }

    #[test]
    fn a_chunk_job_round_trips() {
        // face-a-only: an encoding; chunks are world volumes.
        let c = ChunkCoord::new(-3, 7, 2);
        let mut r = JobRunner::new(
            dwell_world::flat_generator(),
            std::sync::Arc::new(dwell_world::lod::generate_flat_lod),
        );
        let done = r.run(&encode_chunk_job(c)).expect("an answer");
        let (at, chunk) = decode_chunk_done(&done).expect("a chunk");
        assert_eq!(at, c);
        let mut want = Chunk::default();
        dwell_world::generate_flat_chunk(c, &mut want);
        assert!(chunk.voxels() == want.voxels());
        assert!(r.run(b"CJ1").is_none(), "a short job is refused");
        assert!(decode_chunk_done(&done[..20]).is_none());
    }
}
