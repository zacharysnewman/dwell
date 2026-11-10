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

use dwell_mesh::heightfield::{
    COLUMNS, DRY, HeightAtlas, LIGHT_SPAN, SIDES, TILE, WaterTile, column_corners, skirt_edge,
};

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
/// id in `quat.w`; the grid is drawn unindexed, six vertices a column, and a dry column's six
/// collapse to one point outside the view (no pixels shaded under the ground or past the shore).
pub fn wgsl() -> String {
    let join = dwell_look::water_tiers::HEIGHTFIELD.join_m as f32;
    let chunk_h = (f64::from(dwell_world::CHUNK_SIZE) * dwell_world::CELL_H) as f32;
    let cell_h = dwell_world::CELL_H as f32;
    format!(
        "\n\
const HF_TILE: i32 = {tile};\n\
const HF_COLUMNS: u32 = {columns}u;\n\
const HF_DRY: f32 = {dry:?};\n\
const HF_JOIN_M: f32 = {join:?};\n\
const HF_CHUNK_H: f32 = {chunk_h:?};\n\
const HF_CELL_H: f32 = {cell_h:?};\n\
const HF_LIGHT_SPAN: f32 = {span:?};\n\
const HF_GRID_VERTICES: u32 = {grid}u;\n\
@group({group}) @binding(0) var hf_heights: texture_2d<f32>;\n\
\n\
fn hf_level(tile: vec2i, c: vec2i) -> f32 {{\n\
    return textureLoad(hf_heights, tile + clamp(c, vec2i(0), vec2i(HF_TILE - 1)), 0).r;\n\
}}\n\
// A column's flood light (sky, block as shares of 15), packed sky × 16 + block under\n\
// HF_LIGHT_SPAN, its water's floor row × HF_LIGHT_SPAN over it (dwell-mesh `pack_column`).\n\
fn hf_packed(tile: vec2i, c: vec2i) -> f32 {{\n\
    return textureLoad(hf_heights, tile + clamp(c, vec2i(0), vec2i(HF_TILE - 1)), 0).g;\n\
}}\n\
fn hf_light(tile: vec2i, c: vec2i) -> vec2f {{\n\
    let q = hf_packed(tile, c);\n\
    let p = q - HF_LIGHT_SPAN * floor(q / HF_LIGHT_SPAN);\n\
    return vec2f(floor(p / 16.0), p - 16.0 * floor(p / 16.0)) / 15.0;\n\
}}\n\
// A column's water floor, face-local metres (dwell-mesh `column_floor`).\n\
fn hf_floor(tile: vec2i, c: vec2i) -> f32 {{\n\
    return floor(hf_packed(tile, c) / HF_LIGHT_SPAN) * HF_CELL_H;\n\
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
// Skirt vertex `index` (past the grid's, dwell-mesh heightfield.rs `skirt_vertex`, the\n\
// reference): side `side` of its column, the vertical quad from the surface's edge down to the\n\
// neighbour's lower water or the column's floor (`skirt_bottom`); collapsed where none shows.\n\
fn hf_skirt(tile: vec2i, index: u32, o: ptr<function, FluidOut>) -> bool {{\n\
    let column = vec2i(i32((index / 24u) % HF_COLUMNS), i32((index / 24u) / HF_COLUMNS));\n\
    let side = (index / 6u) % 4u;\n\
    let sides = array<vec2i, 4>({sides});\n\
    let edges = array<vec2i, 8>({edges});\n\
    let own = column + vec2i(1);\n\
    let level = hf_level(tile, own);\n\
    let beside = hf_level(tile, own + sides[side]);\n\
    if !hf_wet(level) || (hf_wet(beside) && level - beside <= HF_JOIN_M) {{ return false; }}\n\
    var bottom = hf_floor(tile, own);\n\
    if hf_wet(beside) {{ bottom = max(bottom, beside); }}\n\
    if bottom >= level {{ return false; }}\n\
    let k = index % 6u;\n\
    let which = array<u32, 6>(0u, 1u, 1u, 0u, 1u, 0u);\n\
    let up = array<bool, 6>(true, true, false, true, false, false);\n\
    let corner = column + edges[side * 2u + which[k]];\n\
    let top = hf_corner(tile, corner);\n\
    var h = top;\n\
    if !up[k] {{ h = min(bottom, top); }}\n\
    (*o).normal = vec3f(f32(sides[side].x), 0.0, f32(sides[side].y));\n\
    (*o).flood = hf_light(tile, own);\n\
    (*o).wrapped = vec2f(f32(corner.x), f32(corner.y));\n\
    (*o).rel = vec3f(f32(corner.x), h, f32(corner.y));\n\
    return true;\n\
}}\n\
\n\
@vertex\n\
fn heightfield_vs(@builtin(vertex_index) index: u32, @builtin(instance_index) k: u32) -> FluidOut {{\n\
    let draw = draws[k];\n\
    let tile = vec2i(draw.quat.xy);\n\
    let face = draw.quat.z;\n\
    // Six unshared vertices per column (dwell-mesh heightfield.rs `grid_vertex`, the reference):\n\
    // a dry column's collapse to one point outside the view, so no fragment of it is shaded and\n\
    // the surface ends at the shoreline's column edge.\n\
    let column = vec2i(i32((index / 6u) % HF_COLUMNS), i32((index / 6u) / HF_COLUMNS));\n\
    var o: FluidOut;\n\
    o.draw = k;\n\
    o.color = vec3f(1.0);\n\
    o.fluid = u32(draw.quat.w + 0.5);\n\
    if index >= HF_GRID_VERTICES {{\n\
        // The skirts: placed face-local in `rel`, then into the world as the grid's corners.\n\
        if !hf_skirt(tile, index - HF_GRID_VERTICES, &o) {{\n\
            o.clip = vec4f(2.0, 2.0, 2.0, 1.0);\n\
            return o;\n\
        }}\n\
        let p = o.rel;\n\
        // face-boundary: as the grid's corners below.\n\
        let sy = select(HF_CHUNK_H - p.y, p.y, face > 0.0);\n\
        let local = vec3f(p.x, sy, p.z);\n\
        o.rel = draw.offset.xyz + local * draw.offset.w;\n\
        o.clip = frame.view_proj * vec4f(o.rel, 1.0);\n\
        o.wrapped = draw.rotation.zw + o.wrapped * draw.offset.w;\n\
        return o;\n\
    }}\n\
    if !hf_wet(hf_level(tile, column + vec2i(1))) {{\n\
        o.clip = vec4f(2.0, 2.0, 2.0, 1.0);\n\
        return o;\n\
    }}\n\
    var offsets = array<vec2i, 6>({offsets_a});\n\
    if face < 0.0 {{ offsets = array<vec2i, 6>({offsets_b}); }}\n\
    let corner = column + offsets[index % 6u];\n\
    let h = hf_corner(tile, corner);\n\
    let n = hf_normal(tile, corner, h);\n\
    // face-boundary: the face-local chunk turned back into world rows, as the chunk mesher's\n\
    // `flip_arrays_y` turns a face-B chunk's mesh (the draw's offset is the chunk's minimum).\n\
    let y = select(HF_CHUNK_H - h, h, face > 0.0);\n\
    let local = vec3f(f32(corner.x), y, f32(corner.y));\n\
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
        columns = COLUMNS,
        dry = DRY,
        span = LIGHT_SPAN,
        grid = dwell_mesh::heightfield::GRID_VERTICES,
        sides = SIDES
            .iter()
            .map(|[x, z]| format!("vec2i({x}, {z})"))
            .collect::<Vec<_>>()
            .join(", "),
        edges = (0..SIDES.len())
            .flat_map(skirt_edge)
            .map(|[x, z]| format!("vec2i({x}, {z})"))
            .collect::<Vec<_>>()
            .join(", "),
        group = GROUP,
        offsets_a = wgsl_offsets(false),
        offsets_b = wgsl_offsets(true),
    )
}

/// A column's six corner offsets as WGSL `vec2i`s (`column_corners`: face B's re-wound).
fn wgsl_offsets(face_b: bool) -> String {
    column_corners(face_b)
        .iter()
        .map(|[x, z]| format!("vec2i({x}, {z})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The heightfield on the GPU: the atlas texture and its bind group (group [`GROUP`]), the water
/// pipelines over the unindexed grid (blended, and reading the scene), and the chunks' slots
/// ([`HeightAtlas`]) with each tile's fluid and face.
pub struct HeightfieldGpu {
    texture: wgpu::Texture,
    pub group: wgpu::BindGroup,
    pub blended: wgpu::RenderPipeline,
    pub scene: wgpu::RenderPipeline,
    atlas: HeightAtlas<dwell_world::ChunkCoord>,
    tiles: std::collections::HashMap<dwell_world::ChunkCoord, (f32, bool)>,
}

/// One draw's vertices: the grid's (six a column, unshared; `dwell_mesh::heightfield::grid_vertex`)
/// then the skirts' (24 a column; `skirt_vertex`).
pub const GRID_VERTICES: u32 = dwell_mesh::heightfield::DRAW_VERTICES as u32;

/// A heightfield draw's `quat`: its tile's texel origin, its face (+1 A, −1 B; face B's columns
/// are re-wound in the shader) and its fluid.
pub fn draw_quat(slot: usize, fluid: f32, face_b: bool) -> [f32; 4] {
    let [x, y] = tile_origin(slot);
    [x as f32, y as f32, if face_b { -1.0 } else { 1.0 }, fluid]
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

    /// Chunk `coord`'s draw `quat`, if it has a tile.
    pub fn draw_of(&self, coord: dwell_world::ChunkCoord) -> Option<[f32; 4]> {
        let slot = self.atlas.slot(coord).filter(|s| *s < MAX_TILES)?;
        let (fluid, face_b) = *self.tiles.get(&coord)?;
        Some(draw_quat(slot, fluid, face_b))
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
    }

    #[test]
    fn the_shader_winds_each_face_as_the_reference_grid() {
        // The WGSL's offset tables are `column_corners`' (face B's re-wound), so the vertex shader
        // and `dwell_mesh::heightfield::grid_vertex` place the same corners.
        let src = wgsl();
        assert!(src.contains(&format!("array<vec2i, 6>({})", wgsl_offsets(false))));
        assert!(src.contains(&format!("array<vec2i, 6>({})", wgsl_offsets(true))));
        assert_ne!(wgsl_offsets(false), wgsl_offsets(true));
        assert!(
            src.contains("o.clip = vec4f(2.0, 2.0, 2.0, 1.0)"),
            "a dry column collapses"
        );
        // The skirts' sides and edges are the reference's (`skirt_vertex`), after the grid's.
        assert!(
            src.contains("array<vec2i, 4>(vec2i(-1, 0), vec2i(1, 0), vec2i(0, -1), vec2i(0, 1))")
        );
        assert!(src.contains("array<vec2i, 8>(vec2i(0, 0), vec2i(0, 1), vec2i(1, 0), vec2i(1, 1)"));
        assert_eq!(
            GRID_VERTICES as usize,
            dwell_mesh::heightfield::GRID_VERTICES + dwell_mesh::heightfield::SKIRT_VERTICES
        );
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
