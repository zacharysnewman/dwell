//! Mesa country on typical seeds (WORK_QUEUE item 15, the map check of 2026-11-14: "mesa country
//! on typical seeds"). On seeds 1, 7, 42, 274005646 and 2741893632, both faces: every face outside
//! the subarctic band holds at least [`MIN_SHARE`] of its land in the red rock mesa
//! (`preset:red_rock_mesa`; 0.9 % to 27 % when written). A subarctic face (both of seed 7's) has
//! no warm dry ground for it, as it should.

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];
const MIN_SHARE: f64 = 0.005;

both_faces!(
    fn every_face_outside_the_subarctic_holds_mesa_country(f: FaceFrame) {
        let e = world_config(NAME).unwrap().expect("the Realistic world");
        let mesa = e
            .biomes
            .iter()
            .position(|b| b.name == "preset:red_rock_mesa")
            .expect("the red rock mesa");
        let mut checked = 0;
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let field = BiomeField::with_params(&map, seed, e.map);
            let band = field.climate().expect("a climate").band().name;
            if band == "subarctic" {
                continue;
            }
            let (mut land, mut found) = (0usize, 0usize);
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
                    found += usize::from(
                        usize::from(field.mix(&e.biomes, x, z, h.max(0.0)).chosen) == mesa,
                    );
                }
            }
            let share = found as f64 / land.max(1) as f64;
            checked += 1;
            assert!(
                share >= MIN_SHARE,
                "seed {world} ({band}): mesa country {:.2} % of the land",
                100.0 * share
            );
        }
        assert!(checked >= 3, "{checked} faces outside the subarctic");
    }
);
