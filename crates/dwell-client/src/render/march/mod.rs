//! The ray-marched terrain renderer's GPU side (docs/RAY_MARCHED_RENDERER.md §4, Phase 25 step
//! A3b): `dwell_march`'s brick structure in GPU buffers ([`gpu_scene`]), marched per pixel by a
//! compute pass ([`wgsl`], a transcription of the reference marcher) that writes the lit linear
//! colour, the view depth and a cell id + face, shaded with the raster renderer's lighting; the
//! shared frame's composite (fog, tone mapping, the sky) draws it to the screen.
//!
//! Behind the renderer seam (step A2, `seam.rs`) it is [`MarchRenderer`]: terrain arrives as
//! [`ChunkBricks`] and [`SectionBricks`] (`set_chunk`, `set_section`, `None` removes), built in the
//! job workers with the meshing jobs (their `Payloads` mask), and [`Marcher::draw`] records the march
//! into [`MarchTargets`] for a [`MarchFrame`] (the camera and the frame uniforms the raster
//! renderer fills, `Renderer::march_frame`), copied into the shared frame's targets. The
//! `?marchtest=1` page (`web/march_test.rs`) draws a generated world with it alone.

pub mod check;
pub mod gpu_scene;
pub mod looks;
pub mod renderer;
pub mod stats;
pub mod wgsl;

#[cfg(test)]
mod perf;
#[cfg(test)]
mod test;

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use dwell_march::{NodeBuild, StateTable, StepSummary, chunk_node};
use dwell_world::ChunkCoord;
use dwell_world::lod::LodCoord;

use gpu_scene::GpuScene;
use stats::{DEBUG_WORDS, HIST_WORDS};

pub use renderer::{
    BUILT, ChunkBricks, MarchRenderer, SectionBricks, build_chunk_bricks, build_section_bricks,
};

/// Words of `MarchUniforms` (wgsl.rs; a test checks the struct's size).
pub const MARCH_UNIFORM_WORDS: usize =
    dwell_march::layout::LEVEL_COUNT * dwell_march::layout::LEVEL_WORDS + 32;

pub const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const ID: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// What the march is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarchSettings {
    /// Iterations before a ray gives up (drawn as the sky behind the fog; counted).
    pub step_cap: u32,
    /// The LOD's pixel error (cone LOD): 4 desktop, 8 phones.
    pub lod_pixels: f32,
    /// The farthest hit (metres): past it the fog has the colour.
    pub max_t: f32,
    /// §4.3 items 3 and 4.
    pub columns: bool,
    pub distances: bool,
    /// Every this many frames the steps-per-ray histogram is filled and read back (0: never).
    pub histogram_every: u32,
    /// Profiling switches (`wgsl::DBG_*`, `?marchdbg=`): parts of the per-step work turned off.
    /// 0 in normal use; anything else draws a wrong image.
    pub debug: u32,
}

impl Default for MarchSettings {
    fn default() -> Self {
        Self {
            step_cap: 512,
            lod_pixels: 4.0,
            max_t: 1.0e6,
            columns: true,
            distances: true,
            histogram_every: 30,
            debug: 0,
        }
    }
}

