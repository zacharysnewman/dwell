//! The Realistic world's climate (docs/proposals/REALISTIC_WORLD.md R3, `shared/worldgen/climate.json`
//! generated into `climate.gen.rs`; owner 2026-11-11: "latitude + prevailing wind + terrain"). Read only
//! where `terrain.json` `tectonics.climate` is 1 (`worlds/realistic.json`).
//!
//! - **Latitude:** each face's seed draws a band (by weight) and turns a north axis; the band spans the
//!   disc north–south, its south latitude at the disc's south edge and its north latitude at the north
//!   edge (game scale), so a column's latitude is a function of where it is.
//! - **Temperature** (°C): the band's sea-level curve at the column's latitude less the lapse rate per
//!   1000 m above the sea. A function: no grid.
//! - **Prevailing wind:** the wind cell (trade winds, westerlies, polar easterlies, by latitude) at the
//!   band's middle latitude, its compass bearing turned with the map's north and jittered by the seed.
//! - **Rain:** the wind marched over the seed's mountains ([`crate::biomemap::rainfall`], systemic
//!   terrain T1: rain on the windward slopes, a shadow in the lee) on a coarse grid ([`CoarseGround`],
//!   ≥ 500 m cells and at most `maxCells` a side, so a bigger map coarsens it instead of growing it:
//!   BIG_MAP.md, BUGS.md #70), read bilinearly. The hydrology marches it over the uplifted ground before
//!   its erosion and weights the discharge by it ([`RainGrid::discharge`]), so the rivers carry the
//!   climate's rain. The grid lives only while the map builds.
//!
//! Face-local throughout: each face draws its own band, north and wind from its face seed (§7 q4), and
//! the march runs on the face's own map.

use dwell_world::Face;

use crate::bifacial::face_seed;
use crate::biomemap::rainfall;
use crate::noise::{mix32, seed_word};
use crate::terrain::BIOME_MAP;

/// A latitude band a face may draw (`climate.json` `bands`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub name: &'static str,
    pub weight: f64,
    /// Degrees north at the map's south and north edges.
    pub latitude: [f64; 2],
    /// The annual rain (mm) where the march rains its flat rate (flat land by the sea; R4).
    pub rain_mm: f64,
}

/// A prevailing wind cell (`climate.json` `winds`): the latitudes it covers and the compass bearing
/// it blows from (0 from the north, 90 from the east).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindCell {
    pub name: &'static str,
    pub latitude: [f64; 2],
    pub from: f64,
}

/// The rain march's tunables (`climate.json` `rain`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RainRules {
    pub cell: f64,
    pub max_cells: usize,
    pub smooth_m: f64,
    pub reach_m: f64,
    pub base_rain: f64,
    pub uplift_rain: f64,
    pub lee_dry: f64,
    /// The biomes' rain: the marched rain low-passed over about this many m (BUGS.md #158: the
    /// march's rain is near all or nothing cell to cell, and read bilinearly the biomes' edges ran
    /// along the grid's rows and columns, km-long straight edges; 0: as marched). The rivers'
    /// discharge reads the march as it is.
    pub blur_m: f64,
}

/// How the biomes read the rain (`climate.json` `biomes`, R4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeRain {
    /// The most annual rain (mm) a column reads.
    pub max_rain_mm: f64,
    /// The annual rain (mm) that is fully wet for the 0..1 wetness.
    pub wet_mm: f64,
    /// The rain's share of the wetness (the rest the rivers' and lakes').
    pub moisture_share: f64,
}

/// A noise of the climate's variety (`climate.json` `variety`): ±`amplitude` at `wavelength` m.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VarietyNoise {
    pub amplitude: f64,
    pub wavelength: f64,
}

