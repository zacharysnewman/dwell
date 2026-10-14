//! Dwell's wgpu renderer (ARCHITECTURE.md §5, ADR 0028; WebGPU only): terrain chunks, LOD sections
//! with their skirts and stand-ins, players, the block outline, debug lines, the sky, height fog
//! and tone mapping, in two passes split at LOD_NEAR_SPLIT_M (§6.6). Everything is drawn relative
//! to the camera (origins in f64, offsets in f32), so the world stays precise far from the origin.
//! It is the Three.js renderer's interface (render/Renderer.ts) in Rust.

pub mod camera;
pub mod display;
pub mod geometry;
pub mod shaders;

use std::collections::HashMap;

use dwell_look::fog::{DEFAULT_FOG, FogSettings, fog_sigma};
use dwell_look::sky_frame::{SkyFrame, sky_rest};
use dwell_look::{DEFAULT_EXPOSURE, HORIZON_COLOR, LIGHT, hex, sanitize_exposure, srgb_to_linear};
use dwell_mesh::{ChunkMeshes, FlatMesh, MeshArrays, SectionMeshes};
use dwell_world::{CHUNK_SIZE, ChunkCoord, chunk_of};
use wgpu::util::DeviceExt;

use camera::{Basis, V3, aim, mul, perspective, vertical_fov, view_matrix};
pub use display::RenderStats;

/// Beyond this the far pass draws (§6.6); the near pass's far plane.
pub const NEAR_SPLIT: f64 = dwell_protocol::lod::NEAR_SPLIT_M as f64;
pub const NEAR_PLANE: f64 = 0.05;
pub const FAR_PLANE: f64 = 5e7;
pub const SAMPLES: u32 = 4;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const WATER_OPACITY: f32 = 0.55;
/// Draw uniforms live at this stride (WebGPU's minimum uniform offset alignment).
const DRAW_STRIDE: u64 = 256;
const MAX_STAND_INS: usize = 64;
/// Debug tints of LOD levels (setLodLevelColors), sRGB.
const LEVEL_TINTS: [u32; 20] = [
    0xffffff, 0xff6060, 0xffb060, 0xffff60, 0x80ff60, 0x60ffd0, 0x60a0ff, 0x9060ff, 0xff60e0,
    0xff8080, 0xffd080, 0xffff90, 0xa0ff90, 0x90ffe0, 0x90c0ff, 0xb090ff, 0xff90f0, 0xc0c0c0,
    0x808080, 0x404040,
];

/// A player drawn by the renderer (remote players, and the local one when dead).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerView {
    /// Feet position.
    pub feet: V3,
    /// The face the player stands on: −1 draws it upside down.
    pub face: f64,
    /// Degrees; 0 = +Z.
    pub yaw: f64,
    pub crouched: bool,
    /// Lying on the ground (cosmetic death pose).
    pub dead: bool,
    pub color: u32,
    pub radius: f32,
    pub height: f32,
}

/// A debug line segment, sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub from: V3,
    pub to: V3,
    pub color: u32,
}

/// An ancestor section drawn clipped to a box (world metres) where the traversal has nothing ready.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandIn {
    pub id: i64,
    pub lo: V3,
    pub hi: V3,
}

struct GpuMesh {
    vertex: Vec<wgpu::Buffer>,
    index: wgpu::Buffer,
    /// Index ranges: [0] the surface; for LOD sections, [1 + face] each side's skirt.
    parts: Vec<std::ops::Range<u32>>,
    bytes: u64,
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    data: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: data,
        usage,
    })
}
fn floats(v: &[f32]) -> &[u8] {
    bytemuck_cast(v)
}
fn u32s(v: &[u32]) -> &[u8] {
    bytemuck_cast(v)
}
/// Plain-old-data as bytes (f32 and u32 slices only; `unsafe` is forbidden, so this copies through
/// a safe path when the platform's byte order is the GPU's, little-endian everywhere we run).
fn bytemuck_cast<T: Copy + ToLeBytes>(v: &[T]) -> &[u8] {
    T::as_bytes(v)
}
trait ToLeBytes: Sized {
    fn as_bytes(v: &[Self]) -> &[u8];
}
impl ToLeBytes for f32 {
    fn as_bytes(v: &[Self]) -> &[u8] {
        bytemuck::cast_slice(v)
    }
}
impl ToLeBytes for u32 {
    fn as_bytes(v: &[Self]) -> &[u8] {
        bytemuck::cast_slice(v)
    }
}

