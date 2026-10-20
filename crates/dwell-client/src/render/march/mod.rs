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
pub mod upscale;
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
    dwell_march::layout::LEVEL_COUNT * dwell_march::layout::LEVEL_WORDS
        + 48
        + 8 * wgsl::MAX_DIRTY;

pub const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const ID: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// The hit records the shade pass reads (`wgsl::hit_record`).
pub const HIT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// The beam prepass's start distances, one per `dwell_march::BEAM_TILE`² pixels, and whether
/// each came from the temporal start (1) or the beam (0).
pub const BEAM: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
/// Per beam tile, the least distance its rays are known clear to (a hit's, the farthest distance
/// on a miss, 0 at the cap): next frame's temporal start.
pub const CLEAR_T: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// The eye may move this far (metres) between frames and keep the temporal start: with the eye
/// still only (A4b, `MarchSettings::moving` off), or while it moves (A4c; farther than this is a
/// jump, where last frame's image says little).
pub const TEMPORAL_STILL_SHIFT: f64 = 1.0e-3;
pub const TEMPORAL_MAX_SHIFT: f64 = 256.0;

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
    /// §4.3 item 5: the beam prepass gives each tile of pixels a start distance.
    pub beam: bool,
    /// §4.3 item 5: with the eye still, a tile starts from last frame's distances instead
    /// (needs `beam`: it runs in the prepass).
    pub temporal: bool,
    /// A4c: the temporal start while the eye moves too (last frame's distances reprojected from
    /// last frame's eye, outside the nodes that changed since); off, only with the eye still and
    /// no brick uploaded (A4b).
    pub moving: bool,
    /// A4c: when nothing the march depends on changed since the last frame (the same eye, view
    /// and size, no brick uploaded, the same settings), its outputs are kept and only the shade
    /// pass runs (the lighting can change).
    pub reuse: bool,
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
            beam: true,
            temporal: true,
            moving: true,
            reuse: true,
            histogram_every: 30,
            debug: 0,
        }
    }
}

impl MarchSettings {
    /// The page's dev switches for the march (local builds, `bench --compare` configurations'
    /// `params`): `?marchbeam=0` turns the beam prepass off, `?marchtemporal=0` the temporal
    /// start, `?marchmoving=0` the temporal start while the eye moves (A4b's still-eye one),
    /// `?marchreuse=0` keeping an unchanged frame's march.
    pub fn apply_query(&mut self, search: &str) {
        for (k, v) in search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|kv| kv.split_once('='))
        {
            let on = !matches!(v, "0" | "off" | "false");
            match k {
                "marchbeam" => self.beam = on,
                "marchtemporal" => self.temporal = on,
                "marchmoving" => self.moving = on,
                "marchreuse" => self.reuse = on,
                _ => {}
            }
        }
    }
}

/// What the temporal start needs of the last frame drawn.
#[derive(Clone, Debug, PartialEq)]
struct PrevFrame {
    eye: [f64; 3],
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    tan: [f32; 2],
    size: (u32, u32),
    serial: u64,
    /// The settings it was marched with (`MarchSettings::reuse` needs them unchanged).
    settings: MarchSettings,
}

