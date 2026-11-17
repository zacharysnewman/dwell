//! Bedded hills under grass where it rains (WORK_QUEUE item 15, the 0.69 follow-up: "brown contour
//! stripes on stone hills"). Where the rock shapes the ground its treads and talus showed the rock's
//! cap and scree (shale's dirt, limestone's stone) on every step, stripes following the contours
//! across whole steppe hillsides. rocks.json `shaping.coverRain` (the Realistic world's: 260 mm
//! ± 60): the treads and talus take the biome's own ground there, only the walls the rock. On seeds
//! 1, 7, 42, 274005646 and 2741893632, both faces, dry land with at least 320 mm of rain (past the
//! noisy edge): at most [`BARE`] of the columns off the walls show a bedded rock's stone or dirt (a volcano's
//! ground and the red desert's beds show their own rock on purpose).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{Band, TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];
const BARE: f64 = 0.03;

both_faces!(
    fn wet_bedded_hills_are_grassy_steps_not_stripes(f: FaceFrame) {
        let (mut n, mut bare) = (0usize, 0usize);
        let mut by = std::collections::BTreeMap::<String, usize>::new();
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
            let (mut n_here, mut bare_here) = (0, 0);
            for z in (-40_000..40_000).step_by(131) {
                for x in (-40_000..40_000).step_by(131) {
                    let c = g.column_at(x, z);
                    if c.outside
                        || c.water != Water::None
                        || c.band != Band::Ground
                        || c.wall
                        || g.rain_at(x.into(), z.into()).is_none_or(|r| r < 320.0)
                    {
                        continue;
                    }
                    n_here += 1;
                    if !g.is_rock_palette(&c) {
                        continue;
                    }
                    // The stripes: a bedded rock's cap or talus (stone, dirt); a volcano's ground
                    // and the red desert's beds show their own rock on purpose.
                    let province = g.province_at(x.into(), z.into());
                    if province.contains("volcanic") || province.contains("hotspot") {
                        continue;
                    }
                    let top = g
                        .ground_y(x, z)
                        .map_or("?", |y| dwell_blocks::info(g.state_at(x, y, z)).name);
                    let top = top.split('[').next().unwrap_or(top);
                    let top = top.strip_suffix("_ramp").unwrap_or(top);
                    if top == "dwell:dirt" || top == "dwell:stone" {
                        bare_here += 1;
                        *by.entry(format!("{province} {top}")).or_default() += 1;
                    }
                }
            }
            eprintln!(
                "seed {world} {:?}: {bare_here} of {n_here} wet ground columns show a rock",
                f.face
            );
            n += n_here;
            bare += bare_here;
        }
        let share = bare as f64 / n.max(1) as f64;
        eprintln!(
            "{:?}: {bare} of {n} ({:.1} %): {by:?}",
            f.face,
            100.0 * share
        );
        assert!(n > 10_000, "{:?}: wet ground sampled ({n})", f.face);
        assert!(
            share <= BARE,
            "{:?}: {:.1} % of the wet ground off the walls shows a rock's blocks",
            f.face,
            100.0 * share
        );
    }
);