impl GpuMesh {
    fn textured(device: &wgpu::Device, m: &MeshArrays) -> Option<Self> {
        if m.indices.is_empty() {
            return None;
        }
        let v = wgpu::BufferUsages::VERTEX;
        Some(Self {
            vertex: [&m.positions, &m.normals, &m.colors, &m.uvs, &m.tiles]
                .iter()
                .map(|a| buffer(device, "chunk", floats(a), v))
                .collect(),
            index: buffer(device, "chunk", u32s(&m.indices), wgpu::BufferUsages::INDEX),
            parts: std::iter::once(0..m.indices.len() as u32).collect(),
            bytes: m.byte_len() as u64,
        })
    }
    /// Flat meshes concatenated into one (an LOD section's surface and its six skirts).
    fn flat(device: &wgpu::Device, meshes: &[&FlatMesh]) -> Option<Self> {
        if meshes.iter().all(|m| m.indices.is_empty()) {
            return None;
        }
        let (mut p, mut n, mut c, mut idx, mut parts) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for m in meshes {
            let base = (p.len() / 3) as u32;
            let start = idx.len() as u32;
            p.extend_from_slice(&m.positions);
            n.extend_from_slice(&m.normals);
            c.extend_from_slice(&m.colors);
            idx.extend(m.indices.iter().map(|i| i + base));
            parts.push(start..idx.len() as u32);
        }
        let v = wgpu::BufferUsages::VERTEX;
        Some(Self {
            vertex: vec![
                buffer(device, "flat", floats(&p), v),
                buffer(device, "flat", floats(&n), v),
                buffer(device, "flat", floats(&c), v),
            ],
            index: buffer(device, "flat", u32s(&idx), wgpu::BufferUsages::INDEX),
            bytes: 4 * (p.len() + n.len() + c.len() + idx.len()) as u64,
            parts,
        })
    }
}

struct Chunk {
    origin: V3,
    opaque: Option<GpuMesh>,
    water: Option<GpuMesh>,
}

struct Section {
    origin: V3,
    cell: f64,
    level: usize,
    opaque: Option<GpuMesh>,
    water: Option<GpuMesh>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Terrain,
    Flat,
    Line,
}

/// One draw, collected before the passes record.
struct DrawItem {
    kind: Kind,
    water: bool,
    no_depth: bool,
    offset: [f32; 4],
    clip: Option<([f32; 3], [f32; 3])>,
    tint: [f32; 4],
    mesh: DrawMesh,
}

enum DrawMesh {
    Chunk(ChunkCoord, bool),
    Section(i64, bool, u8),
    Capsule(u64),
    Outline,
    Debug,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    pixel_ratio: f64,
    targets: Option<(wgpu::TextureView, wgpu::TextureView)>,
    draw_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<(Kind, bool, bool), wgpu::RenderPipeline>,
    sky_pipeline: wgpu::RenderPipeline,
    frame_buffers: [wgpu::Buffer; 2],
    frame_groups: [wgpu::BindGroup; 2],
    draw_buffer: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    draw_capacity: u64,
    chunks: HashMap<ChunkCoord, Chunk>,
    sections: HashMap<i64, Section>,
    shown: Vec<(i64, u8)>,
    stand_ins: Vec<StandIn>,
    level_colors: bool,
    players: HashMap<u32, PlayerView>,
    capsules: HashMap<u64, GpuMesh>,
    outline: Option<(V3, f64)>,
    outline_mesh: GpuMesh,
    debug: Option<GpuMesh>,
    eye: V3,
    yaw: f64,
    pitch: f64,
    face: f64,
    flip: f64,
    fog: FogSettings,
    exposure: f64,
    sky: SkyFrame,
    stats: RenderStats,
}

fn linear(c: u32) -> [f32; 3] {
    hex(c).map(|v| srgb_to_linear(v) as f32)
}

fn player_key(radius: f32, height: f32) -> u64 {
    (u64::from(radius.to_bits()) << 32) | u64::from(height.to_bits())
}

