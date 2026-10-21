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

/// One erosion run over `h` (m above sea level) where `sea` is false; the sea never changes. `uplift` is the
/// uplift rate per cell (m/yr), `rain` the relative rain per cell, `cell` the cell size (m).
pub fn erode(
    grid: Grid,
    cell: f64,
    h: &mut [f64],
    uplift: &[f64],
    rain: &[f64],
    sea: &[bool],
    p: &HydrologyParams,
) {
    let len = grid.len();
    let cell_area = cell * cell;
    let diag = cell * std::f64::consts::SQRT_2;
    let mut area = vec![0.0f64; len];
    let mut level = vec![0.0f64; len];
    let dt = p.time_step;
    let shore = shore(grid, sea);
    let land: Vec<u32> = (0..len as u32).filter(|&i| !sea[i as usize]).collect();
    for _ in 0..p.iterations {
        let f = flood(grid, h, sea, &shore);
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
            let k = p.erodibility * dt * pow(area[c], p.area_exponent) / pow(d, p.slope_exponent);
            let rise = raised - base;
            let x = solve(rise, k, p.slope_exponent);
            h[c] = base + x;
            level[c] = h[c];
        }
        talus(grid, cell, h, sea, &land, p);
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
