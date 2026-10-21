//! Flames (docs/LIGHTS.md §6): the animated fire drawn where a light source with a `flame` burns —
//! a placed torch's top, a torch in a hand. A flame is two camera-facing billboards: the flame
//! itself, a teardrop cut from rising value noise whose top splits into separate tongues, coloured
//! along its look's ramp (deep red-orange tips to a yellow core) in a few painted bands; and a
//! soft glow behind it with a few embers rising from it. Its height and the glow follow the light's
//! flicker (`lights::flicker`, the same phase), so the light and the flame pulse together.
//!
//! This is the platform-free part: the looks (`shared/look/flames.json`, generated into
//! `flames.gen.rs`), the billboard's axes on either face, the CPU twin of the shader's shading
//! ([`flame_shade`], [`glow_shade`]) and the WGSL ([`wgsl`]). The renderer (dwell-client
//! render/flames.rs) builds the quads and draws them.

use dwell_world::FaceFrame;

#[rustfmt::skip]
#[path = "flames.gen.rs"]
mod generated;
pub use generated::{FLAME_TUNABLES, FLAMES};

/// `shared/look/flames.json`'s tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameTunables {
    /// Flames farther than this from the eye are not drawn (m).
    pub draw_m: f64,
    /// The most drawn a frame, nearest first.
    pub max_flames: usize,
    /// The most a flame's brightest channel gives off, as the tone curve's input (the lights'
    /// `glow_peak` for flames).
    pub peak: f64,
}

/// A flame's glow: the soft halo round it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameGlow {
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    pub radius_m: f32,
    pub strength: f32,
}

/// A flame's embers: sparks rising from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameEmbers {
    pub count: u32,
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    pub size_m: f32,
    pub rise_m: f32,
    /// Lives a second.
    pub speed: f32,
}

/// A flame look (`flames` in `shared/look/flames.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameLook {
    /// Namespaced name, e.g. `dwell:torch` (a light source's `flame`).
    pub name: &'static str,
    /// sRGB `0xRRGGBB`, from the tips to the core.
    pub ramp: &'static [u32],
    /// The ramp's painted steps (0: smooth).
    pub bands: u32,
    pub height_m: f32,
    pub width_m: f32,
    /// A placed flame's base over the floor of its cell (m).
    pub base_m: f32,
    /// A held flame's size over a placed one's.
    pub held_scale: f32,
    /// Its width at its base, the stick's tip (m; at most `width_m`).
    pub base_width_m: f32,
    /// The noise's rise, cycles a second.
    pub speed: f32,
    pub noise_scale: f32,
    pub distort: f32,
    pub tongues: f32,
    pub sway: f32,
    /// How much its height follows the light's flicker (× the flicker's dip).
    pub size_flicker: f32,
    pub glow: FlameGlow,
    pub embers: FlameEmbers,
}

/// The most colours a ramp holds (`MAX_RAMP` in flames.mjs).
pub const MAX_RAMP: usize = 6;
/// The flame quad's half width over the look's width: room for the noise to bend it sideways.
pub const QUAD_HALF_WIDTH: f32 = 0.8;
/// The flame body's half width at its widest, over the quad's half width (so the body is
/// `width_m` wide at its widest).
const BODY: f32 = 0.625;
/// Where the flame is widest, up its height.
const WIDEST: f32 = 0.32;

/// A flame look's index by name (its id in the shader).
pub fn find(name: &str) -> Option<usize> {
    FLAMES.iter().position(|f| f.name == name)
}

/// A flame's height factor for the light's flicker factor `flicker` (in [1 − amount, 1],
/// `lights::flicker`): the flame shrinks a little as the light dips, `size_flicker` × the dip.
pub fn height_factor(look: &FlameLook, flicker: f64) -> f32 {
    (1.0 - f64::from(look.size_flicker) * (1.0 - flicker)).max(0.2) as f32
}

