//! Dwell's meshers (ARCHITECTURE.md §5, §6.1, §6.6), ported from the TypeScript client
//! (`client/src/mesh/mesher.ts`, `lodMesher.ts`; ADR 0028, Phase 24e). Pure data: no threads,
//! clocks, I/O or GPU, so the same code runs in the meshing workers, natively and in tests, and
//! builds for `wasm32-unknown-unknown`.
//!
//! Blocks are looked up in the generated registry (`dwell-blocks`) by state id, and the mesher only
//! ever names one block itself (`dwell:water`, for flooded cells), by name.
//!
//! # Chunk meshes ([`mesh_chunk`])
//!
//! Input: a chunk's voxels with a one-voxel apron, [`PADDED_VOLUME`] state ids in
//! [`padded_index`] order (x fastest, then z, then y). Output: [`ChunkMeshes`], an opaque and a
//! transparent [`MeshArrays`], positions chunk-local in blocks (0..32). Per vertex, in separate
//! tightly packed `f32` streams:
//!
//! | stream      | floats | meaning                                                             |
//! |-------------|--------|---------------------------------------------------------------------|
//! | `positions` | 3      | x, y, z (chunk-local blocks)                                         |
//! | `normals`   | 3      | unit normal                                                         |
//! | `colors`    | 3      | face tint × flat colour (sRGB 0–1; white on textured faces)          |
//! | `uvs`       | 2      | texture coordinates in blocks (the shader repeats the tile per unit) |
//! | `tiles`     | 4      | atlas rectangle: u0, v0, width, height ([`tile_rect`])              |
//!
//! The transparent mesh adds `fluids` (1 per vertex): the id of the fluid whose look the face is
//! drawn with (an index into the registry's fluid table, `dwell_blocks::FLUIDS`), or [`NO_FLUID`]
//! for a see-through face that is no fluid's; the opaque mesh leaves it empty.
//!
//! `indices` (`u32`) are triangles, counter-clockwise seen from the side the normal points to.
//! Merged faces are quads of 4 vertices and 6 indices (0 1 2, 0 2 3 for a +axis face, 0 2 1,
//! 0 3 2 for −axis); shaped polygons are fans. The chunk shader samples
//! `tile.xy + fract(uv) × tile.zw` in the atlas of [`build_atlas`].
//!
//! # LOD meshes ([`mesh_section`])
//!
//! Input: a section's 34³ cells (its 32³ and an apron, `dwell_world::lod::cell_index` order) and
//! optional per-column surface data ([`MeshSectionOptions`]). Output: [`SectionMeshes`], each a
//! [`FlatMesh`] with `positions` (3), `normals` (3) and `colors` (3, **linear** light: face tint ×
//! the material's average texture colour) per vertex and `u32` triangle indices, positions in
//! cells (0..32); the `water` mesh adds `fluids` (1 per vertex, as the chunks' transparent mesh). `skirts[f]` holds the border faces the apron hides on face `f` (0 +X, 1 −X, 2 +Y,
//! 3 −Y, 4 +Z, 5 −Z), shown when the neighbour there is not drawn at the same level.

mod lod_mesher;
mod mesher;
mod style;
mod textures;

pub use dwell_look::{Rgb, face_tint, normal_tint};
pub use lod_mesher::{
    FlatMesh, MeshSectionOptions, SIDE_SHARE_MAX, SIDE_SHARE_PER_GRADIENT, SURFACE_STEPS,
    SURFACE_STRIDE, SectionMeshes, lod_color, mesh_section, mirror_rows, section_bytes,
};
pub use mesher::{
    ChunkMeshes, MeshArrays, PAD, PADDED_VOLUME, face_covered, mesh_chunk, mesh_chunk_at,
    mirror_padded, padded_index,
};
/// The `fluids` stream's value for a see-through face that is not a fluid's.
pub const NO_FLUID: f32 = 255.0;

pub use textures::{
    ATLAS_CELLS, ATLAS_SIZE, ATLAS_TILES, CELL, GUTTER, TILE, TILE_ORDER, TileRect,
    average_tile_color, build_atlas, linear_to_srgb_byte, srgb_to_linear, swatch, tile_grids,
    tile_has_alpha, tile_rect,
};

/// JavaScript's `Math.round`: the nearest integer, ties toward +∞ (Rust's `round` ties away from
/// zero, which differs for negative halves).
pub(crate) fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// A mesher's input had the wrong number of voxels or cells (the TS meshers throw a RangeError).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthError {
    pub expected: usize,
    pub got: usize,
}

impl std::fmt::Display for LengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "expected {} entries, got {}", self.expected, self.got)
    }
}

impl std::error::Error for LengthError {}
