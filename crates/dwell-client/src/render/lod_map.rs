//! The LOD colour map on the GPU (a preview behind `?lodmap=on`; ARCHITECTURE.md §6.6 "Colour
//! map"; the rings' placing and raster are `crate::lod_map`): one rgba8 sRGB texture array, a
//! layer per ring, written strip by strip at the toroidal slots of the texels the rings hold, and
//! a uniform block with each ring's held rectangle about the eye. LOD tops (`flat_fs`, the draws
//! `LOD_DRAW` marks) take their colour from the finest ring holding them (`lod_map_albedo`).
//! Until the first strip arrives the texture is a 1×1 stand-in.

use crate::lod_map::{LodMap, Rect, ring_uniforms, toroidal};
use dwell_look::lod_map::{LOD_MAP, LodMapTunables};
use dwell_world::FaceFrame;

/// The frame group's bindings: the uniform block and the texture array (sampled with the water
/// tiles' repeating linear sampler, binding 7: the rings wrap).
pub const UNIFORM_BINDING: u32 = 17;
pub const TEXTURE_BINDING: u32 = 18;

/// The most rings the block holds (shared/look/lodmap.mjs `MAX_RINGS`).
pub const MAX_RINGS: usize = 4;
/// vec4s in the block: the switches, then two per ring.
pub const UNIFORM_VEC4S: usize = 1 + 2 * MAX_RINGS;

/// `rotation.z` of a draw whose top faces the colour map may paint (LOD sections and stand-ins).
pub const LOD_DRAW: f32 = 1.0;

pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: UNIFORM_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(UNIFORM_VEC4S as u64 * 16),
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: TEXTURE_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
    ]
}

/// The uniform block: x on (1) or off, y the rings' face (its sign), z and w where the LOD tops
/// start and finish taking the map's colour (m from the eye); then per ring `ring_uniforms`, and
/// rings it lacks empty.
pub fn uniforms(map: Option<&LodMap>, eye: [f64; 3]) -> Vec<f32> {
    let mut u = vec![0.0f32; UNIFORM_VEC4S * 4];
    for k in 0..MAX_RINGS {
        u[4 + 8 * k..8 + 8 * k].copy_from_slice(&[1.0, 1.0, -1.0, -1.0]);
    }
    let Some(map) = map else {
        return u;
    };
    let t = map.tunables;
    let face = map.face.unwrap_or(FaceFrame::A);
    u[..4].copy_from_slice(&[
        1.0,
        face.sign_f32(),
        t.blend_from_m as f32,
        t.blend_to_m as f32,
    ]);
    for (k, v) in ring_uniforms(map, eye).iter().take(2 * MAX_RINGS).enumerate() {
        u[4 + 4 * k..8 + 4 * k].copy_from_slice(v);
    }
    u
}

/// The shader's half (joined to the scene shaders): `lod_map_albedo`.
pub fn wgsl(t: &LodMapTunables) -> String {
    let top = dwell_look::face_tint(1, 1);
    format!(
        "struct LodMapRing {{ rect: vec4f, at: vec4f, }};
struct LodMapBlock {{ cfg: vec4f, rings: array<LodMapRing, {MAX_RINGS}>, }};
@group(0) @binding({UNIFORM_BINDING}) var<uniform> lod_map: LodMapBlock;
@group(0) @binding({TEXTURE_BINDING}) var lod_map_tex: texture_2d_array<f32>;
const LOD_MAP_RINGS: i32 = {rings};
const LOD_MAP_FADE: f32 = {fade:?};
const LOD_MAP_TOP: vec3f = vec3f({t0:?}, {t1:?}, {t2:?});

// An LOD top's albedo (ARCHITECTURE.md §6.6 'Colour map'): the colour map's, from the finest ring
// holding the point `rel` (from the eye) and handed to the next coarser over the ring's edge, the
// top face's shading as the LOD mesher gives it; blended in from the vertex colour `vertex` over
// the map's distance band. The vertex colour where the map is off, holds nothing there, is of the
// other face or beyond the disc (alpha 0).
fn lod_map_albedo(rel: vec3f, face: f32, vertex: vec3f) -> vec3f {{
    if (lod_map.cfg.x < 0.5 || face != lod_map.cfg.y) {{
        return vertex;
    }}
    let near = smoothstep(lod_map.cfg.z, lod_map.cfg.w, length(rel));
    if (near <= 0.0) {{
        return vertex;
    }}
    var c = vertex;
    for (var k = LOD_MAP_RINGS - 1; k >= 0; k--) {{
        let r = lod_map.rings[k];
        let edge = min(min(rel.x - r.rect.x, r.rect.z - rel.x), min(rel.z - r.rect.y, r.rect.w - rel.z)) * r.at.z;
        if (edge <= 1.0) {{
            continue;
        }}
        let uv = (r.at.xy + rel.xz * r.at.z) * r.at.w;
        let s = textureSampleLevel(lod_map_tex, tile_sampler, uv, k, 0.0);
        let a = smoothstep(1.0, LOD_MAP_FADE, edge) * step(0.5, s.a);
        c = mix(c, s.rgb * LOD_MAP_TOP, a);
    }}
    return mix(vertex, c, near);
}}
",
        rings = t.rings.len().min(MAX_RINGS),
        fade = t.ring_fade_texels as f32,
        t0 = top[0] as f32,
        t1 = top[1] as f32,
        t2 = top[2] as f32,
    )
}

