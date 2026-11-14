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

/// [`top_of`], and where it leaves a column flat over a lower edge neighbour on a diagonal contour
/// (a corner of the slope: the neighbour toward `d` lower, the one opposite not, a side lower too),
/// a **notched** ramp (owner, 2026-11-13: "Notched ramps seems fine"): the facing whose neighbour
/// is lowest, ties in [`RampFacing::ALL`]'s order, steep as rule 2 says. A column with no higher
/// or level neighbour opposite a lower one (a peak, a one-wide ridge's crest) stays flat. Used where
/// the rock shapes the ground (rocks.json `shaping.notched`), so its slopes ramp rather than stair.
pub fn top_of_notched(n: &Around, steep_rows: i32) -> Top {
    let strict = top_of(n, steep_rows);
    if strict != Top::Flat {
        return strict;
    }
    let t = n[1][1];
    let mut best: Option<(i32, RampFacing)> = None;
    for facing in RampFacing::ALL {
        let (dx, dz) = facing.step();
        let toward = at(n, (dx, dz));
        if toward >= t || at(n, (-dx, -dz)) < t {
            continue;
        }
        if best.is_none_or(|(h, _)| toward < h) {
            best = Some((toward, facing));
        }
    }
    match best {
        Some((h, facing)) if h <= t - steep_rows => Top::Steep(facing),
        Some((_, facing)) => Top::Gentle(facing),
        None => Top::Flat,
    }
}

/// Which rule shapes a generator's tops, as data (a grid generator's `ramps`, `grids.json`):
/// none (every top flat), [`top_of`] (HALF_HEIGHT_GRID.md §4, V2) or [`top_of_notched`] (§10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RampRule {
    #[default]
    None,
    Strict,
    Notched,
}

impl RampRule {
    /// The data's name: `"none"`, `"strict"` or `"notched"`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "strict" => Some(Self::Strict),
            "notched" => Some(Self::Notched),
            _ => None,
        }
    }

    /// The top of a column by this rule.
    pub fn top(self, n: &Around, steep_rows: i32) -> Top {
        match self {
            Self::None => Top::Flat,
            Self::Strict => top_of(n, steep_rows),
            Self::Notched => top_of_notched(n, steep_rows),
        }
    }
}

/// Column heights in rows over a 5 × 5 window, `n[dz + 2][dx + 2]`: the column's own 3 × 3 and
/// each neighbour's, for [`RampRule::top_backed`].
pub type Around5 = [[i32; 5]; 5];

/// The 3 × 3 window centred `(dx, dz)` from the centre of `n` (each within ±1).
pub fn window(n: &Around5, (dx, dz): (i32, i32)) -> Around {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| n[(dz + 1 + r as i32) as usize][(dx + 1 + c as i32) as usize])
    })
}

/// The lowest the top of a column `h` rows high, shaped `top`, stands along its edge toward
/// `toward` (rows): a full block's whole height; a ramp's low side toward its facing, its high
/// side away from it, and its sloped triangle's foot along either side.
pub fn edge_height(h: i32, top: Top, toward: RampFacing) -> i32 {
    let (facing, drop) = match top {
        Top::Flat => return h,
        Top::Gentle(f) => (f, 1),
        Top::Steep(f) => (f, 2),
    };
    let away = {
        let (dx, dz) = facing.step();
        (-dx, -dz)
    };
    if toward.step() == away { h } else { h - drop }
}

impl RampRule {
    /// [`RampRule::top`] with every ramp **backed** (BUGS.md #163; owner, 2026-11-13: "I don't
    /// mind slopes with edges exposed but I do mind slopes that back up against open air"): a
    /// ramp's high side must meet the column behind it standing at least the ramp's top height
    /// along their shared edge (that column as this rule shapes it, before its own check), else
    /// the column is a full block. Its triangular sides may still show. Each column's final top is
    /// its unchecked one or flat, and flat stands at least as high along every edge, so a ramp
    /// checked against its neighbour's unchecked top is backed whatever that neighbour becomes.
    pub fn top_backed(self, n: &Around5, steep_rows: i32) -> Top {
        top_backed_by(n, steep_rows, |_| self)
    }
}

