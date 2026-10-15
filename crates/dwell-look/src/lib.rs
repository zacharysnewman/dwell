//! The game's look (WORLD_GENERATION.md §1, Phase 7): warm light, cool shadow, one sky. Everything
//! the meshers, the lights and the fog must agree on lives here as plain data, so the chunk and
//! LOD meshers share it and tests pin it. Colours are sRGB triples in 0–1 unless a name says linear.
//! Port of client/src/render/{look,fog,skyFrame}.ts.

pub mod fog;
pub mod motion;
pub mod sky_frame;

use sky_frame::SkyFrame;

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

fn normalize(v: Rgb) -> Rgb {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
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

/// Where the sun is (unit vector, toward it): ~45° up, upper left in the default spawn view.
pub fn sun_direction() -> Rgb {
    normalize([0.62, 0.7, 0.35])
}
/// Where the moon is, in the world: opposite the sun (BIFACIAL_WORLD.md §6).
pub fn moon_direction_world() -> Rgb {
    sun_direction().map(|c| -c)
}

/// The lights (sRGB colours, three.js intensities).
pub struct Lights {
    pub sun: u32,
    pub sun_intensity: f64,
    pub hemisphere_sky: u32,
    /// Green bounce off the grass, not brown.
    pub hemisphere_ground: u32,
    pub hemisphere_intensity: f64,
    /// Face B's: a cooler, dimmer moonlight, and a night sky's ambient.
    pub moon: u32,
    pub moon_intensity: f64,
    pub moon_hemisphere_sky: u32,
    pub moon_hemisphere_ground: u32,
    pub moon_hemisphere_intensity: f64,
}

pub const LIGHT: Lights = Lights {
    sun: 0xfff1d6,
    sun_intensity: 2.2,
    hemisphere_sky: 0x9cc6f0,
    hemisphere_ground: 0x7a8a4a,
    hemisphere_intensity: 1.9,
    moon: 0xb8ccff,
    moon_intensity: 0.9,
    moon_hemisphere_sky: 0x3a4f8c,
    moon_hemisphere_ground: 0x232c42,
    moon_hemisphere_intensity: 1.1,
};

/// Fragments below the midplane belong to face B (lit by the moon only).
pub const MIDPLANE_Y: f64 = dwell_protocol::world::MIDPLANE_Y as f64;

/// Tone mapping exposure: 1 leaves the lights as set.
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

/// A gradient stop: `at` and an sRGB colour.
pub type Stop = (f64, Rgb);

/// Day sky by the sine of the elevation (0 the horizon, 1 straight up): cerulean overhead grading
/// to a near-white haze at the horizon (§1.3).
pub const SKY_STOPS: [Stop; 4] = [
    (0.0, hex(0xe5e5e1)),
    (0.12, hex(0xb3cce6)),
    (0.4, hex(0x91b7e6)),
    (1.0, hex(0x649ada)),
];
pub const HORIZON_COLOR: Rgb = hex(0xe5e5e1);

/// A light's glow: a tight core and a broad halo, in this colour.
pub struct Glow {
    pub color: Rgb,
    pub tight: f64,
    pub tight_power: f64,
    pub broad: f64,
    pub broad_power: f64,
}

pub const SUN_GLOW: Glow = Glow {
    color: hex(0xf5d9a2),
    tight: 0.4,
    tight_power: 48.0,
    broad: 0.18,
    broad_power: 5.0,
};

/// Face B's night sky: deep blue overhead grading to a dim blue-grey haze at the horizon.
pub const NIGHT_SKY_STOPS: [Stop; 4] = [
    (0.0, hex(0x3a4766)),
    (0.12, hex(0x2b3856)),
    (0.4, hex(0x18213f)),
    (1.0, hex(0x0a1026)),
];
pub const NIGHT_HORIZON_COLOR: Rgb = hex(0x3a4766);

pub const MOON_GLOW: Glow = Glow {
    color: hex(0xcfdcff),
    tight: 0.7,
    tight_power: 220.0,
    broad: 0.12,
    broad_power: 8.0,
};

pub fn gradient(stops: &[Stop], e: f64) -> Rgb {
    let mut c = stops.first().map_or(HORIZON_COLOR, |s| s.1);
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        if e <= b.0 {
            c = mix(a.1, b.1, ((e - a.0) / (b.0 - a.0)).max(0.0));
            break;
        }
        c = b.1;
    }
    c
}

