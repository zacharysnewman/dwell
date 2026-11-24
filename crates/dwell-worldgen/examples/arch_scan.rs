//! Counts arches and overhangs on a world config's face A in 12 km squares (`arch_scan <config>
//! <seed>`), to find where its mesa's arches stand after a terrain change.
use std::sync::Arc;

use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let name = a.get(1).map_or("test:starter", String::as_str);
    let seed: u64 = a.get(2).map_or(0, |s| s.parse().unwrap());
    let e = world_config(name).unwrap().expect("the config");
    let fs = face_seed(seed, dwell_world::Face::A);
    let map = WorldMap::for_height_with_worldgen(fs, e.height.expect("its height"), name).unwrap();
    let g = TerrainGenerator::with_map(fs, Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    let r = 6_000;
    let mut rows = Vec::new();
    for cz in (-42_000..=42_000).step_by(3_000) {
        for cx in (-42_000..=42_000).step_by(3_000) {
            let sites = g.arches_near((cx - r, cz - r), (cx + r, cz + r));
            let arches = sites.iter().filter(|s| s.through).count();
            if arches > 0 {
                rows.push((arches as i64 - (sites.len() - arches) as i64, arches, sites.len() - arches, cx, cz));
            }
        }
    }
    rows.sort_by(|a, b| b.cmp(a));
    for (_, arches, over, cx, cz) in rows.iter().take(12) {
        println!("({cx}, {cz}): {arches} arches, {over} overhangs");
    }
}
