//! Sun and moon shadows on the distant terrain (docs/DAY_NIGHT.md §12, ADR 0046; the lighting's
//! third layer): heightmaps of the terrain as drawn around the eye, horizon maps swept from them
//! toward the light, and one horizon sample per far fragment in the scene's shaders
//! (`LOD_SHADOW_WGSL`, joined to the cascades in `shadow_light`).
//!
//! - **Heightmaps**: up to two levels (each `levelScale` coarser), each two layers of a depth array
//!   (the one in use and the one being filled). A fill draws the chunks and LOD sections over its
//!   square from above (face-local, through `FaceFrame`), depth only, so each texel keeps the
//!   highest surface; a slice of its casters a frame (`fillKtris`, `fillDraws`), then its max mips
//!   (each texel the highest of the four under it, so a long step of a march sees every ridge it
//!   passes), and it replaces the one in use. A level is filled again only once the eye has moved
//!   or some seconds have passed (`refresh`): standing still costs nothing.
//! - **Horizon maps**: per level, two layers of an `r16float` array. A sweep marches every texel's
//!   ray toward the light over the heightmaps (dwell-look `lod_shadows::horizon`) and keeps the
//!   terrain's highest elevation tangent that way, `sweepRows` rows a frame; it replaces the map in
//!   use when whole. A level is swept again when its heightmap was refilled or the light has turned
//!   `turnDeg` about the face's up (its elevation needs no sweep: the fragment compares it).
//! - **Fragments** past the cascades read their column's horizon (bilinear) and shade by the
//!   light's elevation against it (`lod_shadows::shade`): a texture sample, no march.
//!
//! The scheduling ([`Schedule`]), the maps' placing ([`place`]), their matrix ([`height_matrix`])
//! and the uniform blocks are plain code, tested here; the GPU side is [`LodShadowMaps`]. None of
//! it reads anything back or blocks: the main thread's share is the fill's draws (a budget a frame).

use dwell_look::lod_shadows::{LodShadowTier, LodShadowTunables, MAX_LEVELS, NO_HORIZON, RefreshTunables, max_mips};
use dwell_look::shadows::{Caster, ShadowQuality};
use dwell_world::{ChunkCoord, FaceFrame};

use super::camera::{Mat4, V3, cross};

/// The distant shadows an adapter draws: the Shadows setting's, none on a CPU adapter (the
/// cheapest tier of everything there).
pub fn lod_shadows_for(device_type: wgpu::DeviceType, quality: ShadowQuality) -> ShadowQuality {
    if device_type == wgpu::DeviceType::Cpu {
        ShadowQuality::Off
    } else {
        quality
    }
}

/// The maps' format (depth: drawn by the casters' vertex stages with no colour target).
pub const HEIGHT_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Where a map lies: its centre (world x, z), snapped to whole texels so the texel grid stays put
/// in the world and a refill of unchanged terrain gives the same map; its texel, half-width and
/// the face it is of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub centre: [f64; 2],
    pub texel: f64,
    pub half: f64,
    pub face: FaceFrame,
}

impl Place {
    /// Whether a box (world xz, `lo`..`hi`) reaches into the map's square.
    pub fn touches(&self, lo: [f64; 2], hi: [f64; 2]) -> bool {
        (0..2).all(|k| hi[k] > self.centre[k] - self.half && lo[k] < self.centre[k] + self.half)
    }
}

/// Level `level`'s map around `eye` (world) on face `face`.
pub fn place(tier: &LodShadowTier, level: u32, eye: V3, face: FaceFrame) -> Place {
    let texel = tier.texel(level);
    let snap = |v: f64| (v / texel).round() * texel;
    Place {
        centre: [snap(eye[0]), snap(eye[2])],
        texel,
        half: tier.half_width(level),
        face,
    }
}

/// The map's y axis (world): the ortho view looks along the face-local down with world +x to its
/// right, so its up is `back × right` (back the face's up): a proper camera on either face, whose
/// image of world z runs one way on face A and the other on face B.
fn map_up(face: FaceFrame) -> V3 {
    cross(face.up(), [1.0, 0.0, 0.0])
}

/// The eye-relative point → map transform of `place` from `eye` (column-major): x and y over the
/// square (clip space), depth 0 at the top of the heights (`t.heights.topM`, face-local) and 1 at
/// their bottom, so the depth test keeps the highest surface.
pub fn height_matrix(t: &LodShadowTunables, place: &Place, eye: V3) -> Mat4 {
    let f = place.face;
    let up = map_up(f);
    let range = t.heights.top_m - t.heights.bottom_m;
    let c = [place.centre[0] - eye[0], place.centre[1] - eye[2]];
    // Face-local height of a relative point: f.y(eye_y) + rise × rel.y.
    let rise = f.vector([0.0, 1.0, 0.0])[1];
    let rows: [[f64; 4]; 3] = [
        [1.0 / place.half, 0.0, 0.0, -c[0] / place.half],
        [0.0, 0.0, up[2] / place.half, -up[2] * c[1] / place.half],
        [0.0, -rise / range, 0.0, (t.heights.top_m - f.y(eye[1])) / range],
    ];
    let mut m = [[0.0f32; 4]; 4];
    for (col, out) in m.iter_mut().enumerate() {
        for (r, row) in rows.iter().enumerate() {
            out[r] = row[col] as f32;
        }
        out[3] = if col == 3 { 1.0 } else { 0.0 };
    }
    m
}


/// The horizon maps' format (filterable: a fragment reads its column's horizon bilinearly).
pub const HORIZON_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// A caster a fill draws: a chunk's opaque mesh, an LOD section's surface, or a stand-in (an
/// ancestor section clipped to a box, world `lo`..`hi`).
#[derive(Clone, Debug, PartialEq)]
pub enum CasterRef {
    Chunk(ChunkCoord),
    Section(i64),
    StandIn { id: i64, lo: V3, hi: V3 },
}

/// A heightmap being filled.
#[derive(Clone, Debug)]
struct Fill {
    level: u32,
    place: Place,
    /// Its casters and their triangles.
    queue: Vec<(CasterRef, u64)>,
    next: usize,
    started: bool,
    /// The eye (x, z) and the time (s) it started at.
    at: [f64; 2],
    time_s: f64,
}

