//! Rock units per 2 × 2 km window in a Realistic world (WORK_QUEUE item 15, real-world scale; the
//! realism audit: "rock provinces mostly a 2.4 km noise over 45–81 % of land"): the mean number of
//! distinct province stacks over 2 km windows of the land, and each stack's share.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_rock_units -- 1 7 42`

use std::collections::BTreeMap;
use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    const NAME: &str = "test:realistic";
    let seeds: Vec<u64> = std::env::args()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    let (mut units, mut windows) = (0usize, 0usize);
    let mut share = BTreeMap::<&str, usize>::new();
    for world in seeds {
        for face in [Face::A, Face::B] {
            let e = world_config(NAME).unwrap().unwrap();
            let seed = face_seed(world, face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.unwrap())
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            for wz in (-40_000..40_000).step_by(4000) {
                for wx in (-40_000..40_000).step_by(4000) {
                    let mut seen = std::collections::BTreeSet::new();
                    for j in 0..5 {
                        for i in 0..5 {
                            let (x, z) = (wx + i * 500, wz + j * 500);
                            let c = g.column_at(x, z);
                            if c.outside || c.height < dwell_world::SEA_LEVEL_M as f32 {
                                continue;
                            }
                            let p = g.province_at(x.into(), z.into());
                            seen.insert(p);
                            *share.entry(p).or_default() += 1;
                        }
                    }
                    if !seen.is_empty() {
                        units += seen.len();
                        windows += 1;
                    }
                }
            }
        }
    }
    let total: usize = share.values().sum();
    println!(
        "rock units per 2 × 2 km of land {:.2} over {windows} windows; shares {:?}",
        units as f64 / windows.max(1) as f64,
        share
            .iter()
            .map(|(k, v)| (k.trim_start_matches("dwell:"), (100 * v / total.max(1))))
            .collect::<Vec<_>>()
    );
}
