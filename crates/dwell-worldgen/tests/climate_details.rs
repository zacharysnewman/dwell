//! Climate details in ordinary biomes (systemic terrain T3b, ADR 0075): a biome's own snow line from
//! temperature (`snow`), its ground's blocks by soil type (`soilBlocks`, `vegetation.json`
//! `soilTypes`' `ground`/`under`), and its tall grass and ground items scaled by the climate
//! (`undergrowth`, vegetation kinds with a `decoration` or `ground`). Each is opt-in per biome: the
//! default biome has none, so the default world is unchanged (the goldens).

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::config::{biome_from_json, vegetation_from_json};
use dwell_worldgen::terrain::{BIOMES, Band, Biome, TerrainGenerator, Water};
use dwell_worldgen::vegetation::{Kind, SoilType, VEGETATION, Vegetation};
use serde_json::{Value, json};

fn file(path: &str) -> Value {
    let p = format!(
        "{}/../../shared/worldgen/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// A fixture biome by data alone: the default biome's file, renamed, its `where` weighted to rule
/// most columns, with `extra`'s keys over it.
fn fixture_biome(extra: Value) -> Biome {
    let mut v = file("biomes/default.json");
    v["name"] = json!("test:climate");
    v["where"] = json!({ "weight": 40 });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    biome_from_json(&v).unwrap()
}

/// Seed 7's island with the default biome and the fixture (index 1).
fn world(f: FaceFrame, fixture: Biome) -> TerrainGenerator {
    let t = BifacialTerrain::height_field(7);
    t.local(f.face).clone().with_biomes(&[BIOMES[0], fixture])
}

/// Land columns over the island every `step` m whose biome is the fixture: (x, z, column).
fn fixture_columns(
    g: &TerrainGenerator,
    step: usize,
) -> Vec<(i32, i32, dwell_worldgen::terrain::Column)> {
    let r = WORLD_RADIUS * 8 / 10;
    let mut out = Vec::new();
    for x in (-r..=r).step_by(step) {
        for z in (-r..=r).step_by(step) {
            let c = g.column_at(x, z);
            if c.outside || c.water != Water::None || c.band == Band::Shore {
                continue;
            }
            if g.biome_mix(x.into(), z.into()).chosen == 1 {
                out.push((x, z, c));
            }
        }
    }
    out
}

fn top_name(g: &TerrainGenerator, c: &dwell_worldgen::terrain::Column) -> &'static str {
    dwell_blocks::block_of(g.top_state(c)).name
}

/// The landform's slope (rise per run) at (x, z): the world map's, as the snow line reads it.
fn slope(g: &TerrainGenerator, x: i32, z: i32) -> f64 {
    let (_, gx, gz) = g.map().height_at(f64::from(x), f64::from(z));
    (gx * gx + gz * gz).sqrt()
}

dwell_world::both_faces!(
    /// A biome's own snow line (T3b): with the line at the island's median °C, gentle ground
    /// clearly colder than it is snow, ground clearly warmer has none, and cold steep faces are
    /// bare. The world (seed 7's default height field) has no snow line of its own.
    fn a_biome_snow_line_covers_cold_gentle_ground_only(f: FaceFrame) {
        let plain = world(f, fixture_biome(json!({})));
        let celsius =
            |g: &TerrainGenerator, x: i32, z: i32, c: &dwell_worldgen::terrain::Column| {
                g.celsius_at(x.into(), z.into(), f64::from(c.height) - SEA_LEVEL_M)
            };
        let mut temps: Vec<f64> = fixture_columns(&plain, 211)
            .iter()
            .map(|(x, z, c)| celsius(&plain, *x, *z, c))
            .collect();
        assert!(temps.len() > 200, "{} fixture columns", temps.len());
        temps.sort_by(f64::total_cmp);
        let line = temps[temps.len() / 2];
        // No snow anywhere without the biome's line.
        assert!(
            fixture_columns(&plain, 211)
                .iter()
                .all(|(_, _, c)| top_name(&plain, c) != "dwell:snow"),
            "snow without a snow line"
        );
        let max_slope = 0.15;
        let g = world(
            f,
            fixture_biome(json!({
                "snow": { "belowC": line, "maxSlope": max_slope, "block": "dwell:snow" }
            })),
        );
        let (mut cold, mut snowy, mut warm, mut steep) = (0, 0, 0, 0);
        for (x, z, c) in fixture_columns(&g, 211) {
            let t = celsius(&g, x, z, &c);
            let s = slope(&g, x, z);
            let snow = top_name(&g, &c) == "dwell:snow";
            if t > line + 0.3 {
                warm += 1;
                assert!(!snow, "snow at {t:.1} °C over a line at {line:.1} °C");
            } else if t < line - 0.3 && s > 3.0 * max_slope {
                steep += 1;
                assert!(!snow, "snow on a face {s:.2} steep");
            } else if t < line - 0.3 && s < 0.3 * max_slope {
                cold += 1;
                snowy += usize::from(snow);
            }
        }
        println!(
            "line {line:.1} °C: {snowy} of {cold} cold gentle snowy, {warm} warm, {steep} steep"
        );
        assert!(warm > 50 && cold > 50, "{warm} warm, {cold} cold gentle");
        assert!(steep > 5, "{steep} cold steep columns");
        assert!(
            snowy * 10 >= cold * 9,
            "snow on {snowy} of {cold} cold gentle columns"
        );
    }
);

dwell_world::both_faces!(
    /// A biome's glacier (T3b): colder than its `iceBelowC` gentle ground is the ice block, between
    /// the two lines the snow.
    fn a_biome_glacier_lies_below_its_ice_line(f: FaceFrame) {
        let plain = world(f, fixture_biome(json!({})));
        let mut temps: Vec<f64> = fixture_columns(&plain, 211)
            .iter()
            .map(|(x, z, c)| {
                plain.celsius_at((*x).into(), (*z).into(), f64::from(c.height) - SEA_LEVEL_M)
            })
            .collect();
        temps.sort_by(f64::total_cmp);
        let (ice, line) = (temps[temps.len() / 4], temps[temps.len() * 3 / 4]);
        let g = world(
            f,
            fixture_biome(json!({ "snow": {
                "belowC": line, "maxSlope": 10, "block": "dwell:snow",
                "iceBelowC": ice, "ice": "dwell:glacier_ice"
            }})),
        );
        let (mut iced, mut snowed) = (0, 0);
        for (x, z, c) in fixture_columns(&g, 211) {
            let t = g.celsius_at(x.into(), z.into(), f64::from(c.height) - SEA_LEVEL_M);
            match top_name(&g, &c) {
                "dwell:glacier_ice" => {
                    iced += 1;
                    assert!(t < ice + 0.3, "ice at {t:.1} °C over its line {ice:.1}");
                }
                "dwell:snow" => {
                    snowed += 1;
                    assert!(t > ice - 0.3 && t < line + 0.3, "snow at {t:.1} °C");
                }
                _ => {}
            }
        }
        assert!(iced > 20 && snowed > 20, "{iced} ice, {snowed} snow");
    }
);

/// The build's vegetation with soil types' blocks and one fixture kind (fixtures by data alone).
fn vegetation(soil: &[(&str, &'static str)], kind: Option<Kind>) -> Vegetation {
    let types: Vec<SoilType> = VEGETATION
        .soil_types
        .iter()
        .map(|t| SoilType {
            under: soil
                .iter()
                .find(|(n, _)| *n == t.name)
                .map_or("", |(_, b)| *b),
            ..*t
        })
        .collect();
    Vegetation {
        soil_types: Box::leak(types.into_boxed_slice()),
        kinds: match kind {
            Some(k) => Box::leak(Box::new([k])),
            None => VEGETATION.kinds,
        },
        ..VEGETATION
    }
}

dwell_world::both_faces!(
    /// A biome's soil blocks (T3b): with `soilBlocks`, the block under its grass is its soil
    /// type's (by the rock at the surface): sand on sandy soil, stone on rocky, the band's dirt on
    /// loamy (no block named); without it, the band's dirt everywhere.
    fn soil_types_pick_the_blocks_under_the_grass(f: FaceFrame) {
        let veg = vegetation(
            &[
                ("dwell:sandy", "dwell:sand"),
                ("dwell:rocky", "dwell:stone"),
            ],
            None,
        );
        let names = |g: &TerrainGenerator| {
            let mut seen = std::collections::BTreeMap::new();
            for (x, z, c) in fixture_columns(g, 173) {
                if c.band != Band::Ground || top_name(g, &c) != "dwell:grass" {
                    continue;
                }
                let top = g.ground_y(x, z).unwrap();
                let under = dwell_blocks::block_of(g.state_at(x, top - 1, z)).name;
                let rock = g.rock_at_surface(x.into(), z.into()).0;
                let soil = veg.soil_types
                    [usize::from(veg.rock_soil(rock.name, f64::NAN, f64::NAN).soil_type)]
                .name;
                *seen.entry((soil, under)).or_insert(0) += 1;
            }
            seen
        };
        let off = names(&world(f, fixture_biome(json!({}))).with_vegetation(veg));
        println!("without: {off:?}");
        assert!(off.keys().all(|(_, u)| *u == "dwell:dirt"), "{off:?}");
        let on =
            names(&world(f, fixture_biome(json!({ "soilBlocks": true }))).with_vegetation(veg));
        println!("with: {on:?}");
        let want = |soil: &str| match soil {
            "dwell:sandy" => "dwell:sand",
            "dwell:rocky" => "dwell:stone",
            _ => "dwell:dirt",
        };
        for ((soil, under), n) in &on {
            assert_eq!(*under, want(soil), "{n} columns of {soil} over {under}");
        }
        let types = on
            .keys()
            .map(|(s, _)| *s)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            types.len() >= 2,
            "more than one soil type on the island: {types:?}"
        );
    }
);

/// A kind with only a moisture floor `wet` (no fade, any temperature, soil or slope).
fn wet_kind(target: &str, wet: f64) -> Kind {
    let k = Kind {
        name: "test:wet",
        placement: "",
        decoration: "",
        ground: "",
        temperature: [-80.0, 80.0],
        moisture: [wet, 1.0],
        min_soil: 0.0,
        max_slope_deg: 90.0,
        soils: &[],
        fade: [0.0, 0.0, 0.0],
        density: 1.0,
    };
    if target == "meadow_grass" {
        Kind {
            decoration: "meadow_grass",
            ..k
        }
    } else {
        Kind {
            ground: Box::leak(target.to_owned().into_boxed_str()),
            ..k
        }
    }
}

/// The median moisture of the fixture's wet columns (most of the island is dry: wetness 0 away
/// from the rivers without the rain shadow).
fn median_moisture(g: &TerrainGenerator) -> f64 {
    let mut m: Vec<f64> = fixture_columns(g, 211)
        .iter()
        .filter_map(|(x, z, _)| g.climate_at((*x).into(), (*z).into()))
        .map(|c| c.moisture)
        .filter(|&m| m > 0.0)
        .collect();
    assert!(m.len() > 30, "{} wet columns", m.len());
    m.sort_by(f64::total_cmp);
    m[m.len() / 2]
}

/// The tall grass in chunks round (x, z): each one's (x, z).
fn tall_grass_round(g: &TerrainGenerator, (x, z): (i32, i32)) -> Vec<(i32, i32)> {
    let grass = dwell_blocks::require("dwell:tall_grass");
    let mut out = Vec::new();
    for cz in -2..2 {
        for cx in -2..2 {
            let (wx, wz) = (x.div_euclid(CHUNK_SIZE) + cx, z.div_euclid(CHUNK_SIZE) + cz);
            let h = f64::from(g.column_at(wx * CHUNK_SIZE, wz * CHUNK_SIZE).height);
            let row = rows_for_m(h).div_euclid(CHUNK_SIZE);
            for cy in row - 1..=row + 1 {
                let mut c = Chunk::default();
                let coord = ChunkCoord::new(wx, cy, wz);
                g.generate(coord, &mut c);
                for lz in 0..CHUNK_SIZE {
                    for lx in 0..CHUNK_SIZE {
                        for ly in 0..CHUNK_SIZE {
                            if c.get(lx, ly, lz) == grass {
                                out.push((wx * CHUNK_SIZE + lx, wz * CHUNK_SIZE + lz));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

dwell_world::both_faces!(
    /// Tall grass follows the moisture (T3b): with `undergrowth` 1 and a fixture kind over
    /// `meadow_grass` with a moisture floor at the island's median, no tall grass stands where it
    /// is drier, some where it is wetter, and fewer than without the climate.
    fn tall_grass_follows_the_moisture(f: FaceFrame) {
        let free = world(f, fixture_biome(json!({})));
        let wet = median_moisture(&free);
        let g = world(f, fixture_biome(json!({ "undergrowth": 1 })))
            .with_vegetation(vegetation(&[], Some(wet_kind("meadow_grass", wet))));
        // Spots over the fixture's land: four wetter than the floor and four drier.
        let cols = fixture_columns(&free, 211);
        let moist = |x: i32, z: i32| free.climate_at(x.into(), z.into()).unwrap().moisture;
        let pick = |wetter: bool| {
            let v: Vec<(i32, i32)> = cols
                .iter()
                .map(|(x, z, _)| (*x, *z))
                .filter(|&(x, z)| (moist(x, z) >= wet) == wetter)
                .collect();
            (0..4).map(|k| v[k * v.len() / 4]).collect::<Vec<_>>()
        };
        let spots: Vec<(i32, i32)> = pick(true).into_iter().chain(pick(false)).collect();
        let (mut kept, mut all) = (0, 0);
        for &s in &spots {
            all += tall_grass_round(&free, s).len();
            for (x, z) in tall_grass_round(&g, s) {
                kept += 1;
                if g.biome_mix(x.into(), z.into()).chosen != 1 {
                    continue;
                }
                let m = g.climate_at(x.into(), z.into()).unwrap().moisture;
                assert!(
                    m >= wet,
                    "tall grass at wetness {m:.3} under the floor {wet:.3}"
                );
            }
        }
        println!("tall grass: {kept} with the climate of {all} without (floor {wet:.3})");
        assert!(kept > 0 && kept < all, "{kept} of {all}");
    }
);

dwell_world::both_faces!(
    /// Ground items follow the climate (T3b): with `undergrowth` 1 and a fixture kind over the
    /// sticks with a moisture floor, the sticks' density is 0 in the fixture's dry columns and 1
    /// in its wet ones; the rocks (no kind governs them) stay 1; without `undergrowth` nothing varies.
    fn ground_items_follow_the_climate(f: FaceFrame) {
        let free = world(f, fixture_biome(json!({})));
        assert!(
            !free.ground_varies(),
            "no ground or undergrowth: nothing varies"
        );
        let wet = median_moisture(&free);
        let veg = vegetation(&[], Some(wet_kind("dwell:ground_stick", wet)));
        let g = world(f, fixture_biome(json!({ "undergrowth": 1 }))).with_vegetation(veg);
        assert!(g.ground_varies());
        let (mut dry, mut damp) = (0, 0);
        for (x, z, _) in fixture_columns(&g, 211) {
            let (x, z) = (i64::from(x), i64::from(z));
            if g.biome_mix(x, z).iter().any(|(i, w)| i == 1 && w > 0.999) {
                let m = g.climate_at(x, z).unwrap().moisture;
                let stick = g.ground_density_at("dwell:ground_stick", x, z);
                if m < wet {
                    dry += 1;
                    assert!(stick < 1e-3, "sticks {stick} at wetness {m:.3}");
                } else {
                    damp += 1;
                    assert!(
                        (stick - 1.0).abs() < 1e-3,
                        "sticks {stick} at wetness {m:.3}"
                    );
                }
                assert!((g.ground_density_at("dwell:ground_rock", x, z) - 1.0).abs() < 1e-3);
            }
        }
        assert!(dry > 30 && damp > 30, "{dry} dry, {damp} damp");
    }
);

#[test]
fn bad_climate_details_fail_with_a_message() {
    let bad = |extra: Value, msg: &str| {
        let mut v = file("biomes/default.json");
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        let e = biome_from_json(&v).unwrap_err();
        assert!(e.contains(msg), "{e}");
    };
    bad(
        json!({ "snow": { "belowC": 0, "maxSlope": 1, "block": "dwell:nothing" } }),
        "snow.block dwell:nothing is not in the registry",
    );
    bad(
        json!({ "snow": { "belowC": 99, "maxSlope": 1, "block": "dwell:snow" } }),
        "snow.belowC must be in",
    );
    bad(
        json!({ "snow": { "belowC": 0, "maxSlope": 1, "block": "dwell:snow", "iceBelowC": 2, "ice": "dwell:glacier_ice" } }),
        "snow.iceBelowC must be colder than snow.belowC",
    );
    bad(
        json!({ "snow": { "belowC": 0, "maxSlope": 1, "block": "dwell:snow", "ice": "dwell:glacier_ice" } }),
        "ice and iceBelowC go together",
    );
    let veg = |f: &dyn Fn(&mut Value), msg: &str| {
        let mut v = file("vegetation.json");
        f(&mut v);
        let e = vegetation_from_json(&v).unwrap_err();
        assert!(e.contains(msg), "{e}");
    };
    veg(
        &|v| v["soilTypes"][0]["under"] = json!("dwell:nothing"),
        "soil type dwell:sandy: under dwell:nothing is not in the registry",
    );
    veg(
        &|v| v["kinds"][0]["decoration"] = json!("meadow_grass"),
        "must govern exactly one of placement, decoration or ground",
    );
    veg(
        &|v| {
            let k = v["kinds"].as_array_mut().unwrap();
            let mut x = k[0].clone();
            x.as_object_mut().unwrap().remove("placement");
            x["decoration"] = json!("nothing");
            k.push(x);
        },
        "decoration nothing is not a rule of decorations.json",
    );
}

#[test]
fn the_build_and_the_default_biome_opt_in_to_nothing() {
    // both-faces: data only, no face.
    for b in BIOMES {
        assert!(
            b.snow.is_none() && !b.soil_blocks && b.undergrowth == 0.0,
            "{}",
            b.name
        );
    }
    // The checked-in kinds: the meadow grass follows the moisture.
    let v = VEGETATION;
    let k = v
        .kind_for_decoration("meadow_grass")
        .expect("a kind over the meadow grass");
    let c = dwell_worldgen::vegetation::Climate {
        temperature_c: 12.0,
        moisture: 0.8,
        soil_depth: 1.0,
        soil_type: 1,
        slope_deg: 5.0,
        strength: 1.0,
    };
    let dry = dwell_worldgen::vegetation::Climate { moisture: 0.0, ..c };
    assert!(v.density(k, &dry) < v.density(k, &c));
    assert!(v.kind_for_ground("dwell:ground_stick").is_some());
    assert_eq!(
        v.kind_for(""),
        None,
        "a decoration's kind is no placement's"
    );
}
