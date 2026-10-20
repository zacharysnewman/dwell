//! Characters in the raster renderer (docs/CHARACTERS.md §9.1–9.2, Phase 40 C1): other players'
//! full bodies in the scene passes, and your own first-person arms in a pass of their own.
//!
//! - **Meshes**: one per look (or item) and view, built once by `dwell-characters` from the
//!   generated data, each vertex on one bone (rigid parts, no skinning). A character draws its
//!   body and its held item: two indexed draws over one block of bone transforms.
//! - **Shading** (`character_fs`): the material's ramp (shadow → base → light by a half-Lambert
//!   wrap of the key light), its brush tile at its `overlay` (painterly texture set only; the
//!   classic set draws the clean ramp), a faint warm rim, all scaled by the frame's light on that
//!   face so day and night reach characters as they reach terrain. The held block (`$block`) is
//!   its own atlas tiles lit like terrain. Fragments write the frame's colour and view depth, so
//!   the composite tone-maps, fogs, inks and edge-blurs them like everything else.
//! - **Outline**: a second, back-facing draw pushed out along the normals in `ink_soft`
//!   (an inverted hull), dropped past `OUTLINE_M`; small parts drop there too.
//! - **Arms**: drawn after the world's passes into the same targets over a cleared depth buffer,
//!   with their own projection (the data's FOV, a 2 cm near plane), so they never clip into
//!   terrain and keep their size under the speed FOV.

use dwell_characters::anim::{MAX_BONES, Pose, arm_transforms, bone_transforms};
use dwell_characters::math::Affine;
use dwell_characters::mesh::{self, Mesh};
use dwell_characters::{BLOCK_MATERIAL, Character, TABLES, TILE, View};
use dwell_world::hash::HashMap;

use super::camera::{Basis, V3};
use super::frame::{DEPTH, HDR, VIEW_DEPTH};

/// Past this (m) a character drops its outline and its small parts.
pub const OUTLINE_M: f64 = 64.0;
/// Past this (m) a character isn't drawn.
pub const DRAW_M: f64 = 512.0;
/// Parts smaller than this (m, their largest side) are left out of the far mesh.
pub const SMALL_PART_M: f32 = 0.06;
/// The outline's width (m) up close; it widens a little with distance to stay about a pixel.
pub const OUTLINE_WIDTH_M: f32 = 0.006;
/// The arms pass's near and far planes (m).
pub const ARMS_NEAR: f64 = 0.02;
pub const ARMS_FAR: f64 = 10.0;

/// Bytes of one instance's uniforms (bone rows, the held block, settings): a multiple of 256 for
/// the dynamic offset.
pub const INSTANCE_BYTES: u64 = 1280;
const INSTANCE_FLOATS: usize = INSTANCE_BYTES as usize / 4;
/// Floats of the bone rows (three rows of a 3 × 4 per bone).
const BONE_FLOATS: usize = MAX_BONES * 12;

/// What a hand holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Held {
    #[default]
    Empty,
    /// The hotbar's block (a client state id).
    Block(u16),
    /// The brush tool, painting with that block.
    Brush(u16),
}

impl Held {
    /// The item's name in the data (`block`, `brush`), and its block.
    pub fn item(self) -> Option<(&'static str, u16)> {
        match self {
            Held::Empty => None,
            Held::Block(b) => Some(("block", b)),
            Held::Brush(b) => Some(("brush", b)),
        }
    }
}

/// A full body to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyDraw {
    pub feet: V3,
    /// Degrees; 0 faces +z.
    pub yaw: f64,
    /// ±1: upside down on face B.
    pub face: f64,
    /// Lies on its back (the death pose).
    pub dead: bool,
    /// The look's name (unknown names draw the default look).
    pub look: &'static str,
    pub pose: Pose,
    pub held: Held,
}

/// The first-person arms to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmsDraw {
    pub pose: Pose,
    pub held: Held,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeshKey {
    /// A look's name, or an item's.
    name: &'static str,
    item: bool,
    view: View,
    /// The far mesh (small parts left out).
    far: bool,
}

struct GpuMesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    bytes: u64,
}

struct Draw {
    instance: u32,
    body: MeshKey,
    item: Option<MeshKey>,
    outline: bool,
    /// Bit per scene pass it is drawn in (far 1, near 2).
    passes: u8,
}

