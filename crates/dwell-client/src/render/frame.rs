//! The shared frame (docs/RAY_MARCHED_RENDERER.md step A1, docs/WATER_RENDERING.md W2): what the scene passes draw into and
//! the composite reads.
//!
//! - **Colour**: `rgba16float`, lit linear colour premultiplied by coverage (cleared to 0: alpha is
//!   the share of the pixel something covers). Multisampled, resolved by the last scene pass into a
//!   single-sample texture the composite samples.
//! - **View depth**: `r32float`, each opaque fragment's depth along the view axis in metres (0:
//!   nothing). One target spans both depth passes (the far one clears it, the near one draws over
//!   it), so it holds one meaning across the far/near split, where the depth buffer is cleared
//!   between them and holds each pass's own projection. WebGPU can't resolve `r32float`, so it is
//!   multisampled like the scene and stored; readers load its samples (the composite averages the
//!   covered ones). The depth buffer itself stays attachment-only and discarded.
//! - **Depth buffer**: `depth32float`, as before.

pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const VIEW_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// How a scene pass treats the colour and view-depth targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassOps {
    /// Clears them (the first pass) or draws over the pass before.
    pub clear: bool,
    /// Resolves the multisampled colour into the composite's texture (the last pass, with MSAA).
    pub resolve: bool,
    /// Keeps the colour attachment's contents after the pass (the next pass loads them; without
    /// MSAA the attachment is the composite's texture). Discarded once resolved.
    pub store_colour: bool,
}

/// Pass `pass` of `passes` scene passes at `samples` per pixel.
pub fn scene_pass_ops(pass: usize, passes: usize, samples: u32) -> PassOps {
    let resolve = pass + 1 == passes && samples > 1;
    PassOps {
        clear: pass == 0,
        resolve,
        store_colour: !resolve,
    }
}

/// GPU memory per drawing-buffer pixel (`RenderStats::screen_bytes`): per sample the colour (8),
/// view depth (4) and depth (4); the resolved colour (8) with MSAA; the canvas's two buffers (8).
pub fn bytes_per_pixel(samples: u32) -> u64 {
    let samples = u64::from(samples);
    samples * (8 + 4 + 4) + if samples > 1 { 8 } else { 0 } + 8
}

/// The composite's and lines' view of the scene targets (group 2): the resolved colour and the
/// view depth (multisampled with the scene), loaded by texel, never filtered.
pub fn scene_layout(device: &wgpu::Device, samples: u32) -> wgpu::BindGroupLayout {
    let texture = |binding, multisampled| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene"),
        entries: &[texture(0, false), texture(1, samples > 1)],
    })
}

type Target = (wgpu::Texture, wgpu::TextureView);

/// The frame's targets at one drawing-buffer size, destroyed when it changes.
pub struct Targets {
    /// The multisampled colour (none with one sample: the scene draws into `colour`).
    pub colour_ms: Option<Target>,
    /// The colour the composite reads.
    pub colour: Target,
    pub view_depth: Target,
    pub depth: Target,
    /// `colour` and `view_depth` for the composite and lines (`scene_layout`).
    pub scene: wgpu::BindGroup,
}

impl Targets {
    pub fn new(
        device: &wgpu::Device,
        (width, height): (u32, u32),
        samples: u32,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let make = |format, label, samples, usage| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let read = attach | wgpu::TextureUsages::TEXTURE_BINDING;
        let colour_ms = (samples > 1).then(|| make(HDR, "colour (multisampled)", samples, attach));
        let colour = make(HDR, "colour", 1, read);
        let view_depth = make(VIEW_DEPTH, "view depth", samples, read);
        let depth = make(DEPTH, "depth", samples, attach);
        let scene = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&colour.1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view_depth.1),
                },
            ],
        });
        Self {
            colour_ms,
            colour,
            view_depth,
            depth,
            scene,
        }
    }

    /// The colour attachment the scene passes draw into.
    pub fn scene_colour(&self) -> &wgpu::TextureView {
        self.colour_ms.as_ref().map_or(&self.colour.1, |c| &c.1)
    }
}

impl Drop for Targets {
    fn drop(&mut self) {
        if let Some((t, _)) = &self.colour_ms {
            t.destroy();
        }
        self.colour.0.destroy();
        self.view_depth.0.destroy();
        self.depth.0.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_far_pass_clears_and_the_near_pass_resolves_with_msaa() {
        let far = scene_pass_ops(0, 2, 4);
        let near = scene_pass_ops(1, 2, 4);
        assert_eq!(
            far,
            PassOps {
                clear: true,
                resolve: false,
                store_colour: true
            },
            "the near pass loads what the far pass drew"
        );
        assert_eq!(
            near,
            PassOps {
                clear: false,
                resolve: true,
                store_colour: false
            },
            "resolved once, at the end; the multisampled colour is then discarded"
        );
    }

    #[test]
    fn without_msaa_the_scene_draws_straight_into_the_composites_texture() {
        for pass in 0..2 {
            let ops = scene_pass_ops(pass, 2, 1);
            assert!(!ops.resolve && ops.store_colour);
            assert_eq!(ops.clear, pass == 0);
        }
    }

    #[test]
    fn screen_memory_counts_every_target() {
        assert_eq!(bytes_per_pixel(4), 4 * 16 + 8 + 8);
        assert_eq!(bytes_per_pixel(1), 16 + 8);
    }
}
