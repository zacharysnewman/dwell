//! Sun and moon shadows by cascaded shadow maps (docs/DAY_NIGHT.md §10; the lighting's second
//! layer). Each frame the light that casts (dwell-look `shadows::caster`: the sun over the
//! viewer's face, else the moon) gets up to four orthographic depth maps around the camera, one per
//! slice of the view's depth. Each cascade is fitted to its slice's bounding sphere (whose radius
//! doesn't change as the camera turns) and its origin is snapped to whole texels in light space (so
//! moving doesn't make the edges shimmer). The scene's shaders (`SHADOW_WGSL`) pick a fragment's
//! cascade, look it up with a soft PCF, normal-offset and biased, and the result multiplies only the
//! direct sun and moon term (`lambert_lit`'s `shadow`); past the last cascade it fades to the first
//! layer's stand-in.
//!
//! Everything is camera-relative, as the rest of the renderer: a cascade's matrix maps a point
//! relative to the eye to its map, and only the snapping reads world coordinates (in f64).

use dwell_look::shadows::{Caster, MAX_CASCADES, SHADOWS, ShadowQuality, ShadowTier, ShadowTunables};

use super::camera::{Mat4, V3, cross, dot, normalize};

/// The Shadows setting an adapter draws: a CPU adapter draws Low at most (its default; every map
/// is the CPU's time there), a GPU the setting.
pub fn shadows_for(device_type: wgpu::DeviceType, quality: ShadowQuality) -> ShadowQuality {
    if device_type == wgpu::DeviceType::Cpu && quality == ShadowQuality::High {
        ShadowQuality::Low
    } else {
        quality
    }
}

/// The shadow map's format.
pub const SHADOW_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The light's axes (world): `z` toward the light, `x` and `y` across its maps. They follow the
/// light alone (never the camera), so a cascade's texel grid stays put in the world as the camera
/// moves and turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightBasis {
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

/// The light's axes for a light toward `dir`, its maps' y as near `reference` as it can be (the
/// sky's turning axis: the sun's path turns about it, so the maps turn smoothly with the sun).
pub fn light_basis(dir: V3, reference: V3) -> LightBasis {
    let z = normalize(dir);
    let mut r = reference;
    if dot(normalize(r), z).abs() > 0.99 {
        r = [1.0, 0.0, 0.0];
        if dot(r, z).abs() > 0.99 {
            r = [0.0, 0.0, 1.0];
        }
    }
    let x = normalize(cross(r, z));
    let y = cross(z, x);
    LightBasis { x, y, z }
}

/// The view depths the cascades split at (`count + 1` of them, from `near` to `far`): the
/// practical split, `lambda` of the way from even to logarithmic.
pub fn splits(count: usize, near: f64, far: f64, lambda: f64) -> Vec<f64> {
    (0..=count)
        .map(|i| {
            let t = i as f64 / count as f64;
            let log = near * (far / near).powf(t);
            let even = near + (far - near) * t;
            lambda * log + (1.0 - lambda) * even
        })
        .collect()
}

/// The smallest sphere around the slice of the view between depths `d0` and `d1` (half-angle
/// tangents `tan_x`, `tan_y`): its centre's view depth and its radius. It depends on the slice's
/// shape alone, so turning the camera leaves it the same (the stable fit).
pub fn slice_sphere(d0: f64, d1: f64, tan_x: f64, tan_y: f64) -> (f64, f64) {
    let k2 = tan_x * tan_x + tan_y * tan_y;
    // The centre on the axis equidistant from the near and far corners, unless that is past the
    // far face (a wide slice): then the far face's centre, which holds the near corners too.
    let z = ((d1 * d1 - d0 * d0) * (1.0 + k2)) / (2.0 * (d1 - d0));
    if z >= d1 {
        (d1, d1 * k2.sqrt())
    } else {
        let z = z.max(d0);
        (z, ((d1 - z).powi(2) + d1 * d1 * k2).sqrt())
    }
}

