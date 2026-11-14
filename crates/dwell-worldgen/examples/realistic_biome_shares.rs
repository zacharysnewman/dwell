//! The Realistic world's biomes' shares of the land per seed and face, with the climate band
//! (WORK_QUEUE item 15's map check: "mesa country on typical seeds").
//!
//! `cargo run --release -p dwell-worldgen --example realistic_biome_shares -- 1 7 42`

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    const NAME: &str = "test:realistic";
    let e = world_config(NAME).unwrap().expect("the Realistic world");
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
            let field = BiomeField::with_params(&map, seed, e.map);
            let band = field.climate().map_or("none", |c| c.band().name);
            let mut count = vec![0usize; e.biomes.len()];
            let mut at: Vec<(i64, i64)> = Vec::new();
            let want = std::env::var("BIOME_AT").ok();
            let mut land = 0usize;
            for j in (0..map.n).step_by(3) {
                for i in (0..map.n).step_by(3) {
                    let c = j * map.n + i;
                    let h = f64::from(map.height[c]);
                    if map.is_sea(c) || !h.is_finite() {
                        continue;
                    }
                    let (x, z) = (
                        (map.x0 + i as f64 * map.cell) as i64,
                        (map.x0 + j as f64 * map.cell) as i64,
                    );
                    land += 1;
                    let b = usize::from(field.mix(&e.biomes, x, z, h.max(0.0)).chosen);
                    count[b] += 1;
                    if want.as_deref() == Some(e.biomes[b].name) && (i + j) % 60 == 0 {
                        at.push((x, z));
                    }
                }
            }
            let mut shares: Vec<(f64, &str)> = count
                .iter()
                .zip(&e.biomes)
                .filter(|(c, _)| **c > 0)
                .map(|(c, b)| (100.0 * *c as f64 / land.max(1) as f64, b.name))
                .collect();
            shares.sort_by(|a, b| b.0.total_cmp(&a.0));
            let list: Vec<String> = shares.iter().map(|(p, n)| format!("{n} {p:.1}%")).collect();
            println!("seed {s} {face:?} ({band}): {}", list.join(", "));
            if !at.is_empty() {
                println!("  at {:?}", &at[..at.len().min(30)]);
            }
        }
    }
}
