//! Sun and moon shadows on the distant terrain (docs/DAY_NIGHT.md §12, ADR 0046; the lighting's
//! third layer). Heightmaps of the terrain as drawn, around the eye, and the march over them in
//! the scene's shaders (`LOD_SHADOW_WGSL`, joined to the cascades in `shadow_light`).
//!
//! The maps: up to two levels (each `levelScale` coarser), each two layers of a depth array — the
//! one shown and the one being filled. A fill draws the chunks and LOD sections over its square
//! from above (face-local, through `FaceFrame`), depth only, so each texel keeps the highest
//! surface; it takes a slice of its casters a frame (`fillKtris`, `fillDraws`: the cost is spread
//! and never spikes), then its max mips (each texel the highest of the four under it, so a long
//! step of the march sees every ridge it passes), and replaces the shown layer. The levels fill in
//! turn, without end: a map follows the eye, edits and LOD swaps within a few frames, and reading
//! it costs no main-thread work at all.
//!
//! The scheduling ([`Schedule`]), the maps' placing ([`place`]), their matrix ([`height_matrix`])
//! and the uniform block ([`uniforms`]) are plain code, tested here; the GPU side is
//! [`LodShadowMaps`].

use dwell_look::lod_shadows::{LodShadowTier, LodShadowTunables, MAX_LEVELS, max_mips};
use dwell_look::shadows::{Caster, ShadowQuality};
use dwell_world::{ChunkCoord, FaceFrame};

use super::camera::{Mat4, V3, cross};

/// The distant shadows an adapter draws: the Shadows setting's, none on a CPU adapter (the
/// cheapest tier of everything there: every far pixel's march would be the CPU's time).
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

/// A caster a fill draws: a chunk's opaque mesh, an LOD section's surface, or a stand-in (an
/// ancestor section clipped to a box, world `lo`..`hi`).
#[derive(Clone, Debug, PartialEq)]
pub enum CasterRef {
    Chunk(ChunkCoord),
    Section(i64),
    StandIn { id: i64, lo: V3, hi: V3 },
}

/// A map being filled.
#[derive(Clone, Debug)]
struct Fill {
    level: u32,
    place: Place,
    /// Its casters and their triangles.
    queue: Vec<(CasterRef, u64)>,
    next: usize,
    started: bool,
}

/// One frame's slice of a fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub level: u32,
    /// The texture layer it draws into (`2 × level` + the one not shown).
    pub layer: u32,
    pub place: Place,
    /// Clear the layer first (the fill's first slice).
    pub clear: bool,
    pub casters: Vec<CasterRef>,
    /// The fill's last slice: its mips follow, then it is shown.
    pub last: bool,
}

/// The fills' turns and which layer of each level is shown.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    levels: u32,
    /// Per level: the map shown (none until its first fill is whole) and its layer (0 or 1).
    front: [Option<(Place, u32)>; MAX_LEVELS],
    fill: Option<Fill>,
    turn: u32,
}

impl Schedule {
    pub fn new(levels: u32) -> Self {
        Self {
            levels: levels.clamp(1, MAX_LEVELS as u32),
            ..Default::default()
        }
    }

    /// The level whose fill starts next, when none is under way.
    pub fn wants(&self) -> Option<u32> {
        self.fill.is_none().then_some(self.turn)
    }

