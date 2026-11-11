//! Chisel's preview (ADR 0074 B3; the biome editor page, `client/chisel.html`): biomes and the
//! biome map's geography parsed from their JSON (the files of `shared/worldgen/biomes` and
//! `terrain.json`'s `biomeMap`, already checked by `shared/worldgen/biomes.mjs`, the rules gen.mjs
//! uses), put on the real generator (`TerrainGenerator::with_biomes_on`), and a top-down tile of a
//! region rasterised by the big map's own rules (`lod_map::raster_with_heights`, tinted by height)
//! or as an overlay of the biome weights, the temperature or the wetness.
//!
//! The JSON → [`Biome`] mapping is gen.mjs's `biomeFields` (its Rust tables); a test parses every
//! checked-in biome and fixture and compares it with the generated tables, so the two agree.

use dwell_game::minimap::{MAP, decode_height, tint_rgb};
use dwell_look::lod_map::LOD_MAP;
use dwell_worldgen::terrain::{BIOMES, TerrainGenerator, water_tint_of};

use crate::lod_map::{Rect, raster_with_heights};

/// The edits' parser (ADR 0074 B7: moved to `dwell_worldgen::config`, so a test world's maps carry
/// the edit to every generator).
pub use dwell_worldgen::config::*;

/// What a preview tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    /// The top blocks' colours, tinted by height (the big map's look).
    Blocks,
    /// Each biome's colour (`BIOME_HUES`, by index) blended by its weight.
    Biomes,
    /// The biome map's temperature, blue (0) to red (1).
    Temperature,
    /// The biome map's wetness, tan (0) to blue (1).
    Wetness,
    /// The rain shadow's rainfall (systemic terrain T1), tan (0) to green (1).
    Rainfall,
    /// The rock at the surface (systemic terrain T2): its colour, darker on a wall, lighter on a
    /// tread.
    Rock,
    /// The soil (systemic terrain T3): bare rock grey (0 m) to its type's colour (2 m and deeper).
    Soil,
    /// The vegetation's density (systemic terrain T3) for the column's biome: tan (none) to green.
    Vegetation,
    /// The erosion's cut-back (systemic terrain T4): tan where cut down to the floor, dark red at a
    /// remnant's wall, pale where it stands; grey where nothing erodes.
    Erosion,
    /// The tectonics (systemic terrain T5): the two plates (the overriding tan, the other blue-grey,
    /// the old core darker), the boundary dark, the volcanoes red by height; grey when off.
    Tectonics,
    /// The structures placed (ADR 0074 B5): trees, boulders and every placement rule, expected per
    /// hectare by the biomes' `trees` and the climate; pale (none) to dark green (about 60).
    Placements,
}

impl Overlay {
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Biomes,
            2 => Self::Temperature,
            3 => Self::Wetness,
            4 => Self::Rainfall,
            5 => Self::Rock,
            6 => Self::Soil,
            7 => Self::Vegetation,
            8 => Self::Erosion,
            9 => Self::Tectonics,
            10 => Self::Placements,
            _ => Self::Blocks,
        }
    }
}

/// The tectonics overlay's colour at face-local (x, z) (systemic terrain T5), `texel` m wide.
pub fn tectonics_rgb(
    t: &dwell_worldgen::tectonics::Tectonics,
    x: i64,
    z: i64,
    texel: i64,
) -> [f64; 3] {
    if !t.enabled() {
        return [0.5; 3];
    }
    let (x, z) = (x as f64, z as f64);
    let (_, across) = t.frame(x, z);
    let mut c = if t.kind == dwell_worldgen::tectonics::NONE {
        [0.55, 0.65, 0.8]
    } else if across > t.core_distance && t.kind == dwell_worldgen::tectonics::CONVERGENT {
        [0.62, 0.5, 0.4]
    } else if across > 0.0 {
        [0.86, 0.74, 0.58]
    } else {
        [0.55, 0.65, 0.8]
    };
    if t.kind != dwell_worldgen::tectonics::NONE && across.abs() < 1.5 * texel as f64 {
        c = [0.15, 0.1, 0.1];
    }
    for v in t.volcanoes() {
        let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
        if r < v.radius {
            let k = (dwell_worldgen::tectonics::Tectonics::cone(&v, r) / v.height.max(1.0))
                .clamp(0.0, 1.0);
            c = lerp3([0.6, 0.25, 0.15], [0.95, 0.2, 0.1], k);
            if v.crater_radius > 0.0 && r < v.crater_radius {
                c = [0.25, 0.05, 0.05];
            }
        }
    }
    c
}

