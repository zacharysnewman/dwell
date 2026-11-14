//! Long straight seams in a Realistic world's climate fields (BUGS.md #158): the °C and the rain
//! each texel of 50 m reads, the largest neighbour jumps lined up along one axis.
use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;
use std::sync::Arc;
const STEP: i64 = 50;
fn main() {
    let e = world_config("test:realistic").unwrap().unwrap();
    for w in std::env::args().skip(1).map(|a| a.parse::<u64>().unwrap()) {
        for face in [Face::A, Face::B] {
            let seed = face_seed(w, face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.unwrap())
                    .with_worldgen("test:realistic")
                    .unwrap(),
            );
            let field = BiomeField::with_params(&map, seed, e.map);
            let r = 48_000 / STEP;
            let n = (2 * r + 1) as usize;
            let mut c = vec![f64::NAN; n * n];
            let mut rain = vec![f64::NAN; n * n];
            for j in 0..n {
                for i in 0..n {
                    let (x, z) = ((i as i64 - r) * STEP, (j as i64 - r) * STEP);
                    let cell = map.cell_at(x as f64, z as f64);
                    if map.is_sea(cell) {
                        continue;
                    }
                    // A flat elevation: the climate alone, not the ground's lapse.
                    let g = field.geography(x, z, 100.0);
                    c[j * n + i] = g.celsius;
                    rain[j * n + i] = g.rain.unwrap_or(f64::NAN);
                }
            }
            for (name, v, jump) in [("celsius", &c, 0.8), ("rain", &rain, 60.0)] {
                let mut best = (0usize, 0usize, 0usize, 'v');
                for i in 0..n - 1 {
                    let mut run = 0;
                    for j in 0..n {
                        let (a, b) = (v[j * n + i], v[j * n + i + 1]);
                        if (a - b).abs() > jump {
                            run += 1;
                            if run > best.0 {
                                best = (run, i, j, 'v');
                            }
                        } else {
                            run = 0;
                        }
                    }
                }
                for j in 0..n - 1 {
                    let mut run = 0;
                    for i in 0..n {
                        let (a, b) = (v[j * n + i], v[(j + 1) * n + i]);
                        if (a - b).abs() > jump {
                            run += 1;
                            if run > best.0 {
                                best = (run, i, j, 'h');
                            }
                        } else {
                            run = 0;
                        }
                    }
                }
                let (run, i, j, d) = best;
                println!(
                    "{w} {face:?} {name}: longest straight {d} jump (> {jump}) {} m ending ({}, {})",
                    run as i64 * STEP,
                    (i as i64 - r) * STEP,
                    (j as i64 - r) * STEP
                );
            }
        }
    }
}
