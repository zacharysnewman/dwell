//! The flood light's cost around the spawn (docs/DAY_NIGHT.md §10):
//! `cargo run --release -p dwell-mesh --example light_cost -- [seeds…]` generates the procedural
//! terrain's chunks in a 9 × 9 chunk square around each seed's spawn on each face (every row that
//! holds ground and the row over it), loads them into a lit world nearest first as the streamer
//! does, then meshes them with and without the light, digs a 2 × 2 m tunnel 8 m into the ground at
//! the spawn and fills it back, and roofs 16 m square over it and takes the roof away. Prints per face: chunks held, light ms per chunk loaded, the light
//! arrays held (MB), mesh ms per chunk unlit and lit, and the edits' update ms (dig, fill, roof, unroof).

use std::time::Instant;

use dwell_mesh::{mesh_chunk_at, mesh_chunk_lit};
use dwell_world::{
    CHUNK_SIZE, ChunkCoord, Face, FaceFrame, GENERATOR_TERRAIN, VoxelWorld, face_local_chunk,
};
use dwell_worldgen::{BifacialTerrain, air_test_for, generator_for};

const RADIUS: i32 = 4;

fn world_chunk(face: Face, cx: i32, row: i32, cz: i32) -> ChunkCoord {
    (-64..64)
        .map(|y| ChunkCoord::new(cx, y, cz))
        .find(|c| face_local_chunk(*c).y == row && (c.y >= 0) == (face == Face::A))
        .expect("a row inside the world")
}

fn main() {
    let mut seeds: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if seeds.is_empty() {
        seeds = vec![7, 1, 42];
    }
    println!(
        "face held load_ms_per_chunk light_MB full_arrays mesh_ms_unlit mesh_ms_lit dig_ms fill_ms roof_ms unroof_ms"
    );
    for face in [Face::A, Face::B] {
        let f = if face == Face::A {
            FaceFrame::A
        } else {
            FaceFrame::B
        };
        let (mut held, mut load_ms, mut bytes, mut full) = (0usize, 0.0, 0usize, 0usize);
        let (mut meshed, mut unlit_ms, mut lit_ms) = (0usize, 0.0, 0.0);
        let (mut dig_ms, mut fill_ms, mut roof_ms, mut unroof_ms) = (0.0, 0.0, 0.0, 0.0);
        for &seed in &seeds {
            let terrain = BifacialTerrain::new(seed);
            let local = terrain.local(face);
            let sp = local.spawn_point();
            let (sx, sz) = (sp[0].floor() as i32, sp[2].floor() as i32);
            let air = air_test_for(GENERATOR_TERRAIN, seed);
            let mut src =
                VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), Some(air.clone()));
            let (scx, scz) = (sx.div_euclid(CHUNK_SIZE), sz.div_euclid(CHUNK_SIZE));
            let mut coords = Vec::new();
            let mut ground = Vec::new();
            for cz in scz - RADIUS..=scz + RADIUS {
                for cx in scx - RADIUS..=scx + RADIUS {
                    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            if let Some(g) =
                                local.ground_y(cx * CHUNK_SIZE + x, cz * CHUNK_SIZE + z)
                            {
                                lo = lo.min(g);
                                hi = hi.max(g + 1);
                            }
                        }
                    }
                    for row in lo.div_euclid(CHUNK_SIZE)..=hi.div_euclid(CHUNK_SIZE) + 1 {
                        let c = world_chunk(face, cx, row, cz);
                        coords.push(c);
                        if row <= hi.div_euclid(CHUNK_SIZE) {
                            ground.push(c);
                        }
                    }
                }
            }
            // Nearest first, as the streamer sends them.
            let centre = world_chunk(face, scx, (sp[1] as i32).div_euclid(CHUNK_SIZE), scz);
            coords.sort_by_key(|c| {
                (c.x - centre.x).pow(2) + (c.y - centre.y).pow(2) + (c.z - centre.z).pow(2)
            });
            let mut lit = VoxelWorld::streamed();
            lit.enable_light();
            for &c in &coords {
                let chunk = if air(c) {
                    None
                } else {
                    Some(src.read(c).clone())
                };
                if let Some(chunk) = chunk {
                    lit.put(c, chunk);
                }
                let t = Instant::now();
                lit.light_load(c);
                load_ms += t.elapsed().as_secs_f64() * 1000.0;
                held += 1;
            }
            let (_, n) = lit.light().expect("lit").counts();
            full += n;
            bytes += lit.light().expect("lit").bytes();
            for &c in &ground {
                let padded = src.padded_box(c, dwell_world::SKY_APRON);
                let origin = [c.x * CHUNK_SIZE, c.y * CHUNK_SIZE, c.z * CHUNK_SIZE];
                let t = Instant::now();
                let _ = mesh_chunk_at(&padded, face == Face::B, origin).expect("padded");
                unlit_ms += t.elapsed().as_secs_f64() * 1000.0;
                let light = lit.padded_light(c);
                let t = Instant::now();
                let _ = mesh_chunk_lit(&padded, &light, face == Face::B, origin).expect("padded");
                lit_ms += t.elapsed().as_secs_f64() * 1000.0;
                meshed += 1;
            }
            // A trench into the ground at the spawn, 8 m long, 2 m wide and deep, open to the sky.
            let g = local.ground_y(sx, sz).unwrap_or(0);
            let cells: Vec<[i32; 3]> = (0..8)
                .flat_map(|x| {
                    (0..2).flat_map(move |z| (0..4).map(move |y| [sx + x, g - 3 + y, sz + z]))
                })
                .map(|p| f.cell(p))
                .collect();
            let before: Vec<u16> = cells
                .iter()
                .map(|&[x, y, z]| lit.get_voxel(x, y, z))
                .collect();
            for &[x, y, z] in &cells {
                lit.set_voxel(x, y, z, 0);
            }
            let t = Instant::now();
            lit.light_edited(&cells);
            dig_ms += t.elapsed().as_secs_f64() * 1000.0;
            for (&[x, y, z], &id) in cells.iter().zip(&before) {
                lit.set_voxel(x, y, z, id);
            }
            let t = Instant::now();
            lit.light_edited(&cells);
            fill_ms += t.elapsed().as_secs_f64() * 1000.0;
            // A 16 m square roof 4 m over the ground, then taken away.
            let roof: Vec<[i32; 3]> = (0..16 * 16)
                .map(|i| f.cell([sx - 8 + i % 16, g + 8, sz - 8 + i / 16]))
                .collect();
            let rock = dwell_blocks::require("dwell:stone[flooded=false]");
            for &[x, y, z] in &roof {
                lit.set_voxel(x, y, z, rock);
            }
            let t = Instant::now();
            lit.light_edited(&roof);
            roof_ms += t.elapsed().as_secs_f64() * 1000.0;
            for &[x, y, z] in &roof {
                lit.set_voxel(x, y, z, 0);
            }
            let t = Instant::now();
            lit.light_edited(&roof);
            unroof_ms += t.elapsed().as_secs_f64() * 1000.0;
        }
        let k = seeds.len() as f64;
        println!(
            "{face:?} {held} {:.3} {:.1} {full} {:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
            load_ms / held as f64,
            bytes as f64 / 1e6,
            unlit_ms / meshed as f64,
            lit_ms / meshed as f64,
            dig_ms / k,
            fill_ms / k,
            roof_ms / k,
            unroof_ms / k
        );
    }
}
