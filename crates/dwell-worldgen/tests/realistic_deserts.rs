//! Hot sand deserts (WORK_QUEUE item 15; owner, 2026-11-13: "Im not seeing any tan deserts is that
//! not a thing yet?"). The climate's rain falls by latitude (climate.json `rainByLatitude`: the
//! subtropical dry belt, about 0.4 at 26°), and the sand desert (`preset:sand_desert`, tan dunes)
//! wins the warm dry lowlands from the red rock mesa (its weight 2.5, below 800 m). On seeds 1, 7,
//! 42 and 2741893632, both faces:
//!
//! - every face in the subtropical band holds at least [`MIN_SHARE`] of its land in sand desert;
//! - the sand desert sits in warm dry climate: at least [`IN_CLIMATE`] of its columns at 12 °C or
//!   more and 325 mm of rain or less (its `where` with the blends).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const MIN_SHARE: f64 = 0.05;
const IN_CLIMATE: f64 = 0.9;

both_faces!(
    fn subtropical_faces_hold_tan_sand_deserts_in_warm_dry_climate(f: FaceFrame) {
        let e = world_config(NAME).unwrap().expect("the Realistic world");
        let desert = e
            .biomes
            .iter()
            .position(|b| b.name == "preset:sand_desert")
            .expect("the sand desert");
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let field = BiomeField::with_params(&map, seed, e.map);
            let band = field.climate().expect("a climate").band().name;
            let (mut land, mut sand, mut fits) = (0usize, 0usize, 0usize);
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
                    if usize::from(field.mix(&e.biomes, x, z, h.max(0.0)).chosen) != desert {
                        continue;
                    }
                    sand += 1;
                    let g = field.geography(x, z, h.max(0.0));
                    fits += usize::from(g.celsius >= 12.0 && g.rain.unwrap_or(f64::MAX) <= 325.0);
                }
            }
            let share = sand as f64 / land.max(1) as f64;
            eprintln!(
                "seed {world} {:?} [{band}]: sand desert {:.1} % of the land, {fits} of {sand} in warm dry climate",
                f.face,
                100.0 * share
            );
            if band == "subtropical" {
                assert!(
                    share >= MIN_SHARE,
                    "seed {world}: sand desert {:.1} %",
                    100.0 * share
                );
            }
            if sand >= 50 {
                assert!(
                    fits as f64 >= IN_CLIMATE * sand as f64,
                    "seed {world}: sand desert in warm dry climate {fits} of {sand}"
                );
            }
        }
    }
);