/// The climate's rules (`climate.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimateRules {
    pub bands: &'static [Band],
    /// (latitude, sea-level °C), latitudes ascending.
    pub sea_level: &'static [[f64; 2]],
    /// (latitude, the rain's factor), latitudes ascending (0.64: the subtropical dry belt under
    /// the subtropical highs, dry deserts around 20–30°).
    pub rain_by_latitude: &'static [[f64; 2]],
    /// °C per 1000 m.
    pub lapse_rate: f64,
    pub winds: &'static [WindCell],
    pub wind_jitter: f64,
    pub rain: RainRules,
    /// The coarse rain grid's bound (bytes).
    pub max_grid_bytes: usize,
    /// The discharge's rain factor is kept within [min, max].
    pub discharge: [f64; 2],
    pub biomes: BiomeRain,
    /// The rain's factors e^(amplitude × noise) and the °C offsets (`variety`, WORK_QUEUE item 15):
    /// regional districts and local patches, so one band cannot fill a face with one biome.
    pub rain_variety: &'static [VarietyNoise],
    /// Added mm after the factors: each 0..amplitude by its noise (a wetter district, never drier).
    pub rain_mm_variety: &'static [VarietyNoise],
    pub celsius_variety: &'static [VarietyNoise],
}

mod table {
    include!("climate.gen.rs");
}
pub use table::CLIMATE;

impl ClimateRules {
    /// The sea-level temperature (°C) at `latitude` degrees: straight between the curve's points,
    /// held beyond its ends.
    pub fn sea_level_c(&self, latitude: f64) -> f64 {
        curve(self.sea_level, latitude)
    }

    /// The rain's factor at `latitude` degrees (`rainByLatitude`), as [`ClimateRules::sea_level_c`].
    pub fn rain_factor(&self, latitude: f64) -> f64 {
        curve(self.rain_by_latitude, latitude)
    }
}

/// A curve of (latitude, value) points: straight between them, held beyond its ends.
fn curve(s: &[[f64; 2]], latitude: f64) -> f64 {
    if latitude <= s[0][0] {
        return s[0][1];
    }
    for w in s.windows(2) {
        let ([a, ta], [b, tb]) = (w[0], w[1]);
        if latitude <= b {
            return ta + (tb - ta) * (latitude - a) / (b - a);
        }
    }
    s[s.len() - 1][1]
}

impl ClimateRules {
    /// The wind cell covering `latitude` (the last one beyond them all).
    pub fn wind_at(&self, latitude: f64) -> usize {
        self.winds
            .iter()
            .position(|w| w.latitude[0] <= latitude && latitude < w.latitude[1])
            .unwrap_or(self.winds.len() - 1)
    }

    /// The coarse rain grid's cell (m) for a map `extent` m across: `rain.cell`, or coarser when
    /// that would need more than `rain.max_cells` a side; a whole number of the map's `fine` m cells.
    pub fn coarse_step(&self, extent: f64, fine: f64) -> usize {
        let fine_cells = (extent / fine).ceil().max(1.0) as usize + 1;
        let by_cell = (self.rain.cell / fine).round().max(1.0) as usize;
        by_cell.max(fine_cells.div_ceil(self.rain.max_cells))
    }

    /// The coarse rain grid's side for a map of `n` cells of `fine` m.
    pub fn coarse_side(&self, n: usize, fine: f64) -> usize {
        n.div_ceil(self.coarse_step((n.max(1) - 1) as f64 * fine, fine))
    }
}

/// One face's climate: its band, north axis and prevailing wind, drawn from the face seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    pub rules: ClimateRules,
    /// The index of the band in `rules.bands`.
    pub band: usize,
    /// The index of the band's wind cell in `rules.winds`.
    pub wind: usize,
    /// North: face-local degrees from +x toward +z.
    pub north: f64,
    /// The way the prevailing wind blows from: face-local degrees from +x toward +z (the biome map's
    /// `windDirection` convention).
    pub wind_from: f64,
    /// The disc's radius (m) the band spans.
    pub radius: f64,
    /// The variety noises' seed (`rules.rain_variety`, `celsius_variety`).
    pub variety_seed: u32,
}

impl Climate {
    /// Face `face`'s climate in a world of seed `world_seed`.
    pub fn for_face(world_seed: u64, face: Face) -> Self {
        Self::for_seed(face_seed(world_seed, face))
    }

