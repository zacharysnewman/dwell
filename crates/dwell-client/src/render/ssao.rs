//! Screen-space ambient occlusion (docs/SSAO.md, ADR 0080; owner 2026-11-15: "I'd like to implement
//! screen space ambient occlusion"; `shared/look/ssao.json`, `dwell_look::ssao`). Soft contact
//! shadows where things meet, on the hemisphere and block light only, from the scene's view depth
//! once the scene passes are done and before anything reads their colour:
//!
//! 1. **Occlusion** (`ssao_fs`, at the tier's resolution): each texel's view-space point from its
//!    pixel's view depth (sample 0), its normal from the neighbours' depths (the nearer side each
//!    axis, so an edge doesn't bend it), and a spiral of samples within the data's radius in metres
//!    (at most `maxPixels` across), each occluding by how far above the texel's plane it is and how
//!    near; faded in past your arms and out with distance, scaled by the strength.
//! 2. **Blur** (`ssao_blur_h_fs`, `ssao_blur_v_fs`): across, then down, each tap weighted by how
//!    near its depth is to the texel's, so the occlusion doesn't bleed over edges.
//! 3. **Apply** (`ssao_apply_fs`, at the drawing buffer's size): the blurred occlusion upsampled by
//!    depth (four texels), and the scene's resolved colour multiplied (blended) by
//!    1 − (1 − AO) × the pixel's share (`frame_ao_share`, written by the scene's lighting into its
//!    view depth: the hemisphere and block light's share, weakened by the baked corner shading; 0
//!    for water, the sky, self-lit surfaces and whatever never lights through `lambert_lit`).
//!
//! Its own shader module, one group layout and four pipelines, each made on first use
//! ([`pipeline::Lazy`]), so a tier that is off never builds them. Timed as one span
//! (`timing::SSAO`).

use dwell_look::ssao::{MAX_BLUR, MAX_SAMPLES, SSAO, SsaoQuality, SsaoTier};

use super::frame::HDR;
use super::pipeline;

/// The occlusion buffers' format: the occlusion (r) and the texel's view depth (g, for the
/// depth-aware blur and upsample).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The uniform's size: five vec4s (`Ssao` in the shader).
pub const UNIFORM_BYTES: u64 = 80;

/// The setting a frame draws with the Ambient occlusion setting `quality`: none on a CPU adapter
/// or the phone tier (frame rate there first).
pub fn ssao_for(device_type: wgpu::DeviceType, phone: bool, quality: SsaoQuality) -> SsaoQuality {
    if super::is_cpu(device_type) || phone {
        SsaoQuality::Off
    } else {
        quality
    }
}

/// The uniform for a drawing buffer of `scene` px with an occlusion buffer of `buffer` texels at
/// `tier`, a vertical FOV of `fov_deg` and the Settings strength `strength` (× the data's).
pub fn uniforms(
    scene: (u32, u32),
    buffer: (u32, u32),
    tier: SsaoTier,
    fov_deg: f64,
    strength: f64,
) -> [f32; 20] {
    let tan_y = (fov_deg.to_radians() / 2.0).tan();
    let tan_x = tan_y * f64::from(scene.0) / f64::from(scene.1.max(1));
    let l = SSAO;
    // The widest spiral in the buffer's texels.
    let max_texels = l.max_pixels * f64::from(buffer.1) / f64::from(scene.1.max(1));
    [
        tan_x as f32,
        tan_y as f32,
        (l.strength * strength.clamp(0.0, 4.0)) as f32,
        tier.samples.min(MAX_SAMPLES) as f32,
        buffer.0 as f32,
        buffer.1 as f32,
        scene.0 as f32,
        scene.1 as f32,
        l.radius as f32,
        l.intensity as f32,
        l.bias as f32,
        max_texels as f32,
        l.near_from as f32,
        l.near_to as f32,
        l.far_from as f32,
        l.far_to as f32,
        l.blur_sharpness as f32,
        l.blur_radius.min(MAX_BLUR) as f32,
        0.0,
        0.0,
    ]
}

