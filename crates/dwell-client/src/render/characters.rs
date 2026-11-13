//! Characters in the raster renderer (docs/CHARACTERS.md §9.1–9.2, Phase 40 C1): other players'
//! full bodies in the scene passes, and your own first-person arms in a pass of their own.
//!
//! - **Meshes**: one per look (or item) and view, built once by `dwell-characters` from the
//!   generated data, each vertex on one bone (rigid parts, no skinning). A character draws its
//!   body and its held item: two indexed draws over one block of bone transforms.
//! - **Shading** (`character_fs`): the material's ramp (shadow → base → light by a half-Lambert
//!   wrap of the key light), its brush tile at its `overlay`, a faint warm rim, all scaled by the
//!   frame's light on that face so day and night reach characters as they reach terrain. The held block (`$block`) is
//!   its own atlas tiles lit like terrain. Fragments write the frame's colour and view depth, so
//!   the composite tone-maps, fogs, inks and edge-blurs them like everything else.
//! - **Outline**: a second, back-facing draw pushed out along the normals in `ink_soft`
//!   (an inverted hull), dropped past `OUTLINE_M`; small parts drop there too.
//! - **Arms**: drawn after the world's passes into the same targets over a cleared depth buffer,
//!   with their own projection (the data's FOV, a 2 cm near plane), so they never clip into
//!   terrain and keep their size under the speed FOV.
//! - **Your own body** (§5.1): in first person, the full body from the neck down in the scene
//!   passes (`BodyDraw::first_person`). Since 0.54 the player model's (render/skinned.rs,
//!   `SkinnedOwn`, §11.12); with `?fparms=rigid` (`rigid_arms`) the rigid one: its own mesh (`dwell_characters::own`: no head, neck or
//!   arms), moved back from the eye each frame (`own_frame`) so nothing comes near the camera, and
//!   its fragments within `own_discard_m` of the eye discarded as a safety net. Its shadow is the
//!   whole body's, cast separately (`cast_only`).

use dwell_characters::anim::{MAX_BONES, Pose, arm_transforms, bone_transforms, with_item};
use dwell_characters::math::Affine;
use dwell_characters::mesh::{self, Mesh};
use dwell_characters::{BLOCK_MATERIAL, Character, FirstPersonBody, TABLES, TILE, View, own};
use dwell_world::FaceFrame;
use dwell_world::hash::HashMap;

use super::camera::{Basis, V3};
use super::frame::{DEPTH, DEPTH_CLEAR, DEPTH_COMPARE, HDR, VIEW_DEPTH};

/// Past this (m) a character drops its outline and its small parts.
pub const OUTLINE_M: f64 = 64.0;
/// Past this (m) a character isn't drawn.
pub const DRAW_M: f64 = 512.0;
/// Parts smaller than this (m, their largest side) are left out of the far mesh.
pub const SMALL_PART_M: f32 = 0.06;
/// Whether characters, your arms and held items draw their ink outline. Off for now (owner,
/// 2026-10-21): the hull showed through cutout held items (grass, leaves) as dark faces.
pub const OUTLINES: bool = false;

/// Whether a character's draw takes its outline: never while [`OUTLINES`] is off, and not past
/// [`OUTLINE_M`].
fn outline_for(far: bool) -> bool {
    OUTLINES && !far
}

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
    /// A tool (ADR 0067): the item of the characters data its hand shows (the axe's `held`).
    Tool(&'static str),
    /// An inventory item with a shaped look (`shared/inventory` `look`: a stick, a rock), by name:
    /// the hand shows the characters item of its look's shape (`rod`, `lump`) in its look's tile
    /// and tint, the same look it has on the hotbar, loose and on the ground.
    Look(&'static str),
    /// A tool with a model look (the tool models: shared/models/props.json): the characters item
    /// `hold` gives the hand's pose (the axe's `held` for its swing, `stone_axe` for the others)
    /// and the prop `model` is drawn on that hold's bone at its `grip`.
    Model {
        hold: &'static str,
        model: &'static str,
    },
}

impl Held {
    /// Whether this is a weapon in hand (a tool of kind `Weapon`, shared/tools, by the characters
    /// item its hand shows): defeat plants it (CHARACTERS.md §11.13, `kneel_sword`).
    pub fn is_weapon(self) -> bool {
        let hold = match self {
            Held::Tool(h) | Held::Model { hold: h, .. } => h,
            _ => return false,
        };
        dwell_protocol::chopping::TOOLS.iter().any(|t| {
            t.kind == dwell_protocol::chopping::ToolKind::Weapon
                && (t.held == Some(hold) || t.held_vertical == Some(hold))
        })
    }
}

/// The hold a model look's tool is held in when no tool names one (a pickaxe, a shovel): the axe's
/// level hold, a one-handed tool held toward its foot.
pub const MODEL_HOLD: &str = "tool";

/// What a hand shows holding item `item` (by name) in `hold` (its tool's `held`, if it has one):
/// its model on the hold when its look is a model, else the tool's hold or its look.
pub fn held_item(item: &'static str, hold: Option<&'static str>) -> Held {
    let look = dwell_protocol::inventory::item(item).and_then(|d| d.look);
    match (look, hold) {
        (Some(l), _) if !l.model.is_empty() && dwell_models::prop(l.model).is_some() => {
            Held::Model {
                hold: hold.unwrap_or(MODEL_HOLD),
                model: l.model,
            }
        }
        (_, Some(h)) => Held::Tool(h),
        _ => Held::Look(item),
    }
}

/// The characters item a look's shape is held as; None for a model look (drawn as
/// [`Held::Model`] instead).
pub fn look_item(shape: dwell_protocol::inventory::LookShape) -> Option<&'static str> {
    match shape {
        dwell_protocol::inventory::LookShape::Rod => Some("rod"),
        // A shard is held as a lump is: in the fist, its own tile and tint.
        dwell_protocol::inventory::LookShape::Lump
        | dwell_protocol::inventory::LookShape::Shard => Some("lump"),
        dwell_protocol::inventory::LookShape::Cluster => Some("cluster"),
        dwell_protocol::inventory::LookShape::Model => None,
    }
}

impl Held {
    /// The item's name in the data (`block`, `brush`, or the block's own `held` item, such as
    /// `torch`; docs/LIGHTS.md), and its block.
    pub fn item(self) -> Option<(&'static str, u16)> {
        match self {
            Held::Empty => None,
            Held::Block(b) => Some((
                dwell_blocks::try_info(b)
                    .and_then(|i| i.held)
                    .unwrap_or("block"),
                b,
            )),
            Held::Brush(b) => Some(("brush", b)),
            Held::Tool(item) => Some((item, 0)),
            Held::Model { hold, .. } => Some((hold, 0)),
            Held::Look(item) => dwell_protocol::inventory::item(item)
                .and_then(|d| d.look)
                .and_then(|l| Some((look_item(l.shape)?, 0))),
        }
    }

    /// The `$block` material's tiles and colour for the shader: the held block's, or a held look's
    /// texture block's tinted its colour.
    pub fn uniforms(self) -> [[f32; 4]; 4] {
        if let Held::Look(item) = self
            && let Some(l) = dwell_protocol::inventory::item(item).and_then(|d| d.look)
        {
            let block = dwell_blocks::find_block(l.texture).map_or(0, |b| b.first_state);
            let mut u = block_uniforms(block);
            let tint = l
                .color
                .map(|c| dwell_look::srgb_to_linear(f64::from(c) / 255.0) as f32);
            u[3] = [tint[0], tint[1], tint[2], u[3][3]];
            return u;
        }
        block_uniforms(self.item().map_or(0, |(_, b)| b))
    }

    /// The prop drawn in the hand ([`Held::Model`]), if any.
    pub fn model(self) -> Option<&'static str> {
        match self {
            Held::Model { model, .. } => Some(model),
            _ => None,
        }
    }

    /// The light source a hand holding it gives off (the block's `light`; not the brush's).
    pub fn light(self) -> Option<&'static dwell_blocks::LightInfo> {
        match self {
            Held::Block(b) => dwell_blocks::try_info(b).and_then(|i| i.light),
            _ => None,
        }
    }
}

/// A light a hand holds this frame (docs/LIGHTS.md §3): where its item's light point is, relative
/// to the eye, the source and its holder's flicker phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeldLight {
    pub rel: [f32; 3],
    pub source: &'static dwell_blocks::LightInfo,
    pub seed: u32,
}

/// The item a hand of `character` shows for `held`: the held block's own (`torch`) where the
/// character has it, else the block itself.
pub fn item_shown(character: &Character, held: Held) -> Option<&'static str> {
    held.item().map(|(name, _)| {
        if character.item(name).is_some() {
            name
        } else {
            "block"
        }
    })
}

/// The light a hand holding `held` gives off, posed by `bones` (relative to the eye): at its item's
/// light point (docs/LIGHTS.md §3), with flicker phase `seed`. None without a light or a point.
pub fn held_light(
    character: &Character,
    held: Held,
    bones: &[Affine; MAX_BONES],
    seed: u32,
) -> Option<HeldLight> {
    let source = held.light()?;
    let (bone, at) = character.item(item_shown(character, held)?)?.light?;
    Some(HeldLight {
        rel: bones.get(bone)?.apply(at),
        source,
        seed,
    })
}

/// A full body to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyDraw {
    pub feet: V3,
    /// Degrees; 0 faces +z.
    pub yaw: f64,
    /// The face it stands on: upside down on face B.
    pub face: FaceFrame,
    /// The look's name (unknown names draw the default look).
    pub look: &'static str,
    pub pose: Pose,
    pub held: Held,
    /// Drawn into the shadow maps only (your own body in first person).
    pub cast_only: bool,
    /// Your own body in first person (CHARACTERS.md §5.1): drawn from the neck down, kept clear of
    /// the eye, casting no shadow (the `cast_only` body casts it).
    pub first_person: bool,
    /// The flood light it stands in (sky and block level / 15, eased; [1, 0] open sky):
    /// `dwell_world::light::LightMap::sample`, docs/DAY_NIGHT.md §11.
    pub light: [f32; 2],
    /// A stable id (its player's): the phase of a held light's flicker.
    pub seed: u32,
    /// The base role playing (`walk`, `sprint`): another player's skinned body plays its clip
    /// (render/skinned.rs).
    pub role: &'static str,
}

/// The first-person arms to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmsDraw {
    pub pose: Pose,
    pub held: Held,
    /// The flood light at your eye (as `BodyDraw::light`).
    pub light: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeshKey {
    /// A look's name, or an item's.
    name: &'static str,
    item: bool,
    view: View,
    /// The far mesh (small parts left out).
    far: bool,
    /// Your own body in first person (`own::build`).
    own: bool,
    /// A held model (a prop, [`Held::Model`]) drawn in place of the item's parts.
    model: Option<&'static str>,
}

struct GpuMesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    count: u32,
    bytes: u64,
}