/// One frame's slice of a fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub level: u32,
    /// The heightmap layer it draws into (`2 × level` + the one not in use).
    pub layer: u32,
    pub place: Place,
    /// Clear the layer first (the fill's first slice).
    pub clear: bool,
    pub casters: Vec<CasterRef>,
    /// The fill's last slice: its mips follow, then it is used.
    pub last: bool,
}

/// A heightmap in use.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Heights {
    place: Place,
    /// Its layer of the level's two (0 or 1).
    layer: u32,
    at: [f64; 2],
    time_s: f64,
}

/// A horizon map in use: where (its heightmap's place), its layer of the level's two, and the
/// light's direction across the face (unit, x and z) it was swept for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Horizon {
    place: Place,
    layer: u32,
    dir: [f64; 2],
}

/// A horizon sweep under way.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sweep {
    level: u32,
    place: Place,
    dir: [f64; 2],
    layer: u32,
    next_row: u32,
}

/// One frame's band of a sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepBatch {
    pub level: u32,
    /// The horizon layer it writes (`2 × level` + the one not in use).
    pub layer: u32,
    pub place: Place,
    pub dir: [f64; 2],
    /// Its rows of texels.
    pub rows: std::ops::Range<u32>,
    /// The sweep's last band: then it is used.
    pub last: bool,
}

/// The fills' and sweeps' turns and which layers are in use.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    levels: u32,
    heights: [Option<Heights>; MAX_LEVELS],
    fill: Option<Fill>,
    turn: u32,
    horizons: [Option<Horizon>; MAX_LEVELS],
    /// The level's heightmap was refilled since its horizon was swept.
    stale: [bool; MAX_LEVELS],
    sweep: Option<Sweep>,
    sweep_turn: u32,
}

/// The angle between two unit directions (degrees).
fn turned_deg(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]).clamp(-1.0, 1.0).acos().to_degrees()
}

impl Schedule {
    pub fn new(levels: u32) -> Self {
        Self {
            levels: levels.clamp(1, MAX_LEVELS as u32),
            ..Default::default()
        }
    }

    pub fn levels(&self) -> u32 {
        self.levels
    }

    /// The level whose heightmap fill starts now, when none is under way: the first in turn that
    /// has none on face `face`, or whose eye has moved `moveShare` of its half-width since, or
    /// whose fill is `everyS` old at `now_s`.
    pub fn wants(&self, r: &RefreshTunables, eye: V3, face: FaceFrame, now_s: f64) -> Option<u32> {
        if self.fill.is_some() {
            return None;
        }
        (0..self.levels)
            .map(|k| (self.turn + k) % self.levels)
            .find(|&l| match self.heights[l as usize] {
                None => true,
                Some(h) => {
                    let moved = (eye[0] - h.at[0]).hypot(eye[2] - h.at[1]);
                    h.place.face != face
                        || moved > r.move_share * h.place.half
                        || now_s - h.time_s >= r.every_s
                }
            })
    }

    /// Starts filling `level` at `place` with `casters` (and their triangles), the eye at `eye`
    /// at `now_s`.
    pub fn start(&mut self, level: u32, place: Place, casters: Vec<(CasterRef, u64)>, eye: V3, now_s: f64) {
        self.fill = Some(Fill {
            level,
            place,
            queue: casters,
            next: 0,
            started: false,
            at: [eye[0], eye[2]],
            time_s: now_s,
        });
    }

    /// This frame's slice: casters in order up to `ktris` thousand triangles and `draws` draws
    /// (at least one, so an oversized caster still goes), none while no fill is under way.
    pub fn batch(&mut self, ktris: f64, draws: u32) -> Option<Batch> {
        let fill = self.fill.as_mut()?;
        let budget = (ktris * 1000.0) as u64;
        let mut tris = 0u64;
        let mut casters = Vec::new();
        while fill.next < fill.queue.len() && (casters.len() as u32) < draws.max(1) {
            let (c, t) = &fill.queue[fill.next];
            if !casters.is_empty() && tris + t > budget {
                break;
            }
            tris += t;
            casters.push(c.clone());
            fill.next += 1;
        }
        let clear = !fill.started;
        fill.started = true;
        let used = self.heights[fill.level as usize].map_or(1, |h| h.layer);
        Some(Batch {
            level: fill.level,
            layer: 2 * fill.level + (1 - used),
            place: fill.place,
            clear,
            casters,
            last: fill.next >= fill.queue.len(),
        })
    }

    /// After a batch is recorded: a fill whose last slice it was is used, its horizon is stale
    /// (a sweep of it under way starts again) and the next level's turn comes.
    pub fn recorded(&mut self, batch: &Batch) {
        if !batch.last {
            return;
        }
        let Some(fill) = self.fill.take() else {
            return;
        };
        let l = batch.level as usize;
        self.heights[l] = Some(Heights {
            place: batch.place,
            layer: batch.layer % 2,
            at: fill.at,
            time_s: fill.time_s,
        });
        self.stale[l] = true;
        if self.sweep.is_some_and(|s| s.level == batch.level) {
            self.sweep = None;
        }
        self.turn = (batch.level + 1) % self.levels;
    }

    /// This frame's band of a horizon sweep toward `dir` (the light across face `face`, unit x
    /// and z; none without a light): a sweep starts when none is under way and a level's horizon is
    /// missing, stale, of the other face or `turnDeg` off the light; `rows` rows of `resolution`.
    pub fn sweep(
        &mut self,
        r: &RefreshTunables,
        dir: Option<[f64; 2]>,
        face: FaceFrame,
        rows: u32,
        resolution: u32,
    ) -> Option<SweepBatch> {
        let dir = dir?;
        if self.sweep.is_none() {
            let level = (0..self.levels)
                .map(|k| (self.sweep_turn + k) % self.levels)
                .find(|&l| {
                    let Some(h) = self.heights[l as usize].filter(|h| h.place.face == face) else {
                        return false;
                    };
                    match self.horizons[l as usize] {
                        None => true,
                        Some(z) => {
                            self.stale[l as usize]
                                || z.place != h.place
                                || turned_deg(z.dir, dir) > r.turn_deg
                        }
                    }
                })?;
            let h = self.heights[level as usize].expect("found with one");
            let used = self.horizons[level as usize].map_or(1, |z| z.layer);
            self.sweep = Some(Sweep {
                level,
                place: h.place,
                dir,
                layer: 2 * level + (1 - used),
                next_row: 0,
            });
            self.stale[level as usize] = false;
        }
        let s = self.sweep.as_mut().expect("under way");
        let end = (s.next_row + rows.max(1)).min(resolution);
        let batch = SweepBatch {
            level: s.level,
            layer: s.layer,
            place: s.place,
            dir: s.dir,
            rows: s.next_row..end,
            last: end >= resolution,
        };
        s.next_row = end;
        Some(batch)
    }

