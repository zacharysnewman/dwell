//! Fluid looks on the GPU (docs/WATER_RENDERING.md W1b, ADR 0030): the fluid table packed into the
//! storage buffer the water shader indexes by each vertex's fluid id, the water quality setting,
//! and the shader's maths in Rust — Schlick's Fresnel, the normal-detail layers and how each fades
//! out where a pixel covers more than its wavelength (the slope it loses widens the glint instead),
//! and the inverse of the composite's tone mapping (the sky's colour back to the scene's linear
//! light, for its reflection). The layer table and the tone-mapping matrices are emitted into the
//! WGSL from here (`wgsl`), so the shader and these tested twins share their numbers.

use dwell_blocks::FluidInfo;
use dwell_look::{hex, srgb_to_linear};

/// vec4s per fluid in the buffer (`Fluid` in the WGSL).
pub const FLUID_VEC4S: usize = 6;
/// Bytes per fluid.
pub const FLUID_STRIDE: u64 = (FLUID_VEC4S * 16) as u64;

/// The normal detail repeats every `WRAP_M` metres (each layer's wavelength is adjusted to divide
/// it), so the shader works on world x and z wrapped to [0, WRAP_M) per draw, exact at any
/// distance from the origin.
pub const WRAP_M: f64 = 1024.0;

/// A draw's origin wrapped for the normal detail (x and z, metres).
pub fn wrapped_origin(origin: [f64; 3]) -> [f32; 2] {
    [
        origin[0].rem_euclid(WRAP_M) as f32,
        origin[2].rem_euclid(WRAP_M) as f32,
    ]
}

/// One scrolling layer of normal detail: gradient noise on a lattice turned by an integer matrix
/// (so it still wraps at `WRAP_M`), scrolled along `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveLayer {
    /// Its wavelength over the fluid's `waveScale`.
    pub wavelength: f32,
    /// Its slope (height over wavelength) over the fluid's `normalScale`.
    pub slope: f32,
    /// Its scroll speed over the fluid's `flowSpeed`.
    pub speed: f32,
    /// Unit scroll direction (x, z).
    pub dir: [f32; 2],
    /// The lattice's turn: an integer matrix (columns), scaling by `sqrt(det)`.
    pub turn: [[i32; 2]; 2],
}

/// Large, medium and fine ripples (Low draws the first only).
pub const LAYERS: [WaveLayer; 3] = [
    WaveLayer {
        wavelength: 1.0,
        slope: 0.10,
        speed: 1.0,
        dir: [0.9578, 0.2873],
        turn: [[1, 0], [0, 1]],
    },
    WaveLayer {
        wavelength: 0.43,
        slope: 0.08,
        speed: 1.7,
        dir: [-0.5547, 0.8321],
        turn: [[1, -1], [1, 1]],
    },
    WaveLayer {
        wavelength: 0.17,
        slope: 0.06,
        speed: 2.6,
        dir: [0.2425, -0.9701],
        turn: [[2, 1], [-1, 2]],
    },
];

/// A layer is whole while a pixel covers under `FADE_LO` of its wavelength, gone past `FADE_HI`
/// (Nyquist: at least two pixels per wavelength).
pub const FADE_LO: f32 = 0.125;
pub const FADE_HI: f32 = 0.5;

/// The water quality setting (Low, Medium, High; WATER_RENDERING.md §4). W1b: Low draws one layer
/// of normal detail, Medium and High all three (they part with W3 and W5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaterQuality {
    Low,
    /// The desktop default.
    #[default]
    Medium,
    High,
}

impl WaterQuality {
    /// From the shell's level (0 Low, 1 Medium, 2 High; anything higher is High).
    pub fn from_level(level: u32) -> Self {
        match level {
            0 => Self::Low,
            1 => Self::Medium,
            _ => Self::High,
        }
    }
    /// What the frame uniform carries (`water.w`).
    pub fn level(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::Medium => 1.0,
            Self::High => 2.0,
        }
    }
    /// Whether the water reads the scene behind it (W3: refraction, depth absorption, foam and
    /// caustics, from a copy of the scene colour): Medium and High. Low keeps the W1b look, with
    /// no copies.
    pub fn reads_scene(self) -> bool {
        self != Self::Low
    }
    /// Layers of normal detail drawn.
    pub fn layers(self) -> usize {
        match self {
            Self::Low => 1,
            Self::Medium | Self::High => LAYERS.len(),
        }
    }
}

