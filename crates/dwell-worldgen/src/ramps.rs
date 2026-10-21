//! The generation rule of the half-height grid (HALF_HEIGHT_GRID.md §4, Phase 41): which stored
//! cardinal ramp, if any, tops a natural column, from its own height and its 8 neighbours'
//! (rows). A pure function of the 9 heights, so the generator, the point queries and tests agree.
//! The pieces themselves are block data (`rampFamilies`); this is the small fixed vocabulary.

use dwell_world::ramps::RampFacing;

/// The top of a natural column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Top {
    /// A full block.
    Flat,
    /// A ½ m ramp (one cell) descending toward the facing.
    Gentle(RampFacing),
    /// A 1 m ramp (45°, the column's top two cells) descending toward the facing.
    Steep(RampFacing),
}

/// Column heights in rows around a column: `n[dz + 1][dx + 1]`, north (−z) first; the centre is
/// the column's own.
pub type Around = [[i32; 3]; 3];

fn at(n: &Around, (dx, dz): (i32, i32)) -> i32 {
    n[(dz + 1) as usize][(dx + 1) as usize]
}

/// The rule:
///
/// 1. **Ramp** facing `d` (descending toward it): the edge neighbour toward `d` is lower, the one
///    opposite is not, and either both side neighbours and the two diagonals beside the opposite
///    one are not lower (a slope), or the opposite one is higher and both sides are lower (a path
///    or stair one block wide, which becomes a run of ramps). At most one facing qualifies.
/// 2. **Steep**: such a ramp whose neighbour toward `d` is at least 1 m (`steep_rows`) lower is a
///    1 m ramp instead.
/// 3. Otherwise **flat**.
pub fn top_of(n: &Around, steep_rows: i32) -> Top {
    let t = n[1][1];
    for facing in RampFacing::ALL {
        if accepts(n, facing) {
            let (dx, dz) = facing.step();
            return if at(n, (dx, dz)) <= t - steep_rows {
                Top::Steep(facing)
            } else {
                Top::Gentle(facing)
            };
        }
    }
    Top::Flat
}

/// Rule 1 for one facing.
fn accepts(n: &Around, facing: RampFacing) -> bool {
    let t = n[1][1];
    let (dx, dz) = facing.step();
    let toward = at(n, (dx, dz));
    let opposite = at(n, (-dx, -dz));
    if toward >= t || opposite < t {
        return false;
    }
    // The two sides, and the diagonals beside the opposite neighbour.
    let sides = [at(n, (dz, dx)), at(n, (-dz, -dx))];
    let back = [at(n, (dz - dx, dx - dz)), at(n, (-dz - dx, -dx - dz))];
    let slope = sides.iter().chain(&back).all(|&h| h >= t);
    let one_wide = opposite > t && sides.iter().all(|&h| h < t);
    slope || one_wide
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every neighbourhood of heights t − 2 … t + 1 around a column of height t.
    fn all_neighbourhoods(t: i32) -> impl Iterator<Item = Around> {
        (0..4u32.pow(8)).map(move |mut code| {
            let mut n = [[t; 3]; 3];
            for (r, c) in [
                (0, 0),
                (0, 1),
                (0, 2),
                (1, 0),
                (1, 2),
                (2, 0),
                (2, 1),
                (2, 2),
            ] {
                n[r][c] = t - 2 + (code % 4) as i32;
                code /= 4;
            }
            n
        })
    }

    #[test]
    fn at_most_one_facing_qualifies_in_every_neighbourhood() {
        // 4⁸ neighbourhoods: the rule never has two facings to choose from, so the order facings
        // are tried in does not matter.
        let mut ramps = 0;
        for n in all_neighbourhoods(10) {
            let count = RampFacing::ALL.iter().filter(|&&f| accepts(&n, f)).count();
            assert!(count <= 1, "{n:?}");
            ramps += count;
        }
        assert!(ramps > 0);
    }

    #[test]
    fn the_side_and_back_cells_are_the_ones_beside_the_slope() {
        // Facing east (+x): the sides are north and south, the back diagonals north-west and
        // south-west.
        let (dx, dz) = RampFacing::East.step();
        assert_eq!([(dz, dx), (-dz, -dx)], [(0, 1), (0, -1)]);
        assert_eq!(
            [(dz - dx, dx - dz), (-dz - dx, -dx - dz)],
            [(-1, 1), (-1, -1)]
        );
    }

    fn field(f: impl Fn(i32, i32) -> i32) -> Around {
        std::array::from_fn(|r| std::array::from_fn(|c| f(c as i32 - 1, r as i32 - 1)))
    }

    #[test]
    fn a_flat_field_and_a_plateau_stay_flat() {
        assert_eq!(top_of(&field(|_, _| 4), 2), Top::Flat);
        // At the foot of a rise (higher ground to the west, nothing lower): flat; the column
        // above it takes the ramp.
        assert_eq!(
            top_of(&field(|x, _| if x < 0 { 5 } else { 4 }), 2),
            Top::Flat
        );
    }

    #[test]
    fn a_half_metre_edge_is_a_gentle_ramp_and_a_metre_edge_is_steep() {
        // Higher ground to the west, lower to the east: descends east.
        let half = field(|x, _| if x > 0 { 3 } else { 4 });
        assert_eq!(top_of(&half, 2), Top::Gentle(RampFacing::East));
        let metre = field(|x, _| if x > 0 { 2 } else { 4 });
        assert_eq!(top_of(&metre, 2), Top::Steep(RampFacing::East));
        // The same toward north (−z).
        let north = field(|_, z| if z < 0 { 3 } else { 4 });
        assert_eq!(top_of(&north, 2), Top::Gentle(RampFacing::North));
    }

    #[test]
    fn a_plateau_corner_and_a_lone_bump_have_no_piece_and_stay_flat() {
        // Outer corner: lower to the east and the south.
        let corner = field(|x, z| if x > 0 || z > 0 { 3 } else { 4 });
        assert_eq!(top_of(&corner, 2), Top::Flat);
        let bump = field(|x, z| if x == 0 && z == 0 { 4 } else { 3 });
        assert_eq!(top_of(&bump, 2), Top::Flat);
    }

    #[test]
    fn a_one_wide_stair_is_a_run_of_ramps() {
        // A path one block wide climbing west (higher to the west, lower east, walls of lower
        // ground either side).
        let stair = field(|x, z| match (x, z) {
            (-1, 0) => 5,
            (0, 0) => 4,
            (1, 0) => 3,
            _ => 2,
        });
        assert_eq!(top_of(&stair, 2), Top::Gentle(RampFacing::East));
        let steep_stair = field(|x, z| match (x, z) {
            (-1, 0) => 6,
            (0, 0) => 4,
            (1, 0) => 2,
            _ => 0,
        });
        assert_eq!(top_of(&steep_stair, 2), Top::Steep(RampFacing::East));
    }

    #[test]
    fn a_slope_needs_its_back_corners_up() {
        // An edge whose back diagonal is lower is a corner of something: no ramp.
        let mut n = field(|x, _| if x > 0 { 3 } else { 4 });
        n[0][0] = 3;
        assert_eq!(top_of(&n, 2), Top::Flat);
    }
}