/// One cascade this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    /// The slice's view depths.
    pub near: f64,
    pub far: f64,
    /// Its box's half-size across the light (m): the slice's sphere's radius, rounded up.
    pub radius: f64,
    /// Its box's centre (world), snapped to whole texels across the light.
    pub origin: V3,
    /// The box's centre relative to the eye.
    pub origin_rel: V3,
    /// A texel's size (m).
    pub texel_m: f64,
    /// The box's depth along the light (m): its sphere and the casters' reach above it.
    pub depth_m: f64,
    pub basis: LightBasis,
    /// Eye-relative points to the map: xy in [−1, 1] (clip space), z in [0, 1] from the light.
    pub view_proj: Mat4,
}

/// What a frame's cascades are fitted from.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The eye (world).
    pub eye: V3,
    /// The camera's view axis (unit, world).
    pub forward: V3,
    /// The view's half-angle tangents.
    pub tan_x: f64,
    pub tan_y: f64,
}

/// Fits a cascade to the slice `[near, far]` of `view`: around the slice's sphere (its radius
/// rounded up to `radius_step`), its centre snapped to the map's texels across the light, deep
/// enough to hold casters `reach` metres above it toward the light.
pub fn fit(
    view: &View,
    basis: LightBasis,
    near: f64,
    far: f64,
    resolution: u32,
    radius_step: f64,
    reach: f64,
) -> Cascade {
    let (centre_depth, r) = slice_sphere(near, far, view.tan_x, view.tan_y);
    let radius = (r / radius_step).ceil().max(1.0) * radius_step;
    let texel_m = 2.0 * radius / f64::from(resolution);
    let centre = std::array::from_fn::<f64, 3, _>(|k| view.eye[k] + view.forward[k] * centre_depth);
    let snap = |axis: V3| (dot(centre, axis) / texel_m).round() * texel_m;
    let (u, v, w) = (snap(basis.x), snap(basis.y), dot(centre, basis.z));
    let origin = std::array::from_fn(|k| basis.x[k] * u + basis.y[k] * v + basis.z[k] * w);
    let origin_rel: V3 = std::array::from_fn(|k| origin[k] - view.eye[k]);
    let depth_m = 2.0 * radius + reach;
    // Rows of the eye-relative point → map transform (column-major below).
    let rows: [[f64; 4]; 3] = [
        {
            let a = basis.x.map(|c| c / radius);
            [a[0], a[1], a[2], -dot(origin_rel, basis.x) / radius]
        },
        {
            let a = basis.y.map(|c| c / radius);
            [a[0], a[1], a[2], -dot(origin_rel, basis.y) / radius]
        },
        {
            // Depth 0 at the box's face toward the light (`radius + reach` up), 1 at its far face.
            let a = basis.z.map(|c| -c / depth_m);
            [a[0], a[1], a[2], (radius + reach + dot(origin_rel, basis.z)) / depth_m]
        },
    ];
    let mut view_proj = [[0.0f32; 4]; 4];
    for (c, col) in view_proj.iter_mut().enumerate() {
        for (r, row) in rows.iter().enumerate() {
            col[r] = row[c] as f32;
        }
        col[3] = if c == 3 { 1.0 } else { 0.0 };
    }
    Cascade {
        near,
        far,
        radius,
        origin,
        origin_rel,
        texel_m,
        depth_m,
        basis,
        view_proj,
    }
}

impl Cascade {
    /// Where an eye-relative point falls in the map: clip x, y and the depth (f64; the shader's
    /// matrix in full precision).
    pub fn project(&self, rel: V3) -> V3 {
        let d: V3 = std::array::from_fn(|k| rel[k] - self.origin_rel[k]);
        let reach = self.depth_m - 2.0 * self.radius;
        [
            dot(d, self.basis.x) / self.radius,
            dot(d, self.basis.y) / self.radius,
            (self.radius + reach - dot(d, self.basis.z)) / self.depth_m,
        ]
    }

