//! Stream-power fluvial erosion (Cordonnier et al. 2016) with Braun & Willett's (2013) implicit
//! solver, plus an optional talus pass. Each iteration floods the surface (depressions route
//! through their spill point, `flood.rs`), accumulates the rain-weighted drainage area down the
//! flood tree, and lowers each land cell toward its receiver, receivers first:
//!
//!   h' = h + U·Δt − K·Δt·A^m·((h' − h_r') / d)^n
//!
//! solved exactly for n = 1 and by a bracketed Newton iteration otherwise. Cells under a
//! depression's water are not eroded: they take the uplift and fill by `lake_fill` of their depth
//! each iteration (sediment settling in still water), so small basins silt up into flats and only
//! the large, deep ones last. The sea is the fixed base level.

use super::HydrologyParams;
use super::flood::{Grid, NONE, flood, shore};
use super::math::pow;
use crate::plates::FlowRouting;

/// One erosion run over `h` (m above sea level) where `sea` is false; the sea never changes. `uplift` is the
/// uplift rate per cell (m/yr), `erodibility` K per cell, `rain` the relative rain per cell, `cell`
/// the cell size (m). `routing` how each step sends a cell's water down: to the cell it was
/// flooded from (D8), or to a lower neighbour drawn by slope (a Realistic world's, plates.json
/// `variety.flow`), so the channels do not all run along the grid's 45° and 90° lines.
#[allow(clippy::too_many_arguments)]
pub fn erode(
    grid: Grid,
    cell: f64,
    h: &mut [f64],
    uplift: &[f64],
    erodibility: &[f64],
    rain: &[f64],
    sea: &[bool],
    p: &HydrologyParams,
    routing: FlowRouting,
) {
    let len = grid.len();
    let cell_area = cell * cell;
    let diag = cell * std::f64::consts::SQRT_2;
    let mut area = vec![0.0f64; len];
    let mut level = vec![0.0f64; len];
    let dt = p.time_step;
    let shore = shore(grid, sea);
    let land: Vec<u32> = (0..len as u32).filter(|&i| !sea[i as usize]).collect();
    for it in 0..p.iterations {
        crate::progress::report(
            crate::progress::MapStep::Erosion,
            f64::from(it) / f64::from(p.iterations.max(1)),
        );
        let mut f = flood(grid, h, sea, &shore);
        if let FlowRouting::Random { exponent } = routing {
            reroute(grid, cell, &mut f, sea, exponent, it);
        }
        // Drainage area (m², rain-weighted), donors before receivers.
        for &c in &f.order {
            area[c as usize] = rain[c as usize] * cell_area;
        }
        for &c in f.order.iter().rev() {
            let c = c as usize;
            let r = f.parent[c] as usize;
            if !sea[r] {
                area[r] += area[c];
            }
        }
        // The implicit update, receivers first.
        for &c in &f.order {
            let c = c as usize;
            let r = f.parent[c] as usize;
            let base = if sea[r] { 0.0 } else { level[r] };
            let raised = h[c] + uplift[c] * dt;
            if f.level[c] > h[c] {
                // Under a depression's water: uplift, and sediment settling (a share of the depth).
                h[c] = raised + p.lake_fill * (f.level[c] - raised).max(0.0);
                level[c] = h[c].max(base);
                continue;
            }
            if raised <= base {
                // Below its receiver: uplift only.
                h[c] = raised;
                level[c] = base;
                continue;
            }
            let d = if is_diagonal(grid, c, r) { diag } else { cell };
            // A coarser grid than the profile's (a bigger world, `HydrologyParams::sized`): the
            // cell's own share of its area is a profile cell's, so its ridges keep their slope.
            let a = if p.ridge_cell > 0.0 && p.ridge_cell < cell {
                area[c] - rain[c] * cell_area + rain[c] * p.ridge_cell * p.ridge_cell
            } else {
                area[c]
            };
            let k = erodibility[c] * dt * pow(a, p.area_exponent) / pow(d, p.slope_exponent);
            let rise = raised - base;
            let x = solve(rise, k, p.slope_exponent);
            h[c] = base + x;
            level[c] = h[c];
        }
        talus(grid, cell, h, sea, &land, p);
    }
    crate::progress::report(crate::progress::MapStep::Erosion, 1.0);
}

