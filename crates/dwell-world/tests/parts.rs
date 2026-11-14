//! Multi-cell blocks kept whole (BUGS.md #171; owner, 2026-11-14: "the torches sometimes are placed
//! by me or the generator without their tops or bottoms, it should be impossible to have one
//! without the other"): `dwell_world::parts`, one rule over the block data's `extends` / `partOf`.
//! Placing (both cells free, else refused) and breaking either cell (both go) are in `world.rs`
//! (`a_torch_is_one_block_over_two_cells`).

use dwell_blocks::{StateId, require};
use dwell_protocol::{RegionEdit, RegionEditOp};
use dwell_world::edit::above;
use dwell_world::parts::{lone_parts, repair_chunk};
use dwell_world::region::{RegionCheck, check_region_edit};
use dwell_world::*;

fn s(name: &str) -> StateId {
    require(name)
}

/// An empty world with stone at `ground` and a whole torch standing on it: (world, stick, flame).
fn torch_on(f: FaceFrame, ground: [i32; 3]) -> (VoxelWorld, [i32; 3], [i32; 3]) {
    let mut w = VoxelWorld::new(empty_generator(), None);
    let g = f.cell(ground);
    w.set_voxel(g[0], g[1], g[2], s("dwell:stone"));
    let stick = f.cell([ground[0], ground[1] + 1, ground[2]]);
    let flame = above(stick);
    w.set_voxel(stick[0], stick[1], stick[2], s("dwell:torch"));
    w.set_voxel(flame[0], flame[1], flame[2], s("dwell:torch_upper"));
    (w, stick, flame)
}

dwell_world::both_faces!(
    /// On face `f`: a write that takes or replaces either cell of a torch, by any path (a blast's
    /// air, water flowing in, a falling piece's block), leaves the other cell lone, and
    /// `lone_parts` over the written cells (the server's tick changes) clears it; a whole torch
    /// beside the write is left alone.
    fn a_write_over_either_half_clears_the_other(f: FaceFrame) {
        let air = s("dwell:air");
        for (taken, by) in [
            (0, air),
            (1, air),
            (0, s("dwell:water")),
            (1, s("dwell:stone")),
        ] {
            let (mut w, stick, flame) = torch_on(f, [2, 30, 2]);
            // A whole torch next to it, untouched.
            let (other_stick, other_flame) = (f.cell([3, 31, 2]), f.cell([3, 32, 2]));
            w.set_voxel(
                other_stick[0],
                other_stick[1],
                other_stick[2],
                s("dwell:torch"),
            );
            w.set_voxel(
                other_flame[0],
                other_flame[1],
                other_flame[2],
                s("dwell:torch_upper"),
            );
            let (cell, other) = if taken == 0 {
                (stick, flame)
            } else {
                (flame, stick)
            };
            w.set_voxel(cell[0], cell[1], cell[2], by);
            let lone = lone_parts(&w, &[cell, other_stick]);
            assert_eq!(lone, vec![(other, air)], "took cell {taken} with {by}");
        }
        // A whole torch has no lone cell.
        let (w, stick, flame) = torch_on(f, [2, 30, 2]);
        assert!(lone_parts(&w, &[stick, flame]).is_empty());
    }
);

dwell_world::both_faces!(
    /// On face `f`: a fill over a torch's stick (a 1-cell box) also clears its flame above the box,
    /// and a fill of torches puts none (each would stand without its flame): the region edit's
    /// changes, which the client predicts and the server applies alike.
    fn a_fill_over_the_lower_half_clears_the_upper(f: FaceFrame) {
        let (stone, air, torch) = (s("dwell:stone"), s("dwell:air"), s("dwell:torch"));
        let (mut w, stick, flame) = torch_on(f, [2, 30, 2]);
        let eye = f.point([2.5, m_of_row(33), 0.5]);
        let fill = |material| RegionEdit {
            op: RegionEditOp::Fill,
            min: stick,
            size: [1, 1, 1],
            material,
            from: 0,
            cells: Vec::new(),
        };
        let out = check_region_edit(&mut w, &fill(stone), eye, &[], 16.0);
        assert_eq!(out.check, RegionCheck::Ok);
        let mut got = out.changes.clone();
        got.sort_unstable();
        let mut want = vec![(stick, stone), (flame, air)];
        want.sort_unstable();
        assert_eq!(got, want);
        // A fill of torches into empty cells: none (no flame comes with a fill).
        let mut w = VoxelWorld::new(empty_generator(), None);
        let out = check_region_edit(&mut w, &fill(torch), eye, &[], 16.0);
        assert!(out.changes.is_empty(), "{:?}", out.changes);
    }
);

