//! The game's look (WORLD_GENERATION.md §1, Phase 7): warm light, cool shadow, one sky. Everything
//! the meshers, the lights and the fog must agree on lives here as plain data, so the chunk and
//! LOD meshers share it and tests pin it. Colours are sRGB triples in 0–1 unless a name says linear.
//! The sky, the lights and the fog's colour follow the day/night cycle (`daynight`, from
//! `shared/look/daynight.json`).

pub mod clouds;
#[path = "clouds.gen.rs"]
mod clouds_gen;
pub mod daynight;
pub mod fog;
pub mod motion;
pub mod reflections;
#[path = "reflections.gen.rs"]
mod reflections_gen;
pub mod spyglass;

use daynight::{DAY_NIGHT, DayState, Sample};

pub type Rgb = [f64; 3];

pub const fn hex(c: u32) -> Rgb {
    [
        ((c >> 16) & 0xff) as f64 / 255.0,
        ((c >> 8) & 0xff) as f64 / 255.0,
        (c & 0xff) as f64 / 255.0,
    ]
}

// Per-face colour multiplier (linear light): warm on top, cooler on the sides, blue underneath.
// Shaded faces lean blue or violet rather than just darker (§1.2). Chunk and LOD faces both read it.
const TOP: Rgb = [1.0, 0.97, 0.9];
const SIDE_X: Rgb = [0.86, 0.84, 0.88];
const SIDE_Z: Rgb = [0.74, 0.77, 0.9];
const BOTTOM: Rgb = [0.5, 0.55, 0.74];

/// The tint of the face on `axis` (0 X, 1 Y, 2 Z) looking toward `sign` (+1 or −1).
pub fn face_tint(axis: usize, sign: i32) -> Rgb {
    match axis {
        1 if sign > 0 => TOP,
        1 => BOTTOM,
        0 => SIDE_X,
        _ => SIDE_Z,
    }
}

/// The tint of a face with unit normal (nx, ny, nz): the side tints blended by the horizontal part
/// of the normal, then toward the top (or bottom) tint by how vertical it is. Equals `face_tint`
/// on axis-aligned normals.
pub fn normal_tint(nx: f64, ny: f64, nz: f64) -> Rgb {
    let (x2, z2) = (nx * nx, nz * nz);
    let horizontal = x2 + z2;
    let vertical = ny.abs().min(1.0);
    let end = if ny >= 0.0 { TOP } else { BOTTOM };
    let (wx, wz) = if horizontal > 0.0 {
        (x2 / horizontal, z2 / horizontal)
    } else {
        (0.0, 0.0)
    };
    std::array::from_fn(|i| {
        (SIDE_X[i] * wx + SIDE_Z[i] * wz) * (1.0 - vertical) + end[i] * vertical
    })
}

fn dot(a: Rgb, b: Rgb) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Fragments below the midplane belong to face B (lit by the lights above face B).
pub const MIDPLANE_Y: f64 = dwell_protocol::world::MIDPLANE_Y as f64;

/// Tone mapping exposure: 1 leaves the lights as set (the time of day's exposure multiplies it).
pub const DEFAULT_EXPOSURE: f64 = 1.0;
pub const EXPOSURE_MIN: f64 = 0.4;
pub const EXPOSURE_MAX: f64 = 2.5;

/// Clamps an exposure into range; anything missing or not a number takes the default.
pub fn sanitize_exposure(value: Option<f64>) -> f64 {
    match value {
        Some(v) if v.is_finite() => v.clamp(EXPOSURE_MIN, EXPOSURE_MAX),
        _ => DEFAULT_EXPOSURE,
    }
}

/// The most of the sun's glow colour the sky takes, so its disc stays brighter than the sky.
const GLOW_CAP: f64 = 0.75;

/// `2 / σ²` for a Gaussian `σ` degrees wide: `exp((cos θ − 1) · k)` ≈ `exp(−θ²/σ²)`.
fn gauss_k(deg: f64) -> f64 {
    2.0 / deg.to_radians().powi(2)
}

/// A light's halo toward `x` = cos(angle from it): a tight bloom and a broad glow.
fn halo(x: f64, bloom: f64, bloom_deg: f64, glow: f64, glow_deg: f64) -> f64 {
    bloom * ((x - 1.0) * gauss_k(bloom_deg)).exp() + glow * ((x - 1.0) * gauss_k(glow_deg)).exp()
}

