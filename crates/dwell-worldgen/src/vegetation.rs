//! Soil and vegetation from climate (systemic terrain T3, ADR 0075; data:
//! `shared/worldgen/vegetation.json`): a column's climate in °C and its moisture, its soil depth
//! and type from its rock, and each vegetation kind's envelope over them, which scales a
//! placement's chance where a biome opts in (`Biome::vegetation` > 0).
//!
//! The tree line and the dry lee are not placed: they emerge where a kind's envelope reaches 0
//! (the temperature below its least, the moisture below its least). Face-local throughout.

include!("vegetation.gen.rs");

/// A soil type (a name and its overlay colour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoilType {
    pub name: &'static str,
    pub color: u32,
}

/// The soil a rock weathers to on flat ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockSoil {
    pub rock: &'static str,
    /// Metres on flat ground.
    pub depth: f64,
    /// Its soil type (an index into `Vegetation::soil_types`).
    pub soil_type: u8,
}

/// A vegetation kind: a placement's climate envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kind {
    pub name: &'static str,
    /// The placement (`placements.json`, by name) it governs.
    pub placement: &'static str,
    /// °C, least and most.
    pub temperature: [f64; 2],
    /// Wetness 0..1, least and most.
    pub moisture: [f64; 2],
    /// The least soil depth (m).
    pub min_soil: f64,
    pub max_slope_deg: f64,
    /// The soil types it grows on (indices; empty: any).
    pub soils: &'static [u8],
    /// Each edge's fade: °C, wetness, soil m.
    pub fade: [f64; 3],
    pub density: f64,
}

/// Soil and vegetation (`vegetation.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vegetation {
    pub zero_c: f64,
    pub per_unit_c: f64,
    pub flat_slope_deg: f64,
    pub valley: f64,
    pub soil_types: &'static [SoilType],
    pub fallback: RockSoil,
    pub rock_soils: &'static [RockSoil],
    pub kinds: &'static [Kind],
}

/// A column's climate and soil, as placement reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    /// °C (with the lapse rate).
    pub temperature_c: f64,
    /// 0..1: the biome map's wetness (rivers, and the rain shadow by `rainShare`).
    pub moisture: f64,
    /// Metres of soil.
    pub soil_depth: f64,
    pub soil_type: u8,
    /// The ground's slope (degrees).
    pub slope_deg: f64,
    /// How much the climate rules here (the biomes' `vegetation`, weighted): 0 none.
    pub strength: f64,
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 inside [lo, hi] at least `fade` from either edge, easing to 0 at the edges.
fn within(v: f64, [lo, hi]: [f64; 2], fade: f64) -> f64 {
    if fade <= 0.0 {
        return f64::from(u8::from(v >= lo && v <= hi));
    }
    smooth((v - lo) / fade) * smooth((hi - v) / fade)
}

impl Vegetation {
    /// °C of the biome map's temperature.
    pub fn celsius(&self, temperature: f64) -> f64 {
        self.zero_c + self.per_unit_c * temperature
    }

    /// The soil a rock weathers to (`fallback` for a rock not listed).
    pub fn rock_soil(&self, rock: &str) -> RockSoil {
        self.rock_soils
            .iter()
            .copied()
            .find(|r| r.rock == rock)
            .unwrap_or(self.fallback)
    }

    /// The soil depth (m) and type over `rock` at a slope of `slope_deg` with `wetness`: the rock's
    /// depth thinning to 0 at `flat_slope_deg`, deepened by `valley` × the wetness.
    pub fn soil(&self, rock: &str, slope_deg: f64, wetness: f64) -> (f64, u8) {
        let r = self.rock_soil(rock);
        let flat = (1.0 - slope_deg / self.flat_slope_deg.max(1e-6)).clamp(0.0, 1.0);
        (
            r.depth * flat * (1.0 + self.valley * wetness.clamp(0.0, 1.0)),
            r.soil_type,
        )
    }

    /// The kind governing placement `name`, if any.
    pub fn kind_for(&self, placement: &str) -> Option<usize> {
        self.kinds.iter().position(|k| k.placement == placement)
    }

    /// Kind `k`'s density (0..`density`) in climate `c`, before the biome's strength.
    pub fn density(&self, k: usize, c: &Climate) -> f64 {
        let k = &self.kinds[k];
        if c.slope_deg > k.max_slope_deg {
            return 0.0;
        }
        if !k.soils.is_empty() && !k.soils.contains(&c.soil_type) {
            return 0.0;
        }
        let soil = if k.fade[2] > 0.0 {
            smooth((c.soil_depth - k.min_soil) / k.fade[2])
        } else {
            f64::from(u8::from(c.soil_depth >= k.min_soil))
        };
        k.density
            * within(c.temperature_c, k.temperature, k.fade[0])
            * within(c.moisture, k.moisture, k.fade[1])
            * soil
    }

    /// The factor on a placement's chance: 1 where the climate doesn't rule (strength 0), the
    /// kind's density where it fully does.
    pub fn factor(&self, k: usize, c: &Climate) -> f64 {
        let s = c.strength.clamp(0.0, 1.0);
        1.0 - s + s * self.density(k, c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_fades_at_its_edges() {
        // both-faces: climate arithmetic only, no face.
        let v = VEGETATION;
        let k = v.kind_for("pine_groves").unwrap();
        let c = Climate {
            temperature_c: 15.0,
            moisture: 0.8,
            soil_depth: 1.0,
            soil_type: 1,
            slope_deg: 5.0,
            strength: 1.0,
        };
        assert!(v.density(k, &c) > 0.99);
        let cold = Climate {
            temperature_c: 6.0,
            ..c
        };
        assert_eq!(v.density(k, &cold), 0.0, "above the tree line");
        let dry = Climate {
            moisture: 0.01,
            ..c
        };
        assert_eq!(v.density(k, &dry), 0.0, "too dry");
        assert_eq!(
            v.factor(k, &Climate {
                strength: 0.0,
                ..cold
            }),
            1.0,
            "strength 0 changes nothing"
        );
    }
}
