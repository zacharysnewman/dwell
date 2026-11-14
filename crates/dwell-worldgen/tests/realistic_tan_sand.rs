//! Tan sand only where it belongs in the Realistic world (WORK_QUEUE item 15, 0.66; owner,
//! 2026-11-14: "I think we're using tan sand where we shouldn't" and "I don't want to see tan sand
//! in red mesa desert, it looks too much like stripes everywhere"): beaches and shores, river and
//! lake beds, the sand desert and its dunes. Elsewhere bare ground is dirt or rock: rocks.json
//! `surfaces.replace` in the Realistic world (shale's talus was tan sand: contour stripes on every
//! bedded hillside) and the `dwell:mesa_country` blend without tan. On seeds 1, 7, 42, 274005646
//! and 2741893632, both faces, a grid of land columns:
//!
//! - tan sand tops a column only on a bed, a shore, the sand desert or the dune sands' rock;
//! - none in mesa country (the red rock mesa) off its shores and beds.

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{Band, TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];

both_faces!(
    fn tan_sand_only_on_shores_beds_and_the_sand_desert(f: FaceFrame) {
        let (mut land, mut sand, mut stray, mut mesa, mut mesa_sand) = (0, 0, 0, 0, 0);
        for world in SEEDS {
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
            let biome = |name: &str| g.biomes().iter().position(|b| b.name == name).unwrap();
            let (desert, red) = (biome("preset:sand_desert"), biome("preset:red_rock_mesa"));
            let (mut here, mut stray_here) = (0, 0);
            for z in (-40_000..40_000).step_by(193) {
                for x in (-40_000..40_000).step_by(193) {
                    let c = g.column_at(x, z);
                    if c.outside || c.water == Water::Sea {
                        continue;
                    }
                    land += 1;
                    let chosen = usize::from(g.biome_mix(x.into(), z.into()).chosen);
                    let wet = c.water != Water::None || c.band == Band::Shore;
                    if chosen == red && !wet {
                        mesa += 1;
                    }
                    let Some(y) = g.ground_y(x, z) else { continue };
                    let name = dwell_blocks::info(g.state_at(x, y, z)).name;
                    if !(name.starts_with("dwell:sand[") || name.starts_with("dwell:sand_ramp[")) {
                        continue;
                    }
                    here += 1;
                    let dunes = g.province_at(x.into(), z.into()) == "dwell:dune_sands";
                    if !(wet || chosen == desert || dunes) {
                        stray_here += 1;
                    }
                    if chosen == red && !wet {
                        mesa_sand += 1;
                    }
                }
            }
            eprintln!(
                "seed {world} {:?}: {here} tan sand columns, {stray_here} off shores, beds and the desert",
                f.face
            );
            sand += here;
            stray += stray_here;
        }
        eprintln!(
            "{:?}: {land} land columns, {sand} tan sand, {stray} stray; mesa country {mesa}, {mesa_sand} tan",
            f.face
        );
        assert!(land > 10_000, "{:?}: land sampled", f.face);
        assert!(
            stray as f64 <= 0.002 * land as f64,
            "{:?}: {stray} of {land} land columns tan sand off shores, beds and the sand desert",
            f.face
        );
        assert_eq!(
            mesa_sand, 0,
            "{:?}: tan sand in mesa country ({mesa} columns)",
            f.face
        );
    }
);
