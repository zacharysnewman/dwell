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
use dwell_worldgen::geology::{GEOLOGY, Geology, Jointing, Layer, Rock, Stack};
use dwell_worldgen::terrain::{
    BIOME_MAP, BIOMES, Biome, BiomeMap, ElevationBand, HEIGHT, HeightField, TerrainGenerator,
    WaterLook, Where, WhereRange, water_tint_of,
};
use dwell_worldgen::vegetation::{Kind, RockSoil, SoilType, VEGETATION, Vegetation};
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
    /// The rain shadow's rainfall (systemic terrain T1), tan (0) to green (1).
    Rainfall,
    /// The rock at the surface (systemic terrain T2): its colour, darker on a wall, lighter on a
    /// tread.
    Rock,
    /// The soil (systemic terrain T3): bare rock grey (0 m) to its type's colour (2 m and deeper).
    Soil,
    /// The vegetation's density (systemic terrain T3) for the column's biome: tan (none) to green.
    Vegetation,
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
    at.as_f64()
        .ok_or_else(|| format!("{path} must be a number"))
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
        let k = |key: &str| num(v, &format!("banks.{key}"));
        s.river_valley_slope = k("riverSlope")?;
        s.lake_bank = k("lake")?;
        s.lake_bank_slope = k("lakeSlope")?;
        s.beach = k("beach")?;
        Ok(())
    })()
    .map_err(at)?;
    let w = v
        .get("where")
        .ok_or_else(|| at("where is missing".into()))?;
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
        water_look: water_look(v).map_err(at)?,
        rock_stack: v.get("rockStack").and_then(Value::as_str).map(leak),
        vegetation: v.get("vegetation").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

/// A biome's optional `waterLook` (`scatter` "#rrggbb", `absorption` three numbers).
fn water_look(v: &Value) -> Result<Option<WaterLook>, String> {
    let Some(l) = v.get("waterLook") else {
        return Ok(None);
    };
    let scatter = match l.get("scatter") {
        None => None,
        Some(c) => Some(
            c.as_str()
                .and_then(|c| c.strip_prefix('#'))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or("waterLook.scatter must be a colour \"#rrggbb\"")?,
        ),
    };
    let absorption = match l.get("absorption").map(Value::as_array) {
        None => None,
        Some(Some(a)) if a.len() == 3 => {
            let mut out = [0.0f32; 3];
            for (o, x) in out.iter_mut().zip(a) {
                *o = x.as_f64().ok_or("waterLook.absorption must be numbers")? as f32;
            }
            Some(out)
        }
        Some(_) => return Err("waterLook.absorption must be three numbers".into()),
    };
    Ok(Some(WaterLook {
        scatter,
        absorption,
    }))
}

/// The world's height field with an edit's world knobs (`{ "islands": {...}, "seaFloor": {...},
/// "inland": {...} }`, terrain.json's groups, any key left out as the build's): the archipelago's
/// knobs (ADR 0074 B4).
pub fn height_from_json(v: &Value) -> Result<HeightField, String> {
    let mut h = HEIGHT;
    let get = |g: &str, k: &str| v.get(g).and_then(|g| g.get(k)).and_then(Value::as_f64);
    let set = |slot: &mut f32, g: &str, k: &str| {
        if let Some(x) = get(g, k) {
            *slot = x as f32;
        }
    };
    set(&mut h.island_amplitude, "islands", "amplitude");
    set(&mut h.island_cap, "islands", "cap");
    if let Some(x) = get("islands", "wavelength") {
        h.island_wavelength = x as i32;
    }
    if let Some(x) = get("islands", "octaves") {
        h.island_octaves = x as u32;
    }
    if let Some(x) = get("seaFloor", "depth") {
        h.sea_floor = -x as f32;
    }
    set(&mut h.shelf, "seaFloor", "shelf");
    set(&mut h.inland_rise, "inland", "rise");
    set(&mut h.rise_half, "inland", "halfDistance");
    set(&mut h.hill_height, "hills", "amplitude");
    if !(h.island_wavelength >= 64 && h.island_octaves >= 1 && h.shelf > 0.0 && h.rise_half > 0.0) {
        return Err("world: a knob is out of range".into());
    }
    Ok(h)
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
        wind_direction: n("windDirection")?,
        wind_reach: n("windReach")?,
        base_rain: n("baseRain")?,
        uplift_rain: n("upliftRain")?,
        lee_dry: n("leeDry")?,
        rain_share: n("rainShare")?,
        wind_smooth_m: n("windSmoothM")?,
    })
}

