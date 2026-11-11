//! A world's worldgen config (ADR 0074 B7): a Chisel edit or a fixture world parsed from its JSON
//! (the files of `shared/worldgen/biomes`, `terrain.json`'s `biomeMap` and `world` knobs,
//! `rocks.json`, `vegetation.json`; already checked by `shared/worldgen/biomes.mjs`, the rules gen.mjs
//! uses). Chisel's preview parses its edits here, and a test world's maps carry the config's text
//! (`WorldMap::worldgen`), so every generator built over them (the server, the chunk and LOD jobs,
//! the water) puts the same biomes, geology, vegetation and tectonics on the ground.

use crate::geology::{Erosion, GEOLOGY, Geology, Jointing, Layer, Rock, Stack};
use crate::terrain::{
    BIOME_MAP, BIOMES, Biome, BiomeMap, ElevationBand, HEIGHT, HeightField, WaterLook, Where,
    WhereRange,
};
use crate::vegetation::{Kind, RockSoil, SoilType, VEGETATION, Vegetation};
use serde_json::Value;

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
    let placements = names(v, "placements").map_err(at)?;
    Ok(Biome {
        name,
        place,
        surface: s,
        bands: Box::leak(bands.into_boxed_slice()),
        decorations: names(v, "decorations").map_err(at)?,
        placements,
        trees: trees(v, placements).map_err(at)?,
        ground: ground(v).map_err(at)?,
        water: text(v, "water").map_err(at)?,
        water_look: water_look(v).map_err(at)?,
        rock_stack: v.get("rockStack").and_then(Value::as_str).map(leak),
        vegetation: v.get("vegetation").and_then(Value::as_f64).unwrap_or(0.0),
        erosion: v.get("erosion").and_then(Value::as_f64).unwrap_or(0.0),
        gullies: v.get("gullies").and_then(Value::as_f64).unwrap_or(0.0),
        arches: arches(v).map_err(at)?,
    })
}