/// Randomized single-flow routing: each land cell above its depression's water sends its water to
/// one neighbour strictly lower than its level, drawn with a weight of its slope to the power
/// `exponent` by a hash of the cell and the step `it` (so the draw changes every step and the
/// channels average over the lower neighbours, not one grid direction). A strictly lower level was
/// flooded earlier (the flood pops levels in order), so the order stays receivers first.
fn reroute(
    grid: Grid,
    cell: f64,
    f: &mut super::flood::Flood,
    sea: &[bool],
    exponent: f64,
    it: u32,
) {
    let diag = cell * std::f64::consts::SQRT_2;
    let mut cand = [(0usize, 0.0f64); 8];
    // The neighbours in `NEIGHBOURS`' order (the draw's order), as index offsets for a cell off
    // the grid's border (the map's interior: no bounds checks).
    let w = grid.n as isize;
    let offsets = super::flood::NEIGHBOURS.map(|(dx, dz, d)| (dz as isize * w + dx as isize, d));
    for &c in &f.order {
        let c = c as usize;
        let lc = f.level[c];
        let mut k = 0;
        let mut total = 0.0;
        let mut take = |j: usize, d: bool| {
            let lj = if sea[j] { 0.0 } else { f.level[j] };
            if lj < lc {
                let w = pow((lc - lj) / if d { diag } else { cell }, exponent);
                if w > 0.0 && w.is_finite() {
                    cand[k] = (j, w);
                    total += w;
                    k += 1;
                }
            }
        };
        if grid.on_border(c) {
            for (j, d) in grid.neighbours(c) {
                take(j, d);
            }
        } else {
            for &(o, d) in &offsets {
                take((c as isize + o) as usize, d);
            }
        }
        if k < 2 {
            continue;
        }
        let hash = crate::noise::mix32(
            (c as u32).wrapping_mul(0x9E37_79B9) ^ crate::noise::mix32(it.wrapping_add(0x51A7)),
        );
        let mut u = f64::from(hash) / f64::from(u32::MAX) * total;
        let mut pick = cand[k - 1].0;
        for &(j, w) in &cand[..k] {
            if u < w {
                pick = j;
                break;
            }
            u -= w;
        }
        f.parent[c] = pick as u32;
    }
}

fn is_diagonal(grid: Grid, a: usize, b: usize) -> bool {
    a % grid.n != b % grid.n && a / grid.n != b / grid.n
}

/// The x ≥ 0 with x + k·xⁿ = c (c > 0): exact for n = 1, else Newton's method kept inside [0, c]
/// (a bisection step when it leaves), a fixed number of steps.
pub fn solve(c: f64, k: f64, n: f64) -> f64 {
    if n == 1.0 {
        return c / (1.0 + k);
    }
    let (mut lo, mut hi) = (0.0f64, c);
    let mut x = c;
    for _ in 0..40 {
        let g = x + k * pow(x, n) - c;
        if g > 0.0 {
            hi = x;
        } else {
            lo = x;
        }
        let dg = 1.0 + n * k * pow(x, n - 1.0);
        let next = x - g / dg;
        x = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
    }
    x
}

/// Thermal (talus) smoothing: where a land cell is steeper than `talus_slope` toward its lowest
/// neighbour, a quarter of the excess moves down to it (none comes back from the sea). Jacobi
/// passes: every move is computed from the same surface, so the order does not matter.
fn talus(grid: Grid, cell: f64, h: &mut [f64], sea: &[bool], land: &[u32], p: &HydrologyParams) {
    let mut delta = vec![0.0f64; grid.len()];
    for _ in 0..p.talus_passes {
        delta.fill(0.0);
        for &i in land {
            let i = i as usize;
            let mut best = (0.0f64, NONE);
            for (j, diag) in grid.neighbours(i) {
                let d = if diag {
                    cell * std::f64::consts::SQRT_2
                } else {
                    cell
                };
                let hj = if sea[j] { 0.0 } else { h[j] };
                let excess = h[i] - hj - p.talus_slope * d;
                if excess > best.0 {
                    best = (excess, j as u32);
                }
            }
            if best.1 != NONE {
                let t = 0.25 * best.0;
                delta[i] -= t;
                let j = best.1 as usize;
                if !sea[j] {
                    delta[j] += t;
                }
            }
        }
        for (v, d) in h.iter_mut().zip(&delta) {
            *v += d;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_implicit_solve_satisfies_its_equation() {
        for &n in &[0.7, 1.0, 1.5, 2.0] {
            for &(c, k) in &[(10.0, 0.5), (300.0, 20.0), (0.01, 3.0), (50.0, 1e-4)] {
                let x = solve(c, k, n);
                assert!(x >= 0.0 && x <= c);
                let r = x + k * pow(x, n) - c;
                assert!(r.abs() < 1e-9 * c.max(1.0), "n {n} c {c} k {k}: {r}");
            }
        }
    }
}
