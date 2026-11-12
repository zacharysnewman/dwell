//! The tree performance reference world (WORK_QUEUE item 32): `test:forest`, gentle land that is
//! the old-growth forest (`preset:old_growth`) everywhere, its trees the old-growth species
//! (`dwell:old_pine`, `dwell:old_oak`) standing close. Deterministic, at the intended density (trees
//! per hectare in a range), tall, and the biome's tree density data alone changes the stand.
//! The bench's `forest-perf` scenario and the `forest:*` spots stand in it (docs/DEV_TOOLS.md).

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::{Edit, world_config};
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:forest";
/// The bench spots' seed (client/bench/spots.json `forest:*`).
const SEED: u64 = 0;
/// The square the density is counted on: the bench's stand (spots.json `forest:inside`), 200 m a side.
const STAND: (i32, i32) = (0, 0);
const HALF: i32 = 100;
/// The intended density (trees per hectare; the old-growth placement's grid 5, its crowns
/// interleaving 2 blocks, stands ~220): a temperate rainforest's few hundred stems.
const PER_HA: std::ops::RangeInclusive<f64> = 170.0..=280.0;

fn edit() -> Edit {
    world_config(NAME)
        .unwrap()
        .expect("the forest world's edit")
}

fn generator(e: &Edit, f: FaceFrame, seed: u64) -> TerrainGenerator {
    let map = WorldMap::for_height(face_seed(seed, f.face), e.height.expect("its height field"))
        .with_worldgen(NAME)
        .unwrap();
    TerrainGenerator::with_map(face_seed(seed, f.face), Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

/// The trees anchored in the stand's square: (anchor, species).
fn stand(g: &TerrainGenerator) -> Vec<([i32; 3], &'static str)> {
    let (lo, hi) = (
        (STAND.0 - HALF, STAND.1 - HALF),
        (STAND.0 + HALF - 1, STAND.1 + HALF - 1),
    );
    let mut out: Vec<_> = g
        .trees_in(lo, hi)
        .iter()
        .filter(|t| (lo.0..=hi.0).contains(&t.anchor[0]) && (lo.1..=hi.1).contains(&t.anchor[2]))
        .map(|t| (t.anchor, g.planters()[t.planter as usize].structure_name(t)))
        .collect();
    out.sort_unstable();
    out
}

fn hectares() -> f64 {
    f64::from(2 * HALF) * f64::from(2 * HALF) / 10_000.0
}

#[test]
fn the_forest_world_is_old_growth_and_nothing_else() {
    // face-a-only: parses the data, no terrain.
    let e = edit();
    let names: Vec<_> = e.biomes.iter().map(|b| b.name).collect();
    assert_eq!(names, ["dwell:default", "preset:old_growth"]);
    let og = &e.biomes[1];
    assert_eq!(og.placements, &["old_growth"]);
    assert!(og.decorations.contains(&"old_growth_undergrowth"));
}

dwell_world::both_faces!(
    /// Every land column of the stand is the old-growth biome (the default's weight is a token).
    fn every_column_is_old_growth(f: FaceFrame) {
        let e = edit();
        let g = generator(&e, f, SEED);
        let og = e
            .biomes
            .iter()
            .position(|b| b.name == "preset:old_growth")
            .unwrap();
        for z in (-400..=400).step_by(20) {
            for x in (-400..=400).step_by(20) {
                let m = g.biome_mix(x, z);
                assert_eq!(m.chosen as usize, og, "column ({x}, {z})");
            }
        }
    }
);

dwell_world::both_faces!(
    /// The stand at the intended density, both species, the same from two generators (deterministic),
    /// and its pines tall: the old-growth planter reaches over 20 m up.
    fn the_stand_is_dense_tall_and_deterministic(f: FaceFrame) {
        let e = edit();
        let (a, b) = (generator(&e, f, SEED), generator(&e, f, SEED));
        let trees = stand(&a);
        assert_eq!(trees, stand(&b), "deterministic");
        let per_ha = trees.len() as f64 / hectares();
        let pines = trees.iter().filter(|t| t.1 == "dwell:old_pine").count();
        let oaks = trees.iter().filter(|t| t.1 == "dwell:old_oak").count();
        println!(
            "{:?}: {per_ha:.0} trees/ha ({pines} pines, {oaks} broadleaf)",
            f.face
        );
        assert!(
            PER_HA.contains(&per_ha),
            "{per_ha:.0} trees/ha, want {PER_HA:?}"
        );
        assert!(pines > oaks && oaks > 0, "{pines} pines, {oaks} broadleaf");
        let p = a
            .planters()
            .iter()
            .find(|p| p.rule().name == "old_growth")
            .expect("the old-growth planter");
        assert!(
            p.rise() >= dwell_world::rows_for_m(20.0),
            "the tallest old-growth tree rises {} rows",
            p.rise()
        );
    }
);

dwell_world::both_faces!(
    /// The biome's `trees` density is data: halving it by an edit alone stands fewer trees.
    fn the_biomes_tree_density_is_data(f: FaceFrame) {
        let base = edit();
        let mut thin = edit();
        thin.biomes[1].trees = &[("old_growth", 0.4)];
        let (full, sparse) = (
            stand(&generator(&base, f, SEED)).len(),
            stand(&generator(&thin, f, SEED)).len(),
        );
        println!("{:?}: {full} trees at density 1, {sparse} at 0.4", f.face);
        assert!(sparse * 10 < full * 8, "{sparse} vs {full}");
    }
);

dwell_world::both_faces!(
    /// The far tree list (impostors' I-2): the stand's species trees, each once, and a split of the
    /// square into four lists the same trees (no tree in two sections, none lost at a border).
    fn the_far_tree_lists_hold_each_stand_tree_once(f: FaceFrame) {
        let e = edit();
        let g = generator(&e, f, SEED);
        let (lo, hi) = (
            (STAND.0 - HALF, STAND.1 - HALF),
            (STAND.0 + HALF - 1, STAND.1 + HALF - 1),
        );
        let all = g.far_trees(lo, hi);
        let placed = stand(&g);
        assert_eq!(all.len(), placed.len(), "every stand tree is a species tree");
        let anchors: Vec<_> = all.iter().map(|t| t.anchor).collect();
        let want: Vec<_> = placed.iter().map(|p| p.0).collect();
        assert_eq!(anchors, want);
        let (mx, mz) = STAND;
        let mut parts: Vec<_> = [
            (lo, (mx - 1, mz - 1)),
            ((mx, lo.1), (hi.0, mz - 1)),
            ((lo.0, mz), (mx - 1, hi.1)),
            ((mx, mz), hi),
        ]
        .into_iter()
        .flat_map(|(a, b)| g.far_trees(a, b))
        .collect();
        parts.sort_unstable();
        assert_eq!(parts, all, "the four quarters' lists are the square's");
        // Instances: one each, on the baked layers, at the tree's foot.
        let c = dwell_worldgen::impostor::Config::get();
        let inst = c.instances(&all);
        let layers = c.layers_per_species() * dwell_worldgen::species::SPECIES.len() as u32;
        assert_eq!(inst.len(), all.len());
        for (t, i) in all.iter().zip(&inst) {
            assert!(i.layer < layers, "{t:?}");
            assert!(i.quarter_turns < 4);
            assert_eq!(i.foot[0], t.anchor[0] as f32 + 0.5);
        }
    }
);

/// The bench spots' places (client/bench/spots.json `forest:*`): the spawn, the densest 40 m square
/// near the centre and the highest ground in view of it, printed (`cargo test --release -p
/// dwell-worldgen --test forest_world spots -- --ignored --nocapture`).
#[test]
#[ignore]
fn spots() {
    // face-a-only: a measurement for the bench's face-A spots.
    let e = edit();
    let g = generator(&e, FaceFrame::A, SEED);
    println!("spawn {:?}", g.spawn_point());
    let trees = g.trees_in((-300, -300), (299, 299));
    let mut best = (0, 0, 0);
    for z in (-280..=240).step_by(10) {
        for x in (-280..=240).step_by(10) {
            let n = trees
                .iter()
                .filter(|t| {
                    (x..x + 40).contains(&t.anchor[0]) && (z..z + 40).contains(&t.anchor[2])
                })
                .count();
            if n > best.2 {
                best = (x + 20, z + 20, n);
            }
        }
    }
    println!(
        "densest 40 m square centred ({}, {}): {} trees",
        best.0, best.1, best.2
    );
    let mut high = (0, 0, f64::MIN);
    let mut low = (0, 0, f64::MAX);
    for z in (-600..=600).step_by(8) {
        for x in (-600..=600).step_by(8) {
            if let Some(y) = g.surface_y_at(x, z) {
                if y > high.2 {
                    high = (x, z, y);
                }
                if y < low.2 {
                    low = (x, z, y);
                }
            }
        }
    }
    println!("highest ground {high:?}, lowest {low:?}");
    for (x, z) in [(best.0, best.1), (high.0, high.1), (0, 0)] {
        println!("surface at ({x}, {z}): {:?}", g.surface_y_at(x, z));
    }
}
