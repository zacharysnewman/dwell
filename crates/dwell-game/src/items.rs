//! The creative hotbar's items (docs/ITEMS.md): what each slot places, by name — a block and its
//! height in ½ m cells (2: a 1 m block, 1: a ½ m one). Data: `shared/items/items.json`, validated
//! and generated into `items.gen.rs`; the placement rules are the blocks' (`dwell_world::edit`).

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
}

mod table {
    include!("items.gen.rs");
}
pub use table::{BUILD_CLEARANCE, BUILD_MODE_KEY, BUILD_MODES, ITEMS};

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
        assert_eq!(
            item("dwell:stone").map(|i| i.rows),
            Some(2),
            "1 m by default"
        );
        assert_eq!(item("dwell:stone_half").map(|i| i.rows), Some(1));
    }
}