/// The geology from its JSON (`shared/worldgen/rocks.json`, checked by `biomes.mjs`
/// `validateRocks`): the rocks, the stacks, the provinces and the shaping.
pub fn geology_from_json(v: &Value) -> Result<Geology, String> {
    let list = |k: &str| {
        v.get(k)
            .and_then(Value::as_array)
            .filter(|l| !l.is_empty())
            .ok_or(format!("rocks.json: {k} must be a list"))
    };
    let mut rocks = Vec::new();
    for r in list("rocks")? {
        let name = text(r, "name")?;
        let at = |e: String| format!("rock {name}: {e}");
        let jointing = match r.get("jointing").and_then(Value::as_str) {
            Some("bedded") => Jointing::Bedded,
            Some("vertical") => Jointing::Vertical,
            Some("massive") => Jointing::Massive,
            _ => return Err(at("jointing must be bedded, vertical or massive".into())),
        };
        let color = r
            .get("color")
            .and_then(Value::as_str)
            .and_then(|c| c.strip_prefix('#'))
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or_else(|| at("color must be \"#rrggbb\"".into()))?;
        let rock = Rock {
            name,
            hardness: num(r, "hardness").map_err(at)?,
            jointing,
            bed: num(r, "bed").map_err(at)?.max(1.0),
            solubility: num(r, "solubility").map_err(at)?,
            face: text(r, "face").map_err(at)?,
            cap: text(r, "cap").map_err(at)?,
            scree: text(r, "scree").map_err(at)?,
            color,
        };
        for id in [rock.face, rock.cap, rock.scree] {
            if dwell_blocks::state(id).is_none() {
                return Err(at(format!("{id} is not in the registry")));
            }
        }
        rocks.push(rock);
    }
    let mut stacks = Vec::new();
    for st in list("stacks")? {
        let name = text(st, "name")?;
        let at = |e: String| format!("stack {name}: {e}");
        let layers = st
            .get("layers")
            .and_then(Value::as_array)
            .filter(|l| !l.is_empty())
            .ok_or_else(|| at("layers must be a list".into()))?;
        let mut ls = Vec::new();
        for l in layers {
            let rock = text(l, "rock").map_err(at)?;
            if !rocks.iter().any(|r| r.name == rock) {
                return Err(at(format!("{rock} is not a rock")));
            }
            ls.push(Layer {
                rock,
                thickness: num(l, "thickness").map_err(at)?.max(1.0),
            });
        }
        stacks.push(Stack {
            name,
            weight: num(st, "weight").map_err(at)?,
            layers: Box::leak(ls.into_boxed_slice()),
        });
    }
    let g = |grp: &str, k: &str| {
        v.get(grp)
            .ok_or(format!("rocks.json: {grp} is missing"))
            .and_then(|o| num(o, k).map_err(|e| format!("rocks.json: {grp}.{e}")))
    };
    Ok(Geology {
        rocks: Box::leak(rocks.into_boxed_slice()),
        stacks: Box::leak(stacks.into_boxed_slice()),
        province_wavelength: (g("provinces", "wavelength")? as i32).max(256),
        strength: g("shaping", "strength")?.clamp(0.0, 1.0),
        tread: g("shaping", "tread")?,
        sheer: g("shaping", "sheer")?,
    })
}

