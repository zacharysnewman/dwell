//! Rock in ordered units, not a noise (WORK_QUEUE item 15, real-world scale; the realism audit:
//! "rock provinces mostly a 2.4 km noise over 45–81 % of land"). rocks.json `plateProvinces`: each
//! setting one sedimentary cover in regional units (~16 km), the basement's granite only as
//! discrete bodies, basalt only at volcanoes and in rifts. On seeds 1 and 42, both faces: about one
//! rock unit per 2 × 2 km of land (3.6 before), and no basalt flows off the rifts.

use std::collections::BTreeSet;
use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::PlateSetting;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";

both_faces!(
    fn rock_comes_in_ordered_units(f: FaceFrame) {
        let (mut units, mut windows, mut basalt) = (0usize, 0usize, 0usize);
        for world in [1u64, 42] {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            for wz in (-40_000..40_000).step_by(4000) {
                for wx in (-40_000..40_000).step_by(4000) {
                    let mut seen = BTreeSet::new();
                    for j in 0..5 {
                        for i in 0..5 {
                            let (x, z) = (wx + i * 500, wz + j * 500);
                            let c = g.column_at(x, z);
                            if c.outside || c.height < dwell_world::SEA_LEVEL_M as f32 {
                                continue;
                            }
                            let p = g.province_at(x.into(), z.into());
                            let rift = g
                                .plate_settings_at(x.into(), z.into())
                                .is_some_and(|s| s.has(PlateSetting::Rift));
                            basalt += usize::from(p == "dwell:basalt_flows" && !rift);
                            seen.insert(p);
                        }
                    }
                    if !seen.is_empty() {
                        units += seen.len();
                        windows += 1;
                    }
                }
            }
        }
        let mean = units as f64 / windows.max(1) as f64;
        eprintln!(
            "{:?}: {mean:.2} rock units per 2 × 2 km over {windows} windows, {basalt} basalt off the rifts",
            f.face
        );
        assert!(windows >= 100, "{:?}: land windows sampled", f.face);
        assert!(
            mean <= 1.7,
            "{:?}: {mean:.2} rock units per 2 × 2 km",
            f.face
        );
        assert_eq!(basalt, 0, "{:?}: basalt flows off the rifts", f.face);
    }
);
