//! Render scale and upscaling (docs/DEV_TOOLS.md, Render scale and upscaling; ARCHITECTURE.md
//! §6.10; the choices are data, shared/render/upscale.json). The scene and every post pass draw at
//! a share of the screen's pixels per axis, then one of a fixed vocabulary of upscalers brings the
//! picture to the screen:
//!
//! - `Stretch`: the canvas's drawing buffer itself is smaller and the browser stretches it to the
//!   page (CSS, bilinear). No pass, and the browser's compositing of the canvas shrinks too.
//! - `Sharpen`: the canvas stays full size; the frame draws into a low-res picture, and two passes
//!   bring it up: FSR 1's EASU (edge-adaptive upscale) into a full-size picture, then its RCAS
//!   (contrast-adaptive sharpen) onto the canvas (upscale.wgsl).
//! - `StretchPass`: the plain stretch as a pass onto a full-size canvas (for comparing the
//!   browser's stretch with an internal one; not in the menu).
//!
//! A CPU adapter keeps its own reduced buffer (`CPU_RESOLUTION`, stretched) and ignores the scale.

/// WGSL of the upscale passes.
pub const UPSCALE_WGSL: &str = include_str!("upscale.wgsl");

/// The renderer's upscalers: a small fixed vocabulary, named in shared/render/upscale.json.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Upscaler {
    #[default]
    Stretch,
    Sharpen,
    StretchPass,
}

impl Upscaler {
    /// Every kind with its name (the data's `id`).
    pub const ALL: [(Upscaler, &'static str); 3] = [
        (Upscaler::Stretch, "stretch"),
        (Upscaler::Sharpen, "sharpen"),
        (Upscaler::StretchPass, "stretch-pass"),
    ];

    /// The kind named `name`, if it is one.
    pub fn from_name(name: &str) -> Option<Upscaler> {
        Self::ALL.iter().find(|(_, n)| *n == name).map(|(u, _)| *u)
    }

    /// Whether the canvas stays full size and a pass upscales onto it.
    pub fn is_pass(self) -> bool {
        !matches!(self, Upscaler::Stretch)
    }
}

/// The lowest render scale (each axis).
pub const MIN_SCALE: f64 = 0.5;

/// A render scale into range: [MIN_SCALE, 1]; anything not finite is 1.
pub fn clamp_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(MIN_SCALE, 1.0)
    } else {
        1.0
    }
}

/// RCAS's sharpness from its stops (0 the strongest), as FSR's `FsrRcasCon`: 2^-stops.
pub fn sharpness(stops: f64) -> f32 {
    let s = if stops.is_finite() {
        stops.clamp(0.0, 4.0)
    } else {
        0.25
    };
    (-s).exp2() as f32
}

/// The sizes a frame draws at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    /// The canvas's drawing buffer (pixels).
    pub canvas: (u32, u32),
    /// The canvas's pixels per CSS pixel.
    pub canvas_ratio: f64,
    /// The scene's picture (pixels): the canvas's, or smaller with an upscale pass.
    pub render: (u32, u32),
}

fn scaled(css: (f64, f64), ratio: f64) -> (u32, u32) {
    (
        ((css.0 * ratio).round() as u32).max(1),
        ((css.1 * ratio).round() as u32).max(1),
    )
}

/// The canvas's and the scene's sizes for a page of `css` pixels at `ratio` pixels per CSS pixel
/// (the adapter's, `pixel_ratio_for`), drawn at `scale` with `upscaler`; `cpu` for a CPU adapter
/// (its reduced buffer is already in `ratio`; the scale is ignored there).
pub fn sizes(css: (f64, f64), ratio: f64, scale: f64, upscaler: Upscaler, cpu: bool) -> Sizes {
    let scale = if cpu { 1.0 } else { clamp_scale(scale) };
    if upscaler.is_pass() {
        let canvas = scaled(css, ratio);
        Sizes {
            canvas,
            canvas_ratio: ratio,
            render: scaled(css, ratio * scale),
        }
    } else {
        let canvas = scaled(css, ratio * scale);
        Sizes {
            canvas,
            canvas_ratio: ratio * scale,
            render: canvas,
        }
    }
}

/// The upscale passes: their pipelines, made once on first use.
pub struct UpscalePass {
    easu: wgpu::RenderPipeline,
    rcas: wgpu::RenderPipeline,
    bilinear: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    low: Option<Low>,
}

/// The low-res picture the frame draws into, and the full-size one EASU draws for RCAS.
pub struct Low {
    _textures: [wgpu::Texture; 2],
    pub view: wgpu::TextureView,
    mid: wgpu::TextureView,
    /// Reading the low picture (EASU, bilinear) and the mid one (RCAS).
    groups: [wgpu::BindGroup; 2],
    size: (u32, u32),
    out: (u32, u32),
}