/// A billboard's axes in the world: `right` across the screen and `up` the face's up as the
/// screen shows it (the face's up projected on the view plane), both unit and on the view plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Billboard {
    pub right: [f64; 3],
    pub up: [f64; 3],
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    a.map(|v| v / l)
}

/// The billboard of a flame on `face` seen along the view's `forward` (unit) with the camera's
/// `camera_up` (unit, on the view plane): written face-local (+y up) and crossed into the world
/// through the face frame. Its up is the face's up projected on the view plane; looking straight
/// down (or up) at a flame, where that vanishes, it turns smoothly to the camera's up.
pub fn billboard(face: FaceFrame, forward: [f64; 3], camera_up: [f64; 3]) -> Billboard {
    let f = face.vector(forward);
    let c = face.vector(camera_up);
    let up = [0.0, 1.0, 0.0];
    let d = dot(up, f);
    let q = [up[0] - d * f[0], up[1] - d * f[1], up[2] - d * f[2]];
    let len = dot(q, q).sqrt();
    // Past 0.4 (the view more than ~24° off the vertical) the projected up alone.
    let s = ((len - 0.1) / 0.3).clamp(0.0, 1.0);
    let lean = 1.0 - s * s * (3.0 - 2.0 * s);
    let up = normalized(std::array::from_fn(|k| q[k] + c[k] * lean));
    let right = normalized(cross(f, up));
    Billboard {
        right: face.vector(right),
        up: face.vector(up),
    }
}

// --- The shading's CPU twin (the WGSL in `wgsl` mirrors it line for line) ---

fn hash(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((iy as u32).wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h as f32 / 4_294_967_295.0
}

fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ix, iy) = (ix as i32, iy as i32);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash(ix, iy, seed);
    let b = hash(ix.wrapping_add(1), iy, seed);
    let c = hash(ix, iy.wrapping_add(1), seed);
    let d = hash(ix.wrapping_add(1), iy.wrapping_add(1), seed);
    (a + (b - a) * ux) * (1.0 - uy) + (c + (d - c) * ux) * uy
}

fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    noise(x, y, seed) * 0.57
        + noise(x * 2.03, y * 2.03, seed.wrapping_add(1)) * 0.29
        + noise(x * 4.01, y * 4.01, seed.wrapping_add(2)) * 0.14
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn linear(c: u32) -> [f32; 3] {
    crate::hex(c).map(|v| crate::srgb_to_linear(v) as f32)
}

