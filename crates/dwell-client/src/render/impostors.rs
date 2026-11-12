//! Far trees as impostors (docs/proposals/TREE_IMPOSTORS.md phase I-3, WORK_QUEUE item 31): one
//! camera-facing quad a tree, drawn from the atlas `dwell_worldgen::impostor` bakes from the real
//! tree models (albedo, normals, depth), lit by the scene's sun, ambient and fog, its texels moved
//! to their baked depth, and handed over from the near trees' leaves by the screen dither.
//!
//! - **The atlas:** one `texture_2d_array` layer a baked tree (`Config::layer_count`), baked by the
//!   job workers (`Job::ImpostorBake`) and uploaded a layer at a time as they come back
//!   ([`Impostors::upload_layer`]); a layer not yet baked draws nothing.
//! - **The instances:** per tile of [`TILE_M`] m, the trees `TerrainGenerator::far_trees` lists
//!   (`Job::FarTrees`), as `Config::instances` places them; the game keeps the tiles within the
//!   tier's `toM` of the eye ([`tiles_within`]) and the buffer is rebuilt when they change.
//! - **The view:** per pixel, `Config::dither`'s pick of the baked views (the WGSL twin
//!   `imp_pick`), each sampled at the quad point projected into that view.
//! - **The hand-over:** `Far::share` by the fragment's distance; the near leaves keep the pixels
//!   whose noise is under their share (`cutout_fs`, `TREE_LEAF_FADE`) and the impostor the rest, so
//!   no pixel draws both; past the fade the near leaves are dropped whole in the vertex stage.
//! - **Faces:** instances are face-local (`Config::instances`) and cross into the world through
//!   `FaceFrame::point`; the shader turns the baked frame into the world by `face_vector`.
//! - **Shadows:** received (the scene's shadow lookup); not cast (I-5).

use std::collections::HashMap;

use dwell_world::FaceFrame;
use dwell_worldgen::impostor::{Config, Far, FarTree, Instance, Tier};

use super::frame::{DEPTH, DEPTH_COMPARE, HDR, VIEW_DEPTH};
use super::pipeline;

/// A far trees tile's side (m): the unit the game asks the workers for.
pub const TILE_M: i32 = 64;
/// The farthest tile kept when the tier says far trees never stop (`toM` 0), m.
pub const MAX_REACH_M: f32 = 1024.0;
/// Bytes of one instance on the GPU: the world foot (3 f32), layer | turns << 16 (u32), the leaf
/// tint (unorm8x4, a ratio over 2).
pub const INSTANCE_BYTES: u64 = 20;

/// The bind group's bindings (group 3, past the composite's).
const B_UNIFORM: u32 = 20;
const B_ALBEDO: u32 = 21;
const B_NORMAL: u32 = 22;
const B_LAYERS: u32 = 23;

/// The tiles (indices of [`TILE_M`]) whose square comes within `reach` m of the eye's column
/// (`eye`, face-local x and z): the far trees kept.
pub fn tiles_within(eye: [f64; 2], reach: f32) -> Vec<(i32, i32)> {
    let t = f64::from(TILE_M);
    let r = f64::from(reach);
    let lo = |c: f64| ((c - r) / t).floor() as i32;
    let hi = |c: f64| ((c + r) / t).floor() as i32;
    let mut out = Vec::new();
    for k in lo(eye[1])..=hi(eye[1]) {
        for i in lo(eye[0])..=hi(eye[0]) {
            // The square's nearest point to the eye.
            let near = |c: f64, i: i32| c.clamp(f64::from(i) * t, f64::from(i + 1) * t) - c;
            let (dx, dz) = (near(eye[0], i), near(eye[1], k));
            if dx * dx + dz * dz <= r * r {
                out.push((i, k));
            }
        }
    }
    out
}

/// The far tree reach of a tier: its `toM`, or [`MAX_REACH_M`] when it never stops.
pub fn reach(far: &Far) -> f32 {
    if far.to.is_finite() {
        far.to
    } else {
        MAX_REACH_M
    }
}

