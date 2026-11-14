//! Placement rules (docs/ITEMS.md; owner 2026-10-26): pieces 1 m (two ½ m cells) or ½ m tall,
//! against the face aimed at; any block on a slope's top (0.39.0), and a moldable material of the
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
        point: None,
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
    /// A block of another material is placed on a slope's top, in the cell above it, the slope
    /// staying under it (owner, 2026-10-28: "make it so we can build on slopes again"; it was
    /// refused, `OnSlope`, red before); a moldable material of the slope's own group (dirt, grass
    /// and snowy grass are dirt; sand) replaces the slope, as before; a plant stands on it.
    fn slopes_are_built_on_and_molded_by_their_own_material(f: FaceFrame) {
        let mut w = ground(f);
        set(&mut w, f, [1, 0, 2], ramp("dwell:dirt_ramp"));
        let top = 2;
        for m in ["dwell:stone", "dwell:sand"] {
            let o = place(&mut w, f, [1, 0, 2], top, s(m), 1, false);
            assert_eq!(o.check, EditCheck::Ok, "{m} on a dirt slope");
            assert_eq!(cells(f, &o), vec![[1, 1, 2]], "{m} on top of it");
        }
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
        let sand = place(&mut w, f, [2, 0, 2], top, s("dwell:sand"), 1, false);
        assert_eq!(
            (sand.check, cells(f, &sand)),
            (EditCheck::Ok, vec![[2, 0, 2]])
        );
    }
);

