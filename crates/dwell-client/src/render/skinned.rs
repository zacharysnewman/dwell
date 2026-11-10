//! Skinned bodies in the raster renderer (docs/CHARACTERS.md §11, ADR 0070): other players and your
//! shadow drawn as the player's skinned model (`dwell_models::player`, the Peasant), its clips
//! chosen by the body's role and cross-faded (`dwell_models::anim::Player`).
//!
//! - **Mesh**: the model's one mesh, uploaded once: position, normal, its palette colour (sRGB
//!   bytes, one per face from the import's remap) and four joints and weights a vertex.
//! - **Joints**: each body's joint matrices (the clip's pose × the inverse bind matrices) times its
//!   frame (`characters::place_frame`, upside down on face B) and the model's scale and yaw, as
//!   three rows each in the body's instance uniforms; the vertex shader sums its four joints'.
//! - **Shading** (`skinned_fs`): the characters' ramp (cool shadow → colour → warm light by a
//!   half-Lambert wrap of the key light), built from the vertex's palette colour, lit by the frame's
//!   flood light, sun or moon, shadow maps and point lights as the rigid characters are. No texture
//!   and no per-pixel cost beyond the rigid characters' (no CPU-adapter tier needed).
//! - **Shadows**: the bodies cast into the shadow maps as the rigid characters do; your own body
//!   (`cast_only`, the shadow you see in first person) casts into the nearest cascade only.
//! - **Held items** (§11.5): [`hand_frame`] gives the characters rig's hand bone's frame on the
//!   model's hand joint (`hand` in models.json) this frame; the rigid renderer draws the item there.

use dwell_characters::math::Affine;
use dwell_models::anim::{self, Player};
use dwell_models::glb::Model;
use dwell_models::math::{self as mm, Mat4};
use dwell_models::{MAX_JOINTS, ModelData};
use dwell_world::FaceFrame;
use dwell_world::hash::HashMap;

use super::camera::V3;
use super::characters::{CHARACTER_EXPOSURE, DRAW_M, depth_state, place_frame};
use super::frame::{HDR, VIEW_DEPTH};

/// Bytes of one body's uniforms (the joint rows and its light): a multiple of 256 for the dynamic
/// offset.
pub const INSTANCE_BYTES: u64 = 3328;
const INSTANCE_FLOATS: usize = INSTANCE_BYTES as usize / 4;
const JOINT_FLOATS: usize = MAX_JOINTS * 12;
/// Bytes of a vertex: position, normal, colour (rgba8), joints (4 × u8), weights (4 × unorm8).
pub const VERTEX_BYTES: usize = 36;

/// A skinned body to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinnedDraw {
    /// Its player's id (a creature's id): its clip player's key, with its model.
    pub seed: u32,
    /// Its model's name (shared/models/models.json): the player's, or a creature's (ADR 0073).
    pub model: &'static str,
    pub feet: V3,
    /// Degrees; 0 faces +z.
    pub yaw: f64,
    pub face: FaceFrame,
    pub dead: bool,
    /// The base role (`dwell_characters::animator::body_role`'s names).
    pub role: &'static str,
    /// The flood light it stands in (as `BodyDraw::light`).
    pub light: [f32; 2],
    /// Drawn into the shadow maps only (your own body: your shadow).
    pub cast_only: bool,
}

/// A model-space matrix as a rigid frame: its rotation's columns made orthonormal (the model's
/// scale taken out), its translation kept.
pub fn rigid(m: &Mat4) -> Affine {
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        v.map(|x| x / l)
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let col = |k: usize| [m[k][0], m[k][1], m[k][2]];
    let x = norm(col(0));
    let y = col(1);
    let y = norm(std::array::from_fn(|k| y[k] - dot(x, y) * x[k]));
    let z = col(2);
    let z = norm(std::array::from_fn(|k| {
        z[k] - dot(x, z) * x[k] - dot(y, z) * y[k]
    }));
    Affine {
        m: [x, y, z],
        t: [m[3][0], m[3][1], m[3][2]],
    }
}

