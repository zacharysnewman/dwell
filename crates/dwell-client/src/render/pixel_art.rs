//! The pixel-art preview's post pass (docs/proposals/PIXEL_ART_LOOK.md; dwell-look `pixel_art`):
//! while a preset is on, the composite and the lines draw into a low-res picture (`Low`) in the
//! canvas's format, and this pass draws it on the canvas, snapped to the palette with the ordered
//! dither and upscaled by an integer, nearest-neighbour. Made on first use, so a frame without the
//! preview never builds it.

use dwell_look::pixel_art::POST_FLOATS;

/// The pass: its pipeline and block, made once.
pub struct PixelPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    /// The low-res picture and its group, made again when the size changes.
    low: Option<Low>,
}

/// The low-res picture the composite draws into.
pub struct Low {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    group: wgpu::BindGroup,
    size: (u32, u32),
}

impl PixelPass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pixel art"),
            source: wgpu::ShaderSource::Wgsl(dwell_look::pixel_art::POST_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pixel art"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pixel art"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixel art"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("post_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("post_fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pixel art"),
            size: POST_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            buffer,
            low: None,
        }
    }

    /// The low-res picture of `size`, made again when the size changes.
    pub fn low(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> &Low {
        if self.low.as_ref().is_some_and(|l| l.size != size) {
            self.low = None;
        }
        let (layout, buffer) = (&self.layout, &self.buffer);
        self.low.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pixel art low"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pixel art"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            Low {
                _texture: texture,
                view,
                group,
                size,
            }
        })
    }

    /// Drops the low-res picture (the preview turned off).
    pub fn release(&mut self) {
        self.low = None;
    }

    /// Draws the low-res picture into `target` (the canvas) with the block `uniforms`
    /// (dwell-look `post_uniforms`).
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        uniforms: &[f32],
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let Some(low) = &self.low else {
            return;
        };
        let bytes: Vec<u8> = uniforms.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.buffer, 0, &bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pixel art"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &low.group, &[]);
        pass.draw(0..3, 0..1);
    }
}