    /// Starts filling `level` at `place` with `casters` (and their triangles).
    pub fn start(&mut self, level: u32, place: Place, casters: Vec<(CasterRef, u64)>) {
        self.fill = Some(Fill {
            level,
            place,
            queue: casters,
            next: 0,
            started: false,
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
        let shown = self.front[fill.level as usize].map_or(1, |(_, l)| l);
        Some(Batch {
            level: fill.level,
            layer: 2 * fill.level + (1 - shown),
            place: fill.place,
            clear,
            casters,
            last: fill.next >= fill.queue.len(),
        })
    }

    /// After a batch is recorded: a fill whose last slice it was is shown and the next level's
    /// turn comes.
    pub fn recorded(&mut self, batch: &Batch) {
        if !batch.last {
            return;
        }
        self.front[batch.level as usize] = Some((batch.place, batch.layer % 2));
        self.fill = None;
        self.turn = (self.turn + 1) % self.levels;
    }

    /// Level `level`'s map shown on face `face`, and its texture layer.
    pub fn shown(&self, level: u32, face: FaceFrame) -> Option<(Place, u32)> {
        self.front
            .get(level as usize)
            .copied()
            .flatten()
            .filter(|(p, _)| p.face == face)
            .map(|(p, l)| (p, 2 * level + l))
    }

    pub fn levels(&self) -> u32 {
        self.levels
    }
}

/// Floats in the uniform block (`LodShadows` in `LOD_SHADOW_WGSL`).
pub const UNIFORM_FLOATS: usize = (MAX_LEVELS + 6) * 4;

/// The uniform block: per level its shown map's centre relative to the eye (x, z), half-width and
/// layer (−1: none); the heights' top and range, the viewer's face and the map's image of world z;
/// the light and its strength (0: no distant shadows); the march; the soft edge, the edge fade,
/// the maps' texels a side and the normal offset; the mips.
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
        if let Some((p, layer)) = schedule.shown(l, face) {
            u[l as usize * 4..l as usize * 4 + 4].copy_from_slice(&[
                (p.centre[0] - eye[0]) as f32,
                (p.centre[1] - eye[2]) as f32,
                p.half as f32,
                layer as f32,
            ]);
        }
    }
    let at = MAX_LEVELS * 4;
    let m = &t.march;
    u[at..at + 24].copy_from_slice(&[
        t.heights.top_m as f32,
        (t.heights.top_m - t.heights.bottom_m) as f32,
        face.sign_f32(),
        map_up(face)[2] as f32,
        caster.dir[0] as f32,
        caster.dir[1] as f32,
        caster.dir[2] as f32,
        caster.strength as f32,
        tier.steps as f32,
        m.start_texels as f32,
        tier.reach_m as f32,
        m.bias_m as f32,
        (1.0 / m.penumbra_deg.to_radians().tan()) as f32,
        m.edge_fade as f32,
        tier.resolution as f32,
        m.normal_offset_texels as f32,
        max_mips(tier.resolution) as f32,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ]);
    u
}

/// The frame group's distant-shadow entries (bindings 11–13: the uniform block, the maps, a
/// nearest sampler for their gathers).
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
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
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

/// The maps on the GPU: a depth array of `2 × levels` layers with their mips (a 1×1 single layer
/// while there are none, so the frame group always binds one), the views the fills and the mip
/// passes draw into, the mip passes' sources, the sampler and the uniform block, and the fills'
/// frame uniform and group (the casters' pass layout).
pub struct LodShadowMaps {
    /// (resolution, layers, mips) as made.
    pub size: (u32, u32, u32),
    texture: wgpu::Texture,
    pub array_view: wgpu::TextureView,
    /// Per layer, per mip: the view drawn into.
    targets: Vec<Vec<wgpu::TextureView>>,
    /// Per layer, per mip from 1: the group reading the mip before.
    sources: Vec<Vec<wgpu::BindGroup>>,
    mip_pipeline: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,
    pub uniform: wgpu::Buffer,
    pub frame: wgpu::Buffer,
    pub frame_group: wgpu::BindGroup,
}

impl LodShadowMaps {
    /// Maps for `tier` (none: the 1×1 stand-in), with the fills' frame group (`pass_layout`: the
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
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lod shadow heights"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: layers,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HEIGHT_DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("lod shadow heights"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let view = |layer: u32, mip: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("lod shadow height level"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let targets: Vec<Vec<wgpu::TextureView>> = (0..layers)
            .map(|l| (0..mips).map(|m| view(l, m)).collect())
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lod shadow mip"),
            source: wgpu::ShaderSource::Wgsl(MIP_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lod shadow mip"),
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mip_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lod shadow mip"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("mip_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("mip_fs"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: HEIGHT_DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lod shadow heights"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
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
            texture,
            array_view,
            targets,
            sources,
            mip_pipeline,
            sampler,
            uniform,
            frame,
            frame_group,
        }
    }

    /// The frame group's entries for the maps (bindings 11–13).
    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 11,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(&self.array_view),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ]
    }

    /// The view a fill of `layer` draws into (its level 0).
    pub fn target(&self, layer: u32) -> &wgpu::TextureView {
        &self.targets[layer as usize][0]
    }

    /// Records the max mips of `layer`, each from the one before.
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

    /// The maps' GPU bytes (their mips about a third more).
    pub fn bytes(&self) -> u64 {
        let (r, l, _) = self.size;
        u64::from(r) * u64::from(r) * u64::from(l) * 4 * 4 / 3
    }
}

