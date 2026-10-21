//! Dynamic lights in the renderer (docs/LIGHTS.md §3, ADR 0047): the nearest light sources this
//! frame — held torches, yours and other players' (`characters::HeldLight`) — picked and packed by
//! `dwell_look::lights` into one small uniform block (frame binding 14) that every lit surface reads
//! through `point_lights`: terrain, plants, the LOD, water, see-through faces and the characters.
//! Unshadowed (the Torch shadows setting adds shadows in T2) and never in the flood light.
//!
//! Cost: a fixed array of at most `MAX_SHADER_LIGHTS`, written once a frame (`LIGHT_FLOATS` floats);
//! each fragment loops over the lights drawn (none most of the time), a few ALU ops each.

use dwell_look::lights::{LIGHT_FLOATS, LIGHTS, MAX_SHADER_LIGHTS, PointLight, pack, pick};

use super::characters::HeldLight;

/// The frame group's binding of the lights' block.
pub const BINDING: u32 = 14;

/// The lights' block and `point_lights`: the irradiance (linear) the drawn lights give a surface at
/// `rel` (relative to the eye) with unit normal `n`: the falloff on the flood light's curve, the
/// wrapped cosine (`shared/look/lights.json`). Mirrors `dwell_look::lights::irradiance`.
pub fn wgsl() -> String {
    format!(
        r"
const MAX_POINT_LIGHTS: u32 = {MAX_SHADER_LIGHTS}u;
const POINT_CURVE: f32 = {:?};
const POINT_WRAP: f32 = {:?};
const GLOW_PEAK: f32 = {:?};

// What a self-lit surface of linear colour `c` (colour × emissive) gives off (docs/LIGHTS.md §2;
// owner: the flame should stay illuminated regardless of brightness since it is the source of the
// light): the canvas shows it as the tone curve shows `c` at the reference exposure, its
// brightest channel GLOW_PEAK at most, whatever the frame's exposure; its pixels are flagged
// self-lit (`self_lit_depth`), so the time of day's desaturation leaves them alone. So a flame has
// one hue and brightness by day, at night and in a cave. `glow` below mirrors it.
fn glow(c: vec3f) -> vec3f {{
    let peak = max(max(c.r, c.g), c.b);
    let v = c * select(1.0, GLOW_PEAK / peak, peak > GLOW_PEAK);
    return inverse_display(srgb_encode(clamp(ACES_OUTPUT * rrt_odt_fit(ACES_INPUT * v), vec3f(0.0), vec3f(1.0))));
}}

// A surface lit to `lit` with emissive `shine`: its lit colour replaced by up to all of its glow
// (`shine` 1 or more: fully self-lit, unaffected by the sun, ambient, flood light, corner occlusion
// and shadows); `emitted` below mirrors it.
fn emitted(lit: vec3f, albedo: vec3f, shine: f32) -> vec3f {{
    return lit * (1.0 - min(shine, 1.0)) + glow(albedo * shine);
}}
struct PointLights {{
    // x the count
    head: vec4f,
    // per light: xyz relative to the eye, w the radius (m); rgb colour × strength × flicker
    l: array<vec4f, {}>,
}};
@group(0) @binding({BINDING}) var<uniform> point_light_block: PointLights;

fn point_lights(rel: vec3f, n: vec3f) -> vec3f {{
    var sum = vec3f(0.0);
    let count = min(u32(point_light_block.head.x), MAX_POINT_LIGHTS);
    for (var i = 0u; i < count; i = i + 1u) {{
        let a = point_light_block.l[i * 2u];
        let b = point_light_block.l[i * 2u + 1u];
        let to = a.xyz - rel;
        let d = length(to);
        if (d >= a.w) {{
            continue;
        }}
        let x = 1.0 - d / a.w;
        let fall = x / (POINT_CURVE - (POINT_CURVE - 1.0) * x);
        let cos = select(1.0, max((dot(to, n) / d + POINT_WRAP) / (1.0 + POINT_WRAP), 0.0), d > 1e-4);
        sum = sum + b.rgb * (cos * fall);
    }}
    return sum;
}}
",
        LIGHTS.curve as f32,
        LIGHTS.wrap as f32,
        LIGHTS.glow_peak as f32,
        MAX_SHADER_LIGHTS * 2
    )
}

