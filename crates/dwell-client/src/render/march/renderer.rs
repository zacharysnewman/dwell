//! The marcher behind the renderer seam (`seam.rs`, docs/RAY_MARCHED_RENDERER.md §3): the
//! payloads the job workers make with the meshing jobs ([`ChunkBricks`], [`SectionBricks`]:
//! `dwell_march` node builds), the [`TerrainRenderer`] that places them in the GPU pool, and
//! [`MarchRenderer::draw`], which marches into the marcher's own single-sample storage textures
//! (compute can't write multisampled targets) and then copies them into one side of the shared
//! frame (`frame::Targets`) with a full-screen pass that writes every sample: linear colour
//! premultiplied by coverage (1 on a hit, 0 for the sky) and the view depth (0 for none).

use dwell_march::{BuildOptions, NodeBuild, StateTable, build_node, build_section};
use dwell_world::ChunkCoord;
use dwell_world::hash::HashMap;

use super::{MarchFrame, MarchTargets, Marcher};
use crate::render::StandIn;
use crate::render::frame::{HDR, Targets, VIEW_DEPTH};
use crate::render::seam::TerrainRenderer;

/// The marcher draws terrain: `__dwell.renderers()` lists `march`, and the bench runs its
/// configurations.
pub const BUILT: bool = true;

/// A full-detail chunk's bricks: its level-0 node build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkBricks(pub NodeBuild);

/// An LOD section's bricks: its node build at the section's level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionBricks(pub NodeBuild);

impl ChunkBricks {
    /// GPU bytes once placed (node, bricks, column grid).
    pub fn bytes(&self) -> usize {
        self.0.gpu_bytes()
    }
}

impl SectionBricks {
    pub fn bytes(&self) -> usize {
        self.0.gpu_bytes()
    }
}

fn table() -> &'static StateTable {
    static TABLE: std::sync::OnceLock<StateTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(StateTable::registry)
}

/// A chunk's bricks from its padded voxels (34³, the mesher's input; world orientation, so
/// `mirror` — face B — changes nothing here: the shader turns textures and tints by the world face).
/// A malformed input gives an all-air build.
pub fn build_chunk_bricks(padded: &[u16], _mirror: bool) -> ChunkBricks {
    if padded.len() != dwell_mesh::PADDED_VOLUME {
        return ChunkBricks(NodeBuild::Air);
    }
    ChunkBricks(build_node(
        |x, y, z| padded[dwell_mesh::padded_index(x, y, z)],
        table(),
        BuildOptions::CHUNK,
    ))
}

/// An LOD section's bricks from its cells (34³ with the apron, `cell_index` order; world
/// orientation, as for chunks).
pub fn build_section_bricks(cells: &[u16], _mirror: bool) -> SectionBricks {
    if cells.len() != dwell_world::lod::VOLUME {
        return SectionBricks(NodeBuild::Air);
    }
    SectionBricks(build_section(cells, table()))
}

/// A section's level and node from its origin (world metres) and cell size (2^L m).
pub fn section_node(origin: [f64; 3], cell_size: f64) -> (u32, [i32; 3]) {
    let level = cell_size.max(1.0).log2().round() as u32;
    let n = origin.map(|o| {
        let g = o.round() as i64 + dwell_march::layout::GRID_OFFSET;
        (g >> (level + 5)) as i32
    });
    (level, n)
}

const COPY: &str = r"
@group(0) @binding(0) var march_color: texture_2d<f32>;
@group(0) @binding(1) var march_depth: texture_2d<f32>;

struct CopyOut {
    @location(0) color: vec4f,
    @location(1) depth: f32,
};

@vertex
fn copy_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(p[i], 0.0, 1.0);
}

// Every sample of the pixel takes the marched pixel (the march is one ray per pixel).
@fragment
fn copy_fs(@builtin(position) at: vec4f) -> CopyOut {
    let p = vec2i(at.xy);
    var o: CopyOut;
    o.color = textureLoad(march_color, p, 0);
    o.depth = textureLoad(march_depth, p, 0).r;
    return o;
}
";