/// Per frame stats.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CharacterStats {
    pub bodies: u32,
    pub arms: bool,
    /// GPU bytes of the meshes built so far.
    pub mesh_bytes: u64,
    /// Bytes of instance uniforms written this frame.
    pub upload_bytes: u64,
}

pub struct CharacterRenderer {
    character: &'static Character,
    look: &'static str,
    pipeline: wgpu::RenderPipeline,
    outline_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    materials: wgpu::Buffer,
    tiles: wgpu::TextureView,
    sampler: wgpu::Sampler,
    instances: wgpu::Buffer,
    capacity: u64,
    group: wgpu::BindGroup,
    meshes: HashMap<MeshKey, GpuMesh>,
    draws: Vec<Draw>,
    arms: Option<Draw>,
    pub stats: CharacterStats,
}

/// The characters' WGSL, after the shared lighting (`shaders::lighting_source`).
pub const WGSL: &str = r"
struct Instance {
    // Each bone's 3 × 4 transform to eye-relative world space, as three rows (xyz the rotation's
    // row, w the translation's).
    bones: array<vec4f, BONE_ROWS>,
    // The held block's tiles (atlas u0, v0, width, height): its top, sides and bottom; its colour
    // (linear; w 1 when it has textures).
    block_top: vec4f,
    block_side: vec4f,
    block_bottom: vec4f,
    block_color: vec4f,
    // x the brush strokes (0 or 1, the texture set), y the outline's width (m), z 1 for the arms.
    cfg: vec4f,
};
struct Material {
    shadow: vec4f,
    base: vec4f,
    light: vec4f,
    // x overlay, y the brush tile's layer (−1 none)
    extra: vec4f,
};
@group(1) @binding(0) var<uniform> inst: Instance;
@group(1) @binding(1) var<storage, read> materials: array<Material>;
@group(1) @binding(2) var brush_tiles: texture_2d_array<f32>;
@group(1) @binding(3) var brush_sampler: sampler;

struct CharacterIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    // bone | side << 8 | material << 16
    @location(3) packed: u32,
};
struct CharacterOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) @interpolate(flat) packed: u32,
    @location(4) local: vec3f,
    @location(5) local_n: vec3f,
};

fn bone_apply(b: u32, p: vec3f, w: f32) -> vec3f {
    let q = vec4f(p, w);
    return vec3f(dot(inst.bones[b * 3u], q), dot(inst.bones[b * 3u + 1u], q), dot(inst.bones[b * 3u + 2u], q));
}

@vertex
fn character_vs(v: CharacterIn) -> CharacterOut {
    return character_vertex(v);
}

fn character_vertex(v: CharacterIn) -> CharacterOut {
    let bone = v.packed & 0xffu;
    var o: CharacterOut;
    o.rel = bone_apply(bone, v.position, 1.0);
    o.normal = bone_apply(bone, v.normal, 0.0);
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    o.uv = v.uv;
    o.packed = v.packed;
    o.local = v.position;
    o.local_n = v.normal;
    return o;
}

// The inverted hull: pushed out along the normal, drawn from behind.
@vertex
fn character_outline_vs(v: CharacterIn) -> CharacterOut {
    var o = character_vertex(v);
    let n = normalize(o.normal);
    let w = inst.cfg.y * (1.0 + length(o.rel) * 0.08);
    o.rel = o.rel + n * w;
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    return o;
}

// The key light on the fragment's face: the sun while it is up over that face, else the moon.
struct Key {
    dir: vec3f,
    // The light on a surface facing up there, / π (what lambert gives a white top face).
    level: vec3f,
};
fn key_light(frag_y: f32) -> Key {
    let on_a = frag_y >= MIDPLANE_Y;
    let d = frame.day;
    let up = vec3f(0.0, select(-1.0, 1.0, on_a), 0.0);
    var k: Key;
    k.dir = select(d.moon.xyz, d.sun.xyz, own(d.sun.y, on_a) > 0.01);
    k.level = lambert(vec3f(1.0), up, frag_y);
    return k;
}

fn brush_k(i: CharacterOut, layer: i32) -> f32 {
    var b = pow(abs(normalize(i.local_n)), vec3f(4.0));
    b = b / (b.x + b.y + b.z);
    let p = i.local / 0.32;
    let x = textureSampleLevel(brush_tiles, brush_sampler, p.zy, layer, 0.0).r;
    let y = textureSampleLevel(brush_tiles, brush_sampler, p.xz, layer, 0.0).r;
    let z = textureSampleLevel(brush_tiles, brush_sampler, p.xy, layer, 0.0).r;
    return (x * b.x + y * b.y + z * b.z) * 2.0;
}

