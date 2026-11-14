//! Bloom, the glow round self-lit surfaces (WORK_QUEUE item 19; owner 2026-11-11: "Lava should have
//! bloom glow"; `shared/look/bloom.json`, `dwell_look::bloom`). Keyed on emissive, not on brightness:
//! the bright pass takes only the pixels the scene flags self-lit (`self_lit_depth`: lava, a flame, a
//! lamp), their linear light above the threshold, into the chain's first level; the down passes
//! halve it level by level (the dual filter's five taps); the up passes add each level, tent-filtered,
//! into the one above it, ending in the second level, which the composite adds to the scene's linear
//! light before the tone curve (`with_bloom`). Its own small shader module and three pipelines, each
//! made on first use ([`pipeline::Lazy`]), so a tier that is off never builds them.
//!
//! Cost: the bright pass reads its footprint of the scene once (the colour and each sample's view
//! depth); every other pass is five or nine filtered taps over a level a quarter of the last.

use dwell_look::bloom::{BLOOM, BloomQuality};

use super::frame::{self, BloomChain};
use super::pipeline;

/// Bloom's setting a frame draws with the Bloom setting `quality`: none on a CPU adapter or the
/// phone tier (the owner plays on Low on an iPhone: frame rate there first).
pub fn bloom_for(
    device_type: wgpu::DeviceType,
    phone: bool,
    quality: BloomQuality,
) -> BloomQuality {
    if super::is_cpu(device_type) || phone {
        BloomQuality::Off
    } else {
        quality
    }
}

/// The bright pass's module (`bright`) or the down and up passes' (their groups differ, so two
/// modules), for a scene of `samples` per pixel.
pub fn source(samples: u32, bright: bool) -> String {
    let depth = if samples > 1 {
        "texture_multisampled_2d<f32>"
    } else {
        "texture_2d<f32>"
    };
    format!(
        r"
const SCENE_SAMPLES: i32 = {samples};
const THRESHOLD: f32 = {:?};
const RADIUS: f32 = {:?};

struct Out {{
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}};

@vertex
fn bloom_vs(@builtin(vertex_index) i: u32) -> Out {{
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2f(p.x, 1.0 - p.y);
    return o;
}}

{}",
        BLOOM.threshold as f32,
        BLOOM.radius as f32,
        if bright {
            BRIGHT.replace("{depth}", depth)
        } else {
            LEVEL.to_string()
        },
    )
}

const BRIGHT: &str = r"
@group(0) @binding(0) var scene_color: texture_2d<f32>;
@group(0) @binding(1) var scene_depth: {depth};

// The self-lit flag of a stored view depth (render/shaders.rs `frame_self_lit`).
fn self_lit(d: f32) -> bool {
    return (bitcast<u32>(d) & 2u) != 0u;
}

// The bright pass: over the scene's texels this level's texel covers, the self-lit share of each
// (its samples flagged) × its linear light above the threshold (by its brightest channel).
@fragment
fn bloom_bright_fs(i: Out) -> @location(0) vec4f {
    let src = vec2i(textureDimensions(scene_color));
    // The scene's texels per this level's texel, each axis (its tier's 1 / scale).
    let k = max(vec2i(round(vec2f(abs(dpdx(i.uv.x)), abs(dpdy(i.uv.y))) * vec2f(src))), vec2i(1));
    let base = vec2i(floor(i.clip.xy)) * k;
    var sum = vec3f(0.0);
    for (var y = 0; y < k.y; y++) {
        for (var x = 0; x < k.x; x++) {
            let p = min(base + vec2i(x, y), src - 1);
            let c = textureLoad(scene_color, p, 0);
            if (c.a <= 0.0) {
                continue;
            }
            var lit = 0.0;
            for (var s = 0; s < SCENE_SAMPLES; s++) {
                lit += select(0.0, 1.0, self_lit(textureLoad(scene_depth, p, s).r));
            }
            if (lit <= 0.0) {
                continue;
            }
            let v = c.rgb / c.a;
            let peak = max(max(v.r, v.g), v.b);
            let over = max(peak - THRESHOLD, 0.0) / max(peak, 1e-4);
            sum += v * over * (lit / f32(SCENE_SAMPLES)) * min(c.a, 1.0);
        }
    }
    return vec4f(sum / f32(k.x * k.y), 1.0);
}

";

const LEVEL: &str = r"
@group(0) @binding(0) var level: texture_2d<f32>;
@group(0) @binding(1) var level_sampler: sampler;

