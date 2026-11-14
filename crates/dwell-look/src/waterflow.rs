//! How moving water shows its flow (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3d): ripples carried
//! by the current, foam streaks along it and particles drifting on it, per liquid
//! (`shared/look/waterflow.json`, generated into `waterflow.gen.rs`). The current is the client's
//! current table's (`dwell_world::currents`); the renderer and the game read these looks.

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("waterflow.gen.rs");
}
pub use generated::{LIQUID_FLOWS, WATER_FLOW};

/// `shared/look/waterflow.json`'s tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterFlowTunables {
    /// Columns square the flow texture holds around the eye (a power of two; it wraps).
    pub window_columns: u32,
    /// The fastest current the texture holds (m/s).
    pub range_ms: f64,
    /// The flow shows fully within this of the eye, and fades out by `fade_to_m` (m).
    pub fade_from_m: f64,
    pub fade_to_m: f64,
}

/// A liquid's flow look (`liquids` in `shared/look/waterflow.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidFlowLook {
    /// The registry fluid's name, e.g. `dwell:water`.
    pub fluid: &'static str,
    /// Ripples: how far they are carried per metre the water moves, and how long before a ripple
    /// blends in afresh (s).
    pub advect: f64,
    pub cycle_s: f64,
    /// Foam streaks: the current where they begin and where they are full (m/s); sRGB
    /// `0xRRGGBB`; strength at full (0–1); a streak's size along and across the flow (m).
    pub foam_from_ms: f64,
    pub foam_to_ms: f64,
    pub foam_color: u32,
    pub foam_strength: f64,
    pub foam_length_m: f64,
    pub foam_width_m: f64,
    /// Particles (flecks): at most this many near the eye (`particle_cpu_count` on a CPU
    /// adapter), spawned within `particle_radius_m` in currents of at least `particle_from_ms`,
    /// more often and more opaque up to `particle_full_ms`; each `particle_size_m` half-width
    /// across the flow and up to `particle_stretch` times as long along it (at `particle_full_ms`),
    /// this colour (sRGB) at most `particle_opacity`, drifting for `particle_life_s`, faded in
    /// over its first `particle_fade_in_s` and out over its last `particle_fade_out_s`; its edge
    /// broken by `particle_ragged` (0–1) in `particle_lumps` lobes; faded out where it is under
    /// `particle_min_pixels` wide on screen.
    pub particle_count: u32,
    pub particle_cpu_count: u32,
    pub particle_from_ms: f64,
    pub particle_full_ms: f64,
    pub particle_size_m: f64,
    pub particle_stretch: f64,
    pub particle_color: u32,
    pub particle_opacity: f64,
    pub particle_life_s: f64,
    pub particle_fade_in_s: f64,
    pub particle_fade_out_s: f64,
    pub particle_radius_m: f64,
    pub particle_ragged: f64,
    pub particle_lumps: u32,
    pub particle_min_pixels: f64,
    /// Shore foam (WATER_RENDERING.md §14, §20): a contact line where the water meets land or a
    /// block, solid over half of `shore_line_m` and feathering into the water over `shore_feather_m`
    /// beyond it (owner, 2026-11-13: the old soft edge with the new even one), and `shore_bands` soft bands parallel to the shore, each born
    /// at the edge every `shore_period_s`, rolling out over the fluid's foam depth while it thins
    /// (from `shore_width_m`) and fades; slow noise shifts each band's timing and width along the
    /// shore by up to `shore_wobble` (0–1) without cutting holes; `shore_band_softness` (0–1) how
    /// soft a band's sides are; at most `shore_strength`.
    pub shore_bands: u32,
    pub shore_period_s: f64,
    pub shore_line_m: f64,
    pub shore_feather_m: f64,
    pub shore_width_m: f64,
    pub shore_band_softness: f64,
    pub shore_strength: f64,
    pub shore_wobble: f64,
    /// The shore's corners (WATER_RENDERING.md §21; owner, 2026-11-13: 'smoother or thicker around
    /// corners, so it doesn't looks so squared off'): an inner corner's fillet radius (m); an
    /// outer corner is always round (the Euclidean distance to it).
    pub shore_corner_radius_m: f64,
}

/// Liquid `fluid`'s flow look, if it has one (none: its water shows no flow).
pub fn flow_look(fluid: &str) -> Option<&'static LiquidFlowLook> {
    LIQUID_FLOWS.iter().find(|l| l.fluid == fluid)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The shore foam's amount (0–1) `shore` metres from the shore (the depth over a slope, the ray's
