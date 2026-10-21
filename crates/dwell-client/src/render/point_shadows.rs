//! Torch shadows (docs/LIGHTS.md §4, Phase 47 T2): shadow maps for the nearest dynamic lights that
//! may cast them (held torches and, with the setting on, placed ones), a cube of six perspective
//! depth maps per light around it, as many lights and as large as the Torch shadows tier says
//! (`shared/look/lights.json` `shadows`). The maps live in one depth array, six layers a slot; the
//! scene's fragments find the face a point falls in and compare against it (`point_shadow`,
//! called from `point_lights`). A still light's maps are kept until the terrain near it changes,
//! and at most `redrawsPerFrame` lights are drawn again a frame (`dwell_look::lights::
//! TorchShadowCache`), so a room of placed torches costs nothing frame to frame.
//!
//! The face matrices are the same ones the maps are drawn with and read through (eye-relative,
//! made again from each map's world position every frame), so there is no cube-map orientation to
//! agree on. A point light has no up: the faces follow the world's axes on both faces of the world.

use dwell_look::lights::{LIGHTS, MAX_SHADOWED, TorchShadowTier};

use super::camera::{Basis, Mat4, V3, cross, mul, normalize, perspective, view_matrix};
use super::shadows::ShadowMaps;

/// The frame group's bindings: the uniform block and the maps (the compare sampler is the
/// cascades', binding 10).
pub const UNIFORM_BINDING: u32 = 15;
pub const MAP_BINDING: u32 = 16;

/// Faces a light's maps have: +X, −X, +Y, −Y, +Z, −Z (the order `point_shadow` picks them in).
pub const FACES: usize = 6;

/// Floats in the uniform block: a header vec4, then per slot a vec4 (the light, relative to the
/// eye, and the maps' far plane: its radius) and six matrices.
pub const UNIFORM_FLOATS: usize = 4 + MAX_SHADOWED * (4 + FACES * 16);

/// The direction each face looks along.
const DIRS: [V3; FACES] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// A face's camera axes (looking along `DIRS[face]`, a right-handed basis as `camera::aim`'s).
pub fn face_basis(face: usize) -> Basis {
    let forward = DIRS[face];
    let up = if forward[1] == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let back = forward.map(|v| -v);
    let right = normalize(cross(up, back));
    Basis {
        right,
        up: cross(back, right),
        back,
    }
}

/// The face picked for a direction from the light: its largest axis, its sign.
pub fn face_of(d: V3) -> usize {
    let a = d.map(f64::abs);
    let axis = if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    };
    axis * 2 + usize::from(d[axis] < 0.0)
}

/// A face's view-projection for a light at `light` (relative to the eye) reaching `far` metres:
/// eye-relative points to the face's clip space.
pub fn face_view_proj(light: V3, face: usize, far: f64) -> Mat4 {
    let t = &LIGHTS.shadows;
    let fov = 2.0 * t.face_margin.atan().to_degrees();
    let proj = perspective(fov, 1.0, t.near_m, far.max(t.near_m * 2.0));
    let l = light.map(|v| -v as f32);
    let translate: Mat4 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [l[0], l[1], l[2], 1.0],
    ];
    mul(&proj, &mul(&view_matrix(&face_basis(face)), &translate))
}

/// A map slot this frame: its light (relative to the eye) and its reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotMaps {
    pub light: V3,
    pub far: f64,
}

/// The uniform block (`PointShadows` in the shader): the texel's share of a face, the bias and
/// normal offset (m), the near plane; per slot its light and far plane and its faces' matrices
/// (a slot holding no map: far 0).
pub fn uniforms(resolution: u32, slots: &[Option<SlotMaps>]) -> [f32; UNIFORM_FLOATS] {
    let t = &LIGHTS.shadows;
    let mut u = [0.0f32; UNIFORM_FLOATS];
    u[..4].copy_from_slice(&[
        1.0 / resolution.max(1) as f32,
        t.bias_m as f32,
        t.normal_offset_m as f32,
        t.near_m as f32,
    ]);
    for (i, s) in slots.iter().take(MAX_SHADOWED).enumerate() {
        let Some(s) = s else { continue };
        let o = 4 + i * (4 + FACES * 16);
        u[o..o + 4].copy_from_slice(&[
            s.light[0] as f32,
            s.light[1] as f32,
            s.light[2] as f32,
            s.far as f32,
        ]);
        for f in 0..FACES {
            let m = face_view_proj(s.light, f, s.far);
            for (k, col) in m.iter().enumerate() {
                let at = o + 4 + f * 16 + k * 4;
                u[at..at + 4].copy_from_slice(col);
            }
        }
    }
    u
}

