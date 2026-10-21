//! The stored cardinal ramps of the half-height grid (HALF_HEIGHT_GRID.md §3, Phase 41): a natural
//! block's `<name>_ramp` family (`shared/blocks/dwell.json` `rampFamilies`), looked up by name. A
//! ramp rises toward the side opposite its facing (it descends toward `facing`); `half = top` hangs
//! it (face B).

use dwell_blocks::StateId;

use crate::{CELL_H, rows_for_m};

/// Which way a ramp descends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RampFacing {
    North,
    East,
    South,
    West,
}

impl RampFacing {
    pub const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    pub fn name(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::East => "east",
            Self::South => "south",
            Self::West => "west",
        }
    }

    /// The horizontal step toward the side it descends to: (dx, dz), north = −z.
    pub fn step(self) -> (i32, i32) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }
}

/// The state of `base`'s ramp family (`"dwell:grass"` → `dwell:grass_ramp[...]`), dry, or None
/// when `base` has no ramp family.
pub fn ramp_state(base: &str, shape: &str, facing: RampFacing, hanging: bool) -> Option<StateId> {
    ramp_state_flooded(base, shape, facing, hanging, false)
}

/// [`ramp_state`], holding the fluid around it in its open part when `flooded`.
pub fn ramp_state_flooded(
    base: &str,
    shape: &str,
    facing: RampFacing,
    hanging: bool,
    flooded: bool,
) -> Option<StateId> {
    dwell_blocks::state(&format!(
        "{base}_ramp[facing={},flooded={flooded},half={},shape={shape}]",
        facing.name(),
        if hanging { "top" } else { "bottom" },
    ))
}

/// The pieces of a 1 m ramp (45°), from its foot up (face-local): on ½ m cells the steep lower and
/// upper pieces, on 1 m cells one `ramp` piece. None for other cell heights.
pub fn steep_pieces() -> Option<&'static [&'static str]> {
    match rows_for_m(1.0) {
        1 => Some(&["ramp"]),
        2 => Some(&["steep_lower", "steep_upper"]),
        _ => None,
    }
}

/// The cells, from the bottom up, of a 1 m ramp (45°) of `base` descending toward `facing`: on
/// ½ m cells the steep lower and upper pieces, on 1 m cells one `ramp` piece (a wedge as tall as
/// the cell). None for other cell heights or without a ramp family.
pub fn steep_cells(base: &str, facing: RampFacing, hanging: bool) -> Option<Vec<StateId>> {
    let mut cells: Vec<StateId> = steep_pieces()?
        .iter()
        .map(|&p| ramp_state(base, p, facing, hanging))
        .collect::<Option<_>>()?;
    // Hanging (face B, up is −y), the upper piece is the lower one in world y.
    if hanging {
        cells.reverse();
    }
    Some(cells)
}

/// The cell of a ½ m ramp (26.57°) of `base` descending toward `facing`: one `ramp` piece, on ½ m
/// cells. None on 1 m cells (a `ramp` piece there is 45°) or without a ramp family.
pub fn gentle_cell(base: &str, facing: RampFacing, hanging: bool) -> Option<StateId> {
    if CELL_H != 0.5 {
        return None;
    }
    ramp_state(base, "ramp", facing, hanging)
}

/// [`gentle_cell`] upright, flooded or dry, for the generator.
pub fn gentle_cell_flooded(base: &str, facing: RampFacing, flooded: bool) -> Option<StateId> {
    if CELL_H != 0.5 {
        return None;
    }
    ramp_state_flooded(base, "ramp", facing, false, flooded)
}

/// [`steep_cells`] upright, flooded or dry, for the generator.
pub fn steep_cells_flooded(base: &str, facing: RampFacing, flooded: bool) -> Option<Vec<StateId>> {
    steep_pieces()?
        .iter()
        .map(|&p| ramp_state_flooded(base, p, facing, false, flooded))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::shape::{shape_of, solid_span_at};

    #[test]
    fn the_natural_blocks_have_ramps_and_a_45_degree_ramp_rises_a_metre_over_a_metre() {
        for base in ["dwell:stone", "dwell:dirt", "dwell:grass", "dwell:sand"] {
            for facing in RampFacing::ALL {
                for hanging in [false, true] {
                    assert!(steep_cells(base, facing, hanging).is_some(), "{base}");
                }
            }
        }
        assert_eq!(steep_cells("dwell:water", RampFacing::East, false), None);
        // Descending east: at x the surface is 1 − x metres above the ramp's foot, whatever the
        // cell height.
        let cells = steep_cells("dwell:grass", RampFacing::East, false).expect("cells");
        for x in [0.1f32, 0.3, 0.6, 0.9] {
            let mut top = 0.0;
            for (row, &c) in cells.iter().enumerate() {
                let span = solid_span_at(shape_of(c), x, 0.5);
                if span.hi > span.lo {
                    top = (row as f32 + span.hi) * CELL_H as f32;
                }
            }
            assert!((top - (1.0 - x)).abs() < 1e-5, "at {x}: {top}");
        }
    }
}
