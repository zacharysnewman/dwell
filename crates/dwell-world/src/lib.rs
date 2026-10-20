//! Dwell's voxel world (ARCHITECTURE.md §6): chunks and the voxel grid, the build volume, the
//! world's two faces, LOD sections, structure files and block edits. Blocks are referred to by name
//! (`dwell-blocks`); built-in ones the engine needs are resolved once into [`M`].

pub mod bounds;
pub mod brush;
mod cell;
pub mod edit;
pub mod fluid;
pub mod hash;
pub mod lod;
pub mod ramps;
pub mod region;
pub mod structure;
mod voxel;

pub use bounds::{BUILD_VOLUME, BoundsShape, BuildVolume, Overlap, inside_build_volume};
pub use cell::*;
pub use voxel::*;
