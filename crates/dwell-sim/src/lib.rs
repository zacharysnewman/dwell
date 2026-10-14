//! Dwell's authoritative server core (ARCHITECTURE.md §4): no sockets, threads it doesn't own or
//! OS calls; the host feeds it transport events, calls `Server::step` at SIM_HZ and drains the
//! outbox. The same core runs in the native server and in the browser's local mode.

pub mod app_version;
pub mod clock;
pub mod entropy;
pub mod fixed_step;
pub mod lod_propagation;
pub mod server;
pub mod worldgen_pool;