/// The module for a scene of `samples` per pixel.
pub fn source(samples: u32) -> String {
    let depth = if samples > 1 {
        "texture_multisampled_2d<f32>"
    } else {
        "texture_2d<f32>"
    };
    format!(
        "const MAX_SAMPLES: i32 = {MAX_SAMPLES};\nconst MAX_BLUR: i32 = {MAX_BLUR};\n\
         @group(0) @binding(0) var scene_depth: {depth};\n{WGSL}"
    )
}

const WGSL: &str = r"
// x, y the half-FOV tangents, z the strength (data × setting), w the samples; the buffer's size
// (xy) and the drawing buffer's (zw); the radius (m), the intensity, the bias (m) and the widest
// spiral (texels); the near and far fades (m); the blur's sharpness and taps each way.
struct Ssao {
    tan: vec4f,
    size: vec4f,
    look: vec4f,
    fade: vec4f,
    blur: vec4f,
};
@group(0) @binding(1) var ao_in: texture_2d<f32>;
@group(0) @binding(2) var<uniform> ssao: Ssao;

struct Out {
    @builtin(position) clip: vec4f,
};

@vertex
fn ssao_vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

// The view depth (m; 0: nothing drawn) of the drawing buffer's pixel `p`, sample 0, its flags in it.
fn depth_at(p: vec2i) -> f32 {
    let s = vec2i(ssao.size.zw);
    return textureLoad(scene_depth, clamp(p, vec2i(0), s - vec2i(1)), 0).r;
}

// The view-space point (x right, y up, z ahead; m) at the drawing buffer's position `p` (px) and
// view depth `d`.
fn view_point(p: vec2f, d: f32) -> vec3f {
    let ndc = vec2f(p.x / ssao.size.z * 2.0 - 1.0, 1.0 - p.y / ssao.size.w * 2.0);
    return vec3f(ndc * ssao.tan.xy, 1.0) * d;
}

// The step to the neighbour `o` px away (and back from the one opposite), whichever is nearer in
// depth, so a silhouette doesn't bend the normal.
fn tangent(p: vec2i, d: f32, pt: vec3f, o: vec2i) -> vec3f {
    let a = depth_at(p + o);
    let b = depth_at(p - o);
    let c = vec2f(p) + 0.5;
    let use_a = a > 0.0 && (b <= 0.0 || abs(a - d) <= abs(b - d));
    if (use_a) {
        return view_point(c + vec2f(o), a) - pt;
    }
    if (b > 0.0) {
        return pt - view_point(c - vec2f(o), b);
    }
    return view_point(c + vec2f(o), d) - pt;
}

// The occlusion's share kept at view depth `d`: in past the arms, out with distance.
fn depth_fade(d: f32) -> f32 {
    return smoothstep(ssao.fade.x, max(ssao.fade.y, ssao.fade.x + 1e-4), d) * (1.0 - smoothstep(ssao.fade.z, ssao.fade.w, d));
}

const SPIRAL_TURNS: f32 = 7.0;
const TAU: f32 = 6.2831853;