impl PrevFrame {
    fn of(frame: &MarchFrame, size: (u32, u32), serial: u64, settings: MarchSettings) -> Self {
        Self {
            eye: frame.eye,
            forward: frame.forward,
            right: frame.right,
            up: frame.up,
            tan: frame.tan,
            size,
            serial,
            settings,
        }
    }
    /// Whether `frame` at `size` into targets `serial` sees exactly what this frame saw, with
    /// nothing changed in the bricks (`unchanged`): its march's outputs can be kept.
    fn same_view(
        &self,
        frame: &MarchFrame,
        size: (u32, u32),
        serial: u64,
        settings: &MarchSettings,
    ) -> bool {
        let s = &self.settings;
        self.serial == serial
            && self.size == size
            && self.eye == frame.eye
            && self.forward == frame.forward
            && self.right == frame.right
            && self.up == frame.up
            && self.tan == frame.tan
            && (s.step_cap, s.lod_pixels, s.max_t, s.columns, s.distances, s.debug)
                == (
                    settings.step_cap,
                    settings.lod_pixels,
                    settings.max_t,
                    settings.columns,
                    settings.distances,
                    settings.debug,
                )
    }
    /// How far the eye moved since (metres).
    fn shift(&self, frame: &MarchFrame) -> f64 {
        (0..3)
            .map(|k| (frame.eye[k] - self.eye[k]).powi(2))
            .sum::<f64>()
            .sqrt()
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

/// The march's outputs at one size: colour, view depth, id and the hit records the shade pass
/// reads (storage textures, sampled by the composite), and the bind groups over them.
pub struct MarchTargets {
    pub size: (u32, u32),
    pub color: (wgpu::Texture, wgpu::TextureView),
    pub depth: (wgpu::Texture, wgpu::TextureView),
    pub id: (wgpu::Texture, wgpu::TextureView),
    pub hit: (wgpu::Texture, wgpu::TextureView),
    /// The beam prepass's starts (`size` / `BEAM_TILE`, rounded up).
    pub beam: (wgpu::Texture, wgpu::TextureView),
    /// Per beam tile, the distance its rays are known clear to (`CLEAR_T`), read by next frame's
    /// beam prepass, with its min pyramid in the mips (the view: all of them).
    pub clear_t: (wgpu::Texture, wgpu::TextureView),
    /// Per mip after the first, the reduce pass's bind group (the mip below in, this one out)
    /// and the mip's size.
    reduce_groups: Vec<(wgpu::BindGroup, (u32, u32))>,
    /// Tells targets apart (the temporal start reads only the targets the last frame wrote).
    serial: u64,
    out_group: wgpu::BindGroup,
    beam_group: wgpu::BindGroup,
    shade_group: wgpu::BindGroup,
    /// `frame::scene_layout(_, 1)`: what the composite reads.
    pub scene: wgpu::BindGroup,
}

impl Drop for MarchTargets {
    fn drop(&mut self) {
        self.color.0.destroy();
        self.depth.0.destroy();
        self.id.0.destroy();
        self.hit.0.destroy();
        self.beam.0.destroy();
        self.clear_t.0.destroy();
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
    /// The march's workgroup (`wgsl::WORKGROUP` square; profiling variants may change it).
    pub(crate) workgroup: [u32; 2],
    /// Whether the shade pass runs (profiling: the march alone).
    pub(crate) shade: bool,
    /// Whether the march pass runs (profiling: the beam prepass alone, with `shade` off).
    pub(crate) march_pass: bool,
    pipeline: wgpu::ComputePipeline,
    beam_pipeline: wgpu::ComputePipeline,
    shade_pipeline: wgpu::ComputePipeline,
    reduce_pipeline: wgpu::ComputePipeline,
    reduce_layout: wgpu::BindGroupLayout,
    /// Kept to rebuild the pipeline from another source (profiling variants, tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pipeline_layout: wgpu::PipelineLayout,
    composite: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    data_layout: wgpu::BindGroupLayout,
    out_layout: wgpu::BindGroupLayout,
    beam_layout: wgpu::BindGroupLayout,
    shade_layout: wgpu::BindGroupLayout,
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
    /// The last frame drawn (the temporal start's validity), and the targets made so far.
    prev: Option<PrevFrame>,
    target_serial: std::cell::Cell<u64>,
    /// Whether the last draw used the temporal start (telemetry, tests).
    pub temporal_used: bool,
    /// Whether the last draw kept the frame before's march (`MarchSettings::reuse`).
    pub reused: bool,
    /// The last histogram read back.
    pub last_steps: Option<StepSummary>,
    _atlas: (wgpu::TextureView, wgpu::Sampler),
    /// The frame group's tile grids and fluid looks (`render::frame_extras`).
    extras: (wgpu::Buffer, wgpu::Buffer),
    /// The water's tiles the composite samples (water_tiles.rs).
    tiles: super::WaterTiles,
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
                super::frame_extra_entries(vis)[0],
                super::frame_extra_entries(vis)[1],
                // The water's tiles (water_tiles.rs), for the composite's caustics under water.
                super::WaterTiles::layout_entries()[0],
                super::WaterTiles::layout_entries()[1],
                super::WaterTiles::layout_entries()[2],
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
        let sampled = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let out_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march out"),
            entries: &[
                storage_texture(0, DEPTH),
                storage_texture(1, ID),
                storage_texture(2, HIT),
                sampled(3, wgpu::TextureSampleType::Float { filterable: false }),
                storage_texture(4, CLEAR_T),
            ],
        });
        let beam_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march beam"),
            entries: &[
                storage_texture(0, BEAM),
                sampled(1, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let shade_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march shade"),
            entries: &[
                storage_texture(0, COLOR),
                sampled(1, wgpu::TextureSampleType::Uint),
                sampled(2, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march"),
            bind_group_layouts: &[Some(&frame_layout), Some(&data_layout), Some(&out_layout)],
            immediate_size: 0,
        });
        let pipeline = Self::march_pipeline(&device, &layout, &module, "march_main");
        let beam_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march beam"),
            source: wgpu::ShaderSource::Wgsl(wgsl::beam_source().into()),
        });
        let beam_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march beam"),
            bind_group_layouts: &[Some(&frame_layout), Some(&data_layout), Some(&beam_layout)],
            immediate_size: 0,
        });
        let beam_pipeline =
            Self::march_pipeline(&device, &beam_pipeline_layout, &beam_module, "beam_main");
        let shade_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march shade"),
            source: wgpu::ShaderSource::Wgsl(wgsl::shade_source().into()),
        });
        let shade_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("march shade"),
                bind_group_layouts: &[Some(&frame_layout), Some(&data_layout), Some(&shade_layout)],
                immediate_size: 0,
            });
        let shade_pipeline =
            Self::march_pipeline(&device, &shade_pipeline_layout, &shade_module, "shade_main");
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march reduce"),
            entries: &[
                sampled(0, wgpu::TextureSampleType::Float { filterable: false }),
                storage_texture(1, CLEAR_T),
            ],
        });
        let reduce_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march reduce"),
            source: wgpu::ShaderSource::Wgsl(wgsl::reduce_source().into()),
        });
        let reduce_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("march reduce"),
                bind_group_layouts: &[Some(&reduce_layout)],
                immediate_size: 0,
            });
        let reduce_pipeline = Self::march_pipeline(
            &device,
            &reduce_pipeline_layout,
            &reduce_module,
            "reduce_main",
        );
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
        let extras = super::frame_extras(&device, &queue);
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("march frame"),
            size: super::FRAME_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tiles = super::WaterTiles::new(
            &device,
            &raster,
            &frame_buffer,
            &extras.1,
            &dwell_blocks::FLUIDS,
        );
        let [t5, t6, t7] = tiles.group_entries();
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: extras.0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: extras.1.as_entire_binding(),
                },
                t5,
                t6,
                t7,
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
            workgroup: [wgsl::WORKGROUP; 2],
            shade: true,
            march_pass: true,
            pipeline,
            beam_pipeline,
            shade_pipeline,
            reduce_pipeline,
            reduce_layout,
            pipeline_layout: layout,
            composite,
            frame_layout,
            data_layout,
            out_layout,
            beam_layout,
            shade_layout,
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
            prev: None,
            target_serial: std::cell::Cell::new(0),
            temporal_used: false,
            reused: false,
            last_steps: None,
            _atlas: atlas,
            extras,
            tiles,
        }
    }

    fn march_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule,
        entry: &str,
    ) -> wgpu::ComputePipeline {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(layout),
            module,
            entry_point: Some(entry),
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
        self.pipeline =
            Self::march_pipeline(&self.device, &self.pipeline_layout, &module, "march_main");
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
        let make_mips = |size: (u32, u32), format, label, mips: u32| {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: mips,
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
        let make_at = |size, format, label| make_mips(size, format, label, 1);
        let make = |format, label| make_at(size, format, label);
        let tile = dwell_march::BEAM_TILE;
        let beam = make_at(
            (size.0.div_ceil(tile), size.1.div_ceil(tile)),
            BEAM,
            "march beam",
        );
        let tiles = (size.0.div_ceil(tile).max(1), size.1.div_ceil(tile).max(1));
        let mips = 32 - tiles.0.max(tiles.1).leading_zeros();
        let clear_t = make_mips(tiles, CLEAR_T, "march clear t", mips);
        let mip_views: Vec<wgpu::TextureView> = (0..mips)
            .map(|m| {
                clear_t.0.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: m,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let serial = self.target_serial.get() + 1;
        self.target_serial.set(serial);
        let color = make(COLOR, "march colour");
        let depth = make(DEPTH, "march depth");
        let id = make(ID, "march id");
        let hit = make(HIT, "march hit");
        fn view(b: u32, v: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding: b,
                resource: wgpu::BindingResource::TextureView(v),
            }
        }
        let out_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march out"),
            layout: &self.out_layout,
            entries: &[
                view(0, &depth.1),
                view(1, &id.1),
                view(2, &hit.1),
                view(3, &beam.1),
                view(4, &mip_views[0]),
            ],
        });
        let reduce_groups = (1..mips as usize)
            .map(|m| {
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("march reduce"),
                    layout: &self.reduce_layout,
                    entries: &[view(0, &mip_views[m - 1]), view(1, &mip_views[m])],
                });
                ((group), ((tiles.0 >> m).max(1), (tiles.1 >> m).max(1)))
            })
            .collect();
        let beam_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march beam"),
            layout: &self.beam_layout,
            entries: &[view(0, &beam.1), view(1, &clear_t.1)],
        });
        let shade_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march shade"),
            layout: &self.shade_layout,
            entries: &[view(0, &color.1), view(1, &hit.1), view(2, &depth.1)],
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
            hit,
            beam,
            clear_t,
            reduce_groups,
            serial,
            out_group,
            beam_group,
            shade_group,
            scene,
        }
    }

    /// The uniforms' words for a frame at `size`.
    pub fn uniform_words(&self, frame: &MarchFrame, size: (u32, u32), debug: bool) -> Vec<u32> {
        self.uniform_words_with(frame, size, debug, None)
    }

    /// The uniforms, with last frame's camera when the temporal start may use it.
    fn uniform_words_with(
        &self,
        frame: &MarchFrame,
        size: (u32, u32),
        debug: bool,
        prev: Option<&PrevFrame>,
    ) -> Vec<u32> {
        let s = &self.gpu.scene;
        let mut w = s.level_words();
        // Each level's band end (dwell_march::band_end), in the level's spare word.
        let params = self.reference_params(frame, size);
        for l in 0..dwell_march::layout::LEVEL_COUNT {
            w[l * dwell_march::layout::LEVEL_WORDS + 10] =
                dwell_march::band_end(l as u32, &params).to_bits();
        }
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
        if st.beam {
            flags |= wgsl::FLAG_BEAM;
        }
        if prev.is_some() {
            flags |= wgsl::FLAG_TEMPORAL;
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
        let (pf, shift) = match prev {
            Some(p) => (p, p.shift(frame)),
            None => (&PrevFrame::of(frame, size, 0, self.settings), 0.0),
        };
        w.extend(pf.forward.map(f));
        w.push(f(shift as f32));
        w.extend(pf.right.map(f));
        w.push(f(pf.tan[0]));
        w.extend(pf.up.map(f));
        w.push(f(pf.tan[1]));
        let delta: [f32; 3] = std::array::from_fn(|k| (frame.eye[k] - pf.eye[k]) as f32);
        w.extend(delta.map(f));
        let changed = &self.gpu.changed;
        w.push(changed.len() as u32);
        for side in 0..2 {
            for k in 0..wgsl::MAX_DIRTY {
                let b = changed
                    .get(k)
                    .map_or([0; 3], |b| if side == 0 { b.0 } else { b.1 });
                w.extend(b.map(i));
                w.push(0);
            }
        }
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
        // The water's tiles, for the composite's caustics under water.
        self.tiles.draw(encoder, true, None);
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
        // Nothing the march depends on changed: keep its outputs, shade them again.
        let reuse = self.settings.reuse
            && self.march_pass
            && !debug
            && bytes == 0
            && self.data_group.is_some()
            && self.prev.as_ref().is_some_and(|p| {
                p.same_view(frame, targets.size, targets.serial, &self.settings)
            });
        self.reused = reuse;
        crate::telemetry::sample("march.reused", f64::from(u8::from(reuse)));
        if reuse {
            self.pending = None;
            let writes = timestamps.map(|w| wgpu::ComputePassTimestampWrites {
                query_set: w.query_set,
                beginning_of_pass_write_index: w.beginning_of_pass_write_index,
                end_of_pass_write_index: w.end_of_pass_write_index,
            });
            self.shade_pass(encoder, targets, writes);
            return;
        }
        // The temporal start: last frame drew these targets with the same field of view and
        // size, from an eye at most this far away; the nodes changed since are named in the
        // uniforms (`GpuScene::changed`), and last frame's distances are not used in them.
        // (A4b's, `moving` off: the eye still and no brick uploaded.)
        let (max_shift, data_ok) = if self.settings.moving {
            (TEMPORAL_MAX_SHIFT, true)
        } else {
            (TEMPORAL_STILL_SHIFT, bytes == 0 && self.data_group.is_some())
        };
        let prev = self.prev.take().filter(|p| {
            self.settings.beam
                && self.settings.temporal
                && data_ok
                && p.serial == targets.serial
                && p.size == targets.size
                && p.tan == frame.tan
                && p.shift(frame) <= max_shift
        });
        self.temporal_used = prev.is_some();
        // Per frame: whether the temporal start applies, and how many changed-node boxes it
        // works around (the bench reads them as `march.temporal` and `march.changed`).
        crate::telemetry::sample("march.temporal", f64::from(u8::from(self.temporal_used)));
        crate::telemetry::sample("march.changed", self.gpu.changed.len() as f64);
        let u = self.uniform_words_with(frame, targets.size, debug, prev.as_ref());
        self.prev = self.march_pass.then(|| {
            PrevFrame::of(frame, targets.size, targets.serial, self.settings)
        });
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
        // The timestamps begin with the beam prepass (or the march) and end with the shade pass.
        // Split into a beginning (the first pass) and an end (the last pass).
        let qs = timestamps.as_ref().map(|w| w.query_set);
        let mut begin = timestamps
            .as_ref()
            .and_then(|w| w.beginning_of_pass_write_index);
        let mut end = timestamps.as_ref().and_then(|w| w.end_of_pass_write_index);
        let writes = |b: Option<u32>, e: Option<u32>| {
            qs.filter(|_| b.is_some() || e.is_some()).map(|query_set| {
                wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: b,
                    end_of_pass_write_index: e,
                }
            })
        };
        if self.settings.beam {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("march beam"),
                timestamp_writes: writes(
                    begin.take(),
                    if self.march_pass { None } else { end.take() },
                ),
            });
            pass.set_pipeline(&self.beam_pipeline);
            pass.set_bind_group(0, &self.frame_group, &[]);
            pass.set_bind_group(1, self.data_group.as_ref(), &[]);
            pass.set_bind_group(2, &targets.beam_group, &[]);
            let tile = dwell_march::BEAM_TILE;
            let g = wgsl::WORKGROUP;
            pass.dispatch_workgroups(
                targets.size.0.div_ceil(tile).div_ceil(g),
                targets.size.1.div_ceil(tile).div_ceil(g),
                1,
            );
        }
        if self.march_pass {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("march"),
                timestamp_writes: writes(begin.take(), if self.shade { None } else { end.take() }),
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_group, &[]);
            pass.set_bind_group(1, self.data_group.as_ref(), &[]);
            pass.set_bind_group(2, &targets.out_group, &[]);
            let [gx, gy] = self.workgroup;
            pass.dispatch_workgroups(targets.size.0.div_ceil(gx), targets.size.1.div_ceil(gy), 1);
        }
        // The tile distances' min pyramid, for next frame's temporal start.
        if self.march_pass && self.settings.beam && self.settings.temporal {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("march reduce"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            let g = wgsl::WORKGROUP;
            for (group, (w, h)) in &targets.reduce_groups {
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(w.div_ceil(g), h.div_ceil(g), 1);
            }
        }
        if self.shade {
            self.shade_pass(encoder, targets, writes(None, end.take()));
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

    /// Lights the march's hit records into the colour target.
    fn shade_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &MarchTargets,
        timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("march shade"),
            timestamp_writes: timestamps,
        });
        pass.set_pipeline(&self.shade_pipeline);
        pass.set_bind_group(0, &self.frame_group, &[]);
        pass.set_bind_group(1, self.data_group.as_ref(), &[]);
        pass.set_bind_group(2, &targets.shade_group, &[]);
        let g = wgsl::WORKGROUP;
        pass.dispatch_workgroups(targets.size.0.div_ceil(g), targets.size.1.div_ceil(g), 1);
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
        self.extras.0.destroy();
        self.extras.1.destroy();
        for b in &self.readback {
            b.destroy();
        }
    }
}