/// The colour (linear) at `k` in [0, 1] along a ramp: 0 the tips, 1 the core.
fn ramp(look: &FlameLook, k: f32) -> [f32; 3] {
    let n = look.ramp.len();
    let p = k.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = (p.floor() as usize).min(n - 2);
    let t = p - i as f32;
    let (a, b) = (linear(look.ramp[i]), linear(look.ramp[i + 1]));
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

/// The flame's colour (linear, not premultiplied) and coverage at a point of its quad: `x` across
/// it (−1 to 1), `y` up it (0 its base, 1 its top), at time `t` (s) for the flame `seed`; with
/// `animated` false (a CPU adapter) a still teardrop of the same ramp.
pub fn flame_shade(
    look: &FlameLook,
    x: f32,
    y: f32,
    t: f32,
    seed: u32,
    animated: bool,
) -> ([f32; 3], f32) {
    let s = (seed & 1023) as f32 * 0.37;
    let ts = t * look.speed;
    let ns = look.noise_scale;
    let (n1, sway) = if animated {
        (
            fbm(x * ns * 0.8 + s, y * ns - ts, seed),
            (noise(t * 0.6, s, seed.wrapping_add(5)) - 0.5) * 2.0 * look.sway,
        )
    } else {
        (0.5, 0.0)
    };
    let dx = x - sway * y * y - (n1 - 0.5) * look.distort * y;
    // Rising from the stick's tip: `base_width_m` wide at its base, widening a little to its
    // widest at WIDEST, then tapering to its tongues.
    let neck = (look.base_width_m / look.width_m).min(1.0);
    let top = if y > WIDEST {
        (1.0 - (y - WIDEST) / (1.0 - WIDEST))
            .clamp(0.0, 1.0)
            .powf(0.75)
    } else {
        1.0
    };
    let w = (neck + (1.0 - neck) * smooth(0.0, WIDEST, y)) * top * BODY;
    let u = dx / w.max(1e-4);
    let tg = if animated {
        noise(u * 1.7 + s, y * 1.7 - ts * 1.27, seed.wrapping_add(11)) * 0.75
            + noise(u * 3.4, y * 5.2 - ts * 1.93, seed.wrapping_add(12)) * 0.25
    } else {
        0.0
    };
    let heat = 1.0
        - u.abs()
        - smooth(0.42, 0.68, tg)
            * look.tongues
            * smooth(0.3, 0.85, y)
            * (0.3 + 0.7 * smooth(0.0, 0.6, u.abs() + y - 0.5))
        - 0.15 * smooth(0.6, 1.0, y);
    let a = smooth(0.0, 0.12, heat) * (1.0 - smooth(0.9, 1.0, y)) * smooth(0.0, 0.03, y);
    let mut k = (heat * 1.1 * (1.0 - 0.5 * y) + 0.1 * (1.0 - y)).clamp(0.0, 1.0);
    if look.bands > 0 {
        let b = look.bands as f32;
        let f = k * b;
        let i = f.floor();
        k = ((i + smooth(0.25, 0.75, f - i)) / b).clamp(0.0, 1.0);
    }
    (ramp(look, k), a)
}

/// The glow's light (linear, added to the scene) at a point `p` (m, on the billboard, from the
/// flame's base) of a flame `height` tall (m, its flicker's), with the light's flicker factor
/// `flicker`: a soft halo round the flame's middle.
pub fn glow_shade(look: &FlameLook, p: [f32; 2], height: f32, flicker: f32) -> [f32; 3] {
    let g = look.glow;
    let dx = p[0] / g.radius_m;
    let dy = (p[1] - height * 0.4) * 0.8 / g.radius_m;
    let halo = (-(dx * dx + dy * dy) * 3.0).exp() * g.strength * flicker;
    linear(g.color).map(|v| v * halo)
}

/// Ember `k` of a flame `height` tall at time `t` (s), for the flame `seed`: where it is (m from
/// the flame's base, on the billboard) and how bright (0 to 2): each rises from the flame's upper
/// part, wavering, and fades as it goes. The renderer draws each as a small disc there
/// ([`ember_disc`]).
pub fn ember(look: &FlameLook, k: u32, height: f32, t: f32, seed: u32) -> ([f32; 2], f32) {
    let e = look.embers;
    let life = (t * e.speed + hash(k as i32, 3, seed)).fract();
    let side = (hash(k as i32, 7, seed) - 0.5) * look.width_m * 0.8;
    let ex = side + (life * 8.17 + k as f32).sin() * look.width_m * 0.25;
    let ey = height * 0.6 + life * e.rise_m;
    ([ex, ey], (1.0 - life) * smooth(0.0, 0.15, life) * 2.0)
}

/// An ember's disc at `r` (its distance from the ember's centre over its size): 1 inside, soft
/// to 0 at its edge.
pub fn ember_disc(r: f32) -> f32 {
    1.0 - smooth(0.3, 1.0, r)
}

/// The embers' brightness at a point `p` (m from the flame's base): every ember's disc there.
pub fn embers_shade(look: &FlameLook, p: [f32; 2], height: f32, t: f32, seed: u32) -> f32 {
    (0..look.embers.count)
        .map(|k| {
            let (at, v) = ember(look, k, height, t, seed);
            let d = ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2)).sqrt();
            ember_disc(d / look.embers.size_m) * v
        })
        .sum()
}

/// The glow quad's extent (m) round the flame's base along the billboard: (half width, below,
/// above), the halo's reach.
pub fn glow_extent(look: &FlameLook, height: f32) -> (f32, f32, f32) {
    // The halo is 1 / 0.8 as tall as it is wide.
    let (r, v) = (look.glow.radius_m * 1.3, look.glow.radius_m * 1.3 / 0.8);
    (r, (v - height * 0.4).max(0.0), height * 0.4 + v)
}