/// The CPU mirror of `point_shadow`'s depth: the map depth a point `major` metres along its face's
/// axis from the light has (WebGPU's [0, 1] perspective depth).
pub fn map_depth(major: f64, near: f64, far: f64) -> f64 {
    far / (far - near) - near * far / ((far - near) * major.max(near))
}

/// The shader: the maps, the block and `point_shadow(slot, rel, n)`, the share of a slot's light
/// reaching a surface at `rel` (relative to the eye) with normal `n` (1 lit, 0 in shadow), a 2 × 2
/// grid of hardware 2 × 2 compares.
pub fn wgsl() -> String {
    format!(
        r"
const POINT_FACES: u32 = {FACES}u;
struct PointSlot {{
    // xyz the light relative to the eye, w the maps' far plane (0: no map)
    light: vec4f,
    faces: array<mat4x4f, {FACES}>,
}};
struct PointShadows {{
    // x a texel's share of a face, y the bias (m), z the normal offset (m), w the near plane (m)
    cfg: vec4f,
    slots: array<PointSlot, {MAX_SHADOWED}>,
}};
@group(0) @binding({UNIFORM_BINDING}) var<uniform> point_shadows: PointShadows;
@group(0) @binding({MAP_BINDING}) var point_shadow_maps: texture_depth_2d_array;

fn point_shadow(slot: u32, rel: vec3f, n: vec3f) -> f32 {{
    // Read field by field: a copy of the slot (six matrices) a fragment costs.
    let light = point_shadows.slots[slot].light;
    let far = light.w;
    if (far <= 0.0) {{
        return 1.0;
    }}
    let cfg = point_shadows.cfg;
    let p = rel + n * cfg.z;
    let d = p - light.xyz;
    let a = abs(d);
    var axis = 0u;
    var major = a.x;
    if (a.y > major) {{
        axis = 1u;
        major = a.y;
    }}
    if (a.z > major) {{
        axis = 2u;
        major = a.z;
    }}
    let face = axis * 2u + select(0u, 1u, d[axis] < 0.0);
    let clip = point_shadows.slots[slot].faces[face] * vec4f(p, 1.0);
    let uv = clip.xy / clip.w * vec2f(0.5, -0.5) + 0.5;
    let near = cfg.w;
    let dm = max(major - cfg.y, near);
    let depth = far / (far - near) - near * far / ((far - near) * dm);
    let layer = i32(slot * POINT_FACES + face);
    let t = cfg.x * 0.75;
    var lit = 0.0;
    for (var k = 0; k < 4; k++) {{
        let o = vec2f(select(-t, t, (k & 1) != 0), select(-t, t, (k & 2) != 0));
        lit += textureSampleCompareLevel(point_shadow_maps, shadow_sampler, clamp(uv + o, vec2f(0.0), vec2f(1.0)), layer, depth);
    }}
    return lit * 0.25;
}}
"
    )
}

/// The frame group's layout entries (bindings 15 and 16).
pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: UNIFORM_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(UNIFORM_FLOATS as u64 * 4),
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: MAP_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
    ]
}

/// The maps on the GPU for a tier (none: a 1 × 1 stand-in of one slot), with a frame group a layer
/// for the passes that draw them (the cascades' kind: `ShadowMaps`), and the uniform block.
pub struct PointShadowMaps {
    pub tier: Option<TorchShadowTier>,
    pub maps: ShadowMaps,
    pub uniform: wgpu::Buffer,
}

impl PointShadowMaps {
    pub fn new(
        device: &wgpu::Device,
        tier: Option<TorchShadowTier>,
        pass_layout: &wgpu::BindGroupLayout,
        atlas: (&wgpu::TextureView, &wgpu::Sampler),
        frame_bytes: u64,
    ) -> Self {
        let (res, slots) = tier.map_or((1, 1), |t| (t.resolution, t.lights.min(MAX_SHADOWED)));
        let maps = ShadowMaps::new(
            device,
            res,
            (slots * FACES) as u32,
            pass_layout,
            atlas,
            frame_bytes,
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("torch shadows"),
            size: UNIFORM_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            tier,
            maps,
            uniform,
        }
    }

