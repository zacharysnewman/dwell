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

/// The old per-voxel continuation (before BUGS.md #72 round 4), the reference the region-wise one
/// must equal: every voxel of the block looks up whether its neighbour is loaded.
fn continued_per_voxel(w: &VoxelWorld, c: ChunkCoord, apron: [i32; 3], out: &mut [u16]) {
    let n = CHUNK_SIZE;
    let [ax, ay, az] = apron;
    let (ex, ey, ez) = (n + 2 * ax, n + 2 * ay, n + 2 * az);
    let at = |dx: i32, dy: i32, dz: i32| ChunkCoord::new(c.x + dx, c.y + dy, c.z + dz);
    let missing =
        |dx: i32, dy: i32, dz: i32| (dx, dz) != (0, 0) && w.find(at(dx, dy, dz)).is_none();
    let side = |v: i32| v.div_euclid(n).clamp(-1, 1);
    for py in 0..ey {
        let y = py - ay;
        let dy = side(y);
        let Some(own) = w.find(at(0, dy, 0)) else {
            continue;
        };
        for pz in 0..ez {
            for px in 0..ex {
                let (x, z) = (px - ax, pz - az);
                if !missing(side(x), dy, side(z)) {
                    continue;
                }
                let v = own.voxels()[local_index(x.clamp(0, n - 1), y - dy * n, z.clamp(0, n - 1))];
                let info = dwell_blocks::info(v);
                if info.liquid || info.flooded {
                    out[((pz + ez * py) * ex + px) as usize] = v;
                }
            }
        }
    }
}

/// A streamed world of water chunks: a 3 × 3 × 3 block around the origin with `gone` left out.
fn lake(gone: &[ChunkCoord]) -> VoxelWorld {
    let mut w = VoxelWorld::streamed();
    for y in -1..=1 {
        for z in -1..=1 {
            for x in -1..=1 {
                let c = ChunkCoord::new(x, y, z);
                if !gone.contains(&c) {
                    w.put(c, water_chunk());
                }
            }
        }
    }
    w
}

#[test]
fn the_sky_volume_continues_the_water_into_missing_neighbours_as_the_per_voxel_rule_did() {
    // face-a-only: the apron is a copy of voxels with no up in it (the same on both faces).
    let c = ChunkCoord::new(0, 0, 0);
    let n = CHUNK_SIZE;
    let [ax, ay, az] = SKY_APRON;
    let (ex, ez) = (n + 2 * ax, n + 2 * az);
    for gone in [
        vec![],
        vec![ChunkCoord::new(1, 0, 0)],
        vec![ChunkCoord::new(-1, 1, -1), ChunkCoord::new(0, -1, 1)],
        vec![
            ChunkCoord::new(1, 0, 1),
            ChunkCoord::new(-1, 0, 0),
            ChunkCoord::new(0, 0, -1),
        ],
        vec![ChunkCoord::new(0, 1, 0), ChunkCoord::new(1, 1, 0)],
    ] {
        let mut w = lake(&gone);
        let got = w.padded_box(c, SKY_APRON);
        // The reference: the full lake's block with the missing chunks' cells as air, then the
        // per-voxel continuation.
        let mut want = lake(&[]).padded_box(c, SKY_APRON);
        for (i, v) in want.iter_mut().enumerate() {
            let i = i as i32;
            let (px, pz, py) = (i % ex, (i / ex) % ez, i / (ex * ez));
            let k = ChunkCoord::new(
                (px - ax).div_euclid(n),
                (py - ay).div_euclid(n),
                (pz - az).div_euclid(n),
            );
            if gone.contains(&k) {
                *v = M.air;
            }
        }
        continued_per_voxel(&w, c, SKY_APRON, &mut want);
        assert!(got == want, "missing {gone:?}: the blocks differ");
    }
}

#[test]
fn a_chunk_on_the_streamed_edge_copies_its_sky_volume_about_as_fast_as_one_inside() {
    // face-a-only: a cost, no up in it.
    // BUGS.md #72: the per-voxel continuation looked up a neighbour for each of the sky volume's
    // 124k voxels whenever one was missing, at every chunk on the view's edge.
    let c = ChunkCoord::new(0, 0, 0);
    let mut inside = lake(&[]);
    let mut edge = lake(&[
        ChunkCoord::new(1, 0, 0),
        ChunkCoord::new(1, 0, 1),
        ChunkCoord::new(0, 1, 1),
    ]);
    let time = |w: &mut VoxelWorld| {
        let mut best = f64::MAX;
        for _ in 0..7 {
            let t = std::time::Instant::now();
            for _ in 0..4 {
                std::hint::black_box(w.padded_box(c, SKY_APRON));
            }
            best = best.min(t.elapsed().as_secs_f64());
        }
        best
    };
    let (a, b) = (time(&mut inside), time(&mut edge));
    assert!(
        b < 2.5 * a,
        "edge {:.2} ms, inside {:.2} ms",
        b * 250.0,
        a * 250.0
    );
}
