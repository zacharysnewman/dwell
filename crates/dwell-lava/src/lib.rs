//! Dwell's lava (docs/proposals/VOLCANOES.md §6, phase V1; WORK_QUEUE item 8): hot fluids on their own
//! surface grid ([`Field`]), flowing slowly by a viscous law, cooling from their edges and top into
//! rock (new land), quenching to obsidian with a steam event where water touches them, fed by vents
//! as data ([`VentDef`]: a crater lake held at its level, or a river), and burning a player who
//! stands in them ([`Burn`]). Scheduled like the water: near players every tick, fast-forwarded
//! elsewhere ([`Field::advance`]). The tuning is data (`shared/fluids/lava.json`, generated into
//! [`DATA`]); fluids and vents are found by name.
//!
//! The server holds and ticks the world's fields, writes their cells and saves them (`dwell-sim`
//! `lava.rs`). Not yet: the water grid's own columns as a field's water, the look
//! (docs/proposals/VOLCANOES.md §6.1).

pub mod burn;
pub mod data;
pub mod field;
pub mod save;

pub use burn::Burn;
pub use data::{HotFluid, LavaData, Quench, VentDef, VentKind};
pub use field::{Field, LavaEvent, Vent};
pub use save::{field_of, record_of};

mod table {
    include!("lava.gen.rs");
}

/// The build's lava data (`shared/fluids/lava.json`).
pub const DATA: LavaData = table::DATA;

/// Tests only: fixture data (`shared/fluids/fixtures/lava.json`), a fluid and vents added by data
/// alone.
pub mod fixture {
    include!("lava_fixture.gen.rs");
}
