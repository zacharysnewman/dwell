//! Orographic rain (`Rain::Orographic`): a relative rain factor per land cell from the initial
//! ground and a prevailing wind. Windward slopes are wetter by `upslope` × their rise into the
//! wind over a kilometre; ground behind a barrier is drier by `shadow` × (the barrier's height
//! above it ÷ `shadow_height`, at most 1), the barrier being the highest ground within `fetch` m
//! upwind. The factor is kept within [min, max]. It is computed once, from the ground before
//! erosion (the large-scale shape is what steers the weather), and weights the drainage area.
//!
//! Deterministic: the wind's direction from `math::sin_cos`, bilinear samples in f64.

use super::flood::Grid;
use super::math::sin_cos;
use super::Orographic;
use crate::noise::{mix32, seed_word};

/// The relative rain per cell (1 at sea).
pub fn orographic(
    grid: Grid,
    cell: f64,
    h: &[f64],
    sea: &[bool],
    seed: u64,
    o: &Orographic,
) -> Vec<f64> {
    let n = grid.n;
    let turn = f64::from(mix32(seed_word(seed, 0x4859_7100))) / 4_294_967_296.0;
    let degrees = o.direction + o.spread * (2.0 * turn - 1.0);
    let (s, c) = sin_cos(degrees * (std::f64::consts::PI / 180.0));
    // The wind blows from (s, −c) toward (−s, c), in grid units per step; upwind is (s, −c).
    let (ux, uz) = (s, -c);
    let ground = |i: usize| if sea[i] { 0.0 } else { h[i].max(0.0) };
    let at = |gx: f64, gz: f64| -> f64 {
        let max = (n - 1) as f64;
        let (gx, gz) = (gx.clamp(0.0, max), gz.clamp(0.0, max));
        let (x0, z0) = (gx.floor(), gz.floor());
        let (tx, tz) = (gx - x0, gz - z0);
        let (x0, z0) = (x0 as usize, z0 as usize);
        let (x1, z1) = ((x0 + 1).min(n - 1), (z0 + 1).min(n - 1));
        let a = ground(z0 * n + x0) + (ground(z0 * n + x1) - ground(z0 * n + x0)) * tx;
        let b = ground(z1 * n + x0) + (ground(z1 * n + x1) - ground(z1 * n + x0)) * tx;
        a + (b - a) * tz
    };
    let lift_run = 1000.0 / cell;
    let step = (500.0 / cell).max(1.0);
    let steps = (o.fetch / (step * cell)).ceil() as usize;
    let mut rain = vec![1.0f64; grid.len()];
    for (i, r) in rain.iter_mut().enumerate() {
        if sea[i] {
            continue;
        }
        let (gx, gz) = ((i % n) as f64, (i / n) as f64);
        let here = ground(i);
        let rise = (here - at(gx + ux * lift_run, gz + uz * lift_run)) / 1000.0;
        let mut barrier = 0.0f64;
        for k in 1..=steps {
            let d = k as f64 * step;
            barrier = barrier.max(at(gx + ux * d, gz + uz * d));
        }
        let shade = ((barrier - here).max(0.0) / o.shadow_height).min(1.0);
        let f = 1.0 + o.upslope * rise.max(0.0) - o.shadow * shade;
        *r = f.clamp(o.min, o.max);
    }
    rain
}
