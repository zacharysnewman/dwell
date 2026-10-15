//! Dwell's wgpu renderer (ARCHITECTURE.md §5, ADR 0028; WebGPU only): terrain chunks, LOD sections
//! with their skirts and stand-ins, players, the block outline, debug lines, the sky, height fog
//! and tone mapping, in two passes split at LOD_NEAR_SPLIT_M (§6.6). Everything is drawn relative
//! to the camera (origins in f64, offsets in f32), so the world stays precise far from the origin.
//! It is the Three.js renderer's interface (render/Renderer.ts) in Rust.

pub mod arena;
pub mod camera;
pub mod display;
pub mod geometry;
pub mod shaders;

use dwell_world::hash::HashMap;

use dwell_look::fog::{DEFAULT_FOG, FogSettings, fog_sigma};
use dwell_look::sky_frame::{SkyFrame, sky_rest};
use dwell_look::{DEFAULT_EXPOSURE, HORIZON_COLOR, LIGHT, hex, sanitize_exposure, srgb_to_linear};
use dwell_mesh::{ChunkMeshes, FlatMesh, MeshArrays, SectionMeshes};
use dwell_world::{CHUNK_SIZE, ChunkCoord, chunk_of};

use arena::{Arena, ArenaRef, Region};
use camera::{Basis, V3, aim, mul, perspective, vertical_fov, view_matrix};
pub use display::RenderStats;

/// Beyond this the far pass draws (§6.6); the near pass's far plane.
pub const NEAR_SPLIT: f64 = dwell_protocol::lod::NEAR_SPLIT_M as f64;
pub const NEAR_PLANE: f64 = 0.05;
pub const FAR_PLANE: f64 = 5e7;
pub const SAMPLES: u32 = 4;

/// Multisampling for an adapter: 4× on a GPU; none on a CPU (software) adapter, where every sample
/// is the main thread's own time (CI's SwiftShader drops to a few frames a second with 4×).
pub fn samples_for(device_type: wgpu::DeviceType) -> u32 {
    if device_type == wgpu::DeviceType::Cpu {
        1
    } else {
        SAMPLES
    }
}
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const WATER_OPACITY: f32 = 0.55;
/// Draw uniforms live at this stride (WebGPU's minimum uniform offset alignment).
/// Bytes per draw in the draws storage array (`Draw` in the shaders: five vec4f).
const DRAW_STRIDE: u64 = 80;
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

/// A mesh in the arena's pages (arena.rs): its vertices with their attributes interleaved (one
/// format per `Kind`) and its indices, drawn from `first_index` with `base_vertex`.
struct GpuMesh {
    region: Region,
    base_vertex: i32,
    first_index: u32,
    /// Index ranges: [0] the surface; for LOD sections, [1 + face] each side's skirt.
    parts: Vec<std::ops::Range<u32>>,
    bytes: u64,
    arena: ArenaRef,
}

/// Bytes per vertex of each `Kind`'s interleaved format (the pipelines' vertex layouts).
const STRIDES: [u64; 3] = [
    4 * (3 + 3 + 3 + 2 + 4), // Terrain: position, normal, colour, uv, tile
    4 * (3 + 3 + 3),         // Flat: position, normal, colour
    4 * (3 + 3),             // Line: position, colour
];

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

/// Index ranges with the empty ones dropped and touching ones joined: a section's surface and the
/// skirts drawn with it are consecutive in its index buffer, so most draw as one call.
fn merge_ranges(ranges: Vec<std::ops::Range<u32>>) -> Vec<std::ops::Range<u32>> {
    let mut out: Vec<std::ops::Range<u32>> = Vec::with_capacity(ranges.len());
    for r in ranges.into_iter().filter(|r| !r.is_empty()) {
        match out.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => out.push(r),
        }
    }
    out
}

/// Streams of `n` floats per vertex, interleaved vertex by vertex (in the pipelines' attribute
/// order, `vertex_layout`).
fn interleave(streams: &[(&[f32], usize)]) -> Vec<f32> {
    let count = streams.first().map_or(0, |(s, n)| s.len() / n);
    let stride: usize = streams.iter().map(|(_, n)| n).sum();
    let mut out = Vec::with_capacity(count * stride);
    for v in 0..count {
        for (s, n) in streams {
            out.extend_from_slice(&s[v * n..(v + 1) * n]);
        }
    }
    out
}

