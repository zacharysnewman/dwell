//! Far trees as impostors (docs/proposals/TREE_IMPOSTORS.md phase I-3, WORK_QUEUE item 31): one
//! camera-facing quad a tree, drawn from the atlas `dwell_worldgen::impostor` bakes from the real
//! tree models (albedo, normals, depth), lit by the scene's sun, ambient and fog at each texel's
//! baked position, and handed over from the near trees' leaves by the screen dither. The quad
//! writes its own depth: a `frag_depth` offset to the baked depth disabled early depth testing and
//! cost more than it fixed (WORK_QUEUE item 31); it is Ultra's to bring back (proposal I-5).
//!
//! - **The atlas:** one `texture_2d_array` layer a baked tree (`Config::layer_count`), baked by the
//!   job workers (`Job::ImpostorBake`) and uploaded a layer at a time as they come back
//!   ([`Impostors::upload_layer`]); a layer not yet baked draws nothing.
//! - **The instances:** per tile of [`TILE_M`] m, the trees `TerrainGenerator::far_trees` lists
//!   (`Job::FarTrees`), as `Config::instances` places them; the game keeps the tiles within the
//!   tier's `toM` of the eye ([`tiles_within`]) and the buffer is rebuilt when they change.
//! - **The view:** per pixel, `Config::dither`'s pick of the baked views (the WGSL twin
//!   `imp_pick`), each sampled at the quad point projected into that view.
//! - **The hand-over:** per tree, in time (render/tree_fade.rs, BUGS.md #154): each tree is its
//!   real leaves or its impostor by its distance with hysteresis, a switch cross-fading over the
//!   tier's `fadeS`; the near leaves keep the pixels whose noise is under their tree's share
//!   (`cutout_fs`, `TREE_LEAF_FADE`) and the impostor the rest, so no pixel draws both; past the
//!   leaves' reach the near leaves are dropped whole in the vertex stage.
//! - **Faces:** instances are face-local (`Config::instances`) and cross into the world through
//!   `FaceFrame::point`; the shader turns the baked frame into the world by `face_vector`.
//! - **Shadows:** received (the scene's shadow lookup); not cast (I-5).

use std::collections::{HashMap, HashSet};

use dwell_world::FaceFrame;
use dwell_worldgen::impostor::{Config, Far, FarTree, Instance, Tier};

use super::frame::{DEPTH, DEPTH_COMPARE, HDR, VIEW_DEPTH};
use super::pipeline;
use super::tree_fade::{self, TreeFades};

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

/// A tile's far trees short of the felled (WORK_QUEUE item 31): `trees` as the generator lists them
/// on `face`, `held` the client's state of a world cell where it holds the chunk
/// (`VoxelWorld::voxel_if_held`). A tree whose held stem has air in it is felled
/// (`impostor::stem_seen`) and remembered in `felled` (face bit, anchor), so it stays out after its
/// chunk unloads; one seen standing again (rebuilt, or the cell refilled) is forgotten.
pub fn standing_trees(
    face: FaceFrame,
    trees: &[FarTree],
    held: impl Fn([i32; 3]) -> Option<dwell_blocks::StateId>,
    felled: &mut HashSet<(u8, [i32; 3])>,
) -> Vec<FarTree> {
    let bit = u8::from(face.is_b());
    trees
        .iter()
        .filter(|t| {
            let key = (bit, t.anchor);
            match dwell_worldgen::impostor::stem_seen(t, |c| held(face.cell(c))) {
                Some(false) => {
                    felled.insert(key);
                    false
                }
                Some(true) => {
                    felled.remove(&key);
                    true
                }
                None => !felled.contains(&key),
            }
        })
        .copied()
        .collect()
}

/// The far trees tiles (face bit, i, k) a chunk (`coord`, world chunk indices) lies in: its
/// columns' tile on either face (a chunk's x and z are the same on both).
pub fn tiles_of_chunk(coord: [i32; 3]) -> [(u8, i32, i32); 2] {
    let s = dwell_world::CHUNK_SIZE;
    let (i, k) = (
        (coord[0] * s).div_euclid(TILE_M),
        (coord[2] * s).div_euclid(TILE_M),
    );
    [(0, i, k), (1, i, k)]
}

/// The atlas tier a device bakes at (`impostors.json` `viewPx`): the CPU adapter's cheapest, a
/// phone's smaller atlas (32 px a view: a quarter of the desktop's memory), else the desktop's.
pub fn tier_for(cpu: bool, phone: bool) -> Tier {
    if cpu {
        Tier::Cpu
    } else if phone {
        Tier::Phone
    } else {
        Tier::Desktop
    }
}