@fragment
fn character_fs(i: CharacterOut, @builtin(front_facing) front: bool) -> SceneOut {
    let mat = i.packed >> 16u;
    let side = (i.packed >> 8u) & 0xffu;
    let n = normalize(select(-i.normal, i.normal, front));
    let frag_y = frame.eye.x + i.rel.y;
    var o: SceneOut;
    o.depth = frame_depth(view_depth(i.rel), true);
    if (mat == BLOCK_MATERIAL) {
        // The held block: its own tile on each side (top, bottom, sides), lit like terrain.
        let tile = select(select(inst.block_side, inst.block_bottom, side == 3u), inst.block_top, side == 2u);
        let uv = tile.xy + clamp(i.uv, vec2f(0.002), vec2f(0.998)) * tile.zw;
        let texel = textureSampleLevel(atlas, atlas_sampler, uv, 0.0);
        let has = inst.block_color.w > 0.5;
        if (has && texel.a < 0.5) {
            discard;
        }
        let albedo = inst.block_color.rgb * select(vec3f(1.0), texel.rgb, has);
        o.color = vec4f(lambert(albedo, n, frag_y), 1.0);
        return o;
    }
    let m = materials[mat];
    let key = key_light(frag_y);
    let w = dot(n, key.dir) * 0.5 + 0.5;
    var c = mix(m.shadow.rgb, m.base.rgb, smoothstep(0.16, 0.5, w));
    c = mix(c, m.light.rgb, smoothstep(0.66, 0.98, w) * 0.8);
    c = c * mix(0.9, 1.0, n.y * frame.eye.y * 0.5 + 0.5);
    let layer = i32(m.extra.y);
    if (layer >= 0 && inst.cfg.x > 0.5) {
        c = c * mix(1.0, brush_k(i, layer), m.extra.x);
    }
    let rim = pow(1.0 - max(dot(n, normalize(-i.rel)), 0.0), 3.0);
    c = c + m.light.rgb * rim * 0.14 * smoothstep(0.3, 0.6, w);
    o.color = vec4f(c * key.level * PI * CHARACTER_EXPOSURE, 1.0);
    return o;
}

@fragment
fn character_outline_fs(i: CharacterOut) -> SceneOut {
    var o: SceneOut;
    let level = key_light(frame.eye.x + i.rel.y).level * PI;
    o.color = vec4f(INK_SOFT * clamp(dot(level, vec3f(0.33)), 0.1, 1.0), 1.0);
    o.depth = frame_depth(view_depth(i.rel), true);
    return o;
}
";

/// How bright a character's ramp is drawn under the frame's light (a white face at midday shows its
/// ramp's colour at about this): tuned by eye against the terrain in Chrome.
pub const CHARACTER_EXPOSURE: f32 = 0.5;

/// The module's source: the shared lighting and the characters' entry points.
pub fn source() -> String {
    let ink = dwell_look::hex(0x43354a).map(|v| dwell_look::srgb_to_linear(v) as f32);
    format!(
        "{}\nconst BONE_ROWS: u32 = {}u;\nconst BLOCK_MATERIAL: u32 = {}u;\nconst CHARACTER_EXPOSURE: f32 = {:?};\nconst INK_SOFT: vec3f = vec3f({:?}, {:?}, {:?});\n{WGSL}",
        super::shaders::lighting_source(),
        MAX_BONES * 3,
        BLOCK_MATERIAL,
        CHARACTER_EXPOSURE,
        ink[0],
        ink[1],
        ink[2],
    )
}

/// A material table row (linear colours, overlay, tile).
fn material_rows() -> Vec<f32> {
    let lin = |c: u32| dwell_look::hex(c).map(|v| dwell_look::srgb_to_linear(v) as f32);
    let mut out = Vec::new();
    for m in TABLES.materials {
        for c in [m.shadow, m.base, m.light] {
            out.extend(lin(c));
            out.push(1.0);
        }
        out.extend([m.overlay, m.tile as f32, 0.0, 0.0]);
    }
    out
}