/// The biome overlay's colours, by biome index (the default first), repeating.
pub const BIOME_HUES: [[f64; 3]; 6] = [
    [0.36, 0.62, 0.30],
    [0.90, 0.72, 0.35],
    [0.35, 0.55, 0.85],
    [0.80, 0.35, 0.40],
    [0.60, 0.45, 0.80],
    [0.40, 0.80, 0.75],
];

/// A water texel's colour moved toward its look's scatter colour by how far the look differs
/// from the default water's (none where it is the default's).
fn water_tinted(
    c: [u8; 3],
    plain: dwell_worldgen::terrain::WaterTint,
    at: dwell_worldgen::terrain::WaterTint,
) -> [u8; 3] {
    let diff: f32 = (0..3)
        .map(|i| (at.scatter[i] - plain.scatter[i]).abs())
        .sum();
    if diff < 1e-4 {
        return c;
    }
    let srgb = |l: f32| {
        let l = l.clamp(0.0, 1.0);
        if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        }
    };
    let t = f64::from((diff * 2.0).min(0.7));
    let target = [0, 1, 2].map(|i| f64::from(srgb(at.scatter[i])));
    let base = c.map(|v| f64::from(v) / 255.0);
    bytes(lerp3(base, target, t))
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn bytes(c: [f64; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8)
}

/// The world overview's frame (ADR 0074 B3): `n` texels a side (even) of the returned `texel` m
/// from face-local (x0, x0) = (-n/2 · texel), the smallest whole texel that fits the whole disc
/// (`WORLD_RADIUS`, island and sea) inside the square. Chisel's page draws the same frame
/// (`client/src/chisel/overview.ts` `overviewFrame`).
pub fn overview_frame(n: i64) -> (i64, i64) {
    let n = (n.max(2) / 2) * 2;
    let texel = (2 * i64::from(dwell_world::WORLD_RADIUS) + n - 1) / n;
    (-(n / 2) * texel, texel)
}

/// A preview tile: `n` × `n` texels of `texel` m from face-local (x0, z0) (snapped to the texel
/// grid), RGBA rows of x from z0 (alpha 0 beyond the disc).
pub fn preview(g: &TerrainGenerator, x0: i64, z0: i64, n: i64, texel: i64, o: Overlay) -> Vec<u8> {
    preview_shaded(g, x0, z0, n, texel, o, false)
}