    /// Whether a box (its centre relative to the eye and half-size, axis-aligned) can cast into
    /// this cascade: it overlaps the cascade's box, which reaches up toward the light.
    pub fn touches(&self, centre_rel: V3, half: V3) -> bool {
        let d: V3 = std::array::from_fn(|k| centre_rel[k] - self.origin_rel[k]);
        let extent = |a: V3| (0..3).map(|k| a[k].abs() * half[k]).sum::<f64>();
        let reach = self.depth_m - 2.0 * self.radius;
        let (x, y, z) = (dot(d, self.basis.x), dot(d, self.basis.y), dot(d, self.basis.z));
        let (ex, ey, ez) = (
            extent(self.basis.x),
            extent(self.basis.y),
            extent(self.basis.z),
        );
        x.abs() <= self.radius + ex
            && y.abs() <= self.radius + ey
            && z >= -self.radius - ez
            && z <= self.radius + reach + ez
    }
}

/// This frame's shadows: the light, the tier and its cascades (none: no shadows drawn).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub caster: Caster,
    pub tier: ShadowTier,
    pub cascades: Vec<Cascade>,
    /// The shadow distance (m of view depth) and where its fade to the stand-in starts.
    pub distance: f64,
    pub fade_start: f64,
}

/// The frame's shadow plan: for `quality` (already `shadows_for` the adapter), the light `caster`
/// (none when neither light is up), `view`, the sky's axis (the light's map axes) and the
/// spyglass's `magnification` (the shadow distance grows with it, so the zoomed view has shadows
/// where it looks, up to `spyglassMaxM`).
pub fn plan(
    t: &ShadowTunables,
    quality: ShadowQuality,
    caster: Option<Caster>,
    view: &View,
    sky_axis: V3,
    magnification: f64,
) -> Option<Plan> {
    let tier = *t.tier(quality)?;
    let caster = caster.filter(|c| c.strength > 1e-3)?;
    let c = &t.cascades;
    let distance = (tier.distance_m * magnification.max(1.0)).min(c.spyglass_max_m.max(tier.distance_m));
    let count = (tier.cascades as usize).clamp(1, MAX_CASCADES);
    let basis = light_basis(caster.dir, sky_axis);
    let depths = splits(count, c.near_m.min(distance * 0.5), distance, tier.split_lambda);
    // The first slice reaches back to the eye (the near plane): what is under your feet.
    let cascades = (0..count)
        .map(|i| {
            let near = if i == 0 { 0.0 } else { depths[i] };
            fit(
                view,
                basis,
                near,
                depths[i + 1],
                tier.resolution,
                c.radius_step_m,
                c.caster_reach_m,
            )
        })
        .collect();
    Some(Plan {
        caster,
        tier,
        cascades,
        distance,
        fade_start: distance * (1.0 - c.edge_fade),
    })
}

/// Floats in the shadow uniform block (`Shadows` in `SHADOW_WGSL`).
pub const UNIFORM_FLOATS: usize = MAX_CASCADES * 16 + MAX_CASCADES * 4 + 8;

/// The shadow uniform block: each cascade's matrix; per cascade its texel (m), the normal offset
/// (m), the compare's bias (map depth) and the filter's radius (texels); then the cascade count
/// (0: no shadows), the strength, the fade's start and end (view depth); the filter's taps a
/// side, the map's size (texels).
pub fn uniforms(t: &ShadowTunables, plan: Option<&Plan>) -> [f32; UNIFORM_FLOATS] {
    let mut u = [0.0f32; UNIFORM_FLOATS];
    let Some(p) = plan else {
        return u;
    };
    for (i, c) in p.cascades.iter().enumerate() {
        for (k, col) in c.view_proj.iter().enumerate() {
            u[i * 16 + k * 4..i * 16 + k * 4 + 4].copy_from_slice(col);
        }
        let at = MAX_CASCADES * 16 + i * 4;
        u[at..at + 4].copy_from_slice(&[
            c.texel_m as f32,
            (c.texel_m * t.bias.normal_offset_texels) as f32,
            (t.bias.depth_bias_m / c.depth_m) as f32,
            p.tier.filter_texels as f32,
        ]);
    }
    let at = MAX_CASCADES * 20;
    u[at..at + 8].copy_from_slice(&[
        p.cascades.len() as f32,
        p.caster.strength as f32,
        p.fade_start as f32,
        p.distance as f32,
        p.tier.filter_taps as f32,
        p.tier.resolution as f32,
        0.0,
        0.0,
    ]);
    u
}

