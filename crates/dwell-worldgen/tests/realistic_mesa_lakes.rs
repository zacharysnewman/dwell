//! A lake in mesa country is edged in the mesa's palette (WORK_QUEUE item 15, the map check: "a grey
//! stone band edges a lake in mesa country (the lake shore takes the mesa palette)"). A lake raises
//! the wetness round it past the red rock mesa's envelope (`where.wetness`), so another biome took
//! its shore, with grey stone on the steep banks. Not reproduced on 0.70's ground (0 of 167 and 1 of
//! 685 shore columns grey on seeds 1, 7, 42 and 2741893632): a guard. On seeds 1 and 2741893632,
//! both faces:
//! dry columns within 24 m of lake water, where mesa country weighs at least a third 150 m out from
//! the shore, are at most [`GREY`] grey stone.

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 2] = [1, 2_741_893_632];
const GREY: f64 = 0.03;

both_faces!(
    fn mesa_lakes_have_red_shores(f: FaceFrame) {
        let (mut n, mut grey) = (0usize, 0usize);
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
            let mesa = g
                .biomes()
                .iter()
                .position(|b| b.name == "preset:red_rock_mesa")
                .unwrap();
            for z in (-40_000..40_000).step_by(53) {
                for x in (-40_000..40_000).step_by(53) {
                    if g.column_at(x, z).water != Water::Lake {
                        continue;
                    }
                    // Out from the shore along 8 directions: the first dry column within 24 m.
                    for k in 0..8 {
                        let a = f64::from(k) * std::f64::consts::TAU / 8.0;
                        let (dx, dz) = (a.cos(), a.sin());
                        let Some(d) = (1..=24).find(|&d| {
                            let c = g.column_at(
                                x + (dx * f64::from(d)) as i32,
                                z + (dz * f64::from(d)) as i32,
                            );
                            !c.outside && c.water == Water::None
                        }) else {
                            continue;
                        };
                        let (sx, sz) = (
                            x + (dx * f64::from(d)) as i32,
                            z + (dz * f64::from(d)) as i32,
                        );
                        let out = (
                            i64::from(x + (dx * 150.0) as i32),
                            i64::from(z + (dz * 150.0) as i32),
                        );
                        if g.biome_mix(out.0, out.1).weight(mesa) < 0.33 {
                            continue;
                        }
                        let Some(y) = g.ground_y(sx, sz) else {
                            continue;
                        };
                        let top = dwell_blocks::info(g.state_at(sx, y, sz)).name;
                        n += 1;
                        grey += usize::from(top.starts_with("dwell:stone"));
                    }
                }
            }
        }
        let share = grey as f64 / n.max(1) as f64;
        eprintln!(
            "{:?}: {grey} of {n} mesa lake shore columns grey stone ({:.1} %)",
            f.face,
            100.0 * share
        );
        assert!(n >= 30, "{:?}: mesa lake shores sampled ({n})", f.face);
        assert!(
            share <= GREY,
            "{:?}: {:.1} % of mesa lake shores grey stone",
            f.face,
            100.0 * share
        );
    }
);