/// The bytes one tier's atlas holds (two RGBA8 textures over every layer, its mips included).
pub fn atlas_bytes(config: &Config, tier: Tier) -> u64 {
    let px = config.px(tier);
    let n = config.views.len() as u32;
    let cols = (n as f32).sqrt().ceil().max(1.0) as u32;
    let rows = n.div_ceil(cols);
    let (w, h) = (cols * px, rows * px);
    let per_level: u64 = (0..mip_count(config, px))
        .map(|l| u64::from((w >> l).max(1)) * u64::from((h >> l).max(1)))
        .sum();
    per_level * 8 * u64::from(config.layer_count())
}

/// The atlas's mip levels at `px` texels a view: the chain down to a texel a view when the data
/// asks for mips (`impostors.json` `mips`, opt-in until the owner approves the look), else 1.
pub fn mip_count(config: &Config, px: u32) -> u32 {
    if config.mips {
        dwell_worldgen::impostor::Impostor::mip_levels(px)
    } else {
        1
    }
}

/// The levels a baked layer uploads (`size` the atlas's texels, `px` a view's side, `mips` the
/// level count): (width, height, albedo bytes, normal + depth bytes) per level, level 0 the bake
/// itself and each next `Impostor::mip` (each view's square halved alone).
pub fn mip_chain(
    size: (u32, u32),
    px: u32,
    mips: u32,
    albedo: &[u8],
    normal_depth: &[u8],
) -> Vec<(u32, u32, Vec<u8>, Vec<u8>)> {
    let mut out = vec![(size.0, size.1, albedo.to_vec(), normal_depth.to_vec())];
    if mips <= 1 || px == 0 {
        return out;
    }
    let texels = |b: &[u8]| -> Vec<[u8; 4]> {
        b.chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect()
    };
    let im = dwell_worldgen::impostor::Impostor {
        px,
        cols: size.0 / px,
        rows: size.1 / px,
        centre: [0.0; 3],
        radius: 0.0,
        views: Vec::new(),
        albedo: texels(albedo),
        normal_depth: texels(normal_depth),
    };
    for level in 1..mips {
        let (a, n) = im.mip(level);
        out.push((
            (size.0 >> level).max(1),
            (size.1 >> level).max(1),
            a.into_flattened(),
            n.into_flattened(),
        ));
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

/// The hand-over the frame draws with (BUGS.md #106): the tier's `far` only while the impostors
/// are `ready` to draw the far trees ([`Impostors::ready`]: the atlas baked and the area's
/// instances in), else `None`, so the near crowns draw to the full-detail distance and the LOD
/// sections keep their trees, as before the impostors. Without this a tier "on" whose impostors
/// never drew left every tree past the hand-over a bare trunk.
pub fn drawn(far: Option<Far>, ready: bool) -> Option<Far> {
    far.filter(|f| f.impostor && ready)
}

/// The LOD sections mesh their trees (Graphics → Far trees on) unless the impostors draw them
/// (`drawn`, [`drawn`]).
pub fn lod_trees(far_trees: bool, drawn: Option<Far>) -> bool {
    far_trees && drawn.is_none()
}

/// The radius (m) about the eye whose far trees tiles must all be in before the impostors take
/// over ([`ready`]): the hand-over's end and a tile, past which the tiles stream in nearest first.
pub fn ready_radius(far: &Far) -> f32 {
    (tree_fade::leaves_reach(far) + TILE_M as f32).min(reach(far))
}

/// Whether the impostors can draw the far trees around the eye (`eye`, world, on `face`): every
/// atlas layer baked (`baked` of `layer_count`), every tile within [`ready_radius`] held (`held`:
/// its instance count, `None` not in yet) and at least one tree among them. An area whose tiles
/// list no trees at all stays not ready, which is harmless (no trees, no leaves to drop) and keeps
/// a worker that answers every tile empty (no terrain) from dropping the near crowns.
pub fn ready(
    baked: u32,
    layer_count: u32,
    far: &Far,
    face: FaceFrame,
    eye: [f64; 3],
    held: impl Fn(&(u8, i32, i32)) -> Option<usize>,
) -> bool {
    if !far.impostor || layer_count == 0 || baked < layer_count {
        return false;
    }
    let local = face.point(eye);
    let bit = u8::from(face.is_b());
    let mut trees = 0;
    for (i, k) in tiles_within([local[0], local[2]], ready_radius(far)) {
        match held(&(bit, i, k)) {
            Some(n) => trees += n,
            None => return false,
        }
    }
    trees > 0
}

/// What the impostors hold now (`__dwell.impostors()`, the bench): their readiness and why not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Made at all (the tier draws impostors).
    pub made: bool,
    /// The tier's hand-over is impostors.
    pub tier_on: bool,
    /// The frame draws them (`ready`).
    pub ready: bool,
    pub baked: u32,
    pub layers: u32,
    /// Bakes sent to the workers.
    pub requested: bool,
    /// Tiles held on any face, and their instances.
    pub tiles: u32,
    pub instances: u32,
    /// Instances drawn last frame (the eye's face).
    pub drawn: u32,
    /// The last thing that went wrong (a layer dropped, an empty bake), empty when none.
    pub last_error: String,
}

/// The farthest any cutout plant or leaf card draws (m): every block's distance fade end
/// (`fade`, e.g. tall grass) and the leaf cards' (shared/look/leaves.json `cards.fadeEndM`).
pub fn plants_reach() -> f32 {
    static REACH: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *REACH.get_or_init(|| {
        let blocks = dwell_blocks::STATES
            .iter()
            .filter_map(|s| s.fade.map(|f| f.end))
            .fold(0.0f32, f32::max);
        blocks.max(dwell_look::leaves::LEAVES.cards.fade_end_m as f32)
    })
}

/// How far (m) a chunk's cutout mesh is drawn at all with the impostors' hand-over `far`: past
/// both its end (every tree leaf there dropped) and the plants' fades, nothing in the mesh draws,
/// so the chunk's cutout is not submitted (the leaves' vertices are what the forest edge is bound
/// by, WORK_QUEUE item 32). `None`: every chunk's (no impostors).
pub fn cutout_reach(far: Option<Far>) -> Option<f32> {
    let f = far.filter(|f| f.impostor)?;
    Some(tree_fade::leaves_reach(&f).max(plants_reach()) + REACH_MARGIN_M)
}

/// What of a chunk's cutout mesh draws (its triangles ordered with the trees' leaves last,
/// `dwell_mesh::tree_leaves_from`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutoutDraw {
    /// The whole mesh.
    All,
    /// The plants, ladders and other cutouts, not the trees' leaves: the chunk is past the far
    /// trees' hand-over (every leaf there dropped by the shader) but within the plants' fades.
    Plants,
    /// Nothing: past both.
    Nothing,
}

