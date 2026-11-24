//! No fence-thin walls in mesa country (WORK_QUEUE item 15, the map check: "thin wall fins look like
//! fences (minimum fin/wall thickness, erode or remove thin fins)"). Where two eroded fronts came
//! close, the ground left standing between them could be a strip a few metres wide and tens of
//! metres tall. Not reproduced on 0.70's ground: about 0.3 % of the standing ground is such a fence
//! (a `minWidth` cut tried for it changed nothing measurable and was dropped); a guard. On seeds 1, 7, 42
//! and 2741893632, both faces, standing ground in mesa country: at most [`THIN`] of it is a fence
//! (under [`FENCE`] m thick across, at least 30 m long).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const FENCE: i32 = 6;
const THIN: f64 = 0.02;

both_faces!(
    fn mesa_country_has_no_fence_thin_walls(f: FaceFrame) {
        let (mut strips, mut thin) = (0usize, 0usize);
        for world in SEEDS {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height_with_worldgen(seed, e.height.expect("its height field"), NAME)
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
            let standing = |x: i32, z: i32| {
                g.erosion_at(x.into(), z.into())
                    .is_some_and(|(s, m, _)| s > 0.9 && m > 0.0)
            };
            // The length (m) of standing ground through (x, z) along (dx, dz), up to 60 m.
            let run = |x: i32, z: i32, (dx, dz): (i32, i32)| -> i32 {
                let side = |sx: i32, sz: i32| {
                    (1..=30)
                        .take_while(|&k| standing(x + sx * k, z + sz * k))
                        .count() as i32
                };
                1 + side(dx, dz) + side(-dx, -dz)
            };
            for z in (-40_000..=40_000).step_by(211) {
                for x in (-40_000..=40_000).step_by(211) {
                    if usize::from(g.biome_mix(x.into(), z.into()).chosen) != mesa
                        || !standing(x, z)
                    {
                        continue;
                    }
                    let runs = [(1, 0), (0, 1), (1, 1), (1, -1)].map(|d| run(x, z, d));
                    let (lo, hi) = (*runs.iter().min().unwrap(), *runs.iter().max().unwrap());
                    strips += 1;
                    thin += usize::from(lo < FENCE && hi >= 30);
                }
            }
        }
        let share = thin as f64 / strips.max(1) as f64;
        eprintln!(
            "{:?}: {thin} of {strips} standing columns in fences (under {FENCE} m across, 30 m long) ({:.1} %)",
            f.face,
            100.0 * share
        );
        assert!(strips >= 30, "{:?}: strips sampled ({strips})", f.face);
        assert!(
            share <= THIN,
            "{:?}: {:.1} % of the strips fence-thin",
            f.face,
            100.0 * share
        );
    }
);
