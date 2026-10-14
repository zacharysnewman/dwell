//! Dwell's physics (ARCHITECTURE.md §7, ADR 0028): Rapier (`rapier3d-f64`, enhanced determinism)
//! behind a small wrapper, the voxel terrain collider, the voxel probes, and the physics player
//! controller with its networking (PLAYER_CONTROLLER.md).

pub mod config;
pub mod controller;
pub mod frame;
pub mod net;
mod physics;
pub mod predictor;
pub mod query;
pub mod terrain;

pub use physics::*;