/// Fresnel reflectance at normal incidence from the index of refraction (air on the other side).
pub fn f0_from_ior(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    r * r
}

/// Schlick's approximation of the Fresnel reflectance at `cos` (the angle to the normal).
pub fn schlick(cos: f32, f0: f32) -> f32 {
    let m = (1.0 - cos.clamp(0.0, 1.0)).powi(5);
    f0 + (1.0 - f0) * m
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a layer of `wavelength` (m) is drawn where a pixel covers `footprint` (m).
pub fn layer_fade(footprint: f32, wavelength: f32) -> f32 {
    1.0 - smoothstep(wavelength * FADE_LO, wavelength * FADE_HI, footprint)
}

/// The noise tile (render/water_tiles.rs): gradient noise over `NOISE_TILE` lattice cells, wrapping,
/// at `NOISE_TEXELS` texels per cell, so `NOISE_RES` texels square.
pub const NOISE_TILE: i32 = 32;
pub const NOISE_TEXELS: u32 = 16;
pub const NOISE_RES: u32 = NOISE_TILE as u32 * NOISE_TEXELS;

/// The caustic tile: the cell-edge distances over `CAUSTIC_TILE` cells at `CAUSTIC_TEXELS` texels
/// per cell, `CAUSTIC_RES` square, drawn each frame.
pub const CAUSTIC_TILE: i32 = 16;
pub const CAUSTIC_TEXELS: u32 = 32;
pub const CAUSTIC_RES: u32 = CAUSTIC_TILE as u32 * CAUSTIC_TEXELS;

/// How many cells of about `want` metres span `WRAP_M`, rounded to a whole number of `tile`s (a
/// pattern sampled from a tile of `tile` cells then wraps with the position): (cells, cell size).
pub fn tile_cells(want: f32, tile: i32) -> (i32, f32) {
    let tiles = ((WRAP_M as f32) / (want.max(1e-3) * tile as f32))
        .round()
        .max(1.0);
    let n = tiles as i32 * tile;
    (n, WRAP_M as f32 / n as f32)
}

/// A layer's wavelength for a fluid's `wave_scale`, adjusted so a whole number of them spans
/// `WRAP_M` in whole noise tiles (the noise's period in lattice cells, and the wavelength).
pub fn layer_period(layer: &WaveLayer, wave_scale: f32) -> (i32, f32) {
    tile_cells(wave_scale * layer.wavelength, NOISE_TILE)
}

/// The surface's normal from the summed height slopes (dh/dx, dh/dz), toward `up` (±1: the face's
/// up, or −1 seen from below): the normal of y = h(x, z), turned over for −1.
pub fn blend_normal(up: f32, slope: [f32; 2]) -> [f32; 3] {
    let n = [-slope[0], 1.0, -slope[1]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    [up * n[0] / len, up * n[1] / len, up * n[2] / len]
}

/// The glint's GGX width (α) for a fluid's `roughness` (perceptual) and the slope variance the
/// faded layers no longer draw (Toksvig: what the normals lose, the highlight spreads by).
pub fn glint_alpha(roughness: f32, lost_variance: f32) -> f32 {
    let a = (roughness * roughness).max(MIN_ALPHA);
    (a * a + 2.0 * lost_variance).sqrt()
}

/// The narrowest glint (a sun's disc, roughly; and no division blow-up).
pub const MIN_ALPHA: f32 = 0.05;

fn linear_rgb(c: u32) -> [f32; 3] {
    hex(c).map(|v| srgb_to_linear(v) as f32)
}

/// The fluid table as the storage buffer holds it: `FLUID_VEC4S` vec4s per fluid, indexed by
/// fluid id (`Fluid` in the WGSL). Colours in linear light.
pub fn pack_fluids(fluids: &[FluidInfo]) -> Vec<f32> {
    let mut out = Vec::with_capacity(fluids.len().max(1) * FLUID_VEC4S * 4);
    for f in fluids {
        let s = linear_rgb(f.scatter);
        let foam = linear_rgb(f.foam_color);
        out.extend_from_slice(&[f.absorption[0], f.absorption[1], f.absorption[2], f.opacity]);
        out.extend_from_slice(&[s[0], s[1], s[2], f.roughness]);
        out.extend_from_slice(&[f.ior, f.normal_scale, f.wave_scale, f.flow_speed]);
        out.extend_from_slice(&[foam[0], foam[1], foam[2], f.foam_depth]);
        out.extend_from_slice(&[
            f0_from_ior(f.ior),
            f.caustics,
            f.surface_drop as f32,
            f.scattering,
        ]);
        out.extend_from_slice(&[f.refraction, f.caustic_scale, f.caustic_depth, f.sparkle]);
    }
    if out.is_empty() {
        // A storage binding can't be empty: one zero fluid that nothing indexes.
        out.resize(FLUID_VEC4S * 4, 0.0);
    }
    out
}

/// The length of a view ray inside the water (m): from the surface, at view depth `d_water`, to
/// the scene behind it at `d_behind` (0: nothing drawn there, so as deep as can matter), for a
/// surface point `rel_len` metres from the eye. View depths scale along a ray with distance.
pub fn ray_thickness(d_behind: f32, d_water: f32, rel_len: f32) -> f32 {
    if d_behind <= 0.0 {
        return DEEP_M;
    }
    ((d_behind - d_water).max(0.0) * rel_len / d_water.max(1e-4)).min(DEEP_M)
}

/// Deeper than any absorption can tell apart (m).
pub const DEEP_M: f32 = 1e4;

/// Refraction's foreground check: the offset sample shows the scene behind only when what it hits
/// is behind the water (or nothing); otherwise (a bank, a post in front) the undistorted one.
pub fn refracted_sample(offset_behind: f32, d_water: f32) -> bool {
    offset_behind <= 0.0 || offset_behind > d_water
}

/// Beer–Lambert: what is seen through `length` metres of a fluid with absorption `sigma` and
/// `scattering` (per metre). Along the ray the scene behind keeps exp(−(σ + s)·length) and the
/// fluid's lit body (its in-scatter) fills the rest; the scene was lit by light that came `down`
/// metres through the fluid, which keeps exp(−σ·down) of it.
pub fn through_fluid(
    behind: [f32; 3],
    body: [f32; 3],
    sigma: [f32; 3],
    scattering: f32,
    length: f32,
    down: f32,
) -> [f32; 3] {
    std::array::from_fn(|k| {
        let t = (-(sigma[k] + scattering) * length).exp();
        behind[k] * t * (-sigma[k] * down).exp() + body[k] * (1.0 - t)
    })
}

/// How strongly caustics show at `depth` metres under the surface: whole at the surface, 1/e at
/// the fluid's caustic depth.
pub fn caustic_fade(depth: f32, caustic_depth: f32) -> f32 {
    if depth <= 0.0 {
        return 0.0;
    }
    (-depth / caustic_depth.max(1e-3)).exp()
}

/// The shader's lattice wrap (`wrap_cell`): a cell coordinate in [0, period).
pub fn wrap_cell(c: i32, period: i32) -> i32 {
    ((c % period) + period) % period
}

/// The shader's neighbour step (`wrap_step`): a wrapped coordinate moved by −1, 0 or +1 without
/// another modulo.
pub fn wrap_step(w: i32, o: i32, period: i32) -> i32 {
    let v = w + o;
    if v < 0 {
        v + period
    } else if v >= period {
        v - period
    } else {
        v
    }
}

/// The shader's `hash_dir`: a unit vector from the low two bytes of a hash, without trig.
pub fn hash_dir(h: u32) -> [f32; 2] {
    let v = [(h & 0xff) as f32 - 127.5, ((h >> 8) & 0xff) as f32 - 127.5];
    let len = (v[0] * v[0] + v[1] * v[1]).max(1.0).sqrt();
    [v[0] / len, v[1] / len]
}

/// The caustic pattern's cell size for a fluid's `scale`, adjusted so a whole number of caustic
/// tiles spans `WRAP_M` (it is evaluated on wrapped positions): (cells per wrap, cell size).
pub fn caustic_period(scale: f32) -> (i32, f32) {
    tile_cells(scale, CAUSTIC_TILE)
}

/// three's ACES filmic fit (shaders.rs `aces`): the input and output matrices, by columns.
pub const ACES_INPUT: [[f32; 3]; 3] = [
    [0.59719, 0.07600, 0.02840],
    [0.35458, 0.90834, 0.13383],
    [0.04823, 0.01566, 0.83777],
];
pub const ACES_OUTPUT: [[f32; 3]; 3] = [
    [1.60475, -0.10208, -0.00327],
    [-0.53108, 1.10813, -0.07276],
    [-0.07367, -0.00605, 1.07602],
];
/// The RRT+ODT fit: (v² + a·v − b) / (c·v² + d·v + e).
const FIT: [f32; 5] = [0.0245786, 0.000090537, 0.983729, 0.432951, 0.238081];

fn mat_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2])
}

