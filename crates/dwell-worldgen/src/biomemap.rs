//! The biome map (ADR 0074 B2): each column's place in the world's geography (its temperature,
//! wetness, elevation and distance to the coast, from the world map) and each biome's weight there
//! from how well the column fits the biome's `where` ranges, eased over their `blend` so biomes
//! meet in wide, gradual transitions. The world's height field is computed first and never read
//! back from here: biomes only choose surface blocks and blend their bounded fine relief.
//!
//! The wetness is the rivers' and lakes' blended with the rain shadow's rainfall ([`rainfall`],
//! systemic terrain T1): a prevailing wind marched across the map, carrying moisture from the sea
//! and dropping it where the ground rises, so the lee of high ground is dry. `rainShare` 0 (the
//! default world's) leaves the wetness the rivers' alone.
//!
//! A world with one biome never builds a map (`BiomeSet::single`): its weight is 1 everywhere and
//! its columns are exactly the one biome's (the goldens).

use crate::grid::{SparseGrid, most_common};
use crate::noise::{fbm2, hash2, seed_word};
use crate::terrain::{BIOME_MAP, Biome, BiomeMap, WhereRange};
use crate::worldmap::{NO_LAKE, WorldMap};

/// The most biomes a column blends (the heaviest ones; the rest are dropped and the weights
/// renormalised).
pub const MAX_BLEND: usize = 4;

/// A column's geography, the inputs a biome's `where` ranges are read against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geography {
    /// 0..1-ish: the island's climate gradient less the lapse rate with height.
    pub temperature: f64,
    /// 0..1: 1 at a river or lake, easing to 0 `wet_reach` m away.
    pub wetness: f64,
    /// Metres above the sea (the world height field's, without any biome's relief).
    pub elevation: f64,
    /// Metres to the sea (map cells' distance; 0 at sea).
    pub coast: f64,
    /// °C: the climate's where the world has one, else `temperature` by `vegetation.json`'s scale.
    pub celsius: f64,
    /// Annual rain (mm), only where the world has a climate.
    pub rain: Option<f64>,
}

/// A column's biome weights: up to `MAX_BLEND` (biome index, weight), summing to 1, and the
/// biome whose surface it takes (`chosen`: the weights dithered so borders aren't lines).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub weights: [(u8, f32); MAX_BLEND],
    pub len: u8,
    pub chosen: u8,
}

