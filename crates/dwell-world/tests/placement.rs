//! Placement rules (docs/ITEMS.md; owner 2026-10-26): pieces 1 m (two ½ m cells) or ½ m tall,
//! against the face aimed at; nothing but plants on a slope's top, and a moldable material of the
//! slope's own group replacing it; ladders hung on a block's side and gone with it. All face-local,
//! on both faces.

use dwell_blocks::{StateId, require};
use dwell_protocol::{BlockEditAction, BlockEditRequest};
use dwell_world::edit::*;
use dwell_world::*;

fn s(name: &str) -> StateId {
    require(name)
}

fn ramp(block: &str) -> StateId {
    dwell_blocks::find_block(block).expect(block).first_state
}

/// Grass ground at face-local row −1 round the origin.
fn ground(f: FaceFrame) -> VoxelWorld {
    let mut w = VoxelWorld::new(empty_generator(), None);
    for x in -4..=4 {
        for z in -4..=8 {
            let [cx, cy, cz] = f.cell([x, -1, z]);
            w.set_voxel(cx, cy, cz, f.state(s("dwell:grass")));
        }
    }
    w
}

fn set(w: &mut VoxelWorld, f: FaceFrame, at: [i32; 3], m: StateId) {
    let [x, y, z] = f.cell(at);
    w.set_voxel(x, y, z, f.state(m));
}

/// Places `m` against face-local `face` of face-local `cell`, `rows` tall, seen from the eye.
fn place(
    w: &mut VoxelWorld,
    f: FaceFrame,
    cell: [i32; 3],
    face: usize,
    m: StateId,
    rows: u8,
    down: bool,
) -> EditOutcome {
    let r = BlockEditRequest {
        action: BlockEditAction::Place,
        cell: f.cell(cell),
        face: f.cell_face(face) as u8,
        material: f.state(m),
        rows,
        down,
    };
    check_block_edit(w, &r, f.point([0.5, 1.62, 0.5]), &[], DEFAULT_REACH)
}

fn cells(f: FaceFrame, o: &EditOutcome) -> Vec<[i32; 3]> {
    o.cells().map(|c| f.cell(c)).collect()
}

dwell_world::both_faces!(
    /// A 1 m piece fills the two cells against the face aimed at: away from a top face; up from
    /// a side, or down with `down`; a ½ m piece one cell; and it needs both free.
    fn pieces_are_one_or_two_cells_against_the_face(f: FaceFrame) {
        let dirt = s("dwell:dirt");
        let mut w = ground(f);
        let o = place(&mut w, f, [0, -1, 2], 2, dirt, 2, false);
        assert_eq!(o.check, EditCheck::Ok);
        assert_eq!(cells(f, &o), vec![[0, 0, 2], [0, 1, 2]]);
        let o = place(&mut w, f, [0, -1, 2], 2, dirt, 1, false);
        assert_eq!(cells(f, &o), vec![[0, 0, 2]], "½ m");
        // Against a stone's side (its −Z face), up from the placed cell or down from it.
        set(&mut w, f, [0, 2, 3], s("dwell:stone"));
        let up = place(&mut w, f, [0, 2, 3], 5, dirt, 2, false);
        assert_eq!(cells(f, &up), vec![[0, 2, 2], [0, 3, 2]]);
        let down = place(&mut w, f, [0, 2, 3], 5, dirt, 2, true);
        assert_eq!(cells(f, &down), vec![[0, 2, 2], [0, 1, 2]]);
        // The second cell taken: refused.
        set(&mut w, f, [0, 1, 2], s("dwell:stone"));
        assert_eq!(
            place(&mut w, f, [0, -1, 2], 2, dirt, 2, false).check,
            EditCheck::Occupied
        );
        // Written as one edit: both cells.
        let writes = up.writes(&mut w, medium_kept);
        assert!(
            writes.iter().any(|(c, _)| f.cell(*c) == [0, 3, 2]),
            "{writes:?}"
        );
    }
);

