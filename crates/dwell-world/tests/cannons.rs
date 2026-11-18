//! The land cannon's block (docs/CANNONS.md C1; owner, 2026-11-17: "cannon takes up 4 blocks 2m long
//! by 1m tall"; "using the same input as doors, you can fire the cannon, clicking on it. It faces
//! the direction of placement"): four cells, 2 along its facing and 2 up (block data `column` with
//! `columnAlong` 2), placed and cleared whole; a Use (a Place aimed at it) is `used`, nothing
//! written. All through the block data and `dwell_world::parts`.

use dwell_blocks::{StateId, require};
use dwell_protocol::{BlockEditAction, BlockEditRequest};
use dwell_world::cannons::anchor_of;
use dwell_world::edit::*;
use dwell_world::parts::lone_parts;
use dwell_world::*;

fn cannon(part: &str) -> StateId {
    require(&format!("dwell:cannon[facing=north,part={part}]"))
}

fn apply(w: &mut VoxelWorld, writes: &[([i32; 3], StateId)]) {
    for &(c, m) in writes {
        w.set_voxel(c[0], c[1], c[2], m);
    }
}

/// A world with stone under a north-facing cannon placed by its rear bottom cell at face-local
/// (0, 31, 2): (world, its cells rear bottom, rear top, front bottom, front top).
fn cannon_world(f: FaceFrame) -> (VoxelWorld, [[i32; 3]; 4]) {
    let mut w = VoxelWorld::new(empty_generator(), None);
    for z in [1, 2] {
        let g = f.cell([0, 30, z]);
        w.set_voxel(g[0], g[1], g[2], require("dwell:stone"));
    }
    // North is −z: the barrel runs from z 2 to z 1.
    let cells = [
        f.cell([0, 31, 2]),
        f.cell([0, 32, 2]),
        f.cell([0, 31, 1]),
        f.cell([0, 32, 1]),
    ];
    let put = edit_writes(&mut w, cells[0], cannon("rear_bottom"));
    apply(&mut w, &put);
    (w, cells)
}

const PARTS: [&str; 4] = ["rear_bottom", "rear_top", "front_bottom", "front_top"];

dwell_world::both_faces!(
    fn a_cannon_is_one_block_over_four_cells_two_along_and_two_up(f: FaceFrame) {
        let (mut w, cells) = cannon_world(f);
        for (k, c) in cells.iter().enumerate() {
            assert_eq!(w.get_voxel(c[0], c[1], c[2]), cannon(PARTS[k]), "cell {k}");
            // Every cell finds the one it was placed by.
            assert_eq!(anchor_of(*c, cannon(PARTS[k])), cells[0], "anchor from {k}");
        }
        // Breaking any cell clears all four.
        for target in cells {
            let (mut w2, _) = cannon_world(f);
            let gone = edit_writes(&mut w2, target, require("dwell:air"));
            let mut got: Vec<[i32; 3]> = gone.iter().map(|(c, _)| *c).collect();
            got.sort_unstable();
            let mut want = cells.to_vec();
            want.sort_unstable();
            assert_eq!(got, want, "breaking {target:?}");
        }
        // A cell cleared by anything else leaves the rest lone: all cleared.
        w.set_voxel(cells[3][0], cells[3][1], cells[3][2], require("dwell:air"));
        assert_eq!(lone_parts(&w, &[cells[3]]).len(), 3);
    }
);

dwell_world::both_faces!(
    fn using_a_cannon_writes_nothing_and_reports_the_use(f: FaceFrame) {
        let (mut w, cells) = cannon_world(f);
        // The eye beside it, level with it, looking at its +x side.
        let eye = f.point([2.0, m_of_row(32), 1.5]);
        let request = BlockEditRequest {
            action: BlockEditAction::Place,
            cell: cells[2],
            face: f.cell_face(0) as u8,
            material: require("dwell:dirt"),
            rows: 1,
            down: false,
            point: None,
        };
        let out = check_block_edit(&mut w, &request, eye, &[], DEFAULT_REACH);
        assert_eq!(out.check, EditCheck::Ok);
        assert!(out.used && !out.toggled, "a Use of its `use`");
        assert_eq!(out.cell, cells[2]);
        assert_eq!(
            out.material,
            w.get_voxel(cells[2][0], cells[2][1], cells[2][2]),
            "unchanged"
        );
    }
);