impl Mix {
    /// All of biome `i`.
    pub const fn only(i: u8) -> Self {
        Self {
            weights: [(i, 1.0), (0, 0.0), (0, 0.0), (0, 0.0)],
            len: 1,
            chosen: i,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.weights[..usize::from(self.len)]
            .iter()
            .map(|&(i, w)| (usize::from(i), f64::from(w)))
    }

    /// Biome `i`'s weight (0 when it isn't in the mix).
    pub fn weight(&self, i: usize) -> f64 {
        self.iter().find(|&(j, _)| j == i).map_or(0.0, |(_, w)| w)
    }
}

/// How well `v` fits `r`: 1 inside, easing to 0 over ± `blend` at each bound (smoothstep).
pub fn fit(r: &WhereRange, v: f64) -> f64 {
    let edge = |e0: f64, e1: f64| {
        if e1 <= e0 {
            return if v >= e0 { 1.0 } else { 0.0 };
        }
        let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let lo = if r.min.is_finite() {
        edge(r.min - r.blend, r.min + r.blend)
    } else {
        1.0
    };
    let hi = if r.max.is_finite() {
        1.0 - edge(r.max - r.blend, r.max + r.blend)
    } else {
        1.0
    };
    lo * hi
}

/// Each biome's raw weight at `g`: `where.weight` × the fit of every range.
pub fn raw_weight(b: &Biome, g: &Geography) -> f64 {
    let p = &b.place;
    p.weight
        * fit(&p.temperature, g.temperature)
        * fit(&p.wetness, g.wetness)
        * fit(&p.elevation, g.elevation)
        * fit(&p.coast, g.coast)
        * fit(&p.celsius, g.celsius)
        * g.rain.map_or(1.0, |r| fit(&p.rain, r))
}

/// Every biome's weight at `g` (ADR 0074 B3): a biome other than the default (`biomes[0]`) takes
/// its raw weight ([`raw_weight`]: how well `g` fits its `where`, × its `weight`); the default is a
/// fallback, taking only what is left of 1 (× its own raw weight), so a biome that fits a place
/// takes it at an ordinary weight of 1 and the default fills the rest.
pub fn fallback_weights(biomes: &[Biome], g: &Geography) -> Vec<(u8, f64)> {
    let mut ws: Vec<(u8, f64)> = biomes
        .iter()
        .enumerate()
        .map(|(i, b)| (i as u8, raw_weight(b, g)))
        .collect();
    if let Some((first, rest)) = ws.split_first_mut() {
        let others: f64 = rest.iter().map(|w| w.1).sum();
        first.1 *= (1.0 - others).max(0.0);
    }
    ws
}

/// The biome field of one face's map: its distance grids, built once per generator.
#[derive(Debug)]
pub struct BiomeField {
    params: BiomeMap,
    n: usize,
    cell: f64,
    x0: f64,
    /// Per map cell: metres to the nearest sea cell (0 over the sea: stored by its exceptions,
    /// `grid::SparseGrid`, BUGS.md #70).
    coast: SparseGrid<f32>,
    /// Per map cell: metres to the nearest river or lake cell.
    fresh: Vec<f32>,
    climate_seed: u32,
    dither_seed: u32,
    /// The climate gradient's direction (a unit vector from the seed).
    dir: (f64, f64),
    /// The world's disc radius (m; its map's size's): the climate's gradient spans it.
    radius: f64,
    /// Per map cell: the rain shadow's rainfall (0..1, [`rainfall`]; the sea's flat rate over the
    /// sea, stored by its exceptions).
    rain: SparseGrid<f32>,
    /// The world's climate and its rain marched over the eroded map (coarse), if it has one (R4).
    climate: Option<(crate::climate::Climate, crate::climate::RainGrid)>,
}

/// The 0..1 temperature's °C (`vegetation.json` `climate`), and back.
fn celsius_of(t: f64) -> f64 {
    let v = crate::vegetation::VEGETATION;
    v.zero_c + v.per_unit_c * t
}

fn unit_of(c: f64) -> f64 {
    let v = crate::vegetation::VEGETATION;
    (c - v.zero_c) / v.per_unit_c
}

/// The rain shadow on a map's grid ([`rainfall`]): per cell, the rain that falls (0..1) and the
/// moisture the air still carries after it (0..1).
#[derive(Clone, Debug, PartialEq)]
pub struct Rain {
    pub rain: Vec<f32>,
    pub moisture: Vec<f32>,
}

/// The rain shadow (systemic terrain T1) on an `n` × `n` grid of `cell` m whose ground is
/// `height` m (the sea where `sea` marks it): the prevailing wind (`params.wind_direction`, the
/// way it blows from) is marched across the grid, upwind cells first, each cell taking the air of
/// the point two cells upwind (bilinear, so every corner it reads is already done; off the grid,
/// the open sea's). Over the sea the air is refilled to 1 and rains the flat rate; over land the
/// rain is the air's moisture × an intensity (`base_rain` per km, more by `uplift_rain` per 10 %
/// grade the ground rises along the wind, less by `lee_dry` per 10 % grade it falls; within 0..1),
/// and the air loses that rain × the step over `wind_reach`, so its moisture only falls inland:
/// what rained on a windward slope is gone behind it. The wind sees the ground (the sea's at 0)
/// low-passed over `wind_smooth_m` m ([`smooth_ground`]), so a whole range lifts and shadows the
/// air, not each hillside. One pass, O(n² log n) for the order.
pub fn rainfall(n: usize, cell: f64, height: &[f32], sea: &[bool], params: &BiomeMap) -> Rain {
    let a = params.wind_direction.to_radians();
    let up = (a.cos(), a.sin());
    // Downwind is -up: cells by their projection on it, upwind first.
    let key = |c: u32| {
        let (i, j) = ((c as usize % n) as f64, (c as usize / n) as f64);
        -(i * up.0 + j * up.1)
    };
    let mut order: Vec<u32> = (0..(n * n) as u32).collect();
    order.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
    let raw: Vec<f32> = (0..n * n)
        .map(|c| if sea[c] { 0.0 } else { height[c].max(0.0) })
        .collect();
    let smoothed = smooth_ground(n, cell, &raw, params.wind_smooth_m);
    let ground = |c: usize| f64::from(smoothed[c]);
    let mut moisture = vec![1.0f32; n * n];
    let mut rain = vec![0.0f32; n * n];
    let step = 2.0 * cell;
    let dry = (step / params.wind_reach.max(1e-9)).min(1.0);
    let flat = (params.base_rain * step / 1000.0).clamp(0.0, 1.0);
    let last = (n - 1) as f64;
    for &c in &order {
        let c = c as usize;
        if sea[c] {
            moisture[c] = 1.0;
            rain[c] = flat as f32;
            continue;
        }
        let (u, v) = ((c % n) as f64 + 2.0 * up.0, (c / n) as f64 + 2.0 * up.1);
        let (m_up, h_up) = if u < 0.0 || v < 0.0 || u > last || v > last {
            (1.0, 0.0)
        } else {
            let (i, j) = (u.floor() as usize, v.floor() as usize);
            let (i1, j1) = ((i + 1).min(n - 1), (j + 1).min(n - 1));
            let (fu, fv) = (u - i as f64, v - j as f64);
            let bl = |g: &dyn Fn(usize) -> f64| {
                let a = g(j * n + i) + (g(j * n + i1) - g(j * n + i)) * fu;
                let b = g(j1 * n + i) + (g(j1 * n + i1) - g(j1 * n + i)) * fu;
                a + (b - a) * fv
            };
            (bl(&|k| f64::from(moisture[k])), bl(&ground))
        };
        let grade = (ground(c) - h_up) / step * 10.0;
        let r = (flat + params.uplift_rain * grade.max(0.0) - params.lee_dry * (-grade).max(0.0))
            .clamp(0.0, 1.0);
        let m_up = m_up.clamp(0.0, 1.0);
        rain[c] = (m_up * r) as f32;
        moisture[c] = (m_up * (1.0 - r * dry)) as f32;
    }
    Rain { rain, moisture }
}

/// `ground` on an `n` × `n` grid of `cell` m low-passed over about `width` m: two passes of a
/// separable box filter of `width` / 2 m each (a tent of `width` m), the edges clamped; a width
/// under two cells is the ground as it is.
pub fn smooth_ground(n: usize, cell: f64, ground: &[f32], width: f64) -> Vec<f32> {
    let k = (width / (4.0 * cell)).round() as usize;
    if k == 0 || n == 0 {
        return ground.to_vec();
    }
    let mut a: Vec<f64> = ground.iter().map(|&v| f64::from(v)).collect();
    let mut b = vec![0f64; n * n];
    let span = (2 * k + 1) as f64;
    // One box pass along rows (`along` 1) or columns (`along` n), a running sum, clamped edges.
    let pass = |src: &[f64], dst: &mut [f64], along: usize, across: usize| {
        for line in 0..n {
            let at = |t: usize| line * across + t * along;
            let get = |t: i64| src[at(t.clamp(0, n as i64 - 1) as usize)];
            let mut sum: f64 = (-(k as i64)..=k as i64).map(get).sum();
            for t in 0..n {
                dst[at(t)] = sum / span;
                sum += get(t as i64 + k as i64 + 1) - get(t as i64 - k as i64);
            }
        }
    };
    for _ in 0..2 {
        pass(&a, &mut b, 1, n);
        pass(&b, &mut a, n, 1);
    }
    a.into_iter().map(|v| v as f32).collect()
}

/// A two-pass chamfer distance (m) on an `n` × `n` grid of `cell` m from the cells `seed` marks.
fn chamfer(n: usize, cell: f64, seed: impl Fn(usize) -> bool) -> Vec<f32> {
    let far = f32::MAX / 4.0;
    let mut d: Vec<f32> = (0..n * n)
        .map(|c| if seed(c) { 0.0 } else { far })
        .collect();
    let (a, b) = (cell as f32, (cell * std::f64::consts::SQRT_2) as f32);
    let fwd = [(-1i64, 0i64, a), (0, -1, a), (-1, -1, b), (1, -1, b)];
    let n = n as i64;
    let at = |i: i64, j: i64| (j * n + i) as usize;
    for pass in 0..2 {
        for jj in 0..n {
            for ii in 0..n {
                let (i, j) = if pass == 0 {
                    (ii, jj)
                } else {
                    (n - 1 - ii, n - 1 - jj)
                };
                let mut best = d[at(i, j)];
                for &(di, dj, w) in &fwd {
                    let (di, dj) = if pass == 0 { (di, dj) } else { (-di, -dj) };
                    let (ni, nj) = (i + di, j + dj);
                    if ni >= 0 && nj >= 0 && ni < n && nj < n {
                        best = best.min(d[at(ni, nj)] + w);
                    }
                }
                d[at(i, j)] = best;
            }
        }
    }
    d
}

impl BiomeField {
    /// The field of `map`, for a world of seed `world_seed`.
    pub fn new(map: &WorldMap, world_seed: u64) -> Self {
        Self::with_params(map, world_seed, BIOME_MAP)
    }

    /// [`BiomeField::new`] with the biome map's geography `params` in place of the build's
    /// (`terrain.json` `biomeMap`; Chisel's edits).
    pub fn with_params(map: &WorldMap, world_seed: u64, params: BiomeMap) -> Self {
        let climate = map
            .tectonics
            .from_climate()
            .then(|| crate::climate::Climate::for_seed(map.seed));
        Self::with_climate(map, world_seed, params, climate)
    }

    /// [`BiomeField::with_params`] with `climate` in place of the world's (none: the biome map's
    /// 0..1 geography), its rain marched over `map`'s eroded ground.
    pub fn with_climate(
        map: &WorldMap,
        world_seed: u64,
        params: BiomeMap,
        climate: Option<crate::climate::Climate>,
    ) -> Self {
        let (n, cell, x0) = (map.n, map.cell, map.x0);
        let climate = climate.map(|c| {
            let h: Vec<f64> = map.height.iter().map(|&v| f64::from(v)).collect();
            let sea: Vec<bool> = (0..n * n).map(|i| map.is_sea(i)).collect();
            let g = crate::climate::CoarseGround::of(&c.rules, n, cell, x0, &h, &sea);
            let rain = c.rain(&g);
            (c, rain)
        });
        let coast = chamfer(n, cell, |c| map.is_sea(c));
        let coast = SparseGrid::from_dense(&coast, most_common(&coast));
        let mut wet = vec![false; n * n];
        for (c, &l) in map.lake.iter().enumerate() {
            wet[c] = l != NO_LAKE;
        }
        for r in &map.rivers {
            for v in &r.path {
                wet[map.cell_at(f64::from(v.x), f64::from(v.z))] = true;
            }
        }
        let fresh = chamfer(n, cell, |c| wet[c]);
        drop(wet);
        let sea: Vec<bool> = (0..n * n).map(|c| map.is_sea(c)).collect();
        let rain = rainfall(n, cell, &map.height, &sea, &params).rain;
        drop(sea);
        let rain = SparseGrid::from_dense(&rain, most_common(&rain));
        let a = f64::from(seed_word(world_seed, 0xB10E_0001)) / f64::from(u32::MAX)
            * std::f64::consts::TAU;
        Self {
            params,
            n,
            cell,
            x0,
            coast,
            fresh,
            climate_seed: seed_word(world_seed, 0xB10E_0002),
            dither_seed: seed_word(world_seed, 0xB10E_0003),
            dir: (a.cos(), a.sin()),
            radius: map.world_radius() as f64,
            rain,
            climate,
        }
    }

    /// The world's climate, if it has one (R4).
    pub fn climate(&self) -> Option<&crate::climate::Climate> {
        self.climate.as_ref().map(|c| &c.0)
    }

    /// The bytes each of its grids holds.
    pub fn grid_bytes(&self) -> Vec<(&'static str, usize)> {
        let mut v = vec![
            ("biome field coast", self.coast.bytes()),
            ("biome field fresh water", crate::vec_bytes(&self.fresh)),
            ("biome field rain", self.rain.bytes()),
        ];
        if let Some((_, r)) = &self.climate {
            v.push(("biome field climate rain", r.bytes()));
        }
        v
    }

    /// A grid's value at (x, z) (cell c's `grid(c)`), bilinear between the cells' centres.
    fn sample(&self, grid: impl Fn(usize) -> f32, x: f64, z: f64) -> f64 {
        let u = ((x - self.x0) / self.cell).clamp(0.0, (self.n - 1) as f64);
        let v = ((z - self.x0) / self.cell).clamp(0.0, (self.n - 1) as f64);
        let (i, j) = (u.floor() as usize, v.floor() as usize);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let (fu, fv) = (u - i as f64, v - j as f64);
        let g = |i: usize, j: usize| f64::from(grid(j * self.n + i));
        let a = g(i, j) + (g(i1, j) - g(i, j)) * fu;
        let b = g(i, j1) + (g(i1, j1) - g(i, j1)) * fu;
        a + (b - a) * fv
    }

    /// The rain shadow's rainfall at face-local (x, z), 0..1 ([`rainfall`]).
    pub fn rainfall(&self, x: f64, z: f64) -> f64 {
        self.sample(|c| self.rain.get(c), x, z)
    }

    /// The geography at face-local (x, z), whose world height field is `elevation` m.
    pub fn geography(&self, x: i64, z: i64, elevation: f64) -> Geography {
        let p = &self.params;
        let (xf, zf) = (x as f64, z as f64);
        let fresh = self.sample(|c| self.fresh[c], xf, zf);
        let t = (fresh / p.wet_reach).clamp(0.0, 1.0);
        let rivers = 1.0 - t * t * (3.0 - 2.0 * t);
        let coast = self.sample(|c| self.coast.get(c), xf, zf);
        if let Some((c, grid)) = &self.climate {
            let celsius = c.temperature_c(xf, zf, elevation);
            let rain = c.rain_mm(grid, xf, zf);
            let b = c.rules.biomes;
            let wet = (rain / b.wet_mm).clamp(0.0, 1.0);
            return Geography {
                temperature: unit_of(celsius),
                wetness: rivers + (wet - rivers) * b.moisture_share,
                elevation,
                coast,
                celsius,
                rain: Some(rain),
            };
        }
        let r = self.radius;
        let along = ((xf * self.dir.0 + zf * self.dir.1) / r).clamp(-1.0, 1.0);
        let climate = 0.5
            + 0.5 * along
            + 2.0
                * p.climate_noise
                * f64::from(fbm2(self.climate_seed, x, z, p.climate_wavelength, 2, None));
        let temperature = climate - p.lapse_rate * elevation.max(0.0) / 1000.0;
        let mut wetness = rivers;
        if p.rain_share > 0.0 {
            wetness += (self.rainfall(xf, zf) - wetness) * p.rain_share;
        }
        Geography {
            temperature,
            wetness,
            elevation,
            coast,
            celsius: celsius_of(temperature),
            rain: None,
        }
    }

    /// The biomes' weights at (x, z) (`biomes[0]` the world's default, which takes a column no
    /// biome fits), and the one whose surface the column takes: the cumulative weights against a
    /// threshold dithered by a noise and a per-column hash, so a border is a ragged, gradual
    /// mingling, never a line.
    pub fn mix(&self, biomes: &[Biome], x: i64, z: i64, elevation: f64) -> Mix {
        let g = self.geography(x, z, elevation);
        let mut ws = fallback_weights(biomes, &g);
        ws.retain(|&(_, w)| w > 0.0);
        ws.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ws.truncate(MAX_BLEND);
        let total: f64 = ws.iter().map(|w| w.1).sum();
        if ws.is_empty() || total <= 1e-12 {
            return Mix::only(0);
        }
        // Index order, so the dither's cumulative order is stable.
        ws.sort_by_key(|w| w.0);
        let mut mix = Mix::only(0);
        mix.len = ws.len() as u8;
        for (k, &(i, w)) in ws.iter().enumerate() {
            mix.weights[k] = (i, (w / total) as f32);
        }
        let p = &self.params;
        let hash = f64::from(hash2(self.dither_seed, x as i32, z as i32)) / f64::from(u32::MAX);
        let u = (0.5
            + p.dither
                * 2.0
                * f64::from(fbm2(
                    self.dither_seed ^ 0x5bd1_e995,
                    x,
                    z,
                    p.dither_wavelength,
                    2,
                    None,
                ))
            + 0.5 * p.dither * (hash - 0.5))
            .clamp(0.0, 1.0 - 1e-9);
        // The pick reads the weights to the power `sharpen` (1: as they are, the same arithmetic).
        let ws = &mix.weights[..usize::from(mix.len)];
        let sharp = p.sharpen != 1.0;
        let scale = if sharp {
            ws.iter()
                .map(|&(_, w)| f64::from(w).powf(p.sharpen))
                .sum::<f64>()
        } else {
            1.0
        };
        let mut acc = 0.0;
        mix.chosen = ws[ws.len() - 1].0;
        for &(i, w) in ws {
            acc += if sharp {
                f64::from(w).powf(p.sharpen) / scale
            } else {
                f64::from(w)
            };
            if u < acc {
                mix.chosen = i;
                break;
            }
        }
        mix
    }
}
