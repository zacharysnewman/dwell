//! The terrain renderer's data side (ARCHITECTURE.md §5): what the game hands the terrain — each
//! chunk's and LOD section's meshes, the sections shown and their stand-ins — while the camera,
//! players, outline, lines, stats and the shared frame (`frame.rs`) are the renderer's common
//! code. Platform free (no GPU types), so the game's terrain handling is unit-tested with
//! [`Recording`].

use dwell_lod::fade::Fade;
use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_world::ChunkCoord;

use super::StandIn;

/// The terrain's data side: the mesher's chunk and section meshes (its GPU drawing is the
/// renderer's scene passes, into the shared frame's targets). Removing is `None`.
pub trait TerrainRenderer {
    /// Adds, replaces or (`None`) removes a chunk; meshes are chunk-local.
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkMeshes>);
    /// Adds, replaces or (`None`) removes an LOD section, in cells of `cell_size` from `origin`.
    fn set_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        payload: Option<&SectionMeshes>,
    );
    /// The sections to draw this frame, with the sides (bit per face) whose skirt shows and their
    /// fade (dwell-lod fade.rs: dithered while a swap between levels cross-fades them).
    fn show_sections(&mut self, visible: &[(i64, u8, Fade)]);
    /// Ancestor sections drawn clipped to boxes this frame.
    fn show_stand_ins(&mut self, stand_ins: &[StandIn]);
    /// Drops everything it holds.
    fn clear(&mut self);
    /// GPU memory its terrain holds (bytes).
    fn gpu_bytes(&self) -> u64;
}

/// A terrain renderer that records what it is given (tests).
#[derive(Debug, Default)]
pub struct Recording {
    pub chunks: std::collections::BTreeMap<ChunkCoord, ChunkMeshes>,
    pub sections: std::collections::BTreeMap<i64, SectionMeshes>,
    pub shown: Vec<(i64, u8, Fade)>,
    pub clears: u32,
}

impl TerrainRenderer for Recording {
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkMeshes>) {
        match payload {
            Some(p) => self.chunks.insert(coord, p.clone()),
            None => self.chunks.remove(&coord),
        };
    }
    fn set_section(
        &mut self,
        id: i64,
        _origin: [f64; 3],
        _cell: f64,
        payload: Option<&SectionMeshes>,
    ) {
        match payload {
            Some(p) => self.sections.insert(id, p.clone()),
            None => self.sections.remove(&id),
        };
    }
    fn show_sections(&mut self, visible: &[(i64, u8, Fade)]) {
        self.shown = visible.to_vec();
    }
    fn show_stand_ins(&mut self, _stand_ins: &[StandIn]) {}
    fn clear(&mut self) {
        self.chunks.clear();
        self.sections.clear();
        self.clears += 1;
    }
    fn gpu_bytes(&self) -> u64 {
        (self.chunks.len() + self.sections.len()) as u64
    }
}
