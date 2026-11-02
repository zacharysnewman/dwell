//! The creative hotbar's items (docs/ITEMS.md): what each slot places, by name — a block and its
//! height in ½ m cells (2: a 1 m block, 1: a ½ m one). Data: `shared/items/items.json`, validated
//! and generated into `items.gen.rs`; the placement rules are the blocks' (`dwell_world::edit`).

/// What holding Place does with an item (data `drag`; docs/ITEMS.md §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemDrag {
    /// Builds on in the build mode (a plain block).
    Modes,
    /// A line straight up or down only, each piece where it is held (a ladder on its wall).
    Vertical,
    /// One click at a time.
    None,
}

/// Which way an item's `facing` property is turned when placed (data `facing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemFacing {
    /// As its data's state.
    None,
    /// Pointing back at the player (a stored ramp: downhill toward them, uphill ahead).
    Toward,
    /// Pointing along the look.
    Away,
}

/// An item (data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemDef {
    /// Namespaced, e.g. `dwell:stone_half`.
    pub name: &'static str,
    /// What the hotbar calls it.
    pub label: &'static str,
    /// The block it places (namespaced id).
    pub block: &'static str,
    /// Its height in ½ m cells.
    pub rows: u8,
    pub drag: ItemDrag,
    /// Property values of the state it places (none: its block's first placeable state).
    pub state: &'static [(&'static str, &'static str)],
    /// How its `facing` property follows the look.
    pub facing: ItemFacing,
    /// A 2-row piece's upper cell: its state with these values over the lower's (none: the same).
    pub upper: &'static [(&'static str, &'static str)],
}

mod table {
    include!("items.gen.rs");
}
pub use table::{BUILD_CLEARANCE, BUILD_HOLD_MS, BUILD_MODE_KEY, BUILD_MODES, ITEMS};

/// The item named `name`.
pub fn item(name: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|i| i.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_places_a_placeable_block_at_a_height_the_protocol_takes() {
        for i in &ITEMS {
            let block = dwell_blocks::find_block(i.block).unwrap_or_else(|| panic!("{}", i.name));
            assert!(
                (block.first_state..block.first_state + block.state_count)
                    .any(dwell_world::edit::placeable),
                "{} places nothing",
                i.name
            );
            assert!(
                (1..=dwell_protocol::MAX_PIECE_ROWS).contains(&i.rows),
                "{}",
                i.name
            );
        }
        // Owner, 2026-10-28: the hotbar's blocks are ½ m; the 1 m items are gone. A 2-row piece
        // is one of two states (the steep slope, 2026-10-30).
        assert!(
            ITEMS.iter().all(|i| i.rows == 1 || !i.upper.is_empty()),
            "½ m pieces only"
        );
        assert_eq!(item("dwell:stone_half"), None);
        assert_eq!(
            item("dwell:ladder").map(|i| i.drag),
            Some(ItemDrag::Vertical)
        );
    }
}
