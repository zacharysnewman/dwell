//! The biome map (ADR 0074 B2): each column's place in the world's geography (its temperature,
//! wetness, elevation and distance to the coast, from the world map) and each biome's weight there
//! from how well the column fits the biome's `where` ranges, eased over their `blend` so biomes
//! meet in wide, gradual transitions. The world's height field is computed first and never read
//! back from here: biomes only choose surface blocks and blend their bounded fine relief.
//!
//! A world with one biome never builds a map (`BiomeSet::single`): its weight is 1 everywhere and
//! its columns are exactly the one biome's (the goldens).

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
}

/// The biome field of one face's map: its distance grids, built once per generator.
#[derive(Debug)]
pub struct BiomeField {
    params: BiomeMap,
    n: usize,
    cell: f64,
    x0: f64,
    /// Per map cell: metres to the nearest sea cell.
    coast: Vec<f32>,
    /// Per map cell: metres to the nearest river or lake cell.
    fresh: Vec<f32>,
    climate_seed: u32,
    dither_seed: u32,
    /// The climate gradient's direction (a unit vector from the seed).
    dir: (f64, f64),
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
        let (n, cell, x0) = (map.n, map.cell, map.x0);
        let coast = chamfer(n, cell, |c| map.is_sea(c));
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
        let a = f64::from(seed_word(world_seed, 0xB10E_0001)) / f64::from(u32::MAX)
            * std::f64::consts::TAU;
        Self {
            params: BIOME_MAP,
            n,
            cell,
            x0,
            coast,
            fresh,
            climate_seed: seed_word(world_seed, 0xB10E_0002),
            dither_seed: seed_word(world_seed, 0xB10E_0003),
            dir: (a.cos(), a.sin()),
        }
    }

    /// A grid's value at (x, z), bilinear between the cells' centres.
    fn sample(&self, grid: &[f32], x: f64, z: f64) -> f64 {
        let u = ((x - self.x0) / self.cell).clamp(0.0, (self.n - 1) as f64);
        let v = ((z - self.x0) / self.cell).clamp(0.0, (self.n - 1) as f64);
        let (i, j) = (u.floor() as usize, v.floor() as usize);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let (fu, fv) = (u - i as f64, v - j as f64);
        let g = |i: usize, j: usize| f64::from(grid[j * self.n + i]);
        let a = g(i, j) + (g(i1, j) - g(i, j)) * fu;
        let b = g(i, j1) + (g(i1, j1) - g(i, j1)) * fu;
        a + (b - a) * fv
    }

    /// The geography at face-local (x, z), whose world height field is `elevation` m.
    pub fn geography(&self, x: i64, z: i64, elevation: f64) -> Geography {
        let p = &self.params;
        let (xf, zf) = (x as f64, z as f64);
        let r = f64::from(dwell_world::WORLD_RADIUS);
        let along = ((xf * self.dir.0 + zf * self.dir.1) / r).clamp(-1.0, 1.0);
        let climate = 0.5
            + 0.5 * along
            + 2.0
                * p.climate_noise
                * f64::from(fbm2(self.climate_seed, x, z, p.climate_wavelength, 2, None));
        let temperature = climate - p.lapse_rate * elevation.max(0.0) / 1000.0;
        let fresh = self.sample(&self.fresh, xf, zf);
        let t = (fresh / p.wet_reach).clamp(0.0, 1.0);
        Geography {
            temperature,
            wetness: 1.0 - t * t * (3.0 - 2.0 * t),
            elevation,
            coast: self.sample(&self.coast, xf, zf),
        }
    }

    /// The biomes' weights at (x, z) (`biomes[0]` the world's default, which takes a column no
    /// biome fits), and the one whose surface the column takes: the cumulative weights against a
    /// threshold dithered by a noise and a per-column hash, so a border is a ragged, gradual
    /// mingling, never a line.
    pub fn mix(&self, biomes: &[Biome], x: i64, z: i64, elevation: f64) -> Mix {
        let g = self.geography(x, z, elevation);
        let mut ws: Vec<(u8, f64)> = biomes
            .iter()
            .enumerate()
            .map(|(i, b)| (i as u8, raw_weight(b, &g)))
            .filter(|&(_, w)| w > 0.0)
            .collect();
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
        let mut acc = 0.0;
        mix.chosen = mix.weights[usize::from(mix.len) - 1].0;
        for &(i, w) in &mix.weights[..usize::from(mix.len)] {
            acc += f64::from(w);
            if u < acc {
                mix.chosen = i;
                break;
            }
        }
        mix
    }
}