/// The sky's colour (sRGB) toward the unit world direction `dir` for a viewer on `face` (+1 A,
/// −1 B) whose look is `s`, under `day`'s sun and moon: the gradient from the horizon to the zenith,
/// a warm band along the horizon toward the sun, the sun's bloom and glow and the moon's. Below the
/// viewer's horizon it is the horizon's colour in that azimuth. The sky behind the world, the haze
/// far terrain fades into (so the fog is the horizon's colour at every time of day) and the sky
/// water reflects are all this function; [`sky_wgsl`]'s `dwell_sky` is its shader twin. The sun's
/// and moon's discs and the stars are drawn over it behind the world only (`dwell_sky_bodies`).
pub fn sky_color(dir: Rgb, face: f64, s: &Sample, day: &DayState) -> Rgb {
    let up = [0.0, face, 0.0];
    let above = dot(dir, up);
    let mut d = dir;
    if above <= 0.0 {
        let level = [dir[0], 0.0, dir[2]];
        let len = dot(level, level).sqrt();
        if len >= 1e-6 {
            d = level.map(|c| c / len);
        }
    }
    let e = above.clamp(0.0, 1.0);
    let shape = &DAY_NIGHT.sky;
    let mut c = mix(
        s.sky_horizon,
        s.sky_zenith,
        e.max(1e-6).powf(shape.gradient_power),
    );
    let sun = day.sun;
    let sh = [sun[0], 0.0, sun[2]];
    let dh = [d[0], 0.0, d[2]];
    let lens = dot(sh, sh).sqrt() * dot(dh, dh).sqrt();
    let toward = if lens > 1e-6 {
        0.5 + 0.5 * dot(dh, sh) / lens
    } else {
        0.5
    };
    let band = s.band_strength
        * toward.max(1e-6).powf(shape.band_spread)
        * (1.0 - smoothstep(0.0, shape.band_height, e));
    c = mix(c, s.band, band.clamp(0.0, 1.0));
    let sl = &DAY_NIGHT.sun;
    let g = s.glow_strength * halo(dot(d, sun), sl.bloom, sl.bloom_deg, sl.glow, sl.glow_deg);
    c = mix(c, s.glow, g.clamp(0.0, GLOW_CAP));
    let ml = &DAY_NIGHT.moon;
    let m = moon_glow(s, day)
        * halo(
            dot(d, day.moon),
            ml.bloom,
            ml.bloom_deg,
            ml.glow,
            ml.glow_deg,
        );
    mix(c, ml.glow_color, m.clamp(0.0, 1.0))
}

/// The moon's glow strength: the keyframe's, by how much of the disc is lit (a crescent still
/// glows a little).
pub fn moon_glow(s: &Sample, day: &DayState) -> f64 {
    s.moon_glow * day.moon_lit.max(0.25)
}

fn f(n: f64) -> String {
    format!("{n:.6}")
}
fn vec3(c: Rgb) -> String {
    format!("vec3f({}, {}, {})", f(c[0]), f(c[1]), f(c[2]))
}

