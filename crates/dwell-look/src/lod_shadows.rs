//! Sun and moon shadows on the distant terrain (docs/DAY_NIGHT.md §12, ADR 0046): the lighting's
//! third layer, past the shadow cascades. The renderer keeps heightmaps of the terrain as drawn
//! around the eye (`dwell-client` `render/lod_shadows.rs`), sweeps a horizon map from each (every
//! texel's march toward the light: the terrain's highest angle that way), and the scene's shaders
//! shade each far fragment by its horizon (`LOD_SHADOW_WGSL`). The tunables are data
//! (`shared/look/lodshadows.json`, generated into `lodshadows.gen.rs`).
//!
//! [`horizon`] is the sweep's march, [`shade`] the fragment's use of it, and [`combine`] how its
//! result joins the cascades' and the first layer's stand-in, in f64 for tests (the shaders are
//! their copy; shader-source tests pin their lines).

use crate::shadows::ShadowQuality;
use dwell_world::FaceFrame;

#[path = "lodshadows.gen.rs"]
mod generated;
pub use generated::LOD_SHADOWS;

/// The face-local heights the maps hold (m over the midplane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightRange {
    pub bottom_m: f64,
    pub top_m: f64,
}

/// The march toward the light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarchTunables {
    /// The first step (texels of the texel's map).
    pub start_texels: f64,
    /// The ray starts this far over the texel's height (m).
    pub bias_m: f64,
    /// The soft edge's half-width (degrees about the horizon a ray meets).
    pub penumbra_deg: f64,
    /// The shadows fade out over this last share of the outermost map's half-width.
    pub edge_fade: f64,
}

/// When the maps are made again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefreshTunables {
    /// A heightmap is filled again once the eye is this share of its half-width from where it was
    /// filled…
    pub move_share: f64,
    /// …or this long after (s).
    pub every_s: f64,
    /// A horizon map is swept again once the light has turned this far (degrees about the face's
    /// up) from the direction it was swept for, or its heightmap was filled again.
    pub turn_deg: f64,
}

/// One Shadows setting's maps and march (Low, High).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodShadowTier {
    /// Each map's texels a side (a power of two).
    pub resolution: u32,
    /// The nearest map's texel (m).
    pub texel_m: f64,
    /// Maps (1–2), each `level_scale` times coarser.
    pub levels: u32,
    pub level_scale: u32,
    /// The march's steps, from the first step to `reach_m`, geometrically spaced.
    pub steps: u32,
    pub reach_m: f64,
    /// Each frame's share of a map's fill: thousands of triangles, draws.
    pub fill_ktris: f64,
    pub fill_draws: u32,
    /// A horizon sweep's rows of texels a frame.
    pub sweep_rows: u32,
}