/// The sky sphere's gradient by the dot of a direction with the day sky's zenith (−1 the night
/// zenith, +1 the day's), with a twilight between.
pub fn sky_sphere_stops() -> [Stop; 7] {
    [
        (-1.0, hex(0x0a1026)),
        (-0.4, hex(0x18213f)),
        (-0.12, hex(0x2b3856)),
        (0.0, mix(hex(0x2b3856), hex(0xb3cce6), 0.5)), // twilight
        (0.12, hex(0xb3cce6)),
        (0.4, hex(0x91b7e6)),
        (1.0, hex(0x649ada)),
    ]
}
/// How far (in elevation sine) the viewer's horizon haze reaches up, and the twilight's width.
pub const HAZE_BAND: f64 = 0.12;
pub const TWILIGHT: f64 = 0.1;

fn glow(g: &Glow, x: f64) -> f64 {
    g.tight * x.powf(g.tight_power) + g.broad * x.powf(g.broad_power)
}

/// The sky's colour (sRGB) toward the unit world direction `dir` for a viewer on `face` (+1 A,
/// −1 B): one sphere graded from day to night by the dot with the frame's day zenith, a haze at the
/// viewer's horizon, and the sun's and moon's glows. Below the horizon it is the horizon's colour.
/// The sky drawn behind the world and the haze far terrain fades into are both this function
/// ([`sky_wgsl`] is its shader twin).
pub fn sky_color(dir: Rgb, face: f64, frame: &SkyFrame) -> Rgb {
    let up = [0.0, face, 0.0];
    let above = dot(dir, up);
    let e = above.clamp(0.0, 1.0);
    let mut d = dir;
    if above <= 0.0 {
        let level = [
            dir[0] - above * up[0],
            dir[1] - above * up[1],
            dir[2] - above * up[2],
        ];
        let len = dot(level, level).sqrt();
        if len >= 1e-6 {
            d = level.map(|c| c / len);
        }
    }
    let base = gradient(&sky_sphere_stops(), dot(d, frame.day_pole));
    let dayness = smoothstep(-TWILIGHT, TWILIGHT, dot(up, frame.day_pole));
    let haze = mix(NIGHT_HORIZON_COLOR, HORIZON_COLOR, dayness);
    let mut c = mix(base, haze, 1.0 - smoothstep(0.0, HAZE_BAND, e));
    c = mix(
        c,
        SUN_GLOW.color,
        glow(&SUN_GLOW, dot(d, frame.sun).max(0.0)).min(1.0),
    );
    mix(
        c,
        MOON_GLOW.color,
        glow(&MOON_GLOW, dot(d, frame.moon).max(0.0)).min(1.0),
    )
}

fn f(n: f64) -> String {
    format!("{n:.5}")
}
fn vec3(c: Rgb) -> String {
    format!("vec3f({}, {}, {})", f(c[0]), f(c[1]), f(c[2]))
}
fn glow_wgsl(g: &Glow, x: &str) -> String {
    format!(
        "{} * pow({x}, {}) + {} * pow({x}, {})",
        f(g.tight),
        f(g.tight_power),
        f(g.broad),
        f(g.broad_power)
    )
}