    /// After a sweep's band is recorded: its last band puts the horizon in use.
    pub fn swept(&mut self, batch: &SweepBatch) {
        if !batch.last || self.sweep.is_none_or(|s| s.layer != batch.layer) {
            return;
        }
        self.sweep = None;
        self.horizons[batch.level as usize] = Some(Horizon {
            place: batch.place,
            layer: batch.layer % 2,
            dir: batch.dir,
        });
        self.sweep_turn = (batch.level + 1) % self.levels;
    }

    /// Level `level`'s heightmap in use on face `face`, and its texture layer.
    pub fn heights(&self, level: u32, face: FaceFrame) -> Option<(Place, u32)> {
        self.heights
            .get(level as usize)
            .copied()
            .flatten()
            .filter(|h| h.place.face == face)
            .map(|h| (h.place, 2 * level + h.layer))
    }

    /// Level `level`'s horizon map in use on face `face`, and its texture layer.
    pub fn horizon(&self, level: u32, face: FaceFrame) -> Option<(Place, u32)> {
        self.horizons
            .get(level as usize)
            .copied()
            .flatten()
            .filter(|h| h.place.face == face)
            .map(|h| (h.place, 2 * level + h.layer))
    }
}

/// The light's direction across face `face` (unit, x and z), none when it is (nearly) overhead.
pub fn light_dir(face: FaceFrame, caster: &Caster) -> Option<[f64; 2]> {
    let l = face.vector(caster.dir);
    let h = l[0].hypot(l[2]);
    (h > 1e-4).then(|| [l[0] / h, l[2] / h])
}

/// Floats in the scene's uniform block (`LodShadows` in `LOD_SHADOW_WGSL`).
pub const UNIFORM_FLOATS: usize = (MAX_LEVELS + 2) * 4;

/// The scene's uniform block: per level its horizon map's centre relative to the eye (x, z),
/// half-width and layer (−1: none); the light (world) and its strength (0: none); the soft edge
/// (1 / tan of the penumbra), the edge fade, the viewer's face and the maps' image of world z.
pub fn uniforms(
    t: &LodShadowTunables,
    tier: Option<&LodShadowTier>,
    schedule: &Schedule,
    eye: V3,
    face: FaceFrame,
    caster: Option<&Caster>,
) -> [f32; UNIFORM_FLOATS] {
    let mut u = [0.0f32; UNIFORM_FLOATS];
    for l in 0..MAX_LEVELS {
        u[l * 4 + 3] = -1.0;
    }
    let (Some(tier), Some(caster)) = (tier, caster) else {
        return u;
    };
    for l in 0..schedule.levels().min(tier.levels) {
        if let Some((p, layer)) = schedule.horizon(l, face) {
            u[l as usize * 4..l as usize * 4 + 4].copy_from_slice(&[
                (p.centre[0] - eye[0]) as f32,
                (p.centre[1] - eye[2]) as f32,
                p.half as f32,
                layer as f32,
            ]);
        }
    }
    let at = MAX_LEVELS * 4;
    u[at..at + 8].copy_from_slice(&[
        caster.dir[0] as f32,
        caster.dir[1] as f32,
        caster.dir[2] as f32,
        caster.strength as f32,
        (1.0 / t.march.penumbra_deg.to_radians().tan()) as f32,
        t.march.edge_fade as f32,
        face.sign_f32(),
        map_up(face)[2] as f32,
    ]);
    u
}

/// Floats in the sweep's uniform block (`Sweep` in `SWEEP_WGSL`).
pub const SWEEP_FLOATS: usize = (MAX_LEVELS + 3) * 4;

/// The sweep's uniform block: per level its heightmap's centre relative to the swept map's centre
/// (x, z), half-width and layer (−1: none); the heights' top and range, the maps' image of world z
/// and their texels a side; the march (steps, first step in texels, reach, bias); the light's
/// direction across the face (x, z), the swept map's half-width and the mips.
pub fn sweep_uniforms(
    t: &LodShadowTunables,
    tier: &LodShadowTier,
    schedule: &Schedule,
    batch: &SweepBatch,
) -> [f32; SWEEP_FLOATS] {
    let mut u = [0.0f32; SWEEP_FLOATS];
    let f = batch.place.face;
    for l in 0..MAX_LEVELS as u32 {
        let at = l as usize * 4;
        u[at + 3] = -1.0;
        if l >= schedule.levels() {
            continue;
        }
        if let Some((p, layer)) = schedule.heights(l, f) {
            u[at..at + 4].copy_from_slice(&[
                (p.centre[0] - batch.place.centre[0]) as f32,
                (p.centre[1] - batch.place.centre[1]) as f32,
                p.half as f32,
                layer as f32,
            ]);
        }
    }
    let at = MAX_LEVELS * 4;
    u[at..at + 12].copy_from_slice(&[
        t.heights.top_m as f32,
        (t.heights.top_m - t.heights.bottom_m) as f32,
        map_up(f)[2] as f32,
        tier.resolution as f32,
        tier.steps as f32,
        t.march.start_texels as f32,
        tier.reach_m as f32,
        t.march.bias_m as f32,
        batch.dir[0] as f32,
        batch.dir[1] as f32,
        batch.place.half as f32,
        max_mips(tier.resolution) as f32,
    ]);
    u
}

/// The frame group's distant-shadow entries (bindings 11–13: the uniform block, the horizon maps,
/// their linear sampler).
pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: 11,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(UNIFORM_FLOATS as u64 * 4),
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ]
}

