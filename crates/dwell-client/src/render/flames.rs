//! Flames in the renderer (docs/LIGHTS.md §6): where a light source with a `flame` burns — a placed
//! torch's top (`placed_flames`), other players' held torches and your own in the arms view — two
//! camera-facing billboards per flame, built on the CPU each frame (`quads`): a glow (and embers)
//! added behind it, and the flame itself, premultiplied. dwell-look `flames` holds the looks, the
//! billboard's axes and the shading's CPU twin; this module builds the quads and draws them.
//!
//! They draw in the near scene pass after its opaque draws, depth-tested against the depth buffer
//! and writing none: first a mask (`draw_mask`) that writes only the view depth, flagged self-lit
//! and uninked where the flame covers enough (`self_lit_depth`, so the time of day's desaturation
//! leaves the flame alone and the ink draws no outline round it), then the colour (`draw_colour`),
//! after the see-through water where the pass has it. Your own torch's flame draws in the arms
//! pass. A CPU adapter draws a still flame (no noise, no embers).
//!
//! Cost: at most `FLAME_TUNABLES.max_flames` flames, 12 vertices each, written once a frame; the
//! fragments are the flames' few pixels.

use std::ops::Range;

use dwell_look::flames::{
    FLAME_TUNABLES, FLAMES, FlameLook, QUAD_HALF_WIDTH, billboard, find, glow_extent, height_factor,
};
use dwell_world::FaceFrame;

use super::camera::Basis;
use super::characters::HeldLight;
use super::frame::{DEPTH, HDR, VIEW_DEPTH};

/// A placed light source's flame this frame: its base (world), its face, look and flicker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedFlame {
    pub base: [f64; 3],
    pub face: FaceFrame,
    pub source: &'static dwell_blocks::LightInfo,
    pub seed: u32,
}

/// A placed light source's flicker phase: its cell's (the placed light's own seed, so a placed
/// torch's light and flame pulse together).
pub fn cell_seed(c: [i32; 3]) -> u32 {
    (c[0] as u32).wrapping_mul(0x9e37_79b1)
        ^ (c[1] as u32).wrapping_mul(0x85eb_ca77)
        ^ (c[2] as u32).wrapping_mul(0xc2b2_ae3d)
}

/// The placed light sources' flames within `FLAME_TUNABLES.draw_m` of `eye`
/// (`VoxelWorld::light_sources_near`), nearest first: each one's base `base_m` over the floor of
/// the cell that gives off the light, on that cell's face.
pub fn placed_flames(world: &dwell_world::VoxelWorld, eye: [f64; 3]) -> Vec<PlacedFlame> {
    world
        .light_sources_near(eye, FLAME_TUNABLES.draw_m)
        .into_iter()
        .filter_map(|(c, m)| {
            let source = dwell_blocks::try_info(m)?.light?;
            let look = &FLAMES[find(source.flame?)?];
            let face = FaceFrame::at_row(c[1]);
            let lo = dwell_world::pos_of_cell(c);
            let up = face.up();
            let rise = f64::from(look.base_m) - 0.5 * dwell_world::CELL[1];
            let base = std::array::from_fn(|k| lo[k] + 0.5 * dwell_world::CELL[k] + up[k] * rise);
            Some(PlacedFlame {
                base,
                face,
                source,
                seed: cell_seed(c),
            })
        })
        .take(FLAME_TUNABLES.max_flames)
        .collect()
}

/// One flame to draw: its base relative to the eye, face, look, flicker source and phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flame {
    pub rel: [f64; 3],
    pub face: FaceFrame,
    pub look: usize,
    pub source: &'static dwell_blocks::LightInfo,
    pub seed: u32,
}

