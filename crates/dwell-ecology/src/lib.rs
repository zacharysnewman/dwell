//! Dwell's far ecology (docs/ECOLOGY.md; WORK_QUEUE item 11): away from players the deer and wolves
//! are data only, one record per herd or pack ([`Group`]) on a coarse grid of the face's island
//! ([`Island`], sampled from its world map), with each cell's forage a number that grazing depletes
//! and time regrows; the real tall grass is not touched. [`step`] fast-forwards the whole island
//! over days, like the water grid's far pools (ADR 0060). The tuning is data
//! (`shared/ecology/ecology.json`, generated into [`DATA`]); species are found by name
//! ([`Data::find`]).
//!
//! Face-local: an ecology runs on one face's map, in its face-local metres (nothing here depends on
//! which way is up); each face has its own.
//!
//! The server (`dwell-sim`'s `ecology`) holds one per face, steps it by the world's days and saves
//! it ([`Ecosystem::to_bytes`]). Not yet: individuals made when a player comes near, far rendering,
//! grazing applied to the real tall grass (docs/ECOLOGY.md §5).

pub mod codec;
pub mod data;
pub mod island;
pub mod sim;

pub use data::{Data, Diet, Disperse, Flee, GroundDef, GroupRules, IslandParams, SpeciesDef};
pub use island::{Ground, Island};
pub use sim::{Activity, Ecosystem, Group, step};

mod table {
    include!("ecology.gen.rs");
}

/// The build's ecology (`shared/ecology/ecology.json`).
pub const DATA: Data = table::DATA;

/// Tests only: a fixture ecology (`shared/ecology/fixtures/ecology.json`), species added by data
/// alone.
pub mod fixture {
    include!("ecology_fixture.gen.rs");
}