/// The interleaved vertices of a mesh (position, normal, uv, packed) and its indices.
pub fn vertex_bytes(m: &Mesh) -> (Vec<u8>, Vec<u8>) {
    let mut v = Vec::with_capacity(m.vertex_count() * mesh::VERTEX_BYTES);
    for i in 0..m.vertex_count() {
        for f in m.positions[i].iter().chain(&m.normals[i]).chain(&m.uvs[i]) {
            v.extend(f.to_le_bytes());
        }
        let packed =
            u32::from(m.bones[i]) | u32::from(m.faces[i]) << 8 | u32::from(m.materials[i]) << 16;
        v.extend(packed.to_le_bytes());
    }
    let idx = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    (v, idx)
}

/// Rows of an affine for the shader (row k: the rotation's row k and the translation's k).
fn rows(a: &Affine) -> [f32; 12] {
    let m = &a.m;
    [
        m[0][0], m[1][0], m[2][0], a.t[0], m[0][1], m[1][1], m[2][1], a.t[1], m[0][2], m[1][2],
        m[2][2], a.t[2],
    ]
}

/// A body's frame in eye-relative world space: turned to its yaw, upside down on face B (a half
/// turn about its forward axis, so it stays a proper rotation), lying on its back when dead.
pub fn body_frame(d: &BodyDraw, eye: V3) -> Affine {
    let (s, c) = (
        d.yaw.to_radians().sin() as f32,
        d.yaw.to_radians().cos() as f32,
    );
    let f = if d.face < 0.0 { -1.0 } else { 1.0 };
    let place = Affine {
        m: [[f * c, 0.0, -f * s], [0.0, f, 0.0], [s, 0.0, c]],
        t: std::array::from_fn(|k| (d.feet[k] - eye[k]) as f32),
    };
    if d.dead {
        place
            .then(&Affine::translation([0.0, 0.18, 0.0]))
            .then(&Affine::euler([-90.0, 0.0, 0.0]))
    } else {
        place
    }
}

/// The arms' frame: view space (+x left, +y up, +z ahead) to eye-relative world space.
pub fn arms_frame(b: &Basis) -> Affine {
    let r = |v: V3| v.map(|c| c as f32);
    let (right, up, fwd) = (r(b.right), r(b.up), r(b.forward()));
    Affine {
        m: [right.map(|c| -c), up, fwd],
        t: [0.0; 3],
    }
}

/// A block's atlas tiles (top, side, bottom) and colour for the shader: `[top, side, bottom,
/// colour]`.
pub fn block_uniforms(block: u16) -> [[f32; 4]; 4] {
    let Some(info) = dwell_blocks::try_info(block) else {
        return [[0.0; 4], [0.0; 4], [0.0; 4], [1.0, 0.0, 1.0, 0.0]];
    };
    let rect = |name: &str| {
        let t = dwell_mesh::tile_rect(name);
        [
            t.u0 as f32,
            t.v0 as f32,
            (t.u1 - t.u0) as f32,
            (t.v1 - t.v0) as f32,
        ]
    };
    match info.textures {
        Some(t) => [
            rect(t.top),
            rect(t.side),
            rect(t.bottom),
            [1.0, 1.0, 1.0, 1.0],
        ],
        None => {
            let c = dwell_look::hex(info.color).map(|v| dwell_look::srgb_to_linear(v) as f32);
            [[0.0; 4], [0.0; 4], [0.0; 4], [c[0], c[1], c[2], 0.0]]
        }
    }
}