/// The soil and vegetation from its JSON (`shared/worldgen/vegetation.json`, checked by
/// `biomes.mjs` `validateVegetation`; systemic terrain T3).
pub fn vegetation_from_json(v: &Value) -> Result<Vegetation, String> {
    let at = |e: String| format!("vegetation.json: {e}");
    let list = |k: &str| {
        v.get(k)
            .and_then(Value::as_array)
            .ok_or(at(format!("{k} must be a list")))
    };
    let mut types = Vec::new();
    for t in list("soilTypes")? {
        let color = t
            .get("color")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix('#'))
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or(at("a soil type's color must be \"#rrggbb\"".into()))?;
        types.push(SoilType {
            name: text(t, "name").map_err(at)?,
            color,
        });
    }
    if types.is_empty() {
        return Err(at("soilTypes must not be empty".into()));
    }
    let type_of = |s: &Value| -> Result<u8, String> {
        let n = s.as_str().unwrap_or("?");
        types
            .iter()
            .position(|t| t.name == n)
            .map(|i| i as u8)
            .ok_or(at(format!("{n} is not a soil type")))
    };
    let soil = |r: &Value, rock: &'static str| -> Result<RockSoil, String> {
        Ok(RockSoil {
            rock,
            depth: num(r, "depth").map_err(at)?,
            soil_type: type_of(r.get("type").unwrap_or(&Value::Null))?,
        })
    };
    let fallback = soil(v.get("fallback").unwrap_or(&Value::Null), "")?;
    let mut rock_soils = Vec::new();
    for r in list("rockSoils")? {
        rock_soils.push(soil(r, text(r, "rock").map_err(at)?)?);
    }
    let mut kinds = Vec::new();
    for k in list("kinds")? {
        let name = text(k, "name").map_err(at)?;
        let kf = |e: String| format!("vegetation.json: kind {name}: {e}");
        let pair = |key: &str| -> Result<[f64; 2], String> {
            match k.get(key).and_then(Value::as_array).map(Vec::as_slice) {
                Some([a, b]) => match (a.as_f64(), b.as_f64()) {
                    (Some(a), Some(b)) if a < b => Ok([a, b]),
                    _ => Err(kf(format!("{key} must be [least, most]"))),
                },
                _ => Err(kf(format!("{key} must be [least, most]"))),
            }
        };
        let soils: Result<Vec<u8>, String> = k
            .get("soils")
            .and_then(Value::as_array)
            .map(|l| l.iter().map(type_of).collect())
            .unwrap_or(Ok(Vec::new()));
        let placement = text(k, "placement").map_err(kf)?;
        if !dwell_worldgen::placements::PLACEMENTS
            .iter()
            .any(|p| p.name == placement)
        {
            return Err(kf(format!("placement {placement} is not a rule of placements.json")));
        }
        kinds.push(Kind {
            name,
            placement,
            temperature: pair("temperature")?,
            moisture: pair("moisture")?,
            min_soil: num(k, "minSoil").map_err(kf)?,
            max_slope_deg: num(k, "maxSlopeDeg").map_err(kf)?,
            soils: Box::leak(soils?.into_boxed_slice()),
            fade: [
                num(k, "fade.temperature").map_err(kf)?,
                num(k, "fade.moisture").map_err(kf)?,
                num(k, "fade.soil").map_err(kf)?,
            ],
            density: num(k, "density").map_err(kf)?,
        });
    }
    Ok(Vegetation {
        zero_c: num(v, "climate.zeroC").map_err(at)?,
        per_unit_c: num(v, "climate.perUnitC").map_err(at)?,
        flat_slope_deg: num(v, "soil.flatSlopeDeg").map_err(at)?,
        valley: num(v, "soil.valley").map_err(at)?,
        soil_types: Box::leak(types.into_boxed_slice()),
        fallback,
        rock_soils: Box::leak(rock_soils.into_boxed_slice()),
        kinds: Box::leak(kinds.into_boxed_slice()),
    })
}

/// A Chisel edit parsed: the biomes (the default first), the biome map, the geology, the soil and
/// vegetation, and the world's height field when the edit changes its knobs (`world`).
pub struct Edit {
    pub biomes: Vec<Biome>,
    pub map: BiomeMap,
    pub geology: Geology,
    pub vegetation: Vegetation,
    pub height: Option<HeightField>,
}

/// A Chisel edit (`{ "biomes": [...], "biomeMap": {...}, "world": {...} }`) parsed.
pub fn edit_from_json(text: &str) -> Result<Edit, String> {
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
    let height = match v.get("world") {
        Some(w) => Some(height_from_json(w)?).filter(|h| *h != HEIGHT),
        None => None,
    };
    let geology = match v.get("geology") {
        Some(g) => geology_from_json(g)?,
        None => GEOLOGY,
    };
    let vegetation = match v.get("vegetation") {
        Some(g) => vegetation_from_json(g)?,
        None => VEGETATION,
    };
    for r in vegetation.rock_soils {
        if !geology.rocks.iter().any(|k| k.name == r.rock) {
            return Err(format!("vegetation.json: rockSoils: {} is not a rock", r.rock));
        }
    }
    let biomes = biomes?;
    for b in &biomes {
        if let Some(s) = b.rock_stack
            && geology.stack(s).is_none()
        {
            return Err(format!("biome {}: rockStack {s} is not a stack", b.name));
        }
    }
    Ok(Edit {
        biomes,
        map,
        geology,
        vegetation,
        height,
    })
}

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
    // The default water's look: water drawn otherwise (a biome's `waterLook`) is tinted.
    let plain = water_tint_of(&BIOMES[0]);
    let tinted = g.biomes().iter().any(|b| b.water_look.is_some());
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
            Overlay::Rainfall => bytes(match g.biome_rainfall(x, z) {
                None => [0.5; 3],
                // Square-root eased: the dry lee's small values stay readable.
                Some(r) => lerp3([0.86, 0.74, 0.5], [0.1, 0.5, 0.25], r.max(0.0).sqrt()),
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
    use dwell_worldgen::terrain::{FIXTURE_BIOMES, FIXTURE_WORLDS, PRESET_BIOMES};

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
            let e = serde_json::json!({ "biomes": [d, boreal], "biomeMap": map, "vegetation": veg });
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
        let green = |tile: &[u8]| tile.chunks(4).filter(|p| p[3] > 0 && u16::from(p[1]) > u16::from(p[0]) + 60).count();
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
        assert!(vegetation_from_json(&v).unwrap_err().contains("test:nothing"));
        let mut v = file("vegetation.json");
        v["kinds"][0]["soils"] = serde_json::json!(["test:clay"]);
        assert!(vegetation_from_json(&v).unwrap_err().contains("test:clay"));
    }
}