dwell_world::both_faces!(
    /// A slope placed on a slope of its own material (the hotbar's stone slopes, owner
    /// 2026-10-30) is built on, not molded: the steep slope's upper half goes on its lower half,
    /// and a slope on a slope's top. In a molding group a full block still molds a slope, a slope
    /// doesn't (red before: `molds_into` held for two dirt slopes).
    fn a_slope_placed_on_a_slope_is_built_on(f: FaceFrame) {
        let half = |shape: &str| {
            f.state(s(&format!(
                "dwell:stone_ramp[facing=north,flooded=false,half=bottom,shape={shape}]"
            )))
        };
        let mut w = ground(f);
        set(&mut w, f, [1, 0, 2], half("steep_lower"));
        let top = 2;
        let o = place(&mut w, f, [1, 0, 2], top, half("steep_upper"), 1, false);
        assert_eq!(o.check, EditCheck::Ok);
        assert_eq!(cells(f, &o), vec![[1, 1, 2]], "the upper half on the lower");
        let o = place(&mut w, f, [1, 0, 2], top, half("ramp"), 1, false);
        assert_eq!(cells(f, &o), vec![[1, 1, 2]], "a slope on a slope");
        // Stone has no molding group: a stone block goes on top too.
        let o = place(&mut w, f, [1, 0, 2], top, s("dwell:stone"), 1, false);
        assert_eq!(cells(f, &o), vec![[1, 1, 2]], "stone on the stone slope");
        // In a molding group (dirt), a slope doesn't mold a slope; a full block does.
        let dirt = |shape: &str| {
            f.state(s(&format!(
                "dwell:dirt_ramp[facing=north,flooded=false,half=bottom,shape={shape}]"
            )))
        };
        assert!(!dwell_world::edit::molds_into(
            dirt("steep_lower"),
            dirt("steep_upper")
        ));
        assert!(dwell_world::edit::molds_into(
            dirt("steep_lower"),
            s("dwell:dirt")
        ));
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
                point: None,
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

dwell_world::both_faces!(
    /// Regression (owner, 2026-10-28): breaking the block under a ladder — the ground, or the
    /// ladder below — took every ladder above it. A ladder hangs on its wall, not on what is under
    /// it: they stay. Red before: the cascade asked each ladder to stand on the cell under it.
    fn a_ladder_stays_when_what_is_under_it_breaks(f: FaceFrame) {
        let ladder = s("dwell:ladder[facing=north]");
        let mut w = ground(f);
        // A wall at z = 3 and a ladder column on it, rows 0–3, the lowest on the ground.
        for row in 0..4 {
            set(&mut w, f, [0, row, 3], s("dwell:stone"));
            set(&mut w, f, [0, row, 2], ladder);
        }
        // The ground under the column broken: only that cell changes.
        let ground_gone = edit_writes(&mut w, f.cell([0, -1, 2]), s("dwell:air"));
        assert_eq!(ground_gone, vec![(f.cell([0, -1, 2]), s("dwell:air"))]);
        // The lowest ladder broken: the ones above stay.
        let ladder_gone = edit_writes(&mut w, f.cell([0, 0, 2]), s("dwell:air"));
        assert_eq!(ladder_gone, vec![(f.cell([0, 0, 2]), s("dwell:air"))]);
        // Its wall broken: that ladder goes (and only it: the others keep their walls).
        let wall_gone = edit_writes(&mut w, f.cell([0, 2, 3]), s("dwell:air"));
        assert_eq!(
            wall_gone,
            vec![
                (f.cell([0, 2, 3]), s("dwell:air")),
                (f.cell([0, 2, 2]), s("dwell:air"))
            ]
        );
    }
);

dwell_world::both_faces!(
    /// Leaves don't block a body (players pass through them): a leaf may go, or land, where a
    /// player stands; stone may not. Red before (owner, 2026-10-28): a leaf landing beside the
    /// player who placed it was destroyed as if it were stone in their body.
    fn leaves_never_overlap_a_player(f: FaceFrame) {
        let body = EditCapsule {
            center: f.point([0.5, 0.9, 0.5]),
            radius: 0.3,
            half_cylinder: 0.6,
        };
        let cell = f.cell([0, 1, 0]);
        assert!(!overlaps_player(s("dwell:leaves"), cell, &[body]));
        assert!(overlaps_player(s("dwell:stone"), cell, &[body]));
    }
);

dwell_world::both_faces!(
    /// Dynamite is lit by a torch held against it (docs/EXPLOSIVES.md §2): a Place of a torch aimed
    /// at it turns it into its lit state, in its own cell, and places no torch; any other block is
    /// placed against it as usual; lit, nothing breaks it.
    fn a_torch_held_against_dynamite_lights_it(f: FaceFrame) {
        let mut w = ground(f);
        set(&mut w, f, [0, 0, 2], s("dwell:dynamite"));
        let o = place(&mut w, f, [0, 0, 2], 5, s("dwell:torch"), 1, false);
        assert_eq!(o.check, EditCheck::Ok);
        assert_eq!(cells(f, &o), vec![[0, 0, 2]], "the dynamite's own cell");
        assert_eq!(o.material, f.state(s("dwell:lit_dynamite")));
        let writes = o.writes(&mut w, medium_kept);
        assert_eq!(
            writes,
            vec![(f.cell([0, 0, 2]), f.state(s("dwell:lit_dynamite")))]
        );
        // Stone goes against it.
        let o = place(&mut w, f, [0, 0, 2], 5, s("dwell:stone"), 1, false);
        assert_eq!(cells(f, &o), vec![[0, 0, 1]]);
        // Lit: a break is refused (its fuse can't be pulled).
        set(&mut w, f, [0, 0, 2], s("dwell:lit_dynamite"));
        let r = BlockEditRequest {
            action: BlockEditAction::Break,
            cell: f.cell([0, 0, 2]),
            face: f.cell_face(5) as u8,
            material: 0,
            rows: 1,
            down: false,
            point: None,
        };
        let o = check_block_edit(&mut w, &r, f.point([0.5, 1.62, 0.5]), &[], DEFAULT_REACH);
        assert_eq!(o.check, EditCheck::Unbreakable);
    }
);

/// Aims from face-local `eye` at face-local point `at` (m) and places `rows` of stone against the
/// face the hit gives a place (a slope's side on its lower half, `BlockHit::slope_side`): the
/// cell aimed at, the face sent, and the outcome.
fn aim_place(
    w: &mut VoxelWorld,
    f: FaceFrame,
    eye: [f64; 3],
    at: [f64; 3],
    rows: u8,
) -> ([i32; 3], u8, EditOutcome) {
    let d: [f64; 3] = std::array::from_fn(|k| at[k] - eye[k]);
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let dir = f.vector(d.map(|v| v / n)).map(|v| v as f32);
    let hit = raycast_block(w, f.point(eye), dir, 8.0).expect("the slope is aimed at");
    let face = hit.slope_side.unwrap_or(hit.face);
    let r = BlockEditRequest {
        action: BlockEditAction::Place,
        cell: hit.cell,
        face,
        material: f.state(s("dwell:stone")),
        rows,
        down: false,
        point: None,
    };
    let o = check_block_edit(w, &r, f.point(eye), &[], DEFAULT_REACH);
    (f.cell(hit.cell), face, o)
}

dwell_world::both_faces!(
    /// docs/BUGS.md #23 (owner, 2026-11-03): "I can't place a block against the outer edge of
    /// (downhill) side of a shallow ramp, maybe same problem with steep ramps too?" A slope's face
    /// was always its top, so a place went on top of the ramp, never off its low edge. Now a place
    /// goes into the cell the slope faces out into where it is aimed at: from its lower half, out
    /// past its low edge into the downhill cell (the server taking the slope as seen from there);
    /// from its upper half, on top as before. Red before: the downhill places went on top.
    fn a_place_aimed_at_a_ramps_lower_half_goes_off_its_downhill_side(f: FaceFrame) {
        let ramp = |shape: &str| {
            s(&format!(
                "dwell:stone_ramp[facing=north,flooded=false,half=bottom,shape={shape}]"
            ))
        };
        let side = f.cell_face(5) as u8; // −z, the side it descends toward (north)
        let eye = [1.5, 1.62, 0.0];
        // A ½ m ramp in (1, 0, 2): its slope rises 0.5 m from z 2 to 3.
        let mut w = ground(f);
        set(&mut w, f, [1, 0, 2], ramp("ramp"));
        let (cell, face, o) = aim_place(&mut w, f, eye, [1.5, 0.1, 2.2], 1);
        assert_eq!(
            (cell, face),
            ([1, 0, 2], side),
            "its lower half: the low side"
        );
        assert_eq!(o.check, EditCheck::Ok);
        assert_eq!(cells(f, &o), vec![[1, 0, 1]], "off the downhill edge");
        // A 1 m piece there stands on the ground, up from the cell.
        let (_, _, o) = aim_place(&mut w, f, eye, [1.5, 0.1, 2.2], 2);
        assert_eq!(cells(f, &o), vec![[1, 0, 1], [1, 1, 1]]);
        let (_, face, o) = aim_place(&mut w, f, eye, [1.5, 0.4, 2.8], 1);
        assert_eq!(face, f.cell_face(2) as u8, "its upper half: the top");
        assert_eq!(cells(f, &o), vec![[1, 1, 2]], "on top, as before");
        // A 1 m (45°) ramp: its lower piece's slope from z 2 to 2.5, its upper's from 2.5 to 3.
        let mut w = ground(f);
        set(&mut w, f, [1, 0, 2], ramp("steep_lower"));
        set(&mut w, f, [1, 1, 2], ramp("steep_upper"));
        let (cell, face, o) = aim_place(&mut w, f, eye, [1.5, 0.1, 2.1], 1);
        assert_eq!((cell, face, o.check), ([1, 0, 2], side, EditCheck::Ok));
        assert_eq!(cells(f, &o), vec![[1, 0, 1]], "off the steep ramp's foot");
        let (cell, face, o) = aim_place(&mut w, f, eye, [1.5, 0.6, 2.6], 1);
        assert_eq!((cell, face, o.check), ([1, 1, 2], side, EditCheck::Ok));
        assert_eq!(cells(f, &o), vec![[1, 1, 1]], "beside its upper piece");
    }
);

#[test]
fn a_flat_face_or_a_slopes_top_half_has_no_side() {
    // face-a-only: cell-space arithmetic, no up.
    assert_eq!(slope_side([0.0, 1.0, 0.0], [0.5, 1.0, 0.1]), None, "a top");
    assert_eq!(
        slope_side([0.0, 0.0, -1.0], [0.5, 0.5, 0.0]),
        None,
        "a side"
    );
    // A ½ m ramp descending −z (cell-space normal (0, 1, −1)): out the side under half way.
    let n = [0.0, 1.0, -1.0];
    assert_eq!(slope_side(n, [0.5, 0.2, 0.2]), Some(5));
    assert_eq!(slope_side(n, [0.5, 0.5, 0.5]), None, "a tie: the top");
    assert_eq!(slope_side(n, [0.5, 0.8, 0.8]), None);
    // Hanging (face B): out its bottom or its side.
    assert_eq!(slope_side([0.0, -1.0, 1.0], [0.5, 0.8, 0.8]), Some(4));
}
