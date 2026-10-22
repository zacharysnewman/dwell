//! Per-world generation settings (docs/WORLD_GENERATION.md §9, ADR 0050): the create-world
//! screen's sliders, chosen once when a world is created, stored in its file and sent to clients
//! with the seed (`Welcome::worldgen`). To change them, regenerate the world.
//!
//! The sliders are data (`shared/worldgen/settings.json`, generated into `settings.gen.rs` and the
//! client's `worldgenSettings.gen.ts` by `shared/worldgen/gen.mjs`): each drives one or more
//! generator tunables by [`Target`] — the fixed vocabulary this module applies — and [`Op`]. A
//! slider at its default changes nothing, so the default settings are exactly the data files'
//! terrain (`default_settings_are_the_data_files`).
//!
//! **The encoding** (`encode` / `decode`, shared with the client): the sliders off their defaults,
//! in file order, as `id:value` joined by `,`; a value is snapped to its slider's step and written
//! with the step's decimals, trailing zeros dropped (`land:0.7,coast:2.4`). The empty string is
//! the default. **The map hash** (`map_hash`): FNV-1a 64 of the encoding of the sliders that
//! change the world map (stage [`Stage::Map`]), 0 for none: a slot in the map's bytes and its
//! cache key, so maps of other settings are never read as these.

use std::sync::Mutex;

use dwell_world::CELL_H;

use crate::hydrology::{self, HydrologyParams};
use crate::island::IslandParams;
use crate::placements::{PLACEMENTS, Placement, Variant};
use crate::portals::{MIDPLANE, Midplane};
use crate::terrain::{SURFACE, Surface};

/// A group of sliders on the create-world screen.
#[derive(Clone, Copy, Debug)]
pub struct Category {
    pub id: &'static str,
    pub label: &'static str,
}

/// One slider (a toggle is 0–1 in steps of 1).
#[derive(Clone, Copy, Debug)]
pub struct Slider {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub toggle: bool,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// The step's decimals: the encoding writes values with these.
    pub decimals: usize,
    pub default: f64,
    pub drives: &'static [Drive],
}

/// A tunable a slider drives, and how.
#[derive(Clone, Copy, Debug)]
pub struct Drive {
    pub target: Target,
    pub op: Op,
}

/// How a slider's value `v` changes a tunable `t` (its value in the data files).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// t × v (the default is 1).
    Scale,
    /// v × factor (the default × factor is t).
    Set(f64),
    /// t + (v − the default).
    Shift,
    /// values[v] (a toggle; values[default] is t).
    Choose([f64; 2]),
}

/// The tunables sliders may drive: a fixed vocabulary (gen.mjs `TARGETS` checks the data against
/// it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The island's mean radius (m) and its coast detail's amplitude (m) (`island.rs`).
    IslandRadius,
    CoastAmplitude,
    /// The uplift layer's initial height and rate where the strength is 1 (`hydrology.json`).
    UpliftBaseHeight,
    UpliftRate,
    /// Mountain range spines and tectonic plates per face.
    RangeCount,
    PlateCount,
    /// The relative rainfall and the erodibility K.
    RainAmount,
    Erodibility,
    /// The snow line (m above sea level, `terrain.json` `bands`).
    SnowLine,
    /// Every placement's highest anchor (m), and its variants' heights (m), shifted.
    PlacementMaxHeight,
    PlacementVariantHeight,
    /// Every placement's density and its share outside groves (at most 1).
    PlacementDensity,
    PlacementGroveOutside,
    /// The portals' count range and the bedrock's thickness (m; `midplane.json`).
    PortalCountMin,
    PortalCountMax,
    BedrockThickness,
}

/// What a target changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The world map (the map's hash covers it).
    Map,
    /// The chunks' columns shaped from the map.
    Columns,
    /// The bedrock and the portals.
    Midplane,
}

impl Target {
    pub fn stage(self) -> Stage {
        use Target::*;
        match self {
            IslandRadius | CoastAmplitude | UpliftBaseHeight | UpliftRate | RangeCount
            | PlateCount | RainAmount | Erodibility => Stage::Map,
            SnowLine
            | PlacementMaxHeight
            | PlacementVariantHeight
            | PlacementDensity
            | PlacementGroveOutside => Stage::Columns,
            PortalCountMin | PortalCountMax | BedrockThickness => Stage::Midplane,
        }
    }
}

/// A named set of slider values (unnamed sliders keep their defaults).
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub values: &'static [(&'static str, f64)],
}

