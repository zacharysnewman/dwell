//! Rendering the march below the output resolution and upscaling it (docs/RAY_MARCHED_RENDERER.md
//! §4.2 step 3, Phase 25 A4/A5): the marcher marches and shades at `scale` × the output size
//! (its cone LOD follows the coarser pixels), then one of these fills the output-size colour the
//! copy pass reads:
//!
//! - [`Upscaler::Fsr1`]: AMD's FidelityFX Super Resolution 1 — EASU (edge-adaptive spatial
//!   upsampling: a 12-tap, direction- and length-shaped Lanczos-like kernel, dering-clamped to the
//!   four nearest texels) then RCAS (robust contrast-adaptive sharpening), ported to WGSL from
//!   `ffx_fsr1.h`'s float path. Both run on colour tone-mapped into 0..1 by `c / (1 + max(c))`
//!   (FSR expects perceptual input; the HDR colour is turned back after RCAS).
//! - [`Upscaler::Bilateral`]: a joint bilateral upsample — the four bilinear neighbours weighted by
//!   how close their view depth and face are to the nearest texel's, so edges between terrain at
//!   different depths (and the sky) stay sharp.
//!
//! The view depth is not filtered: the copy pass takes the nearest low-resolution texel's, so the
//! overlays and the fog see one surface per pixel. The colour stays premultiplied by coverage
//! (alpha 1 on a hit, 0 for the sky), so a silhouette's blend against the sky upscales too.

/// An upscaler the marcher offers (`?upscaler=`, `__dwell.upscalers()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Upscaler {
    /// Output resolution: nothing to upscale.
    #[default]
    None,
    Fsr1,
    Bilateral,
}

impl Upscaler {
    pub const ALL: [Upscaler; 2] = [Upscaler::Fsr1, Upscaler::Bilateral];
    pub fn name(self) -> &'static str {
        match self {
            Upscaler::None => "none",
            Upscaler::Fsr1 => "fsr1",
            Upscaler::Bilateral => "bilateral",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        [Upscaler::None, Upscaler::Fsr1, Upscaler::Bilateral]
            .into_iter()
            .find(|u| u.name() == name)
    }
}

/// The render scales the marcher accepts (`?scale=` with a marcher upscaler).
pub const SCALE_MIN: f32 = 0.25;

/// The size the march renders at for an output of `full` pixels at `scale` (each side at least 1;
/// scale 1 or no upscaler: the output size).
pub fn render_size(full: (u32, u32), scale: f32, upscaler: Upscaler) -> (u32, u32) {
    if upscaler == Upscaler::None || scale >= 1.0 {
        return full;
    }
    let s = scale.max(SCALE_MIN);
    let side = |v: u32| ((v as f32 * s).round() as u32).max(1);
    (side(full.0), side(full.1))
}

/// RCAS sharpness in stops (0 = most; FSR's default 0.2).
pub const RCAS_STOPS: f32 = 0.2;

const WORKGROUP: u32 = 8;

/// The upscalers' WGSL: `easu_main`, `rcas_main`, `bilateral_main`. Group 0: the input colour
/// (and for the bilateral its depth and hit records), group 1: the output (rgba16float storage).
pub fn source() -> String {
    format!(
        "const RCAS_SHARPNESS: f32 = {:?};\nconst HIT_FACE_SHIFT: u32 = 16u;\n{SOURCE}",
        (-RCAS_STOPS).exp2()
    )
}

const SOURCE: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_depth: texture_2d<f32>;
@group(0) @binding(2) var src_hit: texture_2d<u32>;
@group(1) @binding(0) var dst: texture_storage_2d<rgba16float, write>;

// HDR colour to 0..1 and back (premultiplied rgb; alpha untouched).
fn tonemap(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb / (1.0 + max(c.r, max(c.g, c.b))), c.a);
}
fn untonemap(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb / max(1.0 - max(c.r, max(c.g, c.b)), 1e-4), c.a);
}

fn fetch(p: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(src)) - vec2<i32>(1);
    return tonemap(textureLoad(src, clamp(p, vec2<i32>(0), d), 0));
}