impl Flame {
    /// A held light's flame (its item's light point the base), on the face it is over; none for a
    /// light without a flame.
    pub fn held(h: &HeldLight, eye: [f64; 3]) -> Option<Self> {
        let rel = h.rel.map(f64::from);
        Some(Self {
            rel,
            face: FaceFrame::at_height(eye[1] + rel[1]),
            look: find(h.source.flame?)?,
            source: h.source,
            seed: h.seed,
        })
    }

    /// A placed flame relative to `eye`.
    pub fn placed(p: &PlacedFlame, eye: [f64; 3]) -> Option<Self> {
        Some(Self {
            rel: std::array::from_fn(|k| p.base[k] - eye[k]),
            face: p.face,
            look: find(p.source.flame?)?,
            source: p.source,
            seed: p.seed,
        })
    }
}

/// A vertex: its place relative to the eye, its quad coordinates (the flame's: −1 to 1 across, 0 to
/// 1 up; the glow's: metres from the base), the time (s), the light's flicker factor, the flame's
/// height (m), the look | kind << 8 (0 flame, 1 glow) and the flame's seed.
pub const VERTEX_FLOATS: usize = 10;
const VERTEX_BYTES: u64 = (VERTEX_FLOATS * 4) as u64;
const KIND_GLOW: u32 = 1;

/// The time the shader animates with: seconds, wrapped (the noise is not periodic, so it jumps
/// once every wrap; f32 keeps its precision).
fn wrapped(seconds: f64) -> f32 {
    seconds.rem_euclid(4096.0) as f32
}

/// One quad's six vertices: `corner(x, y)` its place for x, y in {0, 1}; `uv(x, y)` its quad
/// coordinates there.
fn quad(
    out: &mut Vec<f32>,
    corner: impl Fn(f64, f64) -> [f64; 3],
    uv: impl Fn(f64, f64) -> [f32; 2],
    rest: [f32; 3],
    info: u32,
    seed: u32,
) {
    for (x, y) in [
        (0.0, 0.0),
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
    ] {
        let p = corner(x, y);
        let t = uv(x, y);
        out.extend([p[0] as f32, p[1] as f32, p[2] as f32, t[0], t[1]]);
        out.extend(rest);
        out.push(f32::from_bits(info));
        out.push(f32::from_bits(seed));
    }
}

/// The flames' quads at `seconds`, seen with `basis`, farthest first: their glows (`glows`, the
/// vertices' first part) then the flames themselves, so a flame covers its glow and a nearer flame
/// a farther one.
pub fn quads(flames: &[Flame], basis: &Basis, seconds: f64) -> (Vec<f32>, usize) {
    let mut order: Vec<&Flame> = flames.iter().collect();
    let d2 = |f: &Flame| f.rel.iter().map(|v| v * v).sum::<f64>();
    order.sort_by(|a, b| d2(b).total_cmp(&d2(a)).then(a.seed.cmp(&b.seed)));
    let t = wrapped(seconds);
    let mut glows = Vec::new();
    let mut bodies = Vec::new();
    for f in order {
        let look: &FlameLook = &FLAMES[f.look];
        let flicker = dwell_look::lights::flicker(
            seconds,
            f.seed,
            f64::from(f.source.flicker),
            f64::from(f.source.flicker_speed),
        );
        let height = look.height_m * height_factor(look, flicker);
        let b = billboard(f.face, basis.forward(), basis.up);
        let at = |x: f64, y: f64| -> [f64; 3] {
            std::array::from_fn(|k| f.rel[k] + b.right[k] * x + b.up[k] * y)
        };
        let rest = [t, flicker as f32, height];
        let (half, below, above) = glow_extent(look, height);
        let (half, below, above) = (f64::from(half), f64::from(below), f64::from(above));
        quad(
            &mut glows,
            |x, y| at((2.0 * x - 1.0) * half, -below + y * (below + above)),
            |x, y| {
                [
                    ((2.0 * x - 1.0) * half) as f32,
                    (-below + y * (below + above)) as f32,
                ]
            },
            rest,
            f.look as u32 | KIND_GLOW << 8,
            f.seed,
        );
        let w = f64::from(QUAD_HALF_WIDTH * look.width_m);
        quad(
            &mut bodies,
            |x, y| at((2.0 * x - 1.0) * w, y * f64::from(height)),
            |x, y| [(2.0 * x - 1.0) as f32, y as f32],
            rest,
            f.look as u32,
            f.seed,
        );
    }
    let n = glows.len() / VERTEX_FLOATS;
    glows.extend(bodies);
    (glows, n)
}