/// The sky for shaders, generated from the day/night data's fixed shape (the keyframed values come
/// in the `DaySky` uniform block, [`daynight::DAY_SKY_WGSL`], which this needs before it):
///
/// - `fn dwell_sky(dir: vec3f, face: f32, day: DaySky) -> vec3f`: [`sky_color`] (the fog's and the
///   reflections' colour);
/// - `fn dwell_sky_bodies(dir: vec3f, face: f32, day: DaySky, base: vec3f) -> vec3f`: the stars, the
///   moon (its phase lit from the sun's side, with maria and earthshine) and the sun (limb-darkened,
///   larger near the horizon) over `base`, cut at the viewer's horizon — drawn behind the world only.
pub fn sky_wgsl() -> String {
    let d = &DAY_NIGHT;
    let consts = [
        ("GRADIENT_POWER", d.sky.gradient_power),
        ("GLOW_CAP", GLOW_CAP),
        ("BAND_HEIGHT", d.sky.band_height),
        ("BAND_SPREAD", d.sky.band_spread),
        ("SUN_RADIUS", d.sun.radius_deg.to_radians()),
        ("SUN_HORIZON_SCALE", d.sun.horizon_scale),
        ("SUN_LIMB", d.sun.limb_darkening),
        ("SUN_BLOOM", d.sun.bloom),
        ("SUN_BLOOM_K", gauss_k(d.sun.bloom_deg)),
        ("SUN_GLOW", d.sun.glow),
        ("SUN_GLOW_K", gauss_k(d.sun.glow_deg)),
        ("MOON_RADIUS", d.moon.radius_deg.to_radians()),
        ("MOON_MARIA", d.moon.maria),
        ("MOON_EARTHSHINE", d.moon.earthshine),
        ("MOON_BLOOM", d.moon.bloom),
        ("MOON_BLOOM_K", gauss_k(d.moon.bloom_deg)),
        ("MOON_GLOW", d.moon.glow),
        ("MOON_GLOW_K", gauss_k(d.moon.glow_deg)),
        ("STAR_CELLS", d.stars.cells.round()),
        ("STAR_DENSITY", d.stars.density),
        ("STAR_SIZE", d.stars.size_px),
        ("STAR_BRIGHTNESS", d.stars.brightness),
        ("STAR_TWINKLE", d.stars.twinkle),
        ("STAR_TWINKLE_HZ", d.stars.twinkle_hz),
        ("STAR_FADE", d.stars.horizon_fade.max(1e-4)),
    ];
    let mut s: Vec<String> = consts
        .iter()
        .map(|(name, v)| format!("const {name}: f32 = {};", f(*v)))
        .collect();
    s.push(format!("const MOON_COLOR: vec3f = {};", vec3(d.moon.color)));
    s.push(format!(
        "const MOON_GLOW_COLOR: vec3f = {};",
        vec3(d.moon.glow_color)
    ));
    s.push(SKY_WGSL.to_string());
    s.join("\n")
}

/// The sky's shader functions (the constants above come first).
const SKY_WGSL: &str = include_str!("sky.wgsl");

/// A light's weight for a fragment whose face's up is `face_up` (±1): its height over that face's
/// horizon, smoothly (each fragment takes the lights above its own face, BIFACIAL_WORLD.md §6).
pub fn light_weight(light_up: f64, face_up: f64) -> f64 {
    smoothstep(-0.1, 0.1, light_up * face_up)
}