/// The characters rig's hand bone's frame on the model's hand joint under `globals` (each node's
/// model-space transform, [`anim::globals`]), through `place` ([`body_matrix`]): where a held
/// item is drawn. None: the model holds nothing.
pub fn hand_frame(
    data: &ModelData,
    model: &Model,
    globals: &[Mat4],
    place: &Mat4,
) -> Option<Affine> {
    let h = data.hand?;
    let node = *model.skin.get(h.joint)?;
    let joint = rigid(&mm::mul(place, globals.get(node)?));
    Some(
        joint
            .then(&Affine::translation(h.at))
            .then(&Affine::euler(h.rot)),
    )
}

/// The body's model space to eye-relative world space: its place (upside down on face B), the
/// model's yaw and scale.
pub fn body_matrix(d: &SkinnedDraw, data: &ModelData, eye: V3) -> Mat4 {
    let place: Affine = place_frame(d.feet, d.yaw, d.face, d.dead, eye);
    let p: Mat4 = [
        [place.m[0][0], place.m[0][1], place.m[0][2], 0.0],
        [place.m[1][0], place.m[1][1], place.m[1][2], 0.0],
        [place.m[2][0], place.m[2][1], place.m[2][2], 0.0],
        [place.t[0], place.t[1], place.t[2], 1.0],
    ];
    let turn = mm::axis_angle([0.0, 1.0, 0.0], data.yaw);
    mm::mul(&p, &mm::trs([0.0; 3], turn, [data.scale; 3]))
}

/// The interleaved vertices of a model's detail level `level` (ADR 0070 §10) and its indices.
pub fn vertex_bytes(model: &Model, level: usize) -> (Vec<u8>, Vec<u8>) {
    let m = model.level(level);
    let mut v = Vec::with_capacity(m.positions.len() * VERTEX_BYTES);
    for i in 0..m.positions.len() {
        for f in m.positions[i].iter().chain(&m.normals[i]) {
            v.extend(f.to_le_bytes());
        }
        let c = m.colors[i];
        v.extend([
            (c[0] * 255.0).round() as u8,
            (c[1] * 255.0).round() as u8,
            (c[2] * 255.0).round() as u8,
            255,
        ]);
        v.extend(m.joints[i]);
        v.extend(m.weights[i].map(|w| (w * 255.0).round() as u8));
    }
    let idx = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    (v, idx)
}

/// A matrix's top three rows for the shader.
fn rows(m: &Mat4) -> [f32; 12] {
    std::array::from_fn(|k| m[k % 4][k / 4])
}

pub const WGSL: &str = r"
struct SkinInstance {
    // Each joint's 3 × 4 matrix to eye-relative world space, as three rows.
    joints: array<vec4f, JOINT_ROWS>,
    // x sky, y block level / 15 (docs/DAY_NIGHT.md §11).
    light: vec4f,
};
@group(1) @binding(0) var<uniform> skin: SkinInstance;

struct SkinnedIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec4f,
    @location(3) joints: vec4u,
    @location(4) weights: vec4f,
};
struct SkinnedOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
};

fn joint_apply(j: u32, p: vec3f, w: f32) -> vec3f {
    let q = vec4f(p, w);
    return vec3f(dot(skin.joints[j * 3u], q), dot(skin.joints[j * 3u + 1u], q), dot(skin.joints[j * 3u + 2u], q));
}

@vertex
fn skinned_vs(v: SkinnedIn) -> SkinnedOut {
    var p = vec3f(0.0);
    var n = vec3f(0.0);
    for (var k = 0u; k < 4u; k++) {
        let w = v.weights[k];
        if (w > 0.0) {
            p += joint_apply(v.joints[k], v.position, 1.0) * w;
            n += joint_apply(v.joints[k], v.normal, 0.0) * w;
        }
    }
    var o: SkinnedOut;
    o.rel = p;
    o.normal = n;
    o.clip = frame.view_proj * vec4f(p, 1.0);
    // The palette colour, sRGB bytes to linear.
    o.color = pow(v.color.rgb, vec3f(2.2));
    return o;
}

// The key light on the fragment's face: the sun while it is up over that face, else the moon
// (as the rigid characters' `key_light`).
fn skin_key(frag_y: f32, l: TerrainLight) -> Key2 {
    let face = face_at(frag_y);
    let d = frame.day;
    var k: Key2;
    k.dir = select(d.moon.xyz, d.sun.xyz, own(d.sun.xyz, face) > 0.01);
    k.level = lambert_lit(vec3f(1.0), face_up(face), frag_y, l);
    return k;
}
struct Key2 {
    dir: vec3f,
    level: vec3f,
};

