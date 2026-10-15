//! Dwell's block registry (docs/BLOCK_REGISTRY.md). The tables are generated from
//! `shared/blocks/*.json` by `shared/blocks/gen.mjs`; code refers to blocks by name
//! (`"dwell:sand"`), never by a numeric constant, and resolves names once at startup.

pub mod adaptive;
mod registry;
pub mod shape;
mod types;

#[rustfmt::skip]
#[allow(clippy::all)]
mod tables {
    include!("blocks.gen.rs");
    include!("adaptive.gen.rs");
}

pub use registry::*;
pub use tables::{
    ADAPTIVE_DOWN, ADAPTIVE_UP, BLOCKS, PROPERTIES, PROPERTY_VALUES, REGISTRY_HASH, SHAPE_FACES,
    SHAPES, STATES,
};
pub use types::*;

/// A runtime block state id: an index into `STATES`, valid for this build's registry.
pub type StateId = u16;

/// Air is always state 0 (the generator checks it).
pub const AIR: StateId = 0;

/// The state named by `name`: a canonical string (`dwell:ladder[facing=east,flooded=false]`), or
/// any form [`parse_state`] takes (properties in any order, missing ones at their defaults; a block
/// id alone is its default state).
pub fn state(name: &str) -> Option<StateId> {
    if let Some(i) = STATES.iter().position(|s| s.name == name) {
        return StateId::try_from(i).ok();
    }
    parse_state(name).ok()
}

/// Like [`state`], for names that must exist (built-in content); panics with the name otherwise.
pub fn require(name: &str) -> StateId {
    state(name).unwrap_or_else(|| panic!("unknown block state `{name}`"))
}

/// The state's table row.
pub fn info(id: StateId) -> &'static StateInfo {
    &STATES[usize::from(id)]
}

/// The block a state belongs to.
pub fn block_of(id: StateId) -> &'static BlockDef {
    &BLOCKS[info(id).block]
}

/// FNV-1a 64 over the canonical strings (each followed by `\n`), in id order: what the generator
/// stores as [`REGISTRY_HASH`] and world files record.
pub fn compute_hash() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in &STATES {
        for b in s.name.bytes().chain(std::iter::once(b'\n')) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}