struct Copy {
    samples: u32,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

/// The ray marcher behind the seam.
pub struct MarchRenderer {
    pub marcher: Marcher,
    targets: Option<MarchTargets>,
    copy: Option<Copy>,
    copy_group: Option<wgpu::BindGroup>,
    sections: HashMap<i64, (u32, [i32; 3])>,
    chunks: std::collections::HashSet<ChunkCoord>,
    shown: Vec<(i64, u8)>,
}

impl MarchRenderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, output: wgpu::TextureFormat) -> Self {
        Self {
            marcher: Marcher::new(device, queue, output),
            targets: None,
            copy: None,
            copy_group: None,
            sections: HashMap::default(),
            chunks: Default::default(),
            shown: Vec::new(),
        }
    }

    /// Chunks and sections held.
    pub fn held(&self) -> (usize, usize) {
        (self.chunks.len(), self.sections.len())
    }

    fn copy_pipeline(device: &wgpu::Device, samples: u32) -> Copy {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march copy"),
            source: wgpu::ShaderSource::Wgsl(COPY.into()),
        });
        let texture = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march copy"),
            entries: &[texture(0, true), texture(1, false)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march copy"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("march copy"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("copy_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("copy_fs"),
                compilation_options: Default::default(),
                targets: &[Some(HDR.into()), Some(VIEW_DEPTH.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        Copy {
            samples,
            layout,
            pipeline,
        }
    }

    /// The march's id target (cell id and face per pixel, 0 for none: `wgsl::cell_id`), after a
    /// draw.
    pub fn id_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.id.1)
    }

    /// Records the march for `frame` into `targets` (one side of the shared frame). The timestamp
    /// writes time it: the march's compute pass begins it, the copy ends it.
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &MarchFrame,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        self.marcher.collect();
        let size = targets.colour.0.size();
        let size = (size.width, size.height);
        let samples = targets.view_depth.0.sample_count();
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(self.marcher.targets(size));
            self.copy_group = None;
        }
        let device = self.marcher.device().clone();
        if self.copy.as_ref().is_none_or(|c| c.samples != samples) {
            self.copy = Some(Self::copy_pipeline(&device, samples));
            self.copy_group = None;
        }
        let mt = self.targets.as_ref().expect("targets");
        let copy = self.copy.as_ref().expect("copy");
        let group = self.copy_group.get_or_insert_with(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("march copy"),
                layout: &copy.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mt.color.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&mt.depth.1),
                    },
                ],
            })
        });
        let (begin, end) = match timestamp_writes {
            Some(w) => (
                Some(wgpu::ComputePassTimestampWrites {
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
        self.marcher.draw(encoder, frame, mt, begin);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("march copy"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: targets.scene_colour(),
                    depth_slice: None,
                    resolve_target: targets.colour_ms.as_ref().map(|_| &targets.colour.1),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: if targets.colour_ms.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &targets.view_depth.1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: end,
            ..Default::default()
        });
        pass.set_pipeline(&copy.pipeline);
        pass.set_bind_group(0, &*group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// After the frame's submit (the histogram's readback).
    pub fn submitted(&mut self) {
        self.marcher.submitted();
    }
}

impl TerrainRenderer for MarchRenderer {
    type Chunk = ChunkBricks;
    type Section = SectionBricks;
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkBricks>) {
        self.marcher.set_chunk(coord, payload.map(|b| &b.0));
        if payload.is_some() {
            self.chunks.insert(coord);
        } else {
            self.chunks.remove(&coord);
        }
    }
    fn set_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        payload: Option<&SectionBricks>,
    ) {
        let (level, n) = section_node(origin, cell_size);
        match payload {
            Some(b) => {
                self.marcher.gpu.put(level, n, &b.0);
                self.sections.insert(id, (level, n));
            }
            None => {
                if let Some((level, n)) = self.sections.remove(&id) {
                    self.marcher.gpu.scene.remove(level, n);
                }
            }
        }
    }
    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        // Every held section is in the clipmap; cone LOD picks the level per ray.
        self.shown = visible.to_vec();
    }
    fn show_stand_ins(&mut self, _stand_ins: &[StandIn]) {
        // A coarser level is the stand-in: a ray with no data at its level takes the next one up.
    }
    fn clear(&mut self) {
        let device = self.marcher.device().clone();
        let queue = self.marcher.queue().clone();
        let output = self.marcher.output();
        let settings = self.marcher.settings;
        self.marcher = Marcher::new(device, queue, output);
        self.marcher.settings = settings;
        self.targets = None;
        self.copy_group = None;
        self.sections.clear();
        self.chunks.clear();
        self.shown.clear();
    }
    fn gpu_bytes(&self) -> u64 {
        self.marcher.gpu_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    #[test]
    fn chunk_bricks_come_from_the_padded_interior_in_world_orientation() {
        let stone = require("dwell:stone");
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        padded[dwell_mesh::padded_index(-1, 0, 0)] = stone; // the apron: not a cell
        padded[dwell_mesh::padded_index(3, 4, 5)] = stone;
        let b = build_chunk_bricks(&padded, false);
        let cells = dwell_march::build::decode(&b.0, 0);
        assert_eq!(cells.iter().filter(|&&c| c == stone).count(), 1);
        assert_eq!(cells[3 + 32 * (4 + 32 * 5)], stone);
        assert_eq!(
            build_chunk_bricks(&padded, true),
            b,
            "face B: the same bricks"
        );
        assert_eq!(build_chunk_bricks(&[1, 2], false).0, NodeBuild::Air);
    }

    #[test]
    fn sections_are_placed_at_their_level_and_node() {
        // A level-3 section whose origin is the grid's node (100, 2, 7).
        let off = dwell_march::layout::GRID_OFFSET as f64;
        let origin = [100.0 * 256.0 - off, 2.0 * 256.0 - off, 7.0 * 256.0 - off];
        assert_eq!(section_node(origin, 8.0), (3, [100, 2, 7]));
        let chunk = [32.0 * 5.0, -64.0, 0.0];
        let (l, n) = section_node(chunk, 1.0);
        assert_eq!(l, 0);
        assert_eq!(
            n,
            dwell_march::chunk_node(ChunkCoord::new(5, -2, 0)),
            "level 0 is the chunk's node"
        );
    }

    #[test]
    fn holds_what_it_is_given_and_clears_on_the_noop_device() {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let mut m = MarchRenderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
        let stone = require("dwell:stone");
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        padded[dwell_mesh::padded_index(1, 1, 1)] = stone;
        let c = ChunkCoord::new(0, 0, 0);
        m.set_chunk(c, Some(&build_chunk_bricks(&padded, false)));
        let cells = vec![stone; dwell_world::lod::VOLUME];
        m.set_section(
            3,
            [0.0, 0.0, 0.0],
            4.0,
            Some(&build_section_bricks(&cells, false)),
        );
        assert_eq!(m.held(), (1, 1));
        m.set_section(3, [0.0; 3], 4.0, None);
        m.set_chunk(c, None);
        assert_eq!(m.held(), (0, 0));
        m.set_chunk(c, Some(&build_chunk_bricks(&padded, false)));
        m.clear();
        assert_eq!(m.held(), (0, 0));
    }
}
