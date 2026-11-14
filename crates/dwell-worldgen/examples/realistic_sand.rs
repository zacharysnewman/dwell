//! Where a Realistic world's top blocks are tan sand (WORK_QUEUE item 15, 0.66: "tan sand only where
//! it belongs"): sampled land columns whose top is `dwell:sand`, by what put it there (a water bed,
//! the shore band, the sand desert, a rock's palette, a biome's band).
//!
//! `cargo run --release -p dwell-worldgen --example realistic_sand -- 1 7 42`

use std::collections::BTreeMap;
use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{Band, TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    const NAME: &str = "test:realistic";
    let seeds: Vec<u64> = std::env::args()
        .skip(1)
        .map(|s| s.parse().expect("seed"))
        .collect();
    for world in seeds {
        for face in [Face::A, Face::B] {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("height"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            let mut counts = BTreeMap::<String, usize>::new();
            let mut land = 0;
            for z in (-40_000..40_000).step_by(97) {
                for x in (-40_000..40_000).step_by(97) {
                    let c = g.column_at(x, z);
                    if c.outside || c.water == Water::Sea {
                        continue;
                    }
                    land += 1;
                    let Some(y) = g.ground_y(x, z) else { continue };
                    let name = dwell_blocks::info(g.state_at(x, y, z)).name;
                    if !name.starts_with("dwell:sand[") && !name.starts_with("dwell:sand_ramp") {
                        continue;
                    }
                    let b = &e.biomes[usize::from(g.biome_mix(x.into(), z.into()).chosen)].name;
                    let why = if c.water != Water::None {
                        "bed".to_string()
                    } else if c.band == Band::Shore {
                        "shore".to_string()
                    } else if g.is_rock_palette(&c) {
                        format!("rock {}", g.province_at(x.into(), z.into()))
                    } else {
                        format!("biome {b}")
                    };
                    *counts.entry(why).or_default() += 1;
                }
            }
            println!("seed {world} {face:?}: {land} land columns");
            for (k, v) in counts {
                println!("  {v:6} {k}");
            }
        }
    }
}