fn luma(c: vec4<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

// FsrEasuSetF: direction and length from the cross around one of the four bilinear corners.
struct DirLen {
    dir: vec2<f32>,
    len: f32,
};

fn easu_set(acc: DirLen, w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32) -> DirLen {
    var o = acc;
    let dc = ld - lc;
    let cb = lc - lb;
    var len_x = max(abs(dc), abs(cb));
    len_x = 1.0 / max(len_x, 1e-8);
    let dir_x = ld - lb;
    o.dir.x += dir_x * w;
    len_x = clamp(abs(dir_x) * len_x, 0.0, 1.0);
    o.len += len_x * len_x * w;
    let ec = le - lc;
    let ca = lc - la;
    var len_y = max(abs(ec), abs(ca));
    len_y = 1.0 / max(len_y, 1e-8);
    let dir_y = le - la;
    o.dir.y += dir_y * w;
    len_y = clamp(abs(dir_y) * len_y, 0.0, 1.0);
    o.len += len_y * len_y * w;
    return o;
}

struct Acc {
    c: vec4<f32>,
    w: f32,
};

// FsrEasuTapF: one tap of the shaped kernel.
fn easu_tap(acc: Acc, off: vec2<f32>, dir: vec2<f32>, len2: vec2<f32>, lob: f32, clp: f32, c: vec4<f32>) -> Acc {
    var v = vec2<f32>(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x) * len2;
    let d2 = min(v.x * v.x + v.y * v.y, clp);
    var wb = 0.4 * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb *= wb;
    wa *= wa;
    wb = 1.5625 * wb - 0.5625;
    let w = wb * wa;
    return Acc(acc.c + c * w, acc.w + w);
}

@compute @workgroup_size(8, 8)
fn easu_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let out_size = textureDimensions(dst);
    if (gid.x >= out_size.x || gid.y >= out_size.y) {
        return;
    }
    let ratio = vec2<f32>(textureDimensions(src)) / vec2<f32>(out_size);
    var pp = (vec2<f32>(gid.xy) + 0.5) * ratio - 0.5;
    let fp = floor(pp);
    pp -= fp;
    let f0 = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let b = fetch(f0 + vec2<i32>(0, -1));
    let c = fetch(f0 + vec2<i32>(1, -1));
    let e = fetch(f0 + vec2<i32>(-1, 0));
    let f = fetch(f0);
    let g = fetch(f0 + vec2<i32>(1, 0));
    let h = fetch(f0 + vec2<i32>(2, 0));
    let i = fetch(f0 + vec2<i32>(-1, 1));
    let j = fetch(f0 + vec2<i32>(0, 1));
    let k = fetch(f0 + vec2<i32>(1, 1));
    let l = fetch(f0 + vec2<i32>(2, 1));
    let n = fetch(f0 + vec2<i32>(0, 2));
    let o = fetch(f0 + vec2<i32>(1, 2));
    let bl = luma(b);
    let cl = luma(c);
    let el = luma(e);
    let fl = luma(f);
    let gl = luma(g);
    let hl = luma(h);
    let il = luma(i);
    let jl = luma(j);
    let kl = luma(k);
    let ll = luma(l);
    let nl = luma(n);
    let ol = luma(o);
    var dl = DirLen(vec2<f32>(0.0), 0.0);
    dl = easu_set(dl, (1.0 - pp.x) * (1.0 - pp.y), bl, el, fl, gl, jl);
    dl = easu_set(dl, pp.x * (1.0 - pp.y), cl, fl, gl, hl, kl);
    dl = easu_set(dl, (1.0 - pp.x) * pp.y, fl, il, jl, kl, nl);
    dl = easu_set(dl, pp.x * pp.y, gl, jl, kl, ll, ol);
    var dir = dl.dir;
    let dir2 = dir * dir;
    var dir_r = dir2.x + dir2.y;
    let zero = dir_r < 1.0 / 32768.0;
    dir_r = select(inverseSqrt(dir_r), 1.0, zero);
    dir.x = select(dir.x, 1.0, zero);
    dir *= dir_r;
    var len = dl.len * 0.5;
    len *= len;
    let stretch = (dir.x * dir.x + dir.y * dir.y) / max(abs(dir.x), abs(dir.y));
    let len2 = vec2<f32>(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;
    var acc = Acc(vec4<f32>(0.0), 0.0);
    acc = easu_tap(acc, vec2<f32>(0.0, -1.0) - pp, dir, len2, lob, clp, b);
    acc = easu_tap(acc, vec2<f32>(1.0, -1.0) - pp, dir, len2, lob, clp, c);
    acc = easu_tap(acc, vec2<f32>(-1.0, 1.0) - pp, dir, len2, lob, clp, i);
    acc = easu_tap(acc, vec2<f32>(0.0, 1.0) - pp, dir, len2, lob, clp, j);
    acc = easu_tap(acc, vec2<f32>(0.0, 0.0) - pp, dir, len2, lob, clp, f);
    acc = easu_tap(acc, vec2<f32>(-1.0, 0.0) - pp, dir, len2, lob, clp, e);
    acc = easu_tap(acc, vec2<f32>(1.0, 1.0) - pp, dir, len2, lob, clp, k);
    acc = easu_tap(acc, vec2<f32>(2.0, 1.0) - pp, dir, len2, lob, clp, l);
    acc = easu_tap(acc, vec2<f32>(2.0, 0.0) - pp, dir, len2, lob, clp, h);
    acc = easu_tap(acc, vec2<f32>(1.0, 0.0) - pp, dir, len2, lob, clp, g);
    acc = easu_tap(acc, vec2<f32>(1.0, 2.0) - pp, dir, len2, lob, clp, o);
    acc = easu_tap(acc, vec2<f32>(0.0, 2.0) - pp, dir, len2, lob, clp, n);
    // Dering: within the four nearest texels.
    let mn = min(min(f, g), min(j, k));
    let mx = max(max(f, g), max(j, k));
    let pix = min(mx, max(mn, acc.c / acc.w));
    textureStore(dst, vec2<i32>(gid.xy), pix);
}

// RCAS on EASU's output (still tone-mapped); writes HDR colour.
@compute @workgroup_size(8, 8)
fn rcas_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(dst));
    let p = vec2<i32>(gid.xy);
    if (p.x >= size.x || p.y >= size.y) {
        return;
    }
    let d = size - vec2<i32>(1);
    let bb = textureLoad(src, clamp(p + vec2<i32>(0, -1), vec2<i32>(0), d), 0);
    let dd = textureLoad(src, clamp(p + vec2<i32>(-1, 0), vec2<i32>(0), d), 0);
    let ee = textureLoad(src, p, 0);
    let ff = textureLoad(src, clamp(p + vec2<i32>(1, 0), vec2<i32>(0), d), 0);
    let hh = textureLoad(src, clamp(p + vec2<i32>(0, 1), vec2<i32>(0), d), 0);
    let mn4 = min(min(bb.rgb, dd.rgb), min(ff.rgb, hh.rgb));
    let mx4 = max(max(bb.rgb, dd.rgb), max(ff.rgb, hh.rgb));
    let hit_min = min(mn4, ee.rgb) / max(4.0 * mx4, vec3<f32>(1e-5));
    let hit_max = (1.0 - max(mx4, ee.rgb)) / min(4.0 * min(mn4, ee.rgb) - 4.0, vec3<f32>(-1e-5));
    let lobe3 = max(-hit_min, hit_max);
    let lobe = max(-(0.25 - 1.0 / 16.0), min(max(lobe3.r, max(lobe3.g, lobe3.b)), 0.0)) * RCAS_SHARPNESS;
    let rgb = (lobe * (bb.rgb + dd.rgb + hh.rgb + ff.rgb) + ee.rgb) / (4.0 * lobe + 1.0);
    textureStore(dst, p, untonemap(vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(0.999)), ee.a)));
}

