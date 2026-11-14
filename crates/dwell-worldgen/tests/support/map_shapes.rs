//! Shape measures over a world map's land mask (WORK_QUEUE item 15's map checks, 2026-11-14), shared
//! by `tests/realistic_shapes.rs` and `examples/realistic_shapes.rs`: thin land strips (what an
//! opening by a disc leaves), straight runs of an outline (a line fitted in a window round each
//! outline cell), and islands (connected land, their sizes, spacing and irregularity).

#![allow(dead_code)]

use std::collections::VecDeque;

/// A mask of cells on an `n` × `n` grid of `cell` m.
pub struct Mask {
    pub n: usize,
    pub cell: f64,
    pub on: Vec<bool>,
}

const D8: [(i32, i32, f64); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f64::consts::SQRT_2),
    (1, -1, std::f64::consts::SQRT_2),
    (-1, 1, std::f64::consts::SQRT_2),
    (-1, -1, std::f64::consts::SQRT_2),
];

impl Mask {
    fn neighbours(&self, c: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let (i, j) = ((c % self.n) as i32, (c / self.n) as i32);
        let n = self.n as i32;
        D8.iter().filter_map(move |&(di, dj, w)| {
            let (a, b) = (i + di, j + dj);
            (a >= 0 && b >= 0 && a < n && b < n).then(|| ((b * n + a) as usize, w))
        })
    }

    /// Each cell's distance (cells, chamfer) to the nearest cell of `from` (`f64::MAX` if none).
    fn distance_from(&self, from: &[bool]) -> Vec<f64> {
        let mut d = vec![f64::MAX; self.on.len()];
        let mut q = std::collections::BinaryHeap::new();
        for (c, &f) in from.iter().enumerate() {
            if f {
                d[c] = 0.0;
                q.push(std::cmp::Reverse((0u64, c)));
            }
        }
        while let Some(std::cmp::Reverse((k, c))) = q.pop() {
            let dc = k as f64 / 1000.0;
            if dc > d[c] {
                continue;
            }
            for (o, w) in self.neighbours(c) {
                let nd = dc + w;
                if nd < d[o] {
                    d[o] = nd;
                    q.push(std::cmp::Reverse(((nd * 1000.0) as u64, o)));
                }
            }
        }
        d
    }

    /// The cells the opening by a disc of `radius` m leaves out: land narrower than about twice
    /// `radius` (straits' banks, spits, strips and islets).
    pub fn thin(&self, radius: f64) -> Vec<bool> {
        let r = radius / self.cell;
        let off: Vec<bool> = self.on.iter().map(|&o| !o).collect();
        let to_sea = self.distance_from(&off);
        let core: Vec<bool> = to_sea.iter().map(|&d| d > r).collect();
        let to_core = self.distance_from(&core);
        (0..self.on.len())
            .map(|c| self.on[c] && to_core[c] > r)
            .collect()
    }

    /// The 8-connected components of `cells`, each its cells.
    pub fn components(&self, cells: &[bool]) -> Vec<Vec<usize>> {
        let mut seen = vec![false; cells.len()];
        let mut out = Vec::new();
        for s in 0..cells.len() {
            if !cells[s] || seen[s] {
                continue;
            }
            let mut comp = Vec::new();
            let mut q = VecDeque::from([s]);
            seen[s] = true;
            while let Some(c) = q.pop_front() {
                comp.push(c);
                for (o, _) in self.neighbours(c) {
                    if cells[o] && !seen[o] {
                        seen[o] = true;
                        q.push_back(o);
                    }
                }
            }
            out.push(comp);
        }
        out
    }

    /// A component's longest extent (m): its farthest two cells, measured over its bounding box's
    /// diagonal (enough for a strip).
    pub fn extent(&self, comp: &[usize]) -> f64 {
        let (mut a, mut b, mut c, mut d) = (usize::MAX, usize::MAX, 0, 0);
        for &k in comp {
            let (i, j) = (k % self.n, k / self.n);
            a = a.min(i);
            b = b.min(j);
            c = c.max(i);
            d = d.max(j);
        }
        ((c - a) as f64).hypot((d - b) as f64) * self.cell
    }

    /// The outline: cells of the mask with a 4-neighbour off it.
    pub fn outline(&self) -> Vec<bool> {
        let n = self.n;
        (0..self.on.len())
            .map(|c| {
                let (i, j) = (c % n, c / n);
                self.on[c]
                    && [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|&(di, dj)| {
                            let (a, b) = (i as i32 + di, j as i32 + dj);
                            a >= 0
                                && b >= 0
                                && (a as usize) < n
                                && (b as usize) < n
                                && !self.on[b as usize * n + a as usize]
                        })
            })
            .collect()
    }

