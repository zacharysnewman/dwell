//! Measures of a world map's variety in plan view (WORK_QUEUE item 15, the Realistic batch's part B;
//! owner, 2026-11-13: "horizontal slices of noise", "the same terrain almost looks tiled"): how big
//! the largest single region of one label (a rock province, a biome) is on land, how many distinct
//! labels a 2 km × 2 km block holds, the largest label's share, the interior's relief and how much
//! the map's small-scale slopes line up with the grid's 45°/90° directions (the D8 streaks). Read by
//! the Realistic world's tests (`tests/realistic_variety.rs`) and `examples/realistic_variety.rs`.

use crate::worldmap::WorldMap;

/// Labels on the land cells of a map, every `step` cells: the label's index into `names`, or none
/// over the sea.
pub struct Labels {
    /// Cells per side.
    pub n: usize,
    /// Metres per cell.
    pub cell: f64,
    pub at: Vec<Option<u16>>,
    pub names: Vec<&'static str>,
}

impl Labels {
    /// `label(x, z, height)` on every `step`-th land cell of `map`.
    pub fn on_land(
        map: &WorldMap,
        step: usize,
        mut label: impl FnMut(i64, i64, f64) -> &'static str,
    ) -> Self {
        let step = step.max(1);
        let n = map.n.div_ceil(step);
        let mut names: Vec<&'static str> = Vec::new();
        let mut at = vec![None; n * n];
        for j in 0..n {
            for i in 0..n {
                let c = (j * step) * map.n + i * step;
                let h = f64::from(map.height[c]);
                if map.is_sea(c) || !h.is_finite() {
                    continue;
                }
                let x = map.x0 + (i * step) as f64 * map.cell;
                let z = map.x0 + (j * step) as f64 * map.cell;
                let name = label(x as i64, z as i64, h.max(0.0));
                let k = names.iter().position(|&s| s == name).unwrap_or_else(|| {
                    names.push(name);
                    names.len() - 1
                });
                at[j * n + i] = Some(k as u16);
            }
        }
        Self {
            n,
            cell: map.cell * step as f64,
            at,
            names,
        }
    }

    /// The largest 4-connected region of one label, km².
    pub fn largest_region_km2(&self) -> f64 {
        self.largest_region().1
    }

    /// The label of the largest 4-connected region of one label, and its area (km²).
    pub fn largest_region(&self) -> (&'static str, f64) {
        let n = self.n;
        let mut seen = vec![false; n * n];
        let mut best = 0usize;
        let mut label = 0u16;
        let mut stack = Vec::new();
        for s in 0..n * n {
            let Some(l) = self.at[s] else { continue };
            if seen[s] {
                continue;
            }
            seen[s] = true;
            stack.push(s);
            let mut size = 0;
            while let Some(c) = stack.pop() {
                size += 1;
                let (i, j) = (c % n, c / n);
                let mut go = |d: usize| {
                    if !seen[d] && self.at[d] == Some(l) {
                        seen[d] = true;
                        stack.push(d);
                    }
                };
                if i > 0 {
                    go(c - 1);
                }
                if i + 1 < n {
                    go(c + 1);
                }
                if j > 0 {
                    go(c - n);
                }
                if j + 1 < n {
                    go(c + n);
                }
            }
            if size > best {
                (best, label) = (size, l);
            }
        }
        (
            self.names
                .get(usize::from(label))
                .copied()
                .unwrap_or("none"),
            best as f64 * self.cell * self.cell / 1e6,
        )
    }

    /// The mean count of distinct labels in the 2 km × 2 km blocks at least half land.
    pub fn distinct_per_4km2(&self) -> f64 {
        let b = (2000.0 / self.cell).round().max(1.0) as usize;
        let (mut sum, mut count) = (0usize, 0usize);
        for bj in (0..self.n).step_by(b) {
            for bi in (0..self.n).step_by(b) {
                let mut land = 0;
                let mut seen: Vec<u16> = Vec::new();
                for j in bj..(bj + b).min(self.n) {
                    for i in bi..(bi + b).min(self.n) {
                        if let Some(l) = self.at[j * self.n + i] {
                            land += 1;
                            if !seen.contains(&l) {
                                seen.push(l);
                            }
                        }
                    }
                }
                if 2 * land >= b * b {
                    sum += seen.len();
                    count += 1;
                }
            }
        }
        sum as f64 / count.max(1) as f64
    }

    /// The label with the largest share of the land, and that share (0..1).
    pub fn largest_share(&self) -> (&'static str, f64) {
        let mut counts = vec![0usize; self.names.len()];
        let mut land = 0;
        for l in self.at.iter().flatten() {
            counts[usize::from(*l)] += 1;
            land += 1;
        }
        let (k, c) = counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, c)| *c)
            .unwrap_or((0, &0));
        (
            self.names.get(k).copied().unwrap_or("none"),
            *c as f64 / f64::from(land.max(1)),
        )
    }
}