// The joint bilateral upsample: bilinear weights × depth and face agreement with the nearest
// texel.
@compute @workgroup_size(8, 8)
fn bilateral_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let out_size = textureDimensions(dst);
    if (gid.x >= out_size.x || gid.y >= out_size.y) {
        return;
    }
    let in_size = vec2<i32>(textureDimensions(src));
    let ratio = vec2<f32>(in_size) / vec2<f32>(out_size);
    let pp = (vec2<f32>(gid.xy) + 0.5) * ratio - 0.5;
    let fp = floor(pp);
    let fr = pp - fp;
    let f0 = vec2<i32>(fp);
    let hi = in_size - vec2<i32>(1);
    let near = clamp(vec2<i32>(floor(pp + 0.5)), vec2<i32>(0), hi);
    let ref_depth = textureLoad(src_depth, near, 0).r;
    let ref_hit = textureLoad(src_hit, near, 0).r;
    var c = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1);
        let q = clamp(f0 + o, vec2<i32>(0), hi);
        let wb = select(1.0 - fr.x, fr.x, o.x == 1) * select(1.0 - fr.y, fr.y, o.y == 1);
        let dq = textureLoad(src_depth, q, 0).r;
        let hq = textureLoad(src_hit, q, 0).r;
        // Depth: relative difference; the sky (0) agrees only with the sky.
        let rel = abs(dq - ref_depth) / max(max(dq, ref_depth), 1e-3);
        var w = wb * exp(-rel * rel * 400.0);
        if ((hq == 0u) != (ref_hit == 0u)) {
            w *= 1e-3;
        } else if (((hq >> HIT_FACE_SHIFT) & 7u) != ((ref_hit >> HIT_FACE_SHIFT) & 7u)) {
            w *= 0.05;
        }
        c += textureLoad(src, q, 0) * w;
        wsum += w;
    }
    let pix = select(textureLoad(src, near, 0), c / wsum, wsum > 1e-6);
    textureStore(dst, vec2<i32>(gid.xy), pix);
}
";

/// The upscale passes and their output-size targets.
pub struct Upscale {
    easu: wgpu::ComputePipeline,
    rcas: wgpu::ComputePipeline,
    bilateral: wgpu::ComputePipeline,
    in_layout: wgpu::BindGroupLayout,
    out_layout: wgpu::BindGroupLayout,
    /// At the output size: EASU's result, then the final colour.
    targets: Option<Targets>,
}