dwell_world::both_faces!(
    /// On face `f`: a chunk read from a world file with a lone flame or a lone stick in it (a world
    /// saved before the fix) loses that cell; a whole torch in it stays, and a cell whose partner
    /// lies in a chunk not held is left alone.
    fn a_saved_lone_half_is_removed_on_load(f: FaceFrame) {
        let (torch, upper, air) = (s("dwell:torch"), s("dwell:torch_upper"), s("dwell:air"));
        // Rows 4..6 of face `f`'s chunk at row 0..32: all inside one chunk.
        let cell = |x, y| f.cell([x, y, 3]);
        let coord = chunk_of(cell(0, 4)[0], cell(0, 4)[1], cell(0, 4)[2]);
        let mut chunk = Chunk::default();
        let set = |chunk: &mut Chunk, c: [i32; 3], m| {
            chunk.set_at(
                local_index(
                    c[0] - coord.x * CHUNK_SIZE,
                    c[1] - coord.y * CHUNK_SIZE,
                    c[2] - coord.z * CHUNK_SIZE,
                ),
                m,
            );
        };
        let get = |chunk: &Chunk, c: [i32; 3]| {
            chunk.get(
                c[0] - coord.x * CHUNK_SIZE,
                c[1] - coord.y * CHUNK_SIZE,
                c[2] - coord.z * CHUNK_SIZE,
            )
        };
        // Whole at x 1; a lone flame at x 2; a lone stick at x 3.
        set(&mut chunk, cell(1, 4), torch);
        set(&mut chunk, cell(1, 5), upper);
        set(&mut chunk, cell(2, 5), upper);
        set(&mut chunk, cell(3, 4), torch);
        // A stick on the chunk's last row toward its sky: its flame's chunk not held.
        let edge_row = if f.sign() > 0 { 31 } else { 0 };
        let edge = [4, coord.y * CHUNK_SIZE + edge_row, 3];
        set(&mut chunk, edge, torch);
        assert_ne!(
            chunk_of(above(edge)[0], above(edge)[1], above(edge)[2]),
            coord
        );
        assert_eq!(repair_chunk(&mut chunk, coord, |_| None), 2);
        assert_eq!(
            (get(&chunk, cell(1, 4)), get(&chunk, cell(1, 5))),
            (torch, upper)
        );
        assert_eq!(get(&chunk, cell(2, 5)), air);
        assert_eq!(get(&chunk, cell(3, 4)), air);
        assert_eq!(get(&chunk, edge), torch, "its partner's chunk not held");
        // With that chunk held and empty there, the stick goes too.
        assert_eq!(repair_chunk(&mut chunk, coord, |_| Some(air)), 1);
        assert_eq!(get(&chunk, edge), air);
    }
);

/// A structure's torches are stamped whole or not at all (`Placed::cells_whole`): the shrine's
/// torches with every cell free, and none of a torch whose stick's or flame's cell is taken.
// face-a-only: a placement is face-local (the generator mirrors face B's chunks; the shrines'
// test `every_shrine_torch_is_generated_whole` runs on both faces).
#[test]
fn a_structure_stamps_a_torch_whole_or_not_at_all() {
    let (torch, upper) = (s("dwell:torch"), s("dwell:torch_upper"));
    let placed = structure::load("dwell:shrine")
        .expect("the shrine")
        .place([0, 0, 0], 0);
    let count = |cells: &[([i32; 3], StateId)], m| cells.iter().filter(|(_, s)| *s == m).count();
    let all: Vec<_> = placed.cells().collect();
    let free = placed.cells_whole(|_| true);
    assert_eq!(free, all, "every cell free: all of it");
    let torches = count(&all, torch);
    assert!(torches > 0 && count(&all, upper) == torches);
    // One torch's stick cell taken (the ground there): its flame is not stamped either.
    let stick = all.iter().find(|(_, s)| *s == torch).expect("a torch").0;
    let kept = placed.cells_whole(|p| p != stick);
    assert_eq!(count(&kept, torch), torches - 1);
    assert_eq!(count(&kept, upper), torches - 1);
    assert!(
        !kept
            .iter()
            .any(|(p, _)| *p == [stick[0], stick[1] + 1, stick[2]])
    );
    // Another's flame cell taken: its stick is not stamped either.
    let flame = all
        .iter()
        .filter(|(_, s)| *s == upper)
        .nth(1)
        .expect("a flame")
        .0;
    let kept = placed.cells_whole(|p| p != flame);
    assert_eq!(count(&kept, torch), torches - 1);
    assert!(
        !kept
            .iter()
            .any(|(p, _)| *p == [flame[0], flame[1] - 1, flame[2]])
    );
    // Everything else is stamped as before.
    assert_eq!(kept.len(), all.len() - 2);
}
