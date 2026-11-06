//! The water's flow on the GPU (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3d): the current of each
//! column within half a window of the eye, in one two-channel texture of `window_columns` square
//! (`shared/look/waterflow.json`), one texel a column, held at the toroidal slot `x mod N, z mod N`
//! so it wraps as the eye moves (sampled with the water tiles' repeating linear sampler, binding 7).
//! The game fills it from its current table (`Currents::window`) when the table changes or the eye
//! crosses a column; the water shader carries its ripples along it (`flow_at`) and draws foam
//! streaks with it. The switches (`FLOW_RIPPLES`, `FLOW_FOAM`) ride in the frame's `wrap.w`.

use dwell_look::waterflow::WATER_FLOW;

/// The frame group's binding.
pub const BINDING: u32 = 19;
/// Switches in `wrap.w` (bits): ripples carried by the current, foam streaks.
pub const FLOW_RIPPLES: u32 = 1;
pub const FLOW_FOAM: u32 = 2;

/// Columns square the texture holds.
pub fn window() -> u32 {
    WATER_FLOW.window_columns
}

pub fn layout_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: BINDING,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The texture (rg8snorm: each column's current over `range_ms`).
pub struct FlowGpu {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl FlowGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let n = window();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water flow"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Snorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }

    pub fn group_entry(&self) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: BINDING,
            resource: wgpu::BindingResource::TextureView(&self.view),
        }
    }

    /// Writes the whole window: `texels` holds `2 · N²` snorm bytes, row z, then x.
    pub fn write(&self, queue: &wgpu::Queue, texels: &[i8]) {
        let n = window();
        if texels.len() != (2 * n * n) as usize {
            return;
        }
        let bytes: Vec<u8> = texels.iter().map(|&v| v as u8).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(2 * n),
                rows_per_image: Some(n),
            },
            wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: 1,
            },
        );
    }

    /// GPU bytes held.
    pub fn bytes() -> u64 {
        2 * u64::from(window()).pow(2)
    }
}

/// The shader's half (joined to the scene shaders): `flow_at`, the current (m/s, world x and z) at
/// a wrapped surface point `wrapped` (render/water.rs `wrapped_origin`'s frame) `rel` from the eye,
/// faded out over the window's edge band; zero with both switches off.
pub fn wgsl() -> String {
    let t = WATER_FLOW;
    format!(
        "@group(0) @binding({BINDING}) var flow_tex: texture_2d<f32>;
const FLOW_WINDOW: f32 = {n:?};
const FLOW_RANGE: f32 = {range:?};
const FLOW_FADE_FROM: f32 = {from:?};
const FLOW_FADE_TO: f32 = {to:?};
const FLOW_RIPPLES: u32 = {FLOW_RIPPLES}u;
const FLOW_FOAM: u32 = {FLOW_FOAM}u;

fn flow_on(bit: u32) -> bool {{
    return (u32(frame.wrap.w + 0.5) & bit) != 0u;
}}

// The current (m/s along world x and z) at the wrapped surface point `wrapped`, `rel` from the eye
// (docs/WATER_SURFACE_GRID.md §2c): the flow texture's column, faded out toward its window's edge.
fn flow_at(wrapped: vec2f, rel: vec3f) -> vec2f {{
    let fade = 1.0 - smoothstep(FLOW_FADE_FROM, FLOW_FADE_TO, length(rel.xz));
    if (fade <= 0.0) {{
        return vec2f(0.0);
    }}
    let s = textureSampleLevel(flow_tex, tile_sampler, wrapped / FLOW_WINDOW, 0.0).xy;
    return s * FLOW_RANGE * fade;
}}
",
        n = t.window_columns as f32,
        range = t.range_ms as f32,
        from = t.fade_from_m as f32,
        to = t.fade_to_m as f32,
    )
}
