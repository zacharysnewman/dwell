//! The Realistic world's relief and dryness in numbers (owner, 2026-11-17: "I'm seeing a lot more
//! deserts (plural) than I would expect on average ... fewer mountain ranges or super tall
//! mountains ... tall plains"): per seed and face, the land's height percentiles and highest point,
//! its share above 300 / 600 / 1000 m, the ranges (connected land at or above `RANGE_M`, at least
//! `RANGE_KM2`) and their tops, the high plains (land at or above `PLAIN_M` sloping under
//! `PLAIN_SLOPE` across a map cell), and the land's shares of dry, green, savanna and cold biomes.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_relief -- 1 7 42`

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

const RANGE_M: f32 = 800.0;
const RANGE_KM2: f64 = 2.0;
const PLAIN_M: f32 = 400.0;
const PLAIN_SLOPE: f32 = 0.03;

fn group(name: &str) -> &'static str {
    let n = name.trim_start_matches("preset:");
    match n {
        "cold_desert" | "sand_desert" | "red_rock_mesa" | "badlands" | "steppe" => "dry",
        "savanna" => "savanna",
        "alpine_tundra" | "snowfield" => "cold",
        "tropical_beach" | "coastal_cliffs" => "shore",
        "dwell:default" => "default",
        _ => "green",
    }
}

fn main() {
    const NAME: &str = "test:realistic";
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let mut totals: std::collections::BTreeMap<&str, f64> = Default::default();
    let mut faces: f64 = 0.0;
    for s in std::env::args()
        .skip(1)
        .map(|a| a.parse::<u64>().expect("seed"))
    {
        for face in [Face::A, Face::B] {
            let seed = face_seed(s, face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let n = map.n;
            let cell_km2 = (map.cell / 1000.0).powi(2);
            let land = |c: usize| !map.is_sea(c) && map.height[c].is_finite();
            let mut hs: Vec<f32> = (0..n * n)
                .filter(|&c| land(c))
                .map(|c| map.height[c])
                .collect();
            if hs.is_empty() {
                println!("seed {s} {face:?}: no land");
                continue;
            }
            hs.sort_by(f32::total_cmp);
            let q = |p: f64| hs[((hs.len() - 1) as f64 * p) as usize];
            let above =
                |m: f32| 100.0 * hs.iter().filter(|&&h| h >= m).count() as f64 / hs.len() as f64;
            // Ranges: 4-connected land at or above RANGE_M.
            let mut seen = vec![false; n * n];
            let mut ranges: Vec<(f64, f32)> = Vec::new();
            for c0 in 0..n * n {
                if seen[c0] || !land(c0) || map.height[c0] < RANGE_M {
                    continue;
                }
                let (mut cells, mut top, mut stack) = (0usize, 0f32, vec![c0]);
                seen[c0] = true;
                while let Some(c) = stack.pop() {
                    cells += 1;
                    top = top.max(map.height[c]);
                    let (i, j) = (c % n, c / n);
                    for (di, dj) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                        let (a, b) = (i as i64 + di, j as i64 + dj);
                        if a < 0 || b < 0 || a >= n as i64 || b >= n as i64 {
                            continue;
                        }
                        let d = b as usize * n + a as usize;
                        if !seen[d] && land(d) && map.height[d] >= RANGE_M {
                            seen[d] = true;
                            stack.push(d);
                        }
                    }
                }
                let km2 = cells as f64 * cell_km2;
                if km2 >= RANGE_KM2 {
                    ranges.push((km2, top));
                }
            }
            ranges.sort_by(|a, b| b.0.total_cmp(&a.0));
            // High plains: high land that is flat across a map cell.
            let mut plain = 0usize;
            let mut flatish = [0usize; 3];
            let mut high = 0usize;
            for j in 1..n - 1 {
                for i in 1..n - 1 {
                    let c = j * n + i;
                    if !land(c) || map.height[c] < PLAIN_M {
                        continue;
                    }
                    let h = map.height[c];
                    let steep = [c + 1, c - 1, c + n, c - n]
                        .iter()
                        .map(|&d| (map.height[d] - h).abs() / map.cell as f32)
                        .fold(0f32, f32::max);
                    plain += usize::from(steep < PLAIN_SLOPE);
                    for (k, lim) in [0.05f32, 0.1, 0.2].iter().enumerate() {
                        flatish[k] += usize::from(steep < *lim);
                    }
                    high += 1;
                }
            }
            // The biome groups, on every third map cell of land.
            let field = BiomeField::with_params(&map, seed, e.map);
            let band = field.climate().map_or("none", |c| c.band().name);
            let mut groups: std::collections::BTreeMap<&str, usize> = Default::default();
            let mut sampled = 0usize;
            for j in (0..n).step_by(3) {
                for i in (0..n).step_by(3) {
                    let c = j * n + i;
                    if !land(c) {
                        continue;
                    }
                    let (x, z) = (
                        (map.x0 + i as f64 * map.cell) as i64,
                        (map.x0 + j as f64 * map.cell) as i64,
                    );
                    let b = usize::from(
                        field
                            .mix(&e.biomes, x, z, f64::from(map.height[c].max(0.0)))
                            .chosen,
                    );
                    *groups.entry(group(e.biomes[b].name)).or_default() += 1;
                    sampled += 1;
                }
            }
            let share =
                |g: &str| 100.0 * *groups.get(g).unwrap_or(&0) as f64 / sampled.max(1) as f64;
            for g in ["dry", "green", "savanna", "cold", "shore", "default"] {
                *totals.entry(g).or_default() += share(g);
            }
            faces += 1.0;
            if std::env::var("SLOPES").is_ok() {
                println!(
                    "  land ≥ {PLAIN_M} m: {high} cells; under 5 / 10 / 20 % slope: {:?}",
                    flatish.map(|k| format!("{:.1} %", 100.0 * k as f64 / high.max(1) as f64))
                );
            }
            let tops: Vec<String> = ranges
                .iter()
                .take(4)
                .map(|(k, t)| format!("{k:.0} km² to {t:.0} m"))
                .collect();
            println!(
                "seed {s} {face:?} ({band}): land {:.0} km², heights p50 {:.0} p90 {:.0} p99 {:.0} max {:.0} m; above 300/600/1000 m {:.1}/{:.1}/{:.1} %; ranges ≥{RANGE_M} m: {} [{}]; high plains {:.1} % of land; dry {:.0} % green {:.0} % savanna {:.0} % cold {:.0} %",
                hs.len() as f64 * cell_km2,
                q(0.5),
                q(0.9),
                q(0.99),
                hs[hs.len() - 1],
                above(300.0),
                above(600.0),
                above(1000.0),
                ranges.len(),
                tops.join(", "),
                100.0 * plain as f64 / hs.len() as f64,
                share("dry"),
                share("green"),
                share("savanna"),
                share("cold"),
            );
        }
    }
    let avg: Vec<String> = totals
        .iter()
        .map(|(g, t)| format!("{g} {:.0} %", t / faces.max(1.0)))
        .collect();
    println!("mean over {faces} faces: {}", avg.join(", "));
}