/// The shadow lookup the scene's shaders share (group 0, bindings 8–10), after `COMMON`'s
/// helpers. `shadow_light(rel, n, stand_in)` is the factor on the direct sun and moon term: the
/// map's visibility near the eye, fading to `stand_in` (the first layer's: direct light × sky
/// light) past the last cascade and as the light's shadows fade with the time of day.
pub const SHADOW_WGSL: &str = r"
struct Shadows {
    cascades: array<mat4x4f, 4>,
    // Per cascade: x its texel (m), y the normal offset (m), z the compare's bias (map depth),
    // w the filter's radius (texels).
    texel: array<vec4f, 4>,
    // x cascades (0: none), y strength, z the fade to the stand-in's start, w its end (view depth).
    cfg: vec4f,
    // x the filter's taps a side, y the map's size (texels).
    filter: vec4f,
};
@group(0) @binding(8) var<uniform> shadows: Shadows;
@group(0) @binding(9) var shadow_map: texture_depth_2d_array;
@group(0) @binding(10) var shadow_sampler: sampler_comparison;

// The map's visibility of an eye-relative point in cascade `c` (1 lit, 0 shadowed): a grid of
// `taps`² hardware 2×2 compares over the filter's radius, a soft PCF.
fn shadow_pcf(c: i32, p: vec3f) -> f32 {
    let size = shadows.filter.y;
    let uv = vec2f(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let z = p.z - shadows.texel[c].z;
    let taps = i32(shadows.filter.x);
    let radius = shadows.texel[c].w / size;
    if (taps <= 1) {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, c, z);
    }
    let step = 2.0 * radius / f32(taps - 1);
    var sum = 0.0;
    for (var j = 0; j < taps; j++) {
        for (var i = 0; i < taps; i++) {
            let o = vec2f(f32(i), f32(j)) * step - radius;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, c, z);
        }
    }
    return sum / f32(taps * taps);
}

// The first cascade whose map holds an eye-relative point (with the filter's margin), or −1.
fn shadow_cascade(rel: vec3f) -> i32 {
    let count = i32(shadows.cfg.x);
    for (var c = 0; c < count; c++) {
        let p = shadows.cascades[c] * vec4f(rel, 1.0);
        let margin = 1.0 - (shadows.texel[c].w + 2.0) * 2.0 / shadows.filter.y;
        if (all(abs(p.xy) <= vec2f(margin)) && p.z >= 0.0 && p.z <= 1.0) {
            return c;
        }
    }
    return -1;
}

// Clouds' shadows on the terrain (CLOUDS.md §6, a future item, Phase 31's C7): the deck's
// transmittance toward the light over an eye-relative point. None yet.
fn cloud_shadow(rel: vec3f) -> f32 {
    return 1.0;
}

