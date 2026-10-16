//! The marcher's state table (`looks`, group 1 binding 6): per block state, what the traversal
//! asks (its march class, from `dwell_march::StateTable`) and how the hit is shaded — the same
//! colours and atlas tiles the meshers draw it with, from the registry's data (`dwell_blocks`
//! states, `dwell_mesh`'s tiles and flat LOD colours). Nothing here names a block: a state added to
//! `shared/blocks` gets its row from its data.
//!
//! `LOOK_WORDS` words per state: the march flags (`StateGpu::flags`: class in bits 0–1); the chunk
//! mesher's vertex colour (`0xRRGGBB`, white for textured states); the atlas tiles of the top,
//! side and bottom faces (a byte each, `TILE_ORDER` index); the LOD mesher's flat colours of the
//! top, side and bottom (`0xRRGGBB` sRGB); two unused words.

use dwell_blocks::{STATES, StateInfo};
use dwell_march::StateTable;

pub const LOOK_WORDS: usize = 8;

fn tile_index(name: &str) -> u32 {
    dwell_mesh::TILE_ORDER
        .iter()
        .position(|t| *t == name)
        .map_or(0, |i| i as u32)
}

/// The rows for `states` (in id order) and their march table.
pub fn look_words(states: &[StateInfo], table: &StateTable) -> Vec<u32> {
    let flags = table.to_words();
    let mut out = Vec::with_capacity(states.len() * LOOK_WORDS);
    for (i, s) in states.iter().enumerate() {
        let (color, tiles) = match s.textures {
            // The chunk mesher's `surface`: textured faces take their colour from the texture.
            Some(t) => (
                0xff_ffff,
                tile_index(t.top) | tile_index(t.side) << 8 | tile_index(t.bottom) << 16,
            ),
            None => (s.color, 0),
        };
        let id = i as u16;
        out.extend([
            flags[2 * i],
            color,
            tiles,
            dwell_mesh::lod_color(id, 0),
            dwell_mesh::lod_color(id, 1),
            dwell_mesh::lod_color(id, 2),
            0,
            0,
        ]);
    }
    out
}

/// This build's registry.
pub fn registry_looks(table: &StateTable) -> Vec<u32> {
    look_words(&STATES, table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    #[test]
    fn rows_come_from_the_registry_data() {
        let table = StateTable::registry();
        let w = registry_looks(&table);
        assert_eq!(w.len(), STATES.len() * LOOK_WORDS);
        let row = |name: &str| {
            let i = usize::from(require(name)) * LOOK_WORDS;
            w[i..i + LOOK_WORDS].to_vec()
        };
        let grass = row("dwell:grass");
        assert_eq!(grass[0] & 3, 2, "solid");
        assert_eq!(grass[1], 0xff_ffff, "textured: white");
        assert_eq!(grass[2] & 0xff, tile_index("grass"));
        assert_eq!(grass[2] >> 8 & 0xff, tile_index("grassSide"));
        assert_eq!(grass[3], dwell_mesh::lod_color(require("dwell:grass"), 0));
        assert_eq!(row("dwell:air")[0] & 3, 0);
        assert_eq!(row("dwell:water")[0] & 3, 1);
    }

    #[test]
    fn a_state_added_by_data_alone_gets_a_row() {
        // A fixture state: an untextured solid with its own colour.
        let mut states: Vec<StateInfo> = STATES[..2].to_vec();
        let mut extra = STATES[usize::from(require("dwell:stone"))];
        extra.textures = None;
        extra.color = 0x12_3456;
        states.push(extra);
        let table = StateTable::from_states(&states);
        let w = look_words(&states, &table);
        assert_eq!(w.len(), 3 * LOOK_WORDS);
        assert_eq!(w[2 * LOOK_WORDS] & 3, 2);
        assert_eq!(w[2 * LOOK_WORDS + 1], 0x12_3456);
        assert_eq!(w[2 * LOOK_WORDS + 2], 0, "the plain tile");
    }
}
