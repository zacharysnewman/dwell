//! The water's GPU heightfield, shader side (proposals/WATER_GPU_SURFACE.md §3, step C6a): the
//! WGSL that places the shared grid mesh's corners from the height atlas. The tiles, the grid and
//! the atlas's slots are `dwell_mesh::heightfield` (pure data, tested there); a tier draws this
//! when its data says `heightfield: true` (`shared/look/watertiers.json`), which no tier does yet:
//! the water pass's instanced draw over it is the rest of C6a (the proposal's build plan).
//!
//! The atlas is an `r32float` texture of tiles [`TILE`] texels square, [`TILES_PER_ROW`] to a row,
//! read with `textureLoad` (no filtering, so no float-filterable feature). A tile holds a chunk's
//! face-local column levels in chunk-local metres, `DRY` where a column has no surface; corners
//! take the mean of the wet columns around them within `joinM` of the highest
//! (`dwell_mesh::heightfield::corner_height`, the reference this shader is tested against in
//! spirit: the same rule, the same constants). Each instance names its tile and its chunk's
//! face-local floor; the shader crosses into the world through `FaceFrame::wgsl()`'s `face_y`, so
//! one grid serves both faces (BIFACIAL_WORLD.md §11).

use dwell_mesh::heightfield::{COLUMNS, CORNERS, DRY, HeightAtlas, TILE, WaterTile};

/// Tiles per row of the atlas texture (its width is this × [`TILE`] texels).
pub const TILES_PER_ROW: usize = 64;

/// The atlas texel at which slot `slot`'s tile starts.
pub fn tile_origin(slot: usize) -> [u32; 2] {
    [
        ((slot % TILES_PER_ROW) * TILE) as u32,
        ((slot / TILES_PER_ROW) * TILE) as u32,
    ]
}

/// The atlas's size in tiles: [`TILES_PER_ROW`] × this many rows.
pub const TILE_ROWS: usize = 32;
/// The most tiles the atlas holds (near water chunks past it draw no surface).
pub const MAX_TILES: usize = TILES_PER_ROW * TILE_ROWS;
/// The atlas texture's size in texels.
pub const ATLAS_SIZE: [u32; 2] = [(TILES_PER_ROW * TILE) as u32, (TILE_ROWS * TILE) as u32];
/// The heightfield's bind group in the water pipelines (after the scene copy's, group 2).
pub const GROUP: u32 = 3;