// The factor on the direct sun and moon term at an eye-relative point with normal `n`: the map's
// visibility near the eye, `stand_in` (the first layer's) beyond the last cascade and where the
// light's shadows are faded by the time of day, and no visible edge between them.
fn shadow_light(rel: vec3f, n: vec3f, stand_in: f32) -> f32 {
    if (shadows.cfg.x < 0.5) {
        return stand_in * cloud_shadow(rel);
    }
    let depth = view_depth(rel);
    let near = 1.0 - smoothstep(shadows.cfg.z, shadows.cfg.w, depth);
    let w = shadows.cfg.y * near;
    if (w <= 0.0) {
        return stand_in * cloud_shadow(rel);
    }
    let c = shadow_cascade(rel);
    if (c < 0) {
        return stand_in * cloud_shadow(rel);
    }
    // Normal offset: the lookup moves out along the surface's normal by the cascade's texels.
    let at = rel + n * shadows.texel[c].y;
    let p = (shadows.cascades[c] * vec4f(at, 1.0)).xyz;
    return mix(stand_in, shadow_pcf(c, p), w) * cloud_shadow(rel);
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::daynight::DAY_NIGHT;
    use dwell_look::shadows::caster;

    fn view(eye: V3, forward: V3) -> View {
        View {
            eye,
            forward: normalize(forward),
            tan_x: 1.3,
            tan_y: 0.75,
        }
    }

    fn corners(v: &View, d0: f64, d1: f64) -> Vec<V3> {
        let f = v.forward;
        let right = normalize(cross(f, [0.0, 1.0, 0.0]));
        let up = cross(right, f);
        let mut out = Vec::new();
        for d in [d0, d1] {
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    out.push(std::array::from_fn(|k| {
                        f[k] * d + right[k] * sx * v.tan_x * d + up[k] * sy * v.tan_y * d
                    }));
                }
            }
        }
        out
    }

    #[test]
    fn the_splits_run_from_near_to_far_between_even_and_logarithmic() {
        let even = splits(4, 1.0, 161.0, 0.0);
        assert_eq!(even, vec![1.0, 41.0, 81.0, 121.0, 161.0]);
        let log = splits(2, 1.0, 100.0, 1.0);
        assert!((log[1] - 10.0).abs() < 1e-9);
        let mid = splits(4, 0.5, 160.0, 0.75);
        assert_eq!((mid[0], mid[4]), (0.5, 160.0));
        assert!(mid.windows(2).all(|w| w[1] > w[0]));
        // Between the two: nearer than even, farther than log.
        assert!(mid[1] < splits(4, 0.5, 160.0, 0.0)[1] && mid[1] > splits(4, 0.5, 160.0, 1.0)[1]);
    }

    #[test]
    fn a_slices_sphere_holds_its_corners_and_no_wider_than_it_must() {
        for (d0, d1) in [(0.0, 8.0), (8.0, 30.0), (30.0, 160.0), (100.0, 101.0)] {
            let v = view([0.0; 3], [0.0, 0.0, -1.0]);
            let (z, r) = slice_sphere(d0, d1, v.tan_x, v.tan_y);
            let centre = [0.0, 0.0, -z];
            let far: f64 = corners(&v, d0, d1)
                .iter()
                .map(|c| dot(sub(*c, centre), sub(*c, centre)).sqrt())
                .fold(0.0, f64::max);
            assert!(far <= r + 1e-9, "{d0}–{d1}: corner {far} out of {r}");
            // Tight: the farthest corner is on the sphere.
            assert!(far > r - 1e-6, "{d0}–{d1}: {far} well inside {r}");
        }
    }

    fn sub(a: V3, b: V3) -> V3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    #[test]
    fn each_cascade_holds_its_slice_and_the_casters_above_it() {
        let sun = normalize([0.3, 0.8, -0.4]);
        let basis = light_basis(sun, [0.0, 0.0, 1.0]);
        for forward in [[0.0, 0.0, -1.0], [1.0, -0.6, 0.2], [-0.3, 0.9, 0.1]] {
            let v = view([1234.5, 80.25, -987.75], forward);
            let d = splits(3, 0.5, 120.0, 0.7);
            for i in 0..3 {
                let c = fit(&v, basis, d[i], d[i + 1], 2048, 1.0, 300.0);
                for p in corners(&v, d[i], d[i + 1]) {
                    let q = c.project(p);
                    assert!(q[0].abs() <= 1.0 && q[1].abs() <= 1.0, "{q:?}");
                    assert!((0.0..=1.0).contains(&q[2]), "{q:?}");
                    // A caster 250 m up toward the light still lands in the map's depth.
                    let up: V3 = std::array::from_fn(|k| p[k] + sun[k] * 250.0);
                    assert!((0.0..=1.0).contains(&c.project(up)[2]));
                    assert!(c.touches(up, [0.5; 3]));
                }
                // The f32 matrix agrees with the f64 projection.
                let p = corners(&v, d[i], d[i + 1])[5];
                let m = c.view_proj;
                let g: Vec<f64> = (0..3)
                    .map(|r| {
                        (0..3).map(|k| f64::from(m[k][r]) * p[k]).sum::<f64>() + f64::from(m[3][r])
                    })
                    .collect();
                let q = c.project(p);
                for k in 0..3 {
                    assert!((g[k] - q[k]).abs() < 1e-4, "{g:?} vs {q:?}");
                }
            }
        }
    }

    #[test]
    fn turning_the_camera_keeps_each_cascades_size() {
        let basis = light_basis(normalize([0.2, 0.9, 0.3]), [0.0, 0.0, 1.0]);
        let a = fit(&view([5.0, 40.0, 5.0], [0.0, 0.0, -1.0]), basis, 10.0, 40.0, 1024, 1.0, 200.0);
        for f in [[1.0, 0.0, 0.0], [0.3, -0.8, 0.5], [0.0, 1.0, 0.01]] {
            let b = fit(&view([5.0, 40.0, 5.0], f), basis, 10.0, 40.0, 1024, 1.0, 200.0);
            assert_eq!((a.radius, a.texel_m), (b.radius, b.texel_m));
        }
        // A radius rounds up to its step: a slightly wider view keeps it.
        let mut wider = view([5.0, 40.0, 5.0], [0.0, 0.0, -1.0]);
        wider.tan_x *= 1.001;
        let w = fit(&wider, basis, 10.0, 40.0, 1024, 1.0, 200.0);
        assert_eq!(w.radius, a.radius);
    }

    /// A world point's place in the map, in texels.
    fn texel_of(c: &Cascade, eye: V3, world: V3, resolution: f64) -> [f64; 2] {
        let q = c.project(sub(world, eye));
        [(q[0] * 0.5 + 0.5) * resolution, (0.5 - q[1] * 0.5) * resolution]
    }

    #[test]
    fn moving_a_sub_texel_step_keeps_the_light_space_origin_and_the_texel_grid() {
        let basis = light_basis(normalize([0.4, 0.7, -0.2]), [0.0, 0.0, 1.0]);
        let res = 2048;
        let start = [10_000.25, 72.5, -3_000.75];
        let c0 = fit(&view(start, [0.2, -0.3, -1.0]), basis, 0.0, 12.0, res, 1.0, 300.0);
        let step = c0.texel_m * 0.1;
        let landmark = [10_003.0, 70.0, -3_005.0];
        let grid = texel_of(&c0, start, landmark, f64::from(res));
        let mut kept = 0;
        let mut last = c0.origin;
        for k in 1..=30 {
            // Sub-texel steps along the map's x and the light's own axis (depth only).
            let eye: V3 = std::array::from_fn(|i| {
                start[i] + basis.x[i] * step * f64::from(k) + basis.z[i] * 0.37 * f64::from(k)
            });
            let c = fit(&view(eye, [0.2, -0.3, -1.0]), basis, 0.0, 12.0, res, 1.0, 300.0);
            let moved: V3 = sub(c.origin, last);
            let across = [dot(moved, basis.x), dot(moved, basis.y)];
            // The origin moves by whole texels across the light, or not at all.
            for a in across {
                let t = a / c.texel_m;
                assert!((t - t.round()).abs() < 1e-6, "{t} texels");
                assert!(t.abs() <= 1.0 + 1e-6);
            }
            if across.iter().all(|a| (a / c.texel_m).abs() < 1e-6) {
                kept += 1;
            }
            last = c.origin;
            // The texel grid stays put in the world: a fixed point's place in the map moves by
            // whole texels only.
            let g = texel_of(&c, eye, landmark, f64::from(res));
            for i in 0..2 {
                let d = g[i] - grid[i];
                assert!((d - d.round()).abs() < 1e-4, "{d}");
            }
        }
        assert!(kept >= 26, "a tenth-texel step kept the origin only {kept} times of 30");
    }

    #[test]
    fn the_plan_follows_the_setting_the_light_and_the_spyglass() {
        let v = view([0.0, 60.0, 0.0], [0.0, -0.2, -1.0]);
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let sun = caster(&SHADOWS, &noon, 1.0);
        assert!(plan(&SHADOWS, ShadowQuality::Off, sun, &v, noon.axis, 1.0).is_none());
        let high = plan(&SHADOWS, ShadowQuality::High, sun, &v, noon.axis, 1.0).unwrap();
        let low = plan(&SHADOWS, ShadowQuality::Low, sun, &v, noon.axis, 1.0).unwrap();
        assert_eq!(high.cascades.len(), SHADOWS.tiers[1].cascades as usize);
        assert_eq!(low.cascades.len(), SHADOWS.tiers[0].cascades as usize);
        assert_eq!(high.distance, SHADOWS.tiers[1].distance_m);
        assert_eq!(high.cascades.last().unwrap().far, high.distance);
        assert_eq!(high.cascades[0].near, 0.0);
        // The slices tile the view's depth.
        for w in high.cascades.windows(2) {
            assert_eq!(w[0].far, w[1].near);
        }
        // No light up: no maps.
        assert!(plan(&SHADOWS, ShadowQuality::High, None, &v, noon.axis, 1.0).is_none());
        // The spyglass stretches the distance, up to its cap.
        let zoomed = plan(&SHADOWS, ShadowQuality::High, sun, &v, noon.axis, 4.0).unwrap();
        assert_eq!(
            zoomed.distance,
            (SHADOWS.tiers[1].distance_m * 4.0).min(SHADOWS.cascades.spyglass_max_m)
        );
        // A CPU adapter draws Low at most.
        assert_eq!(shadows_for(wgpu::DeviceType::Cpu, ShadowQuality::High), ShadowQuality::Low);
        assert_eq!(shadows_for(wgpu::DeviceType::Cpu, ShadowQuality::Off), ShadowQuality::Off);
        assert_eq!(
            shadows_for(wgpu::DeviceType::DiscreteGpu, ShadowQuality::High),
            ShadowQuality::High
        );
    }

    #[test]
    fn the_uniform_block_carries_the_cascades_and_none_when_off() {
        assert_eq!(uniforms(&SHADOWS, None), [0.0; UNIFORM_FLOATS]);
        let v = view([0.0, 60.0, 0.0], [0.0, -0.2, -1.0]);
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let p = plan(&SHADOWS, ShadowQuality::Low, caster(&SHADOWS, &noon, 1.0), &v, noon.axis, 1.0)
            .unwrap();
        let u = uniforms(&SHADOWS, Some(&p));
        let cfg = MAX_CASCADES * 20;
        assert_eq!(u[cfg], p.cascades.len() as f32);
        assert_eq!(u[cfg + 3], p.distance as f32);
        assert_eq!(u[cfg + 5], SHADOWS.tiers[0].resolution as f32);
        assert_eq!(u[0..4], p.cascades[0].view_proj[0]);
        assert_eq!(u[MAX_CASCADES * 16], p.cascades[0].texel_m as f32);
    }

    dwell_world::both_faces!(
        /// On either face the light over it casts down onto it: a point above the ground (toward
        /// the face's sky) is nearer the light in the map than the ground under it.
        fn the_light_over_either_face_casts_onto_it(f: FaceFrame) {
            // Face A's noon; face B's noon is face A's midnight.
            let time = if f.sign_f64() > 0.0 { 0.5 } else { 0.0 };
            let day = DAY_NIGHT.state(0.0, Some(time), Some(0.5));
            let c = caster(&SHADOWS, &day, f.sign_f64()).expect("the sun is up");
            assert!(!c.moon);
            let up = f.up();
            let eye = f.point([0.0, 60.0, 0.0]);
            let forward = f.vector([0.0, -0.3, -1.0]);
            let p = plan(&SHADOWS, ShadowQuality::High, Some(c), &view(eye, forward), day.axis, 1.0)
                .unwrap();
            let k = &p.cascades[0];
            let ground = [0.0, -1.7 * up[1], -3.0];
            let above: V3 = std::array::from_fn(|i| ground[i] + up[i] * 2.0);
            assert!(k.project(above)[2] < k.project(ground)[2]);
        }
    );
}