@fragment
fn skinned_fs(i: SkinnedOut, @builtin(front_facing) front: bool) -> SceneOut {
    let n = normalize(select(-i.normal, i.normal, front));
    let frag_y = frame.eye.x + i.rel.y;
    var light = terrain_light(1.0, skin.light.xy);
    light.direct = shadow_light(i.rel, n, light.direct);
    let points = point_lights(i.rel, n);
    let key = skin_key(frag_y, light);
    var w = dot(n, key.dir) * 0.5 + 0.5;
    w = mix(min(w, 0.45), w, light.direct);
    // The ramp from the palette colour: cooler and darker in shadow, warmer in the light.
    let base = i.color;
    let shade = base * vec3f(0.55, 0.58, 0.78);
    let lights = min(base * vec3f(1.18, 1.12, 0.96) + vec3f(0.02, 0.016, 0.0), vec3f(1.0));
    var c = mix(shade, base, smoothstep(0.16, 0.5, w));
    c = mix(c, lights, smoothstep(0.66, 0.98, w) * 0.8);
    c = c * mix(0.9, 1.0, face_rise(frame.eye.y, n) * 0.5 + 0.5);
    let rim = pow(1.0 - max(dot(n, normalize(-i.rel)), 0.0), 3.0);
    c = c + lights * rim * 0.14 * smoothstep(0.3, 0.6, w);
    var o: SceneOut;
    o.color = vec4f(c * (key.level * PI + points) * CHARACTER_EXPOSURE, 1.0);
    o.depth = frame_depth(view_depth(i.rel), true);
    return o;
}
";

/// The module's source: the shared lighting and the skinned bodies' entry points.
pub fn source() -> String {
    format!(
        "{}\nconst JOINT_ROWS: u32 = {}u;\nconst CHARACTER_EXPOSURE: f32 = {:?};\n{WGSL}",
        super::shaders::lighting_source(),
        MAX_JOINTS * 3,
        CHARACTER_EXPOSURE,
    )
}

struct Draw {
    instance: u32,
    /// Its mesh: an index into `meshes`.
    mesh: usize,
    /// Bit per scene pass it is drawn in.
    passes: u8,
    casts: bool,
    /// Your own body: into the nearest cascade only (as the rigid `cast_only`).
    cast_only: bool,
}

/// A model's mesh on the GPU.
struct Mesh {
    data: &'static ModelData,
    model: &'static Model,
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &'static ModelData,
    model: &'static Model,
    level: usize,
) -> Mesh {
    let (v, i) = vertex_bytes(model, level);
    let buffer = |usage, bytes: &[u8], label| {
        let b = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (bytes.len() as u64).max(4).next_multiple_of(4),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&b, 0, bytes);
        b
    };
    Mesh {
        data,
        model,
        vertex: buffer(wgpu::BufferUsages::VERTEX, &v, "skinned vertices"),
        index: buffer(wgpu::BufferUsages::INDEX, &i, "skinned indices"),
        count: (i.len() / 4) as u32,
    }
}

pub struct SkinnedRenderer {
    /// The player's model first, then each creature model as it is first drawn.
    meshes: Vec<Mesh>,
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    instances: wgpu::Buffer,
    capacity: u64,
    group: wgpu::BindGroup,
    /// Each body's clip player, by its mesh and its player's (or creature's) id.
    players: HashMap<(usize, u32), Player>,
    draws: Vec<Draw>,
    last_s: Option<f64>,
    /// The Model detail setting's level (`dwell_models::DETAIL_LEVELS`), every mesh drawn at it.
    level: usize,
}

fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("skinned instances"),
        size: capacity * INSTANCE_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    instances: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("skinned"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: instances,
                offset: 0,
                size: wgpu::BufferSize::new(INSTANCE_BYTES),
            }),
        }],
    })
}

