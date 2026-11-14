//! Soil and vegetation from climate (systemic terrain T3, ADR 0075; data:
//! `shared/worldgen/vegetation.json`): a column's climate in °C and its moisture, its soil depth
//! and type from its rock, and each vegetation kind's envelope over them, which scales a
//! placement's chance where a biome opts in (`Biome::vegetation` > 0).
//!
//! The tree line and the dry lee are not placed: they emerge where a kind's envelope reaches 0
//! (the temperature below its least, the moisture below its least). Face-local throughout.

include!("vegetation.gen.rs");

/// A soil type (a name and its overlay colour) and the blocks it shows in a biome with
/// `soilBlocks` (T3b; "" the band's own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoilType {
    pub name: &'static str,
    pub color: u32,
    /// The ground's top block ("" the band's `ground`).
    pub ground: &'static str,
    /// The block under it ("" the band's `under`).
    pub under: &'static str,
}

impl SoilType {
    /// Whether it changes any block of a band.
    pub fn has_blocks(&self) -> bool {
        !self.ground.is_empty() || !self.under.is_empty()
    }
}

/// The soil a rock weathers to on flat ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockSoil {
    pub rock: &'static str,
    /// Metres on flat ground.
    pub depth: f64,
    /// Its soil type (an index into `Vegetation::soil_types`).
    pub soil_type: u8,
    /// The climate it holds in (R5): °C and moisture (0..1) [least, most]; ±1000 for none.
    pub celsius: [f64; 2],
    pub moisture: [f64; 2],
}

impl RockSoil {
    /// Whether it names a climate (R5).
    pub fn has_climate(&self) -> bool {
        self.celsius != NO_RANGE || self.moisture != NO_RANGE
    }

    /// Whether it holds at `celsius` °C and `moisture` (an axis it names no range for always
    /// holds; NaN, no climate known, holds only there).
    fn holds(&self, celsius: f64, moisture: f64) -> bool {
        let within = |r: [f64; 2], v: f64| r == NO_RANGE || (r[0]..=r[1]).contains(&v);
        within(self.celsius, celsius) && within(self.moisture, moisture)
    }
}

/// A [`RockSoil`] range that always holds.
pub const NO_RANGE: [f64; 2] = [-1000.0, 1000.0];

/// A vegetation kind: a climate envelope over one placement, decoration or ground item kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kind {
    pub name: &'static str,
    /// The placement (`placements.json`, by name) it governs ("" none), scaled by a biome's
    /// `vegetation`.
    pub placement: &'static str,
    /// The decoration (`decorations.json`, by name; T3b) it governs ("" none), scaled by a biome's
    /// `undergrowth`.
    pub decoration: &'static str,
    /// The ground item kind (`shared/ground` `kinds`, by name; T3b) it governs ("" none), scaled
    /// by a biome's `undergrowth`.
    pub ground: &'static str,
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

    /// The soil a rock weathers to in a climate of `celsius` °C and `moisture` (0..1): its first
    /// entry whose climate holds (R5), `fallback` for a rock not listed.
    pub fn rock_soil(&self, rock: &str, celsius: f64, moisture: f64) -> RockSoil {
        self.rock_soils
            .iter()
            .copied()
            .find(|r| r.rock == rock && r.holds(celsius, moisture))
            .unwrap_or(self.fallback)
    }

    /// Whether `rock`'s soil depends on the climate (an entry of it names one, R5).
    pub fn climate_dependent(&self, rock: &str) -> bool {
        self.rock_soils
            .iter()
            .any(|r| r.rock == rock && r.has_climate())
    }

    /// The soil depth (m) and type over `rock` at a slope of `slope_deg` with `wetness` at
    /// `celsius` °C: the rock's depth (by climate, R5) thinning to 0 at `flat_slope_deg`,
    /// deepened by `valley` × the wetness.
    pub fn soil(&self, rock: &str, slope_deg: f64, wetness: f64, celsius: f64) -> (f64, u8) {
        let r = self.rock_soil(rock, celsius, wetness);
        let flat = (1.0 - slope_deg / self.flat_slope_deg.max(1e-6)).clamp(0.0, 1.0);
        (
            r.depth * flat * (1.0 + self.valley * wetness.clamp(0.0, 1.0)),
            r.soil_type,
        )
    }

    /// The kind governing placement `name`, if any.
    pub fn kind_for(&self, placement: &str) -> Option<usize> {
        (!placement.is_empty())
            .then(|| self.kinds.iter().position(|k| k.placement == placement))
            .flatten()
    }

    /// The kind governing decoration `name` (T3b), if any.
    pub fn kind_for_decoration(&self, name: &str) -> Option<usize> {
        (!name.is_empty())
            .then(|| self.kinds.iter().position(|k| k.decoration == name))
            .flatten()
    }

    /// The kind governing ground item kind `name` (T3b), if any.
    pub fn kind_for_ground(&self, name: &str) -> Option<usize> {
        (!name.is_empty())
            .then(|| self.kinds.iter().position(|k| k.ground == name))
            .flatten()
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
            v.factor(
                k,
                &Climate {
                    strength: 0.0,
                    ..cold
                }
            ),
            1.0,
            "strength 0 changes nothing"
        );
    }
}

