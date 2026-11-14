//! Where a Realistic world's ground types meet along straight edges (WORK_QUEUE item 15, 0.69: "the
//! long straight dirt→grass line"): windows of the top blocks at 2 m over a face's land, each
//! boundary cell between two named types fitted with a line over 100 m; the windows with the most
//! straight boundary cells, by pair of types.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_edges -- 1 42 2741893632`

#[path = "../tests/support/map_shapes.rs"]
mod map_shapes;

use std::collections::BTreeMap;
use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;
use map_shapes::Mask;

const N: usize = 160;
const STEP: i32 = 2;

fn top(g: &TerrainGenerator, x: i32, z: i32) -> &'static str {
    let name = match g.ground_y(x, z) {
        Some(y) => dwell_blocks::info(g.state_at(x, y, z)).name,
        None => "dwell:air",
    };
    let base = name.split('[').next().unwrap_or(name);
    base.strip_suffix("_ramp").unwrap_or(base)
}

fn main() {
    const NAME: &str = "test:realistic";
    let seeds: Vec<u64> = std::env::args()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    for world in seeds {
        for face in [Face::A, Face::B] {
            let e = world_config(NAME).unwrap().unwrap();
            let seed = face_seed(world, face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.unwrap())
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map.clone())
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            let land: Vec<usize> = (0..map.height.len())
                .filter(|&c| !map.is_sea(c) && map.centre(c).0.hypot(map.centre(c).1) < 40_000.0)
                .collect();
            let mut pairs = BTreeMap::<String, usize>::new();
            let mut worst = Vec::new();
            for k in 0..24 {
                let (cx, cz) = map.centre(land[land.len() * (2 * k + 1) / 48]);
                let (cx, cz) = (cx as i32, cz as i32);
                let names: Vec<&str> = (0..N * N)
                    .map(|c| {
                        let (i, j) = ((c % N) as i32, (c / N) as i32);
                        top(&g, cx + (i - 80) * STEP, cz + (j - 80) * STEP)
                    })
                    .collect();
                let edge: Vec<bool> = (0..N * N)
                    .map(|c| {
                        (c % N + 1 < N && names[c + 1] != names[c])
                            || (c + N < names.len() && names[c + N] != names[c])
                    })
                    .collect();
                let mask = Mask {
                    n: N,
                    cell: f64::from(STEP),
                    on: vec![true; N * N],
                };
                let (share, n) = mask.straight_share(&edge, 60.0, 1.5);
                for c in (0..N * N).filter(|&c| edge[c]) {
                    let other = if c % N + 1 < N && names[c + 1] != names[c] {
                        names[c + 1]
                    } else {
                        names[c + N]
                    };
                    let mut p = [names[c], other];
                    p.sort();
                    *pairs.entry(format!("{}|{}", p[0], p[1])).or_default() += 1;
                }
                worst.push((share * n as f64, cx, cz, share));
            }
            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
            println!("seed {world} {face:?}: worst windows {:?}", &worst[..4]);
            println!("  boundary pairs {pairs:?}");
        }
    }
}