impl Drop for LodShadowMaps {
    fn drop(&mut self) {
        // wgpu's WebGPU backend frees nothing on drop.
        self.texture.destroy();
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

/// The march the scene's shaders share (group 0, bindings 11–13), after `COMMON`'s helpers and
/// before `SHADOW_WGSL` (whose `shadow_light` calls it). `lod_shadow(rel, n)` is the light the
/// distant terrain lets through to an eye-relative point with normal `n`, before the light's
/// strength: dwell-look `lod_shadows::visibility`'s copy (the same steps, levels and soft edge),
/// faded out toward the outermost map's edge.
pub const LOD_SHADOW_WGSL: &str = r"
struct LodShadows {
    // Per level: xy its map's centre relative to the eye (world x, z), z its half-width (m),
    // w its layer (−1: none).
    maps: array<vec4f, 2>,
    // x the heights' top (face-local m, depth 0), y their range (m), z the viewer's face (±1),
    // w the map's image of world z (±1: its v runs against or along z).
    heights: vec4f,
    // xyz toward the light (world), w its shadows' strength (0: none).
    light: vec4f,
    // x steps, y the first step (texels), z the reach (m), w the bias (m).
    march: vec4f,
    // x 1 / tan(the penumbra), y the edge fade (share of the half-width), z texels a side,
    // w the normal offset (texels).
    cfg: vec4f,
    // x the mips.
    mips: vec4f,
};
@group(0) @binding(11) var<uniform> lod_shadows: LodShadows;
@group(0) @binding(12) var lod_heights: texture_depth_2d_array;
@group(0) @binding(13) var lod_heights_sampler: sampler;

// The finest level whose map holds an eye-relative column (strictly inside its square), or −1.
fn lod_level(q: vec2f) -> i32 {
    for (var l = 0; l < 2; l++) {
        let m = lod_shadows.maps[l];
        if (m.w >= 0.0 && all(abs(q - m.xy) < vec2f(m.z))) {
            return l;
        }
    }
    return -1;
}

// The face-local height at an eye-relative column in level `l`'s map at mip `mip`: level 0
// bilinear between texel centres (one gather), a coarser mip the highest of its texel.
fn lod_height(l: i32, q: vec2f, mip: i32) -> f32 {
    let m = lod_shadows.maps[l];
    let d = (q - m.xy) / m.z;
    let uv = vec2f(0.5 + 0.5 * d.x, 0.5 - 0.5 * d.y * lod_shadows.heights.w);
    let res = lod_shadows.cfg.z;
    var depth: f32;
    if (mip == 0) {
        let t = uv * res - 0.5;
        let base = floor(t);
        let f = t - base;
        let g = textureGather(lod_heights, lod_heights_sampler, (base + 1.0) / res, i32(m.w));
        depth = mix(mix(g.w, g.z, f.x), mix(g.x, g.y, f.x), f.y);
    } else {
        let side = i32(res) >> u32(mip);
        let c = clamp(vec2i(floor(uv * f32(side))), vec2i(0), vec2i(side - 1));
        depth = textureLoad(lod_heights, c, i32(m.w), mip);
    }
    return lod_shadows.heights.x - depth * lod_shadows.heights.y;
}

// The light the distant terrain lets through to an eye-relative point with normal `n` (world),
// 0–1, before the light's strength; 1 where no map holds it or on the other face.
fn lod_shadow(rel: vec3f, n: vec3f) -> f32 {
    let face = face_at(frame.eye.x + rel.y);
    if (lod_shadows.light.w <= 0.0 || face != lod_shadows.heights.z) {
        return 1.0;
    }
    let home = lod_level(rel.xz);
    if (home < 0) {
        return 1.0;
    }
    let l = face_vector(face, lod_shadows.light.xyz);
    let horizontal = length(l.xz);
    if (l.y <= 0.0 || horizontal < 1e-4) {
        return 1.0;
    }
    let dir = l.xz / horizontal;
    let rise = l.y / horizontal;
    let res = lod_shadows.cfg.z;
    let texel = 2.0 * lod_shadows.maps[home].z / res;
    let nl = face_vector(face, n);
    let o = vec3f(rel.x, face_y(face, frame.eye.x + rel.y), rel.z) + nl * texel * lod_shadows.cfg.w;
    let h0 = o.y + lod_shadows.march.w;
    let start = texel * lod_shadows.march.y;
    let steps = i32(lod_shadows.march.x);
    let growth = pow(lod_shadows.march.z / start, 1.0 / f32(max(steps, 2) - 1));
    let mips = i32(lod_shadows.mips.x);
    var least = 1.0;
    var s = start;
    for (var i = 0; i < steps; i++) {
        let q = o.xz + dir * s;
        let lv = lod_level(q);
        if (lv >= 0) {
            let tl = 2.0 * lod_shadows.maps[lv].z / res;
            let mip = min(i32(round(log2(max(s * (growth - 1.0) / tl, 1.0)))), mips - 1);
            let h = lod_height(lv, q, mip);
            least = min(least, (h0 + s * rise - h) * lod_shadows.cfg.x / s);
        }
        if (least <= -1.0) {
            break;
        }
        s *= growth;
    }
    let v = smoothstep(-1.0, 1.0, least);
    // Faded out toward the outermost map's edge.
    var outer = home;
    if (lod_shadows.maps[1].w >= 0.0) {
        outer = 1;
    }
    let m = lod_shadows.maps[outer];
    let off = max(abs(rel.x - m.x), abs(rel.z - m.y));
    let edge = 1.0 - smoothstep(m.z * (1.0 - lod_shadows.cfg.y), m.z, off);
    return mix(1.0, v, edge);
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::lod_shadows::LOD_SHADOWS;

    const T: LodShadowTunables = LOD_SHADOWS;

    fn tier() -> LodShadowTier {
        T.tiers[1]
    }

    fn apply(m: &Mat4, p: V3) -> [f64; 3] {
        std::array::from_fn(|r| {
            (0..3).map(|c| f64::from(m[c][r]) * p[c]).sum::<f64>() + f64::from(m[3][r])
        })
    }

    dwell_world::both_faces!(
        fn the_map_puts_a_column_where_the_shader_reads_it_and_the_highest_nearest(f: FaceFrame) {
            let t = tier();
            let eye = f.point([1234.5, 300.0, -876.25]);
            let p = place(&t, 0, eye, f);
            let m = height_matrix(&T, &p, eye);
            // A world point, as the fill draws it (eye-relative) and as the shader reads it.
            let world = f.point([1300.0, 120.0, -800.0]);
            let rel = [world[0] - eye[0], world[1] - eye[1], world[2] - eye[2]];
            let clip = apply(&m, rel);
            let up = map_up(f);
            let d = [(world[0] - p.centre[0]) / p.half, (world[2] - p.centre[1]) / p.half];
            assert!((clip[0] - d[0]).abs() < 1e-5);
            assert!((clip[1] - d[1] * up[2]).abs() < 1e-5);
            // Depth decodes to the face-local height (the shader's `top − depth × range`).
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
            // Its axes (right, up, back toward the face's up) are right-handed, so faces keep
            // their winding: the view is a rotation, never a mirror.
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

    fn at(f: FaceFrame) -> Place {
        place(&tier(), 0, [0.0, 50.0, 0.0], f)
    }

    #[test]
    fn a_fill_is_spread_over_frames_within_its_budget_and_shown_when_whole() {
        for f in FaceFrame::under_test() {
            let mut s = Schedule::new(2);
            assert_eq!(s.wants(), Some(0));
            s.start(0, at(f), casters(100, 10_000));
            let mut seen = Vec::new();
            let mut frames = 0;
            loop {
                let b = s.batch(35.0, 1000).expect("a fill");
                assert_eq!(b.clear, frames == 0);
                assert_eq!(b.layer, 0, "the first fill: the first layer");
                // 35k triangles a frame: three 10k casters.
                assert!(b.casters.len() <= 3);
                assert!(s.shown(0, f).is_none(), "nothing shown before the fill is whole");
                seen.extend(b.casters.clone());
                s.recorded(&b);
                frames += 1;
                if b.last {
                    break;
                }
            }
            assert_eq!(frames, 34);
            assert_eq!(seen, casters(100, 0).into_iter().map(|c| c.0).collect::<Vec<_>>());
            assert_eq!(s.shown(0, f), Some((at(f), 0)));
            // The other level's turn; then level 0 again, into the other layer.
            assert_eq!(s.wants(), Some(1));
            s.start(1, at(f), casters(1, 10));
            let b = s.batch(35.0, 1000).expect("a fill");
            assert_eq!(b.layer, 2);
            s.recorded(&b);
            assert_eq!(s.wants(), Some(0));
            s.start(0, at(f), casters(2, 10));
            let b = s.batch(35.0, 1000).expect("a fill");
            assert_eq!(b.layer, 1, "the layer not shown");
            assert!(b.last);
            // While it fills, the old map stays shown.
            assert_eq!(s.shown(0, f), Some((at(f), 0)));
            s.recorded(&b);
            assert_eq!(s.shown(0, f), Some((at(f), 1)));
            // A map is the face's it was made on.
            assert!(s.shown(0, f.other()).is_none());
        }
    }

    #[test]
    fn a_slice_keeps_to_the_draws_and_an_oversized_caster_still_goes() {
        let f = FaceFrame::A; // face-a-only: the schedule names no face but the place's.
        let mut s = Schedule::new(1);
        s.start(0, at(f), casters(10, 1_000_000));
        let b = s.batch(35.0, 1000).expect("a fill");
        assert_eq!(b.casters.len(), 1);
        let mut s = Schedule::new(1);
        s.start(0, at(f), casters(10, 1));
        let b = s.batch(1000.0, 4).expect("a fill");
        assert_eq!(b.casters.len(), 4);
        // An empty fill clears its layer and is shown at once.
        let mut s = Schedule::new(1);
        s.start(0, at(f), Vec::new());
        let b = s.batch(1.0, 1).expect("a fill");
        assert!(b.clear && b.last && b.casters.is_empty());
        s.recorded(&b);
        assert!(s.shown(0, f).is_some());
    }

    #[test]
    fn the_uniforms_carry_the_shown_maps_and_nothing_without_a_light() {
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
            let p = place(&t, 0, eye, f);
            s.start(0, p, Vec::new());
            let b = s.batch(1.0, 1).expect("a fill");
            s.recorded(&b);
            let u = uniforms(&T, Some(&t), &s, eye, f, Some(&caster));
            assert_eq!(u[3], 0.0);
            assert_eq!(u[2], p.half as f32);
            assert_eq!(u[0], (p.centre[0] - eye[0]) as f32);
            assert_eq!(u[10], f.sign_f32());
            assert_eq!(u[15], 0.75);
            assert_eq!(u[24], max_mips(t.resolution) as f32);
            let u = uniforms(&T, Some(&t), &s, eye, f, None);
            assert_eq!(u[15], 0.0);
            let u = uniforms(&T, None, &s, eye, f, Some(&caster));
            assert_eq!(u[15], 0.0);
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
    fn the_shader_march_is_the_mirrors() {
        // dwell-look `lod_shadows::visibility` is the march in f64; these are its lines here.
        let w = LOD_SHADOW_WGSL;
        for line in [
            "let o = vec3f(rel.x, face_y(face, frame.eye.x + rel.y), rel.z) + nl * texel * lod_shadows.cfg.w;",
            "let h0 = o.y + lod_shadows.march.w;",
            "let start = texel * lod_shadows.march.y;",
            "let growth = pow(lod_shadows.march.z / start, 1.0 / f32(max(steps, 2) - 1));",
            "let mip = min(i32(round(log2(max(s * (growth - 1.0) / tl, 1.0)))), mips - 1);",
            "least = min(least, (h0 + s * rise - h) * lod_shadows.cfg.x / s);",
            "if (least <= -1.0) {",
            "let v = smoothstep(-1.0, 1.0, least);",
            "let edge = 1.0 - smoothstep(m.z * (1.0 - lod_shadows.cfg.y), m.z, off);",
        ] {
            assert!(w.contains(line), "{line}");
        }
        // Face-local through the face frame's helpers only.
        assert!(w.contains("face_vector(face, lod_shadows.light.xyz)"));
        assert!(w.contains("face_vector(face, n)"));
    }
}
