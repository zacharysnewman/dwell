//! Mesa country on typical seeds (WORK_QUEUE item 15, the map check of 2026-11-14: "mesa country
//! on typical seeds"). Before real-world scale (0.71) every face outside the subarctic band held
//! some; now a 1× face spans under a degree of latitude and its lee keeps `rain.leeFloor` of the
//! rain, so only the dry faces do. Over the first [`FACES`] faces with land outside the subarctic
//! (both faces), at least [`MIN_FACES`] hold at least [`MIN_SHARE`] of their land in the red rock
//! mesa (`preset:red_rock_mesa`).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: std::ops::Range<u64> = 0..40;
const MIN_SHARE: f64 = 0.005;
const FACES: usize = 10;
const MIN_FACES: usize = 2;

both_faces!(
    fn dry_faces_outside_the_subarctic_hold_mesa_country(f: FaceFrame) {
        let e = world_config(NAME).unwrap().expect("the Realistic world");
        let mesa = e
            .biomes
            .iter()
            .position(|b| b.name == "preset:red_rock_mesa")
            .expect("the red rock mesa");
        let (mut checked, mut holding, mut report) = (0, 0, Vec::new());
        for world in SEEDS {
            if checked >= FACES {
                break;
            }
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
            // A face nearly all ocean has too little land to count.
            if land < 2000 {
                continue;
            }
            let share = found as f64 / land as f64;
            checked += 1;
            holding += usize::from(share >= MIN_SHARE);
            report.push(format!("seed {world} ({band}): {:.2} %", 100.0 * share));
        }
        eprintln!("{:?}: {}", f.face, report.join(", "));
        assert!(
            checked >= FACES && holding >= MIN_FACES,
            "{:?}: {holding} of {checked} faces hold mesa country: {}",
            f.face,
            report.join(", ")
        );
    }
);