    /// The climate the face seed `seed` draws from the build's rules.
    pub fn for_seed(seed: u64) -> Self {
        Self::generate(CLIMATE, seed)
    }

    /// The climate `seed` draws from `rules`.
    pub fn generate(rules: ClimateRules, seed: u64) -> Self {
        let unit = |s: u32| f64::from(mix32(seed_word(seed, s))) / 4_294_967_296.0;
        let total: f64 = rules.bands.iter().map(|b| b.weight).sum();
        let mut pick = unit(0xC11A_0001) * total;
        let mut band = rules.bands.len() - 1;
        for (i, b) in rules.bands.iter().enumerate() {
            if b.weight > 0.0 && pick < b.weight {
                band = i;
                break;
            }
            pick -= b.weight;
        }
        Self::with_band(rules, seed, band)
    }

    /// The climate of `seed` with its band set to `band` (its north and jitter still the seed's).
    pub fn with_band(rules: ClimateRules, seed: u64, band: usize) -> Self {
        let unit = |s: u32| f64::from(mix32(seed_word(seed, s))) / 4_294_967_296.0;
        let north = unit(0xC11A_0002) * 360.0;
        let [lo, hi] = rules.bands[band].latitude;
        let wind = rules.wind_at(0.5 * (lo + hi));
        let jitter = rules.wind_jitter * (2.0 * unit(0xC11A_0003) - 1.0);
        let wind_from = (north + rules.winds[wind].from + jitter).rem_euclid(360.0);
        Self {
            rules,
            band,
            wind,
            north,
            wind_from,
            radius: f64::from(dwell_world::WORLD_RADIUS),
            variety_seed: seed_word(seed, 0xC11A_0004),
        }
    }

    pub fn band(&self) -> &Band {
        &self.rules.bands[self.band]
    }

    pub fn wind_cell(&self) -> &WindCell {
        &self.rules.winds[self.wind]
    }

    /// North as a face-local unit vector (x, z).
    pub fn north_vector(&self) -> (f64, f64) {
        let a = self.north.to_radians();
        (a.cos(), a.sin())
    }

    /// The way the wind blows from, as a face-local unit vector (x, z): upwind.
    pub fn upwind(&self) -> (f64, f64) {
        let a = self.wind_from.to_radians();
        (a.cos(), a.sin())
    }

    /// The latitude (degrees north) at face-local (x, z).
    pub fn latitude(&self, x: f64, z: f64) -> f64 {
        let (nx, nz) = self.north_vector();
        let along = ((x * nx + z * nz) / self.radius).clamp(-1.0, 1.0);
        let [lo, hi] = self.band().latitude;
        lo + (hi - lo) * (0.5 + 0.5 * along)
    }

    /// The sea-level temperature (°C) at face-local (x, z).
    pub fn sea_level_c(&self, x: f64, z: f64) -> f64 {
        self.rules.sea_level_c(self.latitude(x, z))
    }

    /// The temperature (°C) at face-local (x, z), `elevation` m above the sea.
    pub fn temperature_c(&self, x: f64, z: f64, elevation: f64) -> f64 {
        self.sea_level_c(x, z) - self.rules.lapse_rate * elevation.max(0.0) / 1000.0
            + self.variety(self.rules.celsius_variety, 0, x, z)
    }

    /// The sum of the `list` noises' amplitude × noise (about ±1 each) at (x, z), stream `k`.
    fn variety(&self, list: &[VarietyNoise], k: u32, x: f64, z: f64) -> f64 {
        list.iter()
            .enumerate()
            .map(|(i, n)| {
                let seed = mix32(self.variety_seed ^ mix32(k * 16 + i as u32 + 1));
                let v = crate::noise::fbm2_turned(seed, x, z, n.wavelength, 2, 2.13, 37.0, 0.0);
                n.amplitude * (2.0 * f64::from(v)).clamp(-1.0, 1.0)
            })
            .sum()
    }

