//! World-scale bench (docs/proposals/WORLD_SCALE.md): one face's world map built stage by stage
//! (the steps of `hydrology::from_heights`, timed), the biome field, the spawn search, chunk
//! generation round the spawn and the big map's widest-level tile raster, at the build's
//! `worldRadius` / `islandRadius` (edit `shared/protocol/constants.json` to scale them).
//!
//! `cargo run --release -p dwell-worldgen --example world_scale -- [seed]`; run under
//! `/usr/bin/time -l` for the peak memory. `SKIP_STAGES=1` skips the stage-by-stage build (a copy
//! of `from_heights`' steps, kept here for timing only: the real build follows it), `SKIP_SPAWN=1`
//! the spawn search (the chunks are then at the origin), and `CHUNK_LOOP=<s>` generates chunks for
//! that long (to attach a profiler). Scaling the radii past 65 km also needs `lod.maxLevel` raised
//! (or `lod.rs`'s root assertion relaxed for the bench): the LOD octree's root holds 131 km.

use std::sync::Arc;
use std::time::Instant;

use dwell_world::{Chunk, ChunkCoord, Face, WORLD_RADIUS, chunk_of_pos};
use dwell_worldgen::hydrology::{self, Rain, erosion, lakes, rain, rivers, uplift};
use dwell_worldgen::worldmap::{WorldMap, codec};
use dwell_worldgen::{TerrainGenerator, bifacial::face_seed};