// Down a level (the dual filter): the centre and four diagonal taps half a texel out.
@fragment
fn bloom_down_fs(i: Out) -> @location(0) vec4f {
    let t = 1.0 / vec2f(textureDimensions(level));
    let s = textureSampleLevel(level, level_sampler, i.uv, 0.0).rgb * 4.0
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(-t.x, -t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(t.x, -t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(-t.x, t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(t.x, t.y), 0.0).rgb;
    return vec4f(s / 8.0, 1.0);
}

// Up a level: the level below, tent-filtered (3 × 3 taps RADIUS of its texels apart), added
// (blended) into this one.
@fragment
fn bloom_up_fs(i: Out) -> @location(0) vec4f {
    let t = RADIUS / vec2f(textureDimensions(level));
    var s = textureSampleLevel(level, level_sampler, i.uv, 0.0).rgb * 4.0;
    s += (textureSampleLevel(level, level_sampler, i.uv + vec2f(-t.x, 0.0), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(t.x, 0.0), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(0.0, -t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(0.0, t.y), 0.0).rgb) * 2.0;
    s += textureSampleLevel(level, level_sampler, i.uv + vec2f(-t.x, -t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(t.x, -t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(-t.x, t.y), 0.0).rgb
        + textureSampleLevel(level, level_sampler, i.uv + vec2f(t.x, t.y), 0.0).rgb;
    return vec4f(s / 16.0, 1.0);
}
";

/// Bloom's pipelines: the bright pass (over the scene's read group) and the down and up passes
/// (over one level's group), each made on first use.
pub struct BloomPipelines {
    pub bright: pipeline::Lazy,
    pub down: pipeline::Lazy,
    pub up: pipeline::Lazy,
}

impl BloomPipelines {
    pub fn new(
        device: &wgpu::Device,
        samples: u32,
        scene_read_layout: &wgpu::BindGroupLayout,
        level_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let module = |bright| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bloom"),
                source: wgpu::ShaderSource::Wgsl(source(samples, bright).into()),
            })
        };
        let (bright_module, level_module) = (module(true), module(false));
        let layout = |label, group: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(group)],
                immediate_size: 0,
            })
        };
        let bright_layout = layout("bloom bright", scene_read_layout);
        let level = layout("bloom level", level_layout);
        let make = |label: &'static str,
                    module: &wgpu::ShaderModule,
                    layout: &wgpu::PipelineLayout,
                    entry: &'static str,
                    blend: Option<wgpu::BlendState>| {
            let (device, module, layout) = (device.clone(), module.clone(), layout.clone());
            pipeline::Lazy::new(move || {
                pipeline::create(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
                        label: Some(label),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("bloom_vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: frame::BLOOM,
                                blend,
                                write_mask: wgpu::ColorWrites::ALL,
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
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        Self {
            bright: make(
                "bloom bright",
                &bright_module,
                &bright_layout,
                "bloom_bright_fs",
                None,
            ),
            down: make("bloom down", &level_module, &level, "bloom_down_fs", None),
            up: make("bloom up", &level_module, &level, "bloom_up_fs", Some(add)),
        }
    }

    /// The pipelines, for the warm-up.
    pub fn all(&self) -> [&pipeline::Lazy; 3] {
        [&self.bright, &self.down, &self.up]
    }

    /// Records the chain: the bright pass from `scene_read` into the first level, down to the last,
    /// and back up into the second (the one the composite reads). Returns the passes recorded.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene_read: &wgpu::BindGroup,
        chain: &BloomChain,
    ) -> u32 {
        let n = chain.levels.len();
        if n < 2 {
            return 0;
        }
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    target: &wgpu::TextureView,
                    load: wgpu::LoadOp<wgpu::Color>,
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        pass(encoder, &chain.levels[0], clear, &self.bright, scene_read);
        for l in 1..n {
            pass(
                encoder,
                &chain.levels[l],
                clear,
                &self.down,
                &chain.groups[l - 1],
            );
        }
        for l in (2..n).rev() {
            pass(
                encoder,
                &chain.levels[l - 1],
                wgpu::LoadOp::Load,
                &self.up,
                &chain.groups[l],
            );
        }
        (1 + (n - 1) + (n - 2)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bloom_module_parses_and_validates() {
        for (samples, bright) in [(1, true), (4, true), (1, false)] {
            let module =
                naga::front::wgsl::parse_str(&source(samples, bright)).expect("WGSL parses");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("WGSL validates");
        }
    }

    #[test]
    fn bloom_is_off_on_a_cpu_adapter_and_the_phone_tier() {
        for q in [BloomQuality::Off, BloomQuality::Low, BloomQuality::High] {
            assert_eq!(
                bloom_for(wgpu::DeviceType::Cpu, false, q),
                BloomQuality::Off
            );
            assert_eq!(
                bloom_for(wgpu::DeviceType::IntegratedGpu, true, q),
                BloomQuality::Off
            );
            assert_eq!(bloom_for(wgpu::DeviceType::IntegratedGpu, false, q), q);
        }
    }
}