/// [`sky_color`] for shaders: `fn dwell_sky(dir: vec3f, face: f32, day_pole: vec3f, sun: vec3f,
/// moon: vec3f) -> vec3f`, generated from the same constants.
pub fn sky_wgsl() -> String {
    let stops = sky_sphere_stops();
    let mut s = vec![
        "fn dwell_sky(dir: vec3f, face: f32, day_pole: vec3f, sun: vec3f, moon: vec3f) -> vec3f {"
            .to_string(),
        "\tlet up = vec3f(0.0, face, 0.0);".into(),
        "\tlet above = dot(dir, up);".into(),
        "\tlet e = clamp(above, 0.0, 1.0);".into(),
        "\tvar d = dir;".into(),
        "\tif (above <= 0.0) {".into(),
        "\t\tlet level = dir - above * up;".into(),
        "\t\tlet len = length(level);".into(),
        "\t\tif (len >= 1e-6) { d = level / len; }".into(),
        "\t}".into(),
        "\tlet u = dot(d, day_pole);".into(),
        format!("\tvar c = {};", vec3(stops[0].1)),
    ];
    for w in stops.windows(2) {
        s.push(format!(
            "\tc = mix(c, {}, clamp((u - {}) / {}, 0.0, 1.0));",
            vec3(w[1].1),
            f(w[0].0),
            f(w[1].0 - w[0].0)
        ));
    }
    s.extend([
        format!(
            "\tlet dayness = smoothstep({}, {}, dot(up, day_pole));",
            f(-TWILIGHT),
            f(TWILIGHT)
        ),
        format!(
            "\tlet haze = mix({}, {}, dayness);",
            vec3(NIGHT_HORIZON_COLOR),
            vec3(HORIZON_COLOR)
        ),
        format!(
            "\tc = mix(c, haze, 1.0 - smoothstep(0.0, {}, e));",
            f(HAZE_BAND)
        ),
        "\tlet ds = max(0.0, dot(d, sun));".into(),
        format!(
            "\tc = mix(c, {}, min(1.0, {}));",
            vec3(SUN_GLOW.color),
            glow_wgsl(&SUN_GLOW, "ds")
        ),
        "\tlet dm = max(0.0, dot(d, moon));".into(),
        format!(
            "\tc = mix(c, {}, min(1.0, {}));",
            vec3(MOON_GLOW.color),
            glow_wgsl(&MOON_GLOW, "dm")
        ),
        "\treturn c;".into(),
        "}".into(),
    ]);
    s.join("\n")
}

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
    use sky_frame::{sky_frame, sky_rest};

    const FACES: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

    fn close(a: Rgb, b: Rgb, tol: f64) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < tol, "{a:?} vs {b:?} (±{tol})");
        }
    }
    fn luma(c: Rgb) -> f64 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }
    fn sky(dir: Rgb, face: f64) -> Rgb {
        sky_color(dir, face, &sky_rest())
    }
    /// A direction with elevation sine y, pointing away from the sun's side.
    fn away(y: f64) -> Rgb {
        let h = (1.0 - y * y).sqrt();
        let [sx, _, sz] = sun_direction();
        let n = sx.hypot(sz);
        [-sx / n * h, y, -sz / n * h]
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
    fn the_sky_is_the_horizon_colour_at_and_below_the_horizon_away_from_the_sun() {
        close(sky(away(0.0), 1.0), HORIZON_COLOR, 2e-3);
        close(sky(away(-0.5), 1.0), HORIZON_COLOR, 2e-3);
        assert_eq!(SKY_STOPS[0].1, HORIZON_COLOR);
    }

    #[test]
    fn the_sky_grades_to_the_zenith_bluer_and_darker() {
        let zenith = sky([0.0, 1.0, 0.0], 1.0);
        close(zenith, SKY_STOPS[3].1, 0.03);
        assert!(zenith[2] > zenith[0] && zenith[0] < HORIZON_COLOR[0]);
        let mut last = 0.0;
        for i in 0..=10 {
            let d = HORIZON_COLOR[0] - sky(away(f64::from(i) / 10.0), 1.0)[0];
            assert!(d >= last - 1e-9);
            last = d;
        }
    }

    #[test]
    fn the_sky_glows_warm_toward_the_sun() {
        let sun = sky(sun_direction(), 1.0);
        let same = sky(away(sun_direction()[1]), 1.0);
        assert!(sun[0] > same[0] && sun[0] - sun[2] > same[0] - same[2]);
    }

    #[test]
    fn the_sphere_at_rest_matches_each_faces_gradient() {
        for face in [1.0, -1.0] {
            let stops = if face > 0.0 {
                SKY_STOPS
            } else {
                NIGHT_SKY_STOPS
            };
            let [sx, _, sz] = sun_direction();
            let n = sx.hypot(sz);
            let glow_at =
                |g: &Glow, light: Rgb, dir: Rgb| glow(g, dot(dir, light).max(0.0)).min(1.0);
            for i in 0..=100 {
                let e = f64::from(i) / 100.0;
                let h = (1.0 - e * e).max(0.0).sqrt();
                let dir = [-sx / n * h, face * e, -sz / n * h];
                let mut old = gradient(&stops, e.min(1.0));
                old = mix(
                    old,
                    SUN_GLOW.color,
                    glow_at(&SUN_GLOW, sun_direction(), dir),
                );
                old = mix(
                    old,
                    MOON_GLOW.color,
                    glow_at(&MOON_GLOW, moon_direction_world(), dir),
                );
                close(sky(dir, face), old, if e >= 0.12 { 1e-6 } else { 0.09 });
            }
        }
    }

    #[test]
    fn the_sky_frame_turns_the_sky_as_a_unit() {
        let mut a: i32 = 12345;
        let mut random = move || {
            a = a.wrapping_add(0x6d2b79f5);
            let mut t = (a ^ ((a as u32) >> 15) as i32).wrapping_mul(1 | a);
            t = t.wrapping_add((t ^ ((t as u32) >> 7) as i32).wrapping_mul(61 | t)) ^ t;
            f64::from((t ^ ((t as u32) >> 14) as i32) as u32) / 4294967296.0
        };
        for _ in 0..100 {
            let dir = normalize([
                random() * 2.0 - 1.0,
                random() * 2.0 - 1.0,
                random() * 2.0 - 1.0,
            ]);
            let angle = random() * std::f64::consts::TAU;
            for face in [1.0, -1.0] {
                close(
                    sky_color(dir, face, &sky_frame(angle)),
                    sky_color(dir, face, &sky_frame(angle + 0.001)),
                    0.01,
                );
            }
        }
        let pi = std::f64::consts::PI;
        close(
            sky_color([0.0, 1.0, 0.0], 1.0, &sky_frame(pi)),
            sky_sphere_stops()[0].1,
            0.03,
        );
        close(
            sky_color([0.0, 1.0, 0.0], 1.0, &sky_frame(pi / 2.0)),
            sky_sphere_stops()[3].1,
            0.05,
        );
        close(
            sky_color([0.0, -1.0, 0.0], -1.0, &sky_frame(pi)),
            SKY_STOPS[3].1,
            0.03,
        );
    }

    #[test]
    fn face_b_has_a_moonlit_night_sky_in_its_own_frame() {
        for (m, s) in moon_direction_world().iter().zip(sun_direction()) {
            assert!((m + s).abs() < 1e-12);
        }
        let overhead = sky([0.0, -1.0, 0.0], -1.0);
        let horizon = sky([0.6, 0.0, -0.8], -1.0);
        assert!((horizon[2] - NIGHT_SKY_STOPS[0].1[2]).abs() < 0.005);
        assert!(luma(overhead) < luma(horizon));
        assert!(luma(overhead) < luma(sky([0.0, 1.0, 0.0], 1.0)) * 0.3);
        let flat = sky([0.6, 0.0, 0.8], -1.0);
        let deeper = (0.6f64 * 0.6 + 0.25 + 0.64).sqrt();
        close(
            sky([0.6 / deeper, 0.5 / deeper, 0.8 / deeper], -1.0),
            flat,
            1e-9,
        );
        let moon = sky(moon_direction_world(), -1.0);
        let m = moon_direction_world();
        assert!(luma(moon) > luma(sky([-m[0], m[1], -m[2]], -1.0)) && moon[2] > moon[0]);
        const {
            assert!(
                LIGHT.moon_intensity < LIGHT.sun_intensity
                    && LIGHT.moon_hemisphere_intensity <= LIGHT.hemisphere_intensity
            );
            assert!((LIGHT.moon & 0xff) > (LIGHT.moon >> 16) & 0xff);
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
    fn the_shader_sky_is_valid_wgsl_carrying_the_same_stops() {
        let src = sky_wgsl();
        naga::front::wgsl::parse_str(&src).expect("valid WGSL");
        for (_, c) in sky_sphere_stops() {
            assert!(src.contains(&format!("{}, {}, {}", f(c[0]), f(c[1]), f(c[2]))));
        }
        assert!(src.contains("let up = vec3f(0.0, face, 0.0);"));
    }
}
