//! The sky's clouds (docs/CLOUDS.md, ADR 0034): a thin cloud deck at a fixed altitude, drawn as part
//! of the sky (not 3D). Each sky pixel's view ray meets the deck — a sphere of radius `curvature_m`
//! around a point that far below the eye, `altitude_m` above sea level — and the hit point's
//! horizontal world position drives a domain-warped fBm of value noise. So the clouds have true
//! perspective: large overhead, compressed toward the horizon, which the deck's curve meets at a
//! finite distance; they move past slowly as the eye flies under them.
//!
//! The tunables are data (`shared/look/clouds.json` → `clouds.gen.rs`); the shader is generated
//! here from them ([`cloud_wgsl`]), as the sky's is. The noise repeats every `wrap_m` metres (a
//! whole number of its largest cells, with an octave transform that keeps the lattice), so the
//! renderer wraps the eye's and the wind's offsets into one period in f64 and the shader's f32
//! coordinates stay small anywhere in the world. Client-side visuals only: no network, no
//! simulation.

use crate::{MOON_GLOW, Rgb, SUN_GLOW, TWILIGHT};

/// Where the deck is and how it fades (`clouds.json`'s `deck`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckTunables {
    pub altitude_m: f64,
    pub curvature_m: f64,
    pub ceiling_fade_m: f64,
    pub horizon_fade: f64,
    pub fade_start_m: f64,
    pub fade_end_m: f64,
}

/// The noise and its remapping to cloud (`shape`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeTunables {
    pub scale_m: f64,
    pub wrap_m: f64,
    pub octaves: u32,
    pub low_octaves: u32,
    pub gain: f64,
    pub warp: f64,
    pub coverage: f64,
    pub edge_softness: f64,
    pub opacity: f64,
}

/// The drift (`wind`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindTunables {
    pub speed_mps: f64,
    pub direction_deg: f64,
    pub evolve_mps: f64,
    pub evolve_direction_deg: f64,
}

/// The lighting (`light`): sRGB colours and weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudLight {
    pub sunlit: Rgb,
    pub shade: Rgb,
    pub sunset: Rgb,
    pub night_lit: Rgb,
    pub night_shade: Rgb,
    pub absorption: f64,
    pub shadow_step_m: f64,
    pub silver_lining: f64,
    pub silver_power: f64,
    pub sun_warmth: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudTunables {
    pub deck: DeckTunables,
    pub shape: ShapeTunables,
    pub wind: WindTunables,
    pub light: CloudLight,
}

pub use crate::clouds_gen::CLOUDS;

/// The most octaves the shader loops over (`clouds.mjs`'s MAX_OCTAVES).
pub const MAX_OCTAVES: u32 = 8;
/// How far the octave transform scales: `[[2, 1], [−1, 2]]` maps the integer lattice onto itself
/// (so every octave repeats with the first) and turns each octave 26.6° off the last, which hides
/// the lattice's axes.
pub const LACUNARITY: f64 = 2.236_067_977_499_79;
/// The fBm's spread about its mean is narrow: it is stretched by this before the coverage cut, so
/// `coverage` is close to the share of the sky covered.
const CONTRAST: f64 = 1.9;

/// The Clouds setting: Off, Low (fewer octaves, no self-shadow) or High.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudQuality {
    Off,
    Low,
    High,
}

impl CloudQuality {
    /// From the settings store's name (`off`, `low`, `high`); anything else is None.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }
    pub fn octaves(self, t: &CloudTunables) -> u32 {
        match self {
            Self::Off => 0,
            Self::Low => t.shape.low_octaves,
            Self::High => t.shape.octaves,
        }
    }
    pub fn self_shadow(self) -> bool {
        self == Self::High
    }
}

/// The distance along a unit view ray to the deck, for a ray whose dot with the viewer's up is
/// `above`, with the deck `height` metres above the eye on a sphere of radius `curvature`: the
/// positive root of |r·up + t·dir| = r + h, in the form that doesn't cancel (h straight up). None
/// when the eye is at or above the deck.
pub fn deck_distance(above: f64, height: f64, curvature: f64) -> Option<f64> {
    if height <= 0.0 {
        return None;
    }
    let c = 2.0 * curvature * height + height * height;
    let ry = curvature * above;
    Some(c / (ry + (ry * ry + c).sqrt()))
}