impl GpuMesh {
    fn upload(
        arena: &ArenaRef,
        kind: Kind,
        vertices: &[f32],
        indices: &[u32],
        parts: Vec<std::ops::Range<u32>>,
        bytes: u64,
    ) -> Self {
        let mut a = arena.borrow_mut();
        let region = a.upload(kind as usize, floats(vertices), u32s(indices));
        Self {
            base_vertex: a.base_vertex(&region),
            first_index: (region.index_offset / 4) as u32,
            region,
            parts,
            bytes,
            arena: arena.clone(),
        }
    }

    fn textured(arena: &ArenaRef, m: &MeshArrays) -> Option<Self> {
        if m.indices.is_empty() {
            return None;
        }
        let vertices = interleave(&[
            (&m.positions, 3),
            (&m.normals, 3),
            (&m.colors, 3),
            (&m.uvs, 2),
            (&m.tiles, 4),
        ]);
        Some(Self::upload(
            arena,
            Kind::Terrain,
            &vertices,
            &m.indices,
            std::iter::once(0..m.indices.len() as u32).collect(),
            m.byte_len() as u64,
        ))
    }

    /// Flat meshes concatenated into one (an LOD section's surface and its six skirts).
    fn flat(arena: &ArenaRef, meshes: &[&FlatMesh]) -> Option<Self> {
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
        let bytes = 4 * (p.len() + n.len() + c.len() + idx.len()) as u64;
        let vertices = interleave(&[(&p, 3), (&n, 3), (&c, 3)]);
        Some(Self::upload(
            arena,
            Kind::Flat,
            &vertices,
            &idx,
            parts,
            bytes,
        ))
    }

    fn lines(arena: &ArenaRef, points: &[([f32; 3], [f32; 3])]) -> Self {
        let p: Vec<f32> = points.iter().flat_map(|(p, _)| *p).collect();
        let c: Vec<f32> = points.iter().flat_map(|(_, c)| *c).collect();
        let idx: Vec<u32> = (0..points.len() as u32).collect();
        let bytes = 4 * (p.len() + c.len() + idx.len()) as u64;
        let vertices = interleave(&[(&p, 3), (&c, 3)]);
        Self::upload(
            arena,
            Kind::Line,
            &vertices,
            &idx,
            std::iter::once(0..idx.len() as u32).collect(),
            bytes,
        )
    }
}

/// A mesh's regions go back to its page when it goes (the pages themselves are destroyed with the
/// arena: wgpu's WebGPU backend frees nothing on drop).
impl Drop for GpuMesh {
    fn drop(&mut self) {
        self.arena.borrow_mut().free(&self.region);
    }
}

/// The multisampled colour target (none with one sample: drawn into the frame's target) and the
/// depth buffer, destroyed when the drawing buffer is resized.
struct Targets {
    colour: Option<(wgpu::Texture, wgpu::TextureView)>,
    depth: (wgpu::Texture, wgpu::TextureView),
}