/// path to a wall), `reach` the fluid's foam depth (m), at time `t` (s) where the slow noise along
/// the shore is `n` (about -1.5..1.5). The WGSL `foam_amount` (render/shaders.rs) mirrors this line
/// for line; the tests here pin its profile.
pub fn shore_foam(l: &LiquidFlowLook, reach: f64, shore: f64, t: f64, n: f64) -> f64 {
    let most = l.shore_strength;
    if reach <= 0.0 || most <= 0.0 || shore >= reach + l.shore_width_m + l.shore_feather_m {
        return 0.0;
    }
    let wob = l.shore_wobble;
    // the contact line: solid over its core, feathering into the water
    let line_w = l.shore_line_m * (1.0 + 0.4 * wob * n);
    let mut foam = 1.0 - smoothstep(line_w * 0.5, line_w.max(1e-3) + l.shore_feather_m, shore);
    let bands = l.shore_bands.min(4);
    let period = l.shore_period_s.max(0.1);
    let soft = l.shore_band_softness;
    for k in 0..bands {
        let a = t / period + f64::from(k) / f64::from(bands) + 0.25 * wob * n;
        let age = a - a.floor();
        let at = l.shore_line_m + age * reach;
        let w = l.shore_width_m * (1.0 - 0.7 * age) * (1.0 + 0.5 * wob * n);
        let shape = 1.0
            - smoothstep(
                w * 0.25 * (1.0 - soft),
                w * 0.5 * (1.0 + soft),
                (shore - at).abs(),
            );
        let fade = smoothstep(0.0, 0.12, age) * (1.0 - age) * (1.0 - age);
        foam = foam.max(shape * fade * 0.8);
    }
    (foam * most).clamp(0.0, 1.0)
}

/// A water top cell's solid neighbours at its surface's row, one bit each (face-local; the mesher's
/// `shore_mask`, the WGSL `shore_xz`): −x, +x, −z, +z, then the diagonals −x−z, +x−z, −x+z, +x+z.
pub const SHORE_W: u32 = 1;
pub const SHORE_E: u32 = 2;
pub const SHORE_S: u32 = 4;
pub const SHORE_N: u32 = 8;
pub const SHORE_SW: u32 = 16;
pub const SHORE_SE: u32 = 32;
pub const SHORE_NW: u32 = 64;
pub const SHORE_NE: u32 = 128;
/// What [`shore_xz`] returns where no neighbour is solid (no shore within the cell's reach).
pub const SHORE_FAR: f64 = 1.0e4;

