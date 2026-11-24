//! Times tree placement on the Realistic world: `trees_in` over patches of one seed's face, and
//! `column_at` alone over the same area, so a change to either shows in seconds.
use std::sync::Arc;
use std::time::Instant;

use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    let name = "test:realistic";
    let e = world_config(name).unwrap().expect("the Realistic world");
    let seed = 7;
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.unwrap())
            .with_worldgen(name)
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map)
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
        .with_placements(&PLACEMENTS);
    let repeats: usize = std::env::args().nth(1).map_or(1, |a| a.parse().unwrap());
    let patches: Vec<(i32, i32)> = (0..200)
        .map(|k| ((k % 20) * 3_100 - 30_000, (k / 20) * 5_900 - 28_000))
        .collect();
    let t = Instant::now();
    let mut trees = 0;
    let mut hash = 0u64;
    for &(x, z) in patches.iter().cycle().take(200 * repeats) {
        for t in g.trees_in((x, z), (x + 63, z + 63)) {
            trees += 1;
            hash = hash.wrapping_mul(31).wrapping_add(
                (t.anchor[0] as u64)
                    ^ ((t.anchor[1] as u64) << 20)
                    ^ ((t.anchor[2] as u64) << 40)
                    ^ u64::from(t.planter) << 7
                    ^ u64::from(t.variant) << 3,
            );
        }
    }
    let trees_s = t.elapsed();
    let t = Instant::now();
    let mut acc = 0f32;
    for &(x, z) in &patches[..20] {
        for dz in 0..64 {
            for dx in 0..64 {
                acc += g.column_at(x + dx, z + dz).height;
            }
        }
    }
    let col = t.elapsed();
    println!(
        "trees_in: 200 patches × repeats {trees_s:?} ({trees} trees, hash {hash:016x}); column_at: {:.1} µs each ({acc:.0})",
        col.as_secs_f64() * 1e6 / (20.0 * 4096.0)
    );
}