impl Renderer {
    /// A renderer drawing into `format` targets (a canvas's, or an offscreen texture's).
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dwell"),
            source: wgpu::ShaderSource::Wgsl(shaders::source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
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
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dwell"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        let attrs = |locations: &[(u32, u32)]| -> Vec<(u64, [wgpu::VertexAttribute; 1])> {
            locations
                .iter()
                .map(|&(location, n)| {
                    let format = match n {
                        2 => wgpu::VertexFormat::Float32x2,
                        3 => wgpu::VertexFormat::Float32x3,
                        _ => wgpu::VertexFormat::Float32x4,
                    };
                    (
                        u64::from(n) * 4,
                        [wgpu::VertexAttribute {
                            format,
                            offset: 0,
                            shader_location: location,
                        }],
                    )
                })
                .collect()
        };
        let terrain_attrs = attrs(&[(0, 3), (1, 3), (2, 3), (3, 2), (4, 4)]);
        let flat_attrs = attrs(&[(0, 3), (1, 3), (2, 3)]);
        let line_attrs = attrs(&[(0, 3), (1, 3)]);
        let mut pipelines = HashMap::new();
        for kind in [Kind::Terrain, Kind::Flat, Kind::Line] {
            for water in [false, true] {
                for no_depth in [false, true] {
                    let (vs, fs, a) = match kind {
                        Kind::Terrain => ("terrain_vs", "terrain_fs", &terrain_attrs),
                        Kind::Flat => ("flat_vs", "flat_fs", &flat_attrs),
                        Kind::Line => ("line_vs", "line_fs", &line_attrs),
                    };
                    let buffers: Vec<Option<wgpu::VertexBufferLayout>> = a
                        .iter()
                        .map(|(stride, attr)| {
                            Some(wgpu::VertexBufferLayout {
                                array_stride: *stride,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: attr,
                            })
                        })
                        .collect();
                    let blend =
                        (water || kind == Kind::Line).then_some(wgpu::BlendState::ALPHA_BLENDING);
                    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some(vs),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some(vs),
                            compilation_options: Default::default(),
                            buffers: &buffers,
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some(fs),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: wgpu::PrimitiveState {
                            topology: if kind == Kind::Line {
                                wgpu::PrimitiveTopology::LineList
                            } else {
                                wgpu::PrimitiveTopology::TriangleList
                            },
                            front_face: wgpu::FrontFace::Ccw,
                            // Water is seen from both sides; opaque faces are culled from behind.
                            cull_mode: (!water && kind != Kind::Line).then_some(wgpu::Face::Back),
                            ..Default::default()
                        },
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH,
                            depth_write_enabled: Some(!water && kind != Kind::Line),
                            depth_compare: Some(if no_depth {
                                wgpu::CompareFunction::Always
                            } else {
                                wgpu::CompareFunction::LessEqual
                            }),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: wgpu::MultisampleState {
                            count: SAMPLES,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    });
                    pipelines.insert((kind, water, no_depth), pipeline);
                }
            }
        }
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("sky_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("sky_fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let (atlas_view, sampler) = Self::atlas(&device, &queue);
        let frame_buffers = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame"),
                size: FRAME_FLOATS as u64 * 4,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let frame_groups = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame"),
                layout: &frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame_buffers[i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        });
        let (draw_buffer, draw_group) = Self::draw_buffer(&device, &draw_layout, 1024);
        let edges = geometry::box_edges();
        let outline_mesh = Self::lines_mesh(
            &device,
            &edges
                .iter()
                .map(|p| (*p, [1.0, 1.0, 1.0]))
                .collect::<Vec<_>>(),
        );
        Self {
            device,
            queue,
            format,
            size: (1, 1),
            pixel_ratio: 1.0,
            targets: None,
            draw_layout,
            pipelines,
            sky_pipeline,
            frame_buffers,
            frame_groups,
            draw_buffer,
            draw_group,
            draw_capacity: 1024,
            chunks: HashMap::new(),
            sections: HashMap::new(),
            shown: Vec::new(),
            stand_ins: Vec::new(),
            level_colors: false,
            players: HashMap::new(),
            capsules: HashMap::new(),
            outline: None,
            outline_mesh,
            debug: None,
            eye: [0.0, 6.0, 14.0],
            yaw: 180.0,
            pitch: -20.0,
            face: 1.0,
            flip: 0.0,
            fog: DEFAULT_FOG,
            exposure: DEFAULT_EXPOSURE,
            sky: sky_rest(),
            stats: RenderStats::default(),
        }
    }

    /// Block textures: the tiled-noise atlas, mipmapped (averaged in linear light), sampled crisp
    /// up close and filtered far away.
    fn atlas(device: &wgpu::Device, queue: &wgpu::Queue) -> (wgpu::TextureView, wgpu::Sampler) {
        let size = dwell_mesh::ATLAS_SIZE as u32;
        let levels = 32 - size.leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut level = dwell_mesh::build_atlas();
        let mut s = size;
        for mip in 0..levels {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &level,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * s),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: s,
                    height: s,
                    depth_or_array_layers: 1,
                },
            );
            if s > 1 {
                level = downsample(&level, s as usize);
                s /= 2;
            }
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        (texture.create_view(&Default::default()), sampler)
    }

    fn draw_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draws"),
            size: slots * DRAW_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draws"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });
        (buffer, group)
    }

    fn lines_mesh(device: &wgpu::Device, points: &[([f32; 3], [f32; 3])]) -> GpuMesh {
        let p: Vec<f32> = points.iter().flat_map(|(p, _)| *p).collect();
        let c: Vec<f32> = points.iter().flat_map(|(_, c)| *c).collect();
        let idx: Vec<u32> = (0..points.len() as u32).collect();
        let v = wgpu::BufferUsages::VERTEX;
        GpuMesh {
            vertex: vec![
                buffer(device, "lines", floats(&p), v),
                buffer(device, "lines", floats(&c), v),
            ],
            index: buffer(device, "lines", u32s(&idx), wgpu::BufferUsages::INDEX),
            parts: std::iter::once(0..idx.len() as u32).collect(),
            bytes: 4 * (p.len() + c.len() + idx.len()) as u64,
        }
    }

    /// Resizes the drawing buffer to CSS pixels × device pixel ratio.
    pub fn resize(&mut self, width: u32, height: u32, pixel_ratio: f64) {
        self.size = (width.max(1), height.max(1));
        self.pixel_ratio = pixel_ratio;
        self.targets = None;
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Replaces (or, with `None`, removes) a terrain chunk; meshes are chunk-local.
    pub fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        self.chunks.remove(&coord);
        let Some(m) = meshes else { return };
        let (opaque, water) = (
            GpuMesh::textured(&self.device, &m.opaque),
            GpuMesh::textured(&self.device, &m.transparent),
        );
        if opaque.is_none() && water.is_none() {
            return;
        }
        let origin = [coord.x, coord.y, coord.z].map(|c| f64::from(c * CHUNK_SIZE));
        self.chunks.insert(
            coord,
            Chunk {
                origin,
                opaque,
                water,
            },
        );
    }

    /// LOD sections (§6.6): adds, replaces or removes a section's meshes (in cells from `origin`,
    /// scaled by `cell_size`). Nothing shows until `show_lod_sections` lists it.
    pub fn set_lod_section(
        &mut self,
        id: i64,
        origin: V3,
        cell_size: f64,
        meshes: Option<&SectionMeshes>,
    ) {
        self.sections.remove(&id);
        let Some(m) = meshes else { return };
        let mut parts: Vec<&FlatMesh> = vec![&m.opaque];
        parts.extend(m.skirts.iter());
        let opaque = GpuMesh::flat(&self.device, &parts);
        let water = GpuMesh::flat(&self.device, &[&m.water]);
        let level = cell_size.log2().round().max(0.0) as usize;
        self.sections.insert(
            id,
            Section {
                origin,
                cell: cell_size,
                level,
                opaque,
                water,
            },
        );
    }
    /// The sections to draw from now on, each with the sides (bit per face) whose skirt shows.
    pub fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        self.shown = visible.to_vec();
    }
    pub fn show_lod_stand_ins(&mut self, stand_ins: &[StandIn]) {
        self.stand_ins = stand_ins.to_vec();
    }
    pub fn set_lod_level_colors(&mut self, on: bool) {
        self.level_colors = on;
    }
    pub fn set_sky_frame(&mut self, frame: SkyFrame) {
        self.sky = frame;
    }
    pub fn set_fog(&mut self, fog: FogSettings) {
        self.fog = fog;
    }
    pub fn set_exposure(&mut self, exposure: f64) {
        self.exposure = sanitize_exposure(Some(exposure));
    }
    /// Outlines the targeted block: its min corner and box height, or none.
    pub fn set_block_outline(&mut self, cell: Option<V3>, height: f64) {
        self.outline = cell.map(|c| (c, height));
    }
    pub fn set_player(&mut self, id: u32, view: Option<PlayerView>) {
        match view {
            Some(v) => {
                let height = if v.crouched { v.height / 2.0 } else { v.height };
                let key = player_key(v.radius, height);
                self.capsules.entry(key).or_insert_with(|| {
                    let g = geometry::capsule(v.radius, height, 6, 12);
                    let m = FlatMesh {
                        positions: g.positions,
                        normals: g.normals,
                        colors: g.colors,
                        indices: g.indices,
                    };
                    GpuMesh::flat(&self.device, &[&m]).expect("a capsule has faces")
                });
                self.players.insert(id, v);
            }
            None => {
                self.players.remove(&id);
            }
        }
    }
    /// Places the camera at `eye`, looking along yaw/pitch (degrees) for `face`; `flip` turns the
    /// view over about its right axis across the midplane.
    pub fn set_camera(&mut self, eye: V3, yaw_deg: f64, pitch_deg: f64, face: f64, flip: f64) {
        (self.eye, self.yaw, self.pitch, self.face, self.flip) =
            (eye, yaw_deg, pitch_deg, face, flip);
    }
    /// Debug line segments, or `None` to clear.
    pub fn set_debug_lines(&mut self, lines: Option<&[Line]>) {
        self.debug = lines.filter(|l| !l.is_empty()).map(|lines| {
            let pts: Vec<([f32; 3], [f32; 3])> = lines
                .iter()
                .flat_map(|l| {
                    let c = hex(l.color).map(|v| v as f32);
                    let rel = |p: V3| {
                        [
                            (p[0] - self.eye[0]) as f32,
                            (p[1] - self.eye[1]) as f32,
                            (p[2] - self.eye[2]) as f32,
                        ]
                    };
                    [(rel(l.from), c), (rel(l.to), c)]
                })
                .collect();
            Self::lines_mesh(&self.device, &pts)
        });
    }
    pub fn stats(&self) -> RenderStats {
        let pixels = u64::from(self.size.0) * u64::from(self.size.1);
        let mesh_bytes = self
            .chunks
            .values()
            .flat_map(|c| [&c.opaque, &c.water])
            .chain(self.sections.values().flat_map(|s| [&s.opaque, &s.water]))
            .flatten()
            .map(|m| m.bytes)
            .sum();
        RenderStats {
            mesh_bytes,
            screen_bytes: pixels * (4 * 8 + 8),
            pixel_ratio: self.pixel_ratio,
            ..self.stats
        }
    }

    fn rel(&self, p: V3) -> [f32; 3] {
        [
            (p[0] - self.eye[0]) as f32,
            (p[1] - self.eye[1]) as f32,
            (p[2] - self.eye[2]) as f32,
        ]
    }

    fn frame_uniforms(&self, basis: &Basis, near: f64, far: f64) -> [f32; FRAME_FLOATS] {
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        let fov = vertical_fov(aspect);
        let vp = mul(&perspective(fov, aspect, near, far), &view_matrix(basis));
        let mut u = [0.0f32; FRAME_FLOATS];
        let mut i = 0;
        let mut put = |v: &[f32]| {
            u[i..i + v.len()].copy_from_slice(v);
            i += 4 * v.len().div_ceil(4);
        };
        for col in &vp {
            put(col);
        }
        put(&[
            self.eye[1] as f32,
            self.face as f32,
            self.exposure as f32,
            self.fog.density as f32,
        ]);
        put(&[
            fog_sigma(self.fog.distance_m) as f32,
            self.fog.height_m as f32,
            0.0,
            0.0,
        ]);
        let f3 = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
        put(&f3(self.sky.day_pole));
        put(&f3(self.sky.sun));
        put(&f3(self.sky.moon));
        let lit = |c: u32, k: f64| linear(c).map(|v| v * k as f32);
        put(&lit(LIGHT.sun, LIGHT.sun_intensity));
        put(&lit(LIGHT.moon, LIGHT.moon_intensity));
        put(&lit(LIGHT.hemisphere_sky, LIGHT.hemisphere_intensity));
        put(&lit(LIGHT.hemisphere_ground, LIGHT.hemisphere_intensity));
        put(&lit(
            LIGHT.moon_hemisphere_sky,
            LIGHT.moon_hemisphere_intensity,
        ));
        put(&lit(
            LIGHT.moon_hemisphere_ground,
            LIGHT.moon_hemisphere_intensity,
        ));
        put(&f3(basis.right));
        put(&f3(basis.up));
        put(&f3(basis.forward()));
        let tan_y = (fov.to_radians() / 2.0).tan();
        put(&[(tan_y * aspect) as f32, tan_y as f32, 0.0, 0.0]);
        u
    }

    /// The draws of one pass: everything whose distance range meets [near, far].
    fn collect(
        &self,
        near: f64,
        far: f64,
        chunk_visible: &dyn Fn(ChunkCoord) -> bool,
    ) -> Vec<DrawItem> {
        let mut out = Vec::new();
        let plain = [1.0, 1.0, 1.0, 1.0];
        let water = [1.0, 1.0, 1.0, WATER_OPACITY];
        let reaches = |origin: V3, size: f64| {
            // The distance from the eye to the box [origin, origin + size]³ against the pass's range.
            let d2: f64 = (0..3)
                .map(|k| {
                    let lo = origin[k] - self.eye[k];
                    let d = if lo > 0.0 {
                        lo
                    } else if lo + size < 0.0 {
                        -(lo + size)
                    } else {
                        0.0
                    };
                    d * d
                })
                .sum();
            let far_corner: f64 = (0..3)
                .map(|k| {
                    (origin[k] - self.eye[k])
                        .abs()
                        .max((origin[k] + size - self.eye[k]).abs())
                        .powi(2)
                })
                .sum();
            d2.sqrt() <= far && far_corner.sqrt() >= near
        };
        for (&coord, c) in &self.chunks {
            if !chunk_visible(coord) || !reaches(c.origin, f64::from(CHUNK_SIZE)) {
                continue;
            }
            let r = self.rel(c.origin);
            let offset = [r[0], r[1], r[2], 1.0];
            if c.opaque.is_some() {
                out.push(DrawItem {
                    kind: Kind::Terrain,
                    water: false,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: plain,
                    mesh: DrawMesh::Chunk(coord, false),
                });
            }
            if c.water.is_some() {
                out.push(DrawItem {
                    kind: Kind::Terrain,
                    water: true,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: water,
                    mesh: DrawMesh::Chunk(coord, true),
                });
            }
        }
        let level_tint = |s: &Section, alpha: f32| {
            let t = if self.level_colors {
                linear(LEVEL_TINTS[s.level.min(LEVEL_TINTS.len() - 1)])
            } else {
                [1.0; 3]
            };
            [t[0], t[1], t[2], alpha]
        };
        for &(id, mask) in &self.shown {
            let Some(s) = self.sections.get(&id) else {
                continue;
            };
            if !reaches(s.origin, s.cell * 32.0) {
                continue;
            }
            let r = self.rel(s.origin);
            let offset = [r[0], r[1], r[2], s.cell as f32];
            if s.opaque.is_some() {
                out.push(DrawItem {
                    kind: Kind::Flat,
                    water: false,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: level_tint(s, 1.0),
                    mesh: DrawMesh::Section(id, false, mask),
                });
            }
            if s.water.is_some() {
                out.push(DrawItem {
                    kind: Kind::Flat,
                    water: true,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: level_tint(s, WATER_OPACITY),
                    mesh: DrawMesh::Section(id, true, 0),
                });
            }
        }
        // Stand-ins: the source section's surface, clipped to the hole's box (its own skirts off).
        for st in self.stand_ins.iter().take(MAX_STAND_INS) {
            let Some(s) = self.sections.get(&st.id).filter(|s| s.opaque.is_some()) else {
                continue;
            };
            let r = self.rel(s.origin);
            let clip = Some((self.rel(st.lo), self.rel(st.hi)));
            out.push(DrawItem {
                kind: Kind::Flat,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], s.cell as f32],
                clip,
                tint: level_tint(s, 1.0),
                mesh: DrawMesh::Section(st.id, false, 0),
            });
        }
        for v in self.players.values() {
            if !reaches(v.feet, 4.0) {
                continue;
            }
            let height = if v.crouched { v.height / 2.0 } else { v.height };
            let color = linear(if v.dead { 0x6b6b6b } else { v.color });
            let r = self.rel(v.feet);
            // Upside down on face B: the capsule hangs from the feet.
            let base = if v.face < 0.0 {
                [r[0], r[1] - height, r[2]]
            } else {
                r
            };
            out.push(DrawItem {
                kind: Kind::Flat,
                water: false,
                no_depth: false,
                offset: [base[0], base[1], base[2], 1.0],
                clip: None,
                tint: [color[0], color[1], color[2], 1.0],
                mesh: DrawMesh::Capsule(player_key(v.radius, height)),
            });
        }
        if let Some((cell, _height)) = self.outline.filter(|(c, _)| reaches(*c, 2.0)) {
            let r = self.rel([cell[0] - 0.002, cell[1] - 0.002, cell[2] - 0.002]);
            let c = hex(0x101418).map(|v| v as f32);
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], 1.0],
                clip: None,
                tint: [c[0], c[1], c[2], 0.8],
                mesh: DrawMesh::Outline,
            });
        }
        if self.debug.is_some() && near < NEAR_PLANE * 2.0 {
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: true,
                offset: [0.0, 0.0, 0.0, 1.0],
                clip: None,
                tint: [1.0, 1.0, 1.0, 1.0],
                mesh: DrawMesh::Debug,
            });
        }
        // Opaque first, then the see-through, then lines; within the see-through, far to near.
        out.sort_by(|a, b| {
            let rank = |d: &DrawItem| u8::from(d.water) + 2 * u8::from(d.kind == Kind::Line);
            let dist = |d: &DrawItem| {
                -(d.offset[0] * d.offset[0] + d.offset[1] * d.offset[1] + d.offset[2] * d.offset[2])
            };
            rank(a).cmp(&rank(b)).then(if a.water && b.water {
                dist(a).total_cmp(&dist(b))
            } else {
                std::cmp::Ordering::Equal
            })
        });
        out
    }

    fn ensure_targets(&mut self) {
        if self.targets.is_some() {
            return;
        }
        let size = wgpu::Extent3d {
            width: self.size.0,
            height: self.size.1,
            depth_or_array_layers: 1,
        };
        let make = |format, label| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        self.targets = Some((make(self.format, "colour"), make(DEPTH, "depth")));
    }

    /// Draws one frame into `target` (a canvas texture or an offscreen one, `format`, the size
    /// given to `resize`).
    /// `chunk_visible` says which terrain chunks to draw (the LOD draws instead of the others).
    pub fn render(
        &mut self,
        target: &wgpu::TextureView,
        chunk_visible: &dyn Fn(ChunkCoord) -> bool,
    ) {
        self.ensure_targets();
        let basis = aim(self.yaw, self.pitch, self.face, self.flip);
        // Two passes (§6.6): the far one beyond the split (its near plane pushed out with altitude,
        // where nothing is closer), then a depth clear and the near one. Heights are face-local.
        let local_y = if self.face > 0.0 {
            self.eye[1]
        } else {
            2.0 * dwell_look::MIDPLANE_Y - self.eye[1]
        };
        let altitude = local_y - f64::from(dwell_world::WORLD_MAX_Y);
        let far_near = (NEAR_SPLIT * 0.95).max(altitude * 0.8);
        let passes = [(far_near, FAR_PLANE), (NEAR_PLANE, NEAR_SPLIT)];
        let draws: [Vec<DrawItem>; 2] =
            std::array::from_fn(|i| self.collect(passes[i].0, passes[i].1, chunk_visible));
        for (i, (near, far)) in passes.iter().enumerate() {
            let u = self.frame_uniforms(&basis, *near, *far);
            self.queue
                .write_buffer(&self.frame_buffers[i], 0, floats(&u));
        }
        // Per-draw uniforms, both passes in one buffer.
        let total = (draws[0].len() + draws[1].len()) as u64;
        if total > self.draw_capacity {
            self.draw_capacity = total.next_power_of_two();
            (self.draw_buffer, self.draw_group) =
                Self::draw_buffer(&self.device, &self.draw_layout, self.draw_capacity);
        }
        let mut data = vec![0.0f32; (total * DRAW_STRIDE / 4) as usize];
        for (k, d) in draws.iter().flatten().enumerate() {
            let at = k * (DRAW_STRIDE / 4) as usize;
            data[at..at + 4].copy_from_slice(&d.offset);
            if let Some((lo, hi)) = d.clip {
                data[at + 4..at + 8].copy_from_slice(&[lo[0], lo[1], lo[2], 1.0]);
                data[at + 8..at + 12].copy_from_slice(&[hi[0], hi[1], hi[2], 0.0]);
            }
            data[at + 12..at + 16].copy_from_slice(&d.tint);
        }
        if !data.is_empty() {
            self.queue.write_buffer(&self.draw_buffer, 0, floats(&data));
        }
        let (colour, depth) = self.targets.as_ref().expect("targets");
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let (mut calls, mut triangles) = (0u32, 0u64);
        let mut slot = 0u64;
        for (i, pass_draws) in draws.iter().enumerate() {
            let horizon = HORIZON_COLOR;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if i == 0 { "far" } else { "near" }),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    depth_slice: None,
                    resolve_target: (i == 1).then_some(target),
                    ops: wgpu::Operations {
                        load: if i == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: horizon[0],
                                g: horizon[1],
                                b: horizon[2],
                                a: 1.0,
                            })
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_groups[i], &[]);
            if i == 0 {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(1, &self.draw_group, &[0]);
                pass.draw(0..3, 0..1);
                calls += 1;
            }
            for d in pass_draws {
                let offset = (slot * DRAW_STRIDE) as u32;
                slot += 1;
                let (mesh, ranges): (&GpuMesh, Vec<std::ops::Range<u32>>) = match &d.mesh {
                    DrawMesh::Chunk(c, w) => {
                        let chunk = &self.chunks[c];
                        let m = if *w {
                            chunk.water.as_ref()
                        } else {
                            chunk.opaque.as_ref()
                        }
                        .expect("collected");
                        (m, vec![m.parts[0].clone()])
                    }
                    DrawMesh::Section(id, w, mask) => {
                        let s = &self.sections[id];
                        let m = if *w {
                            s.water.as_ref()
                        } else {
                            s.opaque.as_ref()
                        }
                        .expect("collected");
                        let mut r = vec![m.parts[0].clone()];
                        r.extend(
                            (0..6)
                                .filter(|f| mask & (1 << f) != 0)
                                .filter_map(|f| m.parts.get(1 + f).cloned()),
                        );
                        (m, r)
                    }
                    DrawMesh::Capsule(key) => {
                        let m = &self.capsules[key];
                        (m, vec![m.parts[0].clone()])
                    }
                    DrawMesh::Outline => {
                        (&self.outline_mesh, vec![self.outline_mesh.parts[0].clone()])
                    }
                    DrawMesh::Debug => {
                        let m = self.debug.as_ref().expect("collected");
                        (m, vec![m.parts[0].clone()])
                    }
                };
                pass.set_pipeline(&self.pipelines[&(d.kind, d.water, d.no_depth)]);
                pass.set_bind_group(1, &self.draw_group, &[offset]);
                for (k, b) in mesh.vertex.iter().enumerate() {
                    pass.set_vertex_buffer(k as u32, b.slice(..));
                }
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                for r in ranges.into_iter().filter(|r| !r.is_empty()) {
                    triangles += u64::from(r.len() as u32 / 3);
                    pass.draw_indexed(r, 0, 0..1);
                    calls += 1;
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        self.stats.calls = calls;
        self.stats.triangles = triangles;
    }

    /// The chunk at a world position (tests and tools).
    pub fn has_chunk_at(&self, x: i32, y: i32, z: i32) -> bool {
        self.chunks.contains_key(&chunk_of(x, y, z))
    }
}