/// The heightfield's WGSL, appended to the scene module (`shaders::source`): the atlas binding,
/// `hf_corner` (a grid corner's face-local height), `hf_normal` (the surface's face-local normal
/// from the corners around it) and `heightfield_vs`, a `fluid_vs` twin that places grid vertex
/// `vertex_index` of the draw's tile and feeds `water_fs`/`water_scene_fs` unchanged. A draw
/// carries its tile's texel origin in `quat.xy`, its face (+1 A, −1 B) in `quat.z` and its fluid
/// id in `quat.w`; dry columns are collapsed onto their wet neighbours' corners or, with none,
/// onto the chunk's floor under a discarded fragment (`hf_dry`).
pub fn wgsl() -> String {
    let join = dwell_look::water_tiers::HEIGHTFIELD.join_m as f32;
    let chunk_h = (f64::from(dwell_world::CHUNK_SIZE) * dwell_world::CELL_H) as f32;
    format!(
        "\n\
const HF_TILE: i32 = {tile};\n\
const HF_CORNERS: u32 = {corners}u;\n\
const HF_DRY: f32 = {dry:?};\n\
const HF_JOIN_M: f32 = {join:?};\n\
const HF_CHUNK_H: f32 = {chunk_h:?};\n\
@group({group}) @binding(0) var hf_heights: texture_2d<f32>;\n\
\n\
fn hf_level(tile: vec2i, c: vec2i) -> f32 {{\n\
    return textureLoad(hf_heights, tile + clamp(c, vec2i(0), vec2i(HF_TILE - 1)), 0).r;\n\
}}\n\
// A column's flood light (sky, block as shares of 15), packed sky × 16 + block.\n\
fn hf_light(tile: vec2i, c: vec2i) -> vec2f {{\n\
    let p = textureLoad(hf_heights, tile + clamp(c, vec2i(0), vec2i(HF_TILE - 1)), 0).g;\n\
    return vec2f(floor(p / 16.0), p - 16.0 * floor(p / 16.0)) / 15.0;\n\
}}\n\
// A corner's flood light: the brightest of its wet columns' (the water's light reaches across).\n\
fn hf_corner_light(tile: vec2i, corner: vec2i) -> vec2f {{\n\
    var l = vec2f(0.0);\n\
    for (var k = 0; k < 4; k++) {{\n\
        let c = corner + vec2i(k % 2, k / 2);\n\
        if hf_wet(hf_level(tile, c)) {{ l = max(l, hf_light(tile, c)); }}\n\
    }}\n\
    return l;\n\
}}\n\
fn hf_wet(h: f32) -> bool {{ return h > HF_DRY * 0.5; }}\n\
// A corner's height: the mean of the wet columns around it within HF_JOIN_M of the highest\n\
// (dwell-mesh heightfield.rs `corner_height`); HF_DRY if none is wet. Corner (i, j) touches\n\
// tile texels (i, j) to (i + 1, j + 1).\n\
fn hf_corner(tile: vec2i, corner: vec2i) -> f32 {{\n\
    let h = array<f32, 4>(\n\
        hf_level(tile, corner), hf_level(tile, corner + vec2i(1, 0)),\n\
        hf_level(tile, corner + vec2i(0, 1)), hf_level(tile, corner + vec2i(1, 1)));\n\
    var top = HF_DRY;\n\
    for (var k = 0; k < 4; k++) {{ if hf_wet(h[k]) {{ top = max(top, h[k]); }} }}\n\
    if !hf_wet(top) {{ return HF_DRY; }}\n\
    var sum = 0.0;\n\
    var n = 0.0;\n\
    for (var k = 0; k < 4; k++) {{\n\
        if hf_wet(h[k]) && top - h[k] <= HF_JOIN_M {{ sum += h[k]; n += 1.0; }}\n\
    }}\n\
    return sum / n;\n\
}}\n\
// A neighbour corner's height for the slope, the corner's own where it is dry.\n\
fn hf_or(tile: vec2i, corner: vec2i, own: f32) -> f32 {{\n\
    let h = hf_corner(tile, corner);\n\
    return select(own, h, hf_wet(h) && abs(h - own) <= HF_JOIN_M);\n\
}}\n\
// The face-local normal at a corner: central differences over the corners beside it.\n\
fn hf_normal(tile: vec2i, corner: vec2i, own: f32) -> vec3f {{\n\
    let dx = hf_or(tile, corner + vec2i(1, 0), own) - hf_or(tile, corner - vec2i(1, 0), own);\n\
    let dz = hf_or(tile, corner + vec2i(0, 1), own) - hf_or(tile, corner - vec2i(0, 1), own);\n\
    return normalize(vec3f(-0.5 * dx, 1.0, -0.5 * dz));\n\
}}\n\
// A wet chunk-local column (0..16 in x and z) under a point of the surface.\n\
fn hf_column_wet(tile: vec2i, xz: vec2f) -> bool {{\n\
    let c = clamp(vec2i(floor(xz)), vec2i(0), vec2i(HF_TILE - 3));\n\
    return hf_wet(hf_level(tile, c + vec2i(1)));\n\
}}\n\
\n\
@vertex\n\
fn heightfield_vs(@builtin(vertex_index) index: u32, @builtin(instance_index) k: u32) -> FluidOut {{\n\
    let draw = draws[k];\n\
    let tile = vec2i(draw.quat.xy);\n\
    let face = draw.quat.z;\n\
    let corner = vec2i(i32(index % HF_CORNERS), i32(index / HF_CORNERS));\n\
    var h = hf_corner(tile, corner);\n\
    var n = vec3f(0.0, 1.0, 0.0);\n\
    if hf_wet(h) {{ n = hf_normal(tile, corner, h); }} else {{ h = 0.0; }}\n\
    // face-boundary: the face-local chunk turned back into world rows, as the chunk mesher's\n\
    // `flip_arrays_y` turns a face-B chunk's mesh (the draw's offset is the chunk's minimum).\n\
    let y = select(HF_CHUNK_H - h, h, face > 0.0);\n\
    let local = vec3f(f32(corner.x), y, f32(corner.y));\n\
    var o: FluidOut;\n\
    o.draw = k;\n\
    o.rel = draw.offset.xyz + local * draw.offset.w;\n\
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);\n\
    o.normal = vec3f(n.x, n.y * face, n.z);\n\
    o.color = vec3f(1.0);\n\
    o.wrapped = draw.rotation.zw + local.xz * draw.offset.w;\n\
    o.fluid = u32(draw.quat.w + 0.5);\n\
    o.flood = hf_corner_light(tile, corner);\n\
    return o;\n\
}}\n",
        tile = TILE,
        corners = CORNERS,
        dry = DRY,
        group = GROUP,
    )
}