/// The camera and the shared frame uniforms of one frame.
#[derive(Clone, Debug)]
pub struct MarchFrame {
    /// The raster renderer's `Frame` uniforms (shaders.rs): the lights, the fog, the camera axes.
    pub uniforms: Vec<f32>,
    /// World metres.
    pub eye: [f64; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    /// Half-angle tangents (x, y).
    pub tan: [f32; 2],
}

impl MarchFrame {
    /// The ray through pixel (x, y) of a `w` × `h` image, unit length, as the shader makes it
    /// (up to rounding: the test reads the GPU's own directions back).
    pub fn ray(&self, x: u32, y: u32, (w, h): (u32, u32)) -> [f32; 3] {
        let nx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
        let ny = 1.0 - (y as f32 + 0.5) / h as f32 * 2.0;
        let d: [f32; 3] = std::array::from_fn(|k| {
            self.forward[k] + nx * self.tan[0] * self.right[k] + ny * self.tan[1] * self.up[k]
        });
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        d.map(|v| v / len)
    }
}

/// The march's outputs at one size: colour, view depth and id (storage textures, sampled by the
/// composite), and the bind groups over them.
pub struct MarchTargets {
    pub size: (u32, u32),
    pub color: (wgpu::Texture, wgpu::TextureView),
    pub depth: (wgpu::Texture, wgpu::TextureView),
    pub id: (wgpu::Texture, wgpu::TextureView),
    out_group: wgpu::BindGroup,
    /// `frame::scene_layout(_, 1)`: what the composite reads.
    pub scene: wgpu::BindGroup,
}

impl Drop for MarchTargets {
    fn drop(&mut self) {
        self.color.0.destroy();
        self.depth.0.destroy();
        self.id.0.destroy();
    }
}

const RING: usize = 2;
const IDLE: u8 = 0;
const IN_FLIGHT: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

pub struct Marcher {
    device: wgpu::Device,
    output: wgpu::TextureFormat,
    queue: wgpu::Queue,
    pub gpu: GpuScene,
    pub settings: MarchSettings,
    pipeline: wgpu::ComputePipeline,
    /// Kept to rebuild the pipeline from another source (profiling variants, tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pipeline_layout: wgpu::PipelineLayout,
    composite: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    data_layout: wgpu::BindGroupLayout,
    out_layout: wgpu::BindGroupLayout,
    scene_layout: wgpu::BindGroupLayout,
    frame_buffer: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    stats: wgpu::Buffer,
    stats_words: usize,
    data_group: Option<wgpu::BindGroup>,
    readback: [wgpu::Buffer; RING],
    readback_state: [Arc<AtomicU8>; RING],
    /// The ring buffer this frame's histogram goes to.
    pending: Option<usize>,
    /// Debug records per pixel (the GPU-vs-reference test), for this many pixels.
    debug_pixels: usize,
    frames: u64,
    /// The last histogram read back.
    pub last_steps: Option<StepSummary>,
    _atlas: (wgpu::TextureView, wgpu::Sampler),
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_texture(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

impl Marcher {
    /// A marcher whose composite draws into `output` targets (a canvas's format, or `Rgba8Unorm`
    /// offscreen).
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, output: wgpu::TextureFormat) -> Self {
        Self::with_table(device, queue, output, StateTable::registry())
    }

    /// A marcher over a state table (the registry's, or a fixture's).
    pub fn with_table(
        device: wgpu::Device,
        queue: wgpu::Queue,
        output: wgpu::TextureFormat,
        table: StateTable,
    ) -> Self {
        let looks = if table.len() == dwell_blocks::STATES.len() {
            looks::registry_looks(&table)
        } else {
            // A fixture table: its classes, the registry's looks where it has them.
            let mut w = looks::registry_looks(&StateTable::registry());
            w.resize(table.len() * looks::LOOK_WORDS, 0);
            for (i, f) in table.to_words().chunks(2).enumerate() {
                w[i * looks::LOOK_WORDS] = f[0];
            }
            w
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march"),
            source: wgpu::ShaderSource::Wgsl(wgsl::source().into()),
        });
        let vis = wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: vis,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: vis,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mut data_entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        data_entries.extend((1..=6).map(|b| storage(b, true)));
        data_entries.push(storage(7, false));
        let data_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march data"),
            entries: &data_entries,
        });
        let out_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march out"),
            entries: &[
                storage_texture(0, COLOR),
                storage_texture(1, DEPTH),
                storage_texture(2, ID),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march"),
            bind_group_layouts: &[Some(&frame_layout), Some(&data_layout), Some(&out_layout)],
            immediate_size: 0,
        });
        let pipeline = Self::march_pipeline(&device, &layout, &module);
        // The shared composite (shaders.rs, one sample per pixel) over the march's targets.
        let scene_layout = super::frame::scene_layout(&device, 1);
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("empty"),
            entries: &[],
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march composite"),
            bind_group_layouts: &[Some(&frame_layout), Some(&empty), Some(&scene_layout)],
            immediate_size: 0,
        });
        let raster = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march composite"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::source(1).into()),
        });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("march composite"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module: &raster,
                entry_point: Some("sky_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &raster,
                entry_point: Some("composite_fs"),
                compilation_options: Default::default(),
                targets: &[Some(output.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let atlas = super::Renderer::atlas(&device, &queue);
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("march frame"),
            size: super::FRAME_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march frame"),
            layout: &frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.0),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.1),
                },
            ],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("march uniforms"),
            size: MARCH_UNIFORM_WORDS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stats = Self::stats_buffer(&device, HIST_WORDS);
        let readback = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("march histogram readback"),
                size: HIST_WORDS as u64 * 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let gpu = GpuScene::new(&device, table, looks);
        Self {
            device,
            output,
            queue,
            gpu,
            settings: MarchSettings::default(),
            pipeline,
            pipeline_layout: layout,
            composite,
            frame_layout,
            data_layout,
            out_layout,
            scene_layout,
            frame_buffer,
            frame_group,
            uniforms,
            stats,
            stats_words: HIST_WORDS,
            data_group: None,
            readback,
            readback_state: std::array::from_fn(|_| Arc::new(AtomicU8::new(IDLE))),
            pending: None,
            debug_pixels: 0,
            frames: 0,
            last_steps: None,
            _atlas: atlas,
        }
    }

    fn march_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule,
    ) -> wgpu::ComputePipeline {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("march"),
            layout: Some(layout),
            module,
            entry_point: Some("march_main"),
            compilation_options: Default::default(),
            cache: None,
        })
    }

    /// Replaces the march's shader (profiling variants: `perf.rs`).
    #[cfg(test)]
    pub(crate) fn set_source(&mut self, source: &str) {
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("march variant"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        self.pipeline = Self::march_pipeline(&self.device, &self.pipeline_layout, &module);
    }

    fn stats_buffer(device: &wgpu::Device, words: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("march stats"),
            size: 4 * words as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// The frame layout the march and its composite share (group 0).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    /// The composite's output format.
    pub fn output(&self) -> wgpu::TextureFormat {
        self.output
    }

    pub fn frame_layout(&self) -> &wgpu::BindGroupLayout {
        &self.frame_layout
    }

    /// A chunk's bricks (`None`: gone).
    pub fn set_chunk(&mut self, c: ChunkCoord, bricks: Option<&NodeBuild>) {
        let n = chunk_node(c);
        match bricks {
            Some(b) => {
                self.gpu.put(0, n, b);
            }
            None => self.gpu.scene.remove(0, n),
        }
    }

    /// A LOD section's bricks at its level (`None`: gone).
    pub fn set_section(&mut self, c: LodCoord, bricks: Option<&NodeBuild>) {
        let n = [c.i, c.j, c.k];
        match bricks {
            Some(b) => {
                self.gpu.put(c.level as u32, n, b);
            }
            None => self.gpu.scene.remove(c.level as u32, n),
        }
    }

    /// Records a debug record per pixel for the next draws (tests), for `pixels` pixels; 0 stops.
    pub fn set_debug(&mut self, pixels: usize) {
        self.debug_pixels = pixels;
        let words = HIST_WORDS + pixels * DEBUG_WORDS;
        if words != self.stats_words {
            self.stats.destroy();
            self.stats = Self::stats_buffer(&self.device, words);
            self.stats_words = words;
            self.data_group = None;
        }
    }

    pub fn stats_buffer_ref(&self) -> &wgpu::Buffer {
        &self.stats
    }

    /// Targets at `size`.
    pub fn targets(&self, size: (u32, u32)) -> MarchTargets {
        let make = |format, label| {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        };
        let color = make(COLOR, "march colour");
        let depth = make(DEPTH, "march depth");
        let id = make(ID, "march id");
        fn view(b: u32, v: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding: b,
                resource: wgpu::BindingResource::TextureView(v),
            }
        }
        let out_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march out"),
            layout: &self.out_layout,
            entries: &[view(0, &color.1), view(1, &depth.1), view(2, &id.1)],
        });
        let scene = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march scene"),
            layout: &self.scene_layout,
            entries: &[view(0, &color.1), view(1, &depth.1)],
        });
        MarchTargets {
            size,
            color,
            depth,
            id,
            out_group,
            scene,
        }
    }

    /// The uniforms' words for a frame at `size`.
    pub fn uniform_words(&self, frame: &MarchFrame, size: (u32, u32), debug: bool) -> Vec<u32> {
        let s = &self.gpu.scene;
        let mut w = s.level_words();
        let ray = dwell_march::Ray::from_world(frame.eye, frame.forward);
        let (top, data) = match s.levels_in_use() {
            Some((_, top)) => (top, true),
            None => (0, false),
        };
        let (wlo, whi) = s.bounds().unwrap_or(([0; 3], [0; 3]));
        let st = &self.settings;
        let mut flags = 0;
        if st.columns {
            flags |= wgsl::FLAG_COLUMNS;
        }
        if st.distances {
            flags |= wgsl::FLAG_DISTANCES;
        }
        if self.pending.is_some() {
            flags |= wgsl::FLAG_HISTOGRAM;
        }
        if debug {
            flags |= wgsl::FLAG_DEBUG;
        }
        if data {
            flags |= wgsl::FLAG_DATA;
        }
        let f = f32::to_bits;
        let i = |v: i32| v as u32;
        let pixel_angle = 2.0 * frame.tan[1] / size.1.max(1) as f32;
        w.extend(ray.cell.map(i));
        w.push(top);
        w.extend(ray.frac.map(f));
        w.push(f(st.max_t));
        w.extend(wlo.map(i));
        w.push(st.step_cap.min(stats::MAX_CAP));
        w.extend(whi.map(i));
        w.push(flags);
        w.extend(frame.forward.map(f));
        w.push(f(pixel_angle));
        w.extend(frame.right.map(f));
        w.push(f(st.lod_pixels));
        w.extend(frame.up.map(f));
        w.push(f(frame.tan[0]));
        w.extend([size.0, size.1, f(frame.tan[1]), st.debug]);
        debug_assert_eq!(w.len(), MARCH_UNIFORM_WORDS);
        w
    }

    /// The reference marcher's parameters for this frame (what the GPU is told).
    pub fn reference_params(
        &self,
        frame: &MarchFrame,
        size: (u32, u32),
    ) -> dwell_march::MarchParams {
        dwell_march::MarchParams {
            max_t: self.settings.max_t,
            step_cap: self.settings.step_cap.min(stats::MAX_CAP),
            pixel_angle: 2.0 * frame.tan[1] / size.1.max(1) as f32,
            lod_pixels: self.settings.lod_pixels,
            columns: self.settings.columns,
            distances: self.settings.distances,
        }
    }

    /// Uploads what changed and records the march into `targets`.
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &MarchFrame,
        targets: &MarchTargets,
        timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let bytes = self.gpu.sync(&self.device, &self.queue);
        if bytes > 0 {
            crate::telemetry::count("upload.march.bytes", bytes as f64);
        }
        if self.gpu.rebind {
            self.data_group = None;
            self.gpu.rebind = false;
        }
        self.frames += 1;
        // The histogram, every so often, when a readback buffer is free.
        self.pending = None;
        let every = u64::from(self.settings.histogram_every);
        if every > 0 && self.frames.is_multiple_of(every) {
            self.pending =
                (0..RING).find(|&i| self.readback_state[i].load(Ordering::Acquire) == IDLE);
        }
        let debug = self.debug_pixels >= (targets.size.0 * targets.size.1) as usize;
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::cast_slice(&frame.uniforms));
        let u = self.uniform_words(frame, targets.size, debug);
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&u));
        if self.data_group.is_none() {
            let mut entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniforms.as_entire_binding(),
            }];
            for b in 0..6 {
                entries.push(wgpu::BindGroupEntry {
                    binding: b as u32 + 1,
                    resource: self.gpu.buffer(b).as_entire_binding(),
                });
            }
            entries.push(wgpu::BindGroupEntry {
                binding: 7,
                resource: self.stats.as_entire_binding(),
            });
            self.data_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("march data"),
                layout: &self.data_layout,
                entries: &entries,
            }));
        }
        if self.pending.is_some() || debug {
            encoder.clear_buffer(&self.stats, 0, Some(HIST_WORDS as u64 * 4));
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("march"),
                timestamp_writes: timestamps,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_group, &[]);
            pass.set_bind_group(1, self.data_group.as_ref(), &[]);
            pass.set_bind_group(2, &targets.out_group, &[]);
            let g = wgsl::WORKGROUP;
            pass.dispatch_workgroups(targets.size.0.div_ceil(g), targets.size.1.div_ceil(g), 1);
        }
        if let Some(i) = self.pending {
            encoder.copy_buffer_to_buffer(
                &self.stats,
                0,
                &self.readback[i],
                0,
                HIST_WORDS as u64 * 4,
            );
        }
    }

    /// The shared composite (fog, tone mapping, the sky behind) of `targets` into `view`
    /// (`Rgba8Unorm`).
    pub fn composite(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &MarchTargets,
        view: &wgpu::TextureView,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("march composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: timestamps,
            ..Default::default()
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.frame_group, &[]);
        pass.set_bind_group(2, &targets.scene, &[]);
        pass.draw(0..3, 0..1);
    }

    /// After the frame's submit: maps the histogram it copied, if any.
    pub fn submitted(&mut self) {
        let Some(i) = self.pending.take() else {
            return;
        };
        let state = self.readback_state[i].clone();
        state.store(IN_FLIGHT, Ordering::Release);
        self.readback[i].map_async(wgpu::MapMode::Read, .., move |r| {
            state.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
    }

    /// Histograms the GPU has finished: summarised into telemetry (`march.steps.*`).
    pub fn collect(&mut self) -> Option<StepSummary> {
        let mut out = None;
        for i in 0..RING {
            match self.readback_state[i].load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(view) = self.readback[i].get_mapped_range(..) {
                        let words: Vec<u32> = bytemuck::cast_slice(&view).to_vec();
                        drop(view);
                        let s =
                            stats::summarize(&words, self.settings.step_cap.min(stats::MAX_CAP));
                        for (name, v) in stats::telemetry_samples(&s) {
                            crate::telemetry::sample(name, v);
                        }
                        out = Some(s);
                    }
                    self.readback[i].unmap();
                    self.readback_state[i].store(IDLE, Ordering::Release);
                }
                FAILED => self.readback_state[i].store(IDLE, Ordering::Release),
                _ => {}
            }
        }
        if out.is_some() {
            self.last_steps = out;
        }
        out
    }

    /// GPU bytes: the brick structure's buffers.
    pub fn gpu_bytes(&self) -> u64 {
        self.gpu.gpu_bytes()
    }
}

impl Drop for Marcher {
    fn drop(&mut self) {
        self.frame_buffer.destroy();
        self.uniforms.destroy();
        self.stats.destroy();
        for b in &self.readback {
            b.destroy();
        }
    }
}