/// Floats in the frame uniform block (`Frame` in shaders.rs): a matrix and 15 vec4s.
const FRAME_FLOATS: usize = 16 + 15 * 4;

/// Halves an RGBA8 sRGB image, averaging in linear light.
fn downsample(src: &[u8], size: usize) -> Vec<u8> {
    let half = (size / 2).max(1);
    let to_linear = |b: u8| srgb_to_linear(f64::from(b) / 255.0);
    let to_srgb = |l: f64| {
        let s = if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut out = vec![0u8; half * half * 4];
    for y in 0..half {
        for x in 0..half {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| {
                    src[((2 * y + dy).min(size - 1) * size + (2 * x + dx).min(size - 1)) * 4 + c]
                };
                let px = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                out[(y * half + x) * 4 + c] = if c == 3 {
                    (px.iter().map(|&p| u32::from(p)).sum::<u32>() / 4) as u8
                } else {
                    to_srgb(px.iter().map(|&p| to_linear(p)).sum::<f64>() / 4.0)
                };
            }
        }
    }
    out
}

impl crate::game::Scene for Renderer {
    fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        Renderer::set_terrain_chunk(self, coord, meshes);
    }
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: V3,
        cell_size: f64,
        meshes: Option<&SectionMeshes>,
    ) {
        Renderer::set_lod_section(self, id, origin, cell_size, meshes);
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        Renderer::show_lod_sections(self, visible);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandIn]) {
        Renderer::show_lod_stand_ins(self, stand_ins);
    }
    fn set_player(&mut self, id: u32, view: Option<PlayerView>) {
        Renderer::set_player(self, id, view);
    }
    fn set_camera(&mut self, eye: V3, yaw_deg: f64, pitch_deg: f64, face: f64, flip: f64) {
        Renderer::set_camera(self, eye, yaw_deg, pitch_deg, face, flip);
    }
    fn set_block_outline(&mut self, cell: Option<V3>, height: f64) {
        Renderer::set_block_outline(self, cell, height);
    }
    fn set_debug_lines(&mut self, lines: Option<&[Line]>) {
        Renderer::set_debug_lines(self, lines);
    }
    fn stats(&self) -> RenderStats {
        Renderer::stats(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mipmaps_average_in_linear_light() {
        // Black and white pixels average to linear mid-grey (sRGB ~188), not 128.
        let src = vec![
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ];
        let out = downsample(&src, 2);
        assert_eq!(out.len(), 4);
        assert!((186..=190).contains(&out[0]), "{}", out[0]);
        assert_eq!(out[3], 255);
    }

    #[test]
    fn the_frame_block_matches_the_shader_struct() {
        // mat4x4f + 16 vec4f in `Frame`.
        let fields = shaders::source()
            .split("struct Frame {")
            .nth(1)
            .unwrap()
            .split("};")
            .next()
            .unwrap()
            .matches("vec4f,")
            .count();
        assert_eq!(16 + 4 * fields, FRAME_FLOATS);
    }
}