/// sRGB (0–1) to linear light.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daynight::{DayNight, Keyframe};

    const FACES: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

    fn luma(c: Rgb) -> f64 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }
    fn dist(a: Rgb, b: Rgb) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
    }
    /// The state at time of day `t`.
    fn at(t: f64) -> DayState {
        DAY_NIGHT.state(0.0, Some(t), None)
    }
    /// A direction `deg` degrees of azimuth round from the sun's, with elevation sine `y`.
    fn around_sun(day: &DayState, deg: f64, y: f64) -> Rgb {
        let base = day.sun[2].atan2(day.sun[0]) + deg.to_radians();
        let h = (1.0 - y * y).max(0.0).sqrt();
        [base.cos() * h, y, base.sin() * h]
    }
    /// Face A's time of day whose solar angle is `deg` (by bisection: the angle rises with time).
    fn time_at_solar(deg: f64) -> f64 {
        let unwrap = |t: f64| (DAY_NIGHT.solar_deg(t) - 270.0).rem_euclid(360.0);
        let target = (deg - 270.0).rem_euclid(360.0);
        let (mut lo, mut hi) = (0.0, 1.0 - 1e-12);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if unwrap(mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    #[test]
    fn tints_are_warm_on_top_and_bluest_underneath() {
        let top = face_tint(1, 1);
        assert!(top[0] > top[2]);
        let blueness = |c: Rgb| c[2] / c[0];
        for (axis, sign) in FACES {
            if !(axis == 1 && sign < 0) {
                assert!(blueness(face_tint(1, -1)) > blueness(face_tint(axis, sign)));
            }
            if !(axis == 1 && sign > 0) {
                assert!(blueness(face_tint(axis, sign)) > blueness(top));
            }
        }
        // normal_tint equals face_tint on axis-aligned normals.
        for (axis, sign) in FACES {
            let mut n = [0.0; 3];
            n[axis] = f64::from(sign);
            assert_eq!(normal_tint(n[0], n[1], n[2]), face_tint(axis, sign));
        }
    }

    #[test]
    fn the_fog_is_the_horizons_colour_at_every_time_of_day() {
        // The composite fogs toward `sky(dir)`: below the horizon that is the horizon's colour in
        // the same azimuth; side-on to the sun (its band and glow faint there), the keyframes'
        // horizon colour.
        for i in 0..240 {
            let day = at(f64::from(i) / 240.0);
            for face in [1.0, -1.0] {
                let s = if face > 0.0 { day.a } else { day.b };
                for deg in [0.0, 60.0, 90.0, 135.0, 180.0, 250.0] {
                    let level = around_sun(&day, deg, 0.0);
                    let below = around_sun(&day, deg, -0.4 * face);
                    let c = sky_color(level, face, &s, &day);
                    assert!(dist(sky_color(below, face, &s, &day), c) < 1e-9);
                    if deg == 90.0 {
                        assert!(
                            dist(c, s.sky_horizon) < 0.12,
                            "t {i}/240 face {face}: {c:?} vs {:?}",
                            s.sky_horizon
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_sky_grades_from_the_horizon_to_the_zenith_and_glows_toward_the_sun() {
        let day = at(0.5);
        let zenith = sky_color([0.0, 1.0, 0.0], 1.0, &day.a, &day);
        // Noon is Phase 7's approved sky: cerulean overhead, a pale haze at the horizon.
        assert!(dist(day.a.sky_zenith, hex(0x649ada)) < 1e-3);
        assert!(zenith[2] > zenith[0] + 0.3, "{zenith:?}");
        let horizon = sky_color(around_sun(&day, 180.0, 0.0), 1.0, &day.a, &day);
        assert!(dist(horizon, hex(0xe5e5e1)) < 0.03, "{horizon:?}");
        let mut last = f64::INFINITY;
        for i in 0..=6 {
            let c = sky_color(
                around_sun(&day, 180.0, f64::from(i) / 10.0),
                1.0,
                &day.a,
                &day,
            );
            assert!(c[0] <= last + 1e-9);
            last = c[0];
        }
        let toward = sky_color(day.sun, 1.0, &day.a, &day);
        let away = sky_color(around_sun(&day, 180.0, day.sun[1]), 1.0, &day.a, &day);
        assert!(toward[0] > away[0] && toward[0] - toward[2] > away[0] - away[2]);
    }

    #[test]
    fn sunset_warms_the_horizon_toward_the_sun() {
        let t = DAY_NIGHT
            .keyframes
            .iter()
            .find(|k| k.name == "sunset")
            .expect("a sunset");
        let day = DAY_NIGHT.state(0.0, Some(time_at_solar(t.sun)), None);
        assert!(day.sun[1].abs() < 1e-6);
        let toward = sky_color(around_sun(&day, 0.0, 0.02), 1.0, &day.a, &day);
        let away = sky_color(around_sun(&day, 180.0, 0.02), 1.0, &day.a, &day);
        assert!(toward[0] - toward[2] > 0.25, "{toward:?}");
        assert!(toward[0] - toward[2] > away[0] - away[2] + 0.1);
    }

    #[test]
    fn night_is_dark_but_readable_and_moonlit_blue() {
        let day = at(0.0);
        let s = day.a;
        assert!(luma(s.sky_horizon) > 0.1 && luma(s.sky_horizon) < 0.3);
        assert!(luma(s.sky_zenith) < luma(s.sky_horizon));
        assert!(s.moon_light[2] > s.moon_light[0] && s.hemi_sky[2] > s.hemi_sky[0]);
        assert!(s.sun_intensity == 0.0 && s.moon_intensity > 0.4 && s.hemi_intensity > 0.5);
        assert!(s.saturation < 0.8);
        // Darker than day, never black.
        let noon = at(0.5).a;
        let ambient = |k: &Sample| luma(k.hemi_sky.map(srgb_to_linear)) * k.hemi_intensity;
        assert!(ambient(&s) < ambient(&noon) * 0.5 && ambient(&s) > ambient(&noon) * 0.05);
        assert!(s.stars > 0.9);
    }

    #[test]
    fn face_b_has_night_while_face_a_has_day() {
        let day = at(0.5);
        assert!(day.a.sun_intensity > 2.0 && day.a.stars == 0.0);
        assert!(day.b.sun_intensity == 0.0 && day.b.stars > 0.9);
        assert!(day.sun[1] > 0.8 && (day.moon[1] + day.sun[1]).abs() < 1e-12);
        let midnight = at(0.0);
        assert!(midnight.b.sun_intensity > 2.0 && midnight.a.sun_intensity == 0.0);
        // A face B viewer at noon sees face B's night sky.
        let up = sky_color([0.0, -1.0, 0.0], -1.0, &day.b, &day);
        let a_up = sky_color([0.0, 1.0, 0.0], 1.0, &day.a, &day);
        assert!(luma(up) < luma(a_up) * 0.3);
    }

    #[test]
    fn the_viewers_look_eases_between_the_faces() {
        let day = at(0.3);
        assert_eq!(day.viewer(1.0), day.a);
        assert_eq!(day.viewer(0.0), day.b);
        let mut last = day.viewer(0.0);
        for i in 1..=20 {
            let v = day.viewer(f64::from(i) / 20.0);
            assert!(dist(v.sky_zenith, last.sky_zenith) < 0.1);
            assert!((v.sun_intensity - last.sun_intensity).abs() < 0.25);
            last = v;
        }
    }

    #[test]
    fn light_weights_follow_the_fragments_own_horizon() {
        assert_eq!(light_weight(0.7, 1.0), 1.0);
        assert_eq!(light_weight(-0.7, 1.0), 0.0);
        assert_eq!(light_weight(-0.7, -1.0), 1.0);
        assert_eq!(light_weight(0.7, -1.0), 0.0);
        assert!((light_weight(0.0, 1.0) - 0.5).abs() < 1e-12);
        let mut last = -1.0;
        for i in -30..=30 {
            let w = light_weight(f64::from(i) / 100.0, 1.0);
            assert!(w >= last);
            last = w;
        }
    }

    #[test]
    fn exposure_clamps_and_defaults() {
        assert_eq!(sanitize_exposure(None), DEFAULT_EXPOSURE);
        assert_eq!(sanitize_exposure(Some(f64::NAN)), DEFAULT_EXPOSURE);
        assert_eq!(sanitize_exposure(Some(100.0)), EXPOSURE_MAX);
        assert_eq!(sanitize_exposure(Some(0.0)), EXPOSURE_MIN);
        assert_eq!(sanitize_exposure(Some(1.3)), 1.3);
    }

    #[test]
    fn the_shader_sky_is_valid_wgsl_with_the_uniform_block() {
        let src = format!(
            "{}\n@group(0) @binding(0) var<uniform> day: DaySky;\n{}\n\
             @fragment fn main(@location(0) d: vec3f) -> @location(0) vec4f {{\n\
             let c = dwell_sky(d, 1.0, day);\n\
             return vec4f(dwell_sky_bodies(d, 1.0, day, c), 1.0);\n}}",
            daynight::DAY_SKY_WGSL,
            sky_wgsl()
        );
        let module = naga::front::wgsl::parse_str(&src).expect("valid WGSL");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        // The block's size is what `pack` fills.
        let fields = daynight::DAY_SKY_WGSL.matches(": vec4f,").count();
        assert_eq!(fields * 4, daynight::DAY_SKY_FLOATS);
        assert!(sky_wgsl().contains(&format!(
            "const SUN_RADIUS: f32 = {};",
            f(DAY_NIGHT.sun.radius_deg.to_radians())
        )));
    }

    /// A keyframe changed by data alone changes the sky at its angle and not at its neighbours.
    #[test]
    fn a_keyframe_changed_by_data_changes_the_sky() {
        let mut frames: Vec<Keyframe> = DAY_NIGHT.keyframes.to_vec();
        let noon = frames.iter().position(|k| k.name == "noon").expect("noon");
        frames[noon].sky_zenith = [1.0, 0.0, 0.0];
        let changed = DayNight {
            keyframes: Box::leak(frames.into_boxed_slice()),
            ..DAY_NIGHT
        };
        let at_noon = changed.sample(changed.keyframes[noon].sun);
        assert!(
            dist(at_noon.sky_zenith, [1.0, 0.0, 0.0]) < 1e-3,
            "{:?}",
            at_noon.sky_zenith
        );
        assert!(dist(DAY_NIGHT.sample(90.0).sky_zenith, hex(0x649ada)) < 1e-3);
        let before = changed.keyframes[noon - 1].sun;
        assert!(
            dist(
                changed.sample(before).sky_zenith,
                DAY_NIGHT.sample(before).sky_zenith
            ) < 1e-9
        );
    }
}
