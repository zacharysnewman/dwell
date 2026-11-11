//! The creative hotbar (docs/ITEMS.md §6a; owner, 2026-11-10): `HOTBAR_SLOTS` slots
//! (`shared/inventory/inventory.json`), each holding one palette item the player picked in the
//! creative inventory view. A new world starts with `CREATIVE_HOTBAR` (`shared/items/items.json`
//! `inventory.hotbar`); the shell saves the picks per world and sets them back at join.

use crate::block_interaction::PaletteSlot;
use dwell_protocol::inventory::HOTBAR_SLOTS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreativeHotbar {
    /// Each slot's palette index.
    picks: Vec<usize>,
    /// The selected slot.
    slot: usize,
}

impl CreativeHotbar {
    /// A new world's picks, by item name in `palette`; a name this palette lacks takes the
    /// palette's slot of the same number.
    pub fn new(palette: &[PaletteSlot]) -> Self {
        let n = palette.len().max(1);
        let picks = (0..HOTBAR_SLOTS)
            .map(|i| {
                crate::items::CREATIVE_HOTBAR
                    .get(i)
                    .and_then(|name| palette.iter().position(|p| p.item == *name))
                    .unwrap_or(i % n)
            })
            .collect();
        Self { picks, slot: 0 }
    }

    /// Each slot's palette index.
    pub fn picks(&self) -> &[usize] {
        &self.picks
    }

    /// The selected slot.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The palette index slot `slot` holds.
    pub fn pick(&self, slot: usize) -> Option<usize> {
        self.picks.get(slot).copied()
    }

    /// Selects slot `slot`, wrapping; the slot and the palette index it holds.
    pub fn select(&mut self, slot: i64) -> (usize, usize) {
        self.slot = slot.rem_euclid(self.picks.len() as i64) as usize;
        (self.slot, self.picks[self.slot])
    }

    /// Puts palette item `palette_index` (of `palette_len`) in slot `slot`; false if either is out
    /// of range.
    pub fn set(&mut self, slot: usize, palette_index: usize, palette_len: usize) -> bool {
        if slot >= self.picks.len() || palette_index >= palette_len {
            return false;
        }
        self.picks[slot] = palette_index;
        true
    }
}

/// The hotbar slot a number key picks (`Digit1` the first, `Digit0` the tenth), if the hotbar of
/// `len` slots has it: with 6 slots, keys 1–6 only.
pub fn slot_for_digit(code: &str, len: usize) -> Option<i64> {
    let d = code.strip_prefix("Digit")?.parse::<i64>().ok()?;
    let slot = if d == 0 { 9 } else { d - 1 };
    ((slot as usize) < len).then_some(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_interaction::palette;

    #[test]
    fn number_keys_1_to_6_pick_the_six_slots_and_7_to_0_nothing() {
        for d in 1..=6 {
            assert_eq!(slot_for_digit(&format!("Digit{d}"), HOTBAR_SLOTS), Some(d - 1));
        }
        for d in [7, 8, 9, 0] {
            assert_eq!(slot_for_digit(&format!("Digit{d}"), HOTBAR_SLOTS), None);
        }
        assert_eq!(slot_for_digit("KeyE", HOTBAR_SLOTS), None);
    }

    #[test]
    fn six_slots_from_the_data_with_the_default_picks() {
        let p = palette();
        let h = CreativeHotbar::new(&p);
        assert_eq!(HOTBAR_SLOTS, 6);
        let names: Vec<&str> = h.picks().iter().map(|&i| p[i].item).collect();
        assert_eq!(names, crate::items::CREATIVE_HOTBAR);
    }

    #[test]
    fn selecting_wraps_over_six_and_a_pick_lands_in_the_selected_slot() {
        let p = palette();
        let mut h = CreativeHotbar::new(&p);
        assert_eq!(h.select(6).0, 0);
        assert_eq!(h.select(-1).0, 5);
        let ladder = p.iter().position(|s| s.item == "dwell:ladder").unwrap();
        assert!(h.set(h.slot(), ladder, p.len()));
        assert_eq!(h.select(5), (5, ladder));
        assert!(!h.set(6, 0, p.len()));
        assert!(!h.set(0, p.len(), p.len()));
    }
}
