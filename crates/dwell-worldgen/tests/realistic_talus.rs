//! Talus at cliff feet, into the water too (WORK_QUEUE item 15, the 0.69 follow-up; owner,
//! 2026-11-14: "Stone cliff regions should get rocks and boulders at the base of cliffs. Including
//! in the water."). placements.json `talus`: stones and blocks under rock walls and steep rock,
//! densest at a wall's foot and none `cliffReach` m out (`TerrainGenerator::cliff_factor`), on beds
//! under water too (`wet`). On seeds 1, 7, 42 and 2741893632, both faces, patches of the land:
//!
//! - every talus piece lies within the reach of a wall or steep rock;
//! - there are more at the foot than farther out (counts by distance fall);
//! - some stand on a bed under water (a cliff at a shore).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::{Band, TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const PATCH: i32 = 64;

/// The distance (m, Chebyshev, in whole metres) from (x, z) to the nearest wall or steep-rock
/// column within `r`, if any.
fn to_rock(g: &TerrainGenerator, x: i32, z: i32, r: i32) -> Option<i32> {
    (1..=r).find(|&d| {
        (-d..=d).any(|i| {
            [(i, -d), (i, d), (-d, i), (d, i)].iter().any(|(dx, dz)| {
                let c = g.column_at(x + dx, z + dz);
                !c.outside && (c.wall || c.band == Band::Rock)
            })
        })
    })
}

both_faces!(
    fn talus_lies_at_cliff_feet_and_in_the_shallows(f: FaceFrame) {
        let reach = PLACEMENTS
            .iter()
            .find(|p| p.name == "talus")
            .expect("placements.json talus")
            .cliff_reach as i32;
        let mut bins = [0usize; 3];
        let (mut far, mut wet, mut n) = (0, 0, 0);
        for world in SEEDS {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map.clone())
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
                .with_placements(&PLACEMENTS);
            // Patches over the land, and on its coasts (where a cliff can meet the water).
            let n_map = map.n;
            let coast = (0..map.height.len()).filter(|&c| {
                map.is_sea(c)
                    && [c + 1, c + n_map, c.wrapping_sub(1), c.wrapping_sub(n_map)]
                        .iter()
                        .any(|&k| k < map.height.len() && !map.is_sea(k))
            });
            let coast: Vec<(i32, i32)> = coast
                .step_by(97)
                .take(60)
                .map(|c| {
                    let (x, z) = map.centre(c);
                    (x as i32 - PATCH / 2, z as i32 - PATCH / 2)
                })
                .collect();
            let land = (-40_000..=40_000)
                .step_by(3000)
                .flat_map(|pz| (-40_000..=40_000).step_by(3000).map(move |px| (px, pz)));
            for (px, pz) in land.chain(coast) {
                {
                    for t in g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1)) {
                        let [x, _, z] = t.anchor;
                        if g.planter_name(t.planter) != Some("talus")
                            || !(px..px + PATCH).contains(&x)
                            || !(pz..pz + PATCH).contains(&z)
                        {
                            continue;
                        }
                        n += 1;
                        if g.column_at(x, z).water != Water::None {
                            // Under water a sea cliff's face (in the shore band) counts too: the
                            // rule's own reading.
                            wet += 1;
                            far += usize::from(
                                g.cliff_factor(x.into(), z.into(), f64::from(reach)) <= 0.0,
                            );
                            continue;
                        }
                        match to_rock(&g, x, z, reach + 2) {
                            Some(d) => bins[(3 * (d - 1) / reach).min(2) as usize] += 1,
                            None => far += 1,
                        }
                    }
                }
            }
        }
        eprintln!(
            "{:?}: {n} talus pieces, by distance to rock {bins:?} (thirds of {reach} m), {far} farther, {wet} under water",
            f.face
        );
        assert!(n >= 50, "{:?}: talus sampled ({n})", f.face);
        assert_eq!(far, 0, "{:?}: talus away from any cliff", f.face);
        assert!(
            bins[0] > bins[1] && bins[1] > bins[2],
            "{:?}: talus should thin out from the cliff foot: {bins:?}",
            f.face
        );
        assert!(wet > 0, "{:?}: no talus under water", f.face);
    }
);
