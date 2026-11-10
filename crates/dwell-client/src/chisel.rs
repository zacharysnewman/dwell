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
use dwell_worldgen::terrain::{
    BIOME_MAP, BIOMES, Biome, BiomeMap, ElevationBand, TerrainGenerator, Where, WhereRange,
};
use serde_json::Value;

use crate::lod_map::{Rect, raster_with_heights};

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
}

impl Overlay {
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Biomes,
            2 => Self::Temperature,
            3 => Self::Wetness,
            _ => Self::Blocks,
        }
    }
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

fn leak(s: &str) -> &'static str {
    // An editor session parses a few biomes per edit; their names live as long as the page.
    Box::leak(s.to_owned().into_boxed_str())
}

fn num(v: &Value, path: &str) -> Result<f64, String> {
    let mut at = v;
    for k in path.split('.') {
        at = at.get(k).ok_or_else(|| format!("{path} is missing"))?;
    }
    at.as_f64().ok_or_else(|| format!("{path} must be a number"))
}

fn text(v: &Value, k: &str) -> Result<&'static str, String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(leak)
        .ok_or_else(|| format!("{k} must be a string"))
}

fn names(v: &Value, k: &str) -> Result<&'static [&'static str], String> {
    let list = v
        .get(k)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{k} must be a list"))?;
    let out: Result<Vec<_>, _> = list
        .iter()
        .map(|n| {
            n.as_str()
                .map(leak)
                .ok_or_else(|| format!("{k}: a name must be a string"))
        })
        .collect();
    Ok(Box::leak(out?.into_boxed_slice()))
}

fn range(w: &Value, f: &str) -> Result<WhereRange, String> {
    let Some(r) = w.get(f) else {
        return Ok(WhereRange::ANY);
    };
    let bound = |k: &str, none: f64| r.get(k).and_then(Value::as_f64).unwrap_or(none);
    Ok(WhereRange {
        min: bound("min", f64::NEG_INFINITY),
        max: bound("max", f64::INFINITY),
        blend: num(r, "blend").map_err(|e| format!("where.{f}.{e}"))?,
    })
}

/// A biome from its JSON (a `shared/worldgen/biomes/<name>.json`, checked by `biomes.mjs`): its
/// groups over the world's water shaping (the default biome's surface, as gen.mjs merges them).
pub fn biome_from_json(v: &Value) -> Result<Biome, String> {
    let name = text(v, "name")?;
    let at = |e: String| format!("biome {name}: {e}");
    let mut s = BIOMES[0].surface;
    let d = |k: &str| num(v, &format!("detail.{k}"));
    let sh = |k: &str| num(v, &format!("shaping.{k}"));
    (|| -> Result<(), String> {
        s.detail_amplitude = d("amplitude")?;
        s.detail_wavelength = d("wavelength")? as i32;
        s.detail_octaves = d("octaves")? as u32;
        s.detail_mountain_amplitude = d("mountainAmplitude")?;
        s.detail_mountain_from = d("mountainFrom")?;
        s.detail_mountain_full = d("mountainFull")?;
        s.detail_water_fade = d("waterFade")?;
        s.edge_noise = sh("edgeNoise")?;
        s.edge_wavelength = sh("edgeWavelength")? as i32;
        s.edge_slope_lift = sh("edgeSlopeLift")?;
        s.rock_slope = sh("rockSlope")?;
        s.rock_slope_high = sh("rockSlopeHigh")?;
        s.rock_high = sh("rockHigh")?;
        s.rock_window = sh("rockWindow")?;
        s.shore = sh("shore")?;
        s.under_depth = sh("underDepth")?;
        Ok(())
    })()
    .map_err(at)?;
    let w = v.get("where").ok_or_else(|| at("where is missing".into()))?;
    let place = Where {
        weight: num(w, "weight").map_err(|e| at(format!("where.{e}")))?,
        temperature: range(w, "temperature").map_err(at)?,
        wetness: range(w, "wetness").map_err(at)?,
        elevation: range(w, "elevation").map_err(at)?,
        coast: range(w, "coast").map_err(at)?,
    };
    let bands = v
        .get("bands")
        .and_then(Value::as_array)
        .ok_or_else(|| at("bands must be a list".into()))?;
    let bands: Result<Vec<_>, String> = bands
        .iter()
        .map(|b| {
            Ok(ElevationBand {
                name: text(b, "name")?,
                from: b
                    .get("from")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NEG_INFINITY),
                ground: text(b, "ground")?,
                under: text(b, "under")?,
                rock: text(b, "rock")?,
                shore_block: text(b, "shoreBlock")?,
            })
        })
        .collect();
    let bands = bands.map_err(at)?;
    for b in &bands {
        for id in [b.ground, b.under, b.rock, b.shore_block] {
            if dwell_blocks::state(id).is_none() {
                return Err(at(format!("{id} is not in the registry")));
            }
        }
    }
    Ok(Biome {
        name,
        place,
        surface: s,
        bands: Box::leak(bands.into_boxed_slice()),
        decorations: names(v, "decorations").map_err(at)?,
        placements: names(v, "placements").map_err(at)?,
        water: text(v, "water").map_err(at)?,
    })
}