/// A 3×3 matrix's inverse (columns), in f64 for the WGSL constants.
fn mat_inverse(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let a = |c: usize, r: usize| f64::from(m[c][r]);
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        a(c0, r0) * a(c1, r1) - a(c1, r0) * a(c0, r1)
    };
    let det: f64 = (0..3).map(|c| a(c, 0) * cof(0, c)).sum();
    // inverse[c][r] (column c, row r) = cofactor(c, r) / det: the adjugate is the transpose.
    std::array::from_fn(|c| std::array::from_fn(|r| (cof(c, r) / det) as f32))
}

/// The tone mapping (`aces` in the WGSL, without the encode): linear scene colour → display
/// linear, for `exposure`.
pub fn aces(color: [f32; 3], exposure: f32) -> [f32; 3] {
    let k = exposure / 0.6;
    let v = mat_mul(&ACES_INPUT, color.map(|c| c * k));
    let [a, b, c, d, e] = FIT;
    let fit = v.map(|v| (v * (v + a) - b) / (v * (c * v + d) + e));
    mat_mul(&ACES_OUTPUT, fit).map(|c| c.clamp(0.0, 1.0))
}

/// The inverse of [`aces`] for colours it doesn't clip: what scene colour shows as `display`
/// (display linear). The sky is drawn in display colour; its reflection needs it in scene light.
pub fn inverse_aces(display: [f32; 3], exposure: f32) -> [f32; 3] {
    let [a, b, c, d, e] = FIT;
    let y = mat_mul(&mat_inverse(&ACES_OUTPUT), display).map(|y| y.clamp(0.0, INVERSE_FIT_MAX));
    // (1 − c·y)·v² + (a − d·y)·v − (b + e·y) = 0, the positive root.
    let v = y.map(|y| {
        let qa = 1.0 - c * y;
        let qb = a - d * y;
        let qc = -(b + e * y);
        (-qb + (qb * qb - 4.0 * qa * qc).sqrt()) / (2.0 * qa)
    });
    mat_mul(&mat_inverse(&ACES_INPUT), v).map(|x| (x * 0.6 / exposure).max(0.0))
}