impl LodShadowTier {
    /// Level `level`'s texel (m).
    pub fn texel(&self, level: u32) -> f64 {
        self.texel_m * f64::from(self.level_scale.pow(level))
    }
    /// Level `level`'s half-width (m).
    pub fn half_width(&self, level: u32) -> f64 {
        self.texel(level) * f64::from(self.resolution) * 0.5
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodShadowTunables {
    pub heights: HeightRange,
    pub march: MarchTunables,
    pub refresh: RefreshTunables,
    /// Low, High.
    pub tiers: [LodShadowTier; 2],
}

/// The most maps a tier has (the shader's array; shared/look/lodshadows.mjs `MAX_LEVELS`).
pub const MAX_LEVELS: usize = 2;

impl LodShadowTunables {
    /// The maps a Shadows setting draws (none for Off).
    pub fn tier(&self, q: ShadowQuality) -> Option<&LodShadowTier> {
        match q {
            ShadowQuality::Off => None,
            ShadowQuality::Low => Some(&self.tiers[0]),
            ShadowQuality::High => Some(&self.tiers[1]),
        }
    }
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A heightmap as the GPU holds one (`render/lod_shadows.rs`): the terrain's face-local height at
/// each texel's centre over a square around `centre` (world x, z), and its max mips (each texel the
/// highest of the four under it), which let a long step of the march see every ridge it passes.
#[derive(Clone, Debug)]
pub struct HeightMap {
    pub centre: [f64; 2],
    pub texel: f64,
    pub resolution: u32,
    /// Level 0 first; each level's texels x fastest, then z.
    pub mips: Vec<Vec<f64>>,
}

impl HeightMap {
    /// A map of `resolution` texels of `texel` m around `centre`, from the terrain's face-local
    /// height `h(x, z)` at each texel's centre, with `mips` levels in all.
    pub fn from_fn(
        centre: [f64; 2],
        texel: f64,
        resolution: u32,
        mips: u32,
        h: impl Fn(f64, f64) -> f64,
    ) -> Self {
        let n = resolution as usize;
        let half = texel * f64::from(resolution) * 0.5;
        let mut level: Vec<f64> = (0..n * n)
            .map(|k| {
                let (i, j) = (k % n, k / n);
                let x = centre[0] - half + (i as f64 + 0.5) * texel;
                let z = centre[1] - half + (j as f64 + 0.5) * texel;
                h(x, z)
            })
            .collect();
        let mut out = vec![level.clone()];
        let mut side = n;
        for _ in 1..mips.clamp(1, max_mips(resolution)) {
            let s = side / 2;
            level = (0..s * s)
                .map(|k| {
                    let (i, j) = (k % s, k / s);
                    let at = |a: usize, b: usize| out.last().expect("a level")[a + b * side];
                    at(2 * i, 2 * j)
                        .max(at(2 * i + 1, 2 * j))
                        .max(at(2 * i, 2 * j + 1))
                        .max(at(2 * i + 1, 2 * j + 1))
                })
                .collect();
            out.push(level.clone());
            side = s;
        }
        Self {
            centre,
            texel,
            resolution,
            mips: out,
        }
    }

    pub fn half_width(&self) -> f64 {
        self.texel * f64::from(self.resolution) * 0.5
    }

    /// Whether the map holds a column (strictly inside its square).
    pub fn holds(&self, x: f64, z: f64) -> bool {
        let h = self.half_width();
        (x - self.centre[0]).abs() < h && (z - self.centre[1]).abs() < h
    }

    /// The height at a column the map holds: level 0 bilinear between texel centres (the shader's
    /// gather), a coarser level the highest of the texel the column falls in.
    pub fn height(&self, x: f64, z: f64, level: usize) -> f64 {
        let level = level.min(self.mips.len() - 1);
        let side = (self.resolution as usize) >> level;
        let texel = self.texel * (1u64 << level) as f64;
        let half = self.half_width();
        let (u, v) = (
            (x - self.centre[0] + half) / texel,
            (z - self.centre[1] + half) / texel,
        );
        let at = |i: i64, j: i64| {
            let c = |a: i64| a.clamp(0, side as i64 - 1) as usize;
            self.mips[level][c(i) + c(j) * side]
        };
        if level > 0 {
            return at(u.floor() as i64, v.floor() as i64);
        }
        let (tu, tv) = (u - 0.5, v - 0.5);
        let (i, j) = (tu.floor(), tv.floor());
        let (fu, fv) = (tu - i, tv - j);
        let (i, j) = (i as i64, j as i64);
        let lo = at(i, j) + (at(i + 1, j) - at(i, j)) * fu;
        let hi = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * fu;
        lo + (hi - lo) * fv
    }
}

/// How many mip levels a map of `resolution` texels has at most (the shader's `MAX_MIPS`): down to
/// 8 texels a side.
pub fn max_mips(resolution: u32) -> u32 {
    (resolution.max(8).ilog2() - 2).min(MAX_MIPS)
}

/// The most mip levels a map keeps.
pub const MAX_MIPS: u32 = 9;

/// The mip level a march step of `delta` m reads from a map of `texel` m: the first whose texel
/// is at least the step, so a step sees the highest of everything it passes.
pub fn step_level(delta: f64, texel: f64, mips: usize) -> usize {
    let l = (delta / texel).max(1.0).log2().round();
    (l as usize).min(mips.saturating_sub(1))
}

/// The horizon toward a light at a column (world x, z) of the terrain in `maps` (finest first;
/// face-local heights) on face `f`: the highest tangent of the terrain's elevation seen from the
/// column's height in its map (plus the bias) in the light's direction about the face's up
/// (`light`, world; its elevation doesn't matter). The sweep's march: steps from `startTexels`
/// out, growing geometrically to `reachM`, each reading the finest map holding it at the max mip
/// its length needs (`step_level`). Below −`NO_HORIZON` (nothing that way): `-NO_HORIZON`.
pub fn horizon(
    t: &LodShadowTunables,
    tier: &LodShadowTier,
    maps: &[HeightMap],
    f: FaceFrame,
    column: [f64; 2],
    light: [f64; 3],
) -> f64 {
    let l = f.vector(light);
    let horizontal = (l[0] * l[0] + l[2] * l[2]).sqrt();
    let Some(home) = maps.iter().find(|m| m.holds(column[0], column[1])) else {
        return -NO_HORIZON;
    };
    if horizontal < 1e-6 {
        return -NO_HORIZON;
    }
    let dir = [l[0] / horizontal, l[2] / horizontal];
    let m = &t.march;
    let h0 = home.height(column[0], column[1], 0) + m.bias_m;
    let start = home.texel * m.start_texels;
    let growth = (tier.reach_m / start).powf(1.0 / f64::from(tier.steps.max(2) - 1));
    let mut most = -NO_HORIZON;
    let mut s = start;
    for _ in 0..tier.steps {
        let (x, z) = (column[0] + dir[0] * s, column[1] + dir[1] * s);
        if let Some(map) = maps.iter().find(|m| m.holds(x, z)) {
            let level = step_level(s * (growth - 1.0), map.texel, map.mips.len());
            most = most.max((map.height(x, z, level) - h0) / s);
        }
        s *= growth;
    }
    most.min(NO_HORIZON)
}

/// The horizon's range (a tangent; the map stores it in half floats).
pub const NO_HORIZON: f64 = 64.0;

/// The light let through by a horizon of tangent `horizon` to a light whose elevation's tangent is
/// `rise`, softly over the penumbra: all in shade a penumbra under the horizon, all lit a
/// penumbra over it.
pub fn shade(t: &LodShadowTunables, horizon: f64, rise: f64) -> f64 {
    let cot = 1.0 / t.march.penumbra_deg.to_radians().tan();
    smoothstep(-1.0, 1.0, (rise - horizon) * cot)
}

/// How much of the light at `point` (world, on the terrain) on face `f` the terrain in `maps`
/// lets through toward `light` (world, unit), 0–1: [`shade`] of the [`horizon`] at its column, as
/// the fragment reads it; 1 with the light under the face's horizon (the direct term is gone
/// anyway) or straight overhead.
pub fn visibility(
    t: &LodShadowTunables,
    tier: &LodShadowTier,
    maps: &[HeightMap],
    f: FaceFrame,
    point: [f64; 3],
    light: [f64; 3],
) -> f64 {
    let l = f.vector(light);
    let horizontal = (l[0] * l[0] + l[2] * l[2]).sqrt();
    if l[1] <= 0.0 || horizontal < 1e-4 {
        return 1.0;
    }
    let p = f.point(point);
    if !maps.iter().any(|m| m.holds(p[0], p[2])) {
        return 1.0;
    }
    shade(t, horizon(t, tier, maps, f, [p[0], p[2]], light), l[1] / horizontal)
}

/// The shadows fading out toward the outermost map's edge: 1 inside, 0 at the edge, by the
/// point's farther offset (x or z, m) from that map's centre.
pub fn edge_weight(t: &LodShadowTunables, half_width: f64, offset: f64) -> f64 {
    1.0 - smoothstep(half_width * (1.0 - t.march.edge_fade), half_width, offset)
}

/// The factor on the direct sun and moon term (`shadow_light`): `stand_in` the first layer's
/// (direct × the sky light's ramp), `cascade` the shadow maps' PCF where a cascade holds the
/// point, `near` 1 inside the cascades fading to 0 over their last `edgeFade` (the view depth's
/// smoothstep), `strength` the light's (the time of day's fade), `far` this layer's visibility
/// (1 where it doesn't reach). Inside the cascades it is theirs, past them the stand-in shaded by
/// the far shadow, and between a blend of the two by `near`, so neither edge shows; with `far` 1
/// it is the second layer's alone (`mix(stand_in, pcf, strength × near)`).
pub fn combine(stand_in: f64, cascade: Option<f64>, near: f64, strength: f64, far: f64) -> f64 {
    let past = stand_in * (1.0 + strength * (far - 1.0));
    match cascade {
        None => past,
        Some(pcf) => {
            let inside = stand_in + strength * (pcf - stand_in);
            past + near * (inside - past)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LodShadowTunables = LOD_SHADOWS;

    /// A ridge 200 m over a plain at 40 m (face-local), running along z at x = 0, 80 m wide with
    /// sloped flanks; the plain and its valley lie to the +x side. The field is face-local: on face
    /// B the same ridge in the mirror image.
    fn ridge(x: f64, _z: f64) -> Option<f64> {
        let base = 40.0;
        let top = 240.0;
        let d = x.abs();
        Some(if d < 40.0 {
            top
        } else if d < 240.0 {
            top - (d - 40.0)
        } else {
            base
        })
    }

    /// A light toward −x (behind the ridge, seen from the valley at +x) at `elevation` degrees
    /// over face `f`'s horizon, in world terms.
    fn light(f: FaceFrame, elevation: f64) -> [f64; 3] {
        let e = elevation.to_radians();
        f.vector([-e.cos(), e.sin(), 0.0])
    }

    /// The tier's maps of the ridge around (`cx`, 0), as the GPU would hold them.
    fn maps(tier: &LodShadowTier, cx: f64) -> Vec<HeightMap> {
        (0..tier.levels)
            .map(|l| {
                let texel = tier.texel(l);
                HeightMap::from_fn([cx, 0.0], texel, tier.resolution, max_mips(tier.resolution), |x, z| {
                    ridge(x, z).expect("everywhere")
                })
            })
            .collect()
    }

    fn valley_point(f: FaceFrame, x: f64) -> ([f64; 3], [f64; 3]) {
        (f.point([x, 40.0, 12.0]), f.vector([0.0, 1.0, 0.0]))
    }

    dwell_world::both_faces!(
        fn a_valley_behind_a_ridge_is_shaded_at_dusk_and_lit_at_noon(f: FaceFrame) {
            let tier = T.tiers[1];
            let ms = maps(&tier, 1000.0);
            let at = |x: f64, elevation: f64| {
                let (p, _) = valley_point(f, x);
                visibility(&T, &tier, &ms, f, p, light(f, elevation))
            };
            // 600 m out from the ridge: the ridge's 200 m stand 15–20° over the valley's horizon.
            assert!(at(600.0, 8.0) < 0.01, "low sun behind the ridge: {}", at(600.0, 8.0));
            assert!(at(600.0, 12.0) < 0.01);
            // A high sun clears it.
            assert!(at(600.0, 45.0) > 0.99, "high sun: {}", at(600.0, 45.0));
            assert!(at(600.0, 80.0) > 0.99);
            // Far out on the plain the low sun clears the ridge: the shadow has an end.
            assert!(at(4500.0, 8.0) > 0.99, "past the shadow: {}", at(4500.0, 8.0));
            // The sun on the valley's side: no shadow from the ridge behind it.
            let (p, _) = valley_point(f, 600.0);
            let front = f.vector([1.0, 8f64.to_radians().tan(), 0.0]);
            let len = (front[0] * front[0] + front[1] * front[1]).sqrt();
            let front = front.map(|c| c / len);
            assert!(visibility(&T, &tier, &ms, f, p, front) > 0.99);
            // The ridge's own crest is lit by the sun behind it (the bias keeps it off itself).
            let crest = f.point([0.0, 240.0, 12.0]);
            assert!(
                visibility(&T, &tier, &ms, f, crest, light(f, 8.0)) > 0.99,
                "the crest shades itself"
            );
        }
    );

    dwell_world::both_faces!(
        fn the_shadows_edge_is_soft_and_moves_out_as_the_sun_sinks(f: FaceFrame) {
            let tier = T.tiers[1];
            let ms = maps(&tier, 1000.0);
            // The shadow's end on the plain at an elevation: the first x (10 m steps) lit over half.
            let end = |elevation: f64| {
                (0..600)
                    .map(|i| 240.0 + f64::from(i) * 10.0)
                    .find(|&x| {
                        let (p, _) = valley_point(f, x);
                        visibility(&T, &tier, &ms, f, p, light(f, elevation)) > 0.5
                    })
                    .expect("the shadow ends")
            };
            let (e20, e10) = (end(20.0), end(10.0));
            // A ridge 200 m over the plain casts about 200 / tan(elevation) past its crest's edge.
            let expect = |e: f64| 40.0 + 200.0 / f64::to_radians(e).tan();
            assert!((e20 - expect(20.0)).abs() < 60.0, "{e20} vs {}", expect(20.0));
            assert!((e10 - expect(10.0)).abs() < 120.0, "{e10} vs {}", expect(10.0));
            assert!(e10 > e20);
            // Soft: some point near the end is partly lit.
            let partial = (0..40).any(|i| {
                let (p, _) = valley_point(f, e20 - 200.0 + f64::from(i) * 10.0);
                let v = visibility(&T, &tier, &ms, f, p, light(f, 20.0));
                v > 0.05 && v < 0.95
            });
            assert!(partial, "a penumbra");
        }
    );

    // both-faces: the parity check itself (each face's result against face A's).
    #[test]
    fn both_faces_shade_the_mirror_image_alike() {
        let tier = T.tiers[0];
        let ms = maps(&tier, 1000.0);
        for x in [300.0, 600.0, 1200.0, 2000.0] {
            for e in [5.0, 10.0, 20.0, 40.0] {
                let v: Vec<f64> = [FaceFrame::A, FaceFrame::B]
                    .into_iter()
                    .map(|f| {
                        let (p, _) = valley_point(f, x);
                        visibility(&T, &tier, &ms, f, p, light(f, e))
                    })
                    .collect();
                assert!((v[0] - v[1]).abs() < 1e-9, "x {x} e {e}: {v:?}");
            }
        }
    }

    #[test]
    fn no_light_over_the_horizon_and_no_map_shade_nothing() {
        let tier = T.tiers[1];
        let ms = maps(&tier, 1000.0);
        for f in FaceFrame::under_test() {
            let (p, _) = valley_point(f, 600.0);
            let flat = f.vector([-1.0, 0.0, 0.0]);
            assert_eq!(visibility(&T, &tier, &ms, f, p, flat), 1.0);
            let below = f.vector([-0.8, -0.6, 0.0]);
            assert_eq!(visibility(&T, &tier, &ms, f, p, below), 1.0);
            assert_eq!(visibility(&T, &tier, &[], f, p, light(f, 5.0)), 1.0);
        }
    }

    #[test]
    fn the_cascades_edge_blends_into_the_far_shadow_without_a_step() {
        // Across the cascades' fade (near 1 → 0 by the view depth's smoothstep), for any mix of
        // the maps' PCF, the far shadow and the light's strength, the factor moves continuously
        // from the cascades' to the far shadow's: no step bigger than the smoothstep's own slope
        // allows at 1/1000 of the band.
        let (start, end) = (128.0, 160.0);
        for stand_in in [1.0, 0.6] {
            for pcf in [0.0, 0.3, 1.0] {
                for far in [0.0, 0.5, 1.0] {
                    for strength in [1.0, 0.4] {
                        let at = |depth: f64| {
                            let near = 1.0 - smoothstep(start, end, depth);
                            combine(stand_in, Some(pcf), near, strength, far)
                        };
                        let mut prev = at(start - 1.0);
                        let steps = 2000;
                        for i in 0..=steps {
                            let d = start - 1.0 + (end - start + 2.0) * f64::from(i) / f64::from(steps);
                            let v = at(d);
                            assert!((v - prev).abs() < 2e-3, "step at {d}: {prev} → {v}");
                            prev = v;
                        }
                        // The ends are each layer's alone.
                        let inside = stand_in + strength * (pcf - stand_in);
                        assert!((at(start - 1.0) - inside).abs() < 1e-12);
                        let past = stand_in * (1.0 + strength * (far - 1.0));
                        assert!((at(end + 1.0) - past).abs() < 1e-12);
                        // Past the cascades (none holds the point) it is the far shadow's.
                        assert!((combine(stand_in, None, 0.3, strength, far) - past).abs() < 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn with_no_far_shadow_the_blend_is_the_cascades_alone() {
        // The second layer's factor before this layer: mix(stand_in, pcf, strength × near).
        for stand_in in [1.0, 0.7, 0.2] {
            for pcf in [0.0, 0.5, 1.0] {
                for near in [0.0, 0.25, 1.0] {
                    for strength in [0.0, 0.5, 1.0] {
                        let w = strength * near;
                        let before = stand_in + w * (pcf - stand_in);
                        let now = combine(stand_in, Some(pcf), near, strength, 1.0);
                        assert!((before - now).abs() < 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn the_edge_fades_out_over_its_last_share() {
        let half = 2048.0;
        assert_eq!(edge_weight(&T, half, 0.0), 1.0);
        assert_eq!(edge_weight(&T, half, half * (1.0 - T.march.edge_fade)), 1.0);
        assert_eq!(edge_weight(&T, half, half), 0.0);
        let mid = edge_weight(&T, half, half * (1.0 - T.march.edge_fade * 0.5));
        assert!(mid > 0.4 && mid < 0.6);
    }

    #[test]
    fn the_tiers_follow_the_shadows_setting() {
        assert!(T.tier(ShadowQuality::Off).is_none());
        assert_eq!(T.tier(ShadowQuality::Low), Some(&T.tiers[0]));
        assert_eq!(T.tier(ShadowQuality::High), Some(&T.tiers[1]));
        let high = T.tiers[1];
        assert_eq!(high.texel(1), high.texel_m * f64::from(high.level_scale));
        assert!(high.reach_m <= high.half_width(high.levels - 1));
    }
}