/// The max-mip pass: a full-screen triangle; each texel the highest (the least depth) of the four
/// under it in the level before.
const MIP_WGSL: &str = r"
@group(0) @binding(0) var src: texture_depth_2d;

@vertex
fn mip_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn mip_fs(@builtin(position) at: vec4f) -> @builtin(frag_depth) f32 {
    let c = vec2i(at.xy) * 2;
    let a = textureLoad(src, c, 0);
    let b = textureLoad(src, c + vec2i(1, 0), 0);
    let d = textureLoad(src, c + vec2i(0, 1), 0);
    let e = textureLoad(src, c + vec2i(1, 1), 0);
    return min(min(a, b), min(d, e));
}
";

/// The horizon sweep: a full-screen triangle over a band of rows of a horizon layer; each texel
/// marches toward the light over the heightmaps in use (dwell-look `lod_shadows::horizon`'s copy:
/// the same steps, levels and mips) and keeps the terrain's highest elevation tangent.
pub const SWEEP_WGSL: &str = r"
struct Sweep {
    // Per level: xy its heightmap's centre relative to the swept map's centre (world x, z),
    // z its half-width (m), w its layer (−1: none).
    maps: array<vec4f, 2>,
    // x the heights' top (face-local m, depth 0), y their range (m), z the maps' image of world z
    // (±1), w their texels a side.
    heights: vec4f,
    // x steps, y the first step (texels), z the reach (m), w the bias (m).
    march: vec4f,
    // xy the light's direction across the face (unit; world x, z), z the swept map's half-width,
    // w the mips.
    dir: vec4f,
};
@group(0) @binding(0) var<uniform> sweep: Sweep;
@group(0) @binding(1) var heights: texture_depth_2d_array;

@vertex
fn sweep_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

// The finest level whose heightmap holds a column (strictly inside its square), or −1.
fn sweep_level(q: vec2f) -> i32 {
    for (var l = 0; l < 2; l++) {
        let m = sweep.maps[l];
        if (m.w >= 0.0 && all(abs(q - m.xy) < vec2f(m.z))) {
            return l;
        }
    }
    return -1;
}

// The face-local height at a column in level `l`'s heightmap at mip `mip`: mip 0 bilinear between
// texel centres, a coarser mip the highest of its texel.
fn sweep_height(l: i32, q: vec2f, mip: i32) -> f32 {
    let m = sweep.maps[l];
    let d = (q - m.xy) / m.z;
    let uv = vec2f(0.5 + 0.5 * d.x, 0.5 - 0.5 * d.y * sweep.heights.z);
    let side = i32(sweep.heights.w) >> u32(mip);
    let hi = vec2i(side - 1);
    var depth: f32;
    if (mip == 0) {
        let t = uv * f32(side) - 0.5;
        let base = floor(t);
        let f = t - base;
        let c = vec2i(base);
        let a = textureLoad(heights, clamp(c, vec2i(0), hi), i32(m.w), 0);
        let b = textureLoad(heights, clamp(c + vec2i(1, 0), vec2i(0), hi), i32(m.w), 0);
        let e = textureLoad(heights, clamp(c + vec2i(0, 1), vec2i(0), hi), i32(m.w), 0);
        let g = textureLoad(heights, clamp(c + vec2i(1, 1), vec2i(0), hi), i32(m.w), 0);
        depth = mix(mix(a, b, f.x), mix(e, g, f.x), f.y);
    } else {
        let c = clamp(vec2i(floor(uv * f32(side))), vec2i(0), hi);
        depth = textureLoad(heights, c, i32(m.w), mip);
    }
    return sweep.heights.x - depth * sweep.heights.y;
}

@fragment
fn sweep_fs(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let res = sweep.heights.w;
    let half = sweep.dir.z;
    let uv = at.xy / res;
    // The texel's column relative to the swept map's centre.
    let col = vec2f((2.0 * uv.x - 1.0) * half, (1.0 - 2.0 * uv.y) * sweep.heights.z * half);
    let home = sweep_level(col);
    if (home < 0) {
        return vec4f(-NO_HORIZON, 0.0, 0.0, 1.0);
    }
    let texel = 2.0 * sweep.maps[home].z / res;
    let h0 = sweep_height(home, col, 0) + sweep.march.w;
    let start = texel * sweep.march.y;
    let steps = i32(sweep.march.x);
    let growth = pow(sweep.march.z / start, 1.0 / f32(max(steps, 2) - 1));
    let mips = i32(sweep.dir.w);
    var most = -NO_HORIZON;
    var s = start;
    for (var i = 0; i < steps; i++) {
        let q = col + sweep.dir.xy * s;
        let lv = sweep_level(q);
        if (lv >= 0) {
            let tl = 2.0 * sweep.maps[lv].z / res;
            let mip = min(i32(round(log2(max(s * (growth - 1.0) / tl, 1.0)))), mips - 1);
            most = max(most, (sweep_height(lv, q, mip) - h0) / s);
        }
        s *= growth;
    }
    return vec4f(min(most, NO_HORIZON), 0.0, 0.0, 1.0);
}
";

/// The maps on the GPU (a 1×1 single layer of each while off, so the frame group always binds
/// one): the heightmaps (a depth array with mips), the views fills and mip passes draw into and
/// the mip passes' sources; the horizon maps and their bands' views; the sweep's pipeline, uniform
/// and group; the scene's sampler and uniform block; and the fills' frame uniform and group (the
/// casters' pass layout).
pub struct LodShadowMaps {
    /// (resolution, layers, mips) as made.
    pub size: (u32, u32, u32),
    heights: wgpu::Texture,
    /// Per layer, per mip: the view drawn into.
    targets: Vec<Vec<wgpu::TextureView>>,
    /// Per layer, per mip from 1: the group reading the mip before.
    sources: Vec<Vec<wgpu::BindGroup>>,
    mip_pipeline: wgpu::RenderPipeline,
    horizons: wgpu::Texture,
    pub horizon_view: wgpu::TextureView,
    horizon_targets: Vec<wgpu::TextureView>,
    sweep_pipeline: wgpu::RenderPipeline,
    sweep_uniform: wgpu::Buffer,
    sweep_group: wgpu::BindGroup,
    pub sampler: wgpu::Sampler,
    pub uniform: wgpu::Buffer,
    pub frame: wgpu::Buffer,
    pub frame_group: wgpu::BindGroup,
}

