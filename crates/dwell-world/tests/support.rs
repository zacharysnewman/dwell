//! Support (docs/FELLING.md §2): logs carry, leaves ride, and what loses its hold falls as pieces.
//! Support has no up or down (a log anchors on ground touching it from any side), so these tests
//! need no face frame.

use std::sync::Arc;

use dwell_blocks::{StateId, require};
use dwell_world::support::{Piece, SupportRules, detached_pieces};
use dwell_world::{M, VoxelWorld};

fn log() -> StateId {
    dwell_blocks::parse_state("dwell:log[axis=y]").expect("a log")
}
fn leaves() -> StateId {
    require("dwell:leaves")
}

/// An empty world with a stone floor at row 0 over x, z in −20..20.
fn world() -> VoxelWorld {
    let mut w = VoxelWorld::new(Arc::new(|_, _| {}), None);
    for z in -20..20 {
        for x in -20..20 {
            w.set_voxel(x, 0, z, M.stone);
        }
    }
    w
}

/// A tree at (x, z): a trunk from row 1 up `height` rows, a 3 × 3 crown of leaves round its top
/// three rows, and leaves capping it. Returns the trunk's cells.
fn tree(w: &mut VoxelWorld, x: i32, z: i32, height: i32) -> Vec<[i32; 3]> {
    let trunk: Vec<[i32; 3]> = (1..=height).map(|y| [x, y, z]).collect();
    for &[x, y, z] in &trunk {
        w.set_voxel(x, y, z, log());
    }
    for y in height - 2..=height {
        for dz in -1..=1 {
            for dx in -1..=1 {
                if (dx, dz) != (0, 0) {
                    w.set_voxel(x + dx, y, z + dz, leaves());
                }
            }
        }
    }
    w.set_voxel(x, height + 1, z, leaves());
    trunk
}

fn cut(w: &mut VoxelWorld, at: [i32; 3]) -> Vec<Piece> {
    w.set_voxel(at[0], at[1], at[2], M.air);
    detached_pieces(w, &[at], SupportRules::default())
}

#[test]
fn a_standing_tree_holds_and_a_cut_trunk_fells_its_crown() {
    let mut w = world();
    tree(&mut w, 0, 0, 8);
    // Breaking a leaf: the tree still stands, nothing falls.
    assert!(cut(&mut w, [1, 8, 0]).is_empty());
    // Cutting the trunk at row 2: the log under the cut stands on the floor; everything over it
    // (rows 3–8 of trunk, its 24 remaining leaves) falls as one piece, carriers first.
    let pieces = cut(&mut w, [0, 2, 0]);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    let p = &pieces[0];
    assert_eq!(p.carriers, 6);
    assert!(
        p.cells[..6]
            .iter()
            .all(|c| c[0] == 0 && c[2] == 0 && c[1] >= 3)
    );
    // Three rows of eight round the trunk and the cap, less the broken one.
    assert_eq!(
        p.cells.len() - p.carriers,
        3 * 8 + 1 - 1,
        "leaves: {:?}",
        &p.cells[6..]
    );
    // The stump (row 1, on the floor) was never part of it.
    assert!(!p.cells.contains(&[0, 1, 0]));
}

#[test]
fn leaves_never_hold_up_a_neighbours_crown() {
    // Two trees whose crowns touch: cutting one trunk fells its own crown, the leaves split between
    // the trunks by nearness; the other tree stands, all of it.
    let mut w = world();
    tree(&mut w, 0, 0, 8);
    tree(&mut w, 3, 0, 8);
    let pieces = cut(&mut w, [0, 1, 0]);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    let p = &pieces[0];
    assert_eq!(p.carriers, 7);
    assert!(
        p.cells.iter().all(|c| c[0] <= 1),
        "only the cut tree's side: {:?}",
        p.cells
    );
    // The other tree's trunk is all there.
    assert!((1..=8).all(|y| w.get_voxel(3, y, 0) == log()));
}

#[test]
fn a_log_holds_on_ground_beside_it_and_a_branch_falls_with_its_leaves() {
    // A branch: logs out sideways from a trunk, leaves at its end. Cutting where it meets the
    // trunk fells the branch and its leaves; the trunk stands.
    let mut w = world();
    tree(&mut w, 0, 0, 8);
    for x in 1..=4 {
        w.set_voxel(x, 5, 0, log());
    }
    w.set_voxel(5, 5, 0, leaves());
    w.set_voxel(4, 6, 0, leaves());
    let pieces = cut(&mut w, [1, 5, 0]);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    assert_eq!(pieces[0].carriers, 3);
    assert!(pieces[0].cells.contains(&[5, 5, 0]) && pieces[0].cells.contains(&[4, 6, 0]));
    // A log touching a stone wall from the side is anchored by it (support has no down).
    let mut w = world();
    for y in 1..=6 {
        w.set_voxel(-5, y, 0, M.stone);
    }
    w.set_voxel(-4, 5, 0, log());
    w.set_voxel(-3, 5, 0, log());
    assert!(cut(&mut w, [-3, 6, 0]).is_empty());
}

#[test]
fn leaves_with_no_log_in_reach_fall_on_their_own() {
    let mut w = world();
    // Leaves hanging off a stone ledge hold nothing and nothing holds them.
    for x in 0..3 {
        w.set_voxel(x, 6, 5, leaves());
    }
    w.set_voxel(-1, 6, 5, M.stone);
    let pieces = cut(&mut w, [3, 6, 5]);
    assert_eq!(
        pieces,
        vec![Piece {
            cells: vec![[0, 6, 5], [1, 6, 5], [2, 6, 5]],
            carriers: 0
        }]
    );
    // A leaf farther than the reach from every log falls on its own when the change reaches it.
    let mut w = world();
    tree(&mut w, 0, 0, 8);
    let reach = SupportRules::default().rider_reach as i32;
    for x in 2..=reach + 2 {
        w.set_voxel(x, 8, 0, leaves());
    }
    w.set_voxel(1, 8, 0, M.air);
    w.set_voxel(1, 8, 0, leaves());
    let pieces = detached_pieces(&mut w, &[[reach + 2, 8, 0]], SupportRules::default());
    assert!(
        pieces.iter().all(|p| p.carriers == 0)
            && pieces.iter().any(|p| p.cells.contains(&[reach + 2, 8, 0])),
        "{pieces:?}"
    );
}

#[test]
fn a_system_larger_than_the_bound_is_left_standing() {
    let mut w = world();
    tree(&mut w, 0, 0, 12);
    let rules = SupportRules {
        max_cells: 10,
        ..SupportRules::default()
    };
    w.set_voxel(0, 1, 0, M.air);
    assert!(detached_pieces(&mut w, &[[0, 1, 0]], rules).is_empty());
    // Within the bound it falls.
    assert_eq!(
        detached_pieces(&mut w, &[[0, 1, 0]], SupportRules::default()).len(),
        1
    );
}

#[test]
fn the_same_cut_gives_the_same_pieces() {
    let build = || {
        let mut w = world();
        tree(&mut w, 0, 0, 9);
        tree(&mut w, 2, 1, 7);
        w
    };
    let (mut a, mut b) = (build(), build());
    assert_eq!(cut(&mut a, [0, 3, 0]), cut(&mut b, [0, 3, 0]));
}
