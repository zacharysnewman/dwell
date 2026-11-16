//! A measurement, not a check (the owner's 60 fps pass, 2026-11-15: "Do we skip occluded
//! chunks?"): how much of what the near pass draws lies in chunks the terrain hides wholly from
//! the eye, at a few spots of the default world. Run it by hand:
//! `cargo test --release -p dwell-mesh --test occlusion_probe -- --ignored --nocapture`.

use dwell_world::{CELL_H, CHUNK_SIZE, ChunkCoord, FaceFrame, SKY_APRON, VoxelWorld};

#[test]
#[ignore = "a measurement: run by hand"]
fn how_much_the_terrain_hides() {
    // face-a-only: a measurement of face A's default world.
    use dwell_world::GENERATOR_TERRAIN;
    use dwell_worldgen::{air_test_for, generator_for};
    let seed = 7;
    let air = air_test_for(GENERATOR_TERRAIN, seed);
    let mut src = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), Some(air.clone()));
    let terrain = dwell_worldgen::BifacialTerrain::new(seed);
    let t = terrain.local(dwell_world::Face::A);
    let n = CHUNK_SIZE;
    let sp = t.spawn_search_point();
    let ground = |x: f64, z: f64| t.surface_y_at(x.floor() as i32, z.floor() as i32);
    let radius = 160.0;
    for (name, dx, dz) in [
        ("spawn", 0.0, 0.0),
        ("east 300", 300.0, 0.0),
        ("north 600", 0.0, -600.0),
        ("sw 900", -900.0, 900.0),
    ] {
        let (ex, ez) = (sp[0] + dx, sp[2] + dz);
        let Some(g) = ground(ex, ez) else { continue };
        let eye = [ex, g + 1.6, ez];
        // A ray from the eye to `p` dips under the ground (by 0.3 m) before its last 1.5 m.
        let hidden = |p: [f64; 3]| {
            let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let steps = (len / 0.75) as i32;
            (1..steps).any(|s| {
                let k = f64::from(s) * 0.75;
                if len - k < 1.5 {
                    return false;
                }
                let q = [
                    eye[0] + d[0] * k / len,
                    eye[1] + d[1] * k / len,
                    eye[2] + d[2] * k / len,
                ];
                ground(q[0], q[2]).is_some_and(|h| q[1] < h - 0.3)
            })
        };
        let (mut all, mut gone, mut chunks, mut gone_chunks) = (0usize, 0usize, 0, 0);
        let r = (radius / f64::from(n)).ceil() as i32;
        let (cx0, cz0) = ((ex as i32).div_euclid(n), (ez as i32).div_euclid(n));
        for cz in cz0 - r..=cz0 + r {
            for cx in cx0 - r..=cx0 + r {
                // The chunks the column's ground crosses, one either side.
                let mid = ground(f64::from(cx * n + n / 2), f64::from(cz * n + n / 2)).unwrap_or(g);
                let cy0 = ((mid / CELL_H) as i32).div_euclid(n);
                for cy in cy0 - 1..=cy0 + 1 {
                    let c = ChunkCoord::new(cx, cy, cz);
                    if air(c) {
                        continue;
                    }
                    let block = src.padded_box(c, SKY_APRON);
                    let origin = [cx * n, cy * n, cz * n];
                    let m = dwell_mesh::mesh_chunk_lit(&block, &[], false, origin).expect("block");
                    let tris = m.opaque.indices.len() / 3 + m.cutout.indices.len() / 3;
                    if tris == 0 {
                        continue;
                    }
                    let o = [
                        f64::from(origin[0]),
                        f64::from(origin[1]) * CELL_H,
                        f64::from(origin[2]),
                    ];
                    let verts: Vec<[f64; 3]> = m
                        .opaque
                        .positions
                        .chunks_exact(3)
                        .chain(m.cutout.positions.chunks_exact(3))
                        .step_by(7)
                        .map(|p| {
                            [
                                o[0] + f64::from(p[0]),
                                o[1] + f64::from(p[1]),
                                o[2] + f64::from(p[2]),
                            ]
                        })
                        .collect();
                    chunks += 1;
                    all += tris;
                    if verts.iter().all(|&p| hidden(p)) {
                        gone += tris;
                        gone_chunks += 1;
                    }
                }
            }
        }
        let _ = FaceFrame::A;
        println!(
            "{name}: {gone_chunks} of {chunks} chunks hidden, {gone} of {all} triangles ({:.0} %)",
            100.0 * gone as f64 / all.max(1) as f64
        );
    }
}