/// What of a chunk's cutout mesh draws with its nearest point `d` m from the eye and the
/// impostors' hand-over `far` (shared/trees/impostors/impostors.json `far.tiers`): its leaves
/// only within the hand-over's end ([`cutout_reach`]'s margin past it), so the leaves' vertices
/// past it, which the shader would drop, are not drawn (WORK_QUEUE item 32, the forest edge).
pub fn cutout_draw(far: Option<Far>, d: f32) -> CutoutDraw {
    let (Some(f), Some(end)) = (far.filter(|f| f.impostor), cutout_reach(far)) else {
        return CutoutDraw::All;
    };
    if d <= tree_fade::leaves_reach(&f) + REACH_MARGIN_M {
        CutoutDraw::All
    } else if d <= end {
        CutoutDraw::Plants
    } else {
        CutoutDraw::Nothing
    }
}

/// Metres past a fade's end a chunk's cutout still draws (a swaying vertex's reach).
const REACH_MARGIN_M: f32 = 2.0;

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

/// The scale toward the eye that moves a quad whose sphere centre is `dist` m away to the sphere's
/// near side (radius `r`) while it covers the same pixels (the WGSL twin `imp_near_side`): drawn at
/// the centre, the near tree's trunk and branches (still drawn past the hand-over) stood in front
/// of the crown and showed as bare sticks (BUGS.md #106).
pub fn near_side(dist: f32, r: f32) -> f32 {
    ((dist - r) / dist.max(1e-3)).clamp(0.05, 1.0)
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
// FaceFrame); far: the hand-over (in, out, to, 1 on; the trees' states are render/tree_fade.rs's); leaf: the baked leaf albedo (linear), w a
// view's side in texels.
struct ImpostorScene {
    eye: vec4f,
    far: vec4f,
    leaf: vec4f,
    // x: the debug view (`__dwell.impostorDebug`, BUGS.md #106): 0 off, 1 the whole quad solid,
    // 2 the alpha-tested silhouette solid, 3 the albedo unlit. y, z: a leaf texel's ambient share
    // and albedo scale (impostors.json `leafShade`). w: the atlas's mip levels (`mip_count`).
    debug: vec4f,
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
    // The near tree's share of this tree (render/tree_fade.rs): the impostor keeps the rest.
    @location(7) @interpolate(flat) near: f32,
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

// The scale toward the eye that moves a quad at `dist` m to the near side of its sphere of
// radius `r` (render/impostors.rs `near_side`).
fn imp_near_side(dist: f32, r: f32) -> f32 {
    return clamp((dist - r) / max(dist, 1e-3), 0.05, 1.0);
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
    // Its tree's state (render/tree_fade.rs), looked up at its foot: the leaves read the same tree.
    // Before the impostors draw (`frame.far_trees.w` 0) the near trees are everything.
    o.near = select(1.0, tree_near_at(frame.far_trees.xy + face_vector(face, centre).xz),
        frame.far_trees.w > 0.5);
    // Not baked yet, the real tree's alone or past the far trees: nothing.
    if (info.z < 0.5 || o.near >= 0.999 || dist - r > imp.far.z) {
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
    // The quad stands at the bounding sphere's near side, scaled toward the eye so it covers the
    // same pixels (BUGS.md #106): at the centre the near tree's trunk and branches, which stay
    // drawn, stood in front of its crown and showed as bare sticks.
    o.clip = frame.view_proj * vec4f(rel * imp_near_side(dist, r), 1.0);
    return o;
}

struct ImpOutFs {
    @location(0) color: vec4f,
    @location(1) depth: f32,
    @builtin(sample_mask) mask: u32,
};

@fragment
fn imp_fs(i: ImpOut) -> ImpOutFs {
    let face = imp.eye.w;
    let r = i.radius;
    // The mip level (`mips`, render/impostors.rs `mip_count`): a screen pixel's footprint in a
    // view's texels, taken before any branch (derivatives need uniform control flow).
    let foot = max(length(dpdx(i.local)), length(dpdy(i.local))) / max(r, 1e-3) * 0.5 * imp.leaf.w;
    let levels = max(u32(imp.debug.w), 1u);
    let level = min(u32(max(floor(log2(max(foot, 1.0))), 0.0)), levels - 1u);
    // The view this pixel draws, then the quad point in that view's square.
    let k = imp_pick(i.dir, pixel_noise(i.clip.xy + vec2f(37.0, 17.0)));
    let vk = imp_view(k);
    let right = imp_basis_right(vk);
    let up = cross(vk, right);
    let u = dot(i.local, right) / r;
    let v = dot(i.local, up) / r;
    let dbg = u32(imp.debug.x);
    if (dbg == 1u) {
        var q: ImpOutFs;
        q.color = vec4f(1.0, 0.0, 1.0, 1.0);
        q.depth = frame_depth(view_depth(i.centre), true);
        q.mask = 0xffffffffu;
        return q;
    }
    if (abs(u) >= 1.0 || abs(v) >= 1.0) {
        discard;
    }
    let px = max(floor(imp.leaf.w / f32(1u << level)), 1.0);
    let cell = vec2i(i32(k % IMP_COLS), i32(k / IMP_COLS)) * i32(px);
    let t = vec2i(clamp(vec2f((u * 0.5 + 0.5) * px, (0.5 - v * 0.5) * px), vec2f(0.0), vec2f(px - 1.0)));
    let a = textureLoad(imp_albedo, cell + t, i.layer, i32(level));
    if (a.a < 0.5) {
        discard;
    }
    if (dbg == 2u) {
        var q: ImpOutFs;
        q.color = vec4f(1.0, 0.0, 1.0, 1.0);
        q.depth = frame_depth(view_depth(i.centre), true);
        q.mask = 0xffffffffu;
        return q;
    }
    let nd = textureLoad(imp_normal, cell + t, i.layer, i32(level));
    // The texel's real point: on the view's plane at the sphere's near side, moved back by its depth.
    let p = right * u * r + up * v * r + vk * r * (1.0 - 2.0 * nd.a);
    let rel = i.centre + face_vector(face, imp_turn(p, i.turns));
    // The hand-over: the near leaves keep the pixels whose noise is under their tree's share
    // (`cutout_fs`), cross-faded in time (render/tree_fade.rs).
    let near = i.near;
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
    // The baked occlusion in the alpha (`impostor::ao_alpha`: 128–255 → 0–1; 255 when off).
    var albedo = a.rgb * clamp((a.a * 255.0 - 128.0) / 127.0, 0.0, 1.0);
    var light = open_light();
    if (distance(a.rgb, imp.leaf.rgb) < 0.02) {
        albedo *= i.tint * imp.debug.z;
        light.ambient = imp.debug.y;
    }
    let n = normalize(face_vector(face, imp_turn(nd.rgb * 2.0 - 1.0, i.turns)));
    let lit = scene_out(albedo, n, rel, 1.0, light);
    var o: ImpOutFs;
    o.color = select(lit.color, vec4f(albedo, 1.0), dbg == 3u);
    o.depth = frame_depth(lit.depth, true);
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
    /// The tiles' instances as placed (face-local), for the trees' states.
    placed: HashMap<(u8, i32, i32), Vec<Instance>>,
    /// Each tree's near/impostor state about the eye and the GPU's map of them.
    pub fades: TreeFades,
    dirty: bool,
    /// The hand-over in use (`None`: no impostors drawn).
    pub far: Option<Far>,
    /// Bakes asked for (the game sends them once).
    pub requested: bool,
    /// The debug view (`__dwell.impostorDebug`, BUGS.md #106): 0 off, 1 the quads solid, 2 the
    /// silhouettes solid, 3 the albedo unlit.
    pub debug: u32,
    /// A leaf texel's ambient share and albedo scale (`Config::leaf_ambient`, `leaf_albedo`;
    /// `__dwell.impostorDebug`'s tuning overrides them).
    pub leaf_shade: [f32; 2],
    /// Layers uploaded (of `layers.len()`).
    baked: u32,
    /// The atlas's mip levels ([`mip_count`]).
    mips: u32,
    /// The last upload or bake that went wrong ([`Status::last_error`]).
    last_error: String,
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
        let mips = mip_count(config, px);
        let texture = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: layer_count,
                },
                mip_level_count: mips,
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
            size: 64,
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
            placed: HashMap::new(),
            fades: TreeFades::default(),
            dirty: false,
            far: None,
            requested: false,
            debug: 0,
            leaf_shade: [config.leaf_ambient, config.leaf_albedo],
            baked: 0,
            mips,
            last_error: String::new(),
        }
    }

    /// Whether they can draw the far trees about `eye` (world) on `face` ([`ready`]).
    pub fn ready(&self, face: FaceFrame, eye: [f64; 3]) -> bool {
        self.far.is_some_and(|far| {
            ready(
                self.baked,
                self.layers.len() as u32,
                &far,
                face,
                eye,
                |key| {
                    self.tiles
                        .get(key)
                        .map(|b| b.len() / INSTANCE_BYTES as usize)
                },
            )
        })
    }

    /// What they hold now (`ready` the frame's).
    pub fn status(&self, ready: bool) -> Status {
        Status {
            made: true,
            tier_on: self.far.is_some_and(|f| f.impostor),
            ready,
            baked: self.baked,
            layers: self.layers.len() as u32,
            requested: self.requested,
            tiles: self.tiles.len() as u32,
            instances: self
                .tiles
                .values()
                .map(|b| (b.len() / INSTANCE_BYTES as usize) as u32)
                .sum(),
            drawn: self.count,
            last_error: self.last_error.clone(),
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
            self.last_error = format!("layer {layer} past the atlas's {}", self.layers.len());
            return;
        };
        if albedo.len() != bytes || normal_depth.len() != bytes {
            self.last_error = format!(
                "layer {layer}: {} / {} bytes baked, the atlas takes {bytes}",
                albedo.len(),
                normal_depth.len()
            );
            crate::telemetry::add("impostor.dropped_layers", 1.0);
            return;
        }
        if slot[2] < 0.5 {
            self.baked += 1;
        }
        let px = self.px;
        let levels = mip_chain(self.size, px, self.mips, albedo, normal_depth);
        for (level, (w, h, a, n)) in levels.iter().enumerate() {
            for (t, data) in [(&self.albedo, a), (&self.normal, n)] {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: t,
                        mip_level: level as u32,
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
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(*h),
                    },
                    wgpu::Extent3d {
                        width: *w,
                        height: *h,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        *slot = [centre_y, radius, 1.0, 0.0];
        self.layers_dirty = true;
        self.fades.touch();
    }

    /// A tile's far trees on `face` (tile indices `ti`, `tk`), placed as `Config::instances` does.
    pub fn set_tile(&mut self, face: FaceFrame, ti: i32, tk: i32, trees: &[FarTree]) {
        let leaf = self.config.leaf;
        let mut bytes = Vec::with_capacity(trees.len() * INSTANCE_BYTES as usize);
        for (t, inst) in trees.iter().zip(self.config.instances(trees)) {
            bytes.extend_from_slice(&pack(face, &inst, tint_ratio(t, leaf)));
        }
        let key = (u8::from(face.is_b()), ti, tk);
        self.tiles.insert(key, bytes);
        self.placed.insert(key, self.config.instances(trees));
        self.dirty = true;
        self.fades.touch();
    }

    /// Drops the tiles `keep` doesn't hold (face, i, k).
    pub fn retain_tiles(&mut self, keep: impl Fn(&(u8, i32, i32)) -> bool) {
        let before = self.tiles.len();
        self.tiles.retain(|k, _| keep(k));
        self.placed.retain(|k, _| keep(k));
        if self.tiles.len() != before {
            self.dirty = true;
            self.fades.touch();
        }
    }

    /// Whether tile (face, i, k) is held.
    pub fn has_tile(&self, key: &(u8, i32, i32)) -> bool {
        self.tiles.contains_key(key)
    }

    /// This frame's tree states at `now` (s) about `eye` (world) on `face` (render/tree_fade.rs):
    /// whether the map changed.
    pub fn update_fades(&mut self, now: f64, face: FaceFrame, eye: [f64; 3], far: &Far) -> bool {
        let bit = u8::from(face.is_b());
        let (placed, layers) = (&self.placed, &self.layers);
        self.fades.update(now, face, eye, far, move || {
            Box::new(
                placed
                    .iter()
                    .filter(move |(k, _)| k.0 == bit)
                    .flat_map(|(_, v)| v.iter())
                    .map(move |inst| {
                        let l = layers.get(inst.layer as usize).copied().unwrap_or_default();
                        tree_fade::Tree {
                            key: [
                                inst.foot[0].floor() as i32,
                                (inst.foot[1] / dwell_world::CELL_H as f32).round() as i32,
                                inst.foot[2].floor() as i32,
                            ],
                            foot: inst.foot,
                            centre_y: l[0],
                            radius: l[1],
                            baked: l[2] > 0.5,
                        }
                    }),
            )
        })
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
            in_m: 0.0,
            out_m: 0.0,
            fade_s: 0.0,
            to: 0.0,
        });
        let lin = |c: f32| dwell_look::srgb_to_linear(f64::from(c)) as f32;
        let leaf = self.config.leaf.map(|c| lin((c * 255.0).round() / 255.0));
        let u: [f32; 16] = [
            eye[0] as f32,
            eye[1] as f32,
            eye[2] as f32,
            face.vector([0.0, 1.0, 0.0])[1] as f32,
            far.in_m,
            far.out_m,
            reach(&far),
            if far.impostor { 1.0 } else { 0.0 },
            leaf[0],
            leaf[1],
            leaf[2],
            self.px as f32,
            self.debug as f32,
            self.leaf_shade[0],
            self.leaf_shade[1],
            self.mips as f32,
        ];
        queue.write_buffer(&self.uniform, 0, super::floats(&u));
        if self.layers_dirty {
            queue.write_buffer(
                &self.layer_buffer,
                0,
                super::floats(self.layers.as_flattened()),
            );
            self.layers_dirty = false;
        }
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let face_bit = u8::from(face.is_b());
        // Nearest tiles first, so the depth test rejects most of the hidden quads before shading.
        let local = face.point(eye);
        let t = f64::from(TILE_M);
        let d2 = |&(_, i, k): &(u8, i32, i32)| {
            let dx = (f64::from(i) + 0.5) * t - local[0];
            let dz = (f64::from(k) + 0.5) * t - local[2];
            dx * dx + dz * dz
        };
        let mut keys: Vec<_> = self
            .tiles
            .keys()
            .filter(|k| k.0 == face_bit)
            .copied()
            .collect();
        keys.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
        let all: Vec<u8> = keys
            .iter()
            .flat_map(|k| self.tiles[k].iter().copied())
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
    fn chunk_cutouts_are_cut_only_past_the_hand_over_and_the_plants() {
        // face-a-only: distances only.
        let c = Config::get();
        let on = c.far("on").expect("on");
        assert_eq!(cutout_reach(c.far("off")), None);
        assert_eq!(cutout_reach(None), None);
        let r = cutout_reach(Some(on)).expect("impostors");
        assert!(
            r > tree_fade::leaves_reach(&on),
            "every near leaf dropped first"
        );
        assert!(r > plants_reach(), "tall grass faded out first");
        assert!(plants_reach() >= dwell_look::leaves::LEAVES.cards.fade_end_m as f32);
    }

    #[test]
    fn a_chunk_past_the_hand_over_draws_its_plants_without_its_leaves() {
        // face-a-only: distances only.
        // WORK_QUEUE item 32 (the forest edge): the leaves stop at the data's hand-over end
        // (impostors.json `far.tiers.on` outM and a crown, `tree_fade::leaves_reach`), the plants
        // at their own fades. Red
        // before: every chunk within the plants' reach drew its leaves too.
        let c = Config::get();
        let on = c.far("on").expect("on");
        let leaves_end = tree_fade::leaves_reach(&on) + REACH_MARGIN_M;
        assert!(leaves_end < plants_reach(), "the band this saves is empty");
        assert_eq!(cutout_draw(Some(on), 0.0), CutoutDraw::All);
        assert_eq!(cutout_draw(Some(on), leaves_end), CutoutDraw::All);
        assert_eq!(cutout_draw(Some(on), leaves_end + 0.5), CutoutDraw::Plants);
        let end = cutout_reach(Some(on)).expect("impostors");
        assert_eq!(cutout_draw(Some(on), end), CutoutDraw::Plants);
        assert_eq!(cutout_draw(Some(on), end + 0.5), CutoutDraw::Nothing);
        // The switch is the data's: a later hand-over keeps the leaves later.
        let later = Far {
            out_m: on.out_m + 20.0,
            ..on
        };
        assert_eq!(cutout_draw(Some(later), leaves_end + 0.5), CutoutDraw::All);
        // No impostors (the Far trees tier off, or not ready): the whole mesh at every distance.
        assert_eq!(cutout_draw(c.far("off"), 500.0), CutoutDraw::All);
        assert_eq!(cutout_draw(None, 500.0), CutoutDraw::All);
    }

    #[test]
    fn the_reach_is_the_tiers_to_or_the_cap() {
        // face-a-only: data only.
        let c = Config::get();
        let on = c.far("on").expect("on");
        assert!(reach(&on) > tree_fade::leaves_reach(&on));
        assert!(reach(&on) <= MAX_REACH_M);
    }

    /// BUGS.md #106: the quad stood at its sphere's centre, so the near tree's logs (inside the
    /// sphere, drawn past the hand-over) won the depth test over its crown and every far tree
    /// read as a bare trunk with branches. Every quad point, scaled by `near_side`, lies on the
    /// same ray from the eye (same pixel) and no farther than where that ray enters the sphere,
    /// so nothing of the tree stands in front of it. Red with no scale (the centre's plane).
    #[test]
    fn the_quad_stands_in_front_of_everything_in_its_sphere() {
        // face-a-only: eye-relative geometry, no face.
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        for &(dist, r) in &[(50.0f32, 8.0f32), (80.0, 13.0), (150.0, 13.0), (300.0, 6.0)] {
            let centre = [0.0, 0.0, dist];
            let s = near_side(dist, r);
            for &(cx, cy) in &[(0.0, 0.0), (0.5, 0.0), (0.0, -0.7), (0.6, 0.6), (-0.3, 0.2)] {
                let p = [cx * r, cy * r, dist];
                let q = p.map(|c| c * s);
                let len = dot(q, q).sqrt();
                let ray = q.map(|c| c / len);
                // The same pixel: the scaled point on the unscaled point's ray.
                let pl = dot(p, p).sqrt();
                assert!((dot(ray, p.map(|c| c / pl)) - 1.0).abs() < 1e-5);
                // Where the ray enters the sphere (if it does): t² − 2t(ray·c) + |c|² − r² = 0.
                let b = dot(ray, centre);
                let disc = b * b - (dot(centre, centre) - r * r);
                if disc >= 0.0 {
                    let enter = b - disc.sqrt();
                    assert!(
                        len <= enter + 1e-3,
                        "dist {dist} r {r} at ({cx}, {cy}): the quad at {len} m, the sphere from {enter} m"
                    );
                }
            }
        }
        assert!(IMPOSTOR_WGSL.contains("rel * imp_near_side(dist, r)"));
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
            stem: [0, 0, 0],
            stem_cells: 0,
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
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry}"
            );
        }
    }

    dwell_world::both_faces!(
        /// Felled trees leave the far trees (WORK_QUEUE item 31): a tile's tree whose stem the client
        /// holds with air in it (a cut) is not drawn, stays out once its chunk unloads, and comes back
        /// only when its stem is seen solid again; trees the client holds nothing of are drawn.
        fn a_felled_tree_leaves_its_tile_and_stays_out_after_its_chunk_unloads(f: FaceFrame) {
            use std::cell::RefCell;
            let tree = |x: i32| FarTree {
                anchor: [x, 130, 5],
                species: 0,
                seed: 9,
                stage: 7,
                stem: [x, 130, 5],
                stem_cells: 8,
            };
            let trees = [tree(1), tree(20)];
            let (log, air) = (7, dwell_world::M.air);
            // The world as the client holds it: tree 1's stem cut 1 m up, world cells by the face.
            let cut = f.cell([1, 132, 5]);
            let held = RefCell::new(true);
            let at = |c: [i32; 3]| -> Option<dwell_blocks::StateId> {
                if !*held.borrow() {
                    return None;
                }
                let local = f.cell(c);
                (local[0] == 1 || local[0] == 20).then(|| {
                    if c[0] == cut[0] && c[2] == cut[2] && local[1] >= 132 {
                        air
                    } else {
                        log
                    }
                })
            };
            let mut felled = HashSet::new();
            let drawn = standing_trees(f, &trees, at, &mut felled);
            assert_eq!(drawn, vec![tree(20)], "the cut tree is out");
            *held.borrow_mut() = false;
            assert_eq!(
                standing_trees(f, &trees, at, &mut felled),
                vec![tree(20)],
                "still out once its chunk unloads"
            );
            let solid = |_: [i32; 3]| Some(log);
            assert_eq!(
                standing_trees(f, &trees, solid, &mut felled),
                trees.to_vec()
            );
            assert!(felled.is_empty());
            assert_eq!(
                standing_trees(f, &trees, |_| None, &mut HashSet::new()),
                trees.to_vec(),
                "nothing held: the generator's trees"
            );
            // A chunk's tile on either face.
            assert_eq!(tiles_of_chunk([-1, 4, 2]), [(0, -1, 1), (1, -1, 1)]);
        }
    );

    #[test]
    fn a_layer_uploads_its_mip_chain_only_when_the_data_asks() {
        // face-a-only: texels only.
        let c = Config::get();
        assert_eq!(
            mip_count(c, 64),
            1,
            "mips are opt-in (impostors.json `mips`)"
        );
        let on = Config {
            mips: true,
            ..c.clone()
        };
        assert_eq!(mip_count(&on, 64), 7);
        assert_eq!(mip_count(&on, 48), 1);
        let b = dwell_worldgen::impostor::bake(&dwell_worldgen::species::SPECIES[0], 3, 7, c, 16);
        let (w, h) = b.size();
        let (a, n) = (b.albedo.as_flattened(), b.normal_depth.as_flattened());
        assert_eq!(mip_chain((w, h), 16, 1, a, n).len(), 1);
        let chain = mip_chain((w, h), 16, mip_count(&on, 16), a, n);
        assert_eq!(chain.len(), 5);
        for (level, (lw, lh, la, ln)) in chain.iter().enumerate() {
            assert_eq!((*lw, *lh), (w >> level, h >> level));
            assert_eq!(la.len(), (lw * lh * 4) as usize);
            assert_eq!(ln.len(), la.len());
        }
        assert_eq!(&chain[0].2[..], a);
    }

    #[test]
    fn a_phone_bakes_the_smaller_atlas_the_data_names() {
        // face-a-only: memory only.
        let c = Config::get();
        assert_eq!(tier_for(false, true), Tier::Phone);
        assert_eq!(
            tier_for(true, true),
            Tier::Cpu,
            "a CPU adapter's cheapest wins"
        );
        assert_eq!(tier_for(false, false), Tier::Desktop);
        let (desk, phone) = (atlas_bytes(c, Tier::Desktop), atlas_bytes(c, Tier::Phone));
        assert!(c.px(Tier::Phone) < c.px(Tier::Desktop));
        assert_eq!(desk, phone * 4, "half the side: a quarter of the bytes");
        // 16 MB: 25 views since BUGS.md #154 (6 azimuths × 4 elevations + the top, 1.25× the 17).
        assert!(phone <= 16 << 20, "the phone's atlas {} MB", phone >> 20);
    }
}