struct Targets {
    size: (u32, u32),
    low: (u32, u32),
    tmp: (wgpu::Texture, wgpu::TextureView),
    color: (wgpu::Texture, wgpu::TextureView),
    /// EASU: low colour → tmp; RCAS: tmp → colour; bilateral: low → colour.
    easu_in: wgpu::BindGroup,
    easu_out: wgpu::BindGroup,
    rcas_in: wgpu::BindGroup,
    color_out: wgpu::BindGroup,
}

impl Drop for Targets {
    fn drop(&mut self) {
        self.tmp.0.destroy();
        self.color.0.destroy();
    }
}

impl Upscale {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("march upscale"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let tex = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let in_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("upscale in"),
            entries: &[
                tex(0, unfiltered),
                tex(1, unfiltered),
                tex(2, wgpu::TextureSampleType::Uint),
            ],
        });
        let out_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("upscale out"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: super::COLOR,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("upscale"),
            bind_group_layouts: &[Some(&in_layout), Some(&out_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            easu: pipeline("easu_main"),
            rcas: pipeline("rcas_main"),
            bilateral: pipeline("bilateral_main"),
            in_layout,
            out_layout,
            targets: None,
        }
    }

    /// Records the upscale of `low` (the march's targets) to `size` with `upscaler`; returns the
    /// output-size colour view.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        low: &super::MarchTargets,
        size: (u32, u32),
        upscaler: Upscaler,
    ) -> &wgpu::TextureView {
        if self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != size || t.low != low.size)
        {
            self.targets = None;
            self.targets = Some(self.make(device, low, size));
        }
        let t = self.targets.as_ref().expect("targets");
        let groups = (size.0.div_ceil(WORKGROUP), size.1.div_ceil(WORKGROUP));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("march upscale"),
            timestamp_writes: None,
        });
        match upscaler {
            Upscaler::Fsr1 => {
                pass.set_pipeline(&self.easu);
                pass.set_bind_group(0, &t.easu_in, &[]);
                pass.set_bind_group(1, &t.easu_out, &[]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
                pass.set_pipeline(&self.rcas);
                pass.set_bind_group(0, &t.rcas_in, &[]);
                pass.set_bind_group(1, &t.color_out, &[]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
            }
            Upscaler::Bilateral | Upscaler::None => {
                pass.set_pipeline(&self.bilateral);
                pass.set_bind_group(0, &t.easu_in, &[]);
                pass.set_bind_group(1, &t.color_out, &[]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
            }
        }
        drop(pass);
        &t.color.1
    }

    fn make(&self, device: &wgpu::Device, low: &super::MarchTargets, size: (u32, u32)) -> Targets {
        let make = |label| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::COLOR,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        };
        let tmp = make("upscale tmp");
        let color = make("upscale colour");
        fn view(b: u32, v: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding: b,
                resource: wgpu::BindingResource::TextureView(v),
            }
        }
        let input = |c: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("upscale in"),
                layout: &self.in_layout,
                entries: &[view(0, c), view(1, &low.depth.1), view(2, &low.hit.1)],
            })
        };
        let output = |c: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("upscale out"),
                layout: &self.out_layout,
                entries: &[view(0, c)],
            })
        };
        Targets {
            size,
            low: low.size,
            easu_in: input(&low.color.1),
            easu_out: output(&tmp.1),
            rcas_in: input(&tmp.1),
            color_out: output(&color.1),
            tmp,
            color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upscale_module_parses_and_validates() {
        let src = source();
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("WGSL parses: {}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for entry in ["easu_main", "rcas_main", "bilateral_main"] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "{entry}");
        }
    }

    #[test]
    fn render_sizes_follow_the_scale_only_with_an_upscaler() {
        assert_eq!(render_size((1280, 720), 0.5, Upscaler::Fsr1), (640, 360));
        assert_eq!(render_size((1280, 720), 0.75, Upscaler::Bilateral), (960, 540));
        assert_eq!(render_size((1280, 720), 0.5, Upscaler::None), (1280, 720));
        assert_eq!(render_size((1280, 720), 1.0, Upscaler::Fsr1), (1280, 720));
        assert_eq!(render_size((3, 1), 0.1, Upscaler::Fsr1), (1, 1));
    }

    #[test]
    fn upscalers_round_trip_their_names() {
        for u in [Upscaler::None, Upscaler::Fsr1, Upscaler::Bilateral] {
            assert_eq!(Upscaler::parse(u.name()), Some(u));
        }
        assert_eq!(Upscaler::parse("taa-u"), None);
    }
}
