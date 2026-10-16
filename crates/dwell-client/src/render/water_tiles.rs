//! The water's tiles (docs/WATER_RENDERING.md §8, "Cost"): what the water shader would otherwise
//! evaluate per pixel, drawn once into small tiling textures and sampled.
//!
//! - **Noise tile** (`rgba16float`, `water::NOISE_RES`²): gradient noise and its derivatives over
//!   `water::NOISE_TILE` lattice cells, wrapping. Drawn once. The ripple layers, the sparkle and the
//!   foam sample it instead of hashing four lattice points each (their periods are multiples of
//!   the tile, `water::tile_cells`).
//! - **Caustic tile** (`rg16float`, `water::CAUSTIC_RES`², a layer per fluid): the two caustic
//!   layers' cell-edge distances (Worley F2 − F1) over `water::CAUSTIC_TILE` cells, at this
//!   frame's time. Drawn each frame the water reads the scene (Medium and High), only for the
//!   fluids with caustics. Where a pixel covers at least a texel the caustics sample it; nearer,
//!   they are evaluated per pixel (`caustic_pattern`), so close up they stay exact.
//!
//! Both are in the frame bind group (bindings 5–7), so every pass that draws water reads them;
//! they are drawn by their own pipelines with a small bind group of the frame uniform and the
//! fluid table only (they can't read the textures they draw into).

use dwell_blocks::FluidInfo;

use super::water::{CAUSTIC_RES, NOISE_RES};

const NOISE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const CAUSTIC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The fluids whose caustic layer is drawn: those with caustics.
pub fn caustic_layers(fluids: &[FluidInfo]) -> Vec<u32> {
    fluids
        .iter()
        .enumerate()
        .filter(|(_, f)| f.caustics > 0.0)
        .map(|(i, _)| i as u32)
        .collect()
}

pub struct WaterTiles {
    noise: wgpu::Texture,
    noise_view: wgpu::TextureView,
    caustics: wgpu::Texture,
    caustic_view: wgpu::TextureView,
    /// Per drawn layer: its fluid id and a view of that layer to draw into.
    layer_views: Vec<(u32, wgpu::TextureView)>,
    sampler: wgpu::Sampler,
    noise_pipeline: wgpu::RenderPipeline,
    caustic_pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    noise_drawn: bool,
}

impl WaterTiles {
    /// The frame bind group's entries for the tiles (bindings 5, 6 and 7).
    pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        [
            texture(5, wgpu::TextureViewDimension::D2),
            texture(6, wgpu::TextureViewDimension::D2Array),
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]
    }

    /// The tiles for `fluids` (the fluid buffer's order), drawn with `module`'s `tile_vs`,
    /// `noise_tile_fs` and `caustic_tile_fs`, reading `frame` (the frame uniform) and `fluid_buffer`.
    pub fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        frame: &wgpu::Buffer,
        fluid_buffer: &wgpu::Buffer,
        fluids: &[FluidInfo],
    ) -> Self {
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
        let noise = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water noise tile"),
            size: wgpu::Extent3d {
                width: NOISE_RES,
                height: NOISE_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: NOISE_FORMAT,
            usage,
            view_formats: &[],
        });
        let noise_view = noise.create_view(&Default::default());
        let layers = fluids.len().max(1) as u32;
        let caustics = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water caustic tile"),
            size: wgpu::Extent3d {
                width: CAUSTIC_RES,
                height: CAUSTIC_RES,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAUSTIC_FORMAT,
            usage,
            view_formats: &[],
        });
        let caustic_view = caustics.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layer_views = caustic_layers(fluids)
            .into_iter()
            .map(|i| {
                let view = caustics.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: i,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                (i, view)
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water tiles"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water tiles"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(super::water::FLUID_STRIDE),
                    },
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water tiles"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: fluid_buffer.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water tiles"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("tile_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            noise_pipeline: pipeline("noise_tile_fs", NOISE_FORMAT),
            caustic_pipeline: pipeline("caustic_tile_fs", CAUSTIC_FORMAT),
            noise,
            noise_view,
            caustics,
            caustic_view,
            layer_views,
            sampler,
            group,
            noise_drawn: false,
        }
    }

    /// The frame bind group's entries for the tiles (`layout_entries`).
    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&self.noise_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&self.caustic_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ]
    }

    /// Whether `draw` would draw anything.
    pub fn pending(&self, caustics: bool) -> bool {
        !self.noise_drawn || (caustics && !self.layer_views.is_empty())
    }

    /// Draws the noise tile the first time, and this frame's caustic layers when `caustics` (the
    /// water reads the scene and some water is drawn or the eye is under it). Before the scene
    /// passes, which sample them. Returns the passes drawn.
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        caustics: bool,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let mut targets: Vec<(&wgpu::RenderPipeline, &wgpu::TextureView, u32)> = Vec::new();
        if !self.noise_drawn {
            targets.push((&self.noise_pipeline, &self.noise_view, 0));
            self.noise_drawn = true;
        }
        if caustics {
            for (fluid, view) in &self.layer_views {
                targets.push((&self.caustic_pipeline, view, *fluid));
            }
        }
        let writes = timestamp_writes;
        let n = targets.len();
        for (k, (pipeline, view, instance)) in targets.into_iter().enumerate() {
            // One timed span over all of them: the first pass's start, the last one's end.
            let tw = writes.as_ref().map(|w| wgpu::RenderPassTimestampWrites {
                query_set: w.query_set,
                beginning_of_pass_write_index: if k == 0 {
                    w.beginning_of_pass_write_index
                } else {
                    None
                },
                end_of_pass_write_index: if k + 1 == n {
                    w.end_of_pass_write_index
                } else {
                    None
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("water tile"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: tw,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.draw(0..3, instance..instance + 1);
        }
        n as u32
    }

    /// GPU memory the tiles hold (bytes).
    pub fn bytes(&self) -> u64 {
        u64::from(NOISE_RES * NOISE_RES) * 8
            + u64::from(CAUSTIC_RES * CAUSTIC_RES)
                * 4
                * u64::from(self.caustics.depth_or_array_layers())
    }
}

impl Drop for WaterTiles {
    fn drop(&mut self) {
        self.noise.destroy();
        self.caustics.destroy();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_fluids_with_caustics_get_a_layer_drawn() {
        let fluids = &dwell_blocks::FLUIDS[..];
        let layers = super::caustic_layers(fluids);
        for (i, f) in fluids.iter().enumerate() {
            assert_eq!(layers.contains(&(i as u32)), f.caustics > 0.0, "{}", f.name);
        }
        let mut none = fluids.to_vec();
        for f in &mut none {
            f.caustics = 0.0;
        }
        assert!(super::caustic_layers(&none).is_empty());
    }
}