    /// The rain the wind drops over `g` ([`rainfall`] along the band's wind, `climate.json` `rain`).
    pub fn rain(&self, g: &CoarseGround) -> RainGrid {
        let r = self.rules.rain;
        let mut params = BIOME_MAP;
        params.wind_direction = self.wind_from;
        params.wind_reach = r.reach_m;
        params.base_rain = r.base_rain;
        params.uplift_rain = r.uplift_rain;
        params.lee_dry = r.lee_dry;
        params.wind_smooth_m = r.smooth_m;
        let rain = rainfall(g.n, g.cell, &g.height, &g.sea, &params).rain;
        let land: Vec<f64> = (0..rain.len())
            .filter(|&c| !g.sea[c])
            .map(|c| f64::from(rain[c]))
            .collect();
        let mean_land = if land.is_empty() {
            0.0
        } else {
            land.iter().sum::<f64>() / land.len() as f64
        };
        RainGrid {
            n: g.n,
            cell: g.cell,
            x0: g.x0,
            rain,
            mean_land,
            discharge: self.rules.discharge,
            flat: (r.base_rain * 2.0 * g.cell / 1000.0).clamp(0.0, 1.0),
        }
    }
}

/// A map's ground on the coarse rain grid: each coarse cell the mean of its `step` × `step` map
/// cells' land height (m above the sea, ≥ 0), sea where most of them are.
#[derive(Clone, Debug, PartialEq)]
pub struct CoarseGround {
    pub n: usize,
    pub cell: f64,
    /// The first coarse cell's centre (face-local m, both axes).
    pub x0: f64,
    pub height: Vec<f32>,
    pub sea: Vec<bool>,
}

impl CoarseGround {
    /// The coarse ground of a map of `n` × `n` cells of `cell` m whose first centre is at `x0`, with
    /// heights `h` (m above the sea; −∞ beyond the disc) and its sea `sea`.
    pub fn of(rules: &ClimateRules, n: usize, cell: f64, x0: f64, h: &[f64], sea: &[bool]) -> Self {
        let k = rules.coarse_step((n.max(1) - 1) as f64 * cell, cell);
        let nc = n.div_ceil(k);
        let mut height = vec![0.0f32; nc * nc];
        let mut wet = vec![true; nc * nc];
        for cj in 0..nc {
            for ci in 0..nc {
                let (mut land, mut seas, mut sum) = (0usize, 0usize, 0.0f64);
                for j in cj * k..((cj + 1) * k).min(n) {
                    for i in ci * k..((ci + 1) * k).min(n) {
                        let c = j * n + i;
                        if sea[c] || !h[c].is_finite() {
                            seas += 1;
                        } else {
                            land += 1;
                            sum += h[c].max(0.0);
                        }
                    }
                }
                let c = cj * nc + ci;
                if land > seas {
                    wet[c] = false;
                    height[c] = (sum / land as f64) as f32;
                }
            }
        }
        Self {
            n: nc,
            cell: k as f64 * cell,
            x0: x0 + 0.5 * (k as f64 - 1.0) * cell,
            height,
            sea: wet,
        }
    }

    /// The bytes its grids hold.
    pub fn bytes(&self) -> usize {
        crate::vec_bytes(&self.height) + crate::vec_bytes(&self.sea)
    }
}

/// The rain on the coarse grid (0..1 per cell, [`rainfall`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RainGrid {
    pub n: usize,
    pub cell: f64,
    pub x0: f64,
    pub rain: Vec<f32>,
    /// The mean rain over the grid's land cells.
    pub mean_land: f64,
    discharge: [f64; 2],
    /// The march's flat rate per cell (the rain over the sea and flat land at full moisture).
    pub flat: f64,
}

impl RainGrid {
    /// The grid low-passed over about `width` m (`rain.blurM`, the biomes' rain).
    pub fn blurred(mut self, width: f64) -> Self {
        self.rain = crate::biomemap::smooth_ground(self.n, self.cell, &self.rain, width);
        self
    }