/// The texture array and the uniform block.
pub struct ColorMapGpu {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub uniform: wgpu::Buffer,
    /// Texels square (1: the stand-in).
    pub size: u32,
}

impl ColorMapGpu {
    /// The stand-in (`full` false: 1×1) or the rings' texture.
    pub fn new(device: &wgpu::Device, full: bool) -> Self {
        let rings = LOD_MAP.rings.len().clamp(1, MAX_RINGS) as u32;
        let size = if full { LOD_MAP.rings[0].size as u32 } else { 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lod colour map"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: if full { rings } else { 1 },
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod colour map rings"),
            size: UNIFORM_VEC4S as u64 * 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            uniform,
            size,
        }
    }

    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 2] {
        [
            wgpu::BindGroupEntry {
                binding: UNIFORM_BINDING,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: TEXTURE_BINDING,
                resource: wgpu::BindingResource::TextureView(&self.view),
            },
        ]
    }

    /// GPU bytes held.
    pub fn bytes(&self) -> u64 {
        let layers = if self.size > 1 {
            LOD_MAP.rings.len() as u64
        } else {
            1
        };
        u64::from(self.size).pow(2) * 4 * layers + UNIFORM_VEC4S as u64 * 16
    }

    /// Writes a strip of ring `ring` (world texels `rect`, rgba8 rows of x) at its toroidal slots,
    /// split where it wraps.
    pub fn write(&self, queue: &wgpu::Queue, ring: usize, rect: Rect, rgba: &[u8]) {
        let n = i64::from(self.size);
        if self.size <= 1 || rect.width() > n || rect.height() > n {
            return;
        }
        if rgba.len() as i64 != rect.width() * rect.height() * 4 {
            return;
        }
        for (x0, x1) in wrap_spans(rect.x0, rect.x1, n) {
            for (z0, z1) in wrap_spans(rect.z0, rect.z1, n) {
                let offset = ((z0 - rect.z0) * rect.width() + (x0 - rect.x0)) * 4;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: toroidal(x0, n) as u32,
                            y: toroidal(z0, n) as u32,
                            z: ring as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: offset as u64,
                        bytes_per_row: Some((rect.width() * 4) as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: (x1 - x0) as u32,
                        height: (z1 - z0) as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
}

/// `a..b` (world texels, at most `n` long) split where its toroidal slots wrap past `n`.
pub fn wrap_spans(a: i64, b: i64, n: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut at = a;
    while at < b {
        let end = (at + n - toroidal(at, n)).min(b);
        out.push((at, end));
        at = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_splits_where_its_slots_wrap_and_each_piece_stays_in_the_texture() {
        assert_eq!(wrap_spans(510, 520, 512), vec![(510, 512), (512, 520)]);
        assert_eq!(wrap_spans(-3, 4, 512), vec![(-3, 0), (0, 4)]);
        for (a, b) in wrap_spans(-700, -188, 512) {
            assert!(toroidal(a, 512) + (b - a) <= 512);
        }
    }

    #[test]
    fn the_block_is_off_without_a_map_and_names_the_rings_face_with_one() {
        let off = uniforms(None, [0.0; 3]);
        assert_eq!(off[0], 0.0);
        let mut map = LodMap::default();
        map.update([0.0, 0.0], FaceFrame::B);
        let on = uniforms(Some(&map), [0.0, FaceFrame::B.y(40.0), 0.0]);
        assert_eq!((on[0], on[1]), (1.0, -1.0));
        // Nothing held yet: every ring's rectangle is empty.
        assert!(on[4] > on[6]);
    }
}