/// The biome map's geography from `terrain.json`'s `biomeMap` group.
pub fn biome_map_from_json(v: &Value) -> Result<BiomeMap, String> {
    let n = |k: &str| num(v, k).map_err(|e| format!("biomeMap.{e}"));
    Ok(BiomeMap {
        climate_wavelength: n("climateWavelength")? as i32,
        climate_noise: n("climateNoise")?,
        lapse_rate: n("lapseRate")?,
        wet_reach: n("wetReach")?,
        dither_wavelength: n("ditherWavelength")? as i32,
        dither: n("dither")?,
    })
}

/// A Chisel edit (`{ "biomes": [...], "biomeMap": {...} }`, the default biome first) parsed.
pub fn edit_from_json(text: &str) -> Result<(Vec<Biome>, BiomeMap), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let list = v
        .get("biomes")
        .and_then(Value::as_array)
        .filter(|l| !l.is_empty())
        .ok_or("biomes must be a list of at least one biome")?;
    let biomes: Result<Vec<_>, _> = list.iter().map(biome_from_json).collect();
    let map = match v.get("biomeMap") {
        Some(m) => biome_map_from_json(m)?,
        None => BIOME_MAP,
    };
    Ok((biomes?, map))
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn bytes(c: [f64; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A preview tile: `n` × `n` texels of `texel` m from face-local (x0, z0) (snapped to the texel
/// grid), RGBA rows of x from z0 (alpha 0 beyond the disc).
pub fn preview(g: &TerrainGenerator, x0: i64, z0: i64, n: i64, texel: i64, o: Overlay) -> Vec<u8> {
    let (gx0, gz0) = (x0.div_euclid(texel), z0.div_euclid(texel));
    let rect = Rect {
        x0: gx0,
        z0: gz0,
        x1: gx0 + n,
        z1: gz0 + n,
    };
    let raw = raster_with_heights(g, &LOD_MAP, texel, &rect);
    let texels = (n * n) as usize;
    let (rgba, heights) = raw.split_at(texels * 4);
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
                let c = match decode_height(i32::from(code)) {
                    Some((h, wet)) => tint_rgb(&MAP, rgb, h, wet),
                    None => rgb,
                };
                [(c >> 16) as u8, (c >> 8) as u8, c as u8]
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
        };
        out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_worldgen::bifacial::BifacialTerrain;
    use dwell_worldgen::terrain::FIXTURE_BIOMES;

    fn file(path: &str) -> Value {
        let p = format!(
            "{}/../../shared/worldgen/{path}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn the_json_biomes_are_the_generated_tables() {
        // both-faces: data parsing only, no face.
        assert_eq!(
            biome_from_json(&file("biomes/default.json")).unwrap(),
            BIOMES[0]
        );
        assert_eq!(
            biome_from_json(&file("biomes/fixtures/desert.json")).unwrap(),
            FIXTURE_BIOMES[0]
        );
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
}
