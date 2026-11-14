use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::plate_tectonics::PlateTectonics;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;
use std::sync::Arc;
fn main() {
    let e = world_config("test:realistic").unwrap().unwrap();
    let seed = face_seed(2741893632, Face::A);
    let t = PlateTectonics::for_seed(seed);
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.unwrap())
            .with_worldgen("test:realistic")
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map.clone())
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    for v in t.volcanoes.iter().filter(|v| v.chain == 0).take(3) {
        println!(
            "cone at ({:.0},{:.0}) h {:.0} r {:.0}",
            v.x, v.z, v.height, v.radius
        );
        let mut prev = None;
        for k in 0..40 {
            let r = k as f64 * 100.0;
            let mut sum = 0.0;
            for a in 0..36 {
                let th = a as f64 * std::f64::consts::TAU / 36.0;
                sum += map.height_at(v.x + r * th.cos(), v.z + r * th.sin()).0;
            }
            let m = sum / 36.0;
            let base = t.height(v.x + r, v.z);
            println!(
                "  r {r:.0}: ring mean {m:.0} drop {:.0} (landform+cone {base:.0})",
                prev.map_or(0.0, |p: f64| p - m)
            );
            prev = Some(m);
        }
    }
}