/// [`RampRule::top_backed`] where the rule may differ by column (a terrain's notched ramps only at
/// rock walls' edges): `rule(dx, dz)` is the rule of the column at that offset (within ±1).
pub fn top_backed_by(n: &Around5, steep_rows: i32, rule: impl Fn((i32, i32)) -> RampRule) -> Top {
    {
        let own = rule((0, 0)).top(&window(n, (0, 0)), steep_rows);
        let facing = match own {
            Top::Flat => return own,
            Top::Gentle(f) | Top::Steep(f) => f,
        };
        let (dx, dz) = facing.step();
        let behind = (-dx, -dz);
        let bh = n[(behind.1 + 2) as usize][(behind.0 + 2) as usize];
        let btop = rule(behind).top(&window(n, behind), steep_rows);
        if edge_height(bh, btop, facing) >= n[2][2] {
            own
        } else {
            Top::Flat
        }
    }
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

    #[test]
    fn a_diagonal_contour_is_a_notched_ramp_only_where_notched() {
        // Ground descending toward the north-east: the column's east and north neighbours are a
        // row lower, west and south level. The strict rule leaves it flat (a bare ½ m stair);
        // the notched rule ramps it toward the lower side, north first on a tie.
        let n = field(|x, z| 4 - i32::from(x > 0) - i32::from(z < 0));
        assert_eq!(top_of(&n, 2), Top::Flat);
        assert_eq!(top_of_notched(&n, 2), Top::Gentle(RampFacing::North));
        // The east side a metre lower: the notch faces it, steep.
        let n = field(|x, z| 4 - 2 * i32::from(x > 0) - i32::from(z < 0));
        assert_eq!(top_of(&n, 2), Top::Flat);
        assert_eq!(top_of_notched(&n, 2), Top::Steep(RampFacing::East));
        // Straight edges and flat ground are the strict rule's; a lone peak stays flat.
        let edge = field(|x, _| if x > 0 { 3 } else { 4 });
        assert_eq!(top_of_notched(&edge, 2), top_of(&edge, 2));
        assert_eq!(top_of_notched(&field(|_, _| 4), 2), Top::Flat);
        let peak = field(|x, z| if (x, z) == (0, 0) { 5 } else { 4 });
        assert_eq!(top_of_notched(&peak, 2), Top::Flat);
    }

    /// A field of heights (rows) over `w` × `w` columns and, for each interior column, the top
    /// `pick` gives from its 5 × 5 window: then how many ramps have their high side against a
    /// column standing lower than their top along the shared edge (BUGS.md #163).
    fn exposed(
        heights: &dyn Fn(i32, i32) -> i32,
        w: i32,
        pick: &dyn Fn(&Around5) -> Top,
    ) -> (usize, usize) {
        let n5 = |x: i32, z: i32| -> Around5 {
            std::array::from_fn(|r| {
                std::array::from_fn(|c| heights(x + c as i32 - 2, z + r as i32 - 2))
            })
        };
        let top = |x: i32, z: i32| pick(&n5(x, z));
        let (mut ramps, mut bad) = (0, 0);
        for z in 3..w - 3 {
            for x in 3..w - 3 {
                let f = match top(x, z) {
                    Top::Flat => continue,
                    Top::Gentle(f) | Top::Steep(f) => f,
                };
                ramps += 1;
                let (dx, dz) = f.step();
                let (bx, bz) = (x - dx, z - dz);
                if edge_height(heights(bx, bz), top(bx, bz), f) < heights(x, z) {
                    bad += 1;
                }
            }
        }
        (ramps, bad)
    }

    /// A bowl: heights rising with the distance from its middle, in ½ m rows.
    fn bowl(x: i32, z: i32) -> i32 {
        let (dx, dz) = (f64::from(x - 20), f64::from(z - 20));
        (20.0 + 0.6 * (dx * dx + dz * dz).sqrt()).round() as i32
    }

    #[test]
    fn a_bowls_corners_back_no_ramp_onto_open_air() {
        // BUGS.md #163: the unchecked notched rule leaves ramps backing onto lower ground (red),
        // the backed rule none (green), for the strict and the notched rules.
        for rule in [RampRule::Strict, RampRule::Notched] {
            let raw = exposed(&bowl, 40, &|n| rule.top(&window(n, (0, 0)), 2));
            let backed = exposed(&bowl, 40, &|n| rule.top_backed(n, 2));
            eprintln!("{rule:?}: unchecked {raw:?}, backed {backed:?}");
            // The notched rule (0.59–0.61 everywhere the rock shaped the ground, the owner's
            // seed 274005646 on 0.61.7) showed it: 52 of 600 ramps in this bowl.
            if rule == RampRule::Notched {
                assert!(raw.1 > 0, "the bowl shows the bug unchecked: {raw:?}");
            }
            assert_eq!(
                backed.1, 0,
                "{rule:?}: ramps backing onto air: {backed:?} (unchecked {raw:?})"
            );
            assert!(
                backed.0 * 2 > raw.0,
                "{rule:?}: most ramps stay: {backed:?} of {raw:?}"
            );
        }
    }

    #[test]
    fn straight_slopes_keep_their_ramps() {
        // A slope down to the east, ½ m a column, and one 1 m a column (steep pairs): every
        // column keeps its unchecked top.
        for drop in [1, 2] {
            let slope = move |x: i32, _z: i32| 40 - drop * x;
            for rule in [RampRule::Strict, RampRule::Notched] {
                for z in 3..10 {
                    for x in 3..10 {
                        let n: Around5 = std::array::from_fn(|r| {
                            std::array::from_fn(|c| slope(x + c as i32 - 2, z + r as i32 - 2))
                        });
                        let raw = rule.top(&window(&n, (0, 0)), 2);
                        assert_ne!(raw, Top::Flat);
                        assert_eq!(rule.top_backed(&n, 2), raw, "{rule:?} drop {drop}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_ramps_edges_by_side() {
        let f = RampFacing::East;
        assert_eq!(edge_height(6, Top::Flat, f), 6);
        // A ramp descending east: low (−1) toward the east, high toward the west, its triangle's
        // foot along north and south.
        assert_eq!(edge_height(6, Top::Gentle(f), RampFacing::East), 5);
        assert_eq!(edge_height(6, Top::Gentle(f), RampFacing::West), 6);
        assert_eq!(edge_height(6, Top::Gentle(f), RampFacing::North), 5);
        assert_eq!(edge_height(6, Top::Steep(f), RampFacing::East), 4);
        assert_eq!(edge_height(6, Top::Steep(f), RampFacing::West), 6);
    }
}
