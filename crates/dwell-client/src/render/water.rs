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
pub const FLUID_VEC4S: usize = 5;
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
    Medium,
    #[default]
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

/// A layer's wavelength for a fluid's `wave_scale`, adjusted so a whole number of them spans
/// `WRAP_M` (the noise's period in lattice cells, and the wavelength).
pub fn layer_period(layer: &WaveLayer, wave_scale: f32) -> (i32, f32) {
    let want = (wave_scale * layer.wavelength).max(1e-3);
    let n = ((WRAP_M as f32) / want).round().max(1.0);
    (n as i32, WRAP_M as f32 / n)
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
        out.extend_from_slice(&[f0_from_ior(f.ior), f.caustics, f.surface_drop as f32, 0.0]);
    }
    if out.is_empty() {
        // A storage binding can't be empty: one zero fluid that nothing indexes.
        out.resize(FLUID_VEC4S * 4, 0.0);
    }
    out
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
    s
}

#[cfg(test)]
mod tests {
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
        assert_eq!(&water[0..4], &[0.45, 0.09, 0.06, 0.65]);
        // scatter (linear), roughness
        let s = linear_rgb(0x2a2410);
        assert_eq!(&oil[4..8], &[s[0], s[1], s[2], 0.02]);
        assert_eq!(water[7], 0.08);
        // ior, normal scale, wave scale, flow speed
        assert_eq!(&oil[8..12], &[1.47, 0.4, 4.0, 0.005]);
        assert_eq!(&water[8..12], &[1.333, 1.0, 2.0, 0.03]);
        // foam colour (linear), depth
        assert_eq!(oil[15], 0.0);
        assert_eq!(water[15], 0.4);
        // F0 from the IOR, caustics, surface drop
        assert!(close(oil[16], f0_from_ior(1.47), 1e-7));
        assert_eq!(&oil[17..20], &[0.2, 0.25, 0.0]);
        assert_eq!(&water[17..19], &[1.0, 0.125]);
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
                assert!(close(n as f32 * wavelength, WRAP_M as f32, 1e-3));
                assert!(close(
                    wavelength,
                    scale * l.wavelength,
                    0.01 * scale * l.wavelength + 1e-3
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
        assert_eq!(WaterQuality::default(), WaterQuality::High);
    }
}