    /// The rain at face-local (x, z), bilinear between the cells' centres.
    pub fn at(&self, x: f64, z: f64) -> f64 {
        let last = (self.n - 1) as f64;
        let u = ((x - self.x0) / self.cell).clamp(0.0, last);
        let v = ((z - self.x0) / self.cell).clamp(0.0, last);
        let (i, j) = (u.floor() as usize, v.floor() as usize);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let (fu, fv) = (u - i as f64, v - j as f64);
        let g = |i: usize, j: usize| f64::from(self.rain[j * self.n + i]);
        let a = g(i, j) + (g(i1, j) - g(i, j)) * fu;
        let b = g(i, j1) + (g(i1, j1) - g(i, j1)) * fu;
        a + (b - a) * fv
    }

    /// The discharge's rain factor at (x, z): the rain over its mean on land, within
    /// `climate.json` `discharge` (1 where there is no land rain at all).
    pub fn discharge(&self, x: f64, z: f64) -> f64 {
        if self.mean_land <= 1e-12 {
            return 1.0;
        }
        (self.at(x, z) / self.mean_land).clamp(self.discharge[0], self.discharge[1])
    }

    /// The bytes it holds.
    pub fn bytes(&self) -> usize {
        crate::vec_bytes(&self.rain)
    }
}