impl UpscalePass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("upscale"),
            source: wgpu::ShaderSource::Wgsl(UPSCALE_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("upscale"),
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
            label: Some("upscale"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |label, entry| {
            crate::render::pipeline::create(
                device,
                &wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("up_vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                },
            )
        };
        let easu = make("upscale easu", "easu_fs");
        let rcas = make("upscale rcas", "rcas_fs");
        let bilinear = make("upscale bilinear", "bilinear_fs");
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upscale"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            easu,
            rcas,
            bilinear,
            layout,
            buffer,
            low: None,
        }
    }

    /// The low-res picture of `size` for an output of `out`, made again when either changes.
    pub fn low(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        out: (u32, u32),
    ) -> &Low {
        if self
            .low
            .as_ref()
            .is_some_and(|l| l.size != size || l.out != out)
        {
            self.low = None;
        }
        let (layout, buffer) = (&self.layout, &self.buffer);
        self.low.get_or_insert_with(|| {
            let texture = |label, (width, height): (u32, u32)| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            let textures = [texture("upscale low", size), texture("upscale mid", out)];
            let view = textures[0].create_view(&Default::default());
            let mid = textures[1].create_view(&Default::default());
            let group = |v: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("upscale"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(v),
                        },
                    ],
                })
            };
            let groups = [group(&view), group(&mid)];
            Low {
                _textures: textures,
                view,
                mid,
                groups,
                size,
                out,
            }
        })
    }

    /// Drops the pictures (the scale back to 100 %).
    pub fn release(&mut self) {
        self.low = None;
    }

    /// Draws the low-res picture onto `target` (the canvas) with `upscaler` at `sharpness`.
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        upscaler: Upscaler,
        sharpness: f32,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let Some(low) = &self.low else {
            return;
        };
        let u = [low.out.0 as f32, low.out.1 as f32, sharpness, 0.0];
        let bytes: Vec<u8> = u.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.buffer, 0, &bytes);
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    view: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup,
                    writes| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: writes,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        match upscaler {
            Upscaler::Sharpen => {
                // Timed as one: EASU's pass writes the start, RCAS's the end.
                let (start, end) = match timestamp_writes {
                    Some(w) => (
                        Some(wgpu::RenderPassTimestampWrites {
                            query_set: w.query_set,
                            beginning_of_pass_write_index: w.beginning_of_pass_write_index,
                            end_of_pass_write_index: None,
                        }),
                        Some(wgpu::RenderPassTimestampWrites {
                            query_set: w.query_set,
                            beginning_of_pass_write_index: None,
                            end_of_pass_write_index: w.end_of_pass_write_index,
                        }),
                    ),
                    None => (None, None),
                };
                pass(encoder, &low.mid, &self.easu, &low.groups[0], start);
                pass(encoder, target, &self.rcas, &low.groups[1], end);
            }
            _ => pass(
                encoder,
                target,
                &self.bilinear,
                &low.groups[0],
                timestamp_writes,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upscale_passes_parse_and_validate() {
        let module = naga::front::wgsl::parse_str(UPSCALE_WGSL).expect("upscale.wgsl parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("upscale.wgsl validates");
        for entry in ["up_vs", "easu_fs", "rcas_fs", "bilinear_fs"] {
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry} is an entry point"
            );
        }
    }

    #[test]
    fn every_upscaler_the_data_names_is_one_the_renderer_knows() {
        let data = include_str!("../../../../shared/render/upscale.json");
        let ids: Vec<&str> = data
            .split("\"id\": \"")
            .skip(1)
            .map(|s| s.split('"').next().unwrap())
            .collect();
        assert_eq!(ids, ["stretch", "sharpen"]);
        for id in ids {
            assert!(Upscaler::from_name(id).is_some(), "{id} is an upscaler");
        }
        assert_eq!(
            Upscaler::from_name("stretch-pass"),
            Some(Upscaler::StretchPass)
        );
        assert_eq!(Upscaler::from_name("fsr2"), None);
    }

    #[test]
    fn the_buffer_sizes_per_scale() {
        // 1280 × 800 CSS pixels at 2 pixels each: 2560 × 1600.
        let css = (1280.0, 800.0);
        for (scale, w, h) in [
            (1.0, 2560, 1600),
            (0.75, 1920, 1200),
            (0.67, 1715, 1072),
            (0.5, 1280, 800),
        ] {
            let s = sizes(css, 2.0, scale, Upscaler::Stretch, false);
            assert_eq!((s.canvas, s.render), ((w, h), (w, h)), "stretch at {scale}");
            assert!((s.canvas_ratio - 2.0 * scale).abs() < 1e-12);
            let s = sizes(css, 2.0, scale, Upscaler::Sharpen, false);
            assert_eq!(
                (s.canvas, s.render),
                ((2560, 1600), (w, h)),
                "sharpen at {scale}"
            );
            assert_eq!(s.canvas_ratio, 2.0);
        }
        // Out of range: clamped to 50–100 %.
        assert_eq!(
            sizes(css, 2.0, 0.1, Upscaler::Stretch, false).canvas,
            (1280, 800)
        );
        assert_eq!(
            sizes(css, 2.0, 3.0, Upscaler::Stretch, false).canvas,
            (2560, 1600)
        );
        assert_eq!(
            sizes(css, 2.0, f64::NAN, Upscaler::Sharpen, false).render,
            (2560, 1600)
        );
        // A CPU adapter keeps its own buffer (already in the ratio) and ignores the scale.
        let s = sizes(css, 1.0, 0.5, Upscaler::Sharpen, true);
        assert_eq!((s.canvas, s.render), ((1280, 800), (1280, 800)));
    }

    #[test]
    fn sharpness_is_two_to_the_minus_stops() {
        assert_eq!(sharpness(0.0), 1.0);
        assert_eq!(sharpness(1.0), 0.5);
        assert_eq!(sharpness(-3.0), 1.0);
        assert_eq!(sharpness(f64::NAN), (-0.25f64).exp2() as f32);
    }
}
