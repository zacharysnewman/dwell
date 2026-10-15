//! Adaptive natural blocks (docs/ADAPTIVE_BLOCKS.md, ADR 0024): a block whose state is
//! `adaptive` is stored as a cube and, when nothing solid is above it (a *top block*), takes its
//! shape from its 8 horizontal neighbours — each higher, the same height or lower — through the
//! rule table generated from `shared/blocks/adaptive.json` ([`ADAPTIVE_UP`], [`ADAPTIVE_DOWN`]).
//! Shapes are derived at use, never stored: the meshers, the terrain collider, the controller's
//! probes, the targeting ray and the terrain's point queries all call [`derive`] on the same
//! voxels, so server and client agree. A pure function of occupancy: neighbours are compared by
//! whether their cells are filled, never by their derived shapes, so nothing iterates.

use crate::{ADAPTIVE_DOWN, ADAPTIVE_UP, SHAPES, STATES, ShapeInfo, StateId, StateInfo, shape::shape_of};

/// A state's row, or none for an id outside this build's registry (read as open, never adaptive).
fn row(state: StateId) -> Option<&'static StateInfo> {
    STATES.get(usize::from(state))
}

/// Neighbour offsets `[dx, dz]` in table index order: NW, N, NE, W, E, SW, S, SE (north = −z).
pub const OFFSETS: [[i32; 2]; 8] = [
    [-1, -1],
    [0, -1],
    [1, -1],
    [-1, 0],
    [1, 0],
    [-1, 1],
    [0, 1],
    [1, 1],
];
pub const SAME: usize = 0;
pub const LOWER: usize = 1;
pub const HIGHER: usize = 2;

/// Whether a cell counts as filled when comparing column heights (§3 "Solid"): an adaptive block
/// (whatever shape it takes) or any solid block whose own shape fills its cell; a partial shape
/// (a slab, a placed slope) counts as open, so ground beside it is *lower*.
pub fn fills(state: StateId) -> bool {
    row(state).is_some_and(|s| s.solid && (s.adaptive || is_cube(shape_of(state))))
}

fn is_cube(s: &ShapeInfo) -> bool {
    s.face_count > 0 && !s.fan && s.corners == [2, 2, 2, 2]
}

/// The table index of the 8 codes (`SAME`, `LOWER`, `HIGHER`) in [`OFFSETS`] order.
pub fn index(codes: [usize; 8]) -> usize {
    codes.iter().rev().fold(0, |i, &c| i * 3 + c)
}

/// The derived shape of an adaptive top block: `Some(index into SHAPES)`, or `None` when the
/// cell keeps its own shape (not adaptive, or something solid above it). `get(dx, dy, dz)` reads
/// the cell at that offset; `up` is +1 where up is +y (face A, or a face-B chunk meshed as its
/// mirror image) and −1 for face B's cells read in the world, whose shapes then hang.
pub fn derive(mut get: impl FnMut(i32, i32, i32) -> StateId, up: i32) -> Option<u16> {
    if !row(get(0, 0, 0)).is_some_and(|s| s.adaptive) || row(get(0, up, 0)).is_some_and(|s| s.solid)
    {
        return None;
    }
    let mut codes = [SAME; 8];
    for (code, [dx, dz]) in codes.iter_mut().zip(OFFSETS) {
        *code = if fills(get(dx, up, dz)) {
            HIGHER
        } else if !fills(get(dx, 0, dz)) {
            LOWER
        } else {
            SAME
        };
    }
    let i = index(codes);
    Some(if up > 0 { ADAPTIVE_UP[i] } else { ADAPTIVE_DOWN[i] })
}

/// The shape a cell is drawn, collided and targeted as: its derived shape, or its own.
pub fn shape_at(mut get: impl FnMut(i32, i32, i32) -> StateId, up: i32) -> &'static ShapeInfo {
    match derive(&mut get, up) {
        Some(i) => &SHAPES[usize::from(i)],
        None => shape_of(get(0, 0, 0)),
    }
}

/// Whether a cell holds water in its open part: a flooded state, or an adaptive top block with
/// a liquid above it or beside it in its row (a shore's shaped bank), unless it derives a full
/// cube.
pub fn flooded_at(mut get: impl FnMut(i32, i32, i32) -> StateId, up: i32) -> bool {
    if row(get(0, 0, 0)).is_some_and(|s| s.flooded) {
        return true;
    }
    let Some(i) = derive(&mut get, up) else {
        return false;
    };
    if is_cube(&SHAPES[usize::from(i)]) {
        return false;
    }
    let liquid = |m: StateId| row(m).is_some_and(|s| s.liquid);
    liquid(get(0, up, 0))
        || [[1, 0], [-1, 0], [0, 1], [0, -1]]
            .iter()
            .any(|&[dx, dz]| liquid(get(dx, 0, dz)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::require;

    /// A little column world: heights per (x, z) around the origin, one state everywhere solid.
    fn columns(heights: [[i32; 3]; 3], top: i32) -> impl Fn(i32, i32, i32) -> StateId {
        let stone = require("dwell:stone");
        move |dx, dy, dz| {
            let h = heights[(dz + 1) as usize][(dx + 1) as usize];
            if top + dy < h { stone } else { 0 }
        }
    }

    #[test]
    fn a_flat_field_rests_at_half_height_and_a_buried_block_is_a_cube() {
        let get = columns([[1; 3]; 3], 0);
        let s = shape_at(&get, 1);
        assert_eq!(s.points, [[2; 3]; 3]);
        assert_eq!(s.max_y, 0.5);
        // With a block above it, a cube.
        let below = columns([[2; 3]; 3], 0);
        assert_eq!(derive(&below, 1), None);
        assert_eq!(shape_at(&below, 1).max_y, 1.0);
    }

    #[test]
    fn a_lone_block_is_a_half_pyramid_and_hangs_on_face_b() {
        let get = columns([[0, 0, 0], [0, 1, 0], [0, 0, 0]], 0);
        let up = &SHAPES[usize::from(derive(&get, 1).expect("adaptive"))];
        assert!(up.fan && !up.inverted);
        assert_eq!(up.points, [[0, 0, 0], [0, 2, 0], [0, 0, 0]]);
        // Face B reads its neighbours toward −y: the same columns mirrored give the hanging shape.
        let stone = require("dwell:stone");
        let mirrored = move |dx: i32, dy: i32, dz: i32| {
            if dx == 0 && dz == 0 && dy == 0 { stone } else { 0 }
        };
        let down = &SHAPES[usize::from(derive(mirrored, -1).expect("adaptive"))];
        assert!(down.inverted && down.fan);
        assert_eq!(down.points, up.points);
    }

    #[test]
    fn non_adaptive_blocks_and_water_keep_their_shapes() {
        let water = require("dwell:water");
        assert_eq!(derive(|_, _, _| water, 1), None);
        assert!(!fills(water));
        assert!(fills(require("dwell:grass")));
    }

    #[test]
    fn a_shaped_bank_beside_water_is_flooded() {
        let (sand, water) = (require("dwell:sand"), require("dwell:water"));
        let bank = move |dx: i32, dy: i32, dz: i32| match (dx, dy, dz) {
            (0, 0, 0) | (-1, 0, _) => sand,
            (1, 0, _) => water,
            _ => 0,
        };
        assert!(flooded_at(bank, 1));
        let dry = move |dx: i32, dy: i32, _dz: i32| if dy == 0 && dx <= 0 { sand } else { 0 };
        assert!(!flooded_at(dry, 1));
    }
}