/// The flames' WGSL: the looks (`flame_look`, one case per look), the noise and the shading,
/// mirroring [`flame_shade`], [`glow_shade`] and [`ember_disc`]. `animated` false is a CPU adapter's still flame.
pub fn wgsl(animated: bool) -> String {
    let v3 = |c: [f32; 3]| format!("vec3f({:?}, {:?}, {:?})", c[0], c[1], c[2]);
    let mut cases = String::new();
    for (i, f) in FLAMES.iter().enumerate() {
        let mut ramp: Vec<String> = f.ramp.iter().map(|&c| v3(linear(c))).collect();
        while ramp.len() < MAX_RAMP {
            ramp.push(v3([0.0; 3]));
        }
        cases.push_str(&format!(
            "        case {i}u: {{ return FlameLook(array<vec3f, {MAX_RAMP}>({}), {}u, {}u, vec4f({:?}, {:?}, {:?}, {:?}), vec4f({:?}, {:?}, {:?}, {:?}), vec4f({}, {:?}), vec4f({}, {:?}), vec4f({:?}, {:?}, {:?}, {}.0)); }}\n",
            ramp.join(", "),
            f.ramp.len(),
            f.bands,
            f.height_m,
            f.width_m,
            f.speed,
            f.noise_scale,
            f.distort,
            f.tongues,
            f.sway,
            (f.base_width_m / f.width_m).min(1.0),
            v3(linear(f.glow.color)).trim_start_matches("vec3f(").trim_end_matches(')'),
            f.glow.radius_m,
            v3(linear(f.embers.color)).trim_start_matches("vec3f(").trim_end_matches(')'),
            f.glow.strength,
            f.embers.size_m,
            f.embers.rise_m,
            f.embers.speed,
            f.embers.count,
        ));
    }
    format!(
        r"
const FLAME_ANIMATED: bool = {animated};
const FLAME_BODY: f32 = {BODY:?};
const FLAME_WIDEST: f32 = {WIDEST:?};

// A flame look (dwell-look flames.rs `FlameLook`): its ramp (linear, tips to core) and its count,
// its bands; size (height, width), speed and noise scale; distort, tongues, sway and the base's
// share of the width; the glow's
// colour and radius; the embers' colour and the glow's strength; the embers' size, rise, speed and
// count.
struct FlameLook {{
    ramp: array<vec3f, {MAX_RAMP}>,
    count: u32,
    bands: u32,
    shape: vec4f,
    bend: vec4f,
    glow: vec4f,
    ember: vec4f,
    embers: vec4f,
}};

fn flame_look(i: u32) -> FlameLook {{
    switch i {{
{cases}        default: {{ return flame_look_default(); }}
    }}
}}

fn flame_look_default() -> FlameLook {{
    var l: FlameLook;
    l.count = 2u;
    l.shape = vec4f(0.3, 0.2, 1.0, 1.0);
    l.glow = vec4f(0.0, 0.0, 0.0, 0.1);
    return l;
}}

fn flame_hash(ix: i32, iy: i32, seed: u32) -> f32 {{
    var h = bitcast<u32>(ix) * 0x8da6b343u + bitcast<u32>(iy) * 0xd8163841u + seed * 0xcb1ab31fu;
    h ^= h >> 15u;
    h *= 0x2c1b3c6du;
    h ^= h >> 12u;
    h *= 0x297a2d39u;
    h ^= h >> 15u;
    return f32(h) / 4294967295.0;
}}

fn flame_noise(x: f32, y: f32, seed: u32) -> f32 {{
    let fl = floor(vec2f(x, y));
    let f = vec2f(x, y) - fl;
    let ix = i32(fl.x);
    let iy = i32(fl.y);
    let u = f * f * (3.0 - 2.0 * f);
    let a = flame_hash(ix, iy, seed);
    let b = flame_hash(ix + 1, iy, seed);
    let c = flame_hash(ix, iy + 1, seed);
    let d = flame_hash(ix + 1, iy + 1, seed);
    return (a + (b - a) * u.x) * (1.0 - u.y) + (c + (d - c) * u.x) * u.y;
}}

fn flame_fbm(x: f32, y: f32, seed: u32) -> f32 {{
    return flame_noise(x, y, seed) * 0.57 + flame_noise(x * 2.03, y * 2.03, seed + 1u) * 0.29
        + flame_noise(x * 4.01, y * 4.01, seed + 2u) * 0.14;
}}

fn flame_ramp(look: FlameLook, k: f32) -> vec3f {{
    var ramp = look.ramp;
    let n = look.count;
    let p = clamp(k, 0.0, 1.0) * f32(n - 1u);
    let i = min(u32(floor(p)), n - 2u);
    return mix(ramp[i], ramp[i + 1u], p - f32(i));
}}

// The flame's colour (linear) and coverage at (x across −1 to 1, y up 0 to 1) of its quad at time
// t (s): `flame_shade` in flames.rs.
fn flame_shade(look: FlameLook, x: f32, y: f32, t: f32, seed: u32) -> vec4f {{
    let s = f32(seed & 1023u) * 0.37;
    let ts = t * look.shape.z;
    let ns = look.shape.w;
    var n1 = 0.5;
    var sway = 0.0;
    if (FLAME_ANIMATED) {{
        n1 = flame_fbm(x * ns * 0.8 + s, y * ns - ts, seed);
        sway = (flame_noise(t * 0.6, s, seed + 5u) - 0.5) * 2.0 * look.bend.z;
    }}
    let dx = x - sway * y * y - (n1 - 0.5) * look.bend.x * y;
    var top = 1.0;
    if (y > FLAME_WIDEST) {{
        top = pow(clamp(1.0 - (y - FLAME_WIDEST) / (1.0 - FLAME_WIDEST), 0.0, 1.0), 0.75);
    }}
    let w = (look.bend.w + (1.0 - look.bend.w) * smoothstep(0.0, FLAME_WIDEST, y)) * top * FLAME_BODY;
    let u = dx / max(w, 1e-4);
    var tg = 0.0;
    if (FLAME_ANIMATED) {{
        tg = flame_noise(u * 1.7 + s, y * 1.7 - ts * 1.27, seed + 11u) * 0.75
            + flame_noise(u * 3.4, y * 5.2 - ts * 1.93, seed + 12u) * 0.25;
    }}
    let heat = 1.0 - abs(u)
        - smoothstep(0.42, 0.68, tg) * look.bend.y * smoothstep(0.3, 0.85, y) * (0.3 + 0.7 * smoothstep(0.0, 0.6, abs(u) + y - 0.5))
        - 0.15 * smoothstep(0.6, 1.0, y);
    let a = smoothstep(0.0, 0.12, heat) * (1.0 - smoothstep(0.9, 1.0, y)) * smoothstep(0.0, 0.03, y);
    var k = clamp(heat * 1.1 * (1.0 - 0.5 * y) + 0.1 * (1.0 - y), 0.0, 1.0);
    if (look.bands > 0u) {{
        let b = f32(look.bands);
        let f = k * b;
        let i = floor(f);
        k = clamp((i + smoothstep(0.25, 0.75, f - i)) / b, 0.0, 1.0);
    }}
    return vec4f(flame_ramp(look, k), a);
}}

// The glow's light (linear) at p (m from the flame's base) of a flame `height` tall with flicker
// factor `flicker`: `glow_shade` in flames.rs.
fn flame_glow(look: FlameLook, p: vec2f, height: f32, flicker: f32) -> vec3f {{
    let r = look.glow.w;
    let d = vec2f(p.x / r, (p.y - height * 0.4) * 0.8 / r);
    return look.glow.rgb * (exp(-dot(d, d) * 3.0) * look.ember.w * flicker);
}}

// An ember's light (linear) at `uv` (−1 to 1 across its quad) of brightness `v`: `ember_disc`.
fn flame_ember(look: FlameLook, uv: vec2f, v: f32) -> vec3f {{
    return look.ember.rgb * ((1.0 - smoothstep(0.3, 1.0, length(uv))) * v);
}}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torch() -> &'static FlameLook {
        &FLAMES[find("dwell:torch").expect("the torch's flame")]
    }

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    #[test]
    fn the_shader_is_valid_wgsl_in_both_tiers() {
        for animated in [true, false] {
            validate(&wgsl(animated));
        }
    }

    #[test]
    fn every_look_is_a_case_of_the_shader() {
        let src = wgsl(true);
        for i in 0..FLAMES.len() {
            assert!(src.contains(&format!("case {i}u:")), "look {i}");
        }
    }

    /// The hue (degrees) of a linear colour, by its sRGB.
    fn hue(c: [f32; 3]) -> f32 {
        let [r, g, b] = c.map(|v| {
            if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        });
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        if mx - mn < 1e-6 {
            return 0.0;
        }
        let h = if mx == r {
            ((g - b) / (mx - mn)).rem_euclid(6.0)
        } else if mx == g {
            (b - r) / (mx - mn) + 2.0
        } else {
            (r - g) / (mx - mn) + 4.0
        };
        h * 60.0
    }

    #[test]
    fn the_flame_stays_warm_and_fades_out_at_its_edges() {
        let look = torch();
        for animated in [true, false] {
            let mut covered = 0;
            for frame in 0..24 {
                let t = frame as f32 / 12.0;
                for iy in 0..40 {
                    for ix in 0..32 {
                        let x = (ix as f32 + 0.5) / 16.0 - 1.0;
                        let y = (iy as f32 + 0.5) / 40.0;
                        let (c, a) = flame_shade(look, x, y, t, 7, animated);
                        assert!((0.0..=1.0).contains(&a));
                        if a > 0.05 {
                            covered += 1;
                            // Red through orange to pale yellow: never green, blue or purple.
                            let h = hue(c);
                            assert!(h <= 60.0, "hue {h} at ({x}, {y}), t {t}");
                            assert!(c[0] >= c[1] && c[1] >= c[2], "warm: {c:?}");
                        }
                    }
                }
                // Out of the body: the quad's edges, its top corner and below its base are clear.
                for (x, y) in [
                    (-1.0, 0.5),
                    (1.0, 0.5),
                    (0.95, 0.95),
                    (-0.95, 0.05),
                    (0.0, 1.0),
                ] {
                    assert!(
                        flame_shade(look, x, y, t, 7, animated).1 < 0.01,
                        "({x}, {y}) is clear"
                    );
                }
            }
            assert!(
                covered > 24 * 200,
                "the flame covers its quad's middle ({covered})"
            );
            // The core low in the flame is the ramp's yellow end.
            let (core, a) = flame_shade(look, 0.0, 0.2, 0.0, 7, animated);
            assert!(a > 0.99);
            assert!(core[0] > 0.9 && core[1] > 0.5, "a yellow core: {core:?}");
        }
    }

    #[test]
    fn the_flame_moves_and_flames_do_not_move_together() {
        let look = torch();
        let sample = |t: f32, seed: u32| -> Vec<f32> {
            (0..200)
                .map(|i| {
                    flame_shade(
                        look,
                        (i % 20) as f32 / 10.0 - 0.95,
                        (i / 20) as f32 / 10.0 + 0.05,
                        t,
                        seed,
                        true,
                    )
                    .1
                })
                .collect()
        };
        let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>();
        assert!(diff(&sample(0.0, 7), &sample(0.25, 7)) > 3.0, "it moves");
        assert!(
            diff(&sample(0.0, 7), &sample(0.0, 8)) > 3.0,
            "two torches differ"
        );
        // A still flame (a CPU adapter) does not move.
        let still = |t: f32| flame_shade(look, 0.3, 0.5, t, 7, false);
        assert_eq!(still(0.0), still(1.3));
    }

    #[test]
    fn the_glow_follows_the_flicker_and_embers_rise() {
        let look = torch();
        let at = |p: [f32; 2], f: f32| glow_shade(look, p, look.height_m, f);
        let full = at([0.0, look.height_m * 0.4], 1.0);
        let dim = at([0.0, look.height_m * 0.4], 0.84);
        assert!(dim[0] < full[0] && (dim[0] / full[0] - 0.84).abs() < 1e-4);
        assert!(
            at([look.glow.radius_m * 1.5, 0.0], 1.0)[0] < full[0] * 0.05,
            "the halo is soft and small"
        );
        let (half, below, above) = glow_extent(look, look.height_m);
        for p in [[half, 0.0], [0.0, -below], [0.0, above]] {
            assert!(
                at(p, 1.0)[0] < full[0] * 0.02,
                "the quad holds the halo: {p:?}"
            );
        }
        // Each ember rises from the flame's upper part over its life and fades out.
        for k in 0..look.embers.count {
            let (start, _) = ember(look, k, look.height_m, 0.0, 7);
            let mut highest = start[1];
            let mut brightest = 0.0f32;
            for f in 0..60 {
                let (p, v) = ember(
                    look,
                    k,
                    look.height_m,
                    f as f32 / 60.0 / look.embers.speed,
                    7,
                );
                assert!(
                    p[1] >= look.height_m * 0.6 - 1e-6
                        && p[1] <= look.height_m * 0.6 + look.embers.rise_m + 1e-6
                );
                assert!((0.0..=2.0).contains(&v));
                highest = highest.max(p[1]);
                brightest = brightest.max(v);
            }
            assert!(
                highest > look.height_m * 0.6 + look.embers.rise_m * 0.8,
                "ember {k} rises"
            );
            assert!(brightest > 1.0, "ember {k} shows");
        }
        assert!(ember_disc(0.0) == 1.0 && ember_disc(1.0) == 0.0);
        let (p, v) = ember(look, 0, look.height_m, 0.3, 7);
        assert!((embers_shade(look, p, look.height_m, 0.3, 7) - v).abs() < 0.2 + v * 0.01);
    }

    #[test]
    fn the_flame_shrinks_as_the_light_dips() {
        let look = torch();
        assert_eq!(height_factor(look, 1.0), 1.0);
        assert!(height_factor(look, 0.84) < 1.0 && height_factor(look, 0.84) > 0.85);
    }

    dwell_world::both_faces!(
        fn the_billboard_faces_the_eye_with_the_face_up(f: FaceFrame) {
            // Face-local views: level, tilted down at the flame, and straight down.
            let views: [([f64; 3], [f64; 3]); 3] = [
                ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
                (normalized([0.0, -0.6, -0.8]), normalized([0.0, 0.8, -0.6])),
                ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0]),
            ];
            for (fwd, cam_up) in views {
                let b = billboard(f, f.vector(fwd), f.vector(cam_up));
                let a = billboard(FaceFrame::A, fwd, cam_up);
                // Face B's billboard is face A's mirrored through the frame.
                for k in 0..3 {
                    assert!((b.up[k] - f.vector(a.up)[k]).abs() < 1e-12);
                    assert!((b.right[k] - f.vector(a.right)[k]).abs() < 1e-12);
                }
                // On the view plane, unit and square.
                let w = f.vector(fwd);
                assert!(dot(b.up, w).abs() < 1e-9 && dot(b.right, w).abs() < 1e-9);
                assert!((dot(b.up, b.up) - 1.0).abs() < 1e-9 && dot(b.up, b.right).abs() < 1e-9);
                // Its up leans toward the face's up (never away), screen-up when level.
                assert!(dot(b.up, f.up()) >= -1e-9);
            }
            let level = billboard(f, f.vector([0.0, 0.0, -1.0]), f.vector([0.0, 1.0, 0.0]));
            assert!(
                (dot(level.up, f.up()) - 1.0).abs() < 1e-12,
                "level: the face's up"
            );
        }
    );
}
