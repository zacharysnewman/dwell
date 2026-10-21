// The pixel-art preview's post passes (dwell-look pixel_art.rs; docs/proposals/PIXEL_ART_LOOK.md):
// the low-res picture snapped to the palette with a 4 × 4 Bayer ordered dither at each texel
// (`quantize_fs`; `quantize`'s CPU twin is pixel_art.rs `quantize`), then drawn on the canvas, each
// canvas pixel one texel (`post_fs`: a nearest-neighbour integer upscale, bars outside the rect).

struct Post {
    // x canvas pixels per texel, yz the low-res target's top-left corner (canvas px), w colours.
    grid: vec4f,
    // The visible rect on the canvas: x0, y0, x1, y1 (px).
    rect: vec4f,
    // x the dither's strength, yz the shift (whole canvas px).
    cfg: vec4f,
    // The bars' colour (sRGB).
    bar: vec4f,
    // The palette in OKLab, then in sRGB (output colour).
    lab: array<vec4f, 64>,
    srgb: array<vec4f, 64>,
};
@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var low: texture_2d<f32>;

@vertex
fn post_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn to_linear(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}

fn oklab(srgb: vec3f) -> vec3f {
    let c = to_linear(srgb);
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let q = pow(max(vec3f(l, m, s), vec3f(0.0)), vec3f(1.0 / 3.0));
    return vec3f(
        0.2104542553 * q.x + 0.7936177850 * q.y - 0.0040720468 * q.z,
        1.9779984951 * q.x - 2.4285922050 * q.y + 0.4505937099 * q.z,
        0.0259040371 * q.x + 0.7827717662 * q.y - 0.8086757660 * q.z,
    );
}

fn bayer4(p: vec2u) -> f32 {
    let b = array(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    return (f32(b[(p.y % 4u) * 4u + (p.x % 4u)]) + 0.5) / 16.0;
}

// The palette index for `srgb` at texel `p`: the nearest colour, or the second nearest where the
// colour lies past this texel's threshold toward it.
fn quantize(srgb: vec3f, p: vec2u) -> u32 {
    let c = oklab(srgb);
    let n = u32(post.grid.w);
    var a = 0u;
    var b = 0u;
    var da = 1e30;
    var db = 1e30;
    for (var i = 0u; i < n; i++) {
        let e = c - post.lab[i].xyz;
        let d = dot(e, e);
        if (d < da) {
            b = a;
            db = da;
            a = i;
            da = d;
        } else if (d < db) {
            b = i;
            db = d;
        }
    }
    let strength = post.cfg.x;
    if (n < 2u || strength <= 0.0) {
        return a;
    }
    let pa = post.lab[a].xyz;
    let ab = post.lab[b].xyz - pa;
    let t = clamp(dot(c - pa, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
    let threshold = 0.5 + min(strength, 1.0) * (bayer4(p) - 0.5);
    return select(a, b, t > threshold);
}

// First, at low res: each texel snapped to the palette (`low` is the composite's picture).
@fragment
fn quantize_fs(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let p = vec2i(frag.xy);
    let c = textureLoad(low, p, 0).rgb;
    return vec4f(post.srgb[quantize(c, vec2u(p))].rgb, 1.0);
}

// Then on the canvas: each canvas pixel one texel of the snapped picture (`low` now holds it),
// bars outside the rect.
@fragment
fn post_fs(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let q = frag.xy;
    if (q.x < post.rect.x || q.y < post.rect.y || q.x >= post.rect.z || q.y >= post.rect.w) {
        return vec4f(post.bar.rgb, 1.0);
    }
    let size = vec2i(textureDimensions(low));
    let t = floor((q - post.grid.yz - post.cfg.yz) / post.grid.x);
    let p = clamp(vec2i(t), vec2i(0), size - 1);
    return vec4f(textureLoad(low, p, 0).rgb, 1.0);
}