/// A biome's optional `trees` (ADR 0074 B5): placement name → density in [0, 3], each one of its
/// `placements`.
fn trees(v: &Value, placements: &[&'static str]) -> Result<&'static [(&'static str, f64)], String> {
    let Some(t) = v.get("trees") else {
        return Ok(&[]);
    };
    let t = t
        .as_object()
        .ok_or("trees must map placement names to densities")?;
    if let Some(k) = t.keys().find(|k| !placements.contains(&k.as_str())) {
        return Err(format!("trees: {k:?} is not one of its placements"));
    }
    // In the placements' order, as the generated table has them.
    let mut out = Vec::new();
    for &k in placements {
        let Some(d) = t.get(k) else { continue };
        match d.as_f64() {
            Some(d) if (0.0..=3.0).contains(&d) => out.push((k, d)),
            _ => return Err(format!("trees.{k} must be in [0, 3]")),
        }
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

/// A biome's optional `ground` (ADR 0074 B5): ground item kind (`shared/ground` `kinds`) →
/// density in [0, 3].
fn ground(v: &Value) -> Result<Option<&'static [(&'static str, f64)]>, String> {
    let Some(g) = v.get("ground") else {
        return Ok(None);
    };
    let g = g
        .as_object()
        .ok_or("ground must map ground item kinds to densities")?;
    let mut out = Vec::new();
    // In the kinds' order, as the generated table has them.
    if let Some(k) = g.keys().find(|k| {
        !dwell_world::ground::KINDS
            .iter()
            .any(|g| g.name == k.as_str())
    }) {
        return Err(format!("ground: {k:?} is not a kind of ground.json"));
    }
    for k in dwell_world::ground::KINDS.iter() {
        let Some(d) = g.get(k.name) else { continue };
        match d.as_f64() {
            Some(d) if (0.0..=3.0).contains(&d) => out.push((k.name, d)),
            _ => return Err(format!("ground.{} must be in [0, 3]", k.name)),
        }
    }
    Ok(Some(Box::leak(out.into_boxed_slice())))
}

/// A biome's optional `arches` (ADR 0074 B8; ranges as `shared/worldgen/biomes.mjs` `ARCHES`);
/// density 0 is none.
fn arches(v: &Value) -> Result<Option<crate::arches::Arches>, String> {
    let Some(a) = v.get("arches") else {
        return Ok(None);
    };
    let one = |k: &str, lo: f64, hi: f64| match a.get(k).and_then(Value::as_f64) {
        Some(x) if (lo..=hi).contains(&x) => Ok(x),
        _ => Err(format!("arches.{k} must be in [{lo}, {hi}]")),
    };
    let pair = |k: &str, lo: f64, hi: f64| {
        let r = a.get(k).and_then(Value::as_array).and_then(|r| {
            let (x, y) = (r.first()?.as_f64()?, r.get(1)?.as_f64()?);
            (r.len() == 2 && lo <= x && x <= y && y <= hi).then_some([x, y])
        });
        r.ok_or_else(|| format!("arches.{k} must be [min, max] in [{lo}, {hi}], min <= max"))
    };
    let out = crate::arches::Arches {
        density: one("density", 0.0, crate::arches::DENSITY_MAX)?,
        span: pair("span", 2.0, 64.0)?,
        rise: pair("rise", 1.0, 48.0)?,
        lintel: pair("lintel", 1.0, 32.0)?,
        fin: one("fin", 2.0, 64.0)?,
        overhangs: match a.get("overhangs") {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("arches.overhangs must be true or false".into()),
        },
    };
    Ok((out.density > 0.0).then_some(out))
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
    // Tectonics (systemic terrain T5): terrain.json `tectonics`, any key left out as the build's.
    if let Some(t) = v.get("tectonics") {
        let k = &mut h.tectonics;
        let f = |key: &str| t.get(key).and_then(Value::as_f64);
        let whole = |slot: &mut u32, key: &str| {
            if let Some(x) = f(key) {
                *slot = x.max(0.0) as u32;
            }
        };
        whole(&mut k.kind, "kind");
        whole(&mut k.arc_count, "arcCount");
        whole(&mut k.hotspot_count, "hotspotCount");
        whole(&mut k.ridge_count, "ridgeCount");
        whole(&mut k.vent_count, "ventCount");
        for (key, slot) in [
            ("angle", &mut k.angle),
            ("offset", &mut k.offset),
            ("width", &mut k.width),
            ("rangeHeight", &mut k.range_height),
            ("trenchDepth", &mut k.trench_depth),
            ("riftDepth", &mut k.rift_depth),
            ("shift", &mut k.shift),
            ("arcDistance", &mut k.arc_distance),
            ("arcSpacing", &mut k.arc_spacing),
            ("coneHeight", &mut k.cone_height),
            ("coneRadius", &mut k.cone_radius),
            ("craterRadius", &mut k.crater_radius),
            ("craterDepth", &mut k.crater_depth),
            ("coneHardness", &mut k.cone_hardness),
            ("ridgeDepth", &mut k.ridge_depth),
            ("ridgeWarp", &mut k.ridge_warp),
            ("summitOffset", &mut k.summit_offset),
            ("summitAngle", &mut k.summit_angle),
            ("shoulderHeight", &mut k.shoulder_height),
            ("shoulderDistance", &mut k.shoulder_distance),
            ("shoulderAngle", &mut k.shoulder_angle),
            ("shoulderRadius", &mut k.shoulder_radius),
            ("ventHeight", &mut k.vent_height),
            ("ventRadius", &mut k.vent_radius),
            ("ventDistance", &mut k.vent_distance),
            ("iceAbove", &mut k.ice_above),
            ("iceDome", &mut k.ice_dome),
            ("snowBelowC", &mut k.snow_below_c),
            ("snowMaxSlope", &mut k.snow_max_slope),
            ("hotspotX", &mut k.hotspot_x),
            ("hotspotZ", &mut k.hotspot_z),
            ("hotspotHeading", &mut k.hotspot_heading),
            ("hotspotSpacing", &mut k.hotspot_spacing),
            ("hotspotHeight", &mut k.hotspot_height),
            ("hotspotRadius", &mut k.hotspot_radius),
            ("hotspotDecay", &mut k.hotspot_decay),
            ("hotspotWiden", &mut k.hotspot_widen),
            ("coreDistance", &mut k.core_distance),
        ] {
            if let Some(x) = f(key) {
                *slot = x;
            }
        }
        if !(k.kind <= 3
            && k.arc_count <= 32
            && k.hotspot_count <= 32
            && k.width > 0.0
            && k.arc_spacing > 0.0
            && k.cone_radius > 0.0
            && (0.0..=1.0).contains(&k.cone_hardness)
            && k.ridge_count <= 64
            && k.vent_count <= 16
            && k.shoulder_radius > 0.0
            && k.vent_radius > 0.0
            && k.hotspot_spacing > 0.0
            && k.hotspot_radius > 0.0)
        {
            return Err("world: a tectonics knob is out of range".into());
        }
    }
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
        erosion: erosion_from_json(v.get("erosion"))?,
        // The tectonic provinces' stacks (T5); absent (an edit stored before T5): the build's.
        volcanic_stack: v
            .get("tectonic")
            .and_then(|t| t.get("volcanic"))
            .and_then(Value::as_str)
            .map_or(GEOLOGY.volcanic_stack, leak),
        core_stack: v
            .get("tectonic")
            .and_then(|t| t.get("core"))
            .and_then(Value::as_str)
            .map_or(GEOLOGY.core_stack, leak),
        snow_block: v
            .get("tectonic")
            .and_then(|t| t.get("snow"))
            .and_then(Value::as_str)
            .map_or(GEOLOGY.snow_block, leak),
    })
}

/// The erosion (rocks.json `erosion`, systemic terrain T4); absent (an edit stored before T4):
/// the build's.
fn erosion_from_json(v: Option<&Value>) -> Result<Erosion, String> {
    let Some(v) = v else {
        return Ok(GEOLOGY.erosion);
    };
    let g = |k: &str| num(v, k).map_err(|e| format!("rocks.json: erosion.{e}"));
    Ok(Erosion {
        strength: g("strength")?.clamp(0.0, 1.0),
        reach: g("reach")?.max(0.0),
        valley: g("valley")?,
        resist: g("resist")?.clamp(0.0, 1.0),
        relief: g("relief")?,
        threshold: g("threshold")?.max(1.0),
        snap: g("snap")?.clamp(0.0, 1.0),
        wall: g("wall")?.max(0.5),
        talus_width: g("talusWidth")?.max(0.0),
        talus_share: g("talusShare")?.clamp(0.0, 1.0),
        outline: g("outline")?,
        outline_wavelength: g("outlineWavelength")?.max(16.0),
        gully_wavelength: g("gullyWavelength")?.max(4.0),
        gully_depth: g("gullyDepth")?,
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
        if !crate::placements::PLACEMENTS
            .iter()
            .any(|p| p.name == placement)
        {
            return Err(kf(format!(
                "placement {placement} is not a rule of placements.json"
            )));
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
#[derive(Clone, Debug)]
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
            return Err(format!(
                "vegetation.json: rockSoils: {} is not a rock",
                r.rock
            ));
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

/// A world's worldgen config (a test world, docs/DEV_TOOLS.md "Play a test world"): its height
/// field when not the build's. `""` is the build's (`None`); a fixture world's name
/// (`test:tall_peak`, `FIXTURE_WORLDS`) is that fixture's; a JSON object is a Chisel edit, whose
/// world knobs (`world`, `height_from_json`) shape the ground. The world's record keeps the config,
/// so the world reopens on the same maps.
pub fn worldgen_height(config: &str) -> Result<Option<HeightField>, String> {
    let config = config.trim();
    if config.is_empty() {
        return Ok(None);
    }
    if config.starts_with('{') {
        let v: Value = serde_json::from_str(config).map_err(|e| format!("worldgen: {e}"))?;
        return match v.get("world") {
            Some(w) => Ok(Some(height_from_json(w)?).filter(|h| *h != HEIGHT)),
            None => Ok(None),
        };
    }
    crate::terrain::FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == config)
        .map(|(_, h)| Some(*h))
        .ok_or_else(|| format!("worldgen: no fixture world named {config}"))
}

/// A world's worldgen config parsed (ADR 0074 B7): `None` for the build's (`""`); a fixture world's
/// name (`test:tall_peak`): the build's biomes, geology and vegetation on that fixture's height
/// field; a JSON object with biomes: that Chisel edit, whole; one without (world knobs alone): the
/// build's biomes on those knobs. A test world's maps carry the text (`WorldMap::worldgen`), so every
/// generator over them is the edit's.
pub fn world_config(config: &str) -> Result<Option<Edit>, String> {
    let c = config.trim();
    if c.is_empty() {
        return Ok(None);
    }
    if c.starts_with('{') {
        let v: Value = serde_json::from_str(c).map_err(|e| format!("worldgen: {e}"))?;
        if v.get("biomes")
            .and_then(Value::as_array)
            .is_some_and(|l| !l.is_empty())
        {
            return edit_from_json(c).map(Some);
        }
    }
    Ok(worldgen_height(c)?.map(|h| Edit {
        biomes: BIOMES.to_vec(),
        map: BIOME_MAP,
        geology: GEOLOGY,
        vegetation: VEGETATION,
        height: Some(h),
    }))
}

/// A world map's worldgen config (`WorldMap::worldgen`): its text, kept and stored with the map,
/// and the edit it parses to.
#[derive(Debug)]
pub struct Worldgen {
    pub text: String,
    pub edit: Edit,
}

/// The world map cache key of face seed `seed` under worldgen `config` (`world_config`): the
/// build's key for the build's ground, else that key with the config's hash, so a test world's
/// maps never stand in for the seed's own (`-c`: maps that carry their config, from 0.52.37).
pub fn worldgen_map_key(seed: u64, config: &str) -> String {
    let key = crate::worldmap::cache::key(seed);
    match world_config(config) {
        Ok(Some(_)) => format!("{key}-c{:016x}", worldgen_hash(config)),
        _ => key,
    }
}

/// The hash of worldgen config `config` (FNV-1a of its trimmed text): the map cache key's and the
/// join's (`WorldSettings` tag `WORLDGEN`, ADR 0074 B7), so a guest checks the text it was sent.
pub fn worldgen_hash(config: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in config.trim().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The worldgen config of the world of `world_seed` as this process has its maps (face A's
/// installed or computed map, `WorldMap::worldgen`): `""` for the build's ground, or when its map
/// isn't here. A server sends it to its guests (ADR 0074 B7).
pub fn installed_worldgen(world_seed: u64) -> String {
    let seed = crate::bifacial::face_seed(world_seed, dwell_world::Face::A);
    if !crate::worldmap::cache::is_ready(seed) {
        return String::new();
    }
    crate::worldmap::cache::for_seed(seed)
        .worldgen_text()
        .to_owned()
}

/// Face seed `seed`'s world map under worldgen `config` (`worldmap::build_bytes` for the build's
/// ground; a test world's over its height field, `WorldMap::for_height`, carrying the config).
pub fn worldgen_map(seed: u64, config: &str) -> Result<Option<crate::worldmap::WorldMap>, String> {
    let Some(e) = world_config(config)? else {
        return Ok(None);
    };
    let m = match e.height {
        Some(h) => crate::worldmap::WorldMap::for_height(seed, h),
        None => crate::worldmap::WorldMap::compute(seed),
    };
    Ok(Some(m.with_worldgen(config.trim())?))
}

/// [`worldgen_map`] as bytes.
pub fn worldgen_map_bytes(seed: u64, config: &str) -> Result<Vec<u8>, String> {
    Ok(match worldgen_map(seed, config)? {
        None => crate::worldmap::build_bytes(seed),
        Some(m) => crate::worldmap::codec::encode(&m),
    })
}
