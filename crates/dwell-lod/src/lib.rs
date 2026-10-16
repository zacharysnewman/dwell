//! The client's whole-world view (ARCHITECTURE.md §6.6, ADR 0012), ported from `client/src/lod/`
//! (ADR 0028, Phase 24e): each frame the LOD octree is walked from the root around the camera,
//! refining while cells project larger than the pixel error; sections are generated, asked of the
//! server, meshed and cached through collaborator traits the host implements ([`LodHost`]).
//!
//! No clocks, threads or I/O: time comes in as milliseconds and every job's result is reported back
//! with the token it was started with (stale tokens are ignored), so the crate builds for the
//! browser and tests natively with fakes.

pub mod detail;
pub mod frustum;
pub mod grid;
pub mod system;

pub use frustum::{
    FOV_SETTING_RANGE, Frustum, LodCamera, LodViewport, base_fov, lod_viewport,
    sanitize_fov_setting, vertical_fov,
};
pub use system::*;