/// [`preview`], hill-shaded when `relief` (`crate::chisel_section::hillshade`, from a ring of
/// texels one wider than the tile): each dry texel's colour scaled by its slope's light, so
/// walls and cones read on the 2D views (water is flat: unshaded).
pub fn preview_shaded(
    g: &TerrainGenerator,
    x0: i64,
    z0: i64,
    n: i64,
    texel: i64,
    o: Overlay,
    relief: bool,
) -> Vec<u8> {
    // The default water's look: water drawn otherwise (a biome's `waterLook`) is tinted.
    let plain = water_tint_of(&BIOMES[0]);
    let tinted = g.biomes().iter().any(|b| b.water_look.is_some());
    let (gx0, gz0) = (x0.div_euclid(texel), z0.div_euclid(texel));
    let pad = i64::from(relief);
    let m = n + 2 * pad;
    let rect = Rect {
        x0: gx0 - pad,
        z0: gz0 - pad,
        x1: gx0 + n + pad,
        z1: gz0 + n + pad,
    };
    let raw = raster_with_heights(g, &LOD_MAP, texel, &rect);
    let (rgba_m, heights_m) = raw.split_at((m * m) as usize * 4);
    let code_at = |k: usize| i16::from_le_bytes([heights_m[k * 2], heights_m[k * 2 + 1]]);
    let shade = relief.then(|| {
        let h: Vec<f64> = (0..(m * m) as usize)
            .map(|k| match decode_height(i32::from(code_at(k))) {
                Some((h, false)) => h,
                _ => f64::NAN,
            })
            .collect();
        crate::chisel_section::hillshade(&h, n as usize, texel as f64)
    });
    let texels = (n * n) as usize;
    // The tile's own texels (inside the relief's ring).
    let inner = |k: usize| {
        let (i, j) = (k as i64 % n, k as i64 / n);
        ((j + pad) * m + i + pad) as usize
    };
    let mut rgba = Vec::with_capacity(texels * 4);
    let mut heights = Vec::with_capacity(texels * 2);
    for k in 0..texels {
        let q = inner(k);
        rgba.extend_from_slice(&rgba_m[q * 4..q * 4 + 4]);
        heights.extend_from_slice(&heights_m[q * 2..q * 2 + 2]);
    }
    let mut out = Vec::with_capacity(texels * 4);
    for k in 0..texels {
        let px = &rgba[k * 4..k * 4 + 4];
        if px[3] == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let (gx, gz) = (gx0 + (k as i64) % n, gz0 + (k as i64) / n);
        let (x, z) = (gx * texel + texel / 2, gz * texel + texel / 2);
        let rgb = match o {
            Overlay::Blocks => {
                let code = i16::from_le_bytes([heights[k * 2], heights[k * 2 + 1]]);
                let rgb = (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2]);
                let (c, wet) = match decode_height(i32::from(code)) {
                    Some((h, wet)) => (tint_rgb(&MAP, rgb, h, wet), wet),
                    None => (rgb, false),
                };
                let c = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                if wet && tinted {
                    water_tinted(c, plain, g.water_look_at(x, z))
                } else {
                    c
                }
            }
            Overlay::Biomes => {
                let mut c = [0.0; 3];
                for (i, w) in g.biome_mix(x, z).iter() {
                    let h = BIOME_HUES[i % BIOME_HUES.len()];
                    for j in 0..3 {
                        c[j] += h[j] * w;
                    }
                }
                bytes(c)
            }
            Overlay::Temperature | Overlay::Wetness => bytes(match g.biome_geography(x, z) {
                None => [0.5; 3],
                Some(geo) if o == Overlay::Temperature => {
                    lerp3([0.2, 0.35, 0.9], [0.9, 0.25, 0.15], geo.temperature)
                }
                Some(geo) => lerp3([0.82, 0.72, 0.5], [0.15, 0.35, 0.85], geo.wetness),
            }),
            Overlay::Tectonics => bytes(tectonics_rgb(&g.map().tectonics, x, z, texel)),
            Overlay::Rock => {
                let (rock, part) = g.rock_at_surface(x, z);
                let c = rock.color;
                let base = [c >> 16, c >> 8, c].map(|v| f64::from(v & 0xff) / 255.0);
                use dwell_worldgen::geology::Part;
                bytes(match part {
                    Part::Wall => base.map(|v| v * 0.6),
                    Part::Tread => lerp3(base, [1.0; 3], 0.2),
                    Part::Talus | Part::Plain => base,
                })
            }
            Overlay::Soil => bytes(match g.climate_at(x, z) {
                None => [0.5; 3],
                Some(c) => {
                    let t = g.vegetation().soil_types[usize::from(c.soil_type)].color;
                    let col = [t >> 16, t >> 8, t].map(|v| f64::from(v & 0xff) / 255.0);
                    lerp3([0.62, 0.62, 0.6], col, c.soil_depth / 2.0)
                }
            }),
            Overlay::Vegetation => bytes(match g.vegetation_at(x, z) {
                None => [0.5; 3],
                Some(d) => lerp3([0.86, 0.78, 0.58], [0.1, 0.42, 0.16], d),
            }),
            Overlay::Placements => bytes(match g.placement_density_at(x, z) {
                None => [0.5; 3],
                Some(d) => lerp3(
                    [0.93, 0.88, 0.74],
                    [0.05, 0.3, 0.1],
                    1.0 - (-d / 30.0).exp(),
                ),
            }),
            Overlay::Erosion => bytes(match g.erosion_at(x, z) {
                None => [0.5; 3],
                Some((s, margin, _)) => {
                    let wall = g.geology().erosion.wall.max(1.0);
                    let c = if margin < 0.0 {
                        lerp3(
                            [0.9, 0.78, 0.55],
                            [0.75, 0.6, 0.4],
                            (-margin / 200.0).min(1.0),
                        )
                    } else if margin < 4.0 * wall {
                        [0.55, 0.12, 0.08]
                    } else {
                        [0.93, 0.9, 0.85]
                    };
                    lerp3([0.5; 3], c, s)
                }
            }),
            Overlay::Rainfall => bytes(match g.biome_rainfall(x, z) {
                None => [0.5; 3],
                // Square-root eased: the dry lee's small values stay readable.
                Some(r) => lerp3([0.86, 0.74, 0.5], [0.1, 0.5, 0.25], r.max(0.0).sqrt()),
            }),
        };
        let rgb = match &shade {
            Some(f) if f[k] != 1.0 => {
                rgb.map(|v| (f64::from(v) * f[k]).round().clamp(0.0, 255.0) as u8)
            }
            _ => rgb,
        };
        out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_worldgen::bifacial::BifacialTerrain;
    use dwell_worldgen::geology::GEOLOGY;
    use dwell_worldgen::terrain::{
        BIOME_MAP, FIXTURE_BIOMES, FIXTURE_WORLDS, HEIGHT, PRESET_BIOMES,
    };
    use dwell_worldgen::vegetation::VEGETATION;
    use serde_json::Value;

    fn file(path: &str) -> Value {
        let p = format!(
            "{}/../../shared/worldgen/{path}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn a_worldgen_config_names_a_fixture_or_carries_an_edits_knobs() {
        // both-faces: config parsing only, no face.
        assert_eq!(worldgen_height("").unwrap(), None, "the build's ground");
        let peak = worldgen_height("test:tall_peak").unwrap().unwrap();
        assert_eq!(peak.tectonics.cone_height, 4800.0);
        assert!(worldgen_height("test:nowhere").is_err());
        // A Chisel edit's world knobs: the tall peak's own JSON is the same height field.
        let edit = serde_json::json!({ "world": file("worlds/tall_peak.json")["overrides"] });
        assert_eq!(worldgen_height(&edit.to_string()).unwrap(), Some(peak));
        // An edit that leaves the world alone is the build's ground.
        assert_eq!(worldgen_height(r#"{"biomes":[]}"#).unwrap(), None);
    }

    #[test]
    fn a_test_worlds_maps_are_keyed_apart_and_the_default_key_is_unchanged() {
        // both-faces: cache keys only, no face.
        let key = dwell_worldgen::worldmap::cache::key(7);
        assert_eq!(worldgen_map_key(7, ""), key);
        assert_eq!(worldgen_map_key(7, r#"{"biomes":[]}"#), key);
        let peak = worldgen_map_key(7, "test:tall_peak");
        assert!(peak.starts_with(&key) && peak != key);
        assert_ne!(peak, worldgen_map_key(7, "test:archipelago"));
    }

    #[test]
    fn the_json_biomes_are_the_generated_tables() {
        // both-faces: data parsing only, no face.
        assert_eq!(
            biome_from_json(&file("biomes/default.json")).unwrap(),
            BIOMES[0]
        );
        for f in ["desert", "archipelago"] {
            let b = biome_from_json(&file(&format!("biomes/fixtures/{f}.json"))).unwrap();
            assert_eq!(
                Some(&b),
                FIXTURE_BIOMES.iter().find(|x| x.name == b.name),
                "{f}"
            );
        }
        // The presets, as Chisel sends them (their label and description are the page's alone).
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../shared/worldgen/biomes/presets"
        );
        let mut presets = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let v: Value =
                serde_json::from_str(&std::fs::read_to_string(e.unwrap().path()).unwrap()).unwrap();
            let b = biome_from_json(&v).unwrap();
            assert_eq!(
                Some(&b),
                PRESET_BIOMES.iter().find(|x| x.name == b.name),
                "{}",
                b.name
            );
            presets += 1;
        }
        assert_eq!(presets, PRESET_BIOMES.len());
        // The archipelago's world knobs, as Chisel sends them, are the fixture world's.
        let w = file("worlds/archipelago.json");
        let (_, want) = FIXTURE_WORLDS[0];
        assert_eq!(height_from_json(&w["overrides"]).unwrap(), want);
        assert_eq!(height_from_json(&serde_json::json!({})).unwrap(), HEIGHT);
        assert_eq!(
            biome_map_from_json(&file("terrain.json")["biomeMap"]).unwrap(),
            BIOME_MAP
        );
    }

    #[test]
    fn a_bad_edit_fails_with_a_message() {
        // both-faces: data parsing only, no face.
        let mut d = file("biomes/default.json");
        d["bands"][0]["ground"] = "dwell:nope".into();
        let e = biome_from_json(&d).unwrap_err();
        assert!(e.contains("dwell:nope"), "{e}");
    }

    #[test]
    fn an_edited_biome_changes_the_tile_only_where_it_is() {
        // face-a-only: Chisel previews face A (face B is its mirror, generated by the same rules).
        let t = BifacialTerrain::height_field(7);
        let mut desert = file("biomes/fixtures/desert.json");
        let on = |desert: &Value| {
            let d = biome_from_json(desert).unwrap();
            t.local(dwell_world::Face::A)
                .clone()
                .with_biomes_on(&[BIOMES[0], d], BIOME_MAP)
        };
        let g0 = on(&desert);
        desert["bands"][0]["ground"] = "dwell:dirt".into();
        desert["bands"][0]["under"] = "dwell:dirt".into();
        let g1 = on(&desert);
        // The whole disc, coarsely.
        let n = 64;
        let texel = 2 * i64::from(dwell_world::WORLD_RADIUS) / n;
        let x0 = -n * texel / 2;
        let a = preview(&g0, x0, x0, n, texel, Overlay::Blocks);
        let b = preview(&g1, x0, x0, n, texel, Overlay::Blocks);
        let mut changed = 0;
        for k in 0..(n * n) as usize {
            let (i, j) = (k as i64 % n, k as i64 / n);
            let (x, z) = (x0 + i * texel + texel / 2, x0 + j * texel + texel / 2);
            if a[k * 4..k * 4 + 4] != b[k * 4..k * 4 + 4] {
                assert!(
                    g0.biome_mix(x, z).weight(1) > 0.0,
                    "texel ({x}, {z}) changed outside the desert"
                );
                changed += 1;
            }
        }
        assert!(
            changed > 20,
            "the desert's dunes changed colour: {changed} texels"
        );
    }

    #[test]
    fn the_overview_frame_covers_the_whole_disc() {
        // face-a-only: Chisel previews face A (face B is its mirror, generated by the same rules).
        let r = i64::from(dwell_world::WORLD_RADIUS);
        for n in [32, 128, 512] {
            let (x0, texel) = overview_frame(n);
            assert!(x0 <= -r && x0 + n * texel >= r, "n {n}: {x0} + {n}·{texel}");
            assert!(x0 > -r - texel, "n {n}: the frame is one texel too wide");
            assert_eq!(x0 % texel, 0, "on the texel grid");
        }
        // The tile: every texel well inside the disc drawn, every one well beyond it empty.
        let g = BifacialTerrain::height_field(7)
            .local(dwell_world::Face::A)
            .clone();
        let n = 32;
        let (x0, texel) = overview_frame(n);
        let t = preview(&g, x0, x0, n, texel, Overlay::Biomes);
        let (mut inside, mut outside) = (0, 0);
        for k in 0..(n * n) as usize {
            let (i, j) = (k as i64 % n, k as i64 / n);
            let (x, z) = (x0 + i * texel + texel / 2, x0 + j * texel + texel / 2);
            let d = ((x * x + z * z) as f64).sqrt();
            if d < (r - 2 * texel) as f64 {
                assert_eq!(t[k * 4 + 3], 255, "({x}, {z}) inside the disc is drawn");
                inside += 1;
            } else if d > (r + 2 * texel) as f64 {
                assert_eq!(t[k * 4 + 3], 0, "({x}, {z}) beyond the disc is empty");
                outside += 1;
            }
        }
        assert!(inside > 500 && outside > 50, "{inside} in, {outside} out");
    }

    #[test]
    fn the_tectonics_knobs_round_trip_and_draw_their_overlay() {
        // face-a-only: Chisel's parsing and the overlay's colours are face-local data.
        assert_eq!(height_from_json(&serde_json::json!({})).unwrap(), HEIGHT);
        let h = height_from_json(&serde_json::json!({
            "tectonics": { "kind": 1, "arcCount": 3, "coneHeight": 900, "hotspotCount": 2 }
        }))
        .unwrap();
        let t = h.tectonics;
        assert_eq!((t.kind, t.arc_count, t.hotspot_count), (1, 3, 2));
        assert_eq!(t.cone_height, 900.0);
        assert_eq!(
            t.angle, HEIGHT.tectonics.angle,
            "a key left out is the build's"
        );
        assert!(
            height_from_json(&serde_json::json!({ "tectonics": { "coneRadius": 0 } })).is_err()
        );
        // The tall peak's knob (a hardened cone): read, and out of 0..1 refused.
        let hard = height_from_json(&serde_json::json!({ "tectonics": { "coneHardness": 0.95 } }));
        assert_eq!(hard.unwrap().tectonics.cone_hardness, 0.95);
        assert!(
            height_from_json(&serde_json::json!({ "tectonics": { "coneHardness": 2 } })).is_err()
        );
        assert_eq!(
            tectonics_rgb(&HEIGHT.tectonics, 0, 0, 64),
            [0.5; 3],
            "off: grey"
        );
        let v = t.volcanoes()[0];
        let red = tectonics_rgb(&t, (v.x + v.crater_radius * 2.0) as i64, v.z as i64, 64);
        assert!(red[0] > 0.8 && red[1] < 0.3, "a volcano reads red: {red:?}");
    }

    #[test]
    fn the_archipelago_knobs_break_the_preview_into_islands() {
        // face-a-only: Chisel previews face A (face B is its mirror, generated by the same rules).
        let w = file("worlds/archipelago.json");
        let h = height_from_json(&w["overrides"]).unwrap();
        let seed = dwell_worldgen::bifacial::face_seed(1, dwell_world::Face::A);
        let map = dwell_worldgen::worldmap::WorldMap::for_height(seed, h);
        let g = TerrainGenerator::with_map(seed, std::sync::Arc::new(map));
        let plain = BifacialTerrain::height_field(1)
            .local(dwell_world::Face::A)
            .clone();
        let wet = |g: &TerrainGenerator| {
            let mut n = 0;
            for x in (-8000..8000).step_by(200) {
                for z in (-8000..8000).step_by(200) {
                    n += i32::from(
                        g.column_lod(x, z, 64).water != dwell_worldgen::terrain::Water::None,
                    );
                }
            }
            n
        };
        let bf = dwell_worldgen::base::BaseField::with_height(seed, h);
        let (mut below, mut mapbelow) = (0, 0);
        for x in (-8000..8000).step_by(200) {
            for z in (-8000..8000).step_by(200) {
                below +=
                    i32::from(bf.height_lod(x, z, 100).unwrap() < dwell_world::SEA_LEVEL_M as f32);
                mapbelow += i32::from(g.map().height_at(x as f64, z as f64).0 < 0.0);
            }
        }
        // The sea between the islands reaches the open sea, so the world map keeps it (a sea
        // pocket cut off would be filled as a depression).
        assert!(
            below > 1000 && mapbelow * 10 >= below * 9,
            "the base field's sea is the map's: {below} -> {mapbelow}"
        );
        let (a, b) = (wet(&g), wet(&plain));
        assert!(
            a > b + 200,
            "the archipelago has sea in the view: {a} vs {b}"
        );
    }

    #[test]
    fn a_windy_edit_shows_its_rain_shadow_in_the_rainfall_overlay() {
        // face-a-only: Chisel previews face A (the rain shadow's faces are dwell-worldgen's
        // tests/rain_shadow.rs).
        // An edit by data alone, as the page sends it: terrain.json's biome map with a strong
        // wind, the wetness all rain.
        let mut bm = file("terrain.json")["biomeMap"].clone();
        bm["windDirection"] = 0.into();
        bm["windReach"] = 1500.into();
        bm["rainShare"] = 1.into();
        let desert = file("biomes/fixtures/desert.json");
        let edit = serde_json::json!({
            "biomes": [file("biomes/default.json"), desert],
            "biomeMap": bm,
        });
        let e = edit_from_json(&edit.to_string()).unwrap();
        assert_eq!(e.map.rain_share, 1.0);
        assert_eq!(e.map.wind_reach, 1500.0);
        let t = BifacialTerrain::height_field(7);
        let g = t
            .local(dwell_world::Face::A)
            .clone()
            .with_biomes_on(&e.biomes, e.map);
        let n = 64;
        let texel = 2 * i64::from(dwell_world::WORLD_RADIUS) / n;
        let x0 = -n * texel / 2;
        let tile = preview(&g, x0, x0, n, texel, Overlay::Rainfall);
        let mut shades = std::collections::BTreeSet::new();
        for k in 0..(n * n) as usize {
            if tile[k * 4 + 3] == 255 {
                shades.insert(tile[k * 4 + 1]);
            }
        }
        assert!(
            shades.len() > 20,
            "the rainfall varies: {} shades",
            shades.len()
        );
        // The wetness is the rain's now: it differs from the build's where the rain does.
        let plain = t
            .local(dwell_world::Face::A)
            .clone()
            .with_biomes_on(&e.biomes, BIOME_MAP);
        let a = preview(&g, x0, x0, n, texel, Overlay::Wetness);
        let b = preview(&plain, x0, x0, n, texel, Overlay::Wetness);
        assert_ne!(a, b, "rainShare 1 changes the wetness overlay");
    }

    #[test]
    fn the_json_geology_is_the_generated_tables_and_takes_a_rock_by_data() {
        // face-a-only: Chisel previews face A (the shaping's faces are dwell-worldgen's
        // tests/rocks.rs).
        let g = geology_from_json(&file("rocks.json")).unwrap();
        assert_eq!(g, GEOLOGY);
        // A fixture rock and stack by data alone: a new hard cap over shale, named by a biome.
        let mut v = file("rocks.json");
        v["rocks"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "test:cap", "hardness": 0.95, "jointing": "vertical", "bed": 10,
            "solubility": 0, "face": "dwell:stone", "cap": "dwell:stone", "scree": "dwell:sand",
            "color": "#ff0000"
        }));
        v["stacks"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "test:capped", "weight": 0,
            "layers": [ { "rock": "test:cap", "thickness": 20 }, { "rock": "dwell:shale", "thickness": 30 } ]
        }));
        v["shaping"]["strength"] = 1.into();
        let mut b = file("biomes/default.json");
        b["rockStack"] = "test:capped".into();
        let edit = serde_json::json!({ "biomes": [b], "geology": v }).to_string();
        let e = edit_from_json(&edit).unwrap();
        assert_eq!(e.geology.rocks.len(), GEOLOGY.rocks.len() + 1);
        assert_eq!(e.biomes[0].rock_stack, Some("test:capped"));
        // The rock overlay shows the new rock somewhere, and the blocks differ from the build's.
        let t = BifacialTerrain::height_field(7);
        let base = t.local(dwell_world::Face::A).clone();
        let g1 = base
            .clone()
            .with_biomes_and_geology(&e.biomes, e.map, e.geology);
        let (x0, n, texel) = (-4096, 64, 128);
        let tile = preview(&g1, x0, x0, n, texel, Overlay::Rock);
        assert!(
            tile.chunks(4).any(|p| p[0] > 120 && p[1] == 0 && p[2] == 0),
            "the fixture rock's red shows"
        );
        let a = preview(&g1, x0, x0, n, texel, Overlay::Blocks);
        let b = preview(&base, x0, x0, n, texel, Overlay::Blocks);
        assert_ne!(a, b, "strength 1 reshapes the ground");
        // A stack naming an unknown rock, or a biome an unknown stack, fails with a message.
        v["stacks"][0]["layers"][0]["rock"] = "test:nothing".into();
        assert!(geology_from_json(&v).unwrap_err().contains("test:nothing"));
        let mut b = file("biomes/default.json");
        b["rockStack"] = "test:none".into();
        let bad = serde_json::json!({ "biomes": [b] }).to_string();
        assert!(edit_from_json(&bad).err().unwrap().contains("test:none"));
    }

    #[test]
    fn the_placement_overlay_follows_the_biomes_trees() {
        // face-a-only: Chisel previews face A (the placements' faces are dwell-worldgen's
        // tests/trees.rs).
        let t = BifacialTerrain::height_field(7);
        let base = t.local(dwell_world::Face::A).clone();
        let (x0, n, texel) = (-2048, 32, 128);
        let shade = |b: Value| {
            let e = serde_json::json!({ "biomes": [b] });
            let e = edit_from_json(&e.to_string()).unwrap();
            let g = base
                .clone()
                .with_biomes_and_geology(&e.biomes, e.map, e.geology);
            let tile = preview(&g, x0, x0, n, texel, Overlay::Placements);
            // The darker, the denser: the sum of (255 - green) over the land drawn.
            tile.chunks(4)
                .filter(|p| p[3] > 0)
                .map(|p| u64::from(255 - p[1]))
                .sum::<u64>()
        };
        let mut d = file("biomes/default.json");
        let plain = shade(d.clone());
        d["trees"] = serde_json::json!({ "pine_groves": 0.2 });
        let sparse = shade(d.clone());
        d["placements"] = serde_json::json!([]);
        d.as_object_mut().unwrap().remove("trees");
        let bare = shade(d);
        assert!(
            plain > sparse && sparse > bare,
            "{plain} > {sparse} > {bare}"
        );
    }

    #[test]
    fn the_json_vegetation_is_the_generated_tables_and_takes_a_kind_by_data() {
        // face-a-only: Chisel previews face A (the vegetation's faces are dwell-worldgen's
        // tests/vegetation.rs).
        assert_eq!(
            vegetation_from_json(&file("vegetation.json")).unwrap(),
            VEGETATION
        );
        // The boreal preset beside the default (both opting in), the rain shadow on.
        let mut d = file("biomes/default.json");
        d["vegetation"] = 1.into();
        let boreal = file("biomes/presets/boreal.json");
        let mut map = file("terrain.json")["biomeMap"].clone();
        map["rainShare"] = 0.5.into();
        let edit = |veg: &Value| {
            let e =
                serde_json::json!({ "biomes": [d, boreal], "biomeMap": map, "vegetation": veg });
            edit_from_json(&e.to_string()).unwrap()
        };
        let t = BifacialTerrain::height_field(7);
        let base = t.local(dwell_world::Face::A).clone();
        let gen_of = |e: Edit| {
            base.clone()
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
        };
        let (x0, n, texel) = (-6144, 48, 256);
        let green = |tile: &[u8]| {
            tile.chunks(4)
                .filter(|p| p[3] > 0 && u16::from(p[1]) > u16::from(p[0]) + 60)
                .count()
        };
        let g = gen_of(edit(&file("vegetation.json")));
        let build = green(&preview(&g, x0, x0, n, texel, Overlay::Vegetation));
        assert!(build > 0, "the build's conifers grow somewhere");
        // A fixture kind by data alone: the conifers replaced by one that wants 40 °C and more.
        let mut v = file("vegetation.json");
        v["kinds"][0]["name"] = "test:hot_pines".into();
        v["kinds"][0]["temperature"] = serde_json::json!([40, 80]);
        let hot = gen_of(edit(&v));
        assert_eq!(hot.vegetation().kinds[0].name, "test:hot_pines");
        assert_eq!(
            green(&preview(&hot, x0, x0, n, texel, Overlay::Vegetation)),
            0,
            "nowhere is that hot"
        );
        // The soil overlay draws; a kind naming an unknown placement or soil type fails, named.
        let soil = preview(&g, x0, x0, n, texel, Overlay::Soil);
        assert!(soil.chunks(4).any(|p| p[3] > 0));
        v["kinds"][0]["placement"] = "test:nothing".into();
        assert!(
            vegetation_from_json(&v)
                .unwrap_err()
                .contains("test:nothing")
        );
        let mut v = file("vegetation.json");
        v["kinds"][0]["soils"] = serde_json::json!(["test:clay"]);
        assert!(vegetation_from_json(&v).unwrap_err().contains("test:clay"));
    }
}
