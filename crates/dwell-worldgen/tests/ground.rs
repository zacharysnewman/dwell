//! Ground items per biome (ADR 0074 B5): a biome's `ground` multiplies each ground item kind's
//! density where it has weight (a kind it does not name lies nowhere it rules; no `ground`, every
//! kind at 1). The generator hands each chunk column its densities at its corners
//! (`GroundDensity`), which the server and the client's mesh jobs both scatter with. The default
//! world asks nothing of it: its scatter is as before.

use dwell_world::ground::{GroundDensity, KINDS};
use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::config::biome_from_json;
use dwell_worldgen::terrain::{BIOMES, PRESET_BIOMES, ground_density_of};
use serde_json::{Value, json};

fn file(path: &str) -> Value {
    let p = format!(
        "{}/../../shared/worldgen/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn kind(name: &str) -> usize {
    KINDS.iter().position(|k| k.name == name).expect(name)
}

dwell_world::both_faces!(
    /// The default biome has no `ground`: nothing varies and every chunk's densities are empty
    /// (1 everywhere), so the default world's scatter is unchanged.
    fn the_default_world_leaves_the_ground_items_as_they_were(f: FaceFrame) {
        assert!(BIOMES.iter().all(|b| b.ground.is_none()));
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face);
        assert!(!g.ground_varies());
        assert_eq!(g.ground_density_chunk(3, -2), GroundDensity::default());
        assert_eq!(g.ground_density_at("dwell:ground_stick", 100, 100), 1.0);
    }
);

dwell_world::both_faces!(
    /// A world of two biomes, the second a fixture by data alone (parsed from its JSON as
    /// Chisel's edit is: rocks ×3, no sticks): where it alone has weight the chunk's corners read
    /// its densities, where the default alone has weight 1.
    fn two_biomes_give_each_chunk_its_own_ground_densities(f: FaceFrame) {
        let mut v = file("biomes/presets/sand_desert.json");
        v["name"] = json!("test:rock_field");
        v["ground"] = json!({ "dwell:ground_rock": 3 });
        let fixture = biome_from_json(&v).expect("the fixture parses");
        assert_eq!(ground_density_of(&fixture, "dwell:ground_rock"), 3.0);
        assert_eq!(ground_density_of(&fixture, "dwell:ground_stick"), 0.0);
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face).clone().with_biomes(&[BIOMES[0], fixture]);
        assert!(g.ground_varies());
        let (stick, rock) = (kind("dwell:ground_stick"), kind("dwell:ground_rock"));
        let (mut fixture_seen, mut default_seen) = (0, 0);
        let r = i64::from(WORLD_RADIUS) / 2;
        let n = i64::from(CHUNK_SIZE);
        for x in (-r..r).step_by(1500) {
            for z in (-r..r).step_by(1500) {
                let (cx, cz) = ((x / n) as i32, (z / n) as i32);
                let d = g.ground_density_chunk(cx, cz);
                assert_eq!(d.corners.len(), KINDS.len());
                let at = |x: i64, z: i64| {
                    let w = g.biome_mix(x, z);
                    w.iter().find(|(i, _)| *i == 1).map_or(0.0, |(_, w)| w)
                };
                let (x0, z0) = (i64::from(cx) * n, i64::from(cz) * n);
                let corners = [(x0, z0), (x0 + n, z0), (x0, z0 + n), (x0 + n, z0 + n)];
                let fw: Vec<f64> = corners.iter().map(|&(x, z)| at(x, z)).collect();
                if fw.iter().all(|&w| w > 0.999) {
                    fixture_seen += 1;
                    assert!(d.corners[stick].iter().all(|&v| v < 0.01), "{d:?}");
                    assert!(d.corners[rock].iter().all(|&v| v > 2.99), "{d:?}");
                } else if fw.iter().all(|&w| w < 0.001) {
                    default_seen += 1;
                    let near_1 = d.corners.iter().flatten().all(|v| (v - 1.0).abs() < 0.01);
                    assert!(near_1, "{d:?}");
                }
            }
        }
        assert!(fixture_seen > 0 && default_seen > 0, "{fixture_seen} {default_seen}");
    }
);

#[test]
fn a_bad_ground_entry_fails_with_a_message() {
    // both-faces: parsing, no face.
    let base = file("biomes/presets/sand_desert.json");
    let mut v = base.clone();
    v["ground"] = json!({ "dwell:ground_shell": 1 });
    let e = biome_from_json(&v).unwrap_err();
    assert!(e.contains("is not a kind of ground.json"), "{e}");
    let mut v = base.clone();
    v["ground"] = json!({ "dwell:ground_rock": 4 });
    let e = biome_from_json(&v).unwrap_err();
    assert!(e.contains("ground.dwell:ground_rock must be in [0, 3]"), "{e}");
    let mut v = base;
    v["ground"] = json!([1]);
    assert!(biome_from_json(&v).unwrap_err().contains("ground must map"));
}

#[test]
fn the_presets_pick_their_ground_items() {
    // both-faces: data only.
    let p = |n: &str| {
        *PRESET_BIOMES
            .iter()
            .find(|b| b.name == format!("preset:{n}"))
            .unwrap()
    };
    let (stick, rock) = ("dwell:ground_stick", "dwell:ground_rock");
    for desert in ["sand_desert", "red_rock_mesa", "badlands"] {
        assert_eq!(ground_density_of(&p(desert), stick), 0.0, "{desert}");
        assert!(ground_density_of(&p(desert), rock) > 1.0, "{desert}");
    }
    for forest in ["boreal", "mixed_forest", "cold_rainforest"] {
        assert!(ground_density_of(&p(forest), stick) > 1.0, "{forest}");
    }
    // A preset without `ground` keeps every kind at 1.
    assert_eq!(ground_density_of(&p("temperate_meadow"), stick), 1.0);
}
