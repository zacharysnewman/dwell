//! Dwell's ray-marched terrain, CPU core (docs/RAY_MARCHED_RENDERER.md; Phase 25, step A3a): the
//! GPU data layout of the fixed-depth brickmap ([`layout`]), the builders that turn chunks and LOD
//! sections into it ([`build`]), the pool allocator ([`alloc`]), the CPU mirror of the buffers
//! ([`scene`]) and the reference marcher ([`march`]) with its brute-force oracle ([`reference`])
//! and steps-per-ray statistics ([`stats`]).
//!
//! Pure data, no GPU, threads or I/O: it runs in the workers, natively and in tests, and builds
//! for `wasm32-unknown-unknown`. What counts as occupied comes from the block registry's data
//! ([`state`]); nothing here names a block.
//!
//! Every buffer is described word by word in [`layout`], so the WGSL marcher is a transcription of
//! [`march`] over the words [`scene::Scene::gpu`] returns.

pub mod alloc;
pub mod build;
pub mod layout;
pub mod march;
pub mod reference;
pub mod scene;
pub mod state;
pub mod stats;

pub use build::{BuildOptions, NodeBuild, Packing, build_chunk, build_node, build_section};
pub use march::{Hit, March, MarchParams, Outcome, Ray, face_up, level_for};
pub use scene::{GpuBuffers, Scene, chunk_node};
pub use state::{MarchClass, StateTable};
pub use stats::{StepHistogram, StepSummary};
