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

use dwell_world::{FaceFrame, both_faces};
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

/// The Realistic generator of a face with its placements.
fn realistic(world: u64, f: FaceFrame) -> TerrainGenerator {
    let seed = face_seed(world, f.face);
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    TerrainGenerator::with_map(seed, map)
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
        .with_placements(&PLACEMENTS)
}

both_faces!(
    fn sagebrush_is_sparse_on_the_mesa_tops(f: FaceFrame) {
        // The map check, 2026-11-14 (WORK_QUEUE item 15, 0.66): "tops read green under thick
        // sagebrush". Shrubs per m² on the erosion's standing tops against the floors': before,
        // the rule knew no difference (about 1×); now `tops` (0.12) of it.
        let (mut top, mut top_a, mut floor, mut floor_a) = (0usize, 0usize, 0usize, 0usize);
        for world in SEEDS {
            let g = realistic(world, f);
            let mesa = g
                .biomes()
                .iter()
                .position(|b| b.name == "preset:red_rock_mesa")
                .unwrap();
            // A column's erosion part: Some(true) a standing top, Some(false) a floor.
            let part = |x: i32, z: i32| -> Option<bool> {
                let (s, m, _) = g.erosion_at(x.into(), z.into())?;
                if s < 0.9 || g.column_at(x, z).water != Water::None {
                    return None;
                }
                if m > 20.0 {
                    Some(true)
                } else if m < -40.0 {
                    Some(false)
                } else {
                    None
                }
            };
            for pz in (-44_000..=44_000).step_by(1000) {
                for px in (-44_000..=44_000).step_by(1000) {
                    if usize::from(g.biome_mix(px.into(), pz.into()).chosen) != mesa {
                        continue;
                    }
                    for t in g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1)) {
                        let [x, _, z] = t.anchor;
                        if g.planter_name(t.planter) != Some("sagebrush")
                            || !(px..px + PATCH).contains(&x)
                            || !(pz..pz + PATCH).contains(&z)
                        {
                            continue;
                        }
                        match part(x, z) {
                            Some(true) => top += 1,
                            Some(false) => floor += 1,
                            None => {}
                        }
                    }
                    for z in (pz..pz + PATCH).step_by(4) {
                        for x in (px..px + PATCH).step_by(4) {
                            match part(x, z) {
                                Some(true) => top_a += 16,
                                Some(false) => floor_a += 16,
                                None => {}
                            }
                        }
                    }
                }
            }
        }
        let (dt, df) = (
            top as f64 / top_a.max(1) as f64,
            floor as f64 / floor_a.max(1) as f64,
        );
        eprintln!(
            "{:?}: tops {top} on {top_a} m² ({dt:.4}/m²), floors {floor} on {floor_a} m² ({df:.4}/m²)",
            f.face
        );
        assert!(
            top_a > 2000 && floor_a > 2000,
            "{:?}: tops and floors sampled",
            f.face
        );
        assert!(
            dt <= 0.3 * df,
            "{:?}: sagebrush on the tops {dt:.4}/m², the floors {df:.4}/m²",
            f.face
        );
    }
);

both_faces!(
    fn the_sand_desert_is_open_with_sparse_plants(f: FaceFrame) {
        // The map check, 2026-11-14 (WORK_QUEUE item 15, 0.66): the sand desert's shrubs "so dense
        // it reads as scrubland". Every placement (sagebrush, boulders) per hectare of the sand
        // desert's dry ground: open dune desert (the Realistic world's `trees` for it, and its
        // `treesSharpen` so the red rock mesa's dense sagebrush doesn't spill far across the
        // border). Before (0.65): 474 (A) / 615 (B) per ha; 0.66: 55 / 67, about one per 13 m.
        let (mut n, mut area) = (0usize, 0usize);
        let mut by = std::collections::BTreeMap::<&str, usize>::new();
        for world in SEEDS {
            let g = realistic(world, f);
            let desert = g
                .biomes()
                .iter()
                .position(|b| b.name == "preset:sand_desert")
                .unwrap();
            for pz in (-44_000..=44_000).step_by(1000) {
                for px in (-44_000..=44_000).step_by(1000) {
                    if usize::from(g.biome_mix(px.into(), pz.into()).chosen) != desert {
                        continue;
                    }
                    let dry = |x: i32, z: i32| {
                        usize::from(g.biome_mix(x.into(), z.into()).chosen) == desert
                            && g.column_at(x, z).water == Water::None
                    };
                    for t in g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1)) {
                        let [x, _, z] = t.anchor;
                        if (px..px + PATCH).contains(&x)
                            && (pz..pz + PATCH).contains(&z)
                            && dry(x, z)
                        {
                            n += 1;
                            *by.entry(g.planter_name(t.planter).unwrap_or("?"))
                                .or_default() += 1;
                        }
                    }
                    for z in (pz..pz + PATCH).step_by(4) {
                        for x in (px..px + PATCH).step_by(4) {
                            area += 16 * usize::from(dry(x, z));
                        }
                    }
                }
            }
        }
        let per_ha = n as f64 / area.max(1) as f64 * 10_000.0;
        eprintln!(
            "{:?}: {n} plants and rocks on {area} m² of sand desert: {per_ha:.0} per ha ({by:?})",
            f.face
        );
        assert!(area > 5000, "{:?}: sand desert sampled {area} m²", f.face);
        assert!(
            per_ha <= 40.0,
            "{:?}: {per_ha:.0} per ha in the sand desert",
            f.face
        );
    }
);