impl Drop for Targets {
    fn drop(&mut self) {
        if let Some((t, _)) = &self.colour {
            t.destroy();
        }
        self.depth.0.destroy();
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
#[derive(Clone)]
struct DrawItem {
    kind: Kind,
    water: bool,
    no_depth: bool,
    offset: [f32; 4],
    clip: Option<([f32; 3], [f32; 3])>,
    tint: [f32; 4],
    /// Turn about +y (cos, sin of the yaw): players face their heading.
    rotation: [f32; 2],
    mesh: DrawMesh,
    /// The arena page the mesh is in (filled in by `collect`): draws from a page bind it once.
    page: usize,
    /// The depth passes it is drawn in (bit per pass).
    passes: u8,
    /// Its place in the draw order (`draw_order`, filled in by `collect`).
    key: u128,
}

/// The draw order as one integer. Opaque first, then the see-through, then lines; the see-through
/// far to near; the opaque by pipeline and page (each bound once, `render`), then near to far, so
/// the GPU's depth test rejects what nearer meshes hide before shading it (their order does not
/// change the picture: depth decides). Distances compare by their f32 bits (non-negative floats
/// order as their bits do).
fn draw_order(d: &DrawItem) -> u128 {
    let rank = u128::from(u8::from(d.water) + 2 * u8::from(d.kind == Kind::Line));
    let dist = (d.offset[0] * d.offset[0] + d.offset[1] * d.offset[1] + d.offset[2] * d.offset[2])
        .to_bits();
    let rest: u128 = if d.kind == Kind::Line {
        0
    } else if d.water {
        u128::from(u32::MAX - dist)
    } else {
        (u128::from(d.kind as u8) << 72)
            | (u128::from(u8::from(d.no_depth)) << 64)
            | ((d.page as u128 & 0xffff_ffff) << 32)
            | u128::from(dist)
    };
    rank << 96 | rest
}

/// No turn.
const NO_ROTATION: [f32; 2] = [1.0, 0.0];

#[derive(Clone)]
enum DrawMesh {
    Chunk(ChunkCoord, bool),
    Section(i64, bool, u8),
    Player(PlayerShape),
    Outline,
    Debug,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    pixel_ratio: f64,
    targets: Option<Targets>,
    samples: u32,
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
    capsules: HashMap<PlayerShape, GpuMesh>,
    outline: Option<(V3, f64)>,
    outline_mesh: GpuMesh,
    debug: Option<GpuMesh>,
    /// The GPU pages every mesh lives in (arena.rs).
    arena: ArenaRef,
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

/// What a player's mesh depends on: radius and height (bits), dead, colour, face B.
type PlayerShape = (u32, u32, bool, u32, bool);

fn player_shape(v: &PlayerView) -> PlayerShape {
    let height = if v.crouched { v.height / 2.0 } else { v.height };
    let color = if v.dead { DEAD_COLOR } else { v.color };
    (
        v.radius.to_bits(),
        height.to_bits(),
        v.dead,
        color,
        v.face < 0.0,
    )
}

/// Dead players are grey (and lie down, geometry::player_mesh).
const DEAD_COLOR: u32 = 0x6b6b6b;

impl Renderer {
    /// A renderer drawing into `format` targets (a canvas's, or an offscreen texture's).
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::with_samples(device, queue, format, SAMPLES)
    }

    /// A renderer with `samples` per pixel (1 or 4, `samples_for`).
    pub fn with_samples(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
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
                // Each atlas tile's grid lines (dwell_mesh::tile_grids), written once.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(80),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dwell"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        // One interleaved buffer per mesh (`interleave`): (stride, attributes at their offsets).
        let attrs = |locations: &[(u32, u32)]| -> (u64, Vec<wgpu::VertexAttribute>) {
            let mut offset = 0u64;
            let attributes = locations
                .iter()
                .map(|&(location, n)| {
                    let format = match n {
                        2 => wgpu::VertexFormat::Float32x2,
                        3 => wgpu::VertexFormat::Float32x3,
                        _ => wgpu::VertexFormat::Float32x4,
                    };
                    let a = wgpu::VertexAttribute {
                        format,
                        offset,
                        shader_location: location,
                    };
                    offset += u64::from(n) * 4;
                    a
                })
                .collect();
            (offset, attributes)
        };
        let terrain_attrs = attrs(&[(0, 3), (1, 3), (2, 3), (3, 2), (4, 4)]);
        let flat_attrs = attrs(&[(0, 3), (1, 3), (2, 3)]);
        let line_attrs = attrs(&[(0, 3), (1, 3)]);
        let mut pipelines = HashMap::default();
        for kind in [Kind::Terrain, Kind::Flat, Kind::Line] {
            for water in [false, true] {
                for no_depth in [false, true] {
                    let (vs, fs, a) = match kind {
                        Kind::Terrain => ("terrain_vs", "terrain_fs", &terrain_attrs),
                        Kind::Flat => ("flat_vs", "flat_fs", &flat_attrs),
                        Kind::Line => ("line_vs", "line_fs", &line_attrs),
                    };
                    let buffers = [Some(wgpu::VertexBufferLayout {
                        array_stride: a.0,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &a.1,
                    })];
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
                            count: samples,
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
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let (atlas_view, sampler) = Self::atlas(&device, &queue);
        let tile_grids: Vec<f32> = dwell_mesh::tile_grids().into_iter().flatten().collect();
        let tile_grid_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile grids"),
            size: tile_grids.len() as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&tile_grid_buffer, 0, floats(&tile_grids));
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
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: tile_grid_buffer.as_entire_binding(),
                    },
                ],
            })
        });
        let (draw_buffer, draw_group) = Self::draw_buffer(&device, &draw_layout, 1024);
        let edges = geometry::box_edges();
        let arena = Arena::new(device.clone(), queue.clone(), &STRIDES);
        let outline_mesh = GpuMesh::lines(
            &arena,
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
            samples,
            draw_layout,
            pipelines,
            sky_pipeline,
            frame_buffers,
            frame_groups,
            draw_buffer,
            draw_group,
            draw_capacity: 1024,
            chunks: HashMap::default(),
            sections: HashMap::default(),
            shown: Vec::new(),
            stand_ins: Vec::new(),
            level_colors: false,
            players: HashMap::default(),
            capsules: HashMap::default(),
            outline: None,
            outline_mesh,
            debug: None,
            arena,
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
        let levels = atlas_levels().len() as u32;
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
        for (mip, (level, s)) in atlas_levels().iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                level,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * *s as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: *s as u32,
                    height: *s as u32,
                    depth_or_array_layers: 1,
                },
            );
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
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
                    size: None,
                }),
            }],
        });
        (buffer, group)
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
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Replaces (or, with `None`, removes) a terrain chunk; meshes are chunk-local.
    pub fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        self.chunks.remove(&coord);
        let Some(m) = meshes else { return };
        let t0 = crate::telemetry::now_ms();
        let (opaque, water) = (
            GpuMesh::textured(&self.arena, &m.opaque),
            GpuMesh::textured(&self.arena, &m.transparent),
        );
        crate::telemetry::add("upload.chunk", crate::telemetry::now_ms() - t0);
        crate::telemetry::add("upload.chunks", 1.0);
        crate::telemetry::add(
            "upload.bytes",
            (opaque.as_ref().map_or(0, |m| m.bytes) + water.as_ref().map_or(0, |m| m.bytes)) as f64,
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
        let t0 = crate::telemetry::now_ms();
        let opaque = GpuMesh::flat(&self.arena, &parts);
        let water = GpuMesh::flat(&self.arena, &[&m.water]);
        crate::telemetry::add("upload.section", crate::telemetry::now_ms() - t0);
        crate::telemetry::add("upload.sections", 1.0);
        crate::telemetry::add(
            "upload.bytes",
            (opaque.as_ref().map_or(0, |m| m.bytes) + water.as_ref().map_or(0, |m| m.bytes)) as f64,
        );
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
                let key = player_shape(&v);
                self.capsules.entry(key).or_insert_with(|| {
                    let (radius, height) = (f32::from_bits(key.0), f32::from_bits(key.1));
                    let face = if key.4 { -1.0 } else { 1.0 };
                    let g = geometry::player_mesh(radius, height, key.2, linear(key.3), face);
                    let m = FlatMesh {
                        positions: g.positions,
                        normals: g.normals,
                        colors: g.colors,
                        indices: g.indices,
                    };
                    GpuMesh::flat(&self.arena, &[&m]).expect("a capsule has faces")
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
            GpuMesh::lines(&self.arena, &pts)
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
    /// A draw's mesh and the index ranges it draws (a section's surface and its shown skirts).
    fn mesh_of(&self, m: &DrawMesh) -> (&GpuMesh, Vec<std::ops::Range<u32>>) {
        match m {
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
            DrawMesh::Player(key) => {
                let m = &self.capsules[key];
                (m, vec![m.parts[0].clone()])
            }
            DrawMesh::Outline => (&self.outline_mesh, vec![self.outline_mesh.parts[0].clone()]),
            DrawMesh::Debug => {
                let m = self.debug.as_ref().expect("collected");
                (m, vec![m.parts[0].clone()])
            }
        }
    }

    /// The draws of both depth passes (`passes`: each pass's near and far view depth), in one sweep
    /// over the chunks and sections (gathering each pass on its own walked everything twice).
    fn collect(
        &self,
        passes: [(f64, f64); 2],
        chunk_visible: &dyn Fn(ChunkCoord) -> bool,
    ) -> [Vec<DrawItem>; 2] {
        let mut out = Vec::new();
        let plain = [1.0, 1.0, 1.0, 1.0];
        let water = [1.0, 1.0, 1.0, WATER_OPACITY];
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        let forward = aim(self.yaw, self.pitch, self.face, self.flip).forward();
        let view = PassView::new(self.eye, forward, aspect);
        // Which passes (bit per pass) a box reaches into.
        let reaches = |origin: V3, size: f64| -> u8 {
            u8::from(view.holds(origin, size, passes[0].0, passes[0].1))
                | u8::from(view.holds(origin, size, passes[1].0, passes[1].1)) << 1
        };
        for (&coord, c) in &self.chunks {
            let passes = if chunk_visible(coord) {
                reaches(c.origin, f64::from(CHUNK_SIZE))
            } else {
                0
            };
            if passes == 0 {
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
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
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
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
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
            let passes = reaches(s.origin, s.cell * 32.0);
            if passes == 0 {
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
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
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
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Section(id, true, 0),
                });
            }
        }
        // Stand-ins: the source section's surface, clipped to the hole's box (its own skirts off).
        for st in self.stand_ins.iter().take(MAX_STAND_INS) {
            let passes = 3;
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
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Section(st.id, false, 0),
            });
        }
        for v in self.players.values() {
            let passes = reaches(v.feet, 4.0);
            if passes == 0 {
                continue;
            }
            // The mesh is posed and coloured in feet-local space; the draw turns it to the yaw.
            let r = self.rel(v.feet);
            let yaw = (v.yaw as f32).to_radians();
            out.push(DrawItem {
                kind: Kind::Flat,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], 1.0],
                clip: None,
                tint: [1.0, 1.0, 1.0, 1.0],
                rotation: [yaw.cos(), yaw.sin()],
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Player(player_shape(v)),
            });
        }
        if let Some((cell, _height)) = self.outline.filter(|(c, _)| reaches(*c, 2.0) != 0) {
            let passes = reaches(cell, 2.0);
            let r = self.rel([cell[0] - 0.002, cell[1] - 0.002, cell[2] - 0.002]);
            let c = hex(0x101418).map(|v| v as f32);
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], 1.0],
                clip: None,
                tint: [c[0], c[1], c[2], 0.8],
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Outline,
            });
        }
        // Debug lines: in the pass that draws from the near plane.
        let passes = u8::from(passes[0].0 < NEAR_PLANE * 2.0)
            | u8::from(passes[1].0 < NEAR_PLANE * 2.0) << 1;
        if self.debug.is_some() && passes != 0 {
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: true,
                offset: [0.0, 0.0, 0.0, 1.0],
                clip: None,
                tint: [1.0, 1.0, 1.0, 1.0],
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Debug,
            });
        }
        for d in &mut out {
            d.page = self.mesh_of(&d.mesh).0.region.page;
            d.key = draw_order(d);
        }
        // One stable sort by a precomputed key (draw_order), then each pass takes its draws in that
        // order.
        out.sort_by_key(|d| d.key);
        let mut split: [Vec<DrawItem>; 2] = [Vec::with_capacity(out.len()), Vec::new()];
        for d in out {
            if d.passes == 3 {
                split[1].push(d.clone());
                split[0].push(d);
            } else if d.passes & 1 != 0 {
                split[0].push(d);
            } else {
                split[1].push(d);
            }
        }
        split
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
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: self.samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        };
        let colour = (self.samples > 1).then(|| make(self.format, "colour"));
        self.targets = Some(Targets {
            colour,
            depth: make(DEPTH, "depth"),
        });
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
        let draws = self.collect(passes, chunk_visible);
        for (i, (near, far)) in passes.iter().enumerate() {
            let u = self.frame_uniforms(&basis, *near, *far);
            self.queue
                .write_buffer(&self.frame_buffers[i], 0, floats(&u));
        }
        // Per-draw uniforms, both passes in one buffer.
        let total = (draws[0].len() + draws[1].len()) as u64;
        if total > self.draw_capacity {
            self.draw_capacity = total.next_power_of_two();
            self.draw_buffer.destroy();
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
            data[at + 16..at + 18].copy_from_slice(&d.rotation);
        }
        if !data.is_empty() {
            self.queue.write_buffer(&self.draw_buffer, 0, floats(&data));
        }
        let targets = self.targets.as_ref().expect("targets");
        let (colour, depth) = (targets.colour.as_ref().map(|c| &c.1), &targets.depth.1);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let arena = self.arena.borrow();
        let (mut calls, mut triangles) = (0u32, 0u64);
        let mut slot = 0u64;
        for (i, pass_draws) in draws.iter().enumerate() {
            let horizon = HORIZON_COLOR;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if i == 0 { "far" } else { "near" }),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour.unwrap_or(target),
                    depth_slice: None,
                    resolve_target: colour.and((i == 1).then_some(target)),
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
            pass.set_bind_group(1, &self.draw_group, &[]);
            let mut bound: Option<(Kind, bool, bool)> = None;
            let mut bound_page: Option<(usize, usize)> = None;
            if i == 0 {
                pass.set_pipeline(&self.sky_pipeline);
                pass.draw(0..3, 0..1);
                calls += 1;
            }
            for d in pass_draws {
                let instance = slot as u32;
                slot += 1;
                let (mesh, ranges) = self.mesh_of(&d.mesh);
                // Each WebGPU call crosses from WASM to JS: set only what changes.
                let key = (d.kind, d.water, d.no_depth);
                if bound != Some(key) {
                    pass.set_pipeline(&self.pipelines[&key]);
                    bound = Some(key);
                }
                // A page holds many meshes: bound once for the draws from it in a row.
                let page = (mesh.region.format, mesh.region.page);
                if bound_page != Some(page) {
                    let buffer = arena.buffer(page.0, page.1);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    bound_page = Some(page);
                }
                for r in merge_ranges(ranges) {
                    triangles += u64::from(r.len() as u32 / 3);
                    let first = mesh.first_index;
                    pass.draw_indexed(
                        first + r.start..first + r.end,
                        mesh.base_vertex,
                        instance..instance + 1,
                    );
                    calls += 1;
                }
            }
        }
        drop(arena);
        self.queue.submit([encoder.finish()]);
        self.stats.calls = calls;
        self.stats.triangles = triangles;
    }

    /// The chunk at a world position (tests and tools).
    pub fn has_chunk_at(&self, x: i32, y: i32, z: i32) -> bool {
        self.chunks.contains_key(&chunk_of(x, y, z))
    }
}