    /// The slots the maps hold.
    pub fn slots(&self) -> usize {
        self.tier.map_or(0, |t| t.lights.min(MAX_SHADOWED))
    }

    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 2] {
        [
            wgpu::BindGroupEntry {
                binding: UNIFORM_BINDING,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: MAP_BINDING,
                resource: wgpu::BindingResource::TextureView(&self.maps.array_view),
            },
        ]
    }
}

/// The torch shadows this adapter draws: the setting's, none on a CPU adapter (every pixel there
/// is the CPU's time; six passes a light would be most of a frame).
pub fn torch_shadows_for(
    device_type: wgpu::DeviceType,
    quality: dwell_look::shadows::ShadowQuality,
) -> dwell_look::shadows::ShadowQuality {
    if device_type == wgpu::DeviceType::Cpu {
        dwell_look::shadows::ShadowQuality::Off
    } else {
        quality
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::shadows::ShadowQuality;

    fn apply(m: &Mat4, p: V3) -> [f64; 4] {
        std::array::from_fn(|r| {
            (0..4)
                .map(|k| f64::from(m[k][r]) * if k < 3 { p[k] } else { 1.0 })
                .sum()
        })
    }

    // both-faces: a point light has no up; its cube's faces are the world's axes on either face
    // of the world (`face_basis` is a cube face's, not a world face's).
    #[test]
    fn a_point_falls_in_its_faces_view_at_the_depth_the_shader_compares() {
        // For points all round a light, the face `face_of` picks sees the point inside its view
        // (|ndc| ≤ 1/margin... within the margin) and the shader's depth formula equals the
        // matrix's depth: the maps are drawn and read through the same matrices.
        let light = [3.0, -2.0, 5.0];
        let far = 12.0;
        let near = LIGHTS.shadows.near_m;
        for k in 0..200 {
            let a = f64::from(k) * 2.399_963;
            let z = 1.0 - 2.0 * (f64::from(k) + 0.5) / 200.0;
            let r = (1.0 - z * z).sqrt();
            let dir = [r * a.cos(), z, r * a.sin()];
            let dist = 1.0 + f64::from(k % 9);
            let p: V3 = std::array::from_fn(|i| light[i] + dir[i] * dist);
            let face = face_of(dir);
            let clip = apply(&face_view_proj(light, face, far), p);
            assert!(clip[3] > 0.0, "in front of face {face}");
            let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
            assert!(
                ndc[0].abs() <= 1.0 + 1e-6 && ndc[1].abs() <= 1.0 + 1e-6,
                "{k}: {ndc:?}"
            );
            let major = dir.map(f64::abs).into_iter().fold(0.0, f64::max) * dist;
            let want = map_depth(major, near, far);
            assert!((ndc[2] - want).abs() < 1e-4, "{k}: {} vs {want}", ndc[2]);
        }
        // Each face looks along its own axis.
        for (f, dir) in DIRS.iter().enumerate() {
            assert_eq!(face_of(*dir), f);
            let b = face_basis(f);
            assert!((super::super::camera::dot(b.forward(), *dir) - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn the_uniforms_hold_each_slots_light_and_faces_and_none_where_no_map() {
        let u = uniforms(
            256,
            &[
                None,
                Some(SlotMaps {
                    light: [1.0, 2.0, 3.0],
                    far: 14.0,
                }),
            ],
        );
        assert!((u[0] - 1.0 / 256.0).abs() < 1e-9);
        let stride = 4 + FACES * 16;
        assert_eq!(u[4 + 3], 0.0, "no map in slot 0");
        assert_eq!(&u[4 + stride..4 + stride + 4], &[1.0, 2.0, 3.0, 14.0]);
        let m = face_view_proj([1.0, 2.0, 3.0], 0, 14.0);
        assert_eq!(&u[4 + stride + 4..4 + stride + 8], &m[0]);
    }

    #[test]
    fn a_cpu_adapter_draws_no_torch_shadows() {
        assert_eq!(
            torch_shadows_for(wgpu::DeviceType::Cpu, ShadowQuality::High),
            ShadowQuality::Off
        );
        assert_eq!(
            torch_shadows_for(wgpu::DeviceType::DiscreteGpu, ShadowQuality::Low),
            ShadowQuality::Low
        );
    }

    #[test]
    fn the_wgsl_mirrors_the_depth_and_reads_the_cascades_compare_sampler() {
        let src = wgsl();
        assert!(src.contains("let depth = far / (far - near) - near * far / ((far - near) * dm);"));
        assert!(src.contains("textureSampleCompareLevel(point_shadow_maps, shadow_sampler"));
        assert!(src.contains(&format!("@binding({UNIFORM_BINDING})")));
    }
}
