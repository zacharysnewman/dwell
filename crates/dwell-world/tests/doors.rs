//! Doors (owner, 2026-11-14: "Let's just do 3 blocks. Plank floors, stone brick walls, thatch roof.
//! And doors also." / "2m doors."; docs/BLOCK_REGISTRY.md, Doors): `dwell:door` is a column of
//! four ½ m cells (block data `column`), placed and cleared whole; a Use (a Place aimed at it,
//! whatever is held) flips `open` on every cell (`toggle`); an open door has no collision
//! (`swing`). All through the block data and `dwell_world::parts`, no door code.

use dwell_blocks::{StateId, info, require, state_property, with_property};
use dwell_protocol::{BlockEditAction, BlockEditRequest};
use dwell_world::edit::*;
use dwell_world::parts::{lone_parts, partners};
use dwell_world::*;

fn s(name: &str) -> StateId {
    require(name)
}

fn door(part: &str, open: bool) -> StateId {
    s(&format!(
        "dwell:door[facing=north,hinge=left,open={open},part={part}]"
    ))
}

const PARTS: [&str; 4] = ["bottom", "lower", "upper", "top"];

fn apply(w: &mut VoxelWorld, writes: &[([i32; 3], StateId)]) {
    for &(c, m) in writes {
        w.set_voxel(c[0], c[1], c[2], m);
    }
}

fn place(cell: [i32; 3], face: u8, material: StateId) -> BlockEditRequest {
    BlockEditRequest {
        action: BlockEditAction::Place,
        cell,
        face,
        material,
        rows: 1,
        down: false,
        point: None,
    }
}

/// A world with stone under a door's column at face-local (0, 31..35, 2), the door placed whole by
/// an edit of its bottom cell: (world, the four cells bottom to top).
fn door_world(f: FaceFrame) -> (VoxelWorld, [[i32; 3]; 4]) {
    let mut w = VoxelWorld::new(empty_generator(), None);
    let g = f.cell([0, 30, 2]);
    w.set_voxel(g[0], g[1], g[2], s("dwell:stone"));
    let cells = std::array::from_fn(|k| f.cell([0, 31 + k as i32, 2]));
    let put = edit_writes(&mut w, cells[0], door("bottom", false));
    apply(&mut w, &put);
    (w, cells)
}

dwell_world::both_faces!(
    /// On face `f`: placing a door's bottom cell puts all four, each its own `part`, bottom to top
    /// up the face; breaking any one cell clears all four in the same edit's writes.
    fn a_door_is_one_block_over_four_cells(f: FaceFrame) {
        let (mut w, cells) = door_world(f);
        for (k, c) in cells.iter().enumerate() {
            assert_eq!(w.get_voxel(c[0], c[1], c[2]), door(PARTS[k], false), "cell {k}");
        }
        assert_eq!(partners(cells[1], door("lower", false)).len(), 3);
        for target in cells {
            let (mut w2, _) = door_world(f);
            let gone = edit_writes(&mut w2, target, s("dwell:air"));
            let mut got: Vec<[i32; 3]> = gone.iter().map(|(c, _)| *c).collect();
            got.sort_unstable();
            let mut want = cells.to_vec();
            want.sort_unstable();
            assert_eq!(got, want, "breaking {target:?}");
        }
        // A cell written over by anything else (a blast's air) leaves the rest lone: all cleared.
        w.set_voxel(cells[2][0], cells[2][1], cells[2][2], s("dwell:air"));
        let mut lone: Vec<[i32; 3]> = lone_parts(&w, &[cells[2]]).iter().map(|(c, _)| *c).collect();
        lone.sort_unstable();
        let mut want = vec![cells[0], cells[1], cells[3]];
        want.sort_unstable();
        assert_eq!(lone, want);
    }
);

