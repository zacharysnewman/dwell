//! World files (ARCHITECTURE.md §6.4, ADR 0028 §8): one `redb` database per world holding all of its
//! data, natively in a file and in the browser on an OPFS backend. Chunk and LOD blobs are the
//! protocol's palette + RLE encoding compressed with LZ4.
//!
//! Chunks are stored with *world* state ids: the file's `block_states` table maps them to canonical
//! strings, so a world outlives changes to the code's runtime ids (docs/BLOCK_REGISTRY.md §4).

mod codec;
mod db;
mod store;

pub use codec::*;
pub use db::*;
pub use store::*;
