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

/// The rule (owner, 2026-10-20: option A of the slopes investigation):
///
/// 1. **Facing**: the descent of the 9 heights' Sobel gradient, along its dominant axis
///    ([`facing_of`]; x on a tie). A neighbourhood with no gradient has none.
/// 2. **Ramp** facing `d` (descending toward it): the edge neighbour toward `d` is lower and the
///    one opposite is not. The sides and diagonals are not tested, so a contour that shifts
///    sideways by a column still ramps on its higher column.
/// 3. **Steep**: such a ramp whose neighbour toward `d` is at least 1 m (`steep_rows`) lower is a
///    1 m ramp instead.
/// 4. Otherwise **flat**.
pub fn top_of(n: &Around, steep_rows: i32) -> Top {
    let t = n[1][1];
    let Some(facing) = facing_of(n) else {
        return Top::Flat;
    };
    let (dx, dz) = facing.step();
    let toward = at(n, (dx, dz));
    if toward >= t || at(n, (-dx, -dz)) < t {
        return Top::Flat;
    }
    if toward <= t - steep_rows {
        Top::Steep(facing)
    } else {
        Top::Gentle(facing)
    }
}

/// The facing the 9 heights descend toward: the Sobel gradient's dominant axis (x when the two
/// are equal), or none when the gradient is zero.
pub fn facing_of(n: &Around) -> Option<RampFacing> {
    // Heights far below (beyond the rim) saturate rather than overflow.
    let a = |dx: i32, dz: i32| i64::from(at(n, (dx, dz)));
    let gx = a(1, -1) + 2 * a(1, 0) + a(1, 1) - a(-1, -1) - 2 * a(-1, 0) - a(-1, 1);
    let gz = a(-1, 1) + 2 * a(0, 1) + a(1, 1) - a(-1, -1) - 2 * a(0, -1) - a(1, -1);
    if (gx, gz) == (0, 0) {
        return None;
    }
    Some(if gx.abs() >= gz.abs() {
        if gx < 0 {
            RampFacing::East
        } else {
            RampFacing::West
        }
    } else if gz < 0 {
        RampFacing::South
    } else {
        RampFacing::North
    })
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
    fn every_ramp_descends_toward_a_lower_neighbour_from_one_not_lower() {
        // 4⁸ neighbourhoods: a ramp's facing is the Sobel descent, its neighbour toward it is
        // lower and the one opposite not; a steep ramp's is at least 1 m lower.
        let (mut ramps, mut flats) = (0, 0);
        for n in all_neighbourhoods(10) {
            match top_of(&n, 2) {
                Top::Flat => flats += 1,
                Top::Gentle(f) | Top::Steep(f) => {
                    assert_eq!(facing_of(&n), Some(f), "{n:?}");
                    let (dx, dz) = f.step();
                    let toward = at(&n, (dx, dz));
                    assert!(toward < 10 && at(&n, (-dx, -dz)) >= 10, "{n:?}");
                    let steep = matches!(top_of(&n, 2), Top::Steep(_));
                    assert_eq!(steep, toward <= 8, "{n:?}");
                    ramps += 1;
                }
            }
        }
        assert!(ramps > 0 && flats > 0);
    }

    #[test]
    fn mirroring_the_neighbourhood_mirrors_the_ramp() {
        // East–west and north–south mirrors map the rule onto itself (the tie goes to x, which
        // either mirror keeps).
        let mirror = |f: RampFacing, x: bool| match (f, x) {
            (RampFacing::East, true) => RampFacing::West,
            (RampFacing::West, true) => RampFacing::East,
            (RampFacing::North, false) => RampFacing::South,
            (RampFacing::South, false) => RampFacing::North,
            (f, _) => f,
        };
        for n in all_neighbourhoods(10) {
            for x in [true, false] {
                let m: Around = std::array::from_fn(|r| {
                    std::array::from_fn(|c| if x { n[r][2 - c] } else { n[2 - r][c] })
                });
                let want = match top_of(&n, 2) {
                    Top::Flat => Top::Flat,
                    Top::Gentle(f) => Top::Gentle(mirror(f, x)),
                    Top::Steep(f) => Top::Steep(mirror(f, x)),
                };
                assert_eq!(top_of(&m, 2), want, "{n:?} mirrored in x: {x}");
            }
        }
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
    fn a_lone_bump_a_pit_and_a_ridge_stay_flat() {
        let bump = field(|x, z| if x == 0 && z == 0 { 4 } else { 3 });
        assert_eq!(top_of(&bump, 2), Top::Flat);
        let pit = field(|x, z| if x == 0 && z == 0 { 3 } else { 4 });
        assert_eq!(top_of(&pit, 2), Top::Flat);
        // A ridge along z, lower to the east and the west: no side is not lower.
        let ridge = field(|x, _| if x == 0 { 4 } else { 3 });
        assert_eq!(top_of(&ridge, 2), Top::Flat);
    }

    #[test]
    fn a_plateau_corner_ramps_down_the_steeper_way_and_along_x_on_a_tie() {
        // Outer corner: lower to the east and the south. The gradient points equally east and
        // south, and the tie goes to x.
        let corner = field(|x, z| if x > 0 || z > 0 { 3 } else { 4 });
        assert_eq!(top_of(&corner, 2), Top::Gentle(RampFacing::East));
        // Lower again to the south-east and south-west: south is steeper.
        let south = field(|x, z| match (x, z) {
            (_, 1) => 2,
            (1, _) => 3,
            _ => 4,
        });
        assert_eq!(top_of(&south, 2), Top::Steep(RampFacing::South));
    }

    #[test]
    fn a_contour_that_shifts_sideways_still_ramps_its_higher_column() {
        // Owner, 2026-10-20 (option A): ground descending east whose contour steps one column
        // sideways here (lower to the south as well as the east). The old rule wanted both sides
        // and the back diagonals not lower, so this column stayed flat and left a 1 m face; the
        // Sobel rule ramps it east.
        let shifted = field(|x, z| 4 - i32::from(x > 0) - i32::from(x == 0 && z > 0));
        assert_eq!(shifted[2][1], 3);
        assert_eq!(top_of(&shifted, 2), Top::Gentle(RampFacing::East));
        // And a back diagonal that is lower (the contour's other side) does not stop it.
        let mut n = field(|x, _| if x > 0 { 3 } else { 4 });
        n[0][0] = 3;
        assert_eq!(top_of(&n, 2), Top::Gentle(RampFacing::East));
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
}
