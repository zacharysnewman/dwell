// Render scale's upscalers (render/upscale.rs; docs/DEV_TOOLS.md, Render scale and upscaling).
// The scene and its post passes drew into `picture` at the render size; these draw it at the
// canvas's size. `easu_fs` and `rcas_fs` are a port of AMD FidelityFX Super Resolution 1's EASU
// (edge-adaptive spatial upsampling) and RCAS (robust contrast-adaptive sharpening), MIT licence
// (shared/licenses, THIRD_PARTY_NOTICES), in f32 and with textureLoad taps. `bilinear_fs` is the
// plain stretch drawn as a pass (a full-size canvas), for comparing with the browser's stretch.

struct Upscale {
    // x, y: the output's size in pixels; z: RCAS's sharpness (2^-stops); w unused.
    out: vec4<f32>,
}

@group(0) @binding(0) var<uniform> up: Upscale;
@group(0) @binding(1) var picture: texture_2d<f32>;

@vertex
fn up_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}

fn tap(p: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(picture));
    return textureLoad(picture, clamp(p, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
}

// FSR's luma approximation (b/2 + r/2 + g).
fn luma(c: vec3<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

// The source position of an output pixel's centre: its integer texel and the fraction past it.
fn source_at(frag: vec2<f32>) -> vec2<f32> {
    let scale = vec2<f32>(textureDimensions(picture)) / up.out.xy;
    return frag * scale - vec2<f32>(0.5);
}

@fragment
fn bilinear_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let pp = source_at(frag.xy);
    let fp = floor(pp);
    let f = pp - fp;
    let i = vec2<i32>(fp);
    let top = mix(tap(i), tap(i + vec2<i32>(1, 0)), f.x);
    let bottom = mix(tap(i + vec2<i32>(0, 1)), tap(i + vec2<i32>(1, 1)), f.x);
    return vec4<f32>(mix(top, bottom, f.y), 1.0);
}

// EASU's direction and length from one bilinear corner's cross of lumas (FsrEasuSetF):
// a above, b left, c the corner, d right, e below; w the corner's bilinear weight.
fn easu_set(acc: vec3<f32>, w: f32, a: f32, b: f32, c: f32, d: f32, e: f32) -> vec3<f32> {
    let dc = d - c;
    let cb = c - b;
    var len_x = max(abs(dc), abs(cb));
    len_x = 1.0 / max(len_x, 1.0e-8);
    let dir_x = d - b;
    len_x = clamp(abs(dir_x) * len_x, 0.0, 1.0);
    len_x = len_x * len_x;
    let ec = e - c;
    let ca = c - a;
    var len_y = max(abs(ec), abs(ca));
    len_y = 1.0 / max(len_y, 1.0e-8);
    let dir_y = e - a;
    len_y = clamp(abs(dir_y) * len_y, 0.0, 1.0);
    len_y = len_y * len_y;
    return acc + vec3<f32>(dir_x * w, dir_y * w, (len_x + len_y) * w);
}

// One EASU tap (FsrEasuTapF): a Lanczos-2-like lobe stretched along the edge, into `acc`
// (colour sum, weight sum).
fn easu_tap(
    acc: vec4<f32>,
    off: vec2<f32>,
    dir: vec2<f32>,
    len2: vec2<f32>,
    lob: f32,
    clp: f32,
    c: vec3<f32>,
) -> vec4<f32> {
    var v = vec2<f32>(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x);
    v = v * len2;
    let d2 = min(v.x * v.x + v.y * v.y, clp);
    var wb = 2.0 / 5.0 * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb = wb * wb;
    wa = wa * wa;
    wb = 25.0 / 16.0 * wb - (25.0 / 16.0 - 1.0);
    let w = wb * wa;
    return acc + vec4<f32>(c * w, w);
}

@fragment
fn easu_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var pp = source_at(frag.xy);
    let fp = floor(pp);
    pp = pp - fp;
    let i = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let cb = tap(i + vec2<i32>(0, -1));
    let cc = tap(i + vec2<i32>(1, -1));
    let ce = tap(i + vec2<i32>(-1, 0));
    let cf = tap(i);
    let cg = tap(i + vec2<i32>(1, 0));
    let ch = tap(i + vec2<i32>(2, 0));
    let ci = tap(i + vec2<i32>(-1, 1));
    let cj = tap(i + vec2<i32>(0, 1));
    let ck = tap(i + vec2<i32>(1, 1));
    let cl = tap(i + vec2<i32>(2, 1));
    let cn = tap(i + vec2<i32>(0, 2));
    let co = tap(i + vec2<i32>(1, 2));
    let lb = luma(cb);
    let lc = luma(cc);
    let le = luma(ce);
    let lf = luma(cf);
    let lg = luma(cg);
    let lh = luma(ch);
    let li = luma(ci);
    let lj = luma(cj);
    let lk = luma(ck);
    let ll = luma(cl);
    let ln = luma(cn);
    let lo = luma(co);
    // Direction and length, from the four bilinear corners f, g, j, k.
    var dl = vec3<f32>(0.0);
    dl = easu_set(dl, (1.0 - pp.x) * (1.0 - pp.y), lb, le, lf, lg, lj);
    dl = easu_set(dl, pp.x * (1.0 - pp.y), lc, lf, lg, lh, lk);
    dl = easu_set(dl, (1.0 - pp.x) * pp.y, lf, li, lj, lk, ln);
    dl = easu_set(dl, pp.x * pp.y, lg, lj, lk, ll, lo);
    var dir = dl.xy;
    var len = dl.z;
    let dir2 = dir * dir;
    var dir_r = dir2.x + dir2.y;
    let zero = dir_r < 1.0 / 32768.0;
    dir_r = select(inverseSqrt(dir_r), 1.0, zero);
    dir.x = select(dir.x, 1.0, zero);
    dir = dir * dir_r;
    len = len * 0.5;
    len = len * len;
    let stretch = (dir.x * dir.x + dir.y * dir.y) / max(abs(dir.x), abs(dir.y));
    let len2 = vec2<f32>(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;
    var acc = vec4<f32>(0.0);
    acc = easu_tap(acc, vec2<f32>(0.0, -1.0) - pp, dir, len2, lob, clp, cb);
    acc = easu_tap(acc, vec2<f32>(1.0, -1.0) - pp, dir, len2, lob, clp, cc);
    acc = easu_tap(acc, vec2<f32>(-1.0, 1.0) - pp, dir, len2, lob, clp, ci);
    acc = easu_tap(acc, vec2<f32>(0.0, 1.0) - pp, dir, len2, lob, clp, cj);
    acc = easu_tap(acc, vec2<f32>(0.0, 0.0) - pp, dir, len2, lob, clp, cf);
    acc = easu_tap(acc, vec2<f32>(-1.0, 0.0) - pp, dir, len2, lob, clp, ce);
    acc = easu_tap(acc, vec2<f32>(1.0, 1.0) - pp, dir, len2, lob, clp, ck);
    acc = easu_tap(acc, vec2<f32>(2.0, 1.0) - pp, dir, len2, lob, clp, cl);
    acc = easu_tap(acc, vec2<f32>(2.0, 0.0) - pp, dir, len2, lob, clp, ch);
    acc = easu_tap(acc, vec2<f32>(1.0, 0.0) - pp, dir, len2, lob, clp, cg);
    acc = easu_tap(acc, vec2<f32>(1.0, 2.0) - pp, dir, len2, lob, clp, co);
    acc = easu_tap(acc, vec2<f32>(0.0, 2.0) - pp, dir, len2, lob, clp, cn);
    // Deringing: inside the four nearest texels' range.
    let lo4 = min(min(cf, cg), min(cj, ck));
    let hi4 = max(max(cf, cg), max(cj, ck));
    let c = min(hi4, max(lo4, acc.rgb / max(acc.a, 1.0e-6)));
    return vec4<f32>(c, 1.0);
}

// RCAS's limit on the lobe (FSR_RCAS_LIMIT).
const RCAS_LIMIT: f32 = 0.25 - 1.0 / 16.0;

@fragment
fn rcas_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(frag.xy);
    //    b
    //  d e f
    //    h
    let b = tap(p + vec2<i32>(0, -1));
    let d = tap(p + vec2<i32>(-1, 0));
    let e = tap(p);
    let f = tap(p + vec2<i32>(1, 0));
    let h = tap(p + vec2<i32>(0, 1));
    let mn4 = min(min(b, d), min(f, h));
    let mx4 = max(max(b, d), max(f, h));
    let hit_min = min(mn4, e) / (4.0 * mx4 + vec3<f32>(1.0e-6));
    let hit_max = (vec3<f32>(1.0) - max(mx4, e)) / (4.0 * mn4 - vec3<f32>(4.0) - vec3<f32>(1.0e-6));
    let lobe_rgb = max(-hit_min, hit_max);
    let lobe = max(-RCAS_LIMIT, min(max(lobe_rgb.r, max(lobe_rgb.g, lobe_rgb.b)), 0.0)) * up.out.z;
    let c = (lobe * (b + d + f + h) + e) / (4.0 * lobe + 1.0);
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