impl CharacterRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
        samples: u32,
    ) -> Self {
        let character = dwell_characters::player();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("characters"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("characters"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(INSTANCE_BYTES),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("characters"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 32,
                shader_location: 3,
            },
        ];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: mesh::VERTEX_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let targets = [
            Some(wgpu::ColorTargetState {
                format: HDR,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: VIEW_DEPTH,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ];
        let make = |vs: &str, fs: &str, cull: wgpu::Face| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pipeline_layout),
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
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(cull),
                    ..Default::default()
                },
                depth_stencil: Some(depth_state()),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make("character_vs", "character_fs", wgpu::Face::Back);
        let outline_pipeline = make(
            "character_outline_vs",
            "character_outline_fs",
            wgpu::Face::Front,
        );
        let rows = material_rows();
        let materials = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("character materials"),
            size: (rows.len() * 4).max(64) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&materials, 0, bytemuck::cast_slice(&rows));
        let tiles = brush_tiles(device, queue);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("brush tiles"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let capacity = 8;
        let instances = instance_buffer(device, capacity);
        let group = bind_group(device, &layout, &instances, &materials, &tiles, &sampler);
        Self {
            character,
            look: dwell_characters::DEFAULT_LOOK,
            pipeline,
            outline_pipeline,
            layout,
            materials,
            tiles,
            sampler,
            instances,
            capacity,
            group,
            meshes: HashMap::default(),
            draws: Vec::new(),
            arms: None,
            stats: CharacterStats::default(),
        }
    }

    fn mesh(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, key: MeshKey) {
        if self.meshes.contains_key(&key) {
            return;
        }
        let parts = if key.item {
            self.character.item(key.name).map_or(&[][..], |i| i.parts)
        } else {
            self.character.look(key.name).map_or(&[][..], |l| l.parts)
        };
        let chain = self.character.first_person.map_or(&[][..], |f| f.bones);
        let kept: Vec<_> = parts
            .iter()
            .copied()
            .filter(|p| !key.far || p.size.iter().copied().fold(0.0, f32::max) >= SMALL_PART_M)
            .collect();
        let m = mesh::build(&kept, key.view, chain);
        let (v, i) = vertex_bytes(&m);
        let buffer = |usage, data: &[u8], label| {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (data.len() as u64).max(4).next_multiple_of(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if !data.is_empty() {
                queue.write_buffer(&b, 0, data);
            }
            b
        };
        let g = GpuMesh {
            vertex: buffer(wgpu::BufferUsages::VERTEX, &v, "character vertices"),
            index: buffer(wgpu::BufferUsages::INDEX, &i, "character indices"),
            count: m.indices.len() as u32,
            bytes: (v.len() + i.len()) as u64,
        };
        self.stats.mesh_bytes += g.bytes;
        self.meshes.insert(key, g);
    }

    /// This frame's characters: the bodies (scene passes `passes`: each one's near and far view
    /// depth) and the arms. Builds what meshes are new and writes every instance's uniforms.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: V3,
        basis: &Basis,
        passes: &[(f64, f64)],
        bodies: &[BodyDraw],
        arms: Option<&ArmsDraw>,
        strokes: bool,
    ) {
        self.draws.clear();
        self.arms = None;
        let mut data: Vec<f32> = Vec::new();
        let mut push = |bones: &[Affine; MAX_BONES], held: Held, outline: bool, is_arms: bool| {
            let at = data.len();
            data.resize(at + INSTANCE_FLOATS, 0.0);
            for (k, b) in bones.iter().enumerate() {
                data[at + k * 12..at + k * 12 + 12].copy_from_slice(&rows(b));
            }
            let block = held.item().map(|(_, b)| b).unwrap_or(0);
            let u = block_uniforms(block);
            for (k, v) in u.iter().enumerate() {
                data[at + BONE_FLOATS + k * 4..at + BONE_FLOATS + k * 4 + 4].copy_from_slice(v);
            }
            let cfg = at + BONE_FLOATS + 16;
            data[cfg..cfg + 4].copy_from_slice(&[
                f32::from(u8::from(strokes)),
                if outline { OUTLINE_WIDTH_M } else { 0.0 },
                f32::from(u8::from(is_arms)),
                0.0,
            ]);
            (at / INSTANCE_FLOATS) as u32
        };
        let mut keys = Vec::new();
        for d in bodies {
            let dist = (0..3)
                .map(|k| (d.feet[k] - eye[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            if dist > DRAW_M {
                continue;
            }
            let mut mask = 0u8;
            for (i, (near, far)) in passes.iter().enumerate() {
                if dist + 3.0 >= *near && dist - 3.0 <= *far {
                    mask |= 1 << i;
                }
            }
            if mask == 0 {
                continue;
            }
            let far = dist > OUTLINE_M;
            let frame = body_frame(d, eye);
            let bones = bone_transforms(self.character, &d.pose).map(|b| frame.then(&b));
            let instance = push(&bones, d.held, !far, false);
            let look = if self.character.look(d.look).is_some() {
                d.look
            } else {
                self.look
            };
            let body = MeshKey {
                name: look,
                item: false,
                view: View::Body,
                far,
            };
            let item = d.held.item().map(|(name, _)| MeshKey {
                name,
                item: true,
                view: View::Body,
                far,
            });
            keys.push(body);
            keys.extend(item);
            self.draws.push(Draw {
                instance,
                body,
                item,
                outline: !far,
                passes: mask,
            });
        }
        if let Some(a) = arms {
            let frame = arms_frame(basis);
            let bones = arm_transforms(self.character, &a.pose).map(|b| frame.then(&b));
            let instance = push(&bones, a.held, true, true);
            let body = MeshKey {
                name: self.look,
                item: false,
                view: View::Arms,
                far: false,
            };
            let item = a.held.item().map(|(name, _)| MeshKey {
                name,
                item: true,
                view: View::Arms,
                far: false,
            });
            keys.push(body);
            keys.extend(item);
            self.arms = Some(Draw {
                instance,
                body,
                item,
                outline: true,
                passes: 0,
            });
        }
        for k in keys {
            self.mesh(device, queue, k);
        }
        let count = (data.len() / INSTANCE_FLOATS) as u64;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.instances.destroy();
            self.instances = instance_buffer(device, self.capacity);
            self.group = bind_group(
                device,
                &self.layout,
                &self.instances,
                &self.materials,
                &self.tiles,
                &self.sampler,
            );
        }
        if !data.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&data));
        }
        self.stats.bodies = self.draws.len() as u32;
        self.stats.arms = self.arms.is_some();
        self.stats.upload_bytes = data.len() as u64 * 4;
    }

    /// Whether the arms are drawn this frame (the renderer adds their pass).
    pub fn has_arms(&self) -> bool {
        self.arms.is_some()
    }

    fn record(&self, pass: &mut wgpu::RenderPass<'_>, d: &Draw, calls: &mut u32, tris: &mut u64) {
        let offset = d.instance * INSTANCE_BYTES as u32;
        pass.set_bind_group(1, &self.group, &[offset]);
        let meshes: Vec<&GpuMesh> = [Some(d.body), d.item]
            .into_iter()
            .flatten()
            .filter_map(|k| self.meshes.get(&k))
            .filter(|m| m.count > 0)
            .collect();
        let mut draw = |pipeline: &wgpu::RenderPipeline| {
            pass.set_pipeline(pipeline);
            for m in &meshes {
                pass.set_vertex_buffer(0, m.vertex.slice(..));
                pass.set_index_buffer(m.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
                *calls += 1;
                *tris += u64::from(m.count / 3);
            }
        };
        draw(&self.pipeline);
        if d.outline {
            draw(&self.outline_pipeline);
        }
    }

    /// Draws the bodies of scene pass `i` (group 0 bound to its frame). Leaves group 1 bound to
    /// the characters' group: the caller binds its own again after.
    pub fn draw_bodies(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        i: usize,
        calls: &mut u32,
        tris: &mut u64,
    ) -> bool {
        let mut any = false;
        for d in self.draws.iter().filter(|d| d.passes & (1 << i) != 0) {
            self.record(pass, d, calls, tris);
            any = true;
        }
        any
    }

    /// Draws the arms (group 0 bound to the arms' frame, over a cleared depth buffer).
    pub fn draw_arms(&self, pass: &mut wgpu::RenderPass<'_>, calls: &mut u32, tris: &mut u64) {
        if let Some(d) = &self.arms {
            self.record(pass, d, calls, tris);
        }
    }
}

