//! Snowy leaves at the snow line (WORK_QUEUE item 15, the 0.69 follow-up; owner, 2026-11-14: "trees
//! at snow level should use snow leaves"). The pines' `snowy` variant (placements.json) took a tree
//! by its height alone (1050–1250 m); the Realistic world's snow line is its biomes' elevation
//! bands (snowy grass and snow as the ground), lower in the cold. Now a variant's `on` also takes a
//! tree standing on snowy grass or snow. On seeds 1, 7, 42 and 2741893632, both faces, the pines of
//! the boreal and alpine biomes:
//!
//! - every tree on snowy grass or snow is snowy;
//! - a tree on plain grass below the height band is not.

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
/// Real-world scale (0.71): a 1× face is mostly one plate's interior, often warm or low, so the
/// seeds run from 0 over the faces with land until both kinds of pine are sampled enough.
const SEEDS: std::ops::Range<u64> = 0..30;

#[path = "support/realistic_seeds.rs"]
mod realistic_seeds;

both_faces!(
    fn trees_at_the_snow_line_have_snowy_leaves(f: FaceFrame) {
        let (mut on_snow, mut snowy_on_snow, mut low, mut snowy_low) = (0, 0, 0, 0);
        for world in SEEDS {
            if on_snow >= 20 && low >= 20 {
                break;
            }
            if realistic_seeds::survey(world, f.face).0 < 0.3 {
                continue;
            }
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
                .with_placements(&PLACEMENTS);
            let cold: Vec<usize> = g
                .biomes()
                .iter()
                .enumerate()
                .filter(|(_, b)| matches!(b.name, "preset:boreal" | "preset:alpine_tundra"))
                .map(|(i, _)| i)
                .collect();
            for pz in (-44_000..=44_000).step_by(700) {
                for px in (-44_000..=44_000).step_by(700) {
                    if !cold.contains(&usize::from(g.biome_mix(px.into(), pz.into()).chosen)) {
                        continue;
                    }
                    for t in g.trees_in((px, pz), (px + 63, pz + 63)) {
                        if g.planter_name(t.planter) != Some("pine_groves") {
                            continue;
                        }
                        let [x, _, z] = t.anchor;
                        let Some(y) = g.ground_y(x, z) else { continue };
                        let ground = dwell_blocks::info(g.state_at(x, y, z)).name;
                        let snowy = t.variant != 0;
                        let above = g.column_at(x, z).height - dwell_world::SEA_LEVEL_M as f32;
                        if ground.starts_with("dwell:snowy_grass")
                            || ground.starts_with("dwell:snow[")
                        {
                            on_snow += 1;
                            snowy_on_snow += usize::from(snowy);
                        } else if above < 1000.0 {
                            low += 1;
                            snowy_low += usize::from(snowy);
                        }
                    }
                }
            }
        }
        eprintln!(
            "{:?}: on snow {snowy_on_snow} snowy of {on_snow}; below the band on grass {snowy_low} snowy of {low}",
            f.face
        );
        assert!(
            on_snow >= 20,
            "{:?}: pines on snowy ground sampled ({on_snow})",
            f.face
        );
        assert_eq!(
            snowy_on_snow, on_snow,
            "{:?}: pines on snow without snowy leaves",
            f.face
        );
        assert!(low >= 20, "{:?}: low pines sampled ({low})", f.face);
        assert_eq!(
            snowy_low, 0,
            "{:?}: snowy leaves on low pines on grass",
            f.face
        );
    }
);