    /// The share of `line`'s cells whose window of `radius` m holds a straight run: the line cells
    /// within it lie within `tolerance` m (RMS) of their fitted line and span at least 1.6 ×
    /// `radius` (so a window holding only a dot or a corner is not counted). Windows with fewer
    /// than 8 cells are skipped.
    pub fn straight_share(&self, line: &[bool], radius: f64, tolerance: f64) -> (f64, usize) {
        let n = self.n as i64;
        let r = (radius / self.cell).ceil() as i64;
        let (mut straight, mut all) = (0usize, 0usize);
        for c in (0..line.len()).filter(|&c| line[c]) {
            let (ci, cj) = ((c as i64) % n, (c as i64) / n);
            let mut pts = Vec::new();
            for dj in -r..=r {
                for di in -r..=r {
                    let (a, b) = (ci + di, cj + dj);
                    if a < 0 || b < 0 || a >= n || b >= n || (di * di + dj * dj) > r * r {
                        continue;
                    }
                    if line[(b * n + a) as usize] {
                        pts.push((di as f64 * self.cell, dj as f64 * self.cell));
                    }
                }
            }
            if pts.len() < 8 {
                continue;
            }
            all += 1;
            let k = pts.len() as f64;
            let (mx, mz) = (
                pts.iter().map(|p| p.0).sum::<f64>() / k,
                pts.iter().map(|p| p.1).sum::<f64>() / k,
            );
            let (mut sxx, mut szz, mut sxz) = (0.0, 0.0, 0.0);
            for p in &pts {
                let (x, z) = (p.0 - mx, p.1 - mz);
                sxx += x * x;
                szz += z * z;
                sxz += x * z;
            }
            let (sxx, szz, sxz) = (sxx / k, szz / k, sxz / k);
            let tr = sxx + szz;
            let det = sxx * szz - sxz * sxz;
            let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
            let (big, small) = (tr / 2.0 + disc, (tr / 2.0 - disc).max(0.0));
            // A uniform run of length L has variance L²/12 along it.
            let span = (12.0 * big).sqrt();
            if small.sqrt() <= tolerance && span >= 1.6 * radius {
                straight += 1;
            }
        }
        (straight as f64 / all.max(1) as f64, all)
    }

    /// A component's area (m²), outline length (m, its cells' edges off it), centre (m from the
    /// grid's corner) and its irregularity: outline² / (4π area) over a rasterized disc's of the
    /// same area (1 for a round island at this cell size, more for a ragged one).
    pub fn shape(&self, comp: &[usize]) -> Shape {
        let n = self.n;
        let mut edges = 0usize;
        let (mut sx, mut sz) = (0.0, 0.0);
        for &c in comp {
            let (i, j) = (c % n, c / n);
            sx += i as f64;
            sz += j as f64;
            for (di, dj) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (a, b) = (i as i32 + di, j as i32 + dj);
                if a < 0
                    || b < 0
                    || a as usize >= n
                    || b as usize >= n
                    || !self.on[b as usize * n + a as usize]
                {
                    edges += 1;
                }
            }
        }
        let k = comp.len() as f64;
        let area = k * self.cell * self.cell;
        let perimeter = edges as f64 * self.cell;
        let raw = perimeter * perimeter / (4.0 * std::f64::consts::PI * area);
        Shape {
            area,
            radius: (area / std::f64::consts::PI).sqrt(),
            x: sx / k * self.cell,
            z: sz / k * self.cell,
            irregularity: raw / disc_ratio(comp.len()),
        }
    }
}

/// A rasterized disc of `cells` cells' outline² / (4π area) (≈ 16/π for a large disc: the
/// staircase's length).
fn disc_ratio(cells: usize) -> f64 {
    let r = (cells as f64 / std::f64::consts::PI).sqrt();
    let m = r.ceil() as i64 + 1;
    let inside = |i: i64, j: i64| (i as f64).hypot(j as f64) <= r;
    let (mut a, mut e) = (0usize, 0usize);
    for j in -m..=m {
        for i in -m..=m {
            if !inside(i, j) {
                continue;
            }
            a += 1;
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if !inside(i + di, j + dj) {
                    e += 1;
                }
            }
        }
    }
    (e * e) as f64 / (4.0 * std::f64::consts::PI * a as f64)
}

pub struct Shape {
    pub area: f64,
    pub radius: f64,
    pub x: f64,
    pub z: f64,
    pub irregularity: f64,
}

/// The coefficient of variation (standard deviation over mean) of `v`.
pub fn cv(v: &[f64]) -> f64 {
    let k = v.len() as f64;
    let mean = v.iter().sum::<f64>() / k;
    (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / k).sqrt() / mean
}
