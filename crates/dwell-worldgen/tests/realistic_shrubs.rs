//! Shrubs and bushes in clumps (WORK_QUEUE item 15, the 0.69 follow-up; owner, 2026-11-14: "We
//! also need more shrubbery/bushes etc."). Three shrub species in the tree system
//! (shared/trees `bush`, `low_shrub`, `scrub`) placed in clusters (placements.json `bushes`,
//! `cold_shrubs`, `scrub`, their `groves`), each listed with a density by the Realistic world's
//! biomes and the bushes ruled by the climate's moisture (vegetation.json `dwell:shrubs`). On seeds
//! 1, 7, 42 and 2741893632, both faces, patches of the land:
//!
//! - each kind stands somewhere, within a per-hectare range;
//! - clustered, not uniform: their counts over 16 m cells vary well beyond a random scatter's
//!   (variance over mean above [`CLUSTERED`]; a uniform random scatter reads about 1).

use std::collections::BTreeMap;
use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const PATCH: i32 = 128;
const CELL: i32 = 16;
const CLUSTERED: f64 = 1.5;
const KINDS: [&str; 3] = ["bushes", "cold_shrubs", "scrub"];

both_faces!(
    fn shrubs_and_bushes_grow_in_clumps(f: FaceFrame) {
        // Per kind: shrubs, dry land area (m²) of the patches holding any, and the cells' counts.
        let mut n = BTreeMap::<&str, usize>::new();
        let mut area = BTreeMap::<&str, usize>::new();
        let mut cells = BTreeMap::<&str, Vec<f64>>::new();
        for world in SEEDS {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height_with_worldgen(seed, e.height.expect("its height field"), NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
                .with_placements(&PLACEMENTS);
            for pz in (-40_000..=40_000).step_by(4000) {
                for px in (-40_000..=40_000).step_by(4000) {
                    let c = g.column_at(px, pz);
                    if c.outside || c.water != Water::None {
                        continue;
                    }
                    let mut here = BTreeMap::<&str, Vec<f64>>::new();
                    for t in g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1)) {
                        let [x, _, z] = t.anchor;
                        let Some(k) = g.planter_name(t.planter).filter(|k| KINDS.contains(k))
                        else {
                            continue;
                        };
                        if !(px..px + PATCH).contains(&x) || !(pz..pz + PATCH).contains(&z) {
                            continue;
                        }
                        let v = here.entry(k).or_insert_with(|| {
                            vec![0.0; ((PATCH / CELL) * (PATCH / CELL)) as usize]
                        });
                        v[(((z - pz) / CELL) * (PATCH / CELL) + (x - px) / CELL) as usize] += 1.0;
                    }
                    for (k, v) in here {
                        *n.entry(k).or_default() += v.iter().sum::<f64>() as usize;
                        *area.entry(k).or_default() += (PATCH * PATCH) as usize;
                        cells.entry(k).or_default().extend(v);
                    }
                }
            }
        }
        for k in KINDS {
            let count = n.get(k).copied().unwrap_or(0);
            let a = area.get(k).copied().unwrap_or(0);
            let v = cells.get(k).cloned().unwrap_or_default();
            let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
            let var = v.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / v.len().max(1) as f64;
            let per_ha = count as f64 / a.max(1) as f64 * 10_000.0;
            let vmr = var / mean.max(1e-9);
            eprintln!(
                "{:?}: {k}: {count} in {a} m² of the patches holding them ({per_ha:.0} per ha), variance / mean {vmr:.2}",
                f.face
            );
            assert!(count >= 30, "{:?}: {k}: {count} sampled", f.face);
            assert!(
                (5.0..=400.0).contains(&per_ha),
                "{:?}: {k}: {per_ha:.0} per ha",
                f.face
            );
            assert!(
                vmr >= CLUSTERED,
                "{:?}: {k}: not clustered ({vmr:.2})",
                f.face
            );
        }
    }
);