/// The deck's height above the eye: the eye's height above sea level on its own face.
pub fn deck_height(t: &CloudTunables, eye_y: f64, face: f64) -> f64 {
    let local = if face > 0.0 {
        eye_y
    } else {
        2.0 * crate::MIDPLANE_Y - eye_y
    };
    t.deck.altitude_m - (local - dwell_protocol::world::SEA_LEVEL as f64)
}

fn heading(deg: f64) -> [f64; 2] {
    let r = deg.to_radians();
    [r.cos(), r.sin()]
}

/// The clouds' per-frame uniforms: `offsets` (the noise point under the eye, and the warp field's
/// offset from it, in cells, each wrapped into one period) and `cfg` (coverage, octaves, 1 with the
/// self-shadow, the deck's height above the eye). `seconds` is the clouds' clock (world time where
/// there is one). Face B's clouds are another part of the noise (half a period away).
pub fn cloud_uniforms(
    t: &CloudTunables,
    eye: [f64; 3],
    face: f64,
    seconds: f64,
    quality: CloudQuality,
    coverage: f64,
) -> [[f32; 4]; 2] {
    let wrap = t.shape.wrap_m;
    let [wx, wz] = heading(t.wind.direction_deg).map(|c| c * t.wind.speed_mps * seconds);
    let [ex, ez] = heading(t.wind.evolve_direction_deg).map(|c| c * t.wind.evolve_mps * seconds);
    let side = if face < 0.0 { wrap / 2.0 } else { 0.0 };
    let cells = |m: f64| (m.rem_euclid(wrap) / t.shape.scale_m) as f32;
    [
        [
            cells(eye[0] - wx + side),
            cells(eye[2] - wz + side),
            cells(wx - ex),
            cells(wz - ez),
        ],
        [
            coverage.clamp(0.0, 1.0) as f32,
            quality.octaves(t) as f32,
            if quality.self_shadow() { 1.0 } else { 0.0 },
            deck_height(t, eye[1], face) as f32,
        ],
    ]
}

// ----- The noise, on the CPU (the shader's twin, for tests and tools) -----

/// The lattice's period in cells.
pub fn period(t: &CloudTunables) -> i64 {
    (t.shape.wrap_m / t.shape.scale_m).round() as i64
}

fn hash(x: i64, y: i64, period: i64) -> f64 {
    let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / 4_294_967_295.0
}

fn value(p: [f64; 2], period: i64) -> f64 {
    let (i, j) = (p[0].floor(), p[1].floor());
    let f = [p[0] - i, p[1] - j];
    let u = f.map(|f| f * f * f * (f * (f * 6.0 - 15.0) + 10.0));
    let (i, j) = (i as i64, j as i64);
    let a = hash(i, j, period);
    let b = hash(i + 1, j, period);
    let c = hash(i, j + 1, period);
    let d = hash(i + 1, j + 1, period);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    lerp(lerp(a, b, u[0]), lerp(c, d, u[0]), u[1])
}

fn next_octave(p: [f64; 2]) -> [f64; 2] {
    [2.0 * p[0] + p[1] + 13.0, -p[0] + 2.0 * p[1] + 7.0]
}

fn fbm(t: &CloudTunables, mut p: [f64; 2], octaves: u32) -> f64 {
    let n = period(t);
    let (mut amp, mut sum, mut norm) = (0.5, 0.0, 0.0);
    for _ in 0..octaves {
        sum += amp * value(p, n);
        norm += amp;
        amp *= t.shape.gain;
        p = next_octave(p);
    }
    sum / norm
}

