//! The mesh apron at the streamed view's edge (docs/BUGS.md 8): a chunk of water whose
//! horizontal neighbours aren't loaded read air there and meshed the water's side as a wall (rows
//! of dark blue rectangles across lakes and the sea). Its apron goes on with the water instead.

use dwell_blocks::require;
use dwell_world::*;

fn water_chunk() -> Chunk {
    let (water, stone) = (require("dwell:water"), require("dwell:stone"));
    let n = CHUNK_SIZE;
    let mut chunk = Chunk::default();
    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                let v = if y < 4 {
                    stone
                } else if y < 20 {
                    water
                } else {
                    M.air
                };
                chunk.set(x, y, z, v);
            }
        }
    }
    chunk
}

#[test]
fn the_apron_goes_on_with_the_water_where_no_neighbour_is_loaded() {
    // face-a-only: the apron is a copy of voxels with no up in it (the same on both faces).
    let water = require("dwell:water");
    let n = CHUNK_SIZE;
    let mut w = VoxelWorld::streamed();
    w.put(ChunkCoord::new(0, 0, 0), water_chunk());
    let apron = [1, 1, 1];
    let b = w.padded_box(ChunkCoord::new(0, 0, 0), apron);
    let e = n + 2;
    let at = |x: i32, y: i32, z: i32| b[(((z + 1) + e * (y + 1)) * e + (x + 1)) as usize];
    for (x, z) in [(-1, 5), (n, 5), (7, -1), (7, n), (-1, -1), (n, n)] {
        assert_eq!(at(x, 10, z), water, "water beside at ({x}, {z})");
        assert_eq!(at(x, 2, z), M.air, "the ground's border reads as before");
        assert_eq!(at(x, 25, z), M.air, "air over the water");
    }
    assert_eq!(at(5, n, 5), M.air, "no chunk over it: the surface is drawn");
    // A loaded neighbour is read as it is (dry land beside the water).
    let mut dry = Chunk::default();
    for i in 0..CHUNK_VOLUME {
        dry.set_at(i, require("dwell:stone"));
    }
    w.put(ChunkCoord::new(1, 0, 0), dry);
    let b = w.padded_box(ChunkCoord::new(0, 0, 0), apron);
    let at = |x: i32, y: i32, z: i32| b[(((z + 1) + e * (y + 1)) * e + (x + 1)) as usize];
    assert_eq!(at(n, 10, 5), require("dwell:stone"));
    assert_eq!(at(-1, 10, 5), water, "the missing side still goes on");
}