/// The flames' shader: the frame's lighting (`glow`, `frame_depth`, the frame uniforms) and
/// dwell-look's flame shading.
pub fn source(animated: bool) -> String {
    format!(
        "{}\n{}\n{WGSL}",
        super::shaders::lighting_source(),
        dwell_look::flames::wgsl(animated)
    )
}

const WGSL: &str = r"
struct FlameIn {
    @location(0) rel: vec3f,
    @location(1) uv: vec2f,
    @location(2) rest: vec3f,
    @location(3) info: u32,
    @location(4) seed: u32,
};

struct FlameVary {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) uv: vec2f,
    // x the time (s), y the light's flicker factor, z the flame's height (m)
    @location(2) rest: vec3f,
    @location(3) @interpolate(flat) info: u32,
    @location(4) @interpolate(flat) seed: u32,
};

@vertex
fn flame_vs(v: FlameIn) -> FlameVary {
    var o: FlameVary;
    o.clip = frame.view_proj * vec4f(v.rel, 1.0);
    o.rel = v.rel;
    o.uv = v.uv;
    o.rest = v.rest;
    o.info = v.info;
    o.seed = v.seed;
    return o;
}

// The flame's or the glow's colour, premultiplied: the flame over what is behind it, the glow
// added (alpha 0). Both give off their colour whatever the light (`glow`, docs/LIGHTS.md §2).
fn flame_colour(i: FlameVary) -> vec4f {
    let look = flame_look(i.info & 0xffu);
    if ((i.info >> 8u) == 1u) {
        let c = flame_glow(look, i.uv, i.rest.z, i.rest.y, i.rest.x, i.seed);
        if (max(max(c.r, c.g), c.b) < 1e-4) {
            discard;
        }
        return vec4f(glow(c), 0.0);
    }
    let s = flame_shade(look, i.uv.x, i.uv.y, i.rest.x, i.seed);
    if (s.a < 0.004) {
        discard;
    }
    return vec4f(glow(s.rgb) * s.a, s.a);
}

struct FlameOut {
    @location(0) color: vec4f,
};

// The colour into a pass with the colour target alone (the see-through water's half).
@fragment
fn flame_fs(i: FlameVary) -> FlameOut {
    return FlameOut(flame_colour(i));
}

// The colour into a scene pass (the view depth's target is not written: its mask is).
@fragment
fn flame_scene_fs(i: FlameVary) -> SceneOut {
    var o: SceneOut;
    o.color = flame_colour(i);
    o.depth = 0.0;
    return o;
}

// The mask: where the flame covers enough, the view depth at the flame, flagged self-lit (the time
// of day's desaturation leaves it alone) and uninked (no outline round it). Its colour target is
// not written.
@fragment
fn flame_mask_fs(i: FlameVary) -> SceneOut {
    let look = flame_look(i.info & 0xffu);
    let s = flame_shade(look, i.uv.x, i.uv.y, i.rest.x, i.seed);
    if (s.a < 0.35) {
        discard;
    }
    var o: SceneOut;
    o.color = vec4f(0.0);
    o.depth = self_lit_depth(frame_depth(view_depth(i.rel), false), true);
    return o;
}
";

/// Which flames a draw is for: the scene's (placed and other players') or your arms'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Scene,
    Arms,
}

