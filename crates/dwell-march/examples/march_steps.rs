//! Steps per ray on generated terrain (RAY_MARCHING.md §4.3): builds the brick structure for the
//! chunks around the spawn point and LOD sections out to a few hundred kilometres, marches
//! horizon, downward and high-altitude rays with empty-space skipping on and off, and prints the
//! histograms and the memory the structure takes.
//!
//! `cargo run --release -p dwell-march --example march_steps [seed]`

use std::time::Instant;

use dwell_march::{
    BuildOptions, MarchParams, NodeBuild, Ray, Scene, StateTable, StepHistogram, build_node,
    chunk_node,
};
use dwell_world::lod::{self, LodCoord, LodKind};
use dwell_world::{Chunk, ChunkCoord, GENERATOR_TERRAIN, M, local_index};
use dwell_worldgen::{
    air_test_for, generator_for, lod_bounds_for, lod_generator_for, spawn_point_for,
};

/// Chunks out to the render radius around the spawn; LOD sections this many around the camera on
/// each level up to `TOP_LEVEL`.
const CHUNK_RADIUS: i32 = 12;
const CHUNK_ROWS: i32 = 4;
const SECTION_RADIUS: i32 = 6;
const TOP_LEVEL: i32 = 12;

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let version = GENERATOR_TERRAIN;
    let table = StateTable::registry();
    let mut scene = Scene::new(table.clone());
    let spawn = spawn_point_for(version, seed);
    let eye = [spawn[0], spawn[1] + 1.6, spawn[2]];
    println!("seed {seed}, eye at {eye:?}");

    // Levels 1..: sections.
    let started = Instant::now();
    let gen_lod = lod_generator_for(version, seed);
    let bounds = lod_bounds_for(version, seed);
    let (mut section_bytes, mut section_nodes, mut sections) = (0usize, 0usize, 0usize);
    let mut cells = Vec::new();
    for level in 1..=TOP_LEVEL {
        let size = lod::section_size(level);
        let at = |v: f64, o: i64| ((v - o as f64) / size as f64).floor() as i32;
        let (ci, ck) = (at(eye[0], lod::ORIGIN_X), at(eye[2], lod::ORIGIN_Z));
        for k in ck - SECTION_RADIUS..=ck + SECTION_RADIUS {
            for i in ci - SECTION_RADIUS..=ci + SECTION_RADIUS {
                let b = bounds(level, i, k);
                for j in lod::first_row(level)..=lod::last_row(level) {
                    let c = LodCoord::new(level, i, j, k);
                    if !lod::in_world(c) {
                        continue;
                    }
                    let n = [i, j, k];
                    match lod::kind_from_bounds(c, &b) {
                        LodKind::Empty => scene.put(level as u32, n, &NodeBuild::Air),
                        LodKind::Buried => scene.put(level as u32, n, &NodeBuild::Solid(M.stone)),
                        LodKind::Content => {
                            gen_lod(c, &mut cells);
                            sections += 1;
                            let built = scene.put_section(c, &cells);
                            if let NodeBuild::Node(_) = built {
                                section_nodes += 1;
                                section_bytes += built.gpu_bytes();
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "levels 1–{TOP_LEVEL}: {sections} generated sections, {section_nodes} nodes, {:.1} KiB per node with its column grid ({:.1} s)",
        section_bytes as f64 / section_nodes.max(1) as f64 / 1024.0,
        started.elapsed().as_secs_f64()
    );
    let mut no_chunk_columns = scene.clone();
    // Level 0: chunks.
    let started = Instant::now();
    let generate = generator_for(version, seed);
    let is_air = air_test_for(version, seed);
    let centre = dwell_world::chunk_of(eye[0] as i32, eye[1] as i32, eye[2] as i32);
    let (mut chunk_bytes, mut chunk_nodes, mut chunks) = (0usize, 0usize, 0usize);
    for cz in -CHUNK_RADIUS..=CHUNK_RADIUS {
        for cy in -CHUNK_ROWS..=CHUNK_ROWS {
            for cx in -CHUNK_RADIUS..=CHUNK_RADIUS {
                let c = ChunkCoord::new(centre.x + cx, centre.y + cy, centre.z + cz);
                chunks += 1;
                if is_air(c) {
                    scene.put(0, chunk_node(c), &NodeBuild::Air);
                    no_chunk_columns.put(0, chunk_node(c), &NodeBuild::Air);
                    continue;
                }
                let mut chunk = Chunk::default();
                generate(c, &mut chunk);
                let b = scene.put_chunk(c, &chunk.voxels()[..]);
                let v = chunk.voxels();
                let with = build_node(
                    |x, y, z| v[local_index(x, y, z)],
                    &table,
                    BuildOptions {
                        columns: false,
                        ..BuildOptions::CHUNK
                    },
                );
                no_chunk_columns.put(0, chunk_node(c), &with);
                if let NodeBuild::Node(_) = b {
                    chunk_nodes += 1;
                    chunk_bytes += b.gpu_bytes();
                }
            }
        }
    }
    println!(
        "level 0: {chunks} chunks, {chunk_nodes} nodes, {:.1} KiB per node ({:.1} s)",
        chunk_bytes as f64 / chunk_nodes.max(1) as f64 / 1024.0,
        started.elapsed().as_secs_f64()
    );

    let gpu = scene.gpu();
    println!(
        "GPU words: {:.1} MiB in all (pool {:.1}, nodes {:.1}, columns {:.1}, slots {:.2})",
        gpu.bytes() as f64 / 1048576.0,
        gpu.pool.len() as f64 * 4.0 / 1048576.0,
        gpu.nodes.len() as f64 * 4.0 / 1048576.0,
        gpu.columns.len() as f64 * 4.0 / 1048576.0,
        gpu.slots.len() as f64 * 4.0 / 1048576.0,
    );

    // 1080p, 70° vertical field of view, the desktop LOD's 4 px.
    let base = MarchParams {
        max_t: 400_000.0,
        step_cap: 1024,
        pixel_angle: 70f32.to_radians() / 1080.0,
        lod_pixels: 4.0,
        ..MarchParams::default()
    };
    let high = [eye[0], eye[1] + 3000.0, eye[2]];
    let sets: [(&str, [f64; 3], f32, f32); 3] = [
        ("horizon (pitch −6°..+1°)", eye, -6.0, 1.0),
        ("down (pitch −90°..−30°)", eye, -90.0, -30.0),
        ("high, 3 km up (pitch −30°..0°)", high, -30.0, 0.0),
    ];
    const RAYS: usize = 20_000;
    println!(
        "masks: the hierarchy's masks only; no-chunk: skipping with column grids on sections only; on: all skipping"
    );
    println!(
        "\n{:<34} {:>9} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>6}",
        "rays", "skipping", "median", "p95", "p99", "max", "mean", "cap%", "hit%", ">128%"
    );
    for (name, origin, pitch_lo, pitch_hi) in sets {
        for (config, scene, p) in [
            ("masks", &scene, base.without_skipping()),
            ("no-chunk", &no_chunk_columns, base),
            ("on", &scene, base),
        ] {
            let mut h = StepHistogram::default();
            let mut s: u64 = 0x2545_F491_4F6C_DD1D;
            let mut rnd = || {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 40) as f32 / (1u64 << 24) as f32
            };
            for _ in 0..RAYS {
                let yaw = rnd() * std::f32::consts::TAU;
                let pitch = (pitch_lo + (pitch_hi - pitch_lo) * rnd()).to_radians();
                let dir = [
                    yaw.cos() * pitch.cos(),
                    pitch.sin(),
                    yaw.sin() * pitch.cos(),
                ];
                h.add(&scene.march(&Ray::from_world(origin, dir), &p));
            }
            let r = h.summary();
            println!(
                "{:<34} {:>9} {:>6} {:>6} {:>6} {:>6} {:>7.1} {:>6.2} {:>6.1} {:>6.1}",
                name,
                config,
                r.median,
                r.p95,
                r.p99,
                r.max,
                r.mean,
                100.0 * r.capped_share,
                100.0 * r.hit_share,
                100.0 * h.share_over(128)
            );
        }
    }
}
