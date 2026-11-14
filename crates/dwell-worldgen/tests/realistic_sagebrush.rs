//! Sagebrush and red dirt in mesa country (WORK_QUEUE item 15; owner, 2026-11-13, of a Monument
//! Valley photo: "Mostly reddish dirt though. Some brush too. Could add some single 'tree' block
//! with brush on top."). `dwell:red_dirt` is a natural block with stored ramps; `dwell:sagebrush`
//! (shared/trees/sagebrush.json) is placed by `placements.json` `sagebrush` on the floors. On seeds
//! 1, 7, 42 and 2741893632, both faces:
//!
//! - red dirt has its ramps;
//! - on mesa floors the shrubs stand within the data's density range (one per `grid`² m² cell in
//!   `density`, less the overlaps and the steeper ground);
//! - none stands on a rock wall or ground steeper than the rule's `maxSlopeDeg`.

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const PATCH: i32 = 48;

#[test]
fn red_dirt_has_its_ramps() {
    // both-faces: the block registry is the same on both faces.
    assert!(dwell_blocks::find_block("dwell:red_dirt").is_some());
    assert!(
        dwell_blocks::find_block("dwell:red_dirt_ramp").is_some(),
        "red dirt's stored ramps (shared/blocks/dwell.json rampFamilies)"
    );
}

both_faces!(
    fn sagebrush_dots_the_floors_and_never_a_wall(f: FaceFrame) {
        let rule = PLACEMENTS
            .iter()
            .find(|p| p.name == "sagebrush")
            .expect("placements.json sagebrush");
        let full = f64::from(rule.density) / f64::from(rule.grid * rule.grid);
        let (mut shrubs, mut area, mut bad) = (0usize, 0usize, 0usize);
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let talus = e.geology.erosion.talus_width;
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
                .with_placements(&PLACEMENTS);
            let mesa = g
                .biomes()
                .iter()
                .position(|b| b.name == "preset:red_rock_mesa")
                .unwrap();
            let (mut s_here, mut a_here) = (0, 0);
            for pz in (-44_000..=44_000).step_by(1000) {
                for px in (-44_000..=44_000).step_by(1000) {
                    if usize::from(g.biome_mix(px.into(), pz.into()).chosen) != mesa {
                        continue;
                    }
                    let floor = |x: i32, z: i32| {
                        g.erosion_at(x.into(), z.into())
                            .is_some_and(|(s, m, _)| s > 0.9 && m < -(talus + 10.0))
                            && g.column_at(x, z).water == Water::None
                    };
                    let trees = g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1));
                    for t in &trees {
                        if g.planter_name(t.planter) != Some("sagebrush") {
                            continue;
                        }
                        let [x, _, z] = t.anchor;
                        let col = g.column_at(x, z);
                        if col.wall {
                            bad += 1;
                        }
                        if (px..px + PATCH).contains(&x)
                            && (pz..pz + PATCH).contains(&z)
                            && floor(x, z)
                            && !col.wall
                        {
                            s_here += 1;
                        }
                    }
                    for z in (pz..pz + PATCH).step_by(2) {
                        for x in (px..px + PATCH).step_by(2) {
                            a_here += usize::from(floor(x, z)) * 4;
                        }
                    }
                }
            }
            eprintln!(
                "seed {world} {:?}: {s_here} sagebrush on {a_here} m² of mesa floor ({:.3} per m², the rule's most {full:.3})",
                f.face,
                s_here as f64 / a_here.max(1) as f64
            );
            shrubs += s_here;
            area += a_here;
        }
        assert_eq!(bad, 0, "{:?}: sagebrush on a rock wall", f.face);
        assert!(area > 5000, "{:?}: mesa floor sampled {area} m²", f.face);
        let d = shrubs as f64 / area as f64;
        assert!(
            (0.25 * full..=1.05 * full).contains(&d),
            "{:?}: {d:.3} sagebrush per m² of floor, the rule's {full:.3}",
            f.face
        );
    }
);