/// The heightfield on the GPU: the atlas texture and its bind group (group [`GROUP`]), the shared
/// grid's index buffer (both windings), the water pipelines over it (blended, and reading the
/// scene), and the chunks' slots ([`HeightAtlas`]) with each tile's fluid and face.
pub struct HeightfieldGpu {
    texture: wgpu::Texture,
    pub group: wgpu::BindGroup,
    pub index: wgpu::Buffer,
    pub blended: wgpu::RenderPipeline,
    pub scene: wgpu::RenderPipeline,
    atlas: HeightAtlas<dwell_world::ChunkCoord>,
    tiles: std::collections::HashMap<dwell_world::ChunkCoord, (f32, bool)>,
}

/// One winding's index count in [`HeightfieldGpu::index`].
pub const GRID_INDICES: u32 = (COLUMNS * COLUMNS * 6) as u32;

/// A heightfield draw's `quat`: its tile's texel origin, its face (+1 A, −1 B) and its fluid.
pub fn draw_quat(slot: usize, fluid: f32, face_b: bool) -> [f32; 4] {
    let [x, y] = tile_origin(slot);
    [x as f32, y as f32, if face_b { -1.0 } else { 1.0 }, fluid]
}

/// The index range of a tile's face (face B's grid is re-wound).
pub fn grid_range(face_b: bool) -> std::ops::Range<u32> {
    if face_b {
        GRID_INDICES..2 * GRID_INDICES
    } else {
        0..GRID_INDICES
    }
}

/// What the water pipelines over the grid are built from.
pub struct PipelineParts<'a> {
    pub module: &'a wgpu::ShaderModule,
    pub frame_layout: &'a wgpu::BindGroupLayout,
    pub draw_layout: &'a wgpu::BindGroupLayout,
    pub under_layout: &'a wgpu::BindGroupLayout,
    pub hdr: wgpu::TextureFormat,
    pub view_depth: wgpu::TextureFormat,
    pub depth: wgpu::TextureFormat,
    pub depth_compare: wgpu::CompareFunction,
    pub samples: u32,
}