fn full_screen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: String,
    layout: &wgpu::BindGroupLayout,
    (vs, fs): (&str, &str),
    color: Option<wgpu::TextureFormat>,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let targets = color.map(|format| {
        [Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })]
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: targets.as_ref().map_or(&[], |t| &t[..]),
        }),
        primitive: Default::default(),
        depth_stencil: color.is_none().then_some(wgpu::DepthStencilState {
            format: HEIGHT_DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// The sweep's WGSL: its module with the horizon's range.
pub fn sweep_source() -> String {
    format!("const NO_HORIZON: f32 = {NO_HORIZON:.1};\n{SWEEP_WGSL}")
}

impl LodShadowMaps {
    /// Maps for `tier` (none: the 1×1 stand-ins), with the fills' frame group (`pass_layout`: the
    /// frame uniform, the atlas and its sampler; `frame_bytes`).
    pub fn new(
        device: &wgpu::Device,
        tier: Option<&LodShadowTier>,
        pass_layout: &wgpu::BindGroupLayout,
        atlas: (&wgpu::TextureView, &wgpu::Sampler),
        frame_bytes: u64,
    ) -> Self {
        let size = maps_size(tier);
        let (resolution, layers, mips) = size;
        let extent = wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: layers,
        };
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let heights = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lod shadow heights"),
            size: extent,
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HEIGHT_DEPTH,
            usage,
            view_formats: &[],
        });
        let heights_view = heights.create_view(&wgpu::TextureViewDescriptor {
            label: Some("lod shadow heights"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let view = |texture: &wgpu::Texture, layer: u32, mip: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("lod shadow layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let targets: Vec<Vec<wgpu::TextureView>> = (0..layers)
            .map(|l| (0..mips).map(|m| view(&heights, l, m)).collect())
            .collect();
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lod shadow mip"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let sources = targets
            .iter()
            .map(|levels| {
                levels
                    .iter()
                    .take(levels.len().saturating_sub(1))
                    .map(|v| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("lod shadow mip"),
                            layout: &mip_layout,
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(v),
                            }],
                        })
                    })
                    .collect()
            })
            .collect();
        let mip_pipeline = full_screen_pipeline(
            device,
            "lod shadow mip",
            MIP_WGSL.to_string(),
            &mip_layout,
            ("mip_vs", "mip_fs"),
            None,
        );
        let horizons = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lod shadow horizons"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HORIZON_FORMAT,
            usage,
            view_formats: &[],
        });
        let horizon_view = horizons.create_view(&wgpu::TextureViewDescriptor {
            label: Some("lod shadow horizons"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let horizon_targets = (0..layers).map(|l| view(&horizons, l, 0)).collect();
        let sweep_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lod shadow sweep"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(SWEEP_FLOATS as u64 * 4),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let sweep_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod shadow sweep"),
            size: SWEEP_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sweep_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lod shadow sweep"),
            layout: &sweep_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sweep_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&heights_view),
                },
            ],
        });
        let sweep_pipeline = full_screen_pipeline(
            device,
            "lod shadow sweep",
            sweep_source(),
            &sweep_layout,
            ("sweep_vs", "sweep_fs"),
            Some(HORIZON_FORMAT),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lod shadow horizons"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod shadows"),
            size: UNIFORM_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lod shadow frame"),
            size: frame_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lod shadow frame"),
            layout: pass_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas.0),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(atlas.1),
                },
            ],
        });
        Self {
            size,
            heights,
            targets,
            sources,
            mip_pipeline,
            horizons,
            horizon_view,
            horizon_targets,
            sweep_pipeline,
            sweep_uniform,
            sweep_group,
            sampler,
            uniform,
            frame,
            frame_group,
        }
    }

    /// The frame group's entries for the horizon maps (bindings 11–13).
    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 11,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(&self.horizon_view),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ]
    }

    /// The view a fill of heightmap `layer` draws into (its level 0).
    pub fn target(&self, layer: u32) -> &wgpu::TextureView {
        &self.targets[layer as usize][0]
    }

    /// Records the max mips of heightmap `layer`, each from the one before.
    pub fn record_mips(&self, encoder: &mut wgpu::CommandEncoder, layer: u32) {
        let l = layer as usize;
        for (m, source) in self.sources[l].iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lod shadow mip"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets[l][m + 1],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, source, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Records a sweep's band (its uniform written first: `sweep_uniforms`).
    pub fn record_sweep(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        batch: &SweepBatch,
        uniform: &[f32; SWEEP_FLOATS],
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        queue.write_buffer(&self.sweep_uniform, 0, bytemuck::cast_slice(uniform.as_slice()));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lod shadow sweep"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.horizon_targets[batch.layer as usize],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes,
            ..Default::default()
        });
        let res = self.size.0;
        pass.set_scissor_rect(0, batch.rows.start, res, batch.rows.end - batch.rows.start);
        pass.set_pipeline(&self.sweep_pipeline);
        pass.set_bind_group(0, &self.sweep_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The maps' GPU bytes: the heightmaps with their mips, the horizon maps.
    pub fn bytes(&self) -> u64 {
        let (r, l, _) = self.size;
        let texels = u64::from(r) * u64::from(r) * u64::from(l);
        texels * 4 * 4 / 3 + texels * 2
    }
}

impl Drop for LodShadowMaps {
    fn drop(&mut self) {
        // wgpu's WebGPU backend frees nothing on drop.
        self.heights.destroy();
        self.horizons.destroy();
        self.sweep_uniform.destroy();
        self.uniform.destroy();
        self.frame.destroy();
    }
}

/// The maps' (resolution, layers, mips) for a tier (1×1, one layer, one mip for none).
pub fn maps_size(tier: Option<&LodShadowTier>) -> (u32, u32, u32) {
    tier.map_or((1, 1, 1), |t| {
        (
            t.resolution,
            2 * t.levels.clamp(1, MAX_LEVELS as u32),
            max_mips(t.resolution),
        )
    })
}