mod table {
    include!("settings.gen.rs");
}
pub use table::{CATEGORIES, PRESETS, SLIDERS};

/// The longest encoding a world may carry (bytes; the protocol's and the world file's limit).
pub const MAX_ENCODED_BYTES: usize = dwell_protocol::limits::WORLDGEN_SETTINGS_MAX_BYTES;

/// A slider by id.
pub fn slider(id: &str) -> Option<&'static Slider> {
    SLIDERS.iter().find(|s| s.id == id)
}

impl Slider {
    /// `v` clamped to the range and snapped to a step, as the encoding writes it.
    pub fn format(&self, v: f64) -> String {
        let v = if v.is_nan() { self.default } else { v };
        let k = ((v.clamp(self.min, self.max) - self.min) / self.step).round();
        let t = format!("{:.*}", self.decimals, self.min + k * self.step);
        if t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    }

    /// `v` snapped as the encoding stores it.
    pub fn snap(&self, v: f64) -> f64 {
        self.format(v).parse().expect("a formatted number")
    }
}

/// One world's settings: a value per slider (`SLIDERS`' order).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSettings {
    values: Vec<f64>,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            values: SLIDERS.iter().map(|s| s.default).collect(),
        }
    }
}

impl WorldSettings {
    /// A preset's settings by id.
    pub fn preset(id: &str) -> Option<Self> {
        let p = PRESETS.iter().find(|p| p.id == id)?;
        let mut s = Self::default();
        for &(k, v) in p.values {
            s.set(k, v).expect("presets are validated");
        }
        Some(s)
    }

    pub fn get(&self, id: &str) -> Option<f64> {
        SLIDERS
            .iter()
            .position(|s| s.id == id)
            .map(|i| self.values[i])
    }

    /// Sets a slider (clamped and snapped to its step).
    pub fn set(&mut self, id: &str, v: f64) -> Result<(), String> {
        let i = SLIDERS
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| format!("unknown world setting {id:?}"))?;
        self.values[i] = SLIDERS[i].snap(v);
        Ok(())
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The canonical encoding (module docs); empty for the default.
    pub fn encode(&self) -> String {
        self.encode_where(|_| true)
    }

    fn encode_where(&self, keep: impl Fn(&Slider) -> bool) -> String {
        SLIDERS
            .iter()
            .zip(&self.values)
            .filter(|(s, v)| **v != s.default && keep(s))
            .map(|(s, &v)| format!("{}:{}", s.id, s.format(v)))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Settings from their encoding (any order; values snapped). An unknown slider, a repeated
    /// one or a value that is not a number is an error: a world made by a build with other
    /// sliders is not generated by this one.
    pub fn decode(text: &str) -> Result<Self, String> {
        let mut s = Self::default();
        s.apply_encoded(text)?;
        Ok(s)
    }

    /// Sets the sliders an encoding names, keeping the others (a preset with changes: the
    /// dedicated server's `--preset` then `--worldgen`).
    pub fn apply_encoded(&mut self, text: &str) -> Result<(), String> {
        if text.len() > MAX_ENCODED_BYTES {
            return Err(format!(
                "world settings longer than {MAX_ENCODED_BYTES} bytes"
            ));
        }
        let mut seen = Vec::new();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (id, v) = part
                .split_once(':')
                .ok_or_else(|| format!("world setting {part:?} is not id:value"))?;
            let id = id.trim();
            let v: f64 = v
                .trim()
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())
                .ok_or_else(|| format!("world setting {id}: {v:?} is not a number"))?;
            if seen.contains(&id) {
                return Err(format!("world setting {id} is given twice"));
            }
            seen.push(id);
            self.set(id, v)?;
        }
        Ok(())
    }

    /// The hash of the settings that change the world map (module docs): 0 for none.
    pub fn map_hash(&self) -> u64 {
        let text = self.encode_where(|s| s.drives.iter().any(|d| d.target.stage() == Stage::Map));
        if text.is_empty() {
            return 0;
        }
        crate::worldmap::codec::checksum(text.as_bytes())
    }

    /// The generator's tunables under these settings.
    pub fn tunables(&self) -> Tunables {
        let mut t = Tunables::default();
        for (s, &v) in SLIDERS.iter().zip(&self.values) {
            if v == s.default {
                continue;
            }
            for d in s.drives {
                t.apply(d.target, |old| match d.op {
                    Op::Scale => old * v,
                    Op::Set(factor) => v * factor,
                    Op::Shift => old + (v - s.default),
                    Op::Choose(values) => values[usize::from(v != 0.0)],
                });
            }
        }
        t.map.hash = self.map_hash();
        t
    }
}

