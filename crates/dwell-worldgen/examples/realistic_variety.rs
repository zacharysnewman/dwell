//! The Realistic world's variety in plan view (WORK_QUEUE item 15, the realistic batch part B): per
//! seed and face, the rock provinces' and the biomes' largest single region on land (km²), their
//! distinct count per 4 km², the biomes' largest share of the land, the interior's relief and the
//! map's streak anisotropy (`dwell_worldgen::variety`).
//!
//! `cargo run --release -p dwell-worldgen --example realistic_variety [seed …]`

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::variety::{Labels, anisotropy, interior_relief};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";

fn main() {
    let seeds: Vec<u64> = {
        let a: Vec<u64> = std::env::args()
            .skip(1)
            .filter_map(|s| s.parse().ok())
            .collect();
        if a.is_empty() {
            vec![1, 7, 42, 2_741_893_632]
        } else {
            a
        }
    };
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    println!(
        "seed face | prov maxkm2 per4km2 | biome maxshare maxkm2 per4km2 (top) | relief rms p90 m | aniso"
    );
    for world in seeds {
        for face in [Face::A, Face::B] {
            let seed = face_seed(world, face);
            let t = std::time::Instant::now();
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.unwrap())
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let built = t.elapsed();
            let g = TerrainGenerator::with_map(seed, map.clone())
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            let field = BiomeField::with_params(&map, seed, e.map);
            let prov = Labels::on_land(&map, 1, |x, z, _| g.province_at(x, z));
            let bio = Labels::on_land(&map, 1, |x, z, h| {
                e.biomes[usize::from(field.mix(&e.biomes, x, z, h).chosen)].name
            });
            let (top, share) = bio.largest_share();
            let (lo, hi) = interior_relief(&map);
            println!(
                "{world} {face:?} | {:.0} {:.2} | {:.1}% {:.0} {:.2} ({top}) | {:.0} {:.0} | {:.3}   [map {:.1}s]",
                prov.largest_region_km2(),
                prov.distinct_per_4km2(),
                100.0 * share,
                bio.largest_region_km2(),
                bio.distinct_per_4km2(),
                lo,
                hi,
                anisotropy(&map),
                built.as_secs_f64(),
            );
        }
    }
}