/// The far fragments' shading the scene's shaders share (group 0, bindings 11–13), after
/// `COMMON`'s helpers and before `SHADOW_WGSL` (whose `shadow_past` calls it). `lod_shadow(rel)`
/// is the light the distant terrain lets through to an eye-relative point, before the light's
/// strength: its column's horizon (bilinear) against the light's elevation, softly (dwell-look
/// `lod_shadows::shade`), faded out toward the outermost map's edge.
pub const LOD_SHADOW_WGSL: &str = r"
struct LodShadows {
    // Per level: xy its horizon map's centre relative to the eye (world x, z), z its half-width
    // (m), w its layer (−1: none).
    maps: array<vec4f, 2>,
    // xyz toward the light (world), w its shadows' strength (0: none).
    light: vec4f,
    // x 1 / tan(the penumbra), y the edge fade (share of the half-width), z the viewer's face
    // (±1), w the maps' image of world z (±1).
    cfg: vec4f,
};
@group(0) @binding(11) var<uniform> lod_shadows: LodShadows;
@group(0) @binding(12) var lod_horizons: texture_2d_array<f32>;
@group(0) @binding(13) var lod_horizon_sampler: sampler;

// The light the distant terrain lets through to an eye-relative point, 0–1, before the light's
// strength; 1 where no map holds it, on the other face, or with the light under the horizon.
fn lod_shadow(rel: vec3f) -> f32 {
    let face = face_at(frame.eye.x + rel.y);
    if (lod_shadows.light.w <= 0.0 || face != lod_shadows.cfg.z) {
        return 1.0;
    }
    let l = face_vector(face, lod_shadows.light.xyz);
    let horizontal = length(l.xz);
    if (l.y <= 0.0 || horizontal < 1e-4) {
        return 1.0;
    }
    // The finest level whose map holds the column.
    var lv = -1;
    for (var i = 0; i < 2; i++) {
        let m = lod_shadows.maps[i];
        if (lv < 0 && m.w >= 0.0 && all(abs(rel.xz - m.xy) < vec2f(m.z))) {
            lv = i;
        }
    }
    if (lv < 0) {
        return 1.0;
    }
    let m = lod_shadows.maps[lv];
    let d = (rel.xz - m.xy) / m.z;
    let uv = vec2f(0.5 + 0.5 * d.x, 0.5 - 0.5 * d.y * lod_shadows.cfg.w);
    let horizon = textureSampleLevel(lod_horizons, lod_horizon_sampler, uv, i32(m.w), 0.0).x;
    let v = smoothstep(-1.0, 1.0, (l.y / horizontal - horizon) * lod_shadows.cfg.x);
    // Faded out toward the outermost map's edge.
    var outer = lod_shadows.maps[lv];
    if (lod_shadows.maps[1].w >= 0.0) {
        outer = lod_shadows.maps[1];
    }
    let off = max(abs(rel.x - outer.x), abs(rel.z - outer.y));
    let edge = 1.0 - smoothstep(outer.z * (1.0 - lod_shadows.cfg.y), outer.z, off);
    return mix(1.0, v, edge);
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::lod_shadows::LOD_SHADOWS;

    const T: LodShadowTunables = LOD_SHADOWS;
    const R: RefreshTunables = LOD_SHADOWS.refresh;

    fn tier() -> LodShadowTier {
        T.tiers[1]
    }

    fn apply(m: &Mat4, p: V3) -> [f64; 3] {
        std::array::from_fn(|r| {
            (0..3).map(|c| f64::from(m[c][r]) * p[c]).sum::<f64>() + f64::from(m[3][r])
        })
    }

    dwell_world::both_faces!(
        fn the_map_puts_a_column_where_the_shaders_read_it_and_the_highest_nearest(f: FaceFrame) {
            let t = tier();
            let eye = f.point([1234.5, 300.0, -876.25]);
            let p = place(&t, 0, eye, f);
            let m = height_matrix(&T, &p, eye);
            // A world point, as the fill draws it (eye-relative) and as the shaders read it.
            let world = f.point([1300.0, 120.0, -800.0]);
            let rel = [world[0] - eye[0], world[1] - eye[1], world[2] - eye[2]];
            let clip = apply(&m, rel);
            let up = map_up(f);
            let d = [(world[0] - p.centre[0]) / p.half, (world[2] - p.centre[1]) / p.half];
            assert!((clip[0] - d[0]).abs() < 1e-5);
            assert!((clip[1] - d[1] * up[2]).abs() < 1e-5);
            // Depth decodes to the face-local height (the shaders' `top − depth × range`).
            let range = T.heights.top_m - T.heights.bottom_m;
            let h = T.heights.top_m - clip[2] * range;
            assert!((h - 120.0).abs() < 1e-3, "{h}");
            // Higher (face-locally) is nearer: the depth test keeps it.
            let higher = f.point([1300.0, 130.0, -800.0]);
            let rel2 = [higher[0] - eye[0], higher[1] - eye[1], higher[2] - eye[2]];
            assert!(apply(&m, rel2)[2] < clip[2]);
        }
    );

    dwell_world::both_faces!(
        fn the_map_is_a_proper_camera_on_either_face(f: FaceFrame) {
            // Its axes (right, up, back toward the face's up) are right-handed: the view is a
            // rotation, never a mirror.
            let up = map_up(f);
            let back = f.up();
            let r = [1.0, 0.0, 0.0];
            let det = r[0] * (up[1] * back[2] - up[2] * back[1])
                - r[1] * (up[0] * back[2] - up[2] * back[0])
                + r[2] * (up[0] * back[1] - up[1] * back[0]);
            assert!((det - 1.0).abs() < 1e-12);
        }
    );

    #[test]
    fn a_map_is_placed_on_whole_texels_around_the_eye() {
        for f in FaceFrame::under_test() {
            let t = tier();
            for level in 0..t.levels {
                let p = place(&t, level, [1000.3, 50.0, -77.7], f);
                for c in p.centre {
                    assert!((c / p.texel - (c / p.texel).round()).abs() < 1e-9);
                }
                assert!((p.centre[0] - 1000.3).abs() <= p.texel / 2.0);
                assert_eq!(p.half, t.half_width(level));
                // Moving less than half a texel keeps the place (refills give the same map).
                assert_eq!(place(&t, level, [1000.3 + p.texel * 0.1, 50.0, -77.7], f), p);
            }
        }
    }

    fn casters(n: usize, tris: u64) -> Vec<(CasterRef, u64)> {
        (0..n).map(|i| (CasterRef::Section(i as i64), tris)).collect()
    }

    const EYE: V3 = [0.0, 50.0, 0.0];

    fn at(f: FaceFrame, level: u32) -> Place {
        place(&tier(), level, EYE, f)
    }

    /// Runs a fill of `level` to its end; the frames it took.
    fn fill(s: &mut Schedule, f: FaceFrame, level: u32, n: usize, now: f64) -> u32 {
        s.start(level, at(f, level), casters(n, 10_000), EYE, now);
        let mut frames = 0;
        loop {
            let b = s.batch(35.0, 1000).expect("a fill");
            s.recorded(&b);
            frames += 1;
            if b.last {
                return frames;
            }
        }
    }

    #[test]
    fn a_fill_is_spread_over_frames_within_its_budget_and_used_when_whole() {
        for f in FaceFrame::under_test() {
            let mut s = Schedule::new(2);
            assert_eq!(s.wants(&R, EYE, f, 0.0), Some(0));
            s.start(0, at(f, 0), casters(100, 10_000), EYE, 0.0);
            assert_eq!(s.wants(&R, EYE, f, 0.0), None, "one fill at a time");
            let mut seen = Vec::new();
            let mut frames = 0;
            loop {
                let b = s.batch(35.0, 1000).expect("a fill");
                assert_eq!(b.clear, frames == 0);
                assert_eq!(b.layer, 0, "the first fill: the first layer");
                // 35k triangles a frame: three 10k casters.
                assert!(b.casters.len() <= 3);
                assert!(s.heights(0, f).is_none(), "nothing used before the fill is whole");
                seen.extend(b.casters.clone());
                s.recorded(&b);
                frames += 1;
                if b.last {
                    break;
                }
            }
            assert_eq!(frames, 34);
            assert_eq!(seen, casters(100, 0).into_iter().map(|c| c.0).collect::<Vec<_>>());
            assert_eq!(s.heights(0, f), Some((at(f, 0), 0)));
            // The other level's turn; then level 0 again only once it is due, into the other
            // layer, the old map used while it fills.
            assert_eq!(s.wants(&R, EYE, f, 0.0), Some(1));
            fill(&mut s, f, 1, 1, 0.0);
            assert_eq!(s.heights(1, f), Some((at(f, 1), 2)));
            assert_eq!(s.wants(&R, EYE, f, 1.0), None, "nothing changed: no fill");
            assert_eq!(s.wants(&R, EYE, f, R.every_s), Some(0), "due by time");
            s.start(0, at(f, 0), casters(4, 10_000), EYE, R.every_s);
            let b = s.batch(35.0, 1000).expect("a fill");
            assert_eq!(b.layer, 1, "the layer not in use");
            s.recorded(&b);
            assert_eq!(s.heights(0, f), Some((at(f, 0), 0)));
            let b = s.batch(35.0, 1000).expect("a fill");
            assert!(b.last);
            s.recorded(&b);
            assert_eq!(s.heights(0, f), Some((at(f, 0), 1)));
            // A map is the face's it was made on.
            assert!(s.heights(0, f.other()).is_none());
            assert_eq!(s.wants(&R, EYE, f.other(), R.every_s), Some(1));
        }
    }

    #[test]
    fn moving_far_enough_fills_a_level_again() {
        for f in FaceFrame::under_test() {
            let mut s = Schedule::new(2);
            fill(&mut s, f, 0, 3, 0.0);
            fill(&mut s, f, 1, 3, 0.0);
            let half0 = tier().half_width(0);
            let near = [EYE[0] + half0 * R.move_share * 0.9, EYE[1], EYE[2]];
            assert_eq!(s.wants(&R, near, f, 1.0), None);
            let far = [EYE[0] + half0 * R.move_share * 1.1, EYE[1], EYE[2]];
            assert_eq!(s.wants(&R, far, f, 1.0), Some(0), "the near level, not the far one");
        }
    }

    #[test]
    fn a_slice_keeps_to_the_draws_and_an_oversized_caster_still_goes() {
        for f in FaceFrame::under_test() {
            let mut s = Schedule::new(1);
            s.start(0, at(f, 0), casters(10, 1_000_000), EYE, 0.0);
            let b = s.batch(35.0, 1000).expect("a fill");
            assert_eq!(b.casters.len(), 1);
            let mut s = Schedule::new(1);
            s.start(0, at(f, 0), casters(10, 1), EYE, 0.0);
            let b = s.batch(1000.0, 4).expect("a fill");
            assert_eq!(b.casters.len(), 4);
            // An empty fill clears its layer and is used at once.
            let mut s = Schedule::new(1);
            s.start(0, at(f, 0), Vec::new(), EYE, 0.0);
            let b = s.batch(1.0, 1).expect("a fill");
            assert!(b.clear && b.last && b.casters.is_empty());
            s.recorded(&b);
            assert!(s.heights(0, f).is_some());
        }
    }

    /// Runs a sweep toward `dir` to its end, or none; its bands.
    fn sweep_all(s: &mut Schedule, f: FaceFrame, dir: [f64; 2]) -> Vec<SweepBatch> {
        let mut out = Vec::new();
        while let Some(b) = s.sweep(&R, Some(dir), f, 100, 1024) {
            s.swept(&b);
            let last = b.last;
            out.push(b);
            if last {
                break;
            }
        }
        out
    }

    #[test]
    fn a_horizon_is_swept_in_bands_once_its_heightmap_is_there_and_again_only_when_needed() {
        for f in FaceFrame::under_test() {
            let mut s = Schedule::new(2);
            let east = [1.0, 0.0];
            assert!(s.sweep(&R, Some(east), f, 100, 1024).is_none(), "no heightmap yet");
            fill(&mut s, f, 0, 2, 0.0);
            assert!(s.sweep(&R, None, f, 100, 1024).is_none(), "no light");
            let bands = sweep_all(&mut s, f, east);
            assert_eq!(bands.len(), 11);
            assert_eq!(bands[0].rows, 0..100);
            assert_eq!(bands[10].rows, 1000..1024);
            assert!(bands.iter().all(|b| b.level == 0 && b.layer == 0));
            assert_eq!(s.horizon(0, f), Some((at(f, 0), 0)));
            // Nothing changed: no sweep, no cost.
            assert!(s.sweep(&R, Some(east), f, 100, 1024).is_none());
            // The light turns a little: still none; past turnDeg: a sweep into the other layer.
            let turn = |deg: f64| [deg.to_radians().cos(), deg.to_radians().sin()];
            assert!(s.sweep(&R, Some(turn(R.turn_deg * 0.5)), f, 100, 1024).is_none());
            let b = s.sweep(&R, Some(turn(R.turn_deg * 1.5)), f, 100, 1024).expect("a sweep");
            assert_eq!(b.layer, 1);
            s.swept(&b);
            assert_eq!(s.horizon(0, f), Some((at(f, 0), 0)), "the old one used meanwhile");
            sweep_all(&mut s, f, turn(R.turn_deg * 1.5));
            assert_eq!(s.horizon(0, f), Some((at(f, 0), 1)));
            // A refilled heightmap makes its horizon stale; a refill during a sweep restarts it.
            fill(&mut s, f, 1, 1, 0.0);
            let b = s.sweep(&R, Some(east), f, 100, 1024).expect("level 1's");
            assert_eq!(b.level, 1);
            s.swept(&b);
            fill(&mut s, f, 1, 1, 0.0);
            let b = s.sweep(&R, Some(east), f, 100, 1024).expect("again");
            assert_eq!((b.level, b.rows.start), (1, 0), "restarted from its first row");
            assert!(s.horizon(1, f.other()).is_none());
        }
    }

    #[test]
    fn the_uniforms_carry_the_maps_in_use_and_nothing_without_a_light() {
        for f in FaceFrame::under_test() {
            let t = tier();
            let mut s = Schedule::new(t.levels);
            let eye = f.point([10.0, 80.0, 20.0]);
            let caster = Caster {
                dir: [0.0, 1.0, 0.0],
                moon: false,
                strength: 0.75,
            };
            let u = uniforms(&T, Some(&t), &s, eye, f, Some(&caster));
            assert_eq!((u[3], u[7]), (-1.0, -1.0), "no maps yet");
            fill(&mut s, f, 0, 1, 0.0);
            sweep_all(&mut s, f, [0.0, 1.0]);
            let p = at(f, 0);
            let u = uniforms(&T, Some(&t), &s, eye, f, Some(&caster));
            assert_eq!(u[3], 0.0);
            assert_eq!(u[2], p.half as f32);
            assert_eq!(u[0], (p.centre[0] - eye[0]) as f32);
            assert_eq!(u[11], 0.75);
            assert_eq!(u[14], f.sign_f32());
            assert_eq!(u[15], map_up(f)[2] as f32);
            assert_eq!(uniforms(&T, Some(&t), &s, eye, f, None)[11], 0.0);
            assert_eq!(uniforms(&T, None, &s, eye, f, Some(&caster))[11], 0.0);
            // The sweep's: the heightmaps relative to the swept one, the march, the light.
            fill(&mut s, f, 1, 1, 0.0);
            let b = s.sweep(&R, Some([0.6, 0.8]), f, 32, t.resolution).expect("a sweep");
            let w = sweep_uniforms(&T, &t, &s, &b);
            assert_eq!(b.level, 1);
            assert_eq!((w[0], w[1]), ((at(f, 0).centre[0] - at(f, 1).centre[0]) as f32, 0.0));
            assert_eq!((w[3], w[7]), (0.0, 2.0));
            assert_eq!(w[10], map_up(f)[2] as f32);
            assert_eq!(w[12], t.steps as f32);
            assert_eq!((w[16], w[17]), (0.6, 0.8));
            assert_eq!(w[18], t.half_width(1) as f32);
        }
    }

    #[test]
    fn a_cpu_adapter_draws_no_distant_shadows() {
        for q in [ShadowQuality::Off, ShadowQuality::Low, ShadowQuality::High] {
            assert_eq!(lod_shadows_for(wgpu::DeviceType::Cpu, q), ShadowQuality::Off);
            assert_eq!(lod_shadows_for(wgpu::DeviceType::DiscreteGpu, q), q);
            assert_eq!(lod_shadows_for(wgpu::DeviceType::IntegratedGpu, q), q);
        }
        assert_eq!(maps_size(None), (1, 1, 1));
        let t = tier();
        assert_eq!(maps_size(Some(&t)), (t.resolution, 2 * t.levels, max_mips(t.resolution)));
    }

    #[test]
    fn the_sweep_parses_validates_and_is_the_mirrors_march() {
        let module = naga::front::wgsl::parse_str(&sweep_source()).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        let module = naga::front::wgsl::parse_str(MIP_WGSL).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        // dwell-look `lod_shadows::horizon` is the march in f64; these are its lines here.
        for line in [
            "let h0 = sweep_height(home, col, 0) + sweep.march.w;",
            "let start = texel * sweep.march.y;",
            "let growth = pow(sweep.march.z / start, 1.0 / f32(max(steps, 2) - 1));",
            "let mip = min(i32(round(log2(max(s * (growth - 1.0) / tl, 1.0)))), mips - 1);",
            "most = max(most, (sweep_height(lv, q, mip) - h0) / s);",
            "return vec4f(min(most, NO_HORIZON), 0.0, 0.0, 1.0);",
        ] {
            assert!(SWEEP_WGSL.contains(line), "{line}");
        }
        // And `lod_shadows::shade` is the fragment's.
        let w = LOD_SHADOW_WGSL;
        assert!(w.contains(
            "let v = smoothstep(-1.0, 1.0, (l.y / horizontal - horizon) * lod_shadows.cfg.x);"
        ));
        assert!(w.contains(
            "let edge = 1.0 - smoothstep(outer.z * (1.0 - lod_shadows.cfg.y), outer.z, off);"
        ));
        // Face-local through the face frame's helpers only.
        assert!(w.contains("face_vector(face, lod_shadows.light.xyz)"));
    }
}