/// The frame uniform's `far_trees` vec4 (shaders.rs `near_tree_share`): from, fade, reach, 1 when
/// impostors draw; zeros (the near trees' leaves to the full-detail distance) without.
pub fn far_uniform(far: Option<Far>) -> [f32; 4] {
    match far {
        Some(f) if f.impostor => [f.from, f.fade, reach(&f), 1.0],
        _ => [0.0; 4],
    }
}

/// One instance's 20 bytes: `inst`'s face-local foot crossed into the world by `face`, its layer
/// and turns, and its leaf tint as a ratio of the baked leaf colour over 2 (`imp_fs`).
pub fn pack(face: FaceFrame, inst: &Instance, tint: [f32; 3]) -> [u8; INSTANCE_BYTES as usize] {
    let w = face.point(inst.foot.map(f64::from));
    let mut out = [0u8; INSTANCE_BYTES as usize];
    for (k, v) in w.iter().enumerate() {
        out[k * 4..k * 4 + 4].copy_from_slice(&(*v as f32).to_le_bytes());
    }
    let packed = (inst.layer & 0xffff) | ((inst.quarter_turns & 3) << 16);
    out[12..16].copy_from_slice(&packed.to_le_bytes());
    for (k, c) in tint.iter().enumerate() {
        out[16 + k] = ((c * 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out[19] = 255;
    out
}

/// The leaf tint of a far tree as a ratio of the baked leaf colour (`leaf`, 0–1 sRGB): the same
/// tint its near leaves take (`species::tint`, ADR 0079).
pub fn tint_ratio(tree: &FarTree, leaf: [f32; 3]) -> [f32; 3] {
    let Some(species) = dwell_worldgen::species::SPECIES.get(usize::from(tree.species)) else {
        return [1.0; 3];
    };
    let tint = dwell_worldgen::species::tint(species, tree.seed);
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    let rgb = (q(leaf[0]) << 16) | (q(leaf[1]) << 8) | q(leaf[2]);
    let t = tint.apply_rgb8(rgb);
    let ch = |v: u32, s: u32| ((v >> s) & 0xff) as f32;
    let lin = |c: f32| dwell_look::srgb_to_linear(f64::from(c) / 255.0) as f32;
    [
        lin(ch(t, 16)) / lin(ch(rgb, 16)).max(1e-4),
        lin(ch(t, 8)) / lin(ch(rgb, 8)).max(1e-4),
        lin(ch(t, 0)) / lin(ch(rgb, 0)).max(1e-4),
    ]
}

/// The impostors' WGSL: the layout's constants from the data and the entry points `imp_vs` and
/// `imp_fs`, appended to the scene module (they use its frame, lighting and noise).
pub fn wgsl(config: &Config) -> String {
    let n = config.views.len() as u32;
    let cols = (n as f32).sqrt().ceil().max(1.0) as u32;
    let elev = config
        .elevations
        .iter()
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "const IMP_AZ: u32 = {az}u;\n\
         const IMP_NE: u32 = {ne}u;\n\
         const IMP_TOP: bool = {top};\n\
         const IMP_COLS: u32 = {cols}u;\n\
         const IMP_ELEV = array<f32, {ne}>({elev});\n{body}",
        az = config.azimuths,
        ne = config.elevations.len(),
        top = config.top,
        body = IMPOSTOR_WGSL,
    )
}

const IMPOSTOR_WGSL: &str = r#"
// Far tree impostors (render/impostors.rs). eye: the eye's world position, w the eye's face (±1,
// FaceFrame); far: the hand-over (from, fade, to, 1 on); leaf: the baked leaf albedo (linear), w a
// view's side in texels.
struct ImpostorScene {
    eye: vec4f,
    far: vec4f,
    leaf: vec4f,
};
@group(3) @binding(20) var<uniform> imp: ImpostorScene;
@group(3) @binding(21) var imp_albedo: texture_2d_array<f32>;
@group(3) @binding(22) var imp_normal: texture_2d_array<f32>;
// Per layer: the bounding sphere's centre height over the foot, its radius, 1 once baked.
@group(3) @binding(23) var<storage, read> imp_layers: array<vec4f>;

struct ImpIn {
    @location(0) foot: vec3f,
    @location(1) packed: u32,
    @location(2) tint: vec4f,
};
struct ImpOut {
    @builtin(position) clip: vec4f,
    // The quad point from the bounding centre, in the baked tree's frame.
    @location(0) local: vec3f,
    @location(1) @interpolate(flat) dir: vec3f,
    @location(2) @interpolate(flat) layer: u32,
    @location(3) @interpolate(flat) turns: u32,
    @location(4) @interpolate(flat) tint: vec3f,
    @location(5) @interpolate(flat) centre: vec3f,
    @location(6) @interpolate(flat) radius: f32,
};

// A quarter-turned vector about the tree's up (q quarter turns), as `Config::instances` turns it.
fn imp_turn(v: vec3f, q: u32) -> vec3f {
    var r = v;
    for (var i = 0u; i < q % 4u; i++) {
        r = vec3f(r.z, r.y, -r.x);
    }
    return r;
}

// A view's screen basis (the bake's `basis`): right and up for a camera at direction v.
fn imp_basis_right(v: vec3f) -> vec3f {
    let h = length(v.xz);
    return select(vec3f(0.0, 0.0, -1.0), vec3f(v.z / h, 0.0, -v.x / h), h > 1e-6);
}

// A baked view's direction (the bake's `views`): the ring's azimuths at each elevation, then the top.
fn imp_view(k: u32) -> vec3f {
    if (IMP_TOP && k == IMP_NE * IMP_AZ) {
        return vec3f(0.0, 1.0, 0.0);
    }
    var elevs = IMP_ELEV;
    let e = elevs[k / IMP_AZ];
    let a = f32(k % IMP_AZ) / f32(IMP_AZ) * TAU;
    return vec3f(cos(e) * cos(a), sin(e), cos(e) * sin(a));
}

fn imp_band(b: u32) -> f32 {
    var elevs = IMP_ELEV;
    return select(elevs[min(b, IMP_NE - 1u)], PI * 0.5, b >= IMP_NE);
}

fn imp_ring(b: u32, a: u32) -> u32 {
    return select(b * IMP_AZ + a, IMP_NE * IMP_AZ, b >= IMP_NE);
}

// `Config::dither`: the view whose share of `Config::pick`'s bilinear weights holds `threshold`.
fn imp_pick(dir: vec3f, threshold: f32) -> u32 {
    let d = normalize(dir);
    let elev = asin(clamp(d.y, -1.0, 1.0));
    var az = atan2(d.z, d.x);
    if (az < 0.0) {
        az += TAU;
    }
    let fa = az / (TAU / f32(IMP_AZ));
    let a0 = u32(floor(fa)) % IMP_AZ;
    let ta = fract(fa);
    let a1 = (a0 + 1u) % IMP_AZ;
    let bands = IMP_NE + select(0u, 1u, IMP_TOP);
    var b0 = 0u;
    var b1 = 0u;
    var tb = 0.0;
    if (elev <= imp_band(0u)) {
        b0 = 0u;
        b1 = 0u;
    } else if (elev >= imp_band(bands - 1u)) {
        b0 = bands - 1u;
        b1 = bands - 1u;
    } else {
        for (var b = 0u; b + 1u < bands; b++) {
            if (imp_band(b) <= elev && elev < imp_band(b + 1u)) {
                b0 = b;
                b1 = b + 1u;
                tb = (elev - imp_band(b)) / (imp_band(b + 1u) - imp_band(b));
            }
        }
    }
    var acc = (1.0 - tb) * (1.0 - ta);
    if (threshold < acc) {
        return imp_ring(b0, a0);
    }
    acc += (1.0 - tb) * ta;
    if (threshold < acc) {
        return imp_ring(b0, a1);
    }
    acc += tb * (1.0 - ta);
    if (threshold < acc) {
        return imp_ring(b1, a0);
    }
    return imp_ring(b1, a1);
}

// `Far::share`: the impostor's share at `dist` m (0 nearer than the hand-over, 1 past its fade).
fn imp_share(dist: f32) -> f32 {
    if (imp.far.w < 0.5 || dist > imp.far.z) {
        return 0.0;
    }
    return clamp((dist - imp.far.x) / max(imp.far.y, 1e-3), 0.0, 1.0);
}

@vertex
fn imp_vs(v: ImpIn, @builtin(vertex_index) vi: u32) -> ImpOut {
    var o: ImpOut;
    let layer = v.packed & 0xffffu;
    let q = (v.packed >> 16u) & 3u;
    let info = imp_layers[layer];
    let face = imp.eye.w;
    let centre = v.foot + face_up(face) * info.x - imp.eye.xyz;
    let r = info.y;
    // Toward the eye from the centre, in the baked tree's frame (the turn undone).
    let d = imp_turn(face_vector(face, -centre), (4u - q) % 4u);
    let dist = length(centre);
    o.layer = layer;
    o.turns = q;
    o.tint = v.tint.rgb * 2.0;
    o.centre = centre;
    o.radius = r;
    o.dir = d;
    // Not baked yet, or every pixel of it the near tree's or past the far trees: nothing.
    if (info.z < 0.5 || imp_share(dist + r) <= 0.0 || dist - r > imp.far.z) {
        o.clip = vec4f(0.0, 0.0, -1.0, 1.0);
        return o;
    }
    let corner = array<vec2f, 6>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
        vec2f(-1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0),
    )[vi % 6u];
    let n = normalize(d);
    let right = imp_basis_right(n);
    let up = cross(n, right);
    o.local = (right * corner.x + up * corner.y) * r;
    let rel = centre + face_vector(face, imp_turn(o.local, q));
    o.clip = frame.view_proj * vec4f(rel, 1.0);
    return o;
}

struct ImpOutFs {
    @location(0) color: vec4f,
    @location(1) depth: f32,
    @builtin(frag_depth) frag_depth: f32,
    @builtin(sample_mask) mask: u32,
};

@fragment
fn imp_fs(i: ImpOut) -> ImpOutFs {
    let face = imp.eye.w;
    let r = i.radius;
    // The view this pixel draws, then the quad point in that view's square.
    let k = imp_pick(i.dir, pixel_noise(i.clip.xy + vec2f(37.0, 17.0)));
    let vk = imp_view(k);
    let right = imp_basis_right(vk);
    let up = cross(vk, right);
    let u = dot(i.local, right) / r;
    let v = dot(i.local, up) / r;
    if (abs(u) >= 1.0 || abs(v) >= 1.0) {
        discard;
    }
    let px = imp.leaf.w;
    let cell = vec2i(i32(k % IMP_COLS), i32(k / IMP_COLS)) * i32(px);
    let t = vec2i(clamp(vec2f((u * 0.5 + 0.5) * px, (0.5 - v * 0.5) * px), vec2f(0.0), vec2f(px - 1.0)));
    let a = textureLoad(imp_albedo, cell + t, i.layer, 0);
    if (a.a < 0.5) {
        discard;
    }
    let nd = textureLoad(imp_normal, cell + t, i.layer, 0);
    // The texel's real point: on the view's plane at the sphere's near side, moved back by its depth.
    let p = right * u * r + up * v * r + vk * r * (1.0 - 2.0 * nd.a);
    let rel = i.centre + face_vector(face, imp_turn(p, i.turns));
    let dist = length(rel);
    // The hand-over: the near leaves keep the pixels whose noise is under their share (`cutout_fs`).
    let near = 1.0 - imp_share(dist);
    let noise = pixel_noise(i.clip.xy);
    var mask = 0xffffffffu;
    if (SCENE_SAMPLES > 1) {
        let s = u32(SCENE_SAMPLES);
        let n = u32(floor(near * f32(SCENE_SAMPLES) + noise));
        let full = (1u << s) - 1u;
        let first = u32(noise * f32(SCENE_SAMPLES)) % s;
        let run = (1u << min(n, s)) - 1u;
        let near_mask = ((run << first) | (run >> (s - first))) & full;
        mask = full & ~near_mask;
        if (mask == 0u) {
            discard;
        }
    } else if (near > noise) {
        discard;
    }
    var albedo = a.rgb;
    if (distance(a.rgb, imp.leaf.rgb) < 0.02) {
        albedo *= i.tint;
    }
    let n = normalize(face_vector(face, imp_turn(nd.rgb * 2.0 - 1.0, i.turns)));
    let lit = scene_out(albedo, n, rel, 1.0, open_light());
    var o: ImpOutFs;
    o.color = lit.color;
    o.depth = frame_depth(lit.depth, true);
    let c = frame.view_proj * vec4f(rel, 1.0);
    o.frag_depth = clamp(c.z / c.w, 0.0, 1.0);
    o.mask = mask;
    return o;
}
"#;

/// The far trees' GPU side: the atlas, the layers' spheres, the instances, the pipeline.
pub struct Impostors {
    config: &'static Config,
    px: u32,
    size: (u32, u32),
    albedo: wgpu::Texture,
    normal: wgpu::Texture,
    layers: Vec<[f32; 4]>,
    layer_buffer: wgpu::Buffer,
    layers_dirty: bool,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    pipeline: pipeline::Lazy,
    instances: Option<wgpu::Buffer>,
    count: u32,
    /// The tiles' instances (packed), by (face, tile i, tile k).
    tiles: HashMap<(u8, i32, i32), Vec<u8>>,
    dirty: bool,
    /// The hand-over in use (`None`: no impostors drawn).
    pub far: Option<Far>,
    /// Bakes asked for (the game sends them once).
    pub requested: bool,
}

impl Impostors {
    /// The atlas at `tier`'s texels, the pipeline in the scene's module with its frame and draw
    /// layouts.
    pub fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        frame_layout: &wgpu::BindGroupLayout,
        draw_layout: &wgpu::BindGroupLayout,
        samples: u32,
        tier: Tier,
    ) -> Self {
        let config = Config::get();
        let px = config.px(tier);
        let n = config.views.len() as u32;
        let cols = (n as f32).sqrt().ceil().max(1.0) as u32;
        let rows = n.div_ceil(cols);
        let size = (cols * px, rows * px);
        let layer_count = config.layer_count().max(1);
        let texture = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: layer_count,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let albedo = texture("impostor albedo", wgpu::TextureFormat::Rgba8UnormSrgb);
        let normal = texture("impostor normal", wgpu::TextureFormat::Rgba8Unorm);
        let layers = vec![[0.0f32; 4]; layer_count as usize];
        let layer_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("impostor layers"),
            size: 16 * u64::from(layer_count),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("impostor scene"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("impostors"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: B_UNIFORM,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(B_ALBEDO),
                tex_entry(B_NORMAL),
                wgpu::BindGroupLayoutEntry {
                    binding: B_LAYERS,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("impostors"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: B_UNIFORM,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: B_ALBEDO,
                    resource: wgpu::BindingResource::TextureView(&view(&albedo)),
                },
                wgpu::BindGroupEntry {
                    binding: B_NORMAL,
                    resource: wgpu::BindingResource::TextureView(&view(&normal)),
                },
                wgpu::BindGroupEntry {
                    binding: B_LAYERS,
                    resource: layer_buffer.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("impostors"),
            bind_group_layouts: &[Some(frame_layout), Some(draw_layout), None, Some(&layout)],
            immediate_size: 0,
        });
        let (device, module) = (device.clone(), module.clone());
        let pipeline = pipeline::Lazy::new(move || {
            let attributes = [
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Unorm8x4,
                    offset: 16,
                    shader_location: 2,
                },
            ];
            let buffers = [Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
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
            pipeline::create(
                &device,
                &wgpu::RenderPipelineDescriptor {
                    label: Some("imp_fs"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("imp_vs"),
                        compilation_options: Default::default(),
                        buffers: &buffers,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("imp_fs"),
                        compilation_options: Default::default(),
                        targets: &targets,
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(DEPTH_COMPARE),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: samples,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                },
            )
        });
        Self {
            config,
            px,
            size,
            albedo,
            normal,
            layers,
            layer_buffer,
            layers_dirty: false,
            uniform,
            group,
            pipeline,
            instances: None,
            count: 0,
            tiles: HashMap::new(),
            dirty: false,
            far: None,
            requested: false,
        }
    }

    /// A view's side in texels (the bake jobs').
    pub fn px(&self) -> u32 {
        self.px
    }

    /// Writes a baked layer into the atlas (a size that doesn't match is dropped).
    pub fn upload_layer(
        &mut self,
        queue: &wgpu::Queue,
        layer: u32,
        centre_y: f32,
        radius: f32,
        albedo: &[u8],
        normal_depth: &[u8],
    ) {
        let bytes = (self.size.0 * self.size.1 * 4) as usize;
        let Some(slot) = self.layers.get_mut(layer as usize) else {
            return;
        };
        if albedo.len() != bytes || normal_depth.len() != bytes {
            return;
        }
        for (t, data) in [(&self.albedo, albedo), (&self.normal, normal_depth)] {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: t,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.size.0 * 4),
                    rows_per_image: Some(self.size.1),
                },
                wgpu::Extent3d {
                    width: self.size.0,
                    height: self.size.1,
                    depth_or_array_layers: 1,
                },
            );
        }
        *slot = [centre_y, radius, 1.0, 0.0];
        self.layers_dirty = true;
    }

    /// A tile's far trees on `face` (tile indices `ti`, `tk`), placed as `Config::instances` does.
    pub fn set_tile(&mut self, face: FaceFrame, ti: i32, tk: i32, trees: &[FarTree]) {
        let leaf = self.config.leaf;
        let mut bytes = Vec::with_capacity(trees.len() * INSTANCE_BYTES as usize);
        for (t, inst) in trees.iter().zip(self.config.instances(trees)) {
            bytes.extend_from_slice(&pack(face, &inst, tint_ratio(t, leaf)));
        }
        self.tiles.insert((u8::from(face.is_b()), ti, tk), bytes);
        self.dirty = true;
    }

    /// Drops the tiles `keep` doesn't hold (face, i, k).
    pub fn retain_tiles(&mut self, keep: impl Fn(&(u8, i32, i32)) -> bool) {
        let before = self.tiles.len();
        self.tiles.retain(|k, _| keep(k));
        self.dirty |= self.tiles.len() != before;
    }

    /// Whether tile (face, i, k) is held.
    pub fn has_tile(&self, key: &(u8, i32, i32)) -> bool {
        self.tiles.contains_key(key)
    }

    /// The instances drawn.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Before the frame's passes: the uniform (eye, face, hand-over), the layers and the instances.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: [f64; 3],
        face: FaceFrame,
    ) {
        let far = self.far.unwrap_or(Far {
            impostor: false,
            from: 0.0,
            fade: 0.0,
            to: 0.0,
        });
        let lin = |c: f32| dwell_look::srgb_to_linear(f64::from(c)) as f32;
        let leaf = self.config.leaf.map(|c| lin((c * 255.0).round() / 255.0));
        let u: [f32; 12] = [
            eye[0] as f32,
            eye[1] as f32,
            eye[2] as f32,
            face.vector([0.0, 1.0, 0.0])[1] as f32,
            far.from,
            far.fade,
            reach(&far),
            if far.impostor { 1.0 } else { 0.0 },
            leaf[0],
            leaf[1],
            leaf[2],
            self.px as f32,
        ];
        queue.write_buffer(&self.uniform, 0, super::floats(&u));
        if self.layers_dirty {
            queue.write_buffer(&self.layer_buffer, 0, super::floats(self.layers.as_flattened()));
            self.layers_dirty = false;
        }
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let face_bit = u8::from(face.is_b());
        let all: Vec<u8> = self
            .tiles
            .iter()
            .filter(|(k, _)| k.0 == face_bit)
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        self.count = (all.len() as u64 / INSTANCE_BYTES) as u32;
        if all.is_empty() {
            return;
        }
        let need = all.len() as u64;
        if self.instances.as_ref().is_none_or(|b| b.size() < need) {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("impostor instances"),
                size: need.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(b) = &self.instances {
            queue.write_buffer(b, 0, &all);
        }
    }

    /// Draws the far trees into a scene pass (its frame and draw groups bound).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, calls: &mut u32, triangles: &mut u64) {
        let (Some(far), Some(buffer)) = (self.far, &self.instances) else {
            return;
        };
        if !far.impostor || self.count == 0 {
            return;
        }
        pass.set_pipeline(self.pipeline.get());
        pass.set_bind_group(3, &self.group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..u64::from(self.count) * INSTANCE_BYTES));
        pass.draw(0..6, 0..self.count);
        *calls += 1;
        *triangles += 2 * u64::from(self.count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiles_kept_are_those_within_reach() {
        // face-a-only: face-local columns, no world position.
        let t = tiles_within([10.0, 10.0], 100.0);
        assert!(t.contains(&(0, 0)));
        assert!(t.contains(&(1, 1)), "(64..128)² comes within 100 m");
        assert!(!t.contains(&(2, 2)), "(128..192)² is 166 m off");
        assert!(t.contains(&(-2, 0)), "x −128..−64 is 74 m off");
        assert!(!t.contains(&(-3, 0)));
    }

    #[test]
    fn the_reach_is_the_tiers_to_or_the_cap() {
        // face-a-only: data only.
        let c = Config::get();
        let on = c.far("on").expect("on");
        assert!(reach(&on) > on.from + on.fade);
        assert!(reach(&on) <= MAX_REACH_M);
    }

    dwell_world::both_faces!(
        /// An instance's foot crosses into the world through the face (its world y the face's
        /// image of the face-local one), with its layer, turns and tint packed.
        fn an_instance_packs_its_world_foot_layer_turns_and_tint(f: FaceFrame) {
            let inst = Instance {
                foot: [10.5, 140.0, -3.5],
                layer: 37,
                quarter_turns: 3,
            };
            let b = pack(f, &inst, [1.0, 0.5, 2.0]);
            let rd = |k: usize| f32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap());
            let want = f.point([10.5, 140.0, -3.5]);
            assert_eq!([rd(0), rd(1), rd(2)], want.map(|v| v as f32));
            let packed = u32::from_le_bytes(b[12..16].try_into().unwrap());
            assert_eq!((packed & 0xffff, packed >> 16), (37, 3));
            assert_eq!(&b[16..19], &[128, 64, 255]);
        }
    );

    #[test]
    fn a_tree_of_no_tint_keeps_the_baked_leaf() {
        // face-a-only: colours only.
        let c = Config::get();
        let tree = FarTree {
            anchor: [0, 0, 0],
            species: u16::MAX,
            seed: 1,
            stage: 4,
        };
        assert_eq!(tint_ratio(&tree, c.leaf), [1.0; 3]);
        let real = FarTree { species: 0, ..tree };
        let r = tint_ratio(&real, c.leaf);
        assert!(r.iter().all(|v| (0.2..5.0).contains(v)), "{r:?}");
    }

    #[test]
    fn the_shader_validates_and_its_layout_is_the_datas() {
        // face-a-only: shader text.
        let c = Config::get();
        let src = super::super::shaders::source(4);
        assert!(src.contains(&format!("const IMP_AZ: u32 = {}u;", c.azimuths)));
        let module = naga::front::wgsl::parse_str(&src).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validates");
        for entry in ["imp_vs", "imp_fs"] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "{entry}");
        }
    }
}