@fragment
fn ssao_fs(i: Out) -> @location(0) vec4f {
    let t = floor(i.clip.xy);
    let k = ssao.size.zw / ssao.size.xy;
    let sp = vec2i(floor((t + 0.5) * k));
    let d = depth_at(sp);
    if (d <= 0.0) {
        return vec4f(1.0, 0.0, 0.0, 1.0);
    }
    let fade = depth_fade(d);
    let r = ssao.look.x;
    // The radius in the buffer's texels at this depth, capped.
    let r_px = min(r * ssao.size.y / (2.0 * ssao.tan.y * d), ssao.look.w);
    if (fade <= 0.0 || r_px < 1.0) {
        return vec4f(1.0, d, 0.0, 1.0);
    }
    let pt = view_point(vec2f(sp) + 0.5, d);
    var n = normalize(cross(tangent(sp, d, pt, vec2i(1, 0)), tangent(sp, d, pt, vec2i(0, 1))));
    if (dot(n, pt) > 0.0) {
        n = -n;
    }
    // Each texel turns the spiral by interleaved gradient noise; the blur averages it out.
    let rot = TAU * fract(52.9829189 * fract(dot(t, vec2f(0.06711056, 0.00583715))));
    let count = i32(ssao.tan.w);
    let r2 = r * r;
    var sum = 0.0;
    for (var s = 0; s < MAX_SAMPLES; s++) {
        if (s >= count) {
            break;
        }
        let a = (f32(s) + 0.5) / f32(count);
        let ang = a * SPIRAL_TURNS * TAU + rot;
        let q_px = vec2i(floor((t + 0.5 + vec2f(cos(ang), sin(ang)) * (a * r_px)) * k));
        let qd = depth_at(q_px);
        if (qd <= 0.0) {
            continue;
        }
        let v = view_point(vec2f(q_px) + 0.5, qd) - pt;
        let vv = dot(v, v);
        // Above the texel's plane (less the bias) by the cosine, nearer counting more; none past
        // the radius.
        let up = max(dot(v, n) - ssao.look.z, 0.0) / sqrt(vv + 1e-4);
        sum += up * max(1.0 - vv / r2, 0.0);
    }
    let ao = clamp(1.0 - ssao.look.y * 2.0 * sum / f32(max(count, 1)), 0.0, 1.0);
    return vec4f(1.0 - (1.0 - ao) * fade * ssao.tan.z, d, 0.0, 1.0);
}

// The blur along `dir` at texel `t`: Gaussian taps, each by how near its depth is to the texel's.
fn blurred(t: vec2i, dir: vec2i) -> vec4f {
    let c = textureLoad(ao_in, t, 0);
    if (c.g <= 0.0) {
        return c;
    }
    let size = vec2i(textureDimensions(ao_in));
    let taps = i32(ssao.blur.y);
    let sigma = f32(taps) * 0.5 + 0.5;
    let edge = max(c.g * ssao.blur.x, 1e-4);
    var sum = c.r;
    var weight = 1.0;
    for (var k = 1; k <= MAX_BLUR; k++) {
        if (k > taps) {
            break;
        }
        let g = exp(-f32(k * k) / (2.0 * sigma * sigma));
        for (var side = -1; side <= 1; side += 2) {
            let s = textureLoad(ao_in, clamp(t + dir * k * side, vec2i(0), size - vec2i(1)), 0);
            let w = g * max(1.0 - abs(s.g - c.g) / edge, 0.0) * select(0.0, 1.0, s.g > 0.0);
            sum += s.r * w;
            weight += w;
        }
    }
    return vec4f(sum / weight, c.g, 0.0, 1.0);
}

@fragment
fn ssao_blur_h_fs(i: Out) -> @location(0) vec4f {
    return blurred(vec2i(floor(i.clip.xy)), vec2i(1, 0));
}

@fragment
fn ssao_blur_v_fs(i: Out) -> @location(0) vec4f {
    return blurred(vec2i(floor(i.clip.xy)), vec2i(0, 1));
}

// The occlusion's share a stored view depth carries (render/shaders.rs `frame_ao_share`; none on a
// self-lit surface).
fn stored_share(d: f32) -> f32 {
    let b = bitcast<u32>(d);
    return select(f32((b >> 2u) & 7u) / 7.0, 0.0, (b & 2u) != 0u);
}

