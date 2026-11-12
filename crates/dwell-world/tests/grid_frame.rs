//! Targeting through the grid frame (MOON_WORLD.md §2, M-3): on the home grid's frame
//! `raycast_block_in` is `raycast_block` exactly, on both faces; on a moved, turned grid a world
//! ray finds the cell the same ray finds in the grid's own space, at the same distance.

use dwell_blocks::require;
use dwell_world::edit::{raycast_block, raycast_block_in};
use dwell_world::*;

fn empty() -> VoxelWorld {
    VoxelWorld::new(empty_generator(), None)
}

/// A face-local slab of stone (rows 0..2 over the face's midplane, x and z in −8..8) with a pillar,
/// placed on face `f`.
fn slab(f: FaceFrame) -> VoxelWorld {
    let mut w = empty();
    let stone = require("dwell:stone");
    for x in -8..8 {
        for z in -8..8 {
            for row in 0..2 {
                let [cx, cy, cz] = f.cell([x, MIDPLANE_Y + row, z]);
                w.set_voxel(cx, cy, cz, f.state(stone));
            }
        }
    }
    for row in 2..6 {
        let [cx, cy, cz] = f.cell([3, MIDPLANE_Y + row, -2]);
        w.set_voxel(cx, cy, cz, stone);
    }
    w
}

fn rays() -> Vec<([f64; 3], [f32; 3])> {
    let mut out = Vec::new();
    for i in 0..60 {
        let a = f64::from(i) * 0.37;
        let eye = [a.sin() * 5.0, MIDPLANE_M + 4.0 + a.cos(), a.cos() * 4.0];
        let d = [
            (a * 1.7).cos(),
            -0.6 - 0.3 * (a * 0.9).sin(),
            (a * 2.3).sin(),
        ];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        out.push((eye, d.map(|c| (c / n) as f32)));
    }
    out
}

both_faces!(
    fn on_the_home_grid_the_ray_is_the_worlds(f: FaceFrame) {
        let mut w = slab(f);
        let mut hits = 0;
        for (eye, dir) in rays() {
            let (eye, dir) = (f.point(eye), f.vector(dir.map(f64::from)).map(|c| c as f32));
            let want = raycast_block(&mut w, eye, dir, 20.0);
            let got = raycast_block_in(&mut w, &GridFrame::HOME, eye, dir, 20.0);
            assert_eq!(got, want);
            hits += usize::from(want.is_some());
        }
        assert!(hits > 30, "{hits}");
    }
);

both_faces!(
    fn on_a_turned_grid_a_world_ray_finds_the_grids_cell(f: FaceFrame) {
        let mut w = slab(f);
        let q = quat_axis_angle([0.6, 0.0, 0.8], 2.1);
        let grid = GridFrame::at_pose([250_000.0, -3_000.0, 41_000.5], q);
        let (mut exact, mut total) = (0, 0);
        for (eye, dir) in rays() {
            let (eye, dir) = (f.point(eye), f.vector(dir.map(f64::from)).map(|c| c as f32));
            let want = raycast_block(&mut w, eye, dir, 20.0);
            // The same ray in the world: its points through the grid's frame.
            let world_eye = grid.to_world(eye);
            let world_dir = grid.vector_to_world(dir.map(f64::from)).map(|c| c as f32);
            let got = raycast_block_in(&mut w, &grid, world_eye, world_dir, 20.0);
            assert_eq!(got.is_some(), want.is_some());
            if let (Some(a), Some(b)) = (got, want) {
                // The direction is f32: a ray grazing a cell's edge can enter its neighbour, at
                // the same distance.
                let apart: i32 = (0..3).map(|i| (a.cell[i] - b.cell[i]).abs()).sum();
                assert!(apart <= 1, "{a:?} {b:?}");
                exact += usize::from(apart == 0 && a.face == b.face);
                total += 1;
                assert!((a.distance - b.distance).abs() < 1e-3, "{a:?} {b:?}");
                // The hit's cell is the grid cell of the world point the ray reaches.
                let t = f64::from(a.distance) + 1e-4;
                let p: [f64; 3] =
                    std::array::from_fn(|i| world_eye[i] + f64::from(world_dir[i]) * t);
                let c = grid.cell_of(p);
                let d: i32 = (0..3).map(|i| (c[i] - a.cell[i]).abs()).sum();
                assert!(d <= 1, "{c:?} {:?}", a.cell);
            }
        }
        assert!(total > 30 && exact * 10 >= total * 9, "{exact}/{total}");
    }
);