#[cfg(test)]
mod rock_climate_tests {
    use super::*;

    fn soil_name(v: &Vegetation, rock: &str, c: f64, m: f64) -> &'static str {
        v.soil_types[usize::from(v.rock_soil(rock, c, m).soil_type)].name
    }

    #[test]
    fn one_rock_weathers_to_a_different_soil_by_climate() {
        // R5: andesite (the arcs' rock) by vegetation.json's rockSoils.
        let v = &VEGETATION;
        assert!(v.climate_dependent("dwell:andesite"));
        assert_eq!(soil_name(v, "dwell:andesite", 18.0, 0.7), "dwell:loamy");
        assert_eq!(soil_name(v, "dwell:andesite", -4.0, 0.7), "dwell:rocky");
        assert_eq!(soil_name(v, "dwell:andesite", 18.0, 0.05), "dwell:sandy");
        // Warm and wet: the deepest soil.
        let deep = v.rock_soil("dwell:andesite", 18.0, 0.7).depth;
        let mild = v.rock_soil("dwell:andesite", 5.0, 0.3).depth;
        assert!(deep > mild, "{deep} > {mild}");
        // No climate known (NaN): the rock's own entry.
        assert_eq!(
            soil_name(v, "dwell:andesite", f64::NAN, f64::NAN),
            "dwell:loamy"
        );
        // The rocks of the default and starter worlds name no climate: their soils unchanged.
        for r in [
            "dwell:sandstone",
            "dwell:shale",
            "dwell:granite",
            "dwell:basalt",
            "dwell:limestone",
        ] {
            assert!(!v.climate_dependent(r), "{r}");
        }
    }

    #[test]
    fn a_climate_entry_added_by_data_alone_changes_a_rocks_soil() {
        let mut soils = vec![RockSoil {
            rock: "dwell:granite",
            depth: 1.5,
            soil_type: 1,
            celsius: [10.0, 40.0],
            moisture: [0.5, 1.0],
        }];
        soils.extend_from_slice(VEGETATION.rock_soils);
        let v = Vegetation {
            rock_soils: Box::leak(soils.into_boxed_slice()),
            ..VEGETATION
        };
        assert_eq!(soil_name(&v, "dwell:granite", 20.0, 0.8), "dwell:loamy");
        assert_eq!(soil_name(&v, "dwell:granite", 2.0, 0.8), "dwell:rocky");
        assert_eq!(
            soil_name(&VEGETATION, "dwell:granite", 20.0, 0.8),
            "dwell:rocky"
        );
    }
}
