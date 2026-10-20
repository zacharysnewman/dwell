//! Shore water against the land (docs/WATER_RENDERING.md §9):
//! `cargo run --release -p dwell-mesh --example shore_water -- [seeds…]` scans the coast near each
//! seed's spawn on face A and compares the water's drawn surface with the land's shaped surface in
//! the sea's top row. Prints per seed: the top-row land cells, how many are flooded and dry, the dry
//! ones whose surface lies under the drawn water and that touch drawn water (where the water stands
//! above the land beside it), and the shore cells where the water meets the slope mid-way.

use dwell_blocks::FLUIDS;
use dwell_world::Face;
use dwell_world::{GENERATOR_TERRAIN, SEA_LEVEL, VoxelWorld};
use dwell_worldgen::{BifacialTerrain, air_test_for, generator_for, spawn_point_for};

const R: i32 = 64;

fn main() {
    let mut seeds: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if seeds.is_empty() {
        seeds = vec![7, 1, 42, 1234, 99];
    }
    let water = dwell_blocks::require("dwell:water");
    let drop = FLUIDS[0].surface_drop;
    let row = SEA_LEVEL - 1;
    let w_top = f64::from(row) + 1.0 - drop;
    println!("sea row y={row}, water drawn at {w_top}");
    println!(
        "seed row_cells flooded dry dry_under_water dry_under_water_touching_water piled_max_m mid_slope flooded_top_above_water"
    );
    for seed in seeds {
        let mut w = VoxelWorld::new(
            generator_for(GENERATOR_TERRAIN, seed),
            Some(air_test_for(GENERATOR_TERRAIN, seed)),
        );
        let s = spawn_point_for(GENERATOR_TERRAIN, seed);
        let terrain = BifacialTerrain::new(seed);
        let local = terrain.local(Face::A);
        // The coast along 8 rays from the spawn: the first column whose ground is under the sea.
        let mut coasts = Vec::new();
        for k in 0..8 {
            let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
            for d in (0..40_000).step_by(8) {
                let x = s[0] as i32 + (a.cos() * f64::from(d)) as i32;
                let z = s[2] as i32 + (a.sin() * f64::from(d)) as i32;
                if local.ground_y(x, z).is_some_and(|g| g < row) {
                    coasts.push((x, z));
                    break;
                }
            }
        }
        if let Ok(z) = std::env::var("SHORE_ROW") {
            // The ground's top along one row of x, for finding the waterline by eye.
            let z: i32 = z.parse().expect("z");
            let x0: i32 = std::env::var("SHORE_X")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(s[0] as i32);
            let tops: Vec<String> = (x0..x0 + 40_000)
                .step_by(1)
                .skip_while(|&x| local.ground_y(x, z).is_some_and(|g| g > row + 3))
                .take(40)
                .map(|x| format!("{x}:{}", local.ground_y(x, z).unwrap_or(0)))
                .collect();
            println!("  row z={z}: {}", tops.join(" "));
        }
        if let Ok(at) = std::env::var("SHORE_AT") {
            let v: Vec<i32> = at.split(',').filter_map(|s| s.parse().ok()).collect();
            coasts = vec![(v[0], v[1])];
        }
        let (mut cells, mut flooded, mut dry, mut under, mut touching, mut mid, mut over) =
            (0, 0, 0, 0, 0, 0, 0);
        let mut piled: f64 = 0.0;
        for &(sx, sz) in &coasts {
            for z in sz - R..sz + R {
                for x in sx - R..sx + R {
                    let m = w.get_voxel(x, row, z);
                    if m == 0 || m == water || !dwell_blocks::adaptive::fills(m) {
                        continue;
                    }
                    let above = w.get_voxel(x, row + 1, z);
                    if dwell_blocks::adaptive::fills(above) {
                        continue; // not a top block
                    }
                    cells += 1;
                    let shape = w.shape_at(x, row, z);
                    let lo = f64::from(row) + f64::from(*shape.corners.iter().min().unwrap()) * 0.5;
                    let hi = f64::from(row) + f64::from(shape.max_y);
                    if w.flooded_at(x, row, z) {
                        flooded += 1;
                        if lo < w_top && w_top < hi {
                            mid += 1;
                        }
                        if hi <= w_top {
                            over += 1;
                        }
                    } else {
                        dry += 1;
                        if hi < w_top {
                            under += 1;
                            // Drawn water beside it: a water cell or a flooded bank in its row.
                            let wet = [[1, 0], [-1, 0], [0, 1], [0, -1]].iter().any(|&[dx, dz]| {
                                let n = w.get_voxel(x + dx, row, z + dz);
                                n == water || (n != 0 && w.flooded_at(x + dx, row, z + dz))
                            });
                            if wet {
                                touching += 1;
                                if std::env::var("SHORE_SPOTS").is_ok()
                                    && (x - sx).abs() + (z - sz).abs() < 24
                                {
                                    println!("  piled spot {x} {row} {z} (land {hi})");
                                }
                                piled = piled.max(w_top - hi);
                            }
                        }
                    }
                }
            }
        }
        // The water's side faces in the shore chunks' meshes that stand above its drawn surface.
        let (mut sides, mut above_top) = (0usize, 0usize);
        for &(sx, sz) in &coasts {
            for dz in -2..2 {
                for dx in -2..2 {
                    let c = dwell_world::chunk_of(sx + dx * 32, row, sz + dz * 32);
                    let padded = w.padded_block(c, 1);
                    let m =
                        dwell_mesh::mesh_chunk_at(&padded, false, [c.x * 32, c.y * 32, c.z * 32])
                            .expect("mesh")
                            .transparent;
                    for q in 0..m.positions.len() / 12 {
                        let v = q * 4;
                        if m.normals[v * 3 + 1] != 0.0 {
                            continue;
                        }
                        sides += 1;
                        let top = (0..4)
                            .map(|k| f64::from(m.positions[(v + k) * 3 + 1]) + f64::from(c.y * 32))
                            .fold(f64::MIN, f64::max);
                        if top > w_top + 1e-3 {
                            above_top += 1;
                        }
                    }
                }
            }
        }
        println!("  water side quads {sides}, standing above the drawn surface {above_top}");
        println!("{seed} {cells} {flooded} {dry} {under} {touching} {piled:.3} {mid} {over}");
    }
}