/// The flames' pipelines and this frame's vertices.
pub struct FlameRenderer {
    mask: wgpu::RenderPipeline,
    scene: wgpu::RenderPipeline,
    colour_only: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    capacity: u64,
    /// Per group, its glows' and flames' vertex ranges.
    ranges: [(Range<u32>, Range<u32>); 2],
    /// Flames drawn this frame (telemetry).
    pub drawn: usize,
}

const PREMULTIPLIED: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

impl FlameRenderer {
    pub fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        samples: u32,
        animated: bool,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("flames"),
            source: wgpu::ShaderSource::Wgsl(source(animated).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("flames"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 20,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 32,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 36,
                shader_location: 4,
            },
        ];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let make = |label: &str, fs: &str, targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("flame_vs"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets,
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let colour = |write: bool| wgpu::ColorTargetState {
            format: HDR,
            blend: Some(PREMULTIPLIED),
            write_mask: if write {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        };
        let view_depth = |write: bool| wgpu::ColorTargetState {
            format: VIEW_DEPTH,
            blend: None,
            write_mask: if write {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        };
        let mask = make(
            "flame mask",
            "flame_mask_fs",
            &[Some(colour(false)), Some(view_depth(true))],
        );
        let scene = make(
            "flames",
            "flame_scene_fs",
            &[Some(colour(true)), Some(view_depth(false))],
        );
        let colour_only = make("flames (water)", "flame_fs", &[Some(colour(true))]);
        let capacity = 64;
        Self {
            mask,
            scene,
            colour_only,
            vertices: vertex_buffer(device, capacity),
            capacity,
            ranges: [(0..0, 0..0), (0..0, 0..0)],
            drawn: 0,
        }
    }

    /// This frame's flames: the scene's (`scene`) and your arms' (`arms`), at `seconds`, seen with
    /// `basis`.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        basis: &Basis,
        scene: &[Flame],
        arms: Option<Flame>,
        seconds: f64,
    ) {
        let mut scene: Vec<Flame> = scene.to_vec();
        let d2 = |f: &Flame| f.rel.iter().map(|v| v * v).sum::<f64>();
        scene.retain(|f| d2(f) <= FLAME_TUNABLES.draw_m * FLAME_TUNABLES.draw_m);
        scene.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
        scene.truncate(FLAME_TUNABLES.max_flames);
        let (mut data, scene_glows) = quads(&scene, basis, seconds);
        let scene_end = (data.len() / VERTEX_FLOATS) as u32;
        let (arms_data, arms_glows) = quads(arms.as_slice(), basis, seconds);
        data.extend(arms_data);
        let end = (data.len() / VERTEX_FLOATS) as u32;
        let g = scene_glows as u32;
        let a = scene_end + arms_glows as u32;
        self.ranges = [(0..g, g..scene_end), (scene_end..a, a..end)];
        self.drawn = scene.len() + usize::from(arms.is_some());
        let count = u64::from(end);
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.vertices.destroy();
            self.vertices = vertex_buffer(device, self.capacity);
        }
        if !data.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&data));
        }
    }

    /// Whether a group has flames this frame.
    pub fn has(&self, group: Group) -> bool {
        !self.ranges[group as usize].1.is_empty()
    }

    /// The mask (into a scene pass, after its opaque draws): the view depth flagged self-lit
    /// where the flames cover.
    pub fn draw_mask(&self, pass: &mut wgpu::RenderPass<'_>, group: Group, calls: &mut u32) {
        let (_, bodies) = self.ranges[group as usize].clone();
        if bodies.is_empty() {
            return;
        }
        pass.set_pipeline(&self.mask);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(bodies, 0..1);
        *calls += 1;
    }

    /// The colour: the glows then the flames, into a scene pass (`scene`) or the see-through water's
    /// half (colour target only).
    pub fn draw_colour(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        group: Group,
        scene: bool,
        calls: &mut u32,
    ) {
        let (glows, bodies) = self.ranges[group as usize].clone();
        if bodies.is_empty() {
            return;
        }
        pass.set_pipeline(if scene {
            &self.scene
        } else {
            &self.colour_only
        });
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(glows.start..bodies.end, 0..1);
        *calls += 1;
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("flame vertices"),
        size: capacity * VERTEX_BYTES,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::flames::flame_shade;

    fn torch() -> &'static dwell_blocks::LightInfo {
        dwell_blocks::light("dwell:torch").expect("the torch light")
    }

    #[test]
    fn the_shader_validates_in_both_tiers() {
        for animated in [true, false] {
            let module = naga::front::wgsl::parse_str(&source(animated)).expect("WGSL parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("WGSL validates");
        }
    }

    #[test]
    fn the_mask_flags_the_flame_self_lit_and_uninked() {
        // docs/LIGHTS.md §2, §6: the flame's pixels are flagged in the view depth, so the time of
        // day's desaturation leaves them alone (the composite's `scene_self_lit_at`) and the ink
        // draws no outline round them; the colour passes never write the view depth.
        let mask = WGSL
            .split("fn flame_mask_fs")
            .nth(1)
            .expect("the mask entry");
        assert!(
            mask.contains("o.depth = self_lit_depth(frame_depth(view_depth(i.rel), false), true);")
        );
        let scene = WGSL
            .split("fn flame_scene_fs")
            .nth(1)
            .expect("the scene entry");
        let scene = scene.split("@fragment").next().unwrap();
        assert!(!scene.contains("self_lit_depth"));
    }

    /// The hue (degrees) and saturation (HSV) of a display colour (0–1, sRGB-encoded).
    fn hue_sat(c: [f32; 3]) -> (f64, f64) {
        let [r, g, b] = c.map(f64::from);
        let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
        let d = hi - lo;
        let h = if d <= 0.0 {
            0.0
        } else if hi == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if hi == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h, if hi > 0.0 { d / hi } else { 0.0 })
    }

    #[test]
    fn the_flame_stays_lit_and_warm_by_day_at_night_and_in_a_cave() {
        // Owner (2026-10-21): "the flame should stay illuminated regardless of brightness since
        // it's the source of the light". The flame's colours (its body, mid-height, through the
        // CPU twin) through `glow`, the tone curve and the time of day's saturation, which a
        // flagged pixel skips (render/lights.rs's mirrors): by day, at midnight and at a bright
        // exposure the same warm, saturated colour, never cream.
        use super::super::lights::{displayed, glow};
        let look = &FLAMES[find("dwell:torch").expect("the torch's flame")];
        let sample = |solar: f64| {
            let s = dwell_look::daynight::DAY_NIGHT.sample(solar);
            (
                (dwell_look::DEFAULT_EXPOSURE * s.exposure) as f32,
                s.saturation as f32,
            )
        };
        let (day, day_sat) = sample(60.0);
        let (night, night_sat) = sample(-90.0);
        assert!(
            night_sat < 1.0,
            "moonlight is desaturated (the case that washed it to cream)"
        );
        for (x, y) in [(0.0, 0.12), (0.35, 0.3), (0.0, 0.5)] {
            let (c, a) = flame_shade(look, x, y, 0.0, 7, true);
            assert!(a > 0.9, "({x}, {y}) is in the flame");
            let show = |e: f32, s: f32| displayed(glow(c, e), e, s, true);
            let base = show(day, day_sat);
            for (when, e, s) in [
                ("noon", day, day_sat),
                ("midnight", night, night_sat),
                ("bright", 2.5, 1.0),
            ] {
                let shown = show(e, s);
                let (h, sat) = hue_sat(shown);
                assert!(
                    (5.0..60.0).contains(&h),
                    "({x}, {y}) {when}: hue {h:.0}° ({shown:?})"
                );
                assert!(
                    sat >= 0.3,
                    "({x}, {y}) {when}: saturation {sat:.2} ({shown:?})"
                );
                for k in 0..3 {
                    assert!(
                        (shown[k] - base[k]).abs() < 0.02,
                        "({x}, {y}) {when}: {shown:?} vs noon {base:?}"
                    );
                }
            }
        }
    }

    fn basis_of(face: FaceFrame, yaw: f64, pitch: f64) -> Basis {
        super::super::camera::aim(yaw, pitch, face, 0.0)
    }

    dwell_world::both_faces!(
        fn a_flames_quads_rise_along_its_faces_up_and_face_the_eye(f: FaceFrame) {
            let basis = basis_of(f, 30.0, -20.0);
            let flame = Flame {
                rel: f.vector([0.0, -0.5, -2.0]),
                face: f,
                look: find("dwell:torch").unwrap(),
                source: torch(),
                seed: 3,
            };
            let (v, glows) = quads(&[flame], &basis, 1.0);
            assert_eq!(v.len() / VERTEX_FLOATS, 12, "two quads");
            assert_eq!(glows, 6);
            let at = |i: usize| -> [f64; 3] {
                std::array::from_fn(|k| f64::from(v[i * VERTEX_FLOATS + k]))
            };
            // The flame quad (vertices 6..12): its top corners (uv.y 1) over its base along the
            // face's up, and every corner on the plane facing the view.
            let up = f.up();
            let fwd = basis.forward();
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            for i in 6..12 {
                let uv_y = v[i * VERTEX_FLOATS + 4];
                let p = at(i);
                let d: [f64; 3] = std::array::from_fn(|k| p[k] - flame.rel[k]);
                if uv_y > 0.5 {
                    assert!(dot(d, up) > 0.2, "the top rises along the face's up: {d:?}");
                } else {
                    assert!(dot(d, up).abs() < 1e-5);
                }
                assert!(dot(d, fwd).abs() < 1e-5, "on the view plane");
            }
        }
    );

    dwell_world::both_faces!(
        fn a_placed_torch_burns_on_its_stick(f: FaceFrame) {
            // A placed torch (two cells): the flame's base `base_m` over its upper cell's floor,
            // face-locally, and none past the draw distance.
            let mut w = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            w.enable_light();
            let ground = f.cell([3, 30, 3]);
            w.set_voxel(
                ground[0],
                ground[1],
                ground[2],
                dwell_blocks::require("dwell:stone"),
            );
            let lower = f.cell([3, 31, 3]);
            let upper = f.cell([3, 32, 3]);
            w.set_voxel(
                lower[0],
                lower[1],
                lower[2],
                dwell_blocks::require("dwell:torch"),
            );
            w.set_voxel(
                upper[0],
                upper[1],
                upper[2],
                dwell_blocks::require("dwell:torch_upper"),
            );
            w.light_edited(&[ground, lower, upper]);
            // pos_of_cell is the cell's world corner; face-locally its floor is the lower end.
            let y0 = dwell_world::pos_of_cell(upper)[1];
            let cell_floor = f.y(y0).min(f.y(y0 + dwell_world::CELL[1]));
            let eye = f.point([3.5, cell_floor + 1.6, 6.0]);
            let flames = placed_flames(&w, eye);
            assert_eq!(flames.len(), 1, "the upper cell burns");
            let look = &FLAMES[find("dwell:torch").unwrap()];
            let base = f.point(flames[0].base);
            assert!((base[0] - 3.5).abs() < 1e-9 && (base[2] - 3.5).abs() < 1e-9);
            assert!(
                (base[1] - cell_floor - f64::from(look.base_m)).abs() < 1e-6,
                "{base:?} over {cell_floor}"
            );
            assert_eq!(flames[0].face, f);
            assert_eq!(flames[0].seed, cell_seed(upper));
            let far = f.point([3.5, cell_floor, 3.5 + FLAME_TUNABLES.draw_m + 4.0]);
            assert!(placed_flames(&w, far).is_empty());
        }
    );
}