/// The camera as a pass's culling sees it: where it is, its view axis, and how much farther than
/// its depth a point in the view can be (1 / the cosine of the angle out to a corner of the view).
#[derive(Clone, Copy, Debug)]
struct PassView {
    eye: V3,
    forward: V3,
    stretch: f64,
}

impl PassView {
    fn new(eye: V3, forward: V3, aspect: f64) -> Self {
        let tan_y = (vertical_fov(aspect).to_radians() / 2.0).tan();
        let tan_x = tan_y * aspect;
        Self {
            eye,
            forward,
            stretch: (1.0 + tan_x * tan_x + tan_y * tan_y).sqrt(),
        }
    }

    /// Whether the box [origin, origin + size]³ has anything a pass drawing view depths [near, far]
    /// draws. The passes clip by depth along the view axis, not by distance: a box beside the view
    /// can be farther than `far` and still in front of the far plane.
    fn holds(&self, origin: V3, size: f64, near: f64, far: f64) -> bool {
        let (mut d2, mut z_lo, mut z_hi) = (0.0, 0.0, 0.0);
        for ((o, e), f) in origin.iter().zip(self.eye).zip(self.forward) {
            let lo = o - e;
            let hi = lo + size;
            let d = if lo > 0.0 {
                lo
            } else if hi < 0.0 {
                -hi
            } else {
                0.0
            };
            d2 += d * d;
            let (a, b) = (lo * f, hi * f);
            z_lo += a.min(b);
            z_hi += a.max(b);
        }
        // Nothing in the view at depth ≤ far is farther than far × stretch.
        z_lo <= far && z_hi >= near && d2.sqrt() <= far * self.stretch
    }
}