// The scene's colour × 1 − (1 − AO) × its share: the occlusion at the pixel, its buffer's four
// texels round it each by its bilinear share and by how near its depth is to the pixel's.
@fragment
fn ssao_apply_fs(i: Out) -> @location(0) vec4f {
    let p = vec2i(floor(i.clip.xy));
    let raw = depth_at(p);
    let share = stored_share(raw);
    if (raw <= 0.0 || share <= 0.0) {
        return vec4f(1.0);
    }
    let size = vec2i(textureDimensions(ao_in));
    let uv = (vec2f(p) + 0.5) * vec2f(size) / ssao.size.zw - 0.5;
    let base = vec2i(floor(uv));
    let f = uv - floor(uv);
    let edge = max(raw * ssao.blur.x * 2.0, 1e-4);
    var sum = 0.0;
    var weight = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2i(k & 1, k >> 1u);
        let s = textureLoad(ao_in, clamp(base + o, vec2i(0), size - vec2i(1)), 0);
        let b = mix(1.0 - f.x, f.x, f32(o.x)) * mix(1.0 - f.y, f.y, f32(o.y));
        let w = b * max(max(1.0 - abs(s.g - raw) / edge, 0.0), 1e-3) * select(1e-3, 1.0, s.g > 0.0);
        sum += s.r * w;
        weight += w;
    }
    let ao = sum / max(weight, 1e-6);
    let factor = 1.0 - (1.0 - ao) * share;
    return vec4f(factor, factor, factor, 1.0);
}
";

/// The passes' group (0): the view depth (as the scene's), one occlusion buffer, the uniform.
pub fn layout(device: &wgpu::Device, samples: u32) -> wgpu::BindGroupLayout {
    let texture = |binding, multisampled| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ssao"),
        entries: &[
            texture(0, samples > 1),
            texture(1, false),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            },
        ],
    })
}

/// The occlusion buffers (`a`: the occlusion, then the finished blur; `b`: the blur across) and a
/// group reading each. 1 × 1 with the occlusion off.
pub struct SsaoTargets {
    a: (wgpu::Texture, wgpu::TextureView),
    b: (wgpu::Texture, wgpu::TextureView),
    reads_a: wgpu::BindGroup,
    reads_b: wgpu::BindGroup,
}

impl SsaoTargets {
    pub fn new(
        device: &wgpu::Device,
        (width, height): (u32, u32),
        layout: &wgpu::BindGroupLayout,
        view_depth: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
    ) -> Self {
        let make = |label| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        };
        let (a, b) = (make("ssao"), make("ssao blur"));
        let group = |input: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view_depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            })
        };
        let (reads_a, reads_b) = (group(&a.1), group(&b.1));
        Self {
            a,
            b,
            reads_a,
            reads_b,
        }
    }

    /// The buffers' size.
    pub fn size(&self) -> (u32, u32) {
        let s = self.a.0.size();
        (s.width, s.height)
    }
}

impl Drop for SsaoTargets {
    fn drop(&mut self) {
        self.a.0.destroy();
        self.b.0.destroy();
    }
}

/// The passes' pipelines, each made on first use.
pub struct SsaoPipelines {
    pub occlusion: pipeline::Lazy,
    pub blur_h: pipeline::Lazy,
    pub blur_v: pipeline::Lazy,
    pub apply: pipeline::Lazy,
}

impl SsaoPipelines {
    pub fn new(device: &wgpu::Device, samples: u32, group: &wgpu::BindGroupLayout) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao"),
            source: wgpu::ShaderSource::Wgsl(source(samples).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao"),
            bind_group_layouts: &[Some(group)],
            immediate_size: 0,
        });
        let make = |label: &'static str,
                    entry: &'static str,
                    format: wgpu::TextureFormat,
                    blend: Option<wgpu::BlendState>,
                    write_mask: wgpu::ColorWrites| {
            let (device, module, layout) = (device.clone(), module.clone(), layout.clone());
            pipeline::Lazy::new(move || {
                pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
                        label: Some(label),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("ssao_vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend,
                                write_mask,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    },
                )
            })
        };
        let all = wgpu::ColorWrites::ALL;
        Self {
            occlusion: make("ssao", "ssao_fs", FORMAT, None, all),
            blur_h: make("ssao blur across", "ssao_blur_h_fs", FORMAT, None, all),
            blur_v: make("ssao blur down", "ssao_blur_v_fs", FORMAT, None, all),
            apply: make(
                "ssao apply",
                "ssao_apply_fs",
                HDR,
                Some(MULTIPLY),
                wgpu::ColorWrites::COLOR,
            ),
        }
    }

    /// The pipelines, for the warm-up.
    pub fn all(&self) -> [&pipeline::Lazy; 4] {
        [&self.occlusion, &self.blur_h, &self.blur_v, &self.apply]
    }

    /// Records the passes: the occlusion into `a`, across into `b`, down into `a`, and the scene's
    /// resolved `colour` multiplied by it. `timer` times them as one span. Returns the passes.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &SsaoTargets,
        colour: &wgpu::TextureView,
        timer: Option<(&super::timing::GpuTimer, Option<usize>)>,
    ) -> u32 {
        let steps: [(
            &wgpu::TextureView,
            &wgpu::RenderPipeline,
            &wgpu::BindGroup,
            bool,
        ); 4] = [
            (&targets.a.1, &self.occlusion, &targets.reads_b, true),
            (&targets.b.1, &self.blur_h, &targets.reads_a, true),
            (&targets.a.1, &self.blur_v, &targets.reads_b, true),
            (colour, &self.apply, &targets.reads_a, false),
        ];
        let last = steps.len() - 1;
        for (k, (view, pipeline, group, clear)) in steps.into_iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ssao"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if clear {
                            wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: timer.and_then(|(t, q)| t.writes_span(q, k == 0, k == last)),
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        4
    }
}

