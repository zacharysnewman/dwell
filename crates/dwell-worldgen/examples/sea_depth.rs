//! How deep the seas are (WORK_QUEUE: owner, 2026-11-18, "We may need to generally deepen oceans";
//! "Show me options first"): sampled sea columns' depth below sea level, by share, per seed and
//! face, on the build's terrain or a test world (`--world test:realistic`).
//!
//! `cargo run --release -p dwell-worldgen --example sea_depth -- 0 1 7 [--world test:realistic]`

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let world = args
        .iter()
        .position(|a| a == "--world")
        .map(|i| args[i + 1].clone());
    let seeds: Vec<u64> = args
        .iter()
        .filter(|a| a.parse::<u64>().is_ok())
        .map(|s| s.parse().unwrap())
        .collect();
    let bins = [2.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, f64::MAX];
    println!("world {}", world.as_deref().unwrap_or("build's"));
    for s in seeds {
        for face in [Face::A, Face::B] {
            let seed = face_seed(s, face);
            let g = match &world {
                None => TerrainGenerator::new(seed),
                Some(n) => {
                    let e = world_config(n).unwrap().expect("a test world");
                    let map = Arc::new(
                        WorldMap::for_height(seed, e.height.expect("height"))
                            .with_worldgen(n)
                            .unwrap(),
                    );
                    TerrainGenerator::with_map(seed, map)
                        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                }
            };
            let mut depths = Vec::new();
            // The open sea nearest the origin, for screenshots.
            let mut nearest: Option<(i64, i32, i32, f64)> = None;
            for z in (-30_000..30_000).step_by(211) {
                for x in (-30_000..30_000).step_by(211) {
                    let c = g.column_at(x, z);
                    if c.outside || c.water != Water::Sea {
                        continue;
                    }
                    let d = dwell_world::SEA_LEVEL_M - f64::from(c.height);
                    depths.push(d);
                    let r2 = i64::from(x).pow(2) + i64::from(z).pow(2);
                    if d >= 20.0 && nearest.is_none_or(|n| r2 < n.0) {
                        nearest = Some((r2, x, z, d));
                    }
                }
            }
            depths.sort_by(f64::total_cmp);
            let n = depths.len().max(1) as f64;
            let at = |q: f64| depths.get(((n - 1.0) * q) as usize).copied().unwrap_or(0.0);
            let mut lo = 0.0;
            let shares: Vec<String> = bins
                .iter()
                .map(|&hi| {
                    let k = depths.iter().filter(|&&d| d >= lo && d < hi).count();
                    let label = if hi == f64::MAX {
                        format!("{lo}+")
                    } else {
                        format!("{lo}-{hi}")
                    };
                    lo = hi;
                    format!("{label} m {:.0}%", 100.0 * k as f64 / n)
                })
                .collect();
            println!(
                "seed {s} face {face:?}: {} sea samples, depth p10 {:.1} median {:.1} p90 {:.1} max {:.1} | {}",
                depths.len(),
                at(0.1),
                at(0.5),
                at(0.9),
                at(1.0),
                shares.join(", ")
            );
            if let Some((_, x, z, d)) = nearest {
                println!("  open sea nearest the origin: ({x}, {z}), {d:.1} m deep");
                // Toward the origin from there: the depth every 250 m to the shore.
                let mut line = Vec::new();
                for k in 0..40 {
                    let t = 1.0 - f64::from(k) * 250.0 / f64::from(x).hypot(f64::from(z));
                    let (px, pz) = ((f64::from(x) * t) as i32, (f64::from(z) * t) as i32);
                    let c = g.column_at(px, pz);
                    if c.water != Water::Sea {
                        line.push(format!("shore at ({px}, {pz})"));
                        break;
                    }
                    line.push(format!(
                        "{:.0}",
                        dwell_world::SEA_LEVEL_M - f64::from(c.height)
                    ));
                }
                println!("  depth every 250 m to the shore: {}", line.join(" "));
            }
        }
    }
}