impl SkinnedRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
        shadow_frame_layout: &wgpu::BindGroupLayout,
        samples: u32,
    ) -> Self {
        let (data, model) = dwell_models::player();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skinned"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skinned"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(INSTANCE_BYTES),
                },
                count: None,
            }],
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
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint8x4,
                offset: 28,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 32,
                shader_location: 4,
            },
        ];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
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
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("skinned_vs"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("skinned_fs"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // The packs' meshes aren't all closed (the hat's brim): both sides drawn.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state()),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned shadows"),
            bind_group_layouts: &[Some(shadow_frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned shadows"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("skinned_vs"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(super::shadows::caster_depth(&dwell_look::shadows::SHADOWS)),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let level = dwell_models::DETAIL_DEFAULT;
        let player = upload(device, queue, data, model, level);
        let capacity = 4;
        let instances = instance_buffer(device, capacity);
        let group = bind_group(device, &layout, &instances);
        Self {
            meshes: vec![player],
            pipeline,
            shadow_pipeline,
            layout,
            instances,
            capacity,
            group,
            players: HashMap::default(),
            draws: Vec::new(),
            last_s: None,
            level,
        }
    }

    /// The Model detail setting: every skinned model drawn at `level` (an index into
    /// `dwell_models::DETAIL_LEVELS`), its meshes re-uploaded; the clip players keep playing.
    pub fn set_level(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, level: usize) {
        if level == self.level {
            return;
        }
        self.level = level;
        for m in &mut self.meshes {
            *m = upload(device, queue, m.data, m.model, level);
        }
    }

    /// The level drawn (`set_level`'s).
    pub fn level(&self) -> usize {
        self.level
    }

    /// GPU bytes of the mesh.
    pub fn mesh_bytes(&self) -> u64 {
        self.meshes
            .iter()
            .map(|m| m.vertex.size() + m.index.size())
            .sum()
    }

    /// This frame's bodies (`now_s`: the frame's clock, seconds): advances each one's clip player
    /// by its role and writes its joints. Returns each body's [`hand_frame`] (None where it is not
    /// drawn or holds nothing), in `bodies`' order.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: V3,
        passes: &[(f64, f64)],
        cast_m: f64,
        bodies: &[SkinnedDraw],
        now_s: f64,
    ) -> Vec<Option<Affine>> {
        let dt = self
            .last_s
            .map_or(0.0, |l| (now_s - l).clamp(0.0, 0.25) as f32);
        self.last_s = Some(now_s);
        self.draws.clear();
        for d in bodies {
            if self.meshes.iter().all(|m| m.data.name != d.model)
                && let Some((data, model)) = dwell_models::model(d.model)
            {
                self.meshes.push(upload(device, queue, data, model, self.level));
            }
        }
        let mesh_of = |name: &str| self.meshes.iter().position(|m| m.data.name == name);
        let keys: Vec<Option<usize>> = bodies.iter().map(|b| mesh_of(b.model)).collect();
        self.players.retain(|(m, id), _| {
            bodies
                .iter()
                .zip(&keys)
                .any(|(b, k)| b.seed == *id && *k == Some(*m))
        });
        let mut floats: Vec<f32> = Vec::new();
        let mut hands = vec![None; bodies.len()];
        for (n, d) in bodies.iter().enumerate() {
            let Some(mesh) = keys[n] else {
                continue;
            };
            let (data, model) = (self.meshes[mesh].data, self.meshes[mesh].model);
            let p = self
                .players
                .entry((mesh, d.seed))
                .or_insert_with(|| Player::new(data, model, d.role));
            p.update(data, model, d.role, dt);
            let dist = (0..3)
                .map(|k| (d.feet[k] - eye[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            if dist > DRAW_M {
                continue;
            }
            let mut mask = 0u8;
            for (i, (near, far)) in passes.iter().enumerate() {
                if !d.cast_only && dist + 3.0 >= *near && dist - 3.0 <= *far {
                    mask |= 1 << i;
                }
            }
            let casts = dist - 3.0 <= cast_m;
            if mask == 0 && !casts {
                continue;
            }
            let place = body_matrix(d, data, eye);
            let globals = anim::globals(model, &p.pose(model));
            hands[n] = hand_frame(data, model, &globals, &place);
            let joints: Vec<Mat4> = model
                .skin
                .iter()
                .zip(&model.inverse_bind)
                .map(|(&j, ib)| mm::mul(&globals[j], ib))
                .collect();
            let at = floats.len();
            floats.resize(at + INSTANCE_FLOATS, 0.0);
            for (k, j) in joints.iter().take(MAX_JOINTS).enumerate() {
                floats[at + k * 12..at + k * 12 + 12].copy_from_slice(&rows(&mm::mul(&place, j)));
            }
            floats[at + JOINT_FLOATS..at + JOINT_FLOATS + 2].copy_from_slice(&d.light);
            self.draws.push(Draw {
                instance: (at / INSTANCE_FLOATS) as u32,
                mesh,
                passes: mask,
                casts,
                cast_only: d.cast_only,
            });
        }
        let count = (floats.len() / INSTANCE_FLOATS) as u64;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.instances.destroy();
            self.instances = instance_buffer(device, self.capacity);
            self.group = bind_group(device, &self.layout, &self.instances);
        }
        if !floats.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&floats));
        }
        hands
    }

    /// Where the model holds an item (models.json `hand`).
    pub fn hand(&self) -> Option<dwell_models::Hand> {
        self.meshes[0].data.hand
    }

    /// How many bodies are drawn this frame.
    pub fn bodies(&self) -> usize {
        self.draws.len()
    }

    /// The role → clip players (tests and telemetry).
    pub fn player_for(&self, seed: u32) -> Option<&Player> {
        self.players.get(&(0, seed))
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, d: &Draw, calls: &mut u32, tris: &mut u64) {
        pass.set_bind_group(1, &self.group, &[d.instance * INSTANCE_BYTES as u32]);
        let m = &self.meshes[d.mesh];
        pass.set_vertex_buffer(0, m.vertex.slice(..));
        pass.set_index_buffer(m.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..m.count, 0, 0..1);
        *calls += 1;
        *tris += u64::from(m.count / 3);
    }

    /// Draws the bodies of scene pass `i`; whether any were (group 1 is then the skinned group's).
    pub fn draw_bodies(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        i: usize,
        calls: &mut u32,
        tris: &mut u64,
    ) -> bool {
        let mut any = false;
        for d in self.draws.iter().filter(|d| d.passes & (1 << i) != 0) {
            if !any {
                pass.set_pipeline(&self.pipeline);
            }
            self.draw(pass, d, calls, tris);
            any = true;
        }
        any
    }

    /// Draws the bodies within the shadow distance into a shadow map's pass (your own into the
    /// nearest cascade only, `nearest`).
    pub fn draw_shadows(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        nearest: bool,
        calls: &mut u32,
        tris: &mut u64,
    ) {
        let mut first = true;
        for d in self
            .draws
            .iter()
            .filter(|d| d.casts && (nearest || !d.cast_only))
        {
            if first {
                pass.set_pipeline(&self.shadow_pipeline);
                first = false;
            }
            self.draw(pass, d, calls, tris);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peasant_head(m: &Model) -> usize {
        (0..m.positions.len())
            .max_by(|&a, &b| m.positions[a][1].total_cmp(&m.positions[b][1]))
            .unwrap()
    }

    // A skinned body stands on its face, its head up the face's up, and faces its yaw: the same
    // face-local result on both faces.
    dwell_world::both_faces! {
        fn a_skinned_body_stands_on_its_face(face: FaceFrame) {
            let (data, model) = dwell_models::player();
            let d = SkinnedDraw {
                model: dwell_models::PLAYER,
                seed: 1,
                feet: face.point([10.0, 5.0, 0.0]),
                yaw: 90.0,
                face,
                dead: false,
                role: "idle",
                light: [1.0, 0.0],
                cast_only: false,
            };
            let place = body_matrix(&d, data, [0.0; 3]);
            let joints: Vec<Mat4> = anim::joint_matrices(model, &anim::rest(model))
                .iter()
                .map(|j| mm::mul(&place, j))
                .collect();
            let head = anim::skin_point(model, &joints, peasant_head(model));
            // Face-local height over its feet: about the player's height.
            let up = face.y(f64::from(head[1])) - face.y(d.feet[1]);
            assert!((1.6..2.0).contains(&up), "{up}");
            // Its front (model +z) faces +x at yaw 90° on either face.
            let ahead = mm::point(&place, [0.0, 0.0, 1.0]);
            assert!((f64::from(ahead[0]) - d.feet[0] - f64::from(data.scale)).abs() < 1e-3, "{ahead:?}");
        }
    }

    fn walking_hand(face: FaceFrame, t: f32) -> (Affine, Affine) {
        let (data, model) = dwell_models::player();
        let d = SkinnedDraw {
            model: dwell_models::PLAYER,
            seed: 1,
            feet: face.point([4.0, 7.0, -2.0]),
            yaw: 30.0,
            face,
            dead: false,
            role: "walk",
            light: [1.0, 0.0],
            cast_only: false,
        };
        let place = body_matrix(&d, data, [0.0; 3]);
        let mut p = Player::new(data, model, "walk");
        p.update(data, model, "walk", t);
        let globals = anim::globals(model, &p.pose(model));
        let hand = hand_frame(data, model, &globals, &place).expect("the Peasant holds items");
        let h = data.hand.unwrap();
        let joint = rigid(&mm::mul(&place, &globals[model.skin[h.joint]]));
        (hand, joint)
    }

    // A held item's frame follows the hand joint's world matrix as the clip plays: at the joint
    // (plus the data's offset in its frame), turned with it, on both faces; and the hand hangs at
    // about hip height over the feet, face-locally.
    dwell_world::both_faces! {
        fn the_held_item_follows_the_skinned_hand_joint(face: FaceFrame) {
            let (_, model) = dwell_models::player();
            let h = dwell_models::player().0.hand.unwrap();
            let mut origins = Vec::new();
            for t in [0.0, 0.3, 0.6] {
                let (hand, joint) = walking_hand(face, t);
                let want = joint.then(&Affine::translation(h.at)).then(&Affine::euler(h.rot));
                for k in 0..3 {
                    assert!((hand.t[k] - want.t[k]).abs() < 1e-5, "{t}: {hand:?} {want:?}");
                    for r in 0..3 {
                        assert!((hand.m[k][r] - want.m[k][r]).abs() < 1e-5);
                    }
                }
                let at = joint.apply(h.at);
                assert!((0..3).all(|k| (hand.t[k] - at[k]).abs() < 1e-5));
                let up = face.y(f64::from(hand.t[1])) - face.y(face.point([4.0, 7.0, -2.0])[1]);
                assert!((0.5..1.2).contains(&up), "hand {up} m over the feet");
                origins.push(hand.t);
            }
            // The walk swings the hand: it moves between the samples.
            let moved = (0..3).map(|k| (origins[0][k] - origins[1][k]).powi(2)).sum::<f32>().sqrt();
            assert!(moved > 0.02, "{moved}");
            let _ = model;
        }
    }

    #[test]
    fn a_hand_is_data_and_a_model_without_one_holds_nothing() {
        // The fixture's hand comes from its models.json alone.
        let two = dwell_models::find(dwell_models::fixture::MODELS, "two_bone").unwrap();
        let h = two.hand.expect("the fixture names a hand");
        assert_eq!((h.joint, h.bone), (1, "hand_r"));
        // An animal has no hand: no held item frame.
        let (_, model) = dwell_models::player();
        let deer = dwell_models::find(dwell_models::MODELS, "deer").unwrap();
        assert!(deer.hand.is_none());
        let g = anim::globals(model, &anim::rest(model));
        assert!(hand_frame(deer, model, &g, &mm::IDENTITY).is_none());
    }

    #[test]
    fn the_vertex_bytes_carry_colours_joints_and_weights() {
        let (_, model) = dwell_models::player();
        let (v, i) = vertex_bytes(model, 0);
        assert_eq!(v.len(), model.positions.len() * VERTEX_BYTES);
        assert_eq!(i.len(), model.indices.len() * 4);
        // Each Model detail level its own mesh, fewer triangles a level (ADR 0070 §10).
        let mut last = i.len();
        for k in 1..dwell_models::DETAIL_LEVELS.len() {
            let (v, i) = vertex_bytes(model, k);
            assert_eq!(v.len(), model.level(k).positions.len() * VERTEX_BYTES);
            assert!(i.len() < last, "level {k}");
            last = i.len();
        }
        // The first vertex's weights sum to about 255.
        let w: u32 = v[32..36].iter().map(|&b| u32::from(b)).sum();
        assert!((253..=257).contains(&w), "{w}");
    }

    #[test]
    fn the_skinned_module_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(&source()).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for e in ["skinned_vs", "skinned_fs"] {
            assert!(module.entry_points.iter().any(|p| p.name == e), "{e}");
        }
    }

    #[test]
    fn the_shader_reads_the_joint_block_the_cpu_writes() {
        let src = source();
        assert!(src.contains(&format!("const JOINT_ROWS: u32 = {}u;", MAX_JOINTS * 3)));
        // Joint rows then the light: within the instance block.
        assert!((JOINT_FLOATS + 4) * 4 <= INSTANCE_BYTES as usize);
        assert_eq!(INSTANCE_BYTES % 256, 0);
    }
}
