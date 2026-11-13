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
//! - **Your own body** (§5.1, §11.12): in first person the same model, posed as your shadow is,
//!   drawn by a second index buffer that leaves out its head and arms (models.json
//!   `firstPerson.body`, by skin weight), moved back from the eye as the rigid body was
//!   ([`own_joints`]); fragments inside the near margin are discarded (the instance's `light.z`).
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
/// The remap's rows after the light: two vec4 a pair (docs/CHARACTERS.md §11.11).
pub const REMAP_FLOATS: usize = dwell_models::MAX_REMAP * 8;
/// A body drawn in its model's own colours.
pub const NO_REMAP: [f32; REMAP_FLOATS] = [0.0; REMAP_FLOATS];
const _: () = assert!(JOINT_FLOATS + 4 + REMAP_FLOATS <= INSTANCE_FLOATS);
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
    /// The base role (`dwell_characters::animator::body_role`'s names).
    pub role: &'static str,
    /// The flood light it stands in (as `BodyDraw::light`).
    pub light: [f32; 2],
    /// Drawn into the shadow maps only (your own body: your shadow).
    pub cast_only: bool,
    /// The characters rig's pose this frame (the clips, times and blends the rigid body and the
    /// first-person arms play): a model with a retarget map is posed from it (docs/CHARACTERS.md
    /// §11.9); None, or no map: the model plays its own clip for `role`.
    pub rig_pose: Option<dwell_characters::anim::Pose>,
    /// The held item's name (characters.json `items`): the hand's grip (models.json `grips`).
    pub grip: Option<&'static str>,
    /// Its colour remap's shader rows (an NPC's colours: [`remap_floats`] of
    /// [`dwell_models::part_remap`]); [`NO_REMAP`]: as modelled.
    pub remap: [f32; REMAP_FLOATS],
    /// Its idle pose by name (an animal's `graze`, `lie`, ...: models.json `poses`, WORK_QUEUE
    /// item 39), blended in over the model's `pose_blend_s`; None: its clips as they are.
    pub idle_pose: Option<&'static str>,
    /// A flash over its colours (sRGB 0..1 and the strength mixed toward it): a hit creature's hurt
    /// look (shared/creatures `hurtLook`, WORK_QUEUE item 35); [`NO_TINT`]: none.
    pub tint: [f32; 4],
}

/// A body drawn without a flash.
pub const NO_TINT: [f32; 4] = [0.0; 4];

/// A tint packed into one float for the instance's spare `light.w`: the colour at 5 bits a channel
/// as the whole part (r × 1024 + g × 32 + b), the strength (0..1) as the fraction (`/ 0.998`
/// keeps it under 1). Exact in an f32 (15 + 9 bits); 0: none.
pub fn pack_tint(t: [f32; 4]) -> f32 {
    let s = t[3].clamp(0.0, 1.0);
    if s <= 0.0 {
        return 0.0;
    }
    let q = |v: f32| (v.clamp(0.0, 1.0) * 31.0).round();
    q(t[0]) * 1024.0 + q(t[1]) * 32.0 + q(t[2]) + s * 0.998
}