/// Per map cell: the distance (cells, 8-connected steps) to the nearest sea cell.
fn sea_distance(map: &WorldMap) -> Vec<u32> {
    let n = map.n;
    let mut d = vec![u32::MAX; n * n];
    let mut q = std::collections::VecDeque::new();
    for (c, v) in d.iter_mut().enumerate() {
        if map.is_sea(c) || !map.height[c].is_finite() {
            *v = 0;
            q.push_back(c);
        }
    }
    while let Some(c) = q.pop_front() {
        let (i, j) = ((c % n) as i64, (c / n) as i64);
        for dj in -1..=1i64 {
            for di in -1..=1i64 {
                let (a, b) = (i + di, j + dj);
                if a < 0 || b < 0 || a >= n as i64 || b >= n as i64 {
                    continue;
                }
                let e = b as usize * n + a as usize;
                if d[e] == u32::MAX {
                    d[e] = d[c] + 1;
                    q.push_back(e);
                }
            }
        }
    }
    d
}

/// The interior's mid-frequency relief (basins and swells): over the land farther than 5 km from
/// the sea, the root mean square (m) of the height's 1 km box mean minus its 8 km box mean, and
/// the 90th percentile of that difference's magnitude.
pub fn interior_relief(map: &WorldMap) -> (f64, f64) {
    let n = map.n;
    let far = (5000.0 / map.cell).round() as u32;
    let d = sea_distance(map);
    let mut sat = vec![0.0f64; (n + 1) * (n + 1)];
    for j in 0..n {
        for i in 0..n {
            let h = f64::from(map.height[j * n + i]);
            let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
            sat[(j + 1) * (n + 1) + i + 1] =
                h + sat[j * (n + 1) + i + 1] + sat[(j + 1) * (n + 1) + i] - sat[j * (n + 1) + i];
        }
    }
    let mean = |i: usize, j: usize, r: i64| {
        let (a0, b0) = (
            (i as i64 - r).max(0) as usize,
            (j as i64 - r).max(0) as usize,
        );
        let (a1, b1) = (
            (i as i64 + r + 1).min(n as i64) as usize,
            (j as i64 + r + 1).min(n as i64) as usize,
        );
        let s = sat[b1 * (n + 1) + a1] - sat[b0 * (n + 1) + a1] - sat[b1 * (n + 1) + a0]
            + sat[b0 * (n + 1) + a0];
        s / ((a1 - a0) * (b1 - b0)) as f64
    };
    let (r1, r4) = (
        (500.0 / map.cell).round() as i64,
        (4000.0 / map.cell).round() as i64,
    );
    let mut v = Vec::new();
    for j in 0..n {
        for i in 0..n {
            if d[j * n + i] >= far {
                v.push(mean(i, j, r1) - mean(i, j, r4));
            }
        }
    }
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let rms = (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
    let mut a: Vec<f64> = v.iter().map(|x| x.abs()).collect();
    a.sort_by(f64::total_cmp);
    (rms, a[(a.len() - 1) * 9 / 10])
}

/// How much the map's small-scale slopes line up with the grid's 45°/90° directions (the D8
/// streaks): the high-passed height's (minus its 700 m box mean) Sobel gradients on land, the
/// eight-fold harmonic Σ|g|²·cos 8θ / Σ|g|², 0 for slopes facing every way alike, 1 for all of them
/// along a multiple of 45°.
pub fn anisotropy(map: &WorldMap) -> f64 {
    let n = map.n;
    let r = 3i64;
    let h = |i: usize, j: usize| {
        let v = f64::from(map.height[j * n + i]);
        if v.is_finite() { v.max(0.0) } else { 0.0 }
    };
    let mut hp = vec![0.0f64; n * n];
    for j in 0..n {
        for i in 0..n {
            let (mut s, mut k) = (0.0, 0.0);
            for dj in -r..=r {
                for di in -r..=r {
                    let (a, b) = (i as i64 + di, j as i64 + dj);
                    if a >= 0 && b >= 0 && a < n as i64 && b < n as i64 {
                        s += h(a as usize, b as usize);
                        k += 1.0;
                    }
                }
            }
            hp[j * n + i] = h(i, j) - s / k;
        }
    }
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for j in 1..n - 1 {
        for i in 1..n - 1 {
            let land = (j - 1..=j + 1).all(|b| (i - 1..=i + 1).all(|a| !map.is_sea(b * n + a)));
            if !land {
                continue;
            }
            let p = |a: usize, b: usize| hp[b * n + a];
            let gx = (p(i + 1, j - 1) + 2.0 * p(i + 1, j) + p(i + 1, j + 1))
                - (p(i - 1, j - 1) + 2.0 * p(i - 1, j) + p(i - 1, j + 1));
            let gz = (p(i - 1, j + 1) + 2.0 * p(i, j + 1) + p(i + 1, j + 1))
                - (p(i - 1, j - 1) + 2.0 * p(i, j - 1) + p(i + 1, j - 1));
            let w = gx * gx + gz * gz;
            if w <= 0.0 {
                continue;
            }
            num += w * (8.0 * gz.atan2(gx)).cos();
            den += w;
        }
    }
    if den > 0.0 { num / den } else { 0.0 }
}