/// The water's cost-isolation switches (a dev tool, `?waterdbg=`; docs/WATER_RENDERING.md §8): each
/// name turns one piece of the water's shading off, so its GPU time (`gpu.*.water`) can be told
/// apart. Carried to the shader as a bit mask in the frame uniform (`wrap.z`); 0 draws everything.
pub const DEBUG_SWITCHES: [(&str, u32); 14] = [
    ("ripple0", 1),
    ("ripple1", 2),
    ("ripple2", 4),
    ("glint", 8),
    ("sky", 16),
    ("refraction", 32),
    ("absorption", 64),
    ("caustics", 128),
    ("foam", 256),
    ("sparkle", 512),
    ("under", 1024),
    ("flat", 2048),
    ("ssr", 4096),
    ("clouds", 8192),
];

/// The mask for a `?waterdbg=` value: names (`DEBUG_SWITCHES`) separated by commas; `all` sets
/// every switch but `flat`. Unknown names are ignored.
pub fn debug_mask(names: &str) -> u32 {
    names
        .split(',')
        .map(str::trim)
        .map(|n| {
            if n == "all" {
                DEBUG_SWITCHES
                    .iter()
                    .filter(|(k, _)| *k != "flat")
                    .fold(0, |m, (_, b)| m | b)
            } else {
                DEBUG_SWITCHES
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map_or(0, |(_, b)| *b)
            }
        })
        .fold(0, |m, b| m | b)
}

/// The fit's output nears 1 as its input grows without bound: inverting stops short of it.
const INVERSE_FIT_MAX: f32 = 0.995;

fn wgsl_mat(m: &[[f32; 3]; 3]) -> String {
    let col = |c: &[f32; 3]| format!("vec3f({:?}, {:?}, {:?})", c[0], c[1], c[2]);
    format!("mat3x3f({}, {}, {})", col(&m[0]), col(&m[1]), col(&m[2]))
}

