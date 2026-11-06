//! The server's jobs run elsewhere (docs/WATER_SURFACE_GRID.md §2d, the follow-up; ARCHITECTURE.md
//! §5.1): with `ServerConfig::jobs_remote` (the browser's local mode) the chunks the server wants
//! generated (`worldgen_pool.rs`) and its LOD sections to compute (`lod_propagation.rs`) are run by
//! a [`JobRunner`] in the server-jobs worker, not on the tick. A job is bytes (`Server::take_job`),
//! and so is its answer (`Server::deliver_job`).
//!
//! A chunk job is "CJ1" and the chunk's coordinate (x, y, z: i32, little-endian); its answer "CD1",
//! the coordinate and the chunk's 32³ states (u16 each). LOD jobs are `lod_propagation`'s.

use dwell_world::{CHUNK_VOLUME, Chunk, ChunkCoord, ChunkGenerator};

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

/// Runs the server's jobs (the server-jobs worker): chunks generated, LOD sections computed.
pub struct JobRunner {
    generator: ChunkGenerator,
    lod: LodRunner,
}

impl JobRunner {
    /// For the world of generator `generator_version` and `world_seed`.
    pub fn for_world(generator_version: u32, world_seed: u64) -> Self {
        Self::new(
            dwell_worldgen::generator_for(generator_version, world_seed),
            dwell_worldgen::lod_generator_for(generator_version, world_seed),
        )
    }

    pub fn new(generator: ChunkGenerator, lod: dwell_world::lod::LodGenerator) -> Self {
        Self {
            generator,
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
        self.lod.run(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
