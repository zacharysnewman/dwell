//! A test world's worldgen config in the game (ADR 0074 B7): a world made from a Chisel edit (its
//! biomes, biome map, geology, vegetation, erosion and tectonics) generates in the game's generator
//! exactly what Chisel's preview generator does, on both faces; the config travels with the world
//! maps' bytes to every instance that installs them; the build's maps are unchanged.

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::{edit_from_json, worldgen_map, worldgen_map_bytes, worldgen_map_key};
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::{WorldMap, cache, codec};
use serde_json::{Value, json};

fn file(path: &str) -> Value {
    let p = format!(
        "{}/../../shared/worldgen/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// Chisel's red-rock mesa preset beside the default biome, the rock shaping and erosion on, the
/// tectonics convergent with a volcanic arc: every layer an edit can carry.
fn mesa_edit() -> String {
    let mut rocks = file("rocks.json");
    rocks["shaping"]["strength"] = json!(1);
    json!({
        "biomes": [file("biomes/default.json"), file("biomes/presets/red_rock_mesa.json")],
        "geology": rocks,
        "vegetation": file("vegetation.json"),
        "world": { "tectonics": { "kind": 1, "arcCount": 3, "coneHeight": 900 } },
    })
    .to_string()
}

/// Chisel's preview generator of face seed `seed` under `edit` (`web/chisel.rs` `ChiselPreview::edit`).
fn preview(seed: u64, edit: &str) -> TerrainGenerator {
    let e = edit_from_json(edit).unwrap();
    let map = match e.height {
        Some(h) => WorldMap::for_height(seed, h),
        None => WorldMap::compute(seed),
    };
    TerrainGenerator::with_map(seed, Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

#[test]
fn a_world_made_from_an_edit_generates_what_chisel_previews_on_both_faces() {
    // both-faces: each face's game generator is compared with that face's preview.
    let world_seed = 0xC415_E1B7;
    let edit = mesa_edit();
    // The page builds each face's map under the config and every instance installs the bytes.
    for f in [Face::A, Face::B] {
        let bytes = worldgen_map_bytes(face_seed(world_seed, f), &edit).unwrap();
        cache::install(&bytes).unwrap();
    }
    // The game's world is the height field with the terrain's volume (the cliffs) over it; the
    // preview has no volume, so the layers under it are compared (the volume's own carry the
    // config too, below).
    let game = BifacialTerrain::height_field(world_seed);
    let cliffs = BifacialTerrain::new(world_seed);
    let mut mesa_columns = 0;
    let mut rock_columns = 0;
    for f in [Face::A, Face::B] {
        let seed = face_seed(world_seed, f);
        let p = preview(seed, &edit);
        let g = game.local(f);
        assert_eq!(g.biomes().len(), 2, "face {f:?}: the edit's biomes");
        assert_eq!(
            cliffs.local(f).biomes().len(),
            2,
            "face {f:?}: under the cliffs too"
        );
        assert_eq!(
            cliffs.local(f).geology(),
            p.geology(),
            "face {f:?}: the edit's geology"
        );
        assert_eq!(
            g.map().tectonics,
            p.map().tectonics,
            "face {f:?}: tectonics"
        );
        for i in -12..=12 {
            for k in -12..=12 {
                let (x, z) = (i * 211, k * 197);
                let at = format!("face {f:?} at ({x}, {z})");
                let (gm, pm) = (
                    g.biome_mix(x.into(), z.into()),
                    p.biome_mix(x.into(), z.into()),
                );
                assert_eq!(gm.chosen, pm.chosen, "{at}: biome");
                mesa_columns += usize::from(gm.chosen == 1);
                let (gc, pc) = (g.column_at(x, z), p.column_at(x, z));
                assert_eq!(gc, pc, "{at}: column");
                assert_eq!(g.top_state(&gc), p.top_state(&pc), "{at}: top block");
                let rock = g.rock_at_surface(x.into(), z.into());
                assert_eq!(rock, p.rock_at_surface(x.into(), z.into()), "{at}: rock");
                rock_columns += usize::from(rock.0.name != "dwell:sandstone");
                assert_eq!(
                    g.erosion_at(x.into(), z.into()),
                    p.erosion_at(x.into(), z.into()),
                    "{at}: erosion"
                );
                assert_eq!(
                    g.vegetation_at(x.into(), z.into()),
                    p.vegetation_at(x.into(), z.into()),
                    "{at}: vegetation"
                );
            }
        }
        let area = ((-600, -600), (600, 600));
        assert_eq!(
            g.trees_in(area.0, area.1),
            p.trees_in(area.0, area.1),
            "face {f:?}: trees"
        );
    }
    assert!(
        mesa_columns > 0,
        "the mesa biome is somewhere in the sample"
    );
    assert!(rock_columns > 0, "more than one rock shows");
}

#[test]
fn the_config_round_trips_with_the_map_and_the_builds_maps_are_unchanged() {
    // face-a-only: the codec's trailer is the same for either face's map.
    let seed = 0xB7_0001;
    let edit = mesa_edit();
    let m = worldgen_map(seed, &edit).unwrap().unwrap();
    let back = codec::decode(&codec::encode(&m)).unwrap();
    assert_eq!(back.worldgen_text(), edit.trim());
    assert_eq!(back.tectonics, m.tectonics);
    assert_eq!(back.tectonics.kind, dwell_worldgen::tectonics::CONVERGENT);
    assert_eq!(back.digest(), m.digest());
    // The build's ground: no config, no trailer, its own key.
    assert!(worldgen_map(seed, "").unwrap().is_none());
    assert_eq!(worldgen_map_key(seed, ""), cache::key(seed));
    let plain = WorldMap::compute(seed);
    assert_eq!(plain.worldgen_text(), "");
    assert_eq!(
        codec::encode(&plain),
        codec::encode(&codec::decode(&codec::encode(&plain)).unwrap())
    );
    // A config keys its maps apart, a fixture's name too.
    assert_ne!(worldgen_map_key(seed, &edit), cache::key(seed));
    assert_ne!(worldgen_map_key(seed, "test:tall_peak"), cache::key(seed));
    assert!(worldgen_map(seed, "test:tall_peak").unwrap().is_some());
}

#[test]
fn installing_a_test_worlds_map_replaces_the_seeds_own_in_an_instance() {
    // face-a-only: the process's map slots are per face seed, the same for either face.
    // An instance that built the seed's own map first (BUGS.md #68's lead) takes the test world's
    // when its bytes arrive, and back.
    let seed = 0xB7_0002;
    let own = cache::for_seed(seed);
    assert_eq!(own.worldgen_text(), "");
    let edit = mesa_edit();
    cache::install(&worldgen_map_bytes(seed, &edit).unwrap()).unwrap();
    let g = TerrainGenerator::new(seed);
    assert_eq!(g.map().worldgen_text(), edit.trim());
    assert_eq!(g.biomes().len(), 2);
    cache::install(&codec::encode(&own)).unwrap();
    assert_eq!(TerrainGenerator::new(seed).map().worldgen_text(), "");
}