/// The apply pass's blend: the scene's colour × the factor (alpha kept by the write mask).
const MULTIPLY: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ssao_module_parses_and_validates() {
        for samples in [1, 4] {
            let module = naga::front::wgsl::parse_str(&source(samples)).expect("WGSL parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("WGSL validates");
        }
    }

    #[test]
    fn the_plan_per_tier() {
        // Off on a CPU adapter and the phone tier whatever the setting; the setting elsewhere.
        for q in [SsaoQuality::Off, SsaoQuality::Low, SsaoQuality::High] {
            assert_eq!(ssao_for(wgpu::DeviceType::Cpu, false, q), SsaoQuality::Off);
            assert_eq!(
                ssao_for(wgpu::DeviceType::IntegratedGpu, true, q),
                SsaoQuality::Off
            );
            assert_eq!(ssao_for(wgpu::DeviceType::IntegratedGpu, false, q), q);
        }
        // Low (the High preset) at half the drawing buffer, High (Ultra) at full.
        let size = (1280, 800);
        let low = dwell_look::ssao::buffer_size(size, SsaoQuality::Low.tier().unwrap());
        let high = dwell_look::ssao::buffer_size(size, SsaoQuality::High.tier().unwrap());
        assert_eq!((low, high), ((640, 400), size));
    }

    #[test]
    fn the_uniform_carries_the_strength_samples_and_the_spiral_in_the_buffers_texels() {
        let tier = dwell_look::ssao::LOW;
        let u = uniforms((1280, 800), (640, 400), tier, 90.0, 2.0);
        assert!((u[1] - 1.0).abs() < 1e-6, "tan(45°)");
        assert!((u[0] - 1.6).abs() < 1e-6, "× the aspect");
        assert!((u[2] - (SSAO.strength * 2.0) as f32).abs() < 1e-6);
        assert_eq!(u[3], tier.samples as f32);
        assert_eq!(&u[4..8], &[640.0, 400.0, 1280.0, 800.0]);
        assert!((u[11] - (SSAO.max_pixels / 2.0) as f32).abs() < 1e-4);
        assert_eq!(uniforms((1280, 800), (640, 400), tier, 90.0, 0.0)[2], 0.0);
    }

    #[test]
    fn the_apply_pass_multiplies_the_colour_and_keeps_its_coverage() {
        assert_eq!(MULTIPLY.color.dst_factor, wgpu::BlendFactor::Src);
        assert_eq!(MULTIPLY.color.src_factor, wgpu::BlendFactor::Zero);
        assert_eq!(MULTIPLY.alpha.dst_factor, wgpu::BlendFactor::One);
        // The shader reads the share the scene stored, none on a self-lit pixel.
        assert!(WGSL.contains("select(f32((b >> 2u) & 7u) / 7.0, 0.0, (b & 2u) != 0u)"));
    }
}