/// The warped point: `p` moved by two two-octave fields sampled `warp_off` cells from it.
fn warped(t: &CloudTunables, p: [f64; 2], warp_off: [f64; 2]) -> [f64; 2] {
    let w = [p[0] + warp_off[0], p[1] + warp_off[1]];
    let qx = fbm(t, w, 2) - 0.5;
    let qy = fbm(t, [w[0] + 5.2, w[1] + 1.3], 2) - 0.5;
    [
        p[0] + 2.0 * t.shape.warp * qx,
        p[1] + 2.0 * t.shape.warp * qy,
    ]
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn shape(t: &CloudTunables, n: f64, coverage: f64) -> f64 {
    let n = ((n - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0);
    let cut = 1.0 - coverage;
    if coverage <= 0.0 {
        return 0.0;
    }
    smoothstep(cut, cut + t.shape.edge_softness, n)
}

/// The cloud's density (0–1) at the noise point `p` (cells; `cloud_uniforms`' offsets plus the
/// hit's horizontal offset over `scale_m`).
pub fn density(
    t: &CloudTunables,
    p: [f64; 2],
    warp_off: [f64; 2],
    octaves: u32,
    coverage: f64,
) -> f64 {
    if octaves == 0 {
        return 0.0;
    }
    shape(t, fbm(t, warped(t, p, warp_off), octaves), coverage)
}

// ----- The shader -----

fn f(n: f64) -> String {
    format!("{n:.6}")
}
fn vec3(c: Rgb) -> String {
    format!("vec3f({}, {}, {})", f(c[0]), f(c[1]), f(c[2]))
}

/// The clouds for shaders, generated from `t`:
///
/// - `fn dwell_cloud_deck(dir: vec3f, face: f32, offsets: vec4f, height: f32) -> vec4f`: where
///   the view ray `dir` meets the deck: the noise point (xy, cells), the distance (z) and how much
///   of the cloud there shows (w: 0 below the horizon, past the fade, or with the eye at the deck).
///   Call it in uniform control flow and take `fwidth` of its xy (the pixel's footprint in cells).
/// - `fn dwell_clouds(clear: vec3f, dir: vec3f, face: f32, deck: vec4f, fw: f32, offsets: vec4f,
///   cfg: vec4f, lights: CloudLighting) -> vec3f`, with `CloudLighting` the sky frame and the
///   lights' colours from the renderer's frame (the data's colours are tints of those): the sky colour `clear`
///   (sRGB, from `dwell_sky`) with the clouds over it.
///
/// `offsets` and `cfg` are [`cloud_uniforms`]'s.
pub fn cloud_wgsl(t: &CloudTunables) -> String {
    let d = &t.deck;
    let s = &t.shape;
    let l = &t.light;
    let consts = [
        format!("const CLOUD_PERIOD: i32 = {};", period(t)),
        format!("const CLOUD_MAX_OCTAVES: i32 = {MAX_OCTAVES};"),
        format!("const CLOUD_SCALE: f32 = {};", f(s.scale_m)),
        format!("const CLOUD_CURVATURE: f32 = {};", f(d.curvature_m)),
        format!("const CLOUD_CEILING_FADE: f32 = {};", f(d.ceiling_fade_m)),
        format!("const CLOUD_HORIZON_FADE: f32 = {};", f(d.horizon_fade)),
        format!("const CLOUD_FADE_START: f32 = {};", f(d.fade_start_m)),
        format!("const CLOUD_FADE_END: f32 = {};", f(d.fade_end_m)),
        format!("const CLOUD_GAIN: f32 = {};", f(s.gain)),
        format!("const CLOUD_WARP: f32 = {};", f(2.0 * s.warp)),
        format!("const CLOUD_LACUNARITY: f32 = {};", f(LACUNARITY)),
        format!("const CLOUD_CONTRAST: f32 = {};", f(CONTRAST)),
        format!("const CLOUD_SOFTNESS: f32 = {};", f(s.edge_softness)),
        format!("const CLOUD_OPACITY: f32 = {};", f(s.opacity)),
        format!("const CLOUD_SUNLIT: vec3f = {};", vec3(l.sunlit)),
        format!("const CLOUD_SHADE: vec3f = {};", vec3(l.shade)),
        format!("const CLOUD_SUNSET: vec3f = {};", vec3(l.sunset)),
        format!("const CLOUD_NIGHT_LIT: vec3f = {};", vec3(l.night_lit)),
        format!("const CLOUD_NIGHT_SHADE: vec3f = {};", vec3(l.night_shade)),
        format!("const CLOUD_SUN_GLOW: vec3f = {};", vec3(SUN_GLOW.color)),
        format!("const CLOUD_MOON_GLOW: vec3f = {};", vec3(MOON_GLOW.color)),
        format!("const CLOUD_ABSORPTION: f32 = {};", f(l.absorption)),
        format!(
            "const CLOUD_SHADOW_STEP: f32 = {};",
            f(l.shadow_step_m / s.scale_m)
        ),
        format!("const CLOUD_SILVER: f32 = {};", f(l.silver_lining)),
        format!("const CLOUD_SILVER_POWER: f32 = {};", f(l.silver_power)),
        format!("const CLOUD_SUN_WARMTH: f32 = {};", f(l.sun_warmth)),
        format!("const CLOUD_TWILIGHT: f32 = {};", f(TWILIGHT)),
    ];
    format!("{}\n{CLOUD_FNS}", consts.join("\n"))
}

const CLOUD_FNS: &str = r"
fn cloud_hash(c: vec2i) -> f32 {
	let w = vec2u(((c % CLOUD_PERIOD) + CLOUD_PERIOD) % CLOUD_PERIOD);
	var h = (w.x * 0x8da6b343u) ^ (w.y * 0xd8163841u);
	h = (h ^ (h >> 16u)) * 0x7feb352du;
	h = (h ^ (h >> 15u)) * 0x846ca68bu;
	h ^= h >> 16u;
	return f32(h) / 4294967295.0;
}

fn cloud_value(p: vec2f) -> f32 {
	let i = floor(p);
	let f = p - i;
	let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
	let c = vec2i(i);
	let a = cloud_hash(c);
	let b = cloud_hash(c + vec2i(1, 0));
	let d = cloud_hash(c + vec2i(0, 1));
	let e = cloud_hash(c + vec2i(1, 1));
	return mix(mix(a, b, u.x), mix(d, e, u.x), u.y);
}

// The fBm, each octave faded to its mean as it shrinks below the pixel (`fw`: the pixel's
// footprint in cells), so the far deck doesn't shimmer.
fn cloud_fbm(p_in: vec2f, octaves: i32, fw: f32) -> f32 {
	var p = p_in;
	var amp = 0.5;
	var sum = 0.0;
	var norm = 0.0;
	var foot = fw;
	for (var i = 0; i < CLOUD_MAX_OCTAVES; i++) {
		if (i >= octaves) {
			break;
		}
		let keep = 1.0 - smoothstep(0.25, 0.6, foot);
		sum += amp * mix(0.5, cloud_value(p), keep);
		norm += amp;
		amp *= CLOUD_GAIN;
		p = vec2f(2.0 * p.x + p.y + 13.0, -p.x + 2.0 * p.y + 7.0);
		foot *= CLOUD_LACUNARITY;
	}
	return sum / norm;
}

fn cloud_warped(p: vec2f, warp_off: vec2f, fw: f32) -> vec2f {
	let w = p + warp_off;
	let q = vec2f(cloud_fbm(w, 2, fw), cloud_fbm(w + vec2f(5.2, 1.3), 2, fw)) - 0.5;
	return p + CLOUD_WARP * q;
}

fn cloud_shape(n_in: f32, coverage: f32) -> f32 {
	let n = clamp((n_in - 0.5) * CLOUD_CONTRAST + 0.5, 0.0, 1.0);
	let cut = 1.0 - coverage;
	return select(0.0, smoothstep(cut, cut + CLOUD_SOFTNESS, n), coverage > 0.0);
}

fn dwell_cloud_deck(dir: vec3f, face: f32, offsets: vec4f, height: f32) -> vec4f {
	let above = dir.y * face;
	let h = max(height, 1e-3);
	let c = 2.0 * CLOUD_CURVATURE * h + h * h;
	let ry = CLOUD_CURVATURE * above;
	let t = c / (ry + sqrt(ry * ry + c));
	let q = offsets.xy + dir.xz * (t / CLOUD_SCALE);
	let shown = smoothstep(0.0, CLOUD_HORIZON_FADE, above)
		* smoothstep(0.0, CLOUD_CEILING_FADE, height)
		* (1.0 - smoothstep(CLOUD_FADE_START, CLOUD_FADE_END, t));
	return vec4f(q, t, shown);
}

// The light the clouds take, from the renderer's frame (so they follow the sky frame and the
// lights, whatever sets them): the sky frame's directions, and the sun's, moon's and each sky's
// ambient light colours (linear, any intensity: only their hue is used).
struct CloudLighting {
	day_pole: vec3f,
	sun: vec3f,
	moon: vec3f,
	sun_color: vec3f,
	moon_color: vec3f,
	day_ambient: vec3f,
	night_ambient: vec3f,
};

// A light's hue for display: its linear colour scaled to a brightest channel of 1, encoded.
fn cloud_hue(c: vec3f) -> vec3f {
	return pow(c / max(max(c.r, max(c.g, c.b)), 1e-4), vec3f(1.0 / 2.2));
}

fn dwell_clouds(clear: vec3f, dir: vec3f, face: f32, deck: vec4f, fw: f32, offsets: vec4f, cfg: vec4f, lights: CloudLighting) -> vec3f {
	let day_pole = lights.day_pole;
	let sun = lights.sun;
	let moon = lights.moon;
	let octaves = i32(cfg.y);
	if (octaves <= 0 || deck.w <= 0.0) {
		return clear;
	}
	let p = cloud_warped(deck.xy, offsets.zw, fw);
	let d = cloud_shape(cloud_fbm(p, octaves, fw), cfg.x);
	if (d <= 0.0) {
		return clear;
	}
	let up = vec3f(0.0, face, 0.0);
	// The light: the sun or the moon, whichever is above this face.
	let sun_up = dot(sun, up);
	let light = select(moon, sun, sun_up >= dot(moon, up));
	// Self-shadow (High): the density a step toward the light, plus the cloud's own thickness.
	var occl = 0.6 * d;
	if (cfg.z > 0.0) {
		let toward = light.xz * CLOUD_SHADOW_STEP;
		occl = 0.35 * d + cloud_shape(cloud_fbm(p + toward, octaves, fw), cfg.x);
	}
	let lit = exp(-CLOUD_ABSORPTION * occl);
	// Day or night as the sky behind is: the sphere's gradient runs by the direction's dot with the
	// day zenith, so a turned sky can be night on one side and day on the other.
	let dayness = smoothstep(-CLOUD_TWILIGHT, CLOUD_TWILIGHT, dot(up, day_pole))
		* smoothstep(-0.3, 0.1, dot(dir, day_pole));
	var day = mix(CLOUD_SHADE * mix(vec3f(1.0), cloud_hue(lights.day_ambient), 0.5), CLOUD_SUNLIT * cloud_hue(lights.sun_color), lit);
	let low = (1.0 - smoothstep(0.02, 0.4, sun_up)) * CLOUD_SUN_WARMTH;
	day = mix(day, CLOUD_SUNSET * mix(0.75, 1.0, lit), low * lit);
	let night = mix(CLOUD_NIGHT_SHADE * mix(vec3f(1.0), cloud_hue(lights.night_ambient), 0.5), CLOUD_NIGHT_LIT * cloud_hue(lights.moon_color), lit);
	var col = mix(night, day, dayness);
	// The silver lining: thin cloud near the light glows.
	let toward_light = max(dot(dir, light), 0.0);
	let rim = CLOUD_SILVER * pow(toward_light, CLOUD_SILVER_POWER) * (1.0 - d) * (1.0 - d);
	col += rim * mix(CLOUD_MOON_GLOW * 0.5, CLOUD_SUN_GLOW, dayness);
	// Far cloud takes the sky's haze behind it.
	col = mix(col, clear, 0.85 * smoothstep(0.0, CLOUD_FADE_END, deck.z));
	return mix(clear, col, d * CLOUD_OPACITY * deck.w);
}
";

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_shader_is_valid_wgsl() {
        validate(&cloud_wgsl(&CLOUDS));
    }

    #[test]
    fn the_deck_is_its_height_straight_up_and_finite_at_the_horizon() {
        let (h, r) = (1500.0, 120_000.0);
        assert!((deck_distance(1.0, h, r).unwrap() - h).abs() < 1e-9);
        // At the horizon: the tangent's length to the sphere r + h, √(2rh + h²).
        let horizon = deck_distance(0.0, h, r).unwrap();
        assert!((horizon - (2.0 * r * h + h * h).sqrt()).abs() < 1e-6);
        // Farther as the ray drops toward the horizon, and the hit is on the sphere.
        let mut last = 0.0;
        for i in (0..=100).rev() {
            let y = f64::from(i) / 100.0;
            let t = deck_distance(y, h, r).unwrap();
            assert!(t > last);
            last = t;
            let (x, z) = ((1.0 - y * y).sqrt() * t, y * t + r);
            assert!(((x * x + z * z).sqrt() - (r + h)).abs() < 1e-6);
        }
        // A flat deck's 1/sin(e) for a huge radius.
        let flat = deck_distance(0.25, h, 1e12).unwrap();
        assert!((flat - h / 0.25).abs() < 1e-3);
        assert_eq!(deck_distance(1.0, 0.0, r), None);
        assert_eq!(deck_distance(1.0, -10.0, r), None);
    }

    #[test]
    fn the_deck_is_over_each_faces_own_sea_level() {
        let alt = CLOUDS.deck.altitude_m;
        assert_eq!(deck_height(&CLOUDS, 0.0, 1.0), alt);
        assert_eq!(deck_height(&CLOUDS, 300.0, 1.0), alt - 300.0);
        // Face B: the eye 300 m below its sea level's mirror is 300 m up on its own face.
        let mirror = 2.0 * crate::MIDPLANE_Y;
        assert_eq!(deck_height(&CLOUDS, mirror - 300.0, -1.0), alt - 300.0);
    }

    #[test]
    fn the_noise_repeats_seamlessly_every_wrap() {
        let n = period(&CLOUDS) as f64;
        assert_eq!(n * CLOUDS.shape.scale_m, CLOUDS.shape.wrap_m);
        for i in 0..50 {
            let p = [f64::from(i) * 0.731 - 3.0, f64::from(i) * 1.37 + 0.2];
            let w = [0.4, 1.9];
            let at = density(&CLOUDS, p, w, 5, 0.6);
            for shift in [[n, 0.0], [0.0, n], [-n, 2.0 * n]] {
                let q = [p[0] + shift[0], p[1] + shift[1]];
                assert!((density(&CLOUDS, q, w, 5, 0.6) - at).abs() < 1e-9);
            }
        }
    }

    /// The share of a grid of points that is cloud (density over a half).
    fn cover(t: &CloudTunables, coverage: f64, octaves: u32) -> f64 {
        let n = 60;
        let mut c = 0;
        for i in 0..n {
            for j in 0..n {
                let p = [f64::from(i) * 0.53, f64::from(j) * 0.53];
                c += usize::from(density(t, p, [0.0, 0.0], octaves, coverage) > 0.5);
            }
        }
        c as f64 / f64::from(n * n)
    }

    #[test]
    fn coverage_is_roughly_the_share_of_sky_covered() {
        assert_eq!(cover(&CLOUDS, 0.0, 5), 0.0);
        let mut last = 0.0;
        for c in [0.2, 0.4, 0.6, 0.8, 1.0] {
            let k = cover(&CLOUDS, c, 5);
            assert!(k >= last, "{c}: {k} < {last}");
            last = k;
        }
        let half = cover(&CLOUDS, 0.5, 5);
        assert!((0.2..0.6).contains(&half), "{half}");
        assert!(cover(&CLOUDS, 1.0, 5) > 0.9);
        assert_eq!(density(&CLOUDS, [1.0, 2.0], [0.0, 0.0], 0, 1.0), 0.0);
    }

    #[test]
    fn a_tunable_changed_by_data_changes_the_clouds() {
        // What a JSON edit does once generated: the shader carries the new value, and the clouds
        // the CPU twin computes change with it.
        let mut t = CLOUDS;
        t.shape.edge_softness = 0.5;
        t.light.shade = [0.1, 0.2, 0.3];
        let src = cloud_wgsl(&t);
        validate(&src);
        assert_ne!(src, cloud_wgsl(&CLOUDS));
        assert!(src.contains("const CLOUD_SOFTNESS: f32 = 0.500000;"));
        assert!(src.contains("const CLOUD_SHADE: vec3f = vec3f(0.100000, 0.200000, 0.300000);"));
        let mut changed = 0;
        for i in 0..40 {
            let p = [f64::from(i) * 0.37, f64::from(i) * 0.91];
            let a = density(&CLOUDS, p, [0.0, 0.0], 5, 0.5);
            let b = density(&t, p, [0.0, 0.0], 5, 0.5);
            changed += usize::from((a - b).abs() > 1e-6);
        }
        assert!(changed > 10);
        // The uniforms follow the data's coverage and octaves.
        let mut t = CLOUDS;
        t.shape.octaves = 7;
        let u = cloud_uniforms(&t, [0.0; 3], 1.0, 0.0, CloudQuality::High, t.shape.coverage);
        assert_eq!(u[1][1], 7.0);
    }

    #[test]
    fn the_quality_setting_sets_octaves_and_the_self_shadow() {
        let u = |q| cloud_uniforms(&CLOUDS, [0.0; 3], 1.0, 0.0, q, 0.4)[1];
        assert_eq!(u(CloudQuality::Off)[1], 0.0);
        assert_eq!(u(CloudQuality::Low)[1], CLOUDS.shape.low_octaves as f32);
        assert_eq!(u(CloudQuality::High)[1], CLOUDS.shape.octaves as f32);
        assert_eq!(u(CloudQuality::Low)[2], 0.0);
        assert_eq!(u(CloudQuality::High)[2], 1.0);
        assert_eq!(u(CloudQuality::High)[0], 0.4);
        const { assert!(CLOUDS.shape.low_octaves < CLOUDS.shape.octaves) };
        for (name, q) in [
            ("off", CloudQuality::Off),
            ("low", CloudQuality::Low),
            ("high", CloudQuality::High),
        ] {
            assert_eq!(CloudQuality::from_name(name), Some(q));
        }
        assert_eq!(CloudQuality::from_name("ultra"), None);
    }

    #[test]
    fn offsets_stay_in_one_period_far_from_the_origin_and_drift_with_the_wind() {
        let n = period(&CLOUDS) as f32;
        let far = [7.9e6, 1200.0, -6.3e6];
        for seconds in [0.0, 1e3, 7e7] {
            let u = cloud_uniforms(&CLOUDS, far, 1.0, seconds, CloudQuality::High, 0.5)[0];
            assert!(u.iter().all(|&c| (0.0..=n).contains(&c)), "{u:?}");
        }
        // Moving the eye by a metre moves the noise point under it by a metre (in cells).
        let a = cloud_uniforms(
            &CLOUDS,
            [100.0, 0.0, 100.0],
            1.0,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        let b = cloud_uniforms(
            &CLOUDS,
            [101.0, 0.0, 100.0],
            1.0,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        assert!(((b[0] - a[0]) * CLOUDS.shape.scale_m as f32 - 1.0).abs() < 1e-3);
        // After a second the pattern has moved speed_mps downwind: the point under the eye is that
        // far upwind.
        let c = cloud_uniforms(
            &CLOUDS,
            [100.0, 0.0, 100.0],
            1.0,
            1.0,
            CloudQuality::High,
            0.5,
        )[0];
        let moved = ((a[0] - c[0]).hypot(a[1] - c[1])) * CLOUDS.shape.scale_m as f32;
        assert!((f64::from(moved) - CLOUDS.wind.speed_mps).abs() < 1e-2);
        // Face B looks at another part of the noise.
        let d = cloud_uniforms(
            &CLOUDS,
            [100.0, -5000.0, 100.0],
            -1.0,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        assert!((d[0] - a[0]).abs() > 1.0);
    }
}