/// An idle pose's blend (the pose shown and its weight, 0..1) after `dt` seconds wanting `want`:
/// the shown pose fades out before another fades in, each over `blend_s`.
pub fn blend_pose(
    shown: (Option<&'static str>, f32),
    want: Option<&'static str>,
    dt: f32,
    blend_s: f32,
) -> (Option<&'static str>, f32) {
    let step = dt / blend_s.max(1e-3);
    match shown {
        (Some(s), w) if Some(s) == want => (Some(s), (w + step).min(1.0)),
        (Some(s), w) if w - step > 0.0 => (Some(s), w - step),
        _ => (want, if want.is_some() { step.min(1.0) } else { 0.0 }),
    }
}

/// A body's remap rows for the shader (`REMAP_FLOATS` floats): per part slot (part `k + 1`) the
/// model's base and the base in its place (sRGB 0..1), the first's w 1 where used, 0 where not, the
/// second's w the share of the part's shading kept (BUGS.md #152).
pub fn remap_floats(remap: &[Option<dwell_models::PartRemap>]) -> [f32; REMAP_FLOATS] {
    let mut out = [0.0; REMAP_FLOATS];
    for (k, pair) in remap.iter().take(dwell_models::MAX_REMAP).enumerate() {
        let Some((from, to, keep)) = pair else {
            continue;
        };
        out[k * 8..k * 8 + 3].copy_from_slice(from);
        out[k * 8 + 3] = 1.0;
        out[k * 8 + 4..k * 8 + 7].copy_from_slice(to);
        out[k * 8 + 7] = *keep;
    }
    out
}

/// The model's pose this frame: retargeted from the rig's pose where the model has a map and one
/// is given, else its own clip player's.
pub fn model_pose(
    data: &ModelData,
    model: &Model,
    player: &Player,
    rig_pose: Option<&dwell_characters::anim::Pose>,
    grip: Option<&str>,
) -> Vec<anim::Trs> {
    let mut pose = match (data.retarget, rig_pose) {
        (Some(map), Some(pose)) => {
            dwell_models::retarget::pose_from(model, data, &map, dwell_characters::player(), pose)
        }
        _ => player.pose(model),
    };
    if let Some(g) = grip.and_then(|i| data.grip(i)) {
        dwell_models::grip::apply(model, g, &mut pose);
    }
    pose
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
    let place: Affine = place_frame(d.feet, d.yaw, d.face, eye);
    let p: Mat4 = [
        [place.m[0][0], place.m[0][1], place.m[0][2], 0.0],
        [place.m[1][0], place.m[1][1], place.m[1][2], 0.0],
        [place.m[2][0], place.m[2][1], place.m[2][2], 0.0],
        [place.t[0], place.t[1], place.t[2], 1.0],
    ];
    let turn = mm::axis_angle([0.0, 1.0, 0.0], data.yaw);
    mm::mul(&p, &mm::trs([0.0; 3], turn, [data.scale; 3]))
}

/// The interleaved vertices of a model's detail level `level` (ADR 0070 §10) and its indices:
/// each vertex's colour as sRGB bytes (the GLB's is linear; BUGS.md #96) and its part number
/// (`parts`, its model's part map at that level; empty: none) in the colour's alpha byte.
pub fn vertex_bytes(model: &Model, level: usize, parts: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let m = model.level(level);
    let mut v = Vec::with_capacity(m.positions.len() * VERTEX_BYTES);
    for i in 0..m.positions.len() {
        for f in m.positions[i].iter().chain(&m.normals[i]) {
            v.extend(f.to_le_bytes());
        }
        let c = m.colors[i].map(|c| (dwell_models::linear_to_srgb(c) * 255.0).round() as u8);
        v.extend([c[0], c[1], c[2], parts.get(i).copied().unwrap_or(0)]);
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
    // x sky, y block level / 15 (docs/DAY_NIGHT.md §11); z the near margin your own body's
    // fragments are discarded within (m from the eye; 0 for every other body); w a hurt flash
    // (`pack_tint`: the colour at 5 bits a channel as the whole part, the strength the fraction).
    light: vec4f,
    // The body's colour remap by part (an NPC's colours, docs/CHARACTERS.md §11.11): a pair of
    // rows a part (part k + 1 at rows 2k, 2k + 1), the model's ramp base and the base drawn in its
    // place (sRGB 0..1); w 1 where the part is remapped.
    remap: array<vec4f, REMAP_ROWS>,
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

// A vertex's palette colour (sRGB 0..1) after the body's remap, by its part (the colour's alpha
// byte: 0 none, k part k): a remapped part's vertex takes the new base at its shade of the old
// (luminance over the old base's), keeping the to row's w share of it (BUGS.md #152: one skin).
// dwell_models::recolour mirrors it.
fn remap_colour(c: vec4f) -> vec3f {
    let part = u32(round(c.a * 255.0));
    if (part == 0u || part * 2u > REMAP_ROWS) {
        return c.rgb;
    }
    let was = skin.remap[part * 2u - 2u];
    if (was.w < 0.5) {
        return c.rgb;
    }
    let luma = vec3f(0.2126, 0.7152, 0.0722);
    let to = skin.remap[part * 2u - 1u];
    let shade = mix(1.0, dot(c.rgb, luma) / max(dot(was.rgb, luma), 1e-3), to.w);
    return min(to.rgb * shade, vec3f(1.0));
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
    // The palette colour (remapped for this body), sRGB bytes to linear.
    o.color = pow(remap_colour(v.color), vec3f(2.2));
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
    // Your own body's safety net: nothing within the near margin of the eye (0 for every other).
    if (dot(i.rel, i.rel) < skin.light.z * skin.light.z) {
        discard;
    }
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
    // A hit creature's flash (shared/creatures `hurtLook`): toward its tint, lit as the body is.
    let tint_w = skin.light.w;
    if (tint_w > 0.0) {
        let q = floor(tint_w);
        let tint = vec3f(floor(q / 1024.0), floor(q / 32.0) % 32.0, q % 32.0) / 31.0;
        c = mix(c, tint * mix(0.6, 1.0, smoothstep(0.16, 0.5, w)), min(fract(tint_w) / 0.998, 1.0));
    }
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
        "{}\nconst JOINT_ROWS: u32 = {}u;\nconst REMAP_ROWS: u32 = {}u;\nconst CHARACTER_EXPOSURE: f32 = {:?};\n{WGSL}",
        super::shaders::lighting_source(),
        MAX_JOINTS * 3,
        REMAP_FLOATS / 4,
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
    /// Your own body in first person: drawn by the body's index buffer (`own_index`).
    own: bool,
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
    let parts = data.parts.map_or_else(Vec::new, |p| p.level(level));
    let (v, i) = vertex_bytes(model, level, &parts);
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
    pipeline: crate::render::pipeline::Lazy,
    shadow_pipeline: crate::render::pipeline::Lazy,
    layout: wgpu::BindGroupLayout,
    instances: wgpu::Buffer,
    capacity: u64,
    group: wgpu::BindGroup,
    /// Each body's clip player, by its mesh and its player's (or creature's) id.
    players: HashMap<(usize, u32), Player>,
    /// Each body's idle pose blend ([`blend_pose`]), by its clip player's key.
    idle_poses: HashMap<(usize, u32), (Option<&'static str>, f32)>,
    draws: Vec<Draw>,
    last_s: Option<f64>,
    /// The Model detail setting's level (`dwell_models::DETAIL_LEVELS`), every mesh drawn at it.
    level: usize,
    /// Your first-person arms' triangles (the player model's `firstPerson`, over its vertices) and
    /// their count; None: the player model draws no arms.
    arms_index: Option<(wgpu::Buffer, u32)>,
    /// This frame's arms instance, when drawn.
    arms: Option<u32>,
    /// Your own body's triangles in first person (the player model's `firstPerson.body`), their
    /// count and the vertices they use (skinned on the CPU to keep them clear of the eye); None:
    /// the player model draws no body.
    own_index: Option<(wgpu::Buffer, u32, Vec<u32>)>,
}

/// Your own body to draw in first person from the player model (docs/CHARACTERS.md §5.1, §11.12).
#[derive(Clone, Copy, Debug)]
pub struct SkinnedOwn {
    /// The body as your shadow is drawn (its feet, yaw, face, light and rig pose).
    pub draw: SkinnedDraw,
    /// The camera's look (eye-relative world space).
    pub forward: V3,
    /// How it is kept clear of the eye (characters.json `firstPerson.body`).
    pub body: dwell_characters::FirstPersonBody,
    /// Bit per scene pass it is drawn in.
    pub passes: u8,
}

/// The player model's own-body triangles at `level` and the vertices they use (None: it draws
/// none).
pub fn own_body_mesh(
    data: &ModelData,
    model: &Model,
    level: usize,
) -> Option<(Vec<u32>, Vec<u32>)> {
    let fp = data.first_person?;
    data.retarget?;
    let tris = dwell_models::first_person::body_triangles(model, level, &fp);
    if tris.is_empty() {
        return None;
    }
    let verts = dwell_models::first_person::vertices_of(&tris);
    Some((tris, verts))
}

/// Your own body's joints in first person (each skin joint's matrix to eye-relative world space)
/// and how far back it was moved: the model posed by the rig's pose (as your shadow), its place
/// (`place_frame`) leaned back from its soles (the soles where they stand) by `own::keep_clear`
/// over its kept vertices
/// `verts` at `level`, in the body's frame (so the same on both faces). None: no retarget map or
/// no rig pose.
pub fn own_joints(
    data: &ModelData,
    model: &Model,
    level: usize,
    verts: &[u32],
    own: &SkinnedOwn,
    eye: V3,
) -> Option<(Vec<Mat4>, f32)> {
    let map = data.retarget?;
    let pose = own.draw.rig_pose?;
    let local =
        dwell_models::retarget::pose_from(model, data, &map, dwell_characters::player(), &pose);
    let globals = anim::globals(model, &local);
    let to_body = mm::trs(
        [0.0; 3],
        mm::axis_angle([0.0, 1.0, 0.0], data.yaw),
        [data.scale; 3],
    );
    let in_body: Vec<Mat4> = model
        .skin
        .iter()
        .zip(&model.inverse_bind)
        .map(|(&j, ib)| mm::mul(&to_body, &mm::mul(&globals[j], ib)))
        .collect();
    let points = dwell_models::first_person::skin_points(model, level, &in_body, verts);
    let d = &own.draw;
    let frame = place_frame(d.feet, d.yaw, d.face, eye);
    let (eye_local, pitch) = super::characters::eye_in(&frame, own.forward);
    let (keep, s) = dwell_characters::own::keep_clear(&own.body, points, eye_local, pitch, d.role);
    let placed = mat4(&frame.then(&keep));
    Some((in_body.iter().map(|j| mm::mul(&placed, j)).collect(), s))
}

/// Your first-person arms to draw from the player model (docs/CHARACTERS.md §11.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinnedArms {
    /// View space (+x left, +y up, +z ahead) to eye-relative world space
    /// (`characters::arms_frame`).
    pub frame: Affine,
    /// The first-person clips' pose, the held item's turns on.
    pub pose: dwell_characters::anim::Pose,
    /// The held item's name: the hand's grip.
    pub grip: Option<&'static str>,
    pub light: [f32; 2],
}

/// The arms' index buffer at `level` (None: the model draws no arms).
fn arms_index(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &ModelData,
    model: &Model,
    level: usize,
) -> Option<(wgpu::Buffer, u32)> {
    let fp = data.first_person?;
    let tris = dwell_models::first_person::arm_triangles(model, level, &fp);
    let bytes: Vec<u8> = tris.iter().flat_map(|i| i.to_le_bytes()).collect();
    let b = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("skinned arms indices"),
        size: (bytes.len() as u64).max(4).next_multiple_of(4),
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&b, 0, &bytes);
    Some((b, tris.len() as u32))
}

/// Your own body's index buffer at `level`, its count and vertices (None: the model draws none).
fn own_index(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &ModelData,
    model: &Model,
    level: usize,
) -> Option<(wgpu::Buffer, u32, Vec<u32>)> {
    let (tris, verts) = own_body_mesh(data, model, level)?;
    let bytes: Vec<u8> = tris.iter().flat_map(|i| i.to_le_bytes()).collect();
    let b = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("skinned own body indices"),
        size: (bytes.len() as u64).max(4).next_multiple_of(4),
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&b, 0, &bytes);
    Some((b, tris.len() as u32, verts))
}

/// An affine frame as a 4 × 4 (columns).
pub fn mat4(a: &Affine) -> Mat4 {
    [
        [a.m[0][0], a.m[0][1], a.m[0][2], 0.0],
        [a.m[1][0], a.m[1][1], a.m[1][2], 0.0],
        [a.m[2][0], a.m[2][1], a.m[2][2], 0.0],
        [a.t[0], a.t[1], a.t[2], 1.0],
    ]
}

/// The arms' joints (each joint's matrix to eye-relative world space) and the rig's hand bone's
/// frame on the model's hand joint (where the held item is drawn), from `arms`' pose.
pub fn arms_joints(
    data: &ModelData,
    model: &Model,
    arms: &SkinnedArms,
) -> Option<(Vec<Mat4>, Option<Affine>)> {
    let (globals, place) = dwell_models::first_person::pose(
        model,
        data,
        dwell_characters::player(),
        &arms.pose,
        arms.grip,
    )?;
    let place = mm::mul(&mat4(&arms.frame), &place);
    let hand = hand_frame(data, model, &globals, &place);
    let joints = model
        .skin
        .iter()
        .zip(&model.inverse_bind)
        .map(|(&j, ib)| mm::mul(&place, &mm::mul(&globals[j], ib)))
        .collect();
    Some((joints, hand))
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
    /// Its pipelines, for the renderer's warm-up (pipeline.rs).
    pub(crate) fn pipelines(&self) -> [&crate::render::pipeline::Lazy; 2] {
        [&self.pipeline, &self.shadow_pipeline]
    }

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
        let pipeline = {
            let (device, module, pipeline_layout, targets) = (
                device.clone(),
                module.clone(),
                pipeline_layout.clone(),
                targets.clone(),
            );
            crate::render::pipeline::Lazy::new(move || {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })];
                crate::render::pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
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
                    },
                )
            })
        };
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned shadows"),
            bind_group_layouts: &[Some(shadow_frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = {
            let (device, module, shadow_layout) =
                (device.clone(), module.clone(), shadow_layout.clone());
            crate::render::pipeline::Lazy::new(move || {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })];
                crate::render::pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
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
                        depth_stencil: Some(super::shadows::caster_depth(
                            &dwell_look::shadows::SHADOWS,
                        )),
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    },
                )
            })
        };
        let level = dwell_models::DETAIL_DEFAULT;
        let player = upload(device, queue, data, model, level);
        let arms_index = arms_index(device, queue, data, model, level);
        let own_index = own_index(device, queue, data, model, level);
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
            idle_poses: HashMap::default(),
            draws: Vec::new(),
            last_s: None,
            level,
            arms_index,
            arms: None,
            own_index,
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
        let (data, model) = (self.meshes[0].data, self.meshes[0].model);
        self.arms_index = arms_index(device, queue, data, model, level);
        self.own_index = own_index(device, queue, data, model, level);
    }

    /// Whether the player model draws your own body in first person.
    pub fn has_own_body(&self) -> bool {
        self.own_index.is_some()
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
    /// by its role and writes its joints, and your first-person arms' (`arms`, from the player
    /// model). Returns each body's [`hand_frame`] (None where it is not drawn or holds nothing), in
    /// `bodies`' order, and the arms' hand (None: no arms drawn, or they hold nothing).
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: V3,
        passes: &[(f64, f64)],
        cast_m: f64,
        bodies: &[SkinnedDraw],
        own: Option<&SkinnedOwn>,
        arms: Option<&SkinnedArms>,
        now_s: f64,
    ) -> (Vec<Option<Affine>>, Option<Affine>) {
        let dt = self
            .last_s
            .map_or(0.0, |l| (now_s - l).clamp(0.0, 0.25) as f32);
        self.last_s = Some(now_s);
        self.draws.clear();
        for d in bodies {
            if self.meshes.iter().all(|m| m.data.name != d.model)
                && let Some((data, model)) = dwell_models::model(d.model)
            {
                self.meshes
                    .push(upload(device, queue, data, model, self.level));
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
            let mut place = body_matrix(d, data, eye);
            let blend = self.idle_poses.entry((mesh, d.seed)).or_insert((None, 0.0));
            *blend = blend_pose(*blend, d.idle_pose, dt, data.pose_blend_s);
            let mut local = model_pose(data, model, p, d.rig_pose.as_ref(), d.grip);
            if let (Some(name), w) = *blend
                && let Some(turns) = data.pose(name)
            {
                dwell_models::grip::apply_weighted(model, turns, &mut local, w);
                // Its `$drop`: the body lowered (in the model's units, under its scale).
                if turns.drop > 0.0 {
                    let down = [0.0, -turns.drop * w / data.scale.max(1e-6), 0.0];
                    place = mm::mul(&place, &mm::trs(down, [0.0, 0.0, 0.0, 1.0], [1.0; 3]));
                }
            }
            let globals = anim::globals(model, &local);
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
            floats[at + JOINT_FLOATS + 3] = pack_tint(d.tint);
            floats[at + JOINT_FLOATS + 4..at + JOINT_FLOATS + 4 + REMAP_FLOATS]
                .copy_from_slice(&d.remap);
            self.draws.push(Draw {
                instance: (at / INSTANCE_FLOATS) as u32,
                mesh,
                passes: mask,
                casts,
                cast_only: d.cast_only,
                own: false,
            });
        }
        // Your own body in first person (§11.12): its joints, the near margin in `light.z`.
        if let (Some(o), Some((_, _, verts))) = (own, &self.own_index)
            && o.passes != 0
            && let Some((joints, _)) = own_joints(
                self.meshes[0].data,
                self.meshes[0].model,
                self.level,
                verts,
                o,
                eye,
            )
        {
            let at = floats.len();
            floats.resize(at + INSTANCE_FLOATS, 0.0);
            for (k, j) in joints.iter().take(MAX_JOINTS).enumerate() {
                floats[at + k * 12..at + k * 12 + 12].copy_from_slice(&rows(j));
            }
            let near = super::characters::own_discard_m(&o.body);
            floats[at + JOINT_FLOATS..at + JOINT_FLOATS + 3].copy_from_slice(&[
                o.draw.light[0],
                o.draw.light[1],
                near,
            ]);
            self.draws.push(Draw {
                instance: (at / INSTANCE_FLOATS) as u32,
                mesh: 0,
                passes: o.passes,
                casts: false,
                cast_only: false,
                own: true,
            });
        }
        self.arms = None;
        let mut arms_hand = None;
        if let (Some(a), Some(_)) = (arms, &self.arms_index)
            && let Some((joints, hand)) = arms_joints(self.meshes[0].data, self.meshes[0].model, a)
        {
            let at = floats.len();
            floats.resize(at + INSTANCE_FLOATS, 0.0);
            for (k, j) in joints.iter().take(MAX_JOINTS).enumerate() {
                floats[at + k * 12..at + k * 12 + 12].copy_from_slice(&rows(j));
            }
            floats[at + JOINT_FLOATS..at + JOINT_FLOATS + 2].copy_from_slice(&a.light);
            self.arms = Some((at / INSTANCE_FLOATS) as u32);
            arms_hand = hand;
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
        (hands, arms_hand)
    }

    /// Whether your arms are drawn from the player model this frame.
    pub fn has_arms(&self) -> bool {
        self.arms.is_some()
    }

    /// Draws your first-person arms (group 0 bound to the arms' frame, in the arms pass).
    pub fn draw_arms(&self, pass: &mut wgpu::RenderPass<'_>, calls: &mut u32, tris: &mut u64) {
        let (Some(instance), Some((index, count))) = (self.arms, &self.arms_index) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, &self.group, &[instance * INSTANCE_BYTES as u32]);
        pass.set_vertex_buffer(0, self.meshes[0].vertex.slice(..));
        pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..*count, 0, 0..1);
        *calls += 1;
        *tris += u64::from(*count / 3);
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
        let (index, count) = match (&self.own_index, d.own) {
            (Some((b, n, _)), true) => (b, *n),
            _ => (&m.index, m.count),
        };
        pass.set_vertex_buffer(0, m.vertex.slice(..));
        pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..count, 0, 0..1);
        *calls += 1;
        *tris += u64::from(count / 3);
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

    #[test]
    fn an_idle_pose_blends_in_and_out_and_turns_its_joints() {
        // face-a-only: model space, before any face frame. WORK_QUEUE item 39: a pose fades in
        // over the model's blend, out before another comes in.
        let mut b = (None, 0.0);
        for _ in 0..5 {
            b = blend_pose(b, Some("graze"), 0.1, 0.5);
        }
        assert_eq!(b.0, Some("graze"));
        assert!((b.1 - 1.0).abs() < 1e-5);
        b = blend_pose(b, Some("lookUp"), 0.25, 0.5);
        assert_eq!(b, (Some("graze"), 0.5), "the old pose fades out first");
        b = blend_pose(b, Some("lookUp"), 0.25, 0.5);
        b = blend_pose(b, Some("lookUp"), 0.25, 0.5);
        assert_eq!(b.0, Some("lookUp"));
        assert_eq!(blend_pose((Some("lie"), 0.1), None, 0.2, 0.5), (None, 0.0));
        // The data's graze turns the deer's neck: its head joint moves.
        let (data, model) = dwell_models::model("deer").unwrap();
        let turns = data.pose("graze").expect("the deer grazes by data");
        let p = Player::new(data, model, "idle");
        let base = model_pose(data, model, &p, None, None);
        let mut posed = base.clone();
        dwell_models::grip::apply_weighted(model, turns, &mut posed, 1.0);
        let head = |pose: &[anim::Trs]| {
            let g = anim::globals(model, pose);
            let j = model.skin[turns.turns.last().unwrap().0];
            [g[j][3][0], g[j][3][1], g[j][3][2]]
        };
        let (a, z) = (head(&base), head(&posed));
        let moved = (0..3).map(|k| (a[k] - z[k]).powi(2)).sum::<f32>().sqrt();
        assert!(
            moved > 0.0 || turns.turns.len() > 1,
            "the pose turns its joints"
        );
        let mut half = base.clone();
        dwell_models::grip::apply_weighted(model, turns, &mut half, 0.0);
        assert_eq!(head(&half), a, "weight 0: as the clip");
    }

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
                role: "idle",
                light: [1.0, 0.0],
                cast_only: false,
                rig_pose: None,
                grip: None,
                remap: NO_REMAP,
                idle_pose: None,
                tint: NO_TINT,
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
            role: "walk",
            light: [1.0, 0.0],
            cast_only: false,
            rig_pose: None,
            grip: None,
            remap: NO_REMAP,
            idle_pose: None,
            tint: NO_TINT,
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

    /// The Peasant's right hand at rest in model space: the centre of the bounds of the vertices
    /// weighted most to its hand joint or a joint below it (the fingers), and their bounds' size.
    fn peasant_hand() -> ([f32; 3], [f32; 3]) {
        let (data, model) = dwell_models::player();
        let h = data.hand.unwrap();
        let hand_node = model.skin[h.joint];
        let below = |mut n: usize| loop {
            if n == hand_node {
                return true;
            }
            match model.nodes[n].parent {
                Some(p) => n = p,
                None => return false,
            }
        };
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for (i, p) in model.positions.iter().enumerate() {
            let (j, w) = (0..4)
                .map(|k| (model.joints[i][k], model.weights[i][k]))
                .fold((0, 0.0), |b, x| if x.1 > b.1 { x } else { b });
            if w > 0.5 && below(model.skin[usize::from(j)]) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        assert!(lo[0] < f32::MAX, "the Peasant's hand has vertices");
        (
            std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0),
            std::array::from_fn(|k| hi[k] - lo[k]),
        )
    }

    #[test]
    fn a_tool_is_gripped_in_the_peasants_fist() {
        // both-faces: model space at rest, before the body's face frame (place_frame turns both
        // faces' bodies alike; the_held_item_follows_the_skinned_hand_joint checks the faces).
        // Owner, 2026-11-11: the tools "held incorrectly … properly align them to the hand", in
        // other players' hands too. The grip (props.json `hand`, the rig's hand bone's frame) put
        // on the Peasant's hand.R (models.json `hand`): the fist's centre inside the hand's mesh,
        // and the grip axis across the hand, not along its fingers.
        let (data, model) = dwell_models::player();
        let globals = anim::globals(model, &anim::rest(model));
        let scale = mm::trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [data.scale; 3]);
        let hand = hand_frame(data, model, &globals, &scale).expect("the Peasant holds items");
        let grip = dwell_models::HAND_GRIP;
        let fist = hand.apply(grip.at);
        let (centre, size) = peasant_hand();
        let centre = centre.map(|v| v * data.scale);
        let size = size.map(|v| v * data.scale);
        let d = (0..3)
            .map(|k| (fist[k] - centre[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        eprintln!("fist {fist:?} hand centre {centre:?} size {size:?}: {d:.3} m apart");
        assert!(d < 0.04, "the fist's centre is {d:.3} m from the hand's");
        // The grip axis runs across the hand: square to the way from the wrist to the hand's
        // centre (its fingers' way), within 30 degrees.
        let wrist = hand.t;
        let along = [0, 1, 2].map(|k| centre[k] - wrist[k]);
        let l = (0..3).map(|k| along[k] * along[k]).sum::<f32>().sqrt();
        let axis = hand.rotate(grip.axis);
        let cos = (0..3).map(|k| along[k] * axis[k]).sum::<f32>() / l;
        eprintln!("grip axis {axis:?}, the fingers' way {along:?}: cos {cos:.2}");
        assert!(
            cos.abs() < 30f32.to_radians().sin(),
            "the grip runs along the fingers: {cos}"
        );
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
        let (v, i) = vertex_bytes(model, 0, &[]);
        assert_eq!(v.len(), model.positions.len() * VERTEX_BYTES);
        assert_eq!(i.len(), model.indices.len() * 4);
        // Each Model detail level its own mesh, fewer triangles a level (ADR 0070 §10).
        let mut last = i.len();
        for k in 1..dwell_models::DETAIL_LEVELS.len() {
            let (v, i) = vertex_bytes(model, k, &[]);
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

    // The shadow and other players are the rig's pose on the Peasant (docs/CHARACTERS.md §11.9):
    // for the body's clips at sampled times, the skinned hand (where a held item is drawn) and the
    // rigid body's hand bone (where the first-person body and the arms' items are authored) sit
    // within a tolerance of each other in the world, turned alike, on both faces.
    dwell_world::both_faces! {
        fn the_retargeted_hand_follows_the_rigs_hand_in_the_world(face: FaceFrame) {
            use dwell_characters::anim::{bone_transforms, sample};
            let (data, model) = dwell_models::player();
            let ch = dwell_characters::player();
            let hand_r = ch.bone("hand_r").unwrap();
            let feet = face.point([4.0, 7.0, -2.0]);
            let mut worst = (0.0f32, 0.0f32);
            for role in ["idle", "walk", "crouch_walk", "break", "place", "jump", "swim"] {
                let clip = dwell_characters::TABLES.clip(ch.clip(role).unwrap());
                for s in 0..6 {
                    let pose = sample(clip, clip.duration * s as f32 / 6.0);
                    let d = SkinnedDraw {
                        model: dwell_models::PLAYER,
                        seed: 1,
                        feet,
                        yaw: 30.0,
                        face,
                        role,
                        light: [1.0, 0.0],
                        cast_only: true,
                        rig_pose: Some(pose),
                        grip: None,
                        remap: NO_REMAP,
                        idle_pose: None,
                        tint: NO_TINT,
                    };
                    let place = body_matrix(&d, data, [0.0; 3]);
                    let p = Player::new(data, model, "idle");
                    let globals = anim::globals(model, &model_pose(data, model, &p, d.rig_pose.as_ref(), d.grip));
                    let skinned = hand_frame(data, model, &globals, &place).unwrap();
                    let frame = place_frame(feet, 30.0, face, [0.0; 3]);
                    let rigid = frame.then(&bone_transforms(ch, &pose)[hand_r]);
                    let miss = (0..3).map(|k| (skinned.t[k] - rigid.t[k]).powi(2)).sum::<f32>().sqrt();
                    // The angle between the two frames' axes (their worst axis).
                    let turn = (0..3)
                        .map(|c| {
                            let dot: f32 = (0..3).map(|r| skinned.m[c][r] * rigid.m[c][r]).sum();
                            dot.clamp(-1.0, 1.0).acos().to_degrees()
                        })
                        .fold(0.0, f32::max);
                    worst = (worst.0.max(miss), worst.1.max(turn));
                }
            }
            eprintln!("skinned vs rigid hand: worst {:.3} m, {:.1}°", worst.0, worst.1);
            assert!(worst.0 < 0.2, "{worst:?}");
            assert!(worst.1 < 10.0, "{worst:?}");
        }
    }

    /// The Peasant's eyes in its bind pose (model units) and their joint: models.json's decor piece
    /// that adds them (docs/CHARACTERS.md §11.14), so the test follows the data.
    fn peasant_eyes() -> ([f32; 3], String) {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../../../../shared/models/models.json")).unwrap();
        let piece = json["models"]["peasant"]["decor"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["kind"] == "piece")
            .expect("the Peasant's eyes are a decor piece");
        let at = piece["at"].as_array().unwrap();
        (
            std::array::from_fn(|k| at[k].as_f64().unwrap() as f32),
            piece["joint"].as_str().unwrap().to_string(),
        )
    }

    // Owner, 2026-11-13 (BUGS.md #140): "I think my player character is floating a bit above the
    // ground now. ... It's likely shorter and may need to be scaled up." The Peasant at idle, as
    // another player, your shadow or your whole body from a placed camera sees it: its lowest sole
    // within 2 cm of the feet (the collider's bottom, on the ground), its eyes at the camera's eye
    // height within 3 cm (they were 7.3 cm low at scale 1.04), and your own body in first person
    // standing on the same ground.
    dwell_world::both_faces! {
        fn the_peasant_stands_on_its_soles_with_its_eyes_at_the_cameras(face: FaceFrame) {
            use dwell_characters::anim::sample;
            let (data, model) = dwell_models::player();
            let ch = dwell_characters::player();
            let cfg = dwell_physics::config::PlayerControllerConfig::default();
            let body = ch.first_person.and_then(|f| f.body).unwrap();
            let (eyes, eye_joint) = peasant_eyes();
            let eye_skin = model
                .skin
                .iter()
                .position(|&n| model.nodes[n].name == eye_joint)
                .unwrap();
            let level = dwell_models::DETAIL_DEFAULT;
            let (_, own_verts) = own_body_mesh(data, model, level).unwrap();
            let all: Vec<u32> = (0..model.level(level).positions.len() as u32).collect();
            let feet = face.point([4.0, 7.0, -2.0]);
            let eye = face.point([4.0, 7.0 + f64::from(cfg.body.eye_height), -2.0]);
            let over = |p: [f32; 3]| face.y(f64::from(p[1]) + eye[1]) - face.y(feet[1]);
            let lowest = |pts: Vec<[f32; 3]>| pts.into_iter().map(over).fold(f64::MAX, f64::min);
            let clip = dwell_characters::TABLES.clip(ch.clip("idle").unwrap());
            for s in 0..6 {
                let pose = sample(clip, clip.duration * s as f32 / 6.0);
                let d = SkinnedDraw {
                    model: dwell_models::PLAYER,
                    seed: 1,
                    feet,
                    yaw: 30.0,
                    face,
                    role: "idle",
                    light: [1.0, 0.0],
                    cast_only: false,
                    rig_pose: Some(pose),
                    grip: None,
                    remap: NO_REMAP,
                    idle_pose: None,
                    tint: NO_TINT,
                };
                let place = body_matrix(&d, data, eye);
                let p = Player::new(data, model, "idle");
                let joints: Vec<Mat4> =
                    anim::joint_matrices(model, &model_pose(data, model, &p, d.rig_pose.as_ref(), None))
                        .iter()
                        .map(|j| mm::mul(&place, j))
                        .collect();
                let sole = lowest(dwell_models::first_person::skin_points(model, level, &joints, &all));
                assert!(sole.abs() < 0.02, "t {s}/6: the lowest sole {sole:.3} m over the feet");
                let eye_at = over(mm::point(&joints[eye_skin], eyes));
                let miss = eye_at - f64::from(cfg.body.eye_height);
                assert!(
                    miss.abs() < 0.03,
                    "t {s}/6: the model's eyes {eye_at:.3} m over the feet, the camera's {}",
                    cfg.body.eye_height
                );
                // Your own body in first person (moved back only, along the ground) stands on the
                // same soles as everyone else sees.
                let own = SkinnedOwn {
                    draw: d,
                    forward: face.look(30.0, -60.0),
                    body,
                    passes: 1,
                };
                let (own_joints, _) = own_joints(data, model, level, &own_verts, &own, eye).unwrap();
                let own_sole = lowest(dwell_models::first_person::skin_points(model, level, &own_joints, &own_verts));
                let remote_sole = lowest(dwell_models::first_person::skin_points(model, level, &joints, &own_verts));
                assert!(
                    (own_sole - remote_sole).abs() < 1e-3,
                    "own body's sole {own_sole:.4}, another player's {remote_sole:.4}"
                );
            }
        }
    }

    // Owner, 2026-11-13: "Is the first person model lined up with the ground though? The feet were
    // first person feet that didn't line up / seemed to be floating?" Your own body in first person
    // was moved back whole (0.1 m, 0.4 m looking straight down) to keep it off the eye: its soles
    // stood that far behind you, off their shadow (cast where you stand) and, on a ramp or a slope,
    // over or under the ground there. Now it leans back from its soles: at each look down, idle and
    // walking, on flat ground, a ½ m ramp and a 1 m slope rising ahead, your own body's lowest sole
    // is on the ground just where another player's is (1 cm idle; walking 1 cm + 3 cm × the grade, a foot lifted
    // mid-stride leans a little; it was 5 cm to 40 cm off), and on flat ground at idle within 2 cm of
    // the feet.
    dwell_world::both_faces! {
        fn your_own_bodys_soles_stand_on_the_ground_in_first_person(face: FaceFrame) {
            use dwell_characters::anim::sample;
            let (data, model) = dwell_models::player();
            let ch = dwell_characters::player();
            let cfg = dwell_physics::config::PlayerControllerConfig::default();
            let body = ch.first_person.and_then(|f| f.body).unwrap();
            let level = dwell_models::DETAIL_DEFAULT;
            let (_, own_verts) = own_body_mesh(data, model, level).unwrap();
            let feet_l = [4.0, 7.0, -2.0];
            let feet = face.point(feet_l);
            let eye = face.point([4.0, 7.0 + f64::from(cfg.body.eye_height), -2.0]);
            let yaw = 30.0f64;
            let ahead = [yaw.to_radians().sin(), yaw.to_radians().cos()];
            // How high each point is over a ground rising `grade` m per m ahead through the feet.
            let over = |pts: Vec<[f32; 3]>, grade: f64| {
                pts.into_iter()
                    .map(|p| {
                        let l = face.point(std::array::from_fn(|k| f64::from(p[k]) + eye[k]));
                        let along = (l[0] - feet_l[0]) * ahead[0] + (l[2] - feet_l[2]) * ahead[1];
                        l[1] - (feet_l[1] + grade * along)
                    })
                    .fold(f64::MAX, f64::min)
            };
            for (role, samples) in [("idle", 4), ("walk", 8)] {
                let clip = dwell_characters::TABLES.clip(ch.clip(role).unwrap());
                for s in 0..samples {
                    let pose = sample(clip, clip.duration * s as f32 / samples as f32);
                    let d = SkinnedDraw {
                        model: dwell_models::PLAYER,
                        seed: 1,
                        feet,
                        yaw,
                        face,
                        role,
                        light: [1.0, 0.0],
                        cast_only: false,
                        rig_pose: Some(pose),
                        grip: None,
                        remap: NO_REMAP,
                        idle_pose: None,
                        tint: NO_TINT,
                    };
                    let place = body_matrix(&d, data, eye);
                    let p = Player::new(data, model, role);
                    let joints: Vec<Mat4> = anim::joint_matrices(
                        model,
                        &model_pose(data, model, &p, d.rig_pose.as_ref(), None),
                    )
                    .iter()
                    .map(|j| mm::mul(&place, j))
                    .collect();
                    for pitch in [0.0, -60.0, -89.0] {
                        let own = SkinnedOwn {
                            draw: d,
                            forward: face.look(yaw, pitch),
                            body,
                            passes: 1,
                        };
                        let (own_joints, back) =
                            own_joints(data, model, level, &own_verts, &own, eye).unwrap();
                        assert!(back >= body.back, "{role} {s}: kept back {back}");
                        let pts = |j: &[Mat4]| {
                            dwell_models::first_person::skin_points(model, level, j, &own_verts)
                        };
                        for grade in [0.0, 0.5, 1.0] {
                            let mine = over(pts(&own_joints), grade);
                            let theirs = over(pts(&joints), grade);
                            assert!(
                                (mine - theirs).abs() < if role == "idle" { 0.01 } else { 0.01 + 0.03 * grade },
                                "{role} t {s}/{samples}, pitch {pitch}, grade {grade}: your sole \
                                 {mine:.3} m over the ground, another player's {theirs:.3}"
                            );
                            if role == "idle" && grade == 0.0 {
                                assert!(mine.abs() < 0.02, "idle: your sole {mine:.3} m over the ground");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_fist_closes_round_the_handle_and_an_open_hand_does_not() {
        // both-faces: model space at rest, before the body's face frame (as
        // a_tool_is_gripped_in_the_peasants_fist).
        // models.json `grips`: a tool's fist curls the Peasant's fingers so their tips close round
        // the handle's line (props.json `hand`: the grip's centre and axis in the rig's hand bone,
        // on hand.R by models.json `hand`); a block is held in the open hand.
        let (data, model) = dwell_models::player();
        let scale = mm::trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [data.scale; 3]);
        let tip_joint = model
            .skin
            .iter()
            .position(|&n| model.nodes[n].name == "finger.002.R")
            .expect("the Peasant's fingertips");
        let tips_from_handle = |item: &str| {
            let mut pose = anim::rest(model);
            if let Some(g) = data.grip(item) {
                dwell_models::grip::apply(model, g, &mut pose);
            }
            let joints = anim::joint_matrices(model, &pose);
            let mut c = [0.0f32; 3];
            let mut n = 0.0;
            for v in 0..model.positions.len() {
                let (j, w) = (0..4)
                    .map(|k| (model.joints[v][k], model.weights[v][k]))
                    .fold((0, 0.0), |b, x| if x.1 > b.1 { x } else { b });
                if usize::from(j) == tip_joint && w > 0.5 {
                    let p = mm::point(&scale, anim::skin_point(model, &joints, v));
                    for k in 0..3 {
                        c[k] += p[k];
                    }
                    n += 1.0;
                }
            }
            let c = c.map(|x| x / n);
            let hand = hand_frame(data, model, &anim::globals(model, &pose), &scale).unwrap();
            let grip = dwell_models::HAND_GRIP;
            let (at, axis) = (hand.apply(grip.at), hand.rotate(grip.axis));
            let d = [0, 1, 2].map(|k| c[k] - at[k]);
            let along: f32 = (0..3).map(|k| d[k] * axis[k]).sum();
            (0..3)
                .map(|k| (d[k] - along * axis[k]).powi(2))
                .sum::<f32>()
                .sqrt()
        };
        for item in ["stone_axe", "stone_axe_vertical", "tool", "brush"] {
            let d = tips_from_handle(item);
            eprintln!("{item}: fingertips {d:.3} m from the handle's line");
            assert!(
                d < 0.025,
                "{item}: the fingertips are {d:.3} m from the handle"
            );
        }
        for item in ["block", "torch"] {
            assert!(data.grip(item).is_none(), "{item} is held in the open hand");
            assert!(tips_from_handle(item) > 0.06, "the open hand is open");
        }
    }
}

#[cfg(test)]
mod villager_colour_tests {
    // The skinned module (with the hurt flash's unpacking) parses and validates.
    #[test]
    fn the_skinned_module_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(&source()).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    // WORK_QUEUE item 35, round 2: the hurt flash's tint survives its packing into one float.
    #[test]
    fn a_tint_packs_into_one_float_and_back() {
        assert_eq!(pack_tint(NO_TINT), 0.0);
        for t in [
            [0.878, 0.251, 0.165, 0.6],
            [1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 0.01],
        ] {
            let w = pack_tint(t);
            let q = w.floor();
            let back =
                [(q / 1024.0).floor(), (q / 32.0).floor() % 32.0, q % 32.0].map(|v| v / 31.0);
            for k in 0..3 {
                assert!(
                    (back[k] - t[k]).abs() <= 0.5 / 31.0 + 1e-6,
                    "{t:?} -> {back:?}"
                );
            }
            let s = (w.fract() / 0.998).min(1.0);
            assert!((s - t[3]).abs() < 0.01, "{t:?}: strength {s}");
        }
    }

    use super::*;
    use dwell_sim::creatures::{Table, npc_colours};

    /// A colour's base (a palette ramp's, or `#rrggbb`), sRGB 0..1.
    fn base(ramp: &str) -> [f32; 3] {
        let c = dwell_models::colour_base(ramp).unwrap();
        [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32 / 255.0)
    }

    #[test]
    fn every_villager_variant_draws_its_face_in_its_skin_and_its_hair_in_its_hair() {
        // both-faces: model space at rest, before the body's face frame (the colour of a vertex
        // does not depend on the face). BUGS.md #96 (owner, 2026-11-12): "Why do all the villagers
        // look like their faces are jet black? Not even matching the rest of their skin color?",
        // then "their hair and faces are really crazy looking … some of the clothes and hat
        // vertices are being assigned incorrectly". Two causes: the GLB's colours are linear and
        // the shader read them as sRGB (skin_deep drew at a tenth of its brightness: black), and
        // the recolour went by colour, so the hat's repaint by height had caught the hair's fringe
        // and the neck's skin_tan kept its colour. Each vertex is found by position (its head
        // joints' weight, the front of the face, the back of the head), at every detail level.
        let (data, model) = dwell_models::model("peasant").unwrap();
        let parts = data.parts.expect("the Peasant has a part map");
        let def = Table::SHIPPED.npc("dwell:villager").unwrap();
        let joint = |name: &str| {
            model
                .skin
                .iter()
                .position(|&n| model.nodes[n].name == name)
                .unwrap() as u8
        };
        let head = [joint("spine.005"), joint("spine.006")];
        let hat = parts.number("hat").unwrap();
        let clothes = [
            parts.number("shirt").unwrap(),
            parts.number("trousers").unwrap(),
        ];
        for level in 0..dwell_models::DETAIL_LEVELS.len() {
            let m = model.level(level);
            let (v, _) = vertex_bytes(model, level, &parts.level(level));
            let colour = |i: usize| {
                let c = &v[i * VERTEX_BYTES + 24..i * VERTEX_BYTES + 28];
                ([c[0], c[1], c[2]].map(|b| f32::from(b) / 255.0), c[3])
            };
            let on_head = |i: usize| {
                (0..4)
                    .filter(|&k| head.contains(&m.joints[i][k]))
                    .map(|k| m.weights[i][k])
                    .sum::<f32>()
                    >= 0.5
            };
            let n = m.positions.len();
            // The face: the head's front at nose height, near the middle (within 3 cm of the
            // frontmost there); the hair: the back of the head above the neck, inside the brim.
            let band = |i: usize| {
                let p = m.positions[i];
                on_head(i) && (1.36..1.46).contains(&p[1]) && p[0].abs() < 0.08
            };
            let front = (0..n)
                .filter(|&i| band(i))
                .map(|i| m.positions[i][2])
                .fold(f32::MIN, f32::max);
            let face: Vec<usize> = (0..n)
                .filter(|&i| band(i) && m.positions[i][2] > front - 0.03)
                .collect();
            let back: Vec<usize> = (0..n)
                .filter(|&i| {
                    let p = m.positions[i];
                    on_head(i)
                        && (1.38..1.46).contains(&p[1])
                        && p[0].abs() < 0.12
                        && (-0.2..-0.09).contains(&p[2])
                })
                .collect();
            assert!(
                face.len() >= 3 && back.len() >= 3,
                "level {level}: {} face, {} hair",
                face.len(),
                back.len()
            );
            for i in 0..n {
                let (_, part) = colour(i);
                if on_head(i) {
                    assert!(
                        !clothes.contains(&part),
                        "level {level}: head vertex {i} is clothes"
                    );
                }
                if part == hat {
                    assert!(
                        m.positions[i][1] >= 1.44,
                        "level {level}: hat vertex {i} below the brim"
                    );
                }
            }
            for variant in 0..64u32 {
                let colours = npc_colours(def, variant.wrapping_mul(0x9e37_79b9));
                let to = |p: &str| colours.iter().find(|c| c.0 == p).unwrap().2;
                let remap = dwell_models::part_remap(Some(&parts), &colours);
                // What the shader draws: the recolour, then the sRGB bytes to linear.
                let drawn = |i: usize| {
                    let (c, part) = colour(i);
                    dwell_models::recolour(c, part, &remap).map(|c| c.powf(2.2))
                };
                // The face: the skin's ramp at the vertex's shade of the model's (its nose and
                // cheeks a little darker), drawn as bright as the ramp is, never black.
                let skin = base(to("skin"));
                let want = dwell_models::luma(skin.map(|c| c.powf(2.2)));
                let mut lit = 0.0;
                for &i in &face {
                    let (c, part) = colour(i);
                    let d = dwell_models::recolour(c, part, &remap);
                    let r = [0, 1, 2].map(|k| d[k] / skin[k]);
                    assert!(
                        r.iter().all(|x| (x - r[0]).abs() < 0.05) && (0.6..1.1).contains(&r[0]),
                        "level {level} variant {variant}: face vertex {i} draws {d:?}, not its skin {} {skin:?}",
                        to("skin")
                    );
                    lit += dwell_models::luma(drawn(i)) / face.len() as f32;
                }
                assert!(
                    lit > 0.7 * want,
                    "level {level} variant {variant}: the face draws at {lit}, its skin {} at {want}",
                    to("skin")
                );
                // The hair: the hair's ramp at the vertex's shade (the colour over the ramp's
                // base the same in every channel).
                let hair = base(to("hair"));
                for &i in &back {
                    let (c, _) = colour(i);
                    let d = dwell_models::recolour(c, colour(i).1, &remap);
                    let r = [0, 1, 2].map(|k| d[k] / hair[k].max(1e-3));
                    assert!(
                        r.iter()
                            .all(|x| (x - r[0]).abs() < 0.05 || d.iter().any(|&c| c >= 1.0)),
                        "level {level} variant {variant}: hair vertex {i} draws {d:?}, not its hair {}",
                        to("hair")
                    );
                }
            }
        }
    }

    #[test]
    fn every_skin_vertex_of_a_villager_draws_its_one_skin_tone() {
        // both-faces: model space at rest (a vertex's colour does not depend on the face).
        // BUGS.md #152 (owner, 2026-11-13): "face should always match arms, etc. shouldn't have
        // different skin colors on different skin parts." The Peasant's skin part is modelled in
        // two ramps (skin_fair face and arms, a skin_tan neck and jaw a ramp darker), and the
        // recolour kept that whole step, so the neck and jaw read as another skin. Across 200
        // seeded villagers, at every detail level, every vertex of the skin part draws the
        // villager's one tone (its appearance pick) at a shade within a few percent of it.
        let (data, model) = dwell_models::model("peasant").unwrap();
        let parts = data.parts.expect("the Peasant has a part map");
        let skin = parts.number("skin").unwrap();
        let def = Table::SHIPPED.npc("dwell:villager").unwrap();
        let a = def.appearance.expect("villagers have an appearance set");
        for level in 0..dwell_models::DETAIL_LEVELS.len() {
            let (v, _) = vertex_bytes(model, level, &parts.level(level));
            let n = v.len() / VERTEX_BYTES;
            for variant in 0..200u32 {
                let seed = variant.wrapping_mul(0x9e37_79b9);
                let colours = npc_colours(def, seed);
                let (tone, _) = dwell_sim::creatures::npc_appearance(&a, seed);
                let want = base(tone.base);
                let remap = dwell_models::part_remap(Some(&parts), &colours);
                for i in 0..n {
                    let c = &v[i * VERTEX_BYTES + 24..i * VERTEX_BYTES + 28];
                    if c[3] != skin {
                        continue;
                    }
                    let d = dwell_models::recolour(
                        [c[0], c[1], c[2]].map(|b| f32::from(b) / 255.0),
                        c[3],
                        &remap,
                    );
                    let k = [0, 1, 2].map(|j| d[j] / want[j].max(1e-3));
                    assert!(
                        k.iter()
                            .all(|x| (x - k[0]).abs() < 0.02 || d.iter().any(|&c| c >= 1.0))
                            && (0.85..1.06).contains(&k[0]),
                        "level {level} variant {variant}: skin vertex {i} draws {d:?}, not its tone {} {want:?} (x{k:?})",
                        tone.name
                    );
                }
            }
        }
    }
}