impl HeightfieldGpu {
    pub fn new(device: &wgpu::Device, p: &PipelineParts<'_>) -> Self {
        use wgpu::util::DeviceExt;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water heights"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE[0],
                height: ATLAS_SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water heights"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water heights"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water grid"),
            contents: bytemuck::cast_slice(&dwell_mesh::heightfield::grid_indices_both()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let pipeline = |scene: bool| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("water heightfield"),
                bind_group_layouts: &[
                    Some(p.frame_layout),
                    Some(p.draw_layout),
                    scene.then_some(p.under_layout),
                    Some(&layout),
                ],
                immediate_size: 0,
            });
            let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
            let blended_targets = [
                Some(wgpu::ColorTargetState {
                    format: p.hdr,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: p.view_depth,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                }),
            ];
            let scene_targets = [Some(wgpu::ColorTargetState {
                format: p.hdr,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if scene {
                    "heightfield water_scene_fs"
                } else {
                    "heightfield water_fs"
                }),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: p.module,
                    entry_point: Some("heightfield_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: p.module,
                    entry_point: Some(if scene { "water_scene_fs" } else { "water_fs" }),
                    compilation_options: Default::default(),
                    targets: if scene {
                        &scene_targets
                    } else {
                        &blended_targets
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: p.depth,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(p.depth_compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: p.samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            texture,
            group,
            index,
            blended: pipeline(false),
            scene: pipeline(true),
            atlas: HeightAtlas::new(),
            tiles: Default::default(),
        }
    }

    /// Chunk `coord`'s tile from its new meshes (none: dropped). A chunk past the atlas's room
    /// draws no surface.
    pub fn set(&mut self, coord: dwell_world::ChunkCoord, tile: Option<&WaterTile>) {
        match tile {
            Some(t) if self.atlas.slot(coord).is_some() || self.atlas.capacity() < MAX_TILES => {
                // Levels and lights interleaved, one rg32float texel per column.
                let texels = t
                    .levels
                    .iter()
                    .zip(&t.lights)
                    .flat_map(|(h, l)| [*h, *l])
                    .collect();
                self.atlas.set(coord, texels);
                self.tiles.insert(coord, (t.fluid, t.face_b));
            }
            _ => {
                self.atlas.remove(coord);
                self.tiles.remove(&coord);
            }
        }
    }

    /// Chunk `coord`'s draw `quat` and index range, if it has a tile.
    pub fn draw_of(
        &self,
        coord: dwell_world::ChunkCoord,
    ) -> Option<([f32; 4], std::ops::Range<u32>)> {
        let slot = self.atlas.slot(coord).filter(|s| *s < MAX_TILES)?;
        let (fluid, face_b) = *self.tiles.get(&coord)?;
        Some((draw_quat(slot, fluid, face_b), grid_range(face_b)))
    }

    /// Uploads the tiles changed since the last frame (`TILE`² × 8 bytes each); how many.
    pub fn upload(&mut self, queue: &wgpu::Queue) -> usize {
        let dirty = self.atlas.take_dirty();
        for (slot, levels) in &dirty {
            if *slot >= MAX_TILES {
                continue;
            }
            let [x, y] = tile_origin(*slot);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(levels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((TILE * 8) as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: TILE as u32,
                    height: TILE as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        dirty.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_module_carries_the_heightfield_and_validates() {
        let src = super::super::shaders::source(1);
        assert!(src.contains("fn heightfield_vs("));
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        assert!(
            module
                .entry_points
                .iter()
                .any(|p| p.name == "heightfield_vs")
        );
    }

    #[test]
    fn a_draw_names_its_tile_face_and_fluid() {
        assert_eq!(draw_quat(1, 3.0, false), [TILE as f32, 0.0, 1.0, 3.0]);
        assert_eq!(draw_quat(0, 0.0, true)[2], -1.0);
        assert_eq!(grid_range(false), 0..GRID_INDICES);
        assert_eq!(grid_range(true), GRID_INDICES..2 * GRID_INDICES);
    }

    #[test]
    fn tiles_pack_in_rows() {
        assert_eq!(tile_origin(0), [0, 0]);
        assert_eq!(tile_origin(1), [TILE as u32, 0]);
        assert_eq!(tile_origin(TILES_PER_ROW), [0, TILE as u32]);
        let last = tile_origin(MAX_TILES - 1);
        assert!(last[0] < ATLAS_SIZE[0] && last[1] < ATLAS_SIZE[1]);
    }
}