/// The WGSL constants the shaders take from here: the tone mapping's matrices, fit and inverses,
/// the inverse fit's limit, the wrap and the fades (the layer table is unrolled into
/// `water_slope`, shaders.rs).
pub fn wgsl() -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "const ACES_INPUT: mat3x3f = {};\nconst ACES_OUTPUT: mat3x3f = {};\n",
        wgsl_mat(&ACES_INPUT),
        wgsl_mat(&ACES_OUTPUT)
    ));
    s.push_str(&format!(
        "const ACES_INPUT_INV: mat3x3f = {};\nconst ACES_OUTPUT_INV: mat3x3f = {};\nconst INVERSE_FIT_MAX: f32 = {:?};\n",
        wgsl_mat(&mat_inverse(&ACES_INPUT)),
        wgsl_mat(&mat_inverse(&ACES_OUTPUT)),
        INVERSE_FIT_MAX
    ));
    let [a, b, c, d, e] = FIT;
    s.push_str(&format!(
        "const FIT_A: f32 = {a:?};\nconst FIT_B: f32 = {b:?};\nconst FIT_C: f32 = {c:?};\nconst FIT_D: f32 = {d:?};\nconst FIT_E: f32 = {e:?};\n"
    ));
    s.push_str(&format!(
        "const WRAP_M: f32 = {:?};\nconst FADE_LO: f32 = {:?};\nconst FADE_HI: f32 = {:?};\nconst MIN_ALPHA: f32 = {:?};\nconst WAVE_LAYERS: i32 = {};\n",
        WRAP_M as f32,
        FADE_LO,
        FADE_HI,
        MIN_ALPHA,
        LAYERS.len()
    ));
    s.push_str(&format!(
        "const NOISE_TILE: i32 = {NOISE_TILE};\nconst NOISE_TEXELS: f32 = {NOISE_TEXELS}.0;\nconst CAUSTIC_TILE: i32 = {CAUSTIC_TILE};\nconst CAUSTIC_TEXELS: f32 = {CAUSTIC_TEXELS}.0;\n"
    ));
    for (name, bit) in DEBUG_SWITCHES {
        s.push_str(&format!(
            "const DBG_{}: u32 = {bit}u;\n",
            name.to_uppercase()
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn neighbours_wrap_without_a_modulo_as_the_modulo_would() {
        use super::{wrap_cell, wrap_step};
        for period in [1, 2, 7, 731] {
            for c in [
                -3 * period - 1,
                -1,
                0,
                5,
                period - 1,
                period,
                4 * period + 3,
            ] {
                let w = wrap_cell(c, period);
                assert!((0..period).contains(&w));
                for o in -1..=1 {
                    assert_eq!(wrap_step(w, o, period), (c + o).rem_euclid(period));
                }
            }
        }
    }

    #[test]
    fn hashed_directions_are_unit_and_spread_round_the_circle() {
        let mut quadrants = [0; 4];
        for k in 0..4096u32 {
            let h = k.wrapping_mul(0x9e37_79b9) ^ (k >> 3);
            let [x, y] = super::hash_dir(h);
            assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-4);
            quadrants[usize::from(x < 0.0) + 2 * usize::from(y < 0.0)] += 1;
        }
        assert!(quadrants.iter().all(|&q| q > 800), "{quadrants:?}");
    }

    #[test]
    fn the_debug_switches_parse_by_name_and_are_distinct_bits() {
        use super::{DEBUG_SWITCHES, debug_mask};
        assert_eq!(debug_mask(""), 0);
        assert_eq!(debug_mask("caustics"), 128);
        assert_eq!(debug_mask("caustics, foam,nonsense"), 128 | 256);
        let all = DEBUG_SWITCHES.iter().fold(0, |m, (_, b)| {
            assert_eq!(m & b, 0, "bits overlap");
            assert!(b.is_power_of_two());
            m | b
        });
        assert_eq!(debug_mask("all"), all & !2048);
    }

    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn packs_each_fluid_at_its_id_with_its_own_parameters_a_second_fluid_by_data_alone() {
        // shared/blocks/fixtures/test.json adds `test:oil` and nothing else: it gets its own
        // parameters, at its own id, beside water's.
        use dwell_blocks::fixture::FLUIDS;
        let buf = pack_fluids(&FLUIDS);
        assert_eq!(buf.len(), FLUIDS.len() * FLUID_VEC4S * 4);
        let at = |name: &str| {
            let id = FLUIDS.iter().position(|f| f.name == name).expect(name);
            &buf[id * FLUID_VEC4S * 4..(id + 1) * FLUID_VEC4S * 4]
        };
        let oil = at("test:oil");
        let water = at("dwell:water");
        // absorption, opacity
        assert_eq!(&oil[0..4], &[2.0, 2.5, 3.0, 0.8]);
        assert_eq!(&water[0..4], &[1.0, 0.16, 0.045, 0.72]);
        // scatter (linear), roughness
        let s = linear_rgb(0x2a2410);
        assert_eq!(&oil[4..8], &[s[0], s[1], s[2], 0.02]);
        assert_eq!(water[7], 0.08);
        // ior, normal scale, wave scale, flow speed
        assert_eq!(&oil[8..12], &[1.47, 0.4, 4.0, 0.005]);
        assert_eq!(&water[8..12], &[1.333, 1.0, 2.0, 0.3]);
        // foam colour (linear), depth
        assert_eq!(oil[15], 0.0);
        assert_eq!(
            water[15],
            dwell_blocks::require_fluid("dwell:water").foam_depth
        );
        // F0 from the IOR, caustics, surface drop
        assert!(close(oil[16], f0_from_ior(1.47), 1e-7));
        assert_eq!(&oil[17..20], &[0.2, 0.25, 1.0]);
        assert_eq!(water[18], 0.125);
        // refraction, caustic scale, caustic depth, sparkle
        assert_eq!(&oil[20..24], &[0.05, 1.5, 0.5, 0.0]);
        let w = dwell_blocks::require_fluid("dwell:water");
        assert_eq!(
            &water[20..24],
            &[w.refraction, w.caustic_scale, w.caustic_depth, w.sparkle]
        );
        assert_eq!((water[17], water[19]), (w.caustics, w.scattering));
    }

    #[test]
    fn the_ray_in_the_water_runs_from_the_surface_to_the_scene_behind() {
        // Looking straight along the view axis: the depths' difference.
        assert!(close(ray_thickness(12.0, 10.0, 10.0), 2.0, 1e-6));
        // Off-axis, a point 20 m away at view depth 10: the ray is twice its depth difference.
        assert!(close(ray_thickness(12.0, 10.0, 20.0), 4.0, 1e-6));
        // Nothing behind (the sky, past the far plane): as deep as matters.
        assert_eq!(ray_thickness(0.0, 10.0, 10.0), DEEP_M);
        // Never negative where the scene is in front by a hair (MSAA edges).
        assert_eq!(ray_thickness(9.99, 10.0, 10.0), 0.0);
    }

    #[test]
    fn refraction_falls_back_where_the_offset_sample_is_in_front_of_the_water() {
        assert!(refracted_sample(15.0, 10.0), "the floor behind");
        assert!(refracted_sample(0.0, 10.0), "nothing drawn there");
        assert!(
            !refracted_sample(4.0, 10.0),
            "a bank in front: no foreground bleed"
        );
    }

    #[test]
    fn shallow_water_shows_the_floor_and_deep_water_its_body_graded_by_channel() {
        let floor = [0.8, 0.7, 0.4];
        let body = [0.0, 0.1, 0.4];
        let sigma = [0.5, 0.1, 0.03];
        assert_eq!(through_fluid(floor, body, sigma, 0.2, 0.0, 0.0), floor);
        let deep = through_fluid(floor, body, sigma, 0.2, 500.0, 50.0);
        for k in 0..3 {
            assert!(close(deep[k], body[k], 1e-3), "{deep:?}");
        }
        // Red goes first: a metre in, the floor has lost more red than blue.
        let m = through_fluid(floor, [0.0; 3], sigma, 0.0, 1.0, 0.0);
        assert!(m[0] / floor[0] < m[2] / floor[2]);
        // Light that came further down is dimmer.
        let lit = through_fluid(floor, body, sigma, 0.2, 1.0, 0.0);
        let dim = through_fluid(floor, body, sigma, 0.2, 1.0, 3.0);
        assert!((0..3).all(|k| dim[k] < lit[k]));
        // Scattering brings the body in sooner, without dimming the light on the way down.
        let murky = through_fluid(floor, body, sigma, 1.0, 1.0, 0.0);
        assert!(murky[2] > lit[2] && murky[0] < lit[0]);
    }

    #[test]
    fn the_builds_water_grades_from_clear_through_turquoise_to_blue_never_green() {
        // Tropical water over sand (the sand tile's colour, linear), seen straight down: a
        // hand's depth is nearly the sand; once the red has gone (a metre or so) it reads
        // turquoise to cyan, green no more than a little over blue; deep is a rich blue (blue
        // well above green, red faint). Never green.
        let w = dwell_blocks::require_fluid("dwell:water");
        let sand = linear_rgb(0xeed9a4);
        let body = linear_rgb(w.scatter).map(|c| c * 0.8);
        let at = |d: f32| through_fluid(sand, body, w.absorption, w.scattering, d, d);
        let hand = at(0.1);
        assert!(
            (0..3).all(|k| close(hand[k], sand[k], 0.25 * sand[k])),
            "{hand:?}"
        );
        for d in [1.0_f32, 2.0, 4.0, 8.0, 16.0, 40.0] {
            let c = at(d);
            assert!(c[1] <= c[2] * 1.1, "green at {d} m: {c:?}");
            assert!(c[0] < 0.6 * c[1], "red gone at {d} m: {c:?}");
        }
        let two = at(2.0);
        assert!(two[1] > 0.6 * two[2], "turquoise to cyan at 2 m: {two:?}");
        let deep = at(40.0);
        assert!(
            deep[2] > 2.0 * deep[1] && deep[0] < 0.05 * deep[2],
            "blue when deep: {deep:?}"
        );
    }

    #[test]
    fn caustics_are_whole_at_the_surface_and_fade_with_depth_and_wrap() {
        assert_eq!(caustic_fade(0.0, 8.0), 0.0, "nothing above the surface");
        assert!(close(caustic_fade(1e-6, 8.0), 1.0, 1e-4));
        assert!(close(caustic_fade(8.0, 8.0), (-1.0f32).exp(), 1e-6));
        assert!(caustic_fade(30.0, 8.0) < 0.03);
        for scale in [1.5, 2.5, 3.3] {
            let (n, cell) = caustic_period(scale);
            assert_eq!(n % CAUSTIC_TILE, 0, "whole caustic tiles");
            assert!(close(n as f32 * cell, WRAP_M as f32, 1e-3));
            assert!(close(
                cell,
                scale,
                0.6 * scale * scale * CAUSTIC_TILE as f32 / WRAP_M as f32
            ));
        }
    }

    #[test]
    fn the_builds_fluids_fill_the_buffer_and_an_empty_table_still_binds() {
        assert_eq!(
            pack_fluids(&dwell_blocks::FLUIDS).len(),
            dwell_blocks::FLUIDS.len() * FLUID_VEC4S * 4
        );
        assert_eq!(pack_fluids(&[]).len(), FLUID_VEC4S * 4);
    }

    #[test]
    fn fresnel_is_f0_head_on_and_total_at_grazing() {
        let f0 = f0_from_ior(1.333);
        assert!(close(f0, 0.0204, 1e-4), "{f0}");
        assert!(close(schlick(1.0, f0), f0, 1e-7));
        assert!(close(schlick(0.0, f0), 1.0, 1e-7));
        // Monotonic between, and about 6 % at 60° from the normal for water.
        let mut last = f0;
        for i in (0..=100).rev() {
            let r = schlick(i as f32 / 100.0, f0);
            assert!(r >= last - 1e-7);
            last = r;
        }
        assert!(close(schlick(0.5, f0), 0.0510, 1e-3));
    }

    #[test]
    fn the_normal_is_up_for_a_flat_surface_and_leans_against_the_slope_on_either_face() {
        assert_eq!(blend_normal(1.0, [0.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(blend_normal(-1.0, [0.0, 0.0]), [-0.0, -1.0, -0.0]);
        // Rising toward +x: the normal leans back toward −x; unit length.
        let n = blend_normal(1.0, [0.2, 0.0]);
        assert!(n[0] < 0.0 && n[1] > 0.9 && n[2] == 0.0);
        assert!(close(n.iter().map(|c| c * c).sum::<f32>(), 1.0, 1e-6));
        // Face B (BIFACIAL_WORLD.md §6): the same surface mirrored, its normal turned over.
        let b = blend_normal(-1.0, [0.2, 0.0]);
        assert_eq!(b, n.map(|c| -c));
    }

    #[test]
    fn layers_fade_out_before_a_pixel_covers_half_their_wavelength_and_widen_the_glint() {
        assert_eq!(layer_fade(0.01, 1.0), 1.0);
        assert_eq!(layer_fade(0.125, 1.0), 1.0);
        assert_eq!(layer_fade(0.5, 1.0), 0.0);
        assert_eq!(layer_fade(5.0, 1.0), 0.0);
        let mid = layer_fade(0.3, 1.0);
        assert!(mid > 0.0 && mid < 1.0);
        // Nothing lost: the fluid's own roughness; the lost slope widens it.
        assert!(close(glint_alpha(0.3, 0.0), 0.09, 1e-6));
        assert!(glint_alpha(0.3, 0.01) > 0.09);
        assert_eq!(glint_alpha(0.0, 0.0), MIN_ALPHA);
    }

    #[test]
    fn each_layer_wraps_exactly_at_the_wrap_and_the_lattice_turns_are_integer() {
        for scale in [0.5, 2.0, 3.3, 4.0] {
            for l in &LAYERS {
                let (n, wavelength) = layer_period(l, scale);
                assert!(n >= 1);
                assert_eq!(n % NOISE_TILE, 0, "whole noise tiles");
                assert!(close(n as f32 * wavelength, WRAP_M as f32, 1e-3));
                // Within half a tile's rounding over the wrap.
                let want = scale * l.wavelength;
                assert!(close(
                    wavelength,
                    want,
                    0.6 * want * want * NOISE_TILE as f32 / WRAP_M as f32 + 1e-3
                ));
            }
        }
        for l in &LAYERS {
            assert!(close(l.dir[0].hypot(l.dir[1]), 1.0, 1e-3));
        }
        assert_eq!(wrapped_origin([-1.0, 5.0, 2048.0 + 3.0]), [1023.0, 3.0]);
    }

    #[test]
    fn the_inverse_tone_mapping_gives_back_the_scene_colour() {
        for exposure in [0.6, 1.0, 2.0] {
            for c in [
                [0.02, 0.05, 0.1],
                [0.3, 0.5, 0.9],
                [0.8, 0.7, 0.6],
                [0.01, 0.01, 0.01],
            ] {
                let shown = aces(c, exposure);
                let back = inverse_aces(shown, exposure);
                for k in 0..3 {
                    assert!(
                        close(back[k], c[k], 2e-3 + 2e-3 * c[k]),
                        "{c:?} → {shown:?} → {back:?}"
                    );
                }
            }
        }
        // And the sky's colours (display) survive the round trip the other way.
        let sky = [0.55, 0.7, 0.9];
        let again = aces(inverse_aces(sky, 1.0), 1.0);
        for k in 0..3 {
            assert!(close(again[k], sky[k], 1e-3), "{again:?}");
        }
    }

    #[test]
    fn quality_levels_round_trip_and_low_draws_one_layer() {
        for q in [WaterQuality::Low, WaterQuality::Medium, WaterQuality::High] {
            assert_eq!(WaterQuality::from_level(q.level() as u32), q);
        }
        assert_eq!(WaterQuality::from_level(9), WaterQuality::High);
        assert_eq!(WaterQuality::Low.layers(), 1);
        assert_eq!(WaterQuality::High.layers(), 3);
        assert_eq!(
            WaterQuality::default(),
            WaterQuality::Medium,
            "the desktop default"
        );
        // W3: Medium and High read the scene behind; Low keeps W1b's look, with no copies.
        assert!(!WaterQuality::Low.reads_scene());
        assert!(WaterQuality::Medium.reads_scene() && WaterQuality::High.reads_scene());
    }

    #[test]
    fn the_ripples_visibly_move() {
        // The owner (2026-10-26): the water's shading moved so slowly it hardly seemed to. Each
        // layer crosses its own wavelength in a few seconds (a breeze's ripples), the largest in
        // under 10 s.
        let water = dwell_blocks::require_fluid("dwell:water");
        for l in LAYERS {
            let wavelength = l.wavelength * water.wave_scale;
            let seconds = wavelength / (water.flow_speed * l.speed);
            assert!(seconds < 10.0, "{l:?}: {seconds} s a wavelength");
            assert!(seconds > 0.25, "{l:?}: {seconds} s, no blur of motion");
        }
    }
}
