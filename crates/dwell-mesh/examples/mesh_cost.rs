//! The terrain's meshing cost around the spawn (steep terrain, 2026-10-19):
//! `cargo run --release -p dwell-mesh --example mesh_cost -- [seeds…]` generates the procedural
//! terrain's chunks in a 9 × 9 chunk square around each seed's spawn on each face, every row that
//! holds ground, and meshes them as the client does; then the LOD sections of levels 1–4 in a 5 × 5
//! square around the spawn, three rows each around the ground, with their column surfaces. Prints
//! per face the chunks meshed, their vertices, triangles and mesh bytes, and the sections' the same,
//! with the meshing time.

use std::time::Instant;

use dwell_mesh::{MeshSectionOptions, mesh_chunk_at, mesh_section, section_bytes};
use dwell_world::lod::{self, LodCoord, LodSurfaces, section_size};
use dwell_world::{CHUNK_SIZE, ChunkCoord, Face, GENERATOR_TERRAIN, VoxelWorld, face_local_chunk};
use dwell_worldgen::{BifacialTerrain, air_test_for, generator_for};

const RADIUS: i32 = 4;

fn surface_floats(s: &LodSurfaces) -> Vec<f32> {
    s.iter()
        .flat_map(|c| {
            [
                c.height,
                f32::from(c.material),
                f32::from(u8::from(c.valid) | (u8::from(c.wet) << 1)),
                c.water,
            ]
        })
        .collect()
}

#[derive(Default)]
struct Cost {
    count: u64,
    vertices: u64,
    triangles: u64,
    bytes: u64,
    ms: f64,
}

/// The world chunk holding face-local chunk row `row` of column (cx, cz) on `face`.
fn world_chunk(face: Face, cx: i32, row: i32, cz: i32) -> ChunkCoord {
    (-64..64)
        .map(|y| ChunkCoord::new(cx, y, cz))
        .find(|c| {
            face_local_chunk(*c).y == row && (c.y >= 0) == (face == Face::A)
        })
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
        "face chunks chunk_vertices chunk_triangles chunk_MB chunk_ms sections section_vertices section_triangles section_MB section_ms"
    );
    for face in [Face::A, Face::B] {
        let (mut ch, mut se) = (Cost::default(), Cost::default());
        for &seed in &seeds {
            let terrain = BifacialTerrain::new(seed);
            let local = terrain.local(face);
            let sp = local.spawn_point();
            let (sx, sz) = (sp[0].floor() as i32, sp[2].floor() as i32);
            let mut world = VoxelWorld::new(
                generator_for(GENERATOR_TERRAIN, seed),
                Some(air_test_for(GENERATOR_TERRAIN, seed)),
            );
            let (scx, scz) = (sx.div_euclid(CHUNK_SIZE), sz.div_euclid(CHUNK_SIZE));
            for cz in scz - RADIUS..=scz + RADIUS {
                for cx in scx - RADIUS..=scx + RADIUS {
                    // The face-local rows holding this column's ground (and the air over it).
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
                    for row in lo.div_euclid(CHUNK_SIZE)..=hi.div_euclid(CHUNK_SIZE) {
                        let coord = world_chunk(face, cx, row, cz);
                        let padded = world.padded_block(coord, 1);
                        let t = Instant::now();
                        let m = mesh_chunk_at(
                            &padded,
                            face == Face::B,
                            [
                                coord.x * CHUNK_SIZE,
                                coord.y * CHUNK_SIZE,
                                coord.z * CHUNK_SIZE,
                            ],
                        )
                        .expect("padded");
                        ch.ms += t.elapsed().as_secs_f64() * 1000.0;
                        ch.count += 1;
                        for a in [&m.opaque, &m.transparent, &m.cutout] {
                            ch.vertices += (a.positions.len() / 3) as u64;
                            ch.triangles += (a.indices.len() / 3) as u64;
                            ch.bytes += a.byte_len() as u64;
                        }
                    }
                }
            }
            for level in 1..=4 {
                let s = section_size(level);
                // The spawn in world coordinates: face B's ground lies below the midplane.
                let y = if face == Face::A { sp[1] } else { -sp[1] } as i64;
                let at = |p: i64, o: i64| (p - o).div_euclid(s) as i32;
                let (i0, j0, k0) = (
                    at(i64::from(sx), lod::ORIGIN_X),
                    at(y, lod::ORIGIN_Y),
                    at(i64::from(sz), lod::ORIGIN_Z),
                );
                for k in k0 - 2..=k0 + 2 {
                    for i in i0 - 2..=i0 + 2 {
                        for j in j0 - 1..=j0 + 1 {
                            let c = LodCoord::new(level, i, j, k);
                            let mut cells = Vec::new();
                            let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
                            terrain.generate_lod(c, &mut cells, Some(&mut a), Some(&mut b));
                            let surf = surface_floats(if face == Face::A { &a } else { &b });
                            let t = Instant::now();
                            let m = mesh_section(
                                &cells,
                                &MeshSectionOptions {
                                    surface: Some(&surf),
                                    water_drop: 0.125,
                                    mirror: face == Face::B,
                                },
                            )
                            .expect("cells");
                            se.ms += t.elapsed().as_secs_f64() * 1000.0;
                            se.count += 1;
                            for f in std::iter::once(&m.opaque)
                                .chain([&m.water])
                                .chain(m.skirts.iter())
                            {
                                se.vertices += (f.positions.len() / 3) as u64;
                                se.triangles += (f.indices.len() / 3) as u64;
                            }
                            se.bytes += section_bytes(&m) as u64;
                        }
                    }
                }
            }
        }
        println!(
            "{face:?} {} {} {} {:.1} {:.0} {} {} {} {:.1} {:.0}",
            ch.count,
            ch.vertices,
            ch.triangles,
            ch.bytes as f64 / 1e6,
            ch.ms,
            se.count,
            se.vertices,
            se.triangles,
            se.bytes as f64 / 1e6,
            se.ms
        );
    }
}