/// Floats in the frame uniform block (`Frame` in shaders.rs): a matrix and 15 vec4s.
const FRAME_FLOATS: usize = 16 + 15 * 4;

/// The atlas and its mipmaps, each with its side in texels: down to the level where a tile's gutter
/// is one texel wide. Filtering a smaller level at a tile's edge would reach past the gutter into
/// the neighbouring tiles and the atlas's empty (transparent) space, which a grazing view of
/// distant ground samples: a grey, see-through grid along the block edges.
fn atlas_levels() -> Vec<(Vec<u8>, usize)> {
    let mut levels = vec![(dwell_mesh::build_atlas(), dwell_mesh::ATLAS_SIZE)];
    while let Some(&(ref level, s)) = levels.last()
        && s > dwell_mesh::ATLAS_SIZE / dwell_mesh::GUTTER
    {
        levels.push((downsample(level, s), s / 2));
    }
    levels
}

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
    fn draw_order_opaque_near_to_far_by_pipeline_then_water_far_to_near_then_lines() {
        let item = |kind: Kind, water: bool, dist: f32, page: usize| DrawItem {
            kind,
            water,
            no_depth: false,
            offset: [dist, 0.0, 0.0, 1.0],
            clip: None,
            tint: [1.0; 4],
            rotation: NO_ROTATION,
            mesh: DrawMesh::Outline,
            page,
            passes: 1,
            key: 0,
        };
        let mut items = [
            item(Kind::Line, false, 1.0, 0),
            item(Kind::Flat, true, 5.0, 0),
            item(Kind::Flat, false, 50.0, 1),
            item(Kind::Terrain, false, 30.0, 0),
            item(Kind::Flat, true, 50.0, 0),
            item(Kind::Flat, false, 20.0, 1),
            item(Kind::Terrain, false, 10.0, 0),
            item(Kind::Flat, false, 90.0, 0),
        ];
        for d in &mut items {
            d.key = draw_order(d);
        }
        items.sort_by_key(|d| d.key);
        let got: Vec<(bool, f32, usize)> = items
            .iter()
            .map(|d| (d.water, d.offset[0], d.page))
            .collect();
        assert_eq!(
            got,
            vec![
                (false, 10.0, 0), // terrain, near to far
                (false, 30.0, 0),
                (false, 90.0, 0), // flat, page 0 then page 1, each near to far
                (false, 20.0, 1),
                (false, 50.0, 1),
                (true, 50.0, 0), // water far to near
                (true, 5.0, 0),
                (false, 1.0, 0), // lines last
            ]
        );
    }

    #[test]
    fn touching_index_ranges_draw_as_one() {
        assert_eq!(
            merge_ranges(vec![0..6, 6..9, 9..9, 12..15, 15..18]),
            vec![0..9, 12..18]
        );
        assert!(merge_ranges(vec![3..3, 5..5]).is_empty());
    }

    #[test]
    fn streams_interleave_vertex_by_vertex() {
        let p = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let uv = [7.0, 8.0, 9.0, 10.0];
        assert_eq!(
            interleave(&[(&p, 3), (&uv, 2)]),
            vec![1.0, 2.0, 3.0, 7.0, 8.0, 4.0, 5.0, 6.0, 9.0, 10.0]
        );
    }

    #[test]
    fn software_adapters_draw_without_multisampling() {
        assert_eq!(samples_for(wgpu::DeviceType::Cpu), 1);
        assert_eq!(samples_for(wgpu::DeviceType::DiscreteGpu), SAMPLES);
        assert_eq!(samples_for(wgpu::DeviceType::IntegratedGpu), SAMPLES);
    }

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

    /// Bilinear filtering of mip level (`texels`, `size`) at atlas coordinates (u, v), as the GPU
    /// samples it (texel centres at half-integers, clamped at the edge), in 0–255 per channel.
    fn bilinear(texels: &[u8], size: usize, u: f64, v: f64) -> [f64; 4] {
        let (x, y) = (u * size as f64 - 0.5, v * size as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f64, y: f64, c: usize| {
            let x = (x.max(0.0) as usize).min(size - 1);
            let y = (y.max(0.0) as usize).min(size - 1);
            f64::from(texels[(y * size + x) * 4 + c])
        };
        std::array::from_fn(|c| {
            let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1.0, y0, c) * fx;
            let bottom = at(x0, y0 + 1.0, c) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0, c) * fx;
            top * (1.0 - fy) + bottom * fy
        })
    }

    #[test]
    fn filtering_any_mip_level_at_a_tiles_edge_stays_on_that_tile() {
        // A block's texture is sampled up to its tile's edges (fract(uv)), and the bilinear filter
        // reaches half a texel past the sample: at every level the gutter must cover that, or
        // distant ground picks up the neighbouring tiles and the atlas's empty space along every
        // block edge (the grey, see-through grid near the horizon, playtest 2026-10-14).
        let levels = atlas_levels();
        for (t, name) in dwell_mesh::TILE_ORDER.iter().enumerate() {
            let r = dwell_mesh::tile_rect(name);
            let cell = [
                (t % dwell_mesh::ATLAS_CELLS) * dwell_mesh::CELL,
                (t / dwell_mesh::ATLAS_CELLS) * dwell_mesh::CELL,
            ];
            for (level, (texels, size)) in levels.iter().enumerate() {
                let texel = dwell_mesh::ATLAS_SIZE / size;
                for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
                    // The filter's 2 × 2 texels lie in the tile's cell (the tile and its gutter).
                    for (k, w) in [u, v].into_iter().enumerate() {
                        let first = (w * *size as f64 - 0.5).floor();
                        let (lo, hi) = (first as usize * texel, (first as usize + 2) * texel);
                        assert!(
                            first >= 0.0 && lo >= cell[k] && hi <= cell[k] + dwell_mesh::CELL,
                            "{name} at mip {level}: filtering ({u}, {v}) reaches past its cell"
                        );
                    }
                    let got = bilinear(texels, *size, u, v);
                    assert_eq!(got[3], 255.0, "{name} at mip {level}, ({u}, {v})");
                }
            }
        }
    }

    #[test]
    fn a_pass_draws_what_its_depth_range_holds_at_the_edge_of_the_view() {
        // Looking along +z, a box 900 m ahead and 600 m to the side is nearer than the split in
        // view depth (where the near pass draws it and the far pass clips it), though more than the
        // split away in a straight line: it must be in the near pass, or neither pass draws it (the
        // gaps in the ground seen from high up, playtest 2026-10-14).
        let view = PassView::new([0.0; 3], [0.0, 0.0, 1.0], 2.0);
        let origin = [600.0, -16.0, 884.0];
        assert!(view.holds(origin, 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(!view.holds(origin, 32.0, NEAR_SPLIT * 0.95, FAR_PLANE));
        // Beyond the split in depth: only the far pass.
        let beyond = [600.0, -16.0, 1100.0];
        assert!(!view.holds(beyond, 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(view.holds(beyond, 32.0, NEAR_SPLIT * 0.95, FAR_PLANE));
        // Behind the eye, or too far aside to be in the view at all: neither.
        assert!(!view.holds([0.0, 0.0, -100.0], 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(!view.holds([5000.0, 0.0, 500.0], 32.0, NEAR_PLANE, NEAR_SPLIT));
    }

    fn noop_renderer() -> Renderer {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        Renderer::with_samples(device, queue, wgpu::TextureFormat::Rgba8Unorm, 1)
    }

    /// Whether submitting a pass that binds `b` (a vertex or index buffer) fails because it was
    /// destroyed.
    fn destroyed_on(device: &wgpu::Device, queue: &wgpu::Queue, b: &wgpu::Buffer) -> bool {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations::default(),
                })],
                ..Default::default()
            });
            if b.usage().contains(wgpu::BufferUsages::INDEX) {
                pass.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
            } else {
                pass.set_vertex_buffer(0, b.slice(..));
            }
        }
        queue.submit([encoder.finish()]);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::pin!(scope.pop()).as_mut().poll(&mut cx) {
            std::task::Poll::Ready(e) => e.is_some_and(|e| e.to_string().contains("destroyed")),
            std::task::Poll::Pending => panic!("error scope pending"),
        }
    }

    #[test]
    fn replaced_meshes_free_their_gpu_memory() {
        // Every replaced or removed chunk, section and debug-lines mesh must give its GPU memory
        // back, or memory grows and the frame rate falls over a session (playtest 2026-10-14).
        // Meshes live in arena pages (arena.rs): replacing them over and over reuses the same
        // pages, and removing them all leaves nothing in use.
        let mut r = noop_renderer();
        let outline = r.arena.borrow().used_bytes();
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        for x in -1..=32 {
            for z in -1..=32 {
                padded[dwell_mesh::padded_index(x, 0, z)] = dwell_blocks::require("dwell:stone");
            }
        }
        let meshes = dwell_mesh::mesh_chunk(&padded, false).expect("meshed");
        let quad = FlatMesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            normals: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            colors: vec![1.0; 9],
            indices: vec![0, 1, 2],
        };
        let section = SectionMeshes {
            opaque: quad.clone(),
            water: quad,
            skirts: Default::default(),
        };
        let line = Line {
            from: [0.0; 3],
            to: [1.0; 3],
            color: 0xffffff,
        };
        let replace = |r: &mut Renderer| {
            for x in 0..4 {
                r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, Some(&meshes));
                r.set_lod_section(i64::from(x), [0.0; 3], 2.0, Some(&section));
            }
            r.set_debug_lines(Some(&[line]));
        };
        replace(&mut r);
        let (pages, used) = (r.arena.borrow().page_bytes(), r.arena.borrow().used_bytes());
        for _ in 0..500 {
            replace(&mut r);
        }
        assert_eq!(
            r.arena.borrow().page_bytes(),
            pages,
            "replacing reuses the pages"
        );
        assert_eq!(
            r.arena.borrow().used_bytes(),
            used,
            "replaced meshes are freed"
        );
        for x in 0..4 {
            r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, None);
            r.set_lod_section(i64::from(x), [0.0; 3], 2.0, None);
        }
        r.set_debug_lines(None);
        assert_eq!(
            r.arena.borrow().used_bytes(),
            outline,
            "only the block outline is left"
        );
        // The pages themselves are destroyed with the renderer (wgpu's WebGPU backend does not
        // destroy a buffer on drop).
        let pages = r.arena.borrow().buffers();
        let (device, queue) = (r.device.clone(), r.queue.clone());
        drop(r);
        for p in &pages {
            assert!(
                destroyed_on(&device, &queue, p),
                "a page outlived the renderer"
            );
        }
    }

    #[test]
    fn frames_draw_cleanly_across_resizes_and_replaced_meshes() {
        // Destroying the old targets, draw uniforms and meshes between frames is valid WebGPU.
        let mut r = noop_renderer();
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = r.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        for x in -1..=32 {
            for z in -1..=32 {
                padded[dwell_mesh::padded_index(x, 0, z)] = dwell_blocks::require("dwell:grass");
            }
        }
        let meshes = dwell_mesh::mesh_chunk(&padded, false).expect("meshed");
        r.set_camera([16.0, 4.0, 16.0], 0.0, -30.0, 1.0, 0.0);
        for frame in 0..4 {
            r.resize(64, 64, 1.0 + f64::from(frame));
            for x in 0..=frame {
                r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, Some(&meshes));
            }
            r.render(&view, &|_| true);
        }
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::pin!(scope.pop()).as_mut().poll(&mut cx) {
            std::task::Poll::Ready(e) => assert!(e.is_none(), "{e:?}"),
            std::task::Poll::Pending => panic!("error scope pending"),
        }
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