/// How the arms pass treats the frame (CHARACTERS.md §9.1): after `world_passes` scene passes, it
/// loads their colour and view depth (it draws over the world), **clears the depth buffer** (so
/// nothing of the world's depth hides or cuts the arms: they never clip into a wall), and resolves
/// the colour for the composite as the last pass does. The world's passes are planned as
/// `scene_pass_ops(i, world_passes + 1, …)` with it, so the last of them keeps its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmsPass {
    pub ops: super::frame::PassOps,
    pub depth: wgpu::LoadOp<f32>,
}

pub fn arms_pass(world_passes: usize, samples: u32) -> ArmsPass {
    ArmsPass {
        ops: super::frame::scene_pass_ops(world_passes, world_passes + 1, samples),
        depth: wgpu::LoadOp::Clear(1.0),
    }
}

/// The characters' depth test (bodies in the scene passes, the arms in theirs): ordinary
/// less-or-equal with writes; the arms pass starts from a cleared buffer (`arms_pass`).
pub fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("character instances"),
        size: capacity * INSTANCE_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    instances: &wgpu::Buffer,
    materials: &wgpu::Buffer,
    tiles: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("characters"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: instances,
                    offset: 0,
                    size: wgpu::BufferSize::new(INSTANCE_BYTES),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: materials.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(tiles),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// The brush tiles as an `r8unorm` array texture (one layer per tile; at least one).
fn brush_tiles(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let layers = TABLES.tile_count().max(1) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("brush tiles"),
        size: wgpu::Extent3d {
            width: TILE as u32,
            height: TILE as u32,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = if TABLES.tiles.is_empty() {
        vec![128; TILE * TILE]
    } else {
        TABLES.tiles.to_vec()
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(TILE as u32),
            rows_per_image: Some(TILE as u32),
        },
        wgpu::Extent3d {
            width: TILE as u32,
            height: TILE as u32,
            depth_or_array_layers: layers,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_character_module_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(&source()).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for e in [
            "character_vs",
            "character_fs",
            "character_outline_vs",
            "character_outline_fs",
        ] {
            assert!(module.entry_points.iter().any(|x| x.name == e), "{e}");
        }
    }

    /// The arms never test against the world's depth: their pass clears the depth buffer, draws
    /// over the world's colour and view depth, and is the pass that resolves; the world's last
    /// pass keeps its colour for it instead of resolving.
    #[test]
    fn the_arms_pass_clears_depth_and_draws_over_the_world() {
        use super::super::frame::scene_pass_ops;
        for samples in [1, 4] {
            let a = arms_pass(2, samples);
            assert_eq!(a.depth, wgpu::LoadOp::Clear(1.0));
            assert!(!a.ops.clear, "loads the world's colour and view depth");
            assert_eq!(a.ops.resolve, samples > 1);
            let world_last = scene_pass_ops(1, 3, samples);
            assert!(!world_last.resolve && world_last.store_colour);
        }
        assert_eq!(
            depth_state().depth_compare,
            Some(wgpu::CompareFunction::LessEqual)
        );
    }

    #[test]
    fn a_body_faces_its_yaw_and_hangs_upside_down_on_face_b() {
        let mut d = BodyDraw {
            feet: [10.0, 5.0, 0.0],
            yaw: 90.0,
            face: 1.0,
            dead: false,
            look: dwell_characters::DEFAULT_LOOK,
            pose: Pose::REST,
            held: Held::Empty,
        };
        let f = body_frame(&d, [0.0; 3]);
        // Yaw 90° faces +x; the head is up.
        let ahead = f.rotate([0.0, 0.0, 1.0]);
        assert!((ahead[0] - 1.0).abs() < 1e-5, "{ahead:?}");
        assert!((f.apply([0.0, 1.8, 0.0])[1] - 6.8).abs() < 1e-4);
        d.face = -1.0;
        let b = body_frame(&d, [0.0; 3]);
        assert!(
            (b.apply([0.0, 1.8, 0.0])[1] - 3.2).abs() < 1e-4,
            "hangs toward −y"
        );
        assert!(
            (b.rotate([0.0, 0.0, 1.0])[0] - 1.0).abs() < 1e-5,
            "still faces its yaw"
        );
        // A proper rotation (no mirror): the winding stays front-facing.
        let m = b.m;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2])
            - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
        assert!((det - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_arms_sit_ahead_of_the_eye_whatever_the_view() {
        let fp = dwell_characters::player().first_person.unwrap();
        for (yaw, pitch, face) in [(0.0, 0.0, 1.0), (135.0, -40.0, 1.0), (20.0, 30.0, -1.0)] {
            let b = super::super::camera::aim(yaw, pitch, face, 0.0);
            let a = arms_frame(&b);
            let p = a.apply(fp.anchor);
            let f = b.forward();
            let along = p[0] as f64 * f[0] + p[1] as f64 * f[1] + p[2] as f64 * f[2];
            assert!((along - f64::from(fp.anchor[2])).abs() < 1e-5);
            // +x in view space is the left: the anchor's −x lies to the camera's right.
            let r = b.right;
            let right = p[0] as f64 * r[0] + p[1] as f64 * r[1] + p[2] as f64 * r[2];
            assert!((right + f64::from(fp.anchor[0])).abs() < 1e-5);
        }
    }
}