/// What the world map is computed from: the island's and the hydrology's tunables, and their
/// hash (`WorldSettings::map_hash`; 0 for the data files').
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapSettings {
    pub island: IslandParams,
    pub hydrology: HydrologyParams,
    pub hash: u64,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            island: IslandParams::DEFAULT,
            hydrology: *hydrology::default_profile(),
            hash: 0,
        }
    }
}

/// Every tunable a world's settings may change, resolved.
#[derive(Clone, Debug)]
pub struct Tunables {
    pub map: MapSettings,
    pub surface: Surface,
    pub placements: Vec<Placement>,
    pub midplane: Midplane,
}

impl Default for Tunables {
    fn default() -> Self {
        Self {
            map: MapSettings::default(),
            surface: SURFACE,
            placements: PLACEMENTS.to_vec(),
            midplane: MIDPLANE,
        }
    }
}

impl Tunables {
    /// Changes `target` to `f(its value)` (whole-number tunables rounded, shares clamped).
    fn apply(&mut self, target: Target, f: impl Fn(f64) -> f64) {
        use Target::*;
        let whole = |v: f64| v.round().max(0.0);
        let h = &mut self.map.hydrology;
        match target {
            IslandRadius => {
                self.map.island.radius = f(f64::from(self.map.island.radius)) as f32;
            }
            CoastAmplitude => {
                self.map.island.coast_amplitude =
                    f(f64::from(self.map.island.coast_amplitude)) as f32;
            }
            UpliftBaseHeight | UpliftRate | RangeCount | PlateCount => {
                let Some(t) = h.tectonics.as_mut() else {
                    return;
                };
                match target {
                    UpliftBaseHeight => t.base_height = f(t.base_height),
                    UpliftRate => t.rate = f(t.rate),
                    RangeCount => t.ranges.count = whole(f(f64::from(t.ranges.count))) as u32,
                    _ => t.plates.count = whole(f(f64::from(t.plates.count))) as u32,
                }
            }
            RainAmount => h.rainfall = f(h.rainfall),
            Erodibility => h.erodibility = f(h.erodibility),
            SnowLine => self.surface.snow_line = f(self.surface.snow_line),
            PlacementMaxHeight => {
                for p in &mut self.placements {
                    p.max_height = (f(f64::from(p.max_height)).round() as i32).max(p.min_height);
                }
            }
            PlacementVariantHeight => {
                for p in &mut self.placements {
                    let shift = (f(0.0)).round() as i32;
                    p.variants = shifted_variants(p.variants, shift);
                }
            }
            PlacementDensity => {
                for p in &mut self.placements {
                    p.density = (f(f64::from(p.density)) as f32).clamp(0.0, 1.0);
                }
            }
            PlacementGroveOutside => {
                for p in &mut self.placements {
                    p.grove_outside = (f(f64::from(p.grove_outside)) as f32).clamp(0.0, 1.0);
                }
            }
            PortalCountMin => {
                let p = &mut self.midplane.portals;
                p.count_min = whole(f(f64::from(p.count_min))) as u32;
                p.count_max = p.count_max.max(p.count_min);
            }
            PortalCountMax => {
                let p = &mut self.midplane.portals;
                p.count_max = whole(f(f64::from(p.count_max))) as u32;
                p.count_min = p.count_min.min(p.count_max);
            }
            BedrockThickness => {
                let m = f(f64::from(self.midplane.bedrock_rows) * CELL_H);
                self.midplane.bedrock_rows =
                    ((m / CELL_H).round() as i32).clamp(1, dwell_world::CORE_LAYERS);
            }
        }
    }
}

/// `variants` with their heights shifted by `shift` m: interned (one leaked slice per distinct
/// shift and variant list), as placements hold their variants by `&'static`.
fn shifted_variants(variants: &'static [Variant], shift: i32) -> &'static [Variant] {
    type Interned = Vec<((usize, i32), &'static [Variant])>;
    static INTERNED: Mutex<Interned> = Mutex::new(Vec::new());
    if shift == 0 || variants.is_empty() {
        return variants;
    }
    let key = (variants.as_ptr() as usize, shift);
    let mut interned = INTERNED.lock().expect("interned variants");
    if let Some((_, v)) = interned.iter().find(|(k, _)| *k == key) {
        return v;
    }
    let out: &'static [Variant] = Box::leak(
        variants
            .iter()
            .map(|v| Variant {
                from: v.from + shift,
                full: v.full + shift,
                ..*v
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    interned.push((key, out));
    out
}