fn main() {
    let world_seed: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let seed = face_seed(world_seed, Face::A);
    let p = hydrology::default_profile();
    println!(
        "worldRadius {} m, islandRadius {} m, cell {} m, profile {}",
        WORLD_RADIUS,
        dwell_protocol::world::ISLAND_RADIUS,
        p.cell,
        p.name
    );
    let t_all = Instant::now();
    let mut t = Instant::now();
    let mut lap = |name: &str| {
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!("  {name:<22} {ms:>10.0} ms");
        t = Instant::now();
    };

    let stages = std::env::var("SKIP_STAGES").is_err();
    if stages {
        let (grid, x0, h0) = hydrology::sample_seed(seed, p.cell);
        let cell = f64::from(p.cell);
        let len = grid.len();
        println!("  grid n = {} ({} cells)", grid.n, len);
        lap("base height field");

        // `from_heights`, step by step.
        let mut sea = vec![false; len];
        let mut todo = Vec::new();
        for (i, s) in sea.iter_mut().enumerate() {
            if grid.on_border(i) {
                *s = true;
                todo.push(i);
            }
        }
        while let Some(c) = todo.pop() {
            for (j, _) in grid.neighbours(c) {
                if !sea[j] && h0[j] <= 0.0 {
                    sea[j] = true;
                    todo.push(j);
                }
            }
        }
        let mut h: Vec<f64> = (0..len)
            .map(|i| {
                if sea[i] {
                    h0[i].clamp(-1e4, 0.0)
                } else {
                    h0[i]
                }
            })
            .collect();
        lap("open sea");
        let tect = p.tectonics.as_ref().expect("default profile has tectonics");
        let coast = uplift::coast_distance(grid, cell, &sea);
        lap("coast distance");
        let u = uplift::generate(grid, cell, x0, &sea, &coast, seed, tect);
        for (i, v) in h.iter_mut().enumerate() {
            if !sea[i] {
                *v = tect.keep_relief * *v + tect.base_height * u.strength[i];
            }
        }
        let up: Vec<f64> = u.strength.iter().map(|&s| tect.rate * s).collect();
        lap("uplift layer");
        let erod: Vec<f64> = match &p.rock_bands {
            None => vec![p.erodibility; len],
            Some(r) => uplift::rock_factor(grid, cell, x0, &sea, seed, r)
                .iter()
                .map(|&f| p.erodibility * f)
                .collect(),
        };
        let rainv: Vec<f64> = match &p.rain {
            Rain::Uniform => vec![1.0; len],
            Rain::Orographic(o) => rain::orographic(grid, cell, &h, &sea, seed, o),
        };
        lap("rock bands + rain");
        erosion::erode(grid, cell, &mut h, &up, &erod, &rainv, &sea, p);
        lap("erosion");
        lakes::remove_small_pits(grid, cell, &mut h, &sea, p);
        let d = lakes::drainage(grid, cell, &h, &sea);
        lap("pits + lakes/drainage");
        let order = lakes::stack(grid, &d.receiver, &sea);
        let per_cell = cell * cell * p.rainfall / (365.25 * 86_400.0);
        let mut q = vec![0.0f64; len];
        for &c in &order {
            q[c as usize] = rainv[c as usize] * per_cell;
        }
        for &c in order.iter().rev() {
            let r = d.receiver[c as usize];
            if !sea[r as usize] {
                q[r as usize] += q[c as usize];
            }
        }
        let outlets: Vec<u32> = d.lakes.iter().map(|l| l.outlet).collect();
        lap("discharge");
        let (rv, _m) = rivers::trace(
            &rivers::Inputs {
                grid,
                cell,
                x0,
                sea: &sea,
                lake_of: &d.lake_of,
                outlets: &outlets,
                receiver: &d.receiver,
                discharge: &q,
                surface: &d.surface,
            },
            p,
        );
        println!("  rivers {} lakes {}", rv.len(), d.lakes.len());
        lap("rivers");
        drop((h, up, erod, rainv, q, order, coast, u, d, rv, sea, h0));
        lap("(drop scratch)");
    }

    // The real build, for the map the game keeps (and a check the steps above match its cost).
    let hm = hydrology::generate_seed(seed, p);
    lap("hydrology (whole, again)");
    let map = WorldMap::from_hydrology(seed, &hm);
    drop(hm);
    lap("world map (channels, shores)");
    let bytes = codec::encode(&map);
    println!("  encoded map {:.1} MB", bytes.len() as f64 / 1e6);
    lap("encode");
    let bf = dwell_worldgen::biomemap::BiomeField::new(&map, world_seed);
    std::hint::black_box(&bf);
    drop(bf);
    lap("biome field (chamfers)");
    let map = Arc::new(map);
    let g = TerrainGenerator::with_map(seed, map.clone());
    lap("generator (biome field)");
    let total_map = t_all.elapsed().as_secs_f64();

    let spawn = if std::env::var("SKIP_SPAWN").is_ok() {
        [0.5, f64::from(g.column_lod(0, 0, 1).height) + 1.0, 0.5]
    } else {
        g.spawn_point()
    };
    println!("  spawn at {:.0} {:.1} {:.0}", spawn[0], spawn[1], spawn[2]);
    lap("spawn search");

    // Chunks round the spawn: 3 × 3 columns × 4 rows deep.
    let c0 = chunk_of_pos(spawn);
    let mut n = 0;
    let tc = Instant::now();
    for dz in -1..=1 {
        for dx in -1..=1 {
            for dy in -2..=1 {
                let mut chunk = Chunk::default();
                g.generate(ChunkCoord::new(c0.x + dx, c0.y + dy, c0.z + dz), &mut chunk);
                n += 1;
            }
        }
    }
    if let Ok(secs) = std::env::var("CHUNK_LOOP") {
        let secs: f64 = secs.parse().unwrap();
        let tl = Instant::now();
        let mut m = 0;
        while tl.elapsed().as_secs_f64() < secs {
            let mut chunk = Chunk::default();
            g.generate(
                ChunkCoord::new(c0.x + m % 7, c0.y, c0.z + m / 7 % 7),
                &mut chunk,
            );
            m += 1;
        }
        println!(
            "  chunk loop: {m} chunks, {:.2} ms each",
            secs * 1e3 / f64::from(m)
        );
    }
    println!(
        "  chunks at spawn: {n}, {:.2} ms each",
        tc.elapsed().as_secs_f64() * 1e3 / f64::from(n)
    );
    lap("chunk generation");

    // The big map's widest level: 32 × 32 cells of 256 m a tile, clipped to the disc.
    let (cell_m, tile) = (256i64, 32i64);
    let span = cell_m * tile;
    let r = i64::from(WORLD_RADIUS);
    let k = (r + span - 1) / span;
    let mut tiles = Vec::new();
    for tk in -k..k {
        for ti in -k..k {
            // Any corner or the nearest point inside the disc.
            let (ax, az) = (ti * span, tk * span);
            let nx = 0i64.clamp(ax, ax + span);
            let nz = 0i64.clamp(az, az + span);
            if nx * nx + nz * nz < r * r {
                tiles.push((ti, tk));
            }
        }
    }
    // Time a sample spread over the disc (every tile up to 60).
    let step = (tiles.len() / 60).max(1);
    let tt = Instant::now();
    let mut sampled = 0;
    let mut sink = 0f64;
    for &(ti, tk) in tiles.iter().step_by(step) {
        for gz in tk * tile..(tk + 1) * tile {
            for gx in ti * tile..(ti + 1) * tile {
                let (x, z) = (gx * cell_m + cell_m / 2, gz * cell_m + cell_m / 2);
                let col = g.column_lod(x, z, cell_m);
                if col.outside {
                    continue;
                }
                sink += f64::from(col.height) + f64::from(g.top_state(&col));
                if let Some(h) = g.river_hit(x, z, 0.5 * cell_m as f64) {
                    sink += h.d;
                }
            }
        }
        sampled += 1;
    }
    let per_tile = tt.elapsed().as_secs_f64() * 1e3 / f64::from(sampled);
    println!(
        "  widest level: {} tiles, {per_tile:.1} ms a tile ({sampled} timed) -> {:.1} s native (sink {:.0})",
        tiles.len(),
        per_tile * tiles.len() as f64 / 1e3,
        sink.signum()
    );
    lap("big map tiles (sample)");
    println!("TOTAL map+biome {total_map:.1} s");
}