dwell_world::both_faces!(
    /// On face `f`: a Use (a Place aimed at any cell of a door, with stone or nothing held) opens
    /// every cell of it, nothing placed (`toggled`); a second closes it again. Open, no cell
    /// collides; closed, each does. Closing on a player standing in the doorway waits
    /// (`IntoPlayer`), as a place into a body does.
    fn a_use_opens_and_closes_the_whole_door(f: FaceFrame) {
        let (mut w, cells) = door_world(f);
        // The eye 3 m in front of the door, level with its middle.
        let eye = f.point([0.5, m_of_row(33), -1.0]);
        let face = f.cell_face(5) as u8; // its -z side, toward the eye
        for held in [s("dwell:stone"), 0] {
            for target in [cells[0], cells[2]] {
                let out = check_block_edit(&mut w, &place(target, face, held), eye, &[], DEFAULT_REACH);
                assert_eq!(out.check, EditCheck::Ok, "aimed at {target:?} holding {held}");
                assert!(out.toggled);
                assert_eq!(out.cell, target, "nothing placed beside it");
                let writes = out.writes(&mut w, medium_kept);
                assert_eq!(writes.len(), 4, "{writes:?}");
                let was_open = state_property(w.get_voxel(target[0], target[1], target[2]), "open") == Some("true");
                apply(&mut w, &writes);
                for (k, c) in cells.iter().enumerate() {
                    let m = w.get_voxel(c[0], c[1], c[2]);
                    assert_eq!(m, door(PARTS[k], !was_open), "cell {k}");
                    assert_eq!(info(m).solid, was_open, "open: no collision");
                }
            }
        }
        // Open now? Close it once more if not, then open, and stand a body in the doorway.
        if info(w.get_voxel(cells[0][0], cells[0][1], cells[0][2])).solid {
            let out = check_block_edit(&mut w, &place(cells[0], face, 0), eye, &[], DEFAULT_REACH);
            let writes = out.writes(&mut w, medium_kept);
            apply(&mut w, &writes);
        }
        let body = EditCapsule {
            center: f.point([0.5, m_of_row(31) + 0.9, 2.5]),
            radius: 0.3,
            half_cylinder: 0.6,
        };
        let out = check_block_edit(&mut w, &place(cells[0], face, 0), eye, &[body], DEFAULT_REACH);
        assert_eq!(out.check, EditCheck::IntoPlayer);
    }
);

#[test]
fn an_open_door_is_a_plate_turned_toward_its_hinge() {
    // Closed: a full cube that collides; open: the ladder look's plate, no collision, its facing a
    // quarter turn from the door's toward the hinge, and its selection a thin box.
    let closed = info(door("bottom", false));
    assert_eq!((closed.look, closed.solid), ("cube", true));
    let open = info(door("bottom", true));
    assert_eq!((open.look, open.solid), ("ladder", false));
    assert_eq!(open.facing, dwell_blocks::Facing::West);
    let right = with_property(door("bottom", true), "hinge", "right").unwrap();
    assert_eq!(info(right).facing, dwell_blocks::Facing::East);
    let b = open.selection.expect("a plate's box");
    assert!(b[3] - b[0] < 0.2 || b[5] - b[2] < 0.2, "{b:?}");
    // Only the bottom cell stands on what is under it.
    assert_eq!(closed.needs_support, dwell_blocks::Support::Below);
    assert_eq!(info(door("upper", true)).needs_support, dwell_blocks::Support::None);
    // The toggle and the column come from the data.
    assert_eq!(closed.toggle, Some("open"));
    assert_eq!(closed.column.map(|c| (c.index, c.count)), Some((0, 4)));
    assert_eq!(toggled(door("top", false)), Some(door("top", true)));
    assert_eq!(toggled(s("dwell:stone")), None);
}

#[test]
fn the_town_blocks_are_placeable_full_blocks_and_thatch_has_ramps() {
    for name in ["dwell:planks", "dwell:stone_brick", "dwell:thatch"] {
        let m = s(name);
        assert!(placeable(m) && info(m).solid, "{name}");
    }
    let ramp = s("dwell:thatch_ramp[facing=north,flooded=false,half=bottom,shape=ramp]");
    assert!(placeable(ramp));
    assert_eq!(info(ramp).shape, dwell_blocks::VoxelShape::Shaped);
}
