//! Dwell's lava (docs/proposals/VOLCANOES.md §6, phase V1; WORK_QUEUE item 8): hot fluids on their own
//! surface grid ([`Field`]), flowing slowly by a viscous law, cooling from their edges and top into
//! rock (new land), quenching to obsidian with a steam event where water touches them, fed by vents
//! as data ([`VentDef`]: a crater lake held at its level, or a river), and burning a player who
//! stands in them ([`Burn`]). Scheduled like the water: near players every tick, fast-forwarded
//! elsewhere ([`Field::advance`]). The tuning is data (`shared/fluids/lava.json`, generated into
//! [`DATA`]); fluids and vents are found by name.
//!
//! Not yet (the next slices): the server's fields and tick, writing the rock and lava cells into
//! chunks, the water grid's columns and its steam, the damage hooked into the server's vitals, the
//! save table, the lava block and its look (docs/proposals/VOLCANOES.md §6).

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