struct Draw {
    instance: u32,
    /// None: a held item alone (in a skinned body's hand).
    body: Option<MeshKey>,
    item: Option<MeshKey>,
    outline: bool,
    /// Bit per scene pass it is drawn in (far 1, near 2).
    passes: u8,
    /// Within the shadow distance: drawn into the shadow maps.
    casts: bool,
    /// Your own body, drawn into the shadow maps only (into the nearest cascade).
    cast_only: bool,
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
    pipeline: crate::render::pipeline::Lazy,
    outline_pipeline: crate::render::pipeline::Lazy,
    /// The bodies into the shadow maps (depth only, from the light).
    shadow_pipeline: crate::render::pipeline::Lazy,
    layout: wgpu::BindGroupLayout,
    materials: wgpu::Buffer,
    tiles: wgpu::TextureView,
    sampler: wgpu::Sampler,
    instances: wgpu::Buffer,
    capacity: u64,
    group: wgpu::BindGroup,
    meshes: HashMap<MeshKey, GpuMesh>,
    /// Your own body's meshes on the CPU too, by look: `own_frame` reads their vertices.
    own_meshes: HashMap<&'static str, Mesh>,
    draws: Vec<Draw>,
    arms: Option<Draw>,
    /// The lights held this frame (`prepare`): other players', and yours once.
    held_lights: Vec<HeldLight>,
    /// How many of `held_lights` are other players' (their items drawn in the scene), and your
    /// arms' light when the arms are drawn: where flames burn (render/flames.rs).
    others_lights: usize,
    arms_light: Option<HeldLight>,
    /// Other players' bodies and your shadow, skinned (docs/CHARACTERS.md §11): the rigid parts are
    /// kept for your first-person arms and body, and the held items (drawn on the skinned hand).
    skinned: super::skinned::SkinnedRenderer,
    /// This frame's creatures and their light (ADR 0073), set by the game.
    pub creatures: Vec<(crate::creatures::CreatureView, [f32; 2])>,
    pub stats: CharacterStats,
    /// Your first-person arms and your own body drawn from the rigid parts, not the player model
    /// (`?fparms=rigid`, a dev switch for comparison; docs/CHARACTERS.md §11.10, §11.12).
    pub rigid_arms: bool,
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
    // x the radius (m) about the eye within which fragments are discarded (your own body; 0 none),
    // y the outline's width (m), z 1 for the arms.
    cfg: vec4f,
    // The flood light it stands in: x sky, y block level / 15 (docs/DAY_NIGHT.md §11).
    light: vec4f,
};
struct Material {
    shadow: vec4f,
    base: vec4f,
    light: vec4f,
    // x overlay, y the brush tile's layer (−1 none), z how much of its base it gives off unlit (a
    // flame; shared/textures `emissive`)
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
    // The light on a surface facing up there, / π (what lambert gives a white top face), its
    // direct term shaded by `shadow` (`shadow_light`).
    level: vec3f,
};
fn key_light(frag_y: f32, l: TerrainLight) -> Key {
    let face = face_at(frag_y);
    let d = frame.day;
    var k: Key;
    k.dir = select(d.moon.xyz, d.sun.xyz, own(d.sun.xyz, face) > 0.01);
    k.level = lambert_lit(vec3f(1.0), face_up(face), frag_y, l);
    return k;
}

// A character's light: the flood light it stands in (`inst.light`, eased on the CPU) as terrain
// takes it, its direct share the shadow maps' near the eye and the flood light's beyond.
fn character_light(rel: vec3f, n: vec3f) -> TerrainLight {
    var l = terrain_light(1.0, inst.light.xy);
    l.direct = shadow_light(rel, n, l.direct);
    return l;
}

// The ramp's light in shadow: a body in the shade of a tree or a wall shows its ramp's middle at
// most (the key light's side is shaded as the rest), by the shadow's share.
fn shaded_ramp(w: f32, shadow: f32) -> f32 {
    return mix(min(w, 0.45), w, shadow);
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
    if (length(i.rel) < inst.cfg.x) {
        discard;
    }
    let mat = i.packed >> 16u;
    let side = (i.packed >> 8u) & 0xffu;
    let n = normalize(select(-i.normal, i.normal, front));
    let frag_y = frame.eye.x + i.rel.y;
    let light = character_light(i.rel, n);
    let shadow = light.direct;
    // The dynamic lights (held torches; docs/LIGHTS.md §3).
    let points = point_lights(i.rel, n);
    var o: SceneOut;
    o.depth = frame_depth(view_depth(i.rel), true);
    if (mat == BLOCK_MATERIAL) {
        // The held block: its own tile on each side (top, bottom, sides), lit like terrain.
        let tile = select(select(inst.block_side, inst.block_bottom, side == 3u), inst.block_top, side == 2u);
        let uv = tile.xy + clamp(i.uv, vec2f(0.002), vec2f(0.998)) * tile.zw;
        let texel = textureSampleLevel(atlas, atlas_sampler, atlas_crisp(uv), 0.0);
        let has = inst.block_color.w > 0.5;
        if (has && texel.a < 0.5) {
            discard;
        }
        let albedo = inst.block_color.rgb * select(vec3f(1.0), texel.rgb, has);
        var lit = light;
        lit.points = points;
        let shine = select(0.0, tile_glow(tile.xy), has);
        o.color = vec4f(emitted(lambert_lit(albedo, n, frag_y, lit), albedo, shine), 1.0);
        o.depth = self_lit_depth(o.depth, shine >= 1.0);
        return o;
    }
    let m = materials[mat];
    let key = key_light(frag_y, light);
    let w = shaded_ramp(dot(n, key.dir) * 0.5 + 0.5, shadow);
    var c = mix(m.shadow.rgb, m.base.rgb, smoothstep(0.16, 0.5, w));
    c = mix(c, m.light.rgb, smoothstep(0.66, 0.98, w) * 0.8);
    c = c * mix(0.9, 1.0, face_rise(frame.eye.y, n) * 0.5 + 0.5);
    let layer = i32(m.extra.y);
    if (layer >= 0) {
        c = c * mix(1.0, brush_k(i, layer), m.extra.x);
    }
    let rim = pow(1.0 - max(dot(n, normalize(-i.rel)), 0.0), 3.0);
    c = c + m.light.rgb * rim * 0.14 * smoothstep(0.3, 0.6, w);
    // The key light's ramp and the dynamic lights by the true normal; a glowing material (a
    // flame) gives off its base colour × its glow in place of its share of the lit ramp.
    // A glowing material (a flame) is self-lit by its emissive: its base colour whatever the light.
    let lit = c * (key.level * PI + points) * CHARACTER_EXPOSURE;
    o.color = vec4f(emitted(lit, m.base.rgb, m.extra.z), 1.0);
    o.depth = self_lit_depth(o.depth, m.extra.z >= 1.0);
    return o;
}

@fragment
fn character_outline_fs(i: CharacterOut) -> SceneOut {
    if (length(i.rel) < inst.cfg.x) {
        discard;
    }
    var o: SceneOut;
    let frag_y = frame.eye.x + i.rel.y;
    let level = key_light(frag_y, character_light(i.rel, face_up(face_at(frag_y)))).level * PI;
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

/// A material table row (linear colours, overlay, tile, glow).
fn material_rows() -> Vec<f32> {
    let lin = |c: u32| dwell_look::hex(c).map(|v| dwell_look::srgb_to_linear(v) as f32);
    let mut out = Vec::new();
    for m in TABLES.materials {
        for c in [m.shadow, m.base, m.light] {
            out.extend(lin(c));
            out.push(1.0);
        }
        out.extend([m.overlay, m.tile as f32, m.emissive, 0.0]);
    }
    // The held models' colours (each a palette ramp's base, linear), after the data's: a shade
    // darker in shadow and a touch lighter lit, no overlay, tile or glow.
    for c in prop_colours() {
        for k in [PROP_SHADOW, 1.0, PROP_LIGHT] {
            out.extend(c.map(|v| (v * k).min(1.0)));
            out.push(1.0);
        }
        out.extend([0.0, -1.0, 0.0, 0.0]);
    }
    out
}

/// A held model's shadow and lit tones, as multiples of its colour (linear).
const PROP_SHADOW: f32 = 0.62;
const PROP_LIGHT: f32 = 1.18;

/// Every colour of the props an item's look names (linear), in a fixed order: material rows
/// `TABLES.materials.len() + k`.
fn prop_colours() -> &'static [[f32; 3]] {
    static COLOURS: std::sync::OnceLock<Vec<[f32; 3]>> = std::sync::OnceLock::new();
    COLOURS.get_or_init(|| {
        let mut out: Vec<[f32; 3]> = Vec::new();
        for item in dwell_protocol::inventory::ITEMS {
            let Some(l) = item.look.filter(|l| !l.model.is_empty()) else {
                continue;
            };
            let Some((_, m)) = dwell_models::prop(l.model) else {
                continue;
            };
            for c in &m.colors {
                if !out.iter().any(|o| same_colour(o, c)) {
                    out.push(*c);
                }
            }
        }
        out
    })
}