/// The distance across the surface (m) from `p`, a point in a water cell (0..1 on x and z), to the
/// shore its `mask` names ([`SHORE_W`]…): the Euclidean distance to the nearest solid
/// neighbour, so an outer corner (only a diagonal solid) is a quarter circle round its corner, and
/// an inner corner (two sides solid) filleted with radius `r` (at most ½), so the foam's band
/// rounds and thickens into it instead of a sharp L. The WGSL `shore_xz` mirrors this.
pub fn shore_xz(mask: u32, p: [f64; 2], r: f64) -> f64 {
    if mask == 0 {
        return SHORE_FAR;
    }
    let [x, z] = p;
    let (ex, wx, sz, nz) = (1.0 - x, x, z, 1.0 - z);
    let on = |b: u32| mask & b != 0;
    let mut d = SHORE_FAR;
    let side = |d: &mut f64, b: u32, v: f64| {
        if on(b) {
            *d = d.min(v);
        }
    };
    side(&mut d, SHORE_W, wx);
    side(&mut d, SHORE_E, ex);
    side(&mut d, SHORE_S, sz);
    side(&mut d, SHORE_N, nz);
    let corner = |d: &mut f64, b: u32, a: f64, c: f64| {
        if on(b) {
            *d = d.min(a.hypot(c));
        }
    };
    corner(&mut d, SHORE_SW, wx, sz);
    corner(&mut d, SHORE_SE, ex, sz);
    corner(&mut d, SHORE_NW, wx, nz);
    corner(&mut d, SHORE_NE, ex, nz);
    // inner corners: the land filled out by a quarter circle of radius r
    let r = r.clamp(0.0, 0.5);
    let fillet = |d: &mut f64, a: u32, b: u32, u: f64, v: f64| {
        if r > 0.0 && on(a) && on(b) && u < r && v < r {
            *d = d.min((r - (r - u).hypot(r - v)).max(0.0));
        }
    };
    fillet(&mut d, SHORE_W, SHORE_S, wx, sz);
    fillet(&mut d, SHORE_E, SHORE_S, ex, sz);
    fillet(&mut d, SHORE_W, SHORE_N, wx, nz);
    fillet(&mut d, SHORE_E, SHORE_N, ex, nz);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An outer corner (owner, 2026-11-13: 'smoother or thicker around corners'): only the
    /// diagonal is solid, and the distance is the same at the same Euclidean distance from the
    /// corner in every direction, so the foam's band is a quarter circle, not a square.
    #[test]
    fn an_outer_corner_is_round() {
        let r = flow_look("dwell:water").unwrap().shore_corner_radius_m;
        for k in 0..=16 {
            let a = std::f64::consts::FRAC_PI_2 * f64::from(k) / 16.0;
            for &rho in &[0.05, 0.15, 0.3] {
                let p = [rho * a.cos(), rho * a.sin()];
                let d = shore_xz(SHORE_SW, p, r);
                assert!((d - rho).abs() < 1e-9, "at angle {a}: {d} vs {rho}");
                let foam = |d| shore_foam(flow_look("dwell:water").unwrap(), 0.5, d, 0.0, 0.0);
                assert!((foam(d) - foam(rho)).abs() < 1e-9);
            }
        }
        // on the diagonal, √2 × each side's distance (a square band's edge read the side's alone)
        let p = [0.1, 0.1];
        assert!((shore_xz(SHORE_SW, p, r) - 0.1 * 2f64.sqrt()).abs() < 1e-9);
    }

    /// An inner corner: two sides solid, filleted, so the distance has no step anywhere near the
    /// corner (across its bisector or where the fillet meets the straight sides), it is smaller
    /// along the bisector than either side's alone (the band thickens into the corner), and its
    /// contours there are arcs (equal at equal distance from the fillet's centre).
    #[test]
    fn an_inner_corner_is_filleted_without_a_step() {
        let r = flow_look("dwell:water").unwrap().shore_corner_radius_m;
        assert!(r > 0.0 && r <= 0.5);
        let m = SHORE_W | SHORE_S | SHORE_SW;
        let step = 0.002;
        let n = (0.8 / step) as i32;
        let mut worst = 0.0f64;
        for i in 0..n {
            for j in 0..n {
                let p = [f64::from(i) * step, f64::from(j) * step];
                let d = shore_xz(m, p, r);
                for q in [[p[0] + step, p[1]], [p[0], p[1] + step]] {
                    worst = worst.max((shore_xz(m, q, r) - d).abs());
                }
                // mirrored across the bisector, the same
                assert!((shore_xz(m, [p[1], p[0]], r) - d).abs() < 1e-9);
            }
        }
        assert!(worst <= step * 1.0001, "a step of {worst} over {step}");
        let b = 0.1;
        assert!(shore_xz(m, [b, b], r) < b - 1e-3, "thicker into the corner");
        for k in 0..=8 {
            let a = std::f64::consts::FRAC_PI_2 * f64::from(k) / 8.0;
            let p = [r - 0.6 * r * a.cos(), r - 0.6 * r * a.sin()];
            assert!((shore_xz(m, p, r) - 0.4 * r).abs() < 1e-9);
        }
        // no solid neighbour: no shore; the sides alone are their distances
        assert_eq!(shore_xz(0, [0.5, 0.5], r), SHORE_FAR);
        assert!((shore_xz(SHORE_E, [0.7, 0.2], r) - 0.3).abs() < 1e-9);
    }

    /// The shore foam across the edge (owner, 2026-11-13: 'the softness of the old version was
    /// better and the consistency of the new one along the edge is better'): soft, no step over
    /// 0.08 between samples 5 mm apart (0.52.6's contact line stepped 0.2), and even along the
    /// edge: within the feather's first half it never drops under half its most, whatever the noise.
    #[test]
    fn the_shore_foam_feathers_into_the_water_and_holds_along_the_edge() {
        let l = flow_look("dwell:water").unwrap();
        let reach = 0.5;
        let mut worst = 0.0f64;
        let mut low = f64::MAX;
        for ti in 0..60 {
            let t = f64::from(ti) * 0.37;
            for ni in -6..=6 {
                let n = f64::from(ni) * 0.25;
                let mut last = shore_foam(l, reach, 0.0, t, n);
                for si in 1..=160 {
                    let shore = f64::from(si) * 0.005;
                    let v = shore_foam(l, reach, shore, t, n);
                    worst = worst.max((v - last).abs());
                    last = v;
                    if shore <= l.shore_feather_m * 0.5 {
                        low = low.min(v);
                    }
                }
            }
        }
        assert!(
            worst <= 0.08,
            "the foam steps {worst:.3} between samples 5 mm apart"
        );
        assert!(low >= 0.5 * l.shore_strength, "the edge drops to {low:.3}");
        assert!(l.shore_feather_m >= 0.1, "{}", l.shore_feather_m);
    }

    #[test]
    fn water_has_a_flow_look_and_an_unknown_liquid_none() {
        let w = flow_look("dwell:water").expect("water's flow look");
        assert!(w.foam_to_ms > w.foam_from_ms);
        assert!(flow_look("test:nothing").is_none());
        assert!(WATER_FLOW.window_columns.is_power_of_two());
        assert!(WATER_FLOW.fade_to_m <= f64::from(WATER_FLOW.window_columns) / 2.0);
    }
}
