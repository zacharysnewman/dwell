//! BUGS.md #186 (owner, 2026-11-15: "there's some \"sticks\" trees without leaves", "That tree I
//! built seems like it should be handled identically to the others?"): the far tree lists hold the
//! placements' hand-built pines as well as the species trees, so each gets an impostor past the
//! hand-over and owns its leaves (they hand over to it) instead of standing as a bare trunk.

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

/// The bench's `starter:mixed-forest` spot (client/bench/spots.json), where the owner saw them.
const NAME: &str = "test:starter";
const AT: (i32, i32) = (-18100, -6460);

fn generator(f: FaceFrame) -> TerrainGenerator {
    let e = world_config(NAME).unwrap().expect("the starter's edit");
    let map = WorldMap::for_height(face_seed(0, f.face), e.height.expect("its height field"))
        .with_worldgen(NAME)
        .unwrap();
    TerrainGenerator::with_map(face_seed(0, f.face), Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

dwell_world::both_faces!(
    fn every_tree_with_leaves_is_a_far_tree_and_owns_its_leaves(f: FaceFrame) {
        let g = generator(f);
        let (lo, hi) = ((AT.0 - 60, AT.1 - 60), (AT.0 + 60, AT.1 + 60));
        let far: std::collections::HashSet<[i32; 3]> =
            g.far_trees(lo, hi).iter().map(|t| t.anchor).collect();
        let owners: std::collections::HashSet<[i32; 3]> =
            g.tree_tints(lo, hi).iter().map(|t| t.anchor).collect();
        let mut pines = 0;
        for t in g.trees_in(lo, hi) {
            let inside =
                (lo.0..=hi.0).contains(&t.anchor[0]) && (lo.1..=hi.1).contains(&t.anchor[2]);
            let name = g.planters()[t.planter as usize].structure_name(&t);
            let tree = dwell_worldgen::species::find(name).is_some()
                || dwell_worldgen::impostor::structure_tree(name).is_some();
            if !inside || !tree {
                continue;
            }
            pines += usize::from(name.contains("pine_tree"));
            assert!(
                far.contains(&t.anchor),
                "{name} at {:?}: no far tree",
                t.anchor
            );
            assert!(
                owners.contains(&t.anchor),
                "{name} at {:?}: owns no leaves",
                t.anchor
            );
        }
        // The owner's spot is face A's; face B's ground there stands other trees (the rule above
        // still holds for them).
        if !f.is_b() {
            assert!(pines > 0, "the spot has hand-built pines");
        }
    }
);