fn same_colour(a: &[f32; 3], b: &[f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
}

/// The mesh of prop `model` held in characters item `hold`: on the hold's bone (the hand), its
/// handle laid along the hand's grip axis with its grip at the fist's centre and rolled by its
/// `hold` ([`dwell_models::PropData::in_hand`], props.json `hand`), flat shaded, each face its
/// colour's material row ([`prop_colours`]). How it leans is the hold's pose of the hand.
pub fn prop_mesh(character: &Character, hold: &str, model: &str) -> Mesh {
    let mut out = Mesh::default();
    let Some((data, m)) = dwell_models::prop(model) else {
        return out;
    };
    let Some((bone, _)) = character.item(hold).map(|i| i.socket) else {
        return out;
    };
    let hand = dwell_models::HAND_GRIP;
    let place = |p: [f32; 3]| data.in_hand(&hand, p);
    let base = TABLES.materials.len();
    for t in m.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| place(m.positions[i as usize]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        let n = n.map(|v| v / l);
        let colour = m.colors[t[0] as usize];
        let mat = base
            + prop_colours()
                .iter()
                .position(|o| same_colour(o, &colour))
                .unwrap_or(0);
        let first = out.positions.len() as u32;
        for p in [a, b, c] {
            out.positions.push(p);
            out.normals.push(n);
            out.uvs.push([0.0, 0.0]);
            out.bones.push(bone as u8);
            out.materials.push(mat as u16);
            out.faces.push(0);
        }
        out.indices.extend([first, first + 1, first + 2]);
    }
    out.parts.push("model");
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
    place_frame(d.feet, d.yaw, d.face, eye)
}

/// A held item on a skinned body (CHARACTERS.md §11.5): what is needed to pose it once the skinned
/// hand's frame is known.
struct HeldOnSkin {
    held: Held,
    /// The rigid rig's bones (model space) with the item's turns.
    posed: [Affine; MAX_BONES],
    /// The body's frame (relative to the eye).
    frame: Affine,
    seed: u32,
    light: [f32; 2],
    far: bool,
    passes: u8,
    casts: bool,
    cast_only: bool,
}

/// The rigid rig's bones (relative to the eye) for an item held in a skinned hand: the body's own
/// rigid pose, with the hand bone (`hand_bone`, whose frame the items are authored in) on the
/// skinned hand (`hand`, [`super::skinned::hand_frame`]), which the retarget already turned by the
/// item's own turns (an axe's tilt, data `pose`).
fn on_skinned_hand(h: &HeldOnSkin, hand_bone: Option<usize>, hand: &Affine) -> [Affine; MAX_BONES] {
    let mut bones = h.posed.map(|b| h.frame.then(&b));
    if let Some(b) = hand_bone.filter(|&b| b < MAX_BONES) {
        // The skinned body is posed from the pose with the item's turns on, so its hand already
        // carries them: none on top (they were applied twice before 0.54, the axe 50° off).
        bones[b] = *hand;
    }
    bones
}

/// A creature's skinned draw and what its hand shows (docs/CHARACTERS.md §11.11): an NPC in its
/// colours (its kind's ramps for its variant by part, [`dwell_models::part_remap`]), its fist closed
/// round its held item (its kind's `held`, in the item's tool's level hold); an animal as modelled,
/// holding nothing.
pub fn creature_draw(
    character: &Character,
    c: &crate::creatures::CreatureView,
    light: [f32; 2],
    now_s: f64,
) -> (super::skinned::SkinnedDraw, Held) {
    let npc = c.npc.as_ref();
    let held = crate::bodies::with_tool(Held::Empty, npc.and_then(|n| n.held).map(|h| (h, false)));
    // An NPC is posed as a player is (§11.9): the rig's clip for its role, its held item's hold
    // on top, so the sword sits in the hand as in a player's; each NPC at its own phase.
    let rig_pose = npc.map(|_| {
        let item = item_shown(character, held).and_then(|n| character.item(n));
        // A defeated NPC's kneel and stand play once from their start (§11.13).
        let t = if dwell_characters::animator::is_defeat_role(c.role) {
            c.pose_s
        } else {
            now_s + f64::from(c.id % 97) * 0.37
        };
        match npc.and_then(|n| n.guard).and_then(|g| character.clip(g)) {
            // A raised block over its pose (WORK_QUEUE item 35), as a player's is, the item's
            // turns on top.
            Some(g) => {
                let mut pose = npc_rig_pose(character, c.role, t, None);
                let raised = dwell_characters::anim::sample(TABLES.clip(g), 0.0);
                dwell_characters::anim::layer(&mut pose, &raised, 1.0);
                with_item(&pose, item)
            }
            None => npc_rig_pose(character, c.role, t, item),
        }
    });
    let draw = super::skinned::SkinnedDraw {
        model: c.model,
        seed: c.id,
        feet: c.feet,
        yaw: c.yaw,
        face: c.face,
        role: c.role,
        light,
        cast_only: false,
        rig_pose,
        grip: item_shown(character, held),
        remap: npc.map_or(super::skinned::NO_REMAP, |n| {
            let parts = dwell_models::find(dwell_models::MODELS, c.model).and_then(|m| m.parts);
            super::skinned::remap_floats(&dwell_models::part_remap(parts.as_ref(), &n.colours))
        }),
        idle_pose: c.pose,
    };
    (draw, held)
}

/// An NPC's rig pose at `t_s` seconds: the characters rig's clip for `role` (its `idle` for a
/// role the rig has no clip for: an NPC's `look` and `sit`), looped (a one-shot held at its end),
/// with `item`'s turns on.
pub fn npc_rig_pose(
    character: &Character,
    role: &str,
    t_s: f64,
    item: Option<&'static dwell_characters::Item>,
) -> Pose {
    let pose = character
        .clip(role)
        .or_else(|| character.clip("idle"))
        .map(|i| {
            let clip = TABLES.clip(i);
            // A one-shot (a defeated NPC's kneel) holds its end; a loop wraps.
            let t = if !clip.looping {
                t_s
            } else if clip.duration > 0.0 {
                t_s.rem_euclid(f64::from(clip.duration))
            } else {
                0.0
            };
            dwell_characters::anim::sample(clip, t as f32)
        })
        .unwrap_or_default();
    with_item(&pose, item)
}

/// [`body_frame`] from its parts (the skinned bodies' too, render/skinned.rs).
pub fn place_frame(feet: V3, yaw: f64, face: FaceFrame, eye: V3) -> Affine {
    let (s, c) = (yaw.to_radians().sin() as f32, yaw.to_radians().cos() as f32);
    // The face's up (FaceFrame): on face B the body's up and left both turn over.
    let up = face.up();
    let f = up[1] as f32;
    // Upright always: humans never die, a defeated one kneels by its clip (CHARACTERS.md §11.13);
    // before 0.61 a dead body was laid on its back here.
    Affine {
        m: [[f * c, 0.0, -f * s], [0.0, f, 0.0], [s, 0.0, c]],
        t: std::array::from_fn(|k| (feet[k] - eye[k]) as f32),
    }
}

/// The camera's near plane the own body is kept clear of (m: the scene's, `super::NEAR_PLANE`).
pub fn own_discard_m(body: &FirstPersonBody) -> f32 {
    super::NEAR_PLANE as f32 + body.near
}

/// Your own body's frame in first person (CHARACTERS.md §5.1): its `body_frame` moved back along its
/// own −z by `own::shift`, from the eye's place and pitch in the body's frame (the eye is the origin
/// of eye-relative space). `bones`: the pose's bone transforms in the body's frame; `forward`: the
/// camera's look. In the body's frame, so the same on both faces.
pub fn own_frame(
    d: &BodyDraw,
    eye: V3,
    forward: V3,
    body: &FirstPersonBody,
    mesh: &Mesh,
    bones: &[Affine; MAX_BONES],
) -> Affine {
    let frame = body_frame(d, eye);
    let (eye_local, pitch) = eye_in(&frame, forward);
    let s = own::shift(body, mesh, bones, eye_local, pitch);
    frame.then(&Affine::translation([0.0, 0.0, -s]))
}

/// The eye (the origin of eye-relative space) in a body's `frame`, and the pitch (degrees, up
/// positive) of the look `forward` there: what your own body is kept clear of (`own::shift`).
pub fn eye_in(frame: &Affine, forward: V3) -> ([f32; 3], f32) {
    // The frame is a rotation and a translation: its inverse is the transpose.
    let back = |v: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|k| (0..3).map(|j| frame.m[k][j] * v[j]).sum())
    };
    let eye_local = back(frame.t.map(|t| -t));
    let look = back(forward.map(|c| c as f32));
    (eye_local, look[1].clamp(-1.0, 1.0).asin().to_degrees())
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
    /// Its pipelines and its skinned models', for the renderer's warm-up (pipeline.rs).
    pub(crate) fn pipelines(&self) -> Vec<&crate::render::pipeline::Lazy> {
        let mut v = vec![
            &self.pipeline,
            &self.outline_pipeline,
            &self.shadow_pipeline,
        ];
        v.extend(self.skinned.pipelines());
        v
    }

    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
        shadow_frame_layout: &wgpu::BindGroupLayout,
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
        let make = |vs: &'static str, fs: &'static str, cull: wgpu::Face| {
            let (device, module, pipeline_layout, targets) = (
                device.clone(),
                module.clone(),
                pipeline_layout.clone(),
                targets.clone(),
            );
            crate::render::pipeline::Lazy::new(move || {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: mesh::VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })];
                crate::render::pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
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
                    },
                )
            })
        };
        let pipeline = make("character_vs", "character_fs", wgpu::Face::Back);
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("character shadows"),
            bind_group_layouts: &[Some(shadow_frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = {
            let (device, module, shadow_layout) =
                (device.clone(), module.clone(), shadow_layout.clone());
            crate::render::pipeline::Lazy::new(move || {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: mesh::VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })];
                crate::render::pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
                        label: Some("character shadows"),
                        layout: Some(&shadow_layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("character_vs"),
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
        let skinned = super::skinned::SkinnedRenderer::new(
            device,
            queue,
            frame_layout,
            shadow_frame_layout,
            samples,
        );
        Self {
            skinned,
            creatures: Vec::new(),
            character,
            look: dwell_characters::DEFAULT_LOOK,
            pipeline,
            outline_pipeline,
            shadow_pipeline,
            layout,
            materials,
            tiles,
            sampler,
            instances,
            capacity,
            group,
            meshes: HashMap::default(),
            own_meshes: HashMap::default(),
            draws: Vec::new(),
            arms: None,
            held_lights: Vec::new(),
            others_lights: 0,
            arms_light: None,
            stats: CharacterStats::default(),
            rigid_arms: false,
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
        let body = self.character.first_person.and_then(|f| f.body);
        let m = match (key.model, body) {
            (Some(model), _) => prop_mesh(self.character, key.name, model),
            (None, Some(b)) if key.own => own::build(&kept, &b),
            _ => mesh::build(&kept, key.view, chain),
        };
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
    /// depth; the shadow maps those within `cast_m`) and the arms. Builds what meshes are new and
    /// writes every instance's uniforms.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: V3,
        basis: &Basis,
        passes: &[(f64, f64)],
        cast_m: f64,
        bodies: &[BodyDraw],
        arms: Option<&ArmsDraw>,
    ) {
        self.draws.clear();
        self.arms = None;
        self.held_lights.clear();
        self.arms_light = None;
        let character = self.character;
        let item_name = |held: Held| item_shown(character, held);
        let light_at = |held: Held, bones: &[Affine; MAX_BONES], seed: u32| {
            held_light(character, held, bones, seed)
        };
        // Your own light once: the arms' if they are drawn, else your body's.
        let mut own_light = None;
        let mut data: Vec<f32> = Vec::new();
        let own_body = self.character.first_person.and_then(|f| f.body);
        let mut push = |bones: &[Affine; MAX_BONES],
                        held: Held,
                        outline: bool,
                        is_arms: bool,
                        discard: f32,
                        light: [f32; 2]| {
            let at = data.len();
            data.resize(at + INSTANCE_FLOATS, 0.0);
            for (k, b) in bones.iter().enumerate() {
                data[at + k * 12..at + k * 12 + 12].copy_from_slice(&rows(b));
            }
            let u = held.uniforms();
            for (k, v) in u.iter().enumerate() {
                data[at + BONE_FLOATS + k * 4..at + BONE_FLOATS + k * 4 + 4].copy_from_slice(v);
            }
            let cfg = at + BONE_FLOATS + 16;
            data[cfg..cfg + 4].copy_from_slice(&[
                discard,
                if outline { OUTLINE_WIDTH_M } else { 0.0 },
                f32::from(u8::from(is_arms)),
                0.0,
            ]);
            let l = at + BONE_FLOATS + 20;
            data[l..l + 4].copy_from_slice(&[light[0], light[1], 0.0, 0.0]);
            (at / INSTANCE_FLOATS) as u32
        };
        let mut keys = Vec::new();
        let mut skinned = Vec::new();
        let mut skinned_own = None;
        let mut held_items = Vec::new();
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
                if !d.cast_only && dist + 3.0 >= *near && dist - 3.0 <= *far {
                    mask |= 1 << i;
                }
            }
            // Out of the view (or your own), a body still casts into it.
            let casts = dist - 3.0 <= cast_m;
            if mask == 0 && !casts {
                continue;
            }
            let look = if self.character.look(d.look).is_some() {
                d.look
            } else {
                self.look
            };
            // Your own body in first person: its own mesh, moved clear of the eye, no shadow (the
            // `cast_only` body casts it) and no held item (the arms view holds it).
            let own = match (d.first_person, own_body) {
                (false, _) => None,
                (true, Some(b)) => Some(b),
                (true, None) => continue,
            };
            let casts = casts && own.is_none();
            let far = dist > OUTLINE_M && own.is_none();
            // The held item's turns on top of the clips (a block carried on an upturned palm).
            let held_item = item_name(d.held).and_then(|n| self.character.item(n));
            let posed = bone_transforms(self.character, &with_item(&d.pose, held_item));
            // Your own body from the player model (§11.12): the skinned body less its head and
            // arms, posed as your shadow is, kept clear of the eye as the rigid one was; it holds
            // nothing (the arms view does). `?fparms=rigid` keeps the rigid body.
            if let Some(b) = own.filter(|_| !self.rigid_arms && self.skinned.has_own_body()) {
                let bones = posed.map(|p| body_frame(d, eye).then(&p));
                own_light = own_light.or_else(|| light_at(d.held, &bones, d.seed));
                skinned_own = Some(super::skinned::SkinnedOwn {
                    draw: super::skinned::SkinnedDraw {
                        model: dwell_models::PLAYER,
                        seed: u32::MAX - 1,
                        feet: d.feet,
                        yaw: d.yaw,
                        face: d.face,
                        role: d.role,
                        light: d.light,
                        cast_only: false,
                        rig_pose: Some(with_item(&d.pose, held_item)),
                        grip: None,
                        remap: super::skinned::NO_REMAP,
                        idle_pose: None,
                    },
                    forward: basis.forward(),
                    body: b,
                    passes: mask,
                });
                continue;
            }
            let frame = match own {
                Some(b) => {
                    let character = self.character;
                    let m = self.own_meshes.entry(look).or_insert_with(|| {
                        own::build(character.look(look).map_or(&[][..], |l| l.parts), &b)
                    });
                    own_frame(d, eye, basis.forward(), &b, m, &posed)
                }
                None => body_frame(d, eye),
            };
            let bones = posed.map(|b| frame.then(&b));
            // Another player, or your own shadow: the skinned body, its held item drawn in the
            // skinned hand after (CHARACTERS.md §11.5).
            if !d.first_person {
                skinned.push(super::skinned::SkinnedDraw {
                    model: dwell_models::PLAYER,
                    // Your shadow's clip player apart from any other player's.
                    seed: if d.cast_only { u32::MAX } else { d.seed },
                    feet: d.feet,
                    yaw: d.yaw,
                    face: d.face,
                    role: d.role,
                    light: d.light,
                    cast_only: d.cast_only,
                    // The same pose the rigid body and the arms play, its held item's turns on.
                    rig_pose: Some(with_item(&d.pose, held_item)),
                    grip: item_name(d.held),
                    remap: super::skinned::NO_REMAP,
                    idle_pose: None,
                });
                held_items.push(HeldOnSkin {
                    held: d.held,
                    posed,
                    frame,
                    seed: d.seed,
                    light: d.light,
                    far,
                    passes: mask,
                    casts,
                    cast_only: d.cast_only,
                });
                continue;
            }
            let discard = own.map_or(0.0, |b| own_discard_m(&b));
            let instance = push(&bones, d.held, !far, false, discard, d.light);
            let body = Some(MeshKey {
                name: look,
                item: false,
                view: View::Body,
                far,
                own: own.is_some(),
                model: None,
            });
            if d.first_person || d.cast_only {
                own_light = own_light.or_else(|| light_at(d.held, &bones, d.seed));
            } else if let Some(l) = light_at(d.held, &bones, d.seed) {
                self.held_lights.push(l);
            }
            let item = item_name(d.held)
                .filter(|_| own.is_none())
                .map(|name| MeshKey {
                    name,
                    item: true,
                    view: View::Body,
                    far,
                    own: false,
                    model: d.held.model(),
                });
            keys.extend(body);
            keys.extend(item);
            self.draws.push(Draw {
                instance,
                body,
                item,
                outline: outline_for(far),
                passes: mask,
                casts,
                cast_only: d.cast_only,
            });
        }
        // The creatures (ADR 0073): skinned bodies of their own models, after the players (whose
        // held items take the first hands). An NPC is drawn in its colours, its held item on its
        // hand after (docs/CHARACTERS.md §11.11).
        let mut held_at: Vec<usize> = (0..held_items.len()).collect();
        let now_s = crate::render_clock::now_ms() / 1000.0;
        for (c, light) in &self.creatures {
            let (draw, held) = creature_draw(self.character, c, *light, now_s);
            if held != Held::Empty {
                let dist = (0..3)
                    .map(|k| (c.feet[k] - eye[k]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let mut mask = 0u8;
                for (i, (near, far)) in passes.iter().enumerate() {
                    if dist + 3.0 >= *near && dist - 3.0 <= *far {
                        mask |= 1 << i;
                    }
                }
                let held_item = item_name(held).and_then(|n| self.character.item(n));
                let pose = draw
                    .rig_pose
                    .unwrap_or_else(|| with_item(&Pose::default(), held_item));
                held_at.push(skinned.len());
                held_items.push(HeldOnSkin {
                    held,
                    posed: bone_transforms(self.character, &pose),
                    frame: place_frame(c.feet, c.yaw, c.face, eye),
                    seed: c.id,
                    light: *light,
                    far: dist > OUTLINE_M,
                    passes: mask,
                    casts: dist - 3.0 <= cast_m,
                    cast_only: false,
                });
            }
            skinned.push(draw);
        }
        // Your arms from the player model (§11.10): the first-person clips' pose retargeted onto its
        // arm, the held item on its hand after.
        let arms_frame_now = arms.map(|_| arms_frame(basis));
        let skinned_arms = match (arms, arms_frame_now) {
            (Some(a), Some(frame)) if !self.rigid_arms => {
                let held_item =
                    item_shown(self.character, a.held).and_then(|n| self.character.item(n));
                Some(super::skinned::SkinnedArms {
                    frame,
                    pose: with_item(&a.pose, held_item),
                    grip: item_shown(self.character, a.held),
                    light: a.light,
                })
            }
            _ => None,
        };
        let (hands, arms_hand) = self.skinned.prepare(
            device,
            queue,
            eye,
            passes,
            cast_m,
            &skinned,
            skinned_own.as_ref(),
            skinned_arms.as_ref(),
            crate::render_clock::now_ms() / 1000.0,
        );
        let hand_bone = self
            .skinned
            .hand()
            .and_then(|h| self.character.bone(h.bone));
        for (h, &at) in held_items.iter().zip(&held_at) {
            let Some(hand) = hands.get(at).copied().flatten() else {
                continue;
            };
            let bones = on_skinned_hand(h, hand_bone, &hand);
            if h.cast_only {
                own_light = own_light.or_else(|| light_at(h.held, &bones, h.seed));
            } else if let Some(l) = light_at(h.held, &bones, h.seed) {
                self.held_lights.push(l);
            }
            let Some(name) = item_name(h.held) else {
                continue;
            };
            let item = MeshKey {
                name,
                item: true,
                view: View::Body,
                far: h.far,
                own: false,
                model: h.held.model(),
            };
            let instance = push(&bones, h.held, !h.far, false, 0.0, h.light);
            keys.push(item);
            self.draws.push(Draw {
                instance,
                body: None,
                item: Some(item),
                outline: outline_for(h.far),
                passes: h.passes,
                casts: h.casts,
                cast_only: h.cast_only,
            });
        }
        if let (Some(a), Some(frame)) = (arms, arms_frame_now) {
            let held_item = item_shown(self.character, a.held).and_then(|n| self.character.item(n));
            let posed = arm_transforms(self.character, &with_item(&a.pose, held_item));
            let skinned_arms = self.skinned.has_arms();
            // From the player model: the held item on its hand (as on another player's body).
            let bones = match arms_hand.filter(|_| skinned_arms) {
                Some(hand) => on_skinned_hand(
                    &HeldOnSkin {
                        held: a.held,
                        posed,
                        frame,
                        seed: 0,
                        light: a.light,
                        far: false,
                        passes: 0,
                        casts: false,
                        cast_only: false,
                    },
                    hand_bone,
                    &hand,
                ),
                None => posed.map(|b| frame.then(&b)),
            };
            let instance = push(&bones, a.held, true, true, 0.0, a.light);
            if let Some(l) = light_at(a.held, &bones, 0) {
                own_light = Some(l);
                self.arms_light = Some(l);
            }
            let body = MeshKey {
                name: self.look,
                item: false,
                view: View::Arms,
                far: false,
                own: false,
                model: None,
            };
            let item = item_name(a.held).map(|name| MeshKey {
                name,
                item: true,
                view: View::Arms,
                far: false,
                own: false,
                model: a.held.model(),
            });
            if !skinned_arms {
                keys.push(body);
            }
            keys.extend(item);
            self.arms = Some(Draw {
                instance,
                // The rigid arms' parts only when the player model draws none.
                body: (!skinned_arms).then_some(body),
                item,
                outline: outline_for(false),
                passes: 0,
                casts: false,
                cast_only: false,
            });
        }
        self.others_lights = self.held_lights.len();
        self.held_lights.extend(own_light);
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
        self.stats.bodies = (self.draws.len() + self.skinned.bodies()) as u32;
        self.stats.arms = self.arms.is_some();
        self.stats.upload_bytes = data.len() as u64 * 4;
    }

    /// The lights held this frame (`prepare`): docs/LIGHTS.md §3.
    pub fn held_lights(&self) -> &[HeldLight] {
        &self.held_lights
    }

    /// The held lights whose items are drawn this frame, where flames burn (docs/LIGHTS.md §6):
    /// other players' (in the scene passes) and your arms' (in the arms pass).
    pub fn held_flames(&self) -> (&[HeldLight], Option<HeldLight>) {
        (&self.held_lights[..self.others_lights], self.arms_light)
    }

    /// Whether the arms are drawn this frame (the renderer adds their pass).
    /// The Model detail setting (ADR 0070 §10): the skinned bodies' level.
    pub fn set_model_detail(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, level: usize) {
        self.skinned.set_level(device, queue, level);
    }

    pub fn has_arms(&self) -> bool {
        self.arms.is_some()
    }

    fn record(&self, pass: &mut wgpu::RenderPass<'_>, d: &Draw, calls: &mut u32, tris: &mut u64) {
        let offset = d.instance * INSTANCE_BYTES as u32;
        pass.set_bind_group(1, &self.group, &[offset]);
        let meshes: Vec<&GpuMesh> = [d.body, d.item]
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
        self.skinned.draw_bodies(pass, i, calls, tris) || any
    }

    /// Draws the bodies within the shadow distance into a shadow map's pass (group 0 bound to its
    /// cascade's frame; group 1 left bound to the characters' group). Your own body (`cast_only`)
    /// casts into the nearest cascade only (`nearest`): it is drawn every frame, while a far
    /// cascade's map is kept for many and would keep your shadow where you were.
    pub fn draw_shadows(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        nearest: bool,
        calls: &mut u32,
        tris: &mut u64,
    ) {
        for d in self
            .draws
            .iter()
            .filter(|d| d.casts && (nearest || !d.cast_only))
        {
            pass.set_bind_group(1, &self.group, &[d.instance * INSTANCE_BYTES as u32]);
            pass.set_pipeline(&self.shadow_pipeline);
            for m in [d.body, d.item]
                .into_iter()
                .flatten()
                .filter_map(|k| self.meshes.get(&k))
                .filter(|m| m.count > 0)
            {
                pass.set_vertex_buffer(0, m.vertex.slice(..));
                pass.set_index_buffer(m.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
                *calls += 1;
                *tris += u64::from(m.count / 3);
            }
        }
        self.skinned.draw_shadows(pass, nearest, calls, tris);
    }

    /// Draws the arms (group 0 bound to the arms' frame, over a cleared depth buffer).
    pub fn draw_arms(&self, pass: &mut wgpu::RenderPass<'_>, calls: &mut u32, tris: &mut u64) {
        self.skinned.draw_arms(pass, calls, tris);
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
        depth: wgpu::LoadOp::Clear(DEPTH_CLEAR),
    }
}

/// The characters' depth test (bodies in the scene passes, the arms in theirs): the scene's
/// (reversed: greater-or-equal, `frame::DEPTH_COMPARE`) with writes; the arms pass starts from a
/// cleared buffer (`arms_pass`).
pub fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(true),
        depth_compare: Some(DEPTH_COMPARE),
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
    #[test]
    fn outlines_are_off_for_now_near_and_far() {
        assert!(
            !super::outline_for(false),
            "near characters and the arms draw no outline"
        );
        assert!(!super::outline_for(true), "far characters draw no outline");
    }

    use super::*;

    // An item in a skinned hand: the rigid rig's hand bone is moved onto the skinned hand's frame,
    // turned by the item's own turns of that bone (the axe's tilt); the other bones keep the
    // body's frame. The torch's light rides the skinned hand too.
    dwell_world::both_faces! {
        fn a_held_item_is_posed_on_the_skinned_hand(face: FaceFrame) {
            let ch = dwell_characters::player();
            let b = ch.bone("hand_r").unwrap();
            let pose = Pose::default();
            let frame = place_frame(face.point([1.0, 2.0, 3.0]), 45.0, face, [0.0; 3]);
            let hand = Affine::translation([5.0, 6.0, 7.0]).then(&Affine::euler([10.0, 20.0, 30.0]));
            let axe = ch.item("stone_axe").expect("the axe is data");
            let held = |h: Held, item: Option<&dwell_characters::Item>| HeldOnSkin {
                held: h,
                posed: bone_transforms(ch, &with_item(&pose, item)),
                frame,
                seed: 3,
                light: [1.0, 0.0],
                far: false,
                passes: 1,
                casts: true,
                cast_only: false,
            };
            // No turns of its own: exactly the skinned hand's frame.
            let bones = on_skinned_hand(&held(Held::Block(0), None), Some(b), &hand);
            for k in 0..3 {
                assert!((bones[b].t[k] - hand.t[k]).abs() < 1e-5);
                for r in 0..3 {
                    assert!((bones[b].m[k][r] - hand.m[k][r]).abs() < 1e-5);
                }
            }
            // The axe: the skinned hand already carries its tilt (the body is retargeted from
            // the pose with it on), so exactly the skinned hand again.
            let h = held(Held::Tool("stone_axe"), Some(axe));
            let bones = on_skinned_hand(&h, Some(b), &hand);
            assert!((0..3).all(|k| (bones[b].t[k] - hand.t[k]).abs() < 1e-5));
            assert!((0..3).all(|k| (bones[b].m[k][0] - hand.m[k][0]).abs() < 1e-5));
            // Another bone keeps the body's frame.
            let head = ch.bone("head").unwrap();
            let body = frame.then(&h.posed[head]);
            assert!((0..3).all(|k| (bones[head].t[k] - body.t[k]).abs() < 1e-5));
            // A torch's light sits on the skinned hand.
            let torch = Held::Block(dwell_blocks::require("dwell:torch"));
            let h = held(torch, None);
            let bones = on_skinned_hand(&h, Some(b), &hand);
            let l = held_light(ch, torch, &bones, 3).expect("a torch gives light"); {
                let (bone, at) = ch.item("torch").unwrap().light.unwrap();
                assert_eq!(bone, b);
                let p = hand.apply(at);
                assert!((0..3).all(|k| (l.rel[k] - p[k]).abs() < 1e-4), "{:?} {p:?}", l.rel);
            }
        }
    }

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
            assert_eq!(a.depth, wgpu::LoadOp::Clear(DEPTH_CLEAR));
            assert!(!a.ops.clear, "loads the world's colour and view depth");
            assert_eq!(a.ops.resolve, samples > 1);
            let world_last = scene_pass_ops(1, 3, samples);
            assert!(!world_last.resolve && world_last.store_colour);
        }
        assert_eq!(
            depth_state().depth_compare,
            Some(wgpu::CompareFunction::GreaterEqual)
        );
    }

    dwell_world::both_faces! {
        fn another_players_item_is_turned_once_on_the_skinned_hand(face: FaceFrame) {
            // The skinned body is retargeted from the pose with the item's turns on (the axe's
            // tilt of the hand and forearm), so its hand already carries them: the item drawn on it
            // is turned as on the rigid body, not twice (the Peasant's hand.R and the rig's hand
            // bone agree within 10°, the_retargeted_hand_follows_the_rigs_hand_in_the_world).
            use crate::render::skinned::{SkinnedDraw, body_matrix, hand_frame, model_pose};
            let ch = dwell_characters::player();
            let (data, model) = dwell_models::player();
            let b = ch.bone("hand_r").unwrap();
            let feet = face.point([3.0, 6.0, 1.0]);
            for (name, held) in [
                ("stone_axe", Held::Tool("stone_axe")),
                ("tool", Held::Tool("stone_pickaxe")),
            ] {
                let item = ch.item(name).expect("an item");
                for clip in ["biped_idle", "biped_walk"] {
                    let c = (0..)
                        .map(|i| dwell_characters::TABLES.clip(i))
                        .find(|c| c.name == clip)
                        .unwrap();
                    let pose = dwell_characters::anim::sample(c, c.duration * 0.3);
                    let posed = with_item(&pose, Some(item));
                    let d = SkinnedDraw {
                        model: dwell_models::PLAYER,
                        seed: 1,
                        feet,
                        yaw: 30.0,
                        face,
                        role: "idle",
                        light: [1.0, 0.0],
                        cast_only: false,
                        rig_pose: Some(posed),
                        grip: Some(name),
                        remap: crate::render::skinned::NO_REMAP,
                        idle_pose: None,
                    };
                    let place = body_matrix(&d, data, [0.0; 3]);
                    let p = dwell_models::anim::Player::new(data, model, "idle");
                    let g = dwell_models::anim::globals(
                        model,
                        &model_pose(data, model, &p, d.rig_pose.as_ref(), d.grip),
                    );
                    let hand = hand_frame(data, model, &g, &place).unwrap();
                    let frame = place_frame(feet, 30.0, face, [0.0; 3]);
                    let h = HeldOnSkin {
                        held,
                        posed: bone_transforms(ch, &posed),
                        frame,
                        seed: 1,
                        light: [1.0, 0.0],
                        far: false,
                        passes: 1,
                        casts: true,
                        cast_only: false,
                    };
                    let drawn = on_skinned_hand(&h, Some(b), &hand)[b];
                    let rigid = frame.then(&h.posed[b]);
                    let turn = (0..3)
                        .map(|c| {
                            let dot: f32 = (0..3).map(|r| drawn.m[c][r] * rigid.m[c][r]).sum();
                            dot.clamp(-1.0, 1.0).acos().to_degrees()
                        })
                        .fold(0.0, f32::max);
                    assert!(turn < 12.0, "{name} in {clip}: the item is {turn:.1}° off the rig's hand");
                }
            }
        }
    }

    dwell_world::both_faces! {
        fn an_npc_is_drawn_in_its_variants_colours_with_its_sword_in_its_fist(face: FaceFrame) {
            // WORK_QUEUE items 35 and 38: a villager's colours are its kind's ramps for its
            // variant seed (the same seed, the same remap rows), its held sword is the `sword`
            // prop in the sword's level hold, the fist closed on it, on the Peasant's hand.R.
            use crate::creatures::{CreatureView, NpcLook};
            use crate::render::skinned::{NO_REMAP, body_matrix, hand_frame, model_pose};
            let ch = dwell_characters::player();
            let npc = |variant: u32| NpcLook::of(&dwell_protocol::NpcNet {
                id: 7,
                kind: "dwell:villager".into(),
                pose: 0,
                variant,
            });
            let view = |npc: Option<NpcLook>| CreatureView {
                id: 7,
                model: "peasant",
                feet: face.point([3.0, 6.0, 1.0]),
                yaw: 30.0,
                face,
                role: "idle",
                npc,
                pose: None,
                pose_s: 0.0,
            };
            let (a, held) = creature_draw(ch, &view(Some(npc(99))), [1.0, 0.0], 0.0);
            assert_eq!(held, Held::Model { hold: "sword", model: "sword" });
            assert_eq!(a.grip, Some("sword"));
            assert_eq!(ch.item("sword").map(|i| i.name), Some("sword"));
            // Deterministic: the same variant draws the same rows, every time.
            assert_eq!(creature_draw(ch, &view(Some(npc(99))), [1.0, 0.0], 0.0).0.remap, a.remap);
            assert_ne!(a.remap, NO_REMAP, "variant 99 changes a colour");
            // The rows are the kind's colours for that variant: each pair the ramps' bases.
            let parts = dwell_models::find(dwell_models::MODELS, "peasant").unwrap().parts;
            let pairs = dwell_models::part_remap(parts.as_ref(), &npc(99).colours);
            assert_eq!(a.remap, crate::render::skinned::remap_floats(&pairs));
            // Some other variant draws differently (the seed picks per part).
            assert!((0..32).any(|v| creature_draw(ch, &view(Some(npc(v))), [1.0, 0.0], 0.0).0.remap != a.remap));
            // An animal (or an NPC from an older server): as modelled, empty-handed.
            let (plain, none) = creature_draw(ch, &view(None), [1.0, 0.0], 0.0);
            assert_eq!((plain.remap, none, plain.grip), (NO_REMAP, Held::Empty, None));
            // The sword sits on the Peasant's hand: its hand frame is at the body's side, at
            // hand height, where the held item is drawn.
            let (data, model) = dwell_models::model("peasant").unwrap();
            let place = body_matrix(&a, data, [0.0; 3]);
            let p = dwell_models::anim::Player::new(data, model, "idle");
            assert!(a.rig_pose.is_some(), "an NPC is posed from the rig's clips");
            assert!(plain.rig_pose.is_none(), "an animal plays its own clips");
            let g = dwell_models::anim::globals(
                model,
                &model_pose(data, model, &p, a.rig_pose.as_ref(), a.grip),
            );
            let hand = hand_frame(data, model, &g, &place).expect("the villager's hand");
            let rel: [f64; 3] = std::array::from_fn(|k| f64::from(hand.t[k]) - a.feet[k]);
            let up = face.y(rel[1]) - face.y(0.0);
            assert!((0.5..1.3).contains(&up.abs()), "the hand is {up:.2} m up the body");
            assert!(rel[0].hypot(rel[2]) < 0.6, "the hand is by the body: {rel:?}");
        }
    }

    fn torch() -> Held {
        Held::Block(dwell_blocks::require("dwell:torch"))
    }

    /// The mesh a hand holding `held` shows (the item's parts).
    fn held_mesh(held: Held) -> dwell_characters::mesh::Mesh {
        let ch = dwell_characters::player();
        let item = ch
            .item(item_shown(ch, held).expect("an item"))
            .expect("in the data");
        dwell_characters::mesh::build(item.parts, View::Both, &[])
    }

    #[test]
    fn a_cross_plant_is_held_as_its_crossed_sheets_and_a_ladder_as_one_sheet_no_box() {
        // face-a-only: the hand's item mesh, in the hand's frame (the arms view is face-local).
        // Owner, 2026-11-09: "Transparent blocks like ladders and tall grass shouldn't have side
        // renders" — they were a thin box (12 triangles, 6 sides). By their look's data: a cross
        // plant two sheets, a ladder one; each sheet 4 triangles, both sides, no edge faces.
        for (block, sheets) in [("dwell:tall_grass", 2), ("dwell:ladder", 1)] {
            let m = held_mesh(Held::Block(dwell_blocks::require(block)));
            assert_eq!(m.indices.len() / 3, sheets * 4, "{block}");
            // Every vertex on its sheet's two faces: the part's sides +z and −z only.
            assert!(m.faces.iter().all(|&f| f == 4 || f == 5), "{block}");
        }
    }

    #[test]
    fn every_tool_is_held_as_its_model_its_handle_through_the_fist() {
        // face-a-only: the hand's item mesh, in the grip bone's frame (the arms view is face-local).
        let ch = dwell_characters::player();
        let (grip, _) = ch.item(MODEL_HOLD).expect("the tools' hold").socket;
        let base = TABLES.materials.len();
        for t in dwell_protocol::chopping::BREAK_TOOLS {
            let held = held_item(t.item, None);
            let Held::Model { hold, model } = held else {
                panic!("{} is held as {held:?}, not its model", t.item);
            };
            let (data, _) = dwell_models::prop(model).expect("its prop is built in");
            let m = prop_mesh(ch, hold, model);
            assert!(
                m.indices.len() >= 3 * 50,
                "{}: {} triangles",
                t.item,
                m.indices.len() / 3
            );
            assert!(
                m.bones.iter().all(|&b| usize::from(b) == grip),
                "{}",
                t.item
            );
            assert!(
                m.materials.iter().all(
                    |&k| usize::from(k) >= base && usize::from(k) < base + prop_colours().len()
                ),
                "{}: its colours are the props' material rows",
                t.item
            );
            // Its handle through the closed fist along the hand's grip axis (owner, 2026-11-11:
            // "properly align them to the hand"), measured on the mesh as drawn: the centre of the
            // surface's bounds in a slab across the axis at the fist is the fist's centre, and the
            // line through two such slabs along the handle is the grip axis.
            let hand = dwell_models::HAND_GRIP;
            let centre = |from: f32, to: f32| -> [f32; 3] {
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                for tri in m.indices.chunks_exact(3) {
                    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                        let (u, w) = (m.positions[tri[i] as usize], m.positions[tri[j] as usize]);
                        for k in 0..=200 {
                            let f = k as f32 / 200.0;
                            let q = [0, 1, 2].map(|c| u[c] + (w[c] - u[c]) * f);
                            let s = (0..3)
                                .map(|c| (q[c] - hand.at[c]) * hand.axis[c])
                                .sum::<f32>();
                            if (from..=to).contains(&s) {
                                for c in 0..3 {
                                    lo[c] = lo[c].min(q[c]);
                                    hi[c] = hi[c].max(q[c]);
                                }
                            }
                        }
                    }
                }
                assert!(
                    lo[0] < f32::MAX,
                    "{}: nothing at {from}..{to} m along the grip",
                    t.item
                );
                [0, 1, 2].map(|c| (lo[c] + hi[c]) / 2.0)
            };
            let off = |p: [f32; 3]| {
                let d = [0, 1, 2].map(|c| p[c] - hand.at[c]);
                let s = (0..3).map(|c| d[c] * hand.axis[c]).sum::<f32>();
                (0..3)
                    .map(|c| (d[c] - s * hand.axis[c]).powi(2))
                    .sum::<f32>()
                    .sqrt()
            };
            let fist = centre(-0.02, 0.02);
            assert!(
                off(fist) < 0.004,
                "{}: the handle is {:.3} m off the fist",
                t.item,
                off(fist)
            );
            let (p0, p1) = (centre(-0.07, -0.05), centre(0.05, 0.07));
            let d = [0, 1, 2].map(|c| p1[c] - p0[c]);
            let l = (0..3).map(|c| d[c] * d[c]).sum::<f32>().sqrt();
            let cos = (0..3).map(|c| d[c] * hand.axis[c]).sum::<f32>() / l;
            assert!(
                cos > 2f32.to_radians().cos(),
                "{}: the handle is {:.1} degrees off the grip axis",
                t.item,
                cos.acos().to_degrees()
            );
            // The head ahead along the grip axis: most of the tool beyond the fist.
            let along = |p: &[f32; 3]| {
                (0..3)
                    .map(|c| (p[c] - hand.at[c]) * hand.axis[c])
                    .sum::<f32>()
            };
            let (lo, hi) = m
                .positions
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    (lo.min(along(p)), hi.max(along(p)))
                });
            // A shovel is held mid-shaft: its top up to 0.45 of its length behind the fist.
            let length = data.length * data.hold.scale;
            assert!(
                hi > 0.4 * length && lo > -0.45 * length,
                "{}: {lo}..{hi}",
                t.item
            );
        }
        // The axe keeps its swing's holds, drawn as its model.
        assert_eq!(
            held_item("dwell:stone_axe", Some("stone_axe_vertical")),
            Held::Model {
                hold: "stone_axe_vertical",
                model: "stone_axes"
            }
        );
        // A look that is not a model, as before.
        assert_eq!(held_item("dwell:stick", None), Held::Look("dwell:stick"));
        // The held model's mesh is its own: the axe's hold mesh is not reused for it.
        assert_eq!(
            held_item("dwell:stone_axe", Some("stone_axe")).model(),
            Some("stone_axes")
        );
    }

    /// The on-screen bounds (NDC, +x right, +y up) of item `item`'s held model in your arms posed
    /// by clip `clip` at `t` s, held in `hold` (its tool's, None for the default), at the arms'
    /// FOV and `aspect`; and the distance ahead of the eye of its nearest point in view.
    fn held_model_bounds(
        item: &'static str,
        hold: Option<&'static str>,
        clip: &str,
        t: f32,
        aspect: f64,
    ) -> ([f64; 4], f64) {
        let ch = dwell_characters::player();
        let held = held_item(item, hold);
        let Held::Model { hold, model } = held else {
            panic!("{item} is held as {held:?}");
        };
        let clip = TABLES
            .clips
            .iter()
            .find(|c| c.name == clip)
            .expect("the clip");
        let pose = with_item(&dwell_characters::anim::sample(clip, t), ch.item(hold));
        // View space (+x left, +y up, +z ahead): the arms' frame with the identity basis.
        let bones = arm_transforms(ch, &pose);
        let fov = ch.first_person.map_or(62.0, |f| f64::from(f.fov));
        let tan = (fov.to_radians() / 2.0).tan();
        let m = prop_mesh(ch, hold, model);
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        let mut near = f64::MAX;
        for (p, &bone) in m.positions.iter().zip(&m.bones) {
            let v = bones[usize::from(bone)].apply(*p).map(f64::from);
            let z = v[2].max(1e-3);
            let (x, y) = (-v[0] / (z * tan * aspect), v[1] / (z * tan));
            // Only what is in view counts as near (an axe raised over your head is not).
            if v[2] > 0.0 && x.abs() <= 1.0 && y.abs() <= 1.0 {
                near = near.min(v[2]);
            }
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        (b, near)
    }

    #[test]
    fn every_tool_is_held_low_right_clear_of_the_crosshair_and_the_eye() {
        // face-a-only: the arms view is drawn in view space, the same on both faces.
        // Owner, 2026-11-10: the tools filled the right third of the screen, bolt upright; 0.52.22
        // angled them in and halved them. Owner, 2026-11-11: "you made the tools too small in hand,
        // the original size … was much more accurate": full size (hold scale 1), still angled in.
        // So: at their modelled size (not shrunk), in the view's right-lower part, clear of the
        // crosshair, and never inside the arms view's near plane mid-swing.
        let mut bad = Vec::new();
        for t in dwell_protocol::chopping::BREAK_TOOLS {
            for aspect in [16.0 / 9.0, 2880.0 / 1324.0] {
                let ([x0, y0, x1, y1], _) = held_model_bounds(t.item, None, "fp_idle", 0.0, aspect);
                let area = (x1.min(1.0) - x0) * (y1.min(1.0) - y0.max(-1.0));
                eprintln!(
                    "{} {aspect:.2}: x {x0:.2}..{x1:.2} y {y0:.2}..{y1:.2} area {area:.2}",
                    t.item
                );
                if x0 < 0.05 {
                    bad.push(format!("{}: reaches the crosshair, x from {x0:.2}", t.item));
                }
                // 0.52.52: the handle in the fist and the lean from the hand's pose (the `tool`
                // hold); the head stays in view, not past the screen's right edge.
                if x1 > 0.97 {
                    bad.push(format!(
                        "{}: its head off the right edge, x to {x1:.2}",
                        t.item
                    ));
                }
                if y1 > 0.45 {
                    bad.push(format!("{}: up to y {y1:.2}, high in the view", t.item));
                }
                if area < 0.38 {
                    bad.push(format!(
                        "{}: only {area:.2} of the view's area 4: shrunk",
                        t.item
                    ));
                }
            }
            for (hold, clip) in [
                (None, "fp_break"),
                (Some("stone_axe"), "fp_chop_level"),
                (Some("stone_axe_vertical"), "fp_chop_vertical"),
            ] {
                let c = TABLES.clips.iter().find(|c| c.name == clip).expect(clip);
                let near = (0..=40)
                    .map(|i| {
                        let s = c.duration * i as f32 / 40.0;
                        held_model_bounds(t.item, hold, clip, s, 16.0 / 9.0).1
                    })
                    .fold(f64::MAX, f64::min);
                eprintln!("{} {clip}: nearest in view {near:.3} m", t.item);
                if near < ARMS_NEAR + 0.01 {
                    bad.push(format!("{} in {clip}: {near:.3} m from the eye", t.item));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn a_stick_and_a_rock_are_held_as_their_looks_shape_tile_and_tint() {
        // face-a-only: the hand's item and the shader's tiles, no world geometry.
        let ch = dwell_characters::player();
        for (item, shown) in [
            ("dwell:stick", "rod"),
            ("dwell:rock", "lump"),
            ("dwell:grass_seed", "cluster"),
        ] {
            let held = Held::Look(item);
            assert_eq!(item_shown(ch, held), Some(shown), "{item}");
            assert!(!held_mesh(held).indices.is_empty());
            let look = dwell_protocol::inventory::item(item)
                .and_then(|d| d.look)
                .expect(item);
            let u = held.uniforms();
            let side = dwell_blocks::try_info(dwell_blocks::require(look.texture))
                .and_then(|i| i.textures)
                .expect("a textured block")
                .side;
            assert!(
                (f64::from(u[1][0]) - dwell_mesh::tile_rect(side).u0).abs() < 1e-6,
                "{item}"
            );
            // Tinted its look's colour (linear), textured (w 1).
            let r = dwell_look::srgb_to_linear(f64::from(look.color[0]) / 255.0) as f32;
            assert!((u[3][0] - r).abs() < 1e-6 && u[3][3] == 1.0, "{item}");
        }
    }

    #[test]
    fn a_torch_is_held_as_the_torch_item_and_gives_off_its_light() {
        let ch = dwell_characters::player();
        assert_eq!(item_shown(ch, torch()), Some("torch"));
        assert_eq!(torch().light().map(|l| l.name), Some("dwell:torch"));
        let stone = Held::Block(dwell_blocks::require("dwell:stone"));
        assert_eq!(item_shown(ch, stone), Some("block"));
        assert_eq!(stone.light(), None);
        assert_eq!(
            Held::Brush(dwell_blocks::require("dwell:torch")).light(),
            None,
            "a brush gives off nothing"
        );
        // Your arms' torch, held in the arms' idle: in front of the eye, within a metre, the flame
        // above the hand.
        let basis = super::super::camera::aim(30.0, -10.0, FaceFrame::A, 0.0);
        let idle = TABLES
            .clips
            .iter()
            .find(|c| c.name == "fp_idle")
            .expect("fp_idle");
        let pose = dwell_characters::anim::sample(idle, 0.0);
        let bones = arm_transforms(ch, &pose).map(|b| arms_frame(&basis).then(&b));
        let hand = bones[ch.bone("hand_r").expect("hand_r")].t.map(f64::from);
        let l = held_light(ch, torch(), &bones, 0).expect("the arms hold the torch's light");
        let r = l.rel.map(f64::from);
        let len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        assert!(len < 1.0 && len > 0.2, "{r:?}");
        assert!(
            super::super::camera::dot(r, basis.forward()) > 0.1,
            "ahead: {r:?}"
        );
        let up = |v| super::super::camera::dot(v, basis.up);
        assert!(up(r) > up(hand), "over the hand: {r:?}");
        assert!(held_light(ch, stone, &bones, 0).is_none());
    }

    // A held torch's light sits in its holder's hand, the same face-local height on both faces.
    dwell_world::both_faces! {
        fn a_held_torch_lights_from_its_holders_hand(face: FaceFrame) {
            let ch = dwell_characters::player();
            let d = BodyDraw {
                feet: face.point([10.0, 5.0, 0.0]),
                yaw: 90.0,
                face,
                look: dwell_characters::DEFAULT_LOOK,
                pose: Pose::REST,
                held: torch(),
                cast_only: false,
                first_person: false,
                light: [1.0, 0.0],
                seed: 7,
                role: "idle",
            };
            let posed = bone_transforms(ch, &d.pose);
            let frame = body_frame(&d, [0.0; 3]);
            let l = held_light(ch, d.held, &posed.map(|b| frame.then(&b)), d.seed).expect("its light");
            assert_eq!(l.seed, 7);
            // Face-local height over its feet: in the hand's reach (0.5–1.8 m up), within a metre
            // across; mirrored on face B.
            let up = face.y(f64::from(l.rel[1])) - face.y(d.feet[1]);
            assert!((0.5..1.8).contains(&up), "{up} m up ({l:?})");
            let across = (f64::from(l.rel[0]) - 10.0).hypot(f64::from(l.rel[2]));
            assert!(across < 1.0, "{across}");
        }
    }

    // A body stands on its face (its head toward the face's up) and faces its yaw on both faces,
    // as a proper rotation (no mirror), so its winding stays front-facing.
    dwell_world::both_faces! {
        fn a_body_stands_on_its_face_and_faces_its_yaw(face: FaceFrame) {
            let d = BodyDraw {
                feet: face.point([10.0, 5.0, 0.0]),
                yaw: 90.0,
                face,
                look: dwell_characters::DEFAULT_LOOK,
                pose: Pose::REST,
                held: Held::Empty,
                cast_only: false,
                first_person: false,
                light: [1.0, 0.0],
                seed: 0,
                role: "idle",
            };
            let f = body_frame(&d, [0.0; 3]);
            // Yaw 90° faces +x on either face.
            let ahead = f.rotate([0.0, 0.0, 1.0]);
            assert!((ahead[0] - 1.0).abs() < 1e-5, "{ahead:?}");
            // The head is 1.8 m up the face's up.
            let head = f.apply([0.0, 1.8, 0.0])[1] as f64;
            assert!((head - (d.feet[1] + 1.8 * face.up()[1])).abs() < 1e-4, "{head}");
            let m = f.m;
            let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2])
                - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
                + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
            assert!((det - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn the_arms_sit_ahead_of_the_eye_whatever_the_view() {
        let fp = dwell_characters::player().first_person.unwrap();
        for (yaw, pitch, face) in [
            (0.0, 0.0, FaceFrame::A),
            (135.0, -40.0, FaceFrame::A),
            (20.0, 30.0, FaceFrame::B),
        ] {
            let b = crate::render::camera::aim(yaw, pitch, face, 0.0);
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

    /// Your own body in first person never clips the camera (CHARACTERS.md §5.1; the owner: "not
    /// clipping the camera into the player model"): over every body clip (and the swings layered
    /// on the idle and the walk), several phases of each, the pitch from straight down to straight
    /// up, every look, the eye at its height and lagging a step below it (the eye's smoothing), on
    /// both faces, no drawn triangle comes within the near plane plus `near` of the eye, and the
    /// body covers at most `fill` of a 16:9 screen at the base FOV and a wide one.
    mod own_body {
        use super::*;
        use dwell_characters::anim::{layer, sample};
        use dwell_characters::own;

        fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
            std::array::from_fn(|k| a[k] - b[k])
        }
        fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
            a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
        }

        /// The distance from the origin to triangle `abc` (Ericson's closest point on a triangle).
        fn distance(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
            let p = [0.0f32; 3];
            let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
            let at = |u: f32, v: f32| -> f32 {
                let q: [f32; 3] = std::array::from_fn(|k| a[k] + ab[k] * u + ac[k] * v);
                dot(q, q).sqrt()
            };
            let (d1, d2) = (dot(ab, ap), dot(ac, ap));
            if d1 <= 0.0 && d2 <= 0.0 {
                return at(0.0, 0.0);
            }
            let bp = sub(p, b);
            let (d3, d4) = (dot(ab, bp), dot(ac, bp));
            if d3 >= 0.0 && d4 <= d3 {
                return at(1.0, 0.0);
            }
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                return at(d1 / (d1 - d3), 0.0);
            }
            let cp = sub(p, c);
            let (d5, d6) = (dot(ab, cp), dot(ac, cp));
            if d6 >= 0.0 && d5 <= d6 {
                return at(0.0, 1.0);
            }
            let vb = d5 * d2 - d1 * d6;
            if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                return at(0.0, d2 / (d2 - d6));
            }
            let va = d3 * d6 - d5 * d4;
            if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
                let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
                return at(1.0 - w, w);
            }
            let denom = 1.0 / (va + vb + vc);
            at(vb * denom, vc * denom)
        }

        const COLS: usize = 64;
        const ROWS: usize = 36;
        const ASPECT: f32 = 16.0 / 9.0;

        /// Marks the cells of a COLS × ROWS screen whose centres a view-space triangle (x right,
        /// y up, z ahead) covers, clipped to the near plane.
        fn raster(tri: [[f32; 3]; 3], near: f32, f: f32, cells: &mut [bool]) {
            let mut poly: Vec<[f32; 3]> = Vec::with_capacity(4);
            for i in 0..3 {
                let (a, b) = (tri[i], tri[(i + 1) % 3]);
                if a[2] >= near {
                    poly.push(a);
                }
                if (a[2] >= near) != (b[2] >= near) {
                    let t = (near - a[2]) / (b[2] - a[2]);
                    poly.push(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t));
                }
            }
            if poly.len() < 3 {
                return;
            }
            let pts: Vec<[f32; 2]> = poly
                .iter()
                .map(|v| [v[0] / v[2] * f / ASPECT, v[1] / v[2] * f])
                .collect();
            for k in 1..pts.len() - 1 {
                let [a, b, c] = [pts[0], pts[k], pts[k + 1]];
                let edge = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
                    (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
                };
                let area = edge(a, b, c);
                if area.abs() < 1e-12 {
                    continue;
                }
                let lo = |k: usize| a[k].min(b[k]).min(c[k]).max(-1.0);
                let hi = |k: usize| a[k].max(b[k]).max(c[k]).min(1.0);
                if lo(0) > hi(0) || lo(1) > hi(1) {
                    continue;
                }
                let col =
                    |x: f32| ((x + 1.0) / 2.0 * COLS as f32 - 0.5).clamp(0.0, COLS as f32 - 1.0);
                let row =
                    |y: f32| ((y + 1.0) / 2.0 * ROWS as f32 - 0.5).clamp(0.0, ROWS as f32 - 1.0);
                for r in row(lo(1)).floor() as usize..=row(hi(1)).ceil() as usize {
                    for q in col(lo(0)).floor() as usize..=col(hi(0)).ceil() as usize {
                        let p = [
                            (q as f32 + 0.5) / COLS as f32 * 2.0 - 1.0,
                            (r as f32 + 0.5) / ROWS as f32 * 2.0 - 1.0,
                        ];
                        let (w0, w1, w2) = (edge(b, c, p), edge(c, a, p), edge(a, b, p));
                        let sg = area.signum();
                        if w0 * sg >= 0.0 && w1 * sg >= 0.0 && w2 * sg >= 0.0 {
                            cells[r * COLS + q] = true;
                        }
                    }
                }
            }
        }

        /// The poses swept: each base role's clip (but the defeat's, drawn only in third person)
        /// at six phases, and the swings on the idle and walk
        /// (named `role phase` and `role+swing phase`), and whether the eye is the crouch's.
        fn poses() -> Vec<(String, bool, Pose)> {
            let ch = dwell_characters::player();
            let crouched = ["crouch", "crouch_walk", "slide"];
            let mut out = Vec::new();
            for &(role, c) in ch.clips {
                let clip = TABLES.clip(c);
                if clip.layer != dwell_characters::Layer::Base {
                    continue;
                }
                // The defeat's kneel and stand (§11.13, 0.60.7) are never your own body in first
                // person: defeated, the view is third person over the body (`set_own_body(None)`).
                if dwell_characters::animator::is_defeat_role(role) {
                    continue;
                }
                for k in 0..6 {
                    let t = clip.duration * k as f32 / 6.0;
                    let base = sample(clip, t);
                    out.push((format!("{role} {t:.2}s"), crouched.contains(&role), base));
                    if role == "idle" || role == "walk" {
                        for swing in ["hold", "break", "place"] {
                            let Some(sc) = ch.clip(swing) else { continue };
                            let sc = TABLES.clip(sc);
                            let mut p = base;
                            layer(&mut p, &sample(sc, sc.duration * k as f32 / 6.0), 1.0);
                            out.push((format!("{role}+{swing} {t:.2}s"), false, p));
                        }
                    }
                }
            }
            out
        }

        /// A body as drawn in one view: its vertices relative to the eye, its triangles, and
        /// each vertex's part of the leg (`FOOT`, `SHIN`, else 0).
        struct Drawn {
            name: String,
            world: Vec<[f32; 3]>,
            indices: Vec<u32>,
            leg: Vec<u8>,
        }
        const FOOT: u8 = 1;
        const SHIN: u8 = 2;

        /// The sweep (CHARACTERS.md §5.1): every pose of [`poses`], the eye at its height, a step
        /// low on the ground and, crouched, still at the standing eye, every pitch from −90° to
        /// 90° in 15° steps; `draw` gives the body as drawn for a view. No triangle within the
        /// near bar of the eye, the body over at most `fill` of a 16:9 screen at the base FOV
        /// and 1.25× it, the feet in view looking down (−75° and below) and the legs while
        /// sliding (`slide_pitch` and below).
        fn sweep(
            face: FaceFrame,
            slide_pitch: f64,
            draw: &dyn Fn(&BodyDraw, V3, &crate::render::camera::Basis) -> Vec<Drawn>,
        ) {
            let ch = dwell_characters::player();
            let body = ch
                .first_person
                .and_then(|f| f.body)
                .expect("the player draws its body");
            let cfg = dwell_physics::config::PlayerControllerConfig::default();
            let lag = f64::from(cfg.max_step_lift());
            let bar = own_discard_m(&body);
            let base_fov = crate::render::camera::vertical_fov(f64::from(ASPECT));
            let mut worst_near = f32::MAX;
            let mut worst_fill: (f32, String) = (0.0, String::new());
            let mut over: Vec<String> = Vec::new();
            let poses = poses();
            for (name, crouched, pose) in &poses {
                let eye_h = f64::from(if *crouched {
                    cfg.body.crouch_eye_height
                } else {
                    cfg.body.eye_height
                });
                // The eye lags a step below its height on the ground (the step smoothing),
                // and crouched it eases down from the standing eye (the crouch smoothing).
                let role = name.split([' ', '+']).next().unwrap_or("");
                let grounded = matches!(
                    role,
                    "idle" | "walk" | "sprint" | "crouch" | "crouch_walk" | "slide" | "land"
                );
                let rise = f64::from(cfg.body.crouch_eye_height - cfg.body.eye_height);
                let drops: &[f64] = match (grounded, *crouched) {
                    (true, true) => &[0.0, lag, rise],
                    (true, false) => &[0.0, lag],
                    _ => &[0.0],
                };
                for &drop in drops {
                    for pitch in (-6..=6).map(|k| f64::from(k) * 15.0) {
                        let yaw = 30.0;
                        let feet = face.point([5.5, 10.0, 3.5]);
                        let eye = face.point([5.5, 10.0 + eye_h - drop, 3.5]);
                        let basis = crate::render::camera::aim(yaw, pitch, face, 0.0);
                        let d = BodyDraw {
                            feet,
                            yaw,
                            face,
                            look: dwell_characters::DEFAULT_LOOK,
                            pose: *pose,
                            held: Held::Empty,
                            cast_only: false,
                            first_person: true,
                            light: [1.0, 0.0],
                            seed: 0,
                            role: "idle",
                        };
                        let r = |v: V3| v.map(|c| c as f32);
                        let (right, up, fwd) = (r(basis.right), r(basis.up), r(basis.forward()));
                        for m in draw(&d, eye, &basis) {
                            let at =
                                format!("{name}, {}, pitch {pitch}, eye {drop:.2} m lower", m.name);
                            let mut cells = [false; COLS * ROWS];
                            for t in m.indices.chunks(3) {
                                let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.world[i as usize]);
                                let dist = distance(a, b, c);
                                worst_near = worst_near.min(dist);
                                assert!(
                                    dist >= bar,
                                    "{at}: a triangle {dist:.3} m from the eye (the bar {bar:.3} m)"
                                );
                                let view =
                                    [a, b, c].map(|v| [dot(v, right), dot(v, up), dot(v, fwd)]);
                                for fov in [base_fov, base_fov * 1.25] {
                                    let f = 1.0 / (fov.to_radians() as f32 / 2.0).tan();
                                    raster(view, crate::render::NEAR_PLANE as f32, f, &mut cells);
                                }
                            }
                            let fill =
                                cells.iter().filter(|c| **c).count() as f32 / cells.len() as f32;
                            // The feet are in view looking down standing or walking, and the
                            // legs sliding (the owner's ask: "particularly when sliding").
                            let wants = drop == 0.0
                                && ((matches!(role, "idle" | "walk") && pitch <= -75.0)
                                    || (role == "slide" && pitch <= slide_pitch));
                            if wants {
                                let f = 1.0 / (base_fov.to_radians() as f32 / 2.0).tan();
                                let used: std::collections::HashSet<u32> =
                                    m.indices.iter().copied().collect();
                                let seen = used.iter().any(|&i| {
                                    let (v, leg) = (m.world[i as usize], m.leg[i as usize]);
                                    let (x, y, z) = (dot(v, right), dot(v, up), dot(v, fwd));
                                    (leg == FOOT || (role == "slide" && leg == SHIN))
                                        && z > 0.1
                                        && (x / z * f / ASPECT).abs() < 1.0
                                        && (y / z * f).abs() < 1.0
                                });
                                assert!(seen, "{at}: the feet (sliding, the legs) are out of view");
                            }
                            if fill > worst_fill.0 {
                                worst_fill = (fill, at.clone());
                            }
                            if fill > body.fill {
                                over.push(format!("{at}: {:.0}%", fill * 100.0));
                            }
                        }
                    }
                }
            }
            println!(
                "{} poses: nearest triangle {worst_near:.3} m (bar {bar:.3}); most fill {:.1}% at {}",
                poses.len(),
                worst_fill.0 * 100.0,
                worst_fill.1
            );
            assert!(
                over.is_empty(),
                "the body covers more than {:.0}% of the screen in {} views, e.g.\n{}",
                body.fill * 100.0,
                over.len(),
                over.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
            );
        }

        dwell_world::both_faces! {
            /// The rigid body (`?fparms=rigid`).
            fn your_body_never_comes_near_the_camera_or_fills_the_view(face: FaceFrame) {
                let ch = dwell_characters::player();
                let body = ch.first_person.and_then(|f| f.body).expect("the player draws its body");
                let meshes: Vec<(&str, Mesh)> = ch
                    .looks
                    .iter()
                    .map(|l| (l.name, own::build(l.parts, &body)))
                    .collect();
                sweep(face, -15.0, &|d, eye, basis| {
                    let posed = bone_transforms(ch, &d.pose);
                    meshes
                        .iter()
                        .map(|(look, m)| {
                            let frame = own_frame(d, eye, basis.forward(), &body, m, &posed);
                            let bones = posed.map(|b| frame.then(&b));
                            let leg = |b: u8| {
                                let n = ch.bones[usize::from(b)].name;
                                if n.starts_with("foot") { FOOT } else if n.starts_with("shin") { SHIN } else { 0 }
                            };
                            Drawn {
                                name: (*look).to_string(),
                                world: m.positions.iter().zip(&m.bones).map(|(p, &b)| bones[usize::from(b)].apply(*p)).collect(),
                                indices: m.indices.clone(),
                                leg: m.bones.iter().map(|&b| leg(b)).collect(),
                            }
                        })
                        .collect()
                });
            }
        }

        dwell_world::both_faces! {
            /// Your own body from the player model (§11.12), at every model detail level.
            fn your_skinned_body_never_comes_near_the_camera_or_fills_the_view(face: FaceFrame) {
                let ch = dwell_characters::player();
                let body = ch.first_person.and_then(|f| f.body).expect("the player draws its body");
                let (data, model) = dwell_models::player();
                let joint = |bone: &str| data.retarget.unwrap().bones.iter().find(|b| b.bone == bone).unwrap().joint;
                let (feet_j, shins_j) = ([joint("foot_l"), joint("foot_r")], [joint("shin_l"), joint("shin_r")]);
                // Per detail level: its index, triangles, vertices and each vertex's part of the leg.
                type Level = (usize, Vec<u32>, Vec<u32>, Vec<u8>);
                let levels: Vec<Level> = (0..model.level_count())
                    .map(|level| {
                        let (tris, verts) = crate::render::skinned::own_body_mesh(data, model, level).unwrap();
                        let m = model.level(level);
                        let share = |v: usize, js: &[usize]| -> f32 {
                            (0..4).filter(|&k| js.contains(&usize::from(m.joints[v][k]))).map(|k| m.weights[v][k]).sum()
                        };
                        let leg = (0..m.positions.len())
                            .map(|v| if share(v, &feet_j) >= 0.5 { FOOT } else if share(v, &shins_j) >= 0.5 { SHIN } else { 0 })
                            .collect();
                        (level, tris, verts, leg)
                    })
                    .collect();
                // The Peasant's legs are shorter than the rig's: sliding, its feet reach less far
                // ahead, and come into view from 30° down (the rigid legs' from 15°).
                sweep(face, -30.0, &|d, eye, basis| {
                    let own = crate::render::skinned::SkinnedOwn {
                        draw: own_skinned_draw(d),
                        forward: basis.forward(),
                        body,
                        passes: 1,
                    };
                    levels
                        .iter()
                        .map(|(level, tris, verts, leg)| {
                            let (joints, _) = crate::render::skinned::own_joints(data, model, *level, verts, &own, eye).unwrap();
                            let pts = dwell_models::first_person::skin_points(model, *level, &joints, verts);
                            let mut world = vec![[0.0f32; 3]; model.level(*level).positions.len()];
                            for (&v, p) in verts.iter().zip(pts) {
                                world[v as usize] = p;
                            }
                            Drawn { name: format!("Peasant level {level}"), world, indices: tris.clone(), leg: leg.clone() }
                        })
                        .collect()
                });
            }
        }

        /// The skinned own body's draw for a rigid one (as `prepare` builds it).
        fn own_skinned_draw(d: &BodyDraw) -> crate::render::skinned::SkinnedDraw {
            crate::render::skinned::SkinnedDraw {
                model: dwell_models::PLAYER,
                seed: u32::MAX - 1,
                feet: d.feet,
                yaw: d.yaw,
                face: d.face,
                role: d.role,
                light: d.light,
                cast_only: false,
                rig_pose: Some(d.pose),
                grip: None,
                remap: crate::render::skinned::NO_REMAP,
                idle_pose: None,
            }
        }

        dwell_world::both_faces! {
            /// Your skinned body's feet are on the ground under the eye, the rig's eye height
            /// below it, walking, crouched and sliding, as the rigid body's are: looking level and
            /// down.
            fn your_skinned_bodys_feet_are_the_rigs_height_below_the_eye(face: FaceFrame) {
                let ch = dwell_characters::player();
                let body = ch.first_person.and_then(|f| f.body).unwrap();
                let cfg = dwell_physics::config::PlayerControllerConfig::default();
                let (data, model) = dwell_models::player();
                let level = dwell_models::DETAIL_DEFAULT;
                let (_, verts) = crate::render::skinned::own_body_mesh(data, model, level).unwrap();
                let rigid = own::build(ch.look(dwell_characters::DEFAULT_LOOK).unwrap().parts, &body);
                let up = face.up().map(|c| c as f32);
                for (role, crouched) in [("idle", false), ("walk", false), ("crouch", true), ("crouch_walk", true), ("slide", true)] {
                    let clip = TABLES.clip(ch.clip(role).unwrap());
                    let eye_h = if crouched { cfg.body.crouch_eye_height } else { cfg.body.eye_height };
                    for k in 0..4 {
                        let pose = sample(clip, clip.duration * k as f32 / 4.0);
                        for pitch in [0.0, -45.0, -90.0] {
                            let feet = face.point([5.5, 10.0, 3.5]);
                            let eye = face.point([5.5, 10.0 + f64::from(eye_h), 3.5]);
                            let basis = crate::render::camera::aim(30.0, pitch, face, 0.0);
                            let d = BodyDraw {
                                feet,
                                yaw: 30.0,
                                face,
                                look: dwell_characters::DEFAULT_LOOK,
                                pose,
                                held: Held::Empty,
                                cast_only: false,
                                first_person: true,
                                light: [1.0, 0.0],
                                seed: 0,
                                role,
                            };
                            let own = crate::render::skinned::SkinnedOwn { draw: own_skinned_draw(&d), forward: basis.forward(), body, passes: 1 };
                            let (joints, s) = crate::render::skinned::own_joints(data, model, level, &verts, &own, eye).unwrap();
                            let low = dwell_models::first_person::skin_points(model, level, &joints, &verts)
                                .iter()
                                .map(|p| dot(*p, up))
                                .fold(f32::MAX, f32::min);
                            // The rigid body's lowest vertex in the same view.
                            let posed = bone_transforms(ch, &pose);
                            let frame = own_frame(&d, eye, basis.forward(), &body, &rigid, &posed);
                            let rigid_low = rigid
                                .positions
                                .iter()
                                .zip(&rigid.bones)
                                .map(|(p, &b)| dot(frame.then(&posed[usize::from(b)]).apply(*p), up))
                                .fold(f32::MAX, f32::min);
                            let at = format!("{role} {k}/4, pitch {pitch}: lowest {low:.3}, rigid {rigid_low:.3}, eye {eye_h}, back {s:.2}");
                            assert!((low + eye_h).abs() < 0.08, "{at}: the feet are not on the ground under the eye");
                            assert!((low - rigid_low).abs() < 0.08, "{at}: not where the rigid body's feet were");
                        }
                    }
                }
            }
        }
    }
}