dwell_world::both_faces!(
    /// Nothing but plants is placed on a slope's top; a moldable material of the slope's group
    /// (dirt, grass and snowy grass are dirt; sand) replaces the slope instead.
    fn slopes_take_plants_and_are_molded_by_their_own_material(f: FaceFrame) {
        let mut w = ground(f);
        set(&mut w, f, [1, 0, 2], ramp("dwell:dirt_ramp"));
        let top = 2;
        assert_eq!(
            place(&mut w, f, [1, 0, 2], top, s("dwell:stone"), 2, false).check,
            EditCheck::OnSlope
        );
        assert_eq!(
            place(&mut w, f, [1, 0, 2], top, s("dwell:sand"), 2, false).check,
            EditCheck::OnSlope
        );
        for m in ["dwell:dirt", "dwell:grass"] {
            let o = place(&mut w, f, [1, 0, 2], top, s(m), 2, false);
            assert_eq!(o.check, EditCheck::Ok, "{m}");
            assert_eq!(
                cells(f, &o),
                vec![[1, 0, 2], [1, 1, 2]],
                "{m} replaces the slope"
            );
        }
        let plant = place(&mut w, f, [1, 0, 2], top, s("dwell:tall_grass"), 1, false);
        assert_eq!(plant.check, EditCheck::Ok);
        assert_eq!(cells(f, &plant), vec![[1, 1, 2]], "a plant on top of it");
        // Sand molds a sand slope.
        set(&mut w, f, [2, 0, 2], ramp("dwell:sand_ramp"));
        assert_eq!(
            place(&mut w, f, [2, 0, 2], top, s("dwell:sand"), 1, false).check,
            EditCheck::Ok
        );
    }
);

dwell_world::both_faces!(
    /// A ladder hangs on the side of a full block it was placed against (each cell of a 1 m one
    /// needs its block), never on a top; breaking the block takes the ladder with it.
    fn ladders_hang_on_a_blocks_side_and_go_with_it(f: FaceFrame) {
        let ladder = s("dwell:ladder[facing=north]");
        let mut w = ground(f);
        assert_eq!(
            place(&mut w, f, [0, -1, 2], 2, ladder, 1, false).check,
            EditCheck::NoSupport,
            "on a top"
        );
        set(&mut w, f, [0, 0, 3], s("dwell:stone"));
        let one = place(&mut w, f, [0, 0, 3], 5, ladder, 1, false);
        assert_eq!(
            (one.check, cells(f, &one)),
            (EditCheck::Ok, vec![[0, 0, 2]])
        );
        assert_eq!(
            place(&mut w, f, [0, 0, 3], 5, ladder, 2, false).check,
            EditCheck::NoSupport,
            "nothing behind its top"
        );
        set(&mut w, f, [0, 1, 3], s("dwell:stone"));
        assert_eq!(
            place(&mut w, f, [0, 0, 3], 5, ladder, 2, false).check,
            EditCheck::Ok
        );
        // Hung, then its block broken: the ladder goes too.
        set(&mut w, f, [0, 0, 2], ladder);
        let gone = edit_writes(&mut w, f.cell([0, 0, 3]), s("dwell:air"));
        assert!(
            gone.contains(&(f.cell([0, 0, 2]), s("dwell:air"))),
            "{gone:?}"
        );
        // A ramp is no wall.
        set(
            &mut w,
            f,
            [-1, 0, 3],
            s("dwell:stone_ramp[facing=south,flooded=false,half=top,shape=ramp]"),
        );
        assert_eq!(
            place(&mut w, f, [-1, 0, 3], 5, ladder, 1, false).check,
            EditCheck::NoSupport
        );
    }
);

dwell_world::both_faces!(
    /// A break takes one cell whatever the held item is (owner, 2026-10-28: the 1 m break, both
    /// cells of a 1 m piece, is gone): a 1 m request breaks the aimed-at cell only.
    fn a_break_takes_one_cell_whatever_the_piece(f: FaceFrame) {
        let mut w = ground(f);
        set(&mut w, f, [0, 0, 2], s("dwell:dirt"));
        set(&mut w, f, [0, 1, 2], s("dwell:dirt"));
        let brk = |w: &mut VoxelWorld, rows: u8, face: usize, down: bool| {
            let r = BlockEditRequest {
                action: BlockEditAction::Break,
                cell: f.cell([0, 1, 2]),
                face: f.cell_face(face) as u8,
                material: 0,
                rows,
                down,
            };
            check_block_edit(w, &r, f.point([0.5, 1.62, -0.5]), &[], DEFAULT_REACH)
        };
        // From the top, and against a side (up, and down with `down`).
        for (face, down) in [(2, false), (5, false), (5, true)] {
            let o = brk(&mut w, 2, face, down);
            assert_eq!(
                (o.check, cells(f, &o)),
                (EditCheck::Ok, vec![[0, 1, 2]]),
                "face {face}, down {down}"
            );
            assert_eq!(cells(f, &brk(&mut w, 1, face, down)), vec![[0, 1, 2]]);
        }
    }
);