fn srgb_encode(x: f32) -> f32 {
    if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(x: f32) -> f32 {
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x.max(0.0) + 0.055) / 1.055).powf(2.4)
    }
}

/// The CPU mirror of the shader's `glow`: the scene colour (linear) a self-lit surface of linear
/// colour `c` writes at `exposure` (the frame's), so the canvas shows it as the tone curve shows
/// `c` at the reference exposure.
pub fn glow(c: [f32; 3], exposure: f32) -> [f32; 3] {
    let peak = c[0].max(c[1]).max(c[2]);
    let k = if peak > LIGHTS.glow_peak as f32 {
        LIGHTS.glow_peak as f32 / peak
    } else {
        1.0
    };
    let shown = super::water::aces(c.map(|v| v * k), 0.6).map(srgb_encode);
    super::water::inverse_aces(shown.map(srgb_decode), exposure)
}

/// The CPU mirror of the shader's `emitted`: a surface lit to `lit` (linear) with albedo
/// `albedo` and emissive `shine`.
pub fn emitted(lit: [f32; 3], albedo: [f32; 3], shine: f32, exposure: f32) -> [f32; 3] {
    let g = glow(albedo.map(|v| v * shine), exposure);
    std::array::from_fn(|k| lit[k] * (1.0 - shine.min(1.0)) + g[k])
}

/// What the canvas shows for a scene colour (linear) at `exposure` and `saturation`: the tone
/// curve, the encode and the time of day's saturation, which a self-lit pixel skips
/// (`scene_display`), 0–1 sRGB.
pub fn displayed(c: [f32; 3], exposure: f32, saturation: f32, self_lit: bool) -> [f32; 3] {
    let lit = super::water::aces(c, exposure).map(srgb_encode);
    let l = 0.2126 * lit[0] + 0.7152 * lit[1] + 0.0722 * lit[2];
    let s = if self_lit { 1.0 } else { saturation };
    lit.map(|v| l + (v - l) * s)
}

/// The block's layout entry (fragments read it).
pub fn layout_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: BINDING,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The block's buffer.
pub fn buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point lights"),
        size: (LIGHT_FLOATS * 4) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// This frame's lights: the held ones (relative to `eye`) as world lights, the nearest
/// `LIGHTS.max_lights` within `LIGHTS.cull_m` of the eye, packed at time `t` (s).
pub fn frame_block(held: &[HeldLight], eye: [f64; 3], t: f64) -> ([f32; LIGHT_FLOATS], usize) {
    let all: Vec<PointLight> = held
        .iter()
        .map(|h| {
            let pos = std::array::from_fn(|k| eye[k] + f64::from(h.rel[k]));
            PointLight::of(h.source, pos, h.seed)
        })
        .collect();
    let picked = pick(&all, eye, LIGHTS.max_lights, LIGHTS.cull_m);
    (pack(&picked, eye, t), picked.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torch() -> &'static dwell_blocks::LightInfo {
        dwell_blocks::light("dwell:torch").expect("the torch light")
    }

    #[test]
    fn held_lights_are_packed_relative_to_the_eye_nearest_first_within_the_budget() {
        let eye = [100.0, 40.0, -20.0];
        let held: Vec<HeldLight> = (0..20)
            .map(|k| HeldLight {
                rel: [f32::from(k as u8) * 2.0 + 0.5, 0.0, 0.0],
                source: torch(),
                seed: k,
            })
            .collect();
        let (u, n) = frame_block(&held, eye, 1.0);
        assert_eq!(n, LIGHTS.max_lights);
        assert_eq!(u[0] as usize, n);
        assert!((u[4] - 0.5).abs() < 1e-5, "the nearest first, eye-relative");
        assert_eq!(u[7], torch().radius_m);
        let (none, n0) = frame_block(&[], eye, 1.0);
        assert_eq!((none[0], n0), (0.0, 0));
    }

    #[test]
    fn the_wgsl_mirrors_the_look_crates_falloff() {
        let src = wgsl();
        // The same curve and wrap as `dwell_look::lights::falloff` and `wrapped`.
        assert!(src.contains("let fall = x / (POINT_CURVE - (POINT_CURVE - 1.0) * x);"));
        assert!(src.contains("(dot(to, n) / d + POINT_WRAP) / (1.0 + POINT_WRAP)"));
        assert!(src.contains(&format!("@binding({BINDING})")));
    }
}