impl Climate {
    /// The annual rain (mm) at face-local (x, z) on `rain` (R4, Whittaker-style): the band's
    /// `rainMm` × the march's rain over its flat rate, within [0, `biomes.maxRainMm`].
    pub fn rain_mm(&self, rain: &RainGrid, x: f64, z: f64) -> f64 {
        if rain.flat <= 1e-12 {
            return 0.0;
        }
        let factor = self.variety(self.rules.rain_variety, 1, x, z).exp();
        // The added rain is one-sided (0..amplitude): a lee's wetter districts, never a drier one.
        let add = 0.5
            * (self.variety(self.rules.rain_mm_variety, 2, x, z)
                + self
                    .rules
                    .rain_mm_variety
                    .iter()
                    .map(|n| n.amplitude)
                    .sum::<f64>());
        let belt = self.rules.rain_factor(self.latitude(x, z));
        (self.band().rain_mm * belt * factor * rain.at(x, z) / rain.flat + add)
            .clamp(0.0, self.rules.biomes.max_rain_mm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: ClimateRules = CLIMATE;

    fn band(name: &str) -> usize {
        RULES.bands.iter().position(|b| b.name == name).unwrap()
    }

    #[test]
    fn the_same_seed_draws_the_same_climate_and_seeds_differ() {
        assert_eq!(Climate::for_seed(7), Climate::for_seed(7));
        let all: Vec<Climate> = (0..64).map(Climate::for_seed).collect();
        let bands: std::collections::BTreeSet<usize> = all.iter().map(|c| c.band).collect();
        assert_eq!(bands.len(), RULES.bands.len(), "64 seeds draw every band");
        assert!(all.windows(2).any(|w| w[0].north != w[1].north));
    }

    #[test]
    fn a_colder_band_is_colder_at_sea_level() {
        let seed = 42;
        let warm = Climate::with_band(RULES, seed, band("subtropical"));
        let mid = Climate::with_band(RULES, seed, band("temperate"));
        let cold = Climate::with_band(RULES, seed, band("subarctic"));
        for (x, z) in [(0.0, 0.0), (20_000.0, -5_000.0), (-30_000.0, 12_000.0)] {
            let (a, b, c) = (
                warm.sea_level_c(x, z),
                mid.sea_level_c(x, z),
                cold.sea_level_c(x, z),
            );
            assert!(a > b && b > c, "{a} > {b} > {c} °C at ({x}, {z})");
        }
    }

    #[test]
    fn the_band_spans_the_map_north_to_south_and_height_cools() {
        let c = Climate::for_seed(3);
        let (nx, nz) = c.north_vector();
        let r = c.radius;
        let [lo, hi] = c.band().latitude;
        assert!((c.latitude(-nx * r, -nz * r) - lo).abs() < 1e-9);
        assert!((c.latitude(nx * r, nz * r) - hi).abs() < 1e-9);
        assert!(c.sea_level_c(nx * r, nz * r) < c.sea_level_c(-nx * r, -nz * r));
        let sea = c.temperature_c(0.0, 0.0, 0.0);
        let high = c.temperature_c(0.0, 0.0, 2000.0);
        assert!((sea - high - 2.0 * RULES.lapse_rate).abs() < 1e-9);
    }

    #[test]
    fn each_band_takes_its_latitudes_wind() {
        for (name, wind) in [
            ("subtropical", "trade_winds"),
            ("temperate", "westerlies"),
            ("subarctic", "polar_easterlies"),
        ] {
            let c = Climate::with_band(RULES, 9, band(name));
            assert_eq!(c.wind_cell().name, wind);
            let off = (c.wind_from - c.north - c.wind_cell().from).rem_euclid(360.0);
            let off = if off > 180.0 { off - 360.0 } else { off };
            assert!(off.abs() <= RULES.wind_jitter + 1e-9, "{name}: {off}°");
        }
    }

    #[test]
    fn a_band_added_by_data_alone_is_drawn() {
        static BANDS: [Band; 1] = [Band {
            name: "polar",
            weight: 1.0,
            latitude: [75.0, 85.0],
            rain_mm: 200.0,
        }];
        let rules = ClimateRules {
            bands: &BANDS,
            ..RULES
        };
        let c = Climate::generate(rules, 5);
        assert_eq!(c.band().name, "polar");
        assert_eq!(c.wind_cell().name, "polar_easterlies");
        assert!(
            c.sea_level_c(0.0, 0.0)
                < Climate::with_band(RULES, 5, band("subarctic")).sea_level_c(0.0, 0.0)
        );
    }

    /// A ridge across the wind on a coarse grid: the windward slope wetter than the lee.
    #[test]
    fn a_ridge_across_the_wind_is_wet_windward_and_dry_in_its_lee() {
        let mut c = Climate::for_seed(1);
        c.wind_from = 180.0; // from −x: blowing toward +x
        let n = 120;
        let cell = 100.0;
        let x0 = -(n as f64 - 1.0) * cell / 2.0;
        let mut h = vec![0.0; n * n];
        let mut sea = vec![false; n * n];
        for j in 0..n {
            for i in 0..n {
                let x = x0 + i as f64 * cell;
                h[j * n + i] = 1500.0 * (-(x / 1500.0).powi(2)).exp() + 50.0;
                sea[j * n + i] = i < 4 || i >= n - 4 || j < 4 || j >= n - 4;
            }
        }
        let g = CoarseGround::of(&RULES, n, cell, x0, &h, &sea);
        let r = c.rain(&g);
        let wind = r.at(-2500.0, 0.0);
        let lee = r.at(2500.0, 0.0);
        assert!(wind >= 2.0 * lee, "windward {wind} lee {lee}");
        assert!(r.discharge(-2500.0, 0.0) > r.discharge(2500.0, 0.0));
    }

    #[test]
    fn the_coarse_grid_keeps_its_bound_at_any_map_size() {
        // The map's grid at 100 m for this disc, then a 10× disc (BIG_MAP.md).
        let fine = 100.0;
        for radius in [
            f64::from(dwell_world::WORLD_RADIUS),
            10.0 * f64::from(dwell_world::WORLD_RADIUS),
        ] {
            let n = (2.0 * radius / fine).ceil() as usize + 2;
            let side = RULES.coarse_side(n, fine);
            assert!(
                side <= RULES.rain.max_cells,
                "{side} > {}",
                RULES.rain.max_cells
            );
            assert!(side * side * 4 <= RULES.max_grid_bytes);
            let step = RULES.coarse_step((n - 1) as f64 * fine, fine) as f64 * fine;
            assert!(
                step >= RULES.rain.cell,
                "cells ≥ {} m: {step}",
                RULES.rain.cell
            );
        }
    }
}
