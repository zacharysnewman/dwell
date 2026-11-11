//! Trees and structures per biome (ADR 0074 B5): a biome lists the placements that stand where it
//! has weight and, in `trees`, their densities (multipliers on the placement's chance); a placement
//! no biome of the world lists stands nowhere. The default world is unchanged (the goldens pin it).

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::config::biome_from_json;
use dwell_worldgen::terrain::{BIOMES, Biome, PRESET_BIOMES, tree_density_of};
use serde_json::{Value, json};

fn file(path: &str) -> Value {
    let p = format!(
        "{}/../../shared/worldgen/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The structures the generator of face `f` (world seed 7) under `biome` alone places in a patch,
/// counted per placement rule name.
fn counts(f: FaceFrame, biome: &Biome) -> std::collections::HashMap<&'static str, usize> {
    let t = BifacialTerrain::height_field(7);
    let g = t.local(f.face).clone().with_biome(biome);
    let mut out = std::collections::HashMap::new();
    for t in g.trees_in((-200, -200), (199, 199)) {
        *out.entry(g.planters()[t.planter as usize].rule().name)
            .or_insert(0) += 1;
    }
    out
}

dwell_world::both_faces!(
    /// The default biome's own trees: the pines, and none of the rules it does not list.
    fn the_default_biome_stands_only_its_own_placements(f: FaceFrame) {
        let c = counts(f, &BIOMES[0]);
        assert!(c.get("pine_groves").copied().unwrap_or(0) > 20, "{c:?}");
        assert_eq!(c.get("broadleaf_groves"), None, "{c:?}");
        assert_eq!(c.get("boulders"), None, "{c:?}");
    }
);

dwell_world::both_faces!(
    /// A biome's `trees` density drives the count: 0 none, ½ about half, 2 more; a rule it does
    /// not list does not stand; one it lists without a density stands at 1 (as the default).
    fn a_biomes_tree_density_drives_its_placements(f: FaceFrame) {
        let with = |trees: &'static [(&'static str, f64)]| Biome {
            placements: &["pine_groves", "boulders"],
            trees,
            ..BIOMES[0]
        };
        let full = counts(f, &with(&[]));
        let (pines, rocks) = (full["pine_groves"], full["boulders"]);
        assert!(pines > 20 && rocks > 20, "{full:?}");
        assert_eq!(counts(f, &with(&[("pine_groves", 1.0)])), full);
        let none = counts(f, &with(&[("pine_groves", 0.0)]));
        assert_eq!(none.get("pine_groves"), None, "{none:?}");
        let half = counts(f, &with(&[("pine_groves", 0.5)]))["pine_groves"];
        assert!(
            (half as f64) > 0.3 * pines as f64 && (half as f64) < 0.7 * pines as f64,
            "{half} of {pines}"
        );
        let more = counts(f, &with(&[("pine_groves", 2.0)]))["pine_groves"];
        // Spacing and the full grid cap it short of twice.
        assert!(more as f64 > 1.15 * pines as f64, "{more} over {pines}");
        let unlisted = counts(
            f,
            &Biome {
                placements: &["boulders"],
                ..BIOMES[0]
            },
        );
        assert_eq!(unlisted.get("pine_groves"), None, "{unlisted:?}");
    }
);

dwell_world::both_faces!(
    /// A world of two biomes: each stands its own rules where it wins (a fixture biome by data
    /// alone, parsed from its JSON as Chisel's edit is: boulders only, dense).
    fn two_biomes_stand_their_own_rules_where_each_has_weight(f: FaceFrame) {
        let mut v = file("biomes/presets/sand_desert.json");
        v["name"] = json!("test:boulder_field");
        v["placements"] = json!(["boulders"]);
        v["trees"] = json!({ "boulders": 3 });
        let fixture = biome_from_json(&v).expect("the fixture parses");
        assert_eq!(tree_density_of(&fixture, "boulders"), 3.0);
        assert_eq!(tree_density_of(&fixture, "pine_groves"), 0.0);
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face).clone().with_biomes(&[BIOMES[0], fixture]);
        let (mut rocks_in_fixture, mut pines_in_fixture) = (0, 0);
        let r = i64::from(WORLD_RADIUS) / 2;
        for x in (-r..r).step_by(1500) {
            for z in (-r..r).step_by(1500) {
                let (x, z) = (x as i32, z as i32);
                let fixture_wins = g.biome_mix(x.into(), z.into()).chosen == 1;
                for t in g.trees_in((x, z), (x + 127, z + 127)) {
                    let name = g.planters()[t.planter as usize].rule().name;
                    let w = g.biome_mix(t.anchor[0].into(), t.anchor[2].into());
                    let fw = w.iter().find(|(i, _)| *i == 1).map_or(0.0, |(_, w)| w);
                    if name == "boulders" {
                        assert!(fw > 0.0, "a boulder where the fixture has no weight");
                    }
                    if fixture_wins && fw > 0.999 {
                        match name {
                            "boulders" => rocks_in_fixture += 1,
                            "pine_groves" => pines_in_fixture += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        assert!(rocks_in_fixture > 0, "the fixture's boulders stand");
        assert_eq!(
            pines_in_fixture, 0,
            "no pines where only the fixture has weight"
        );
    }
);

#[test]
fn a_bad_trees_entry_fails_with_a_message() {
    // both-faces: parsing, no face.
    let base = file("biomes/presets/sand_desert.json");
    let mut v = base.clone();
    v["trees"] = json!({ "pine_groves": 1 });
    let e = biome_from_json(&v).unwrap_err();
    assert!(e.contains("not one of its placements"), "{e}");
    let mut v = base.clone();
    v["trees"] = json!({ "boulders": 9 });
    let e = biome_from_json(&v).unwrap_err();
    assert!(e.contains("trees.boulders must be in [0, 3]"), "{e}");
    let mut v = base;
    v["trees"] = json!([1]);
    assert!(biome_from_json(&v).unwrap_err().contains("trees must map"));
}

#[test]
fn the_presets_pick_their_trees() {
    // both-faces: data only.
    let p = |n: &str| {
        *PRESET_BIOMES
            .iter()
            .find(|b| b.name == format!("preset:{n}"))
            .unwrap()
    };
    assert_eq!(tree_density_of(&p("sand_desert"), "pine_groves"), 0.0);
    assert!(tree_density_of(&p("sand_desert"), "boulders") > 0.0);
    assert!(tree_density_of(&p("red_rock_mesa"), "boulders") > 1.0);
    assert!(tree_density_of(&p("boreal"), "pine_groves") > 1.0);
    assert!(tree_density_of(&p("mixed_forest"), "broadleaf_groves") > 0.0);
    assert!(tree_density_of(&p("tropical_beach"), "broadleaf_groves") > 0.0);
}
