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
/// `rel` (relative to the eye) with unit normal `n`, softened within `near_min` metres (0: the
/// lights' own; characters soften more, so a held torch doesn't burn out the hand holding it).
/// Mirrors `dwell_look::lights::irradiance`.
pub fn wgsl() -> String {
    format!(
        r"
const MAX_POINT_LIGHTS: u32 = {MAX_SHADER_LIGHTS}u;
struct PointLights {{
    // x the count
    head: vec4f,
    // per light: xyz relative to the eye, w the radius (m); rgb colour × strength × flicker, w the
    // softening distance (m)
    l: array<vec4f, {}>,
}};
@group(0) @binding({BINDING}) var<uniform> point_light_block: PointLights;

fn point_lights(rel: vec3f, n: vec3f, near_min: f32) -> vec3f {{
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
        let x = d / a.w;
        let x4 = x * x * x * x;
        let window = (1.0 - x4) * (1.0 - x4);
        let near = max(b.w, near_min);
        let cos = select(1.0, max(dot(to, n) / d, 0.0), d > 1e-4);
        sum = sum + b.rgb * (cos * window * (1.0 + near * near) / (d * d + near * near));
    }}
    return sum;
}}
",
        MAX_SHADER_LIGHTS * 2
    )
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
    (pack(&picked, eye, t, LIGHTS.near_m), picked.len())
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
        // The same window and softening as `dwell_look::lights::falloff`.
        assert!(src.contains("let window = (1.0 - x4) * (1.0 - x4);"));
        assert!(src.contains("(1.0 + near * near) / (d * d + near * near)"));
        assert!(src.contains(&format!("@binding({BINDING})")));
    }
}
