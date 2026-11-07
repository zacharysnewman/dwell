//! Waterfalls drawn (docs/WATER_SURFACE_GRID.md §2e, Phase 53 G6; the Waterfalls setting): for each
//! fall near the eye (`dwell_world::falls`, found from the client's water cells and current table),
//! a falling sheet along its lip, curved as the water is thrown out over it and falls, its opacity by
//! the flow, pale streaks running down it at the fall's speed; a foam patch where it lands; and spray
//! puffs thrown up there. Looks per liquid from `shared/look/waterfalls.json`.
//!
//! The quads go through the flames' billboard pass (`render/flames.rs`: premultiplied, lit, over
//! what is behind them, depth-tested and writing no depth), as the flecks on moving water do. A CPU
//! adapter draws the sheets and foam still and no spray (`spray_cpu_count`). Cost: a few quads per
//! fall (`sheet_segments` + 1 + its spray), at most `max_falls` falls, written once a frame.
//!
//! Face-local: each fall's geometry is laid out in its face's frame and mapped into the world through
//! its `FaceFrame`, so face B's falls are face A's mirror.

use dwell_look::waterfalls::{FALL_LOOKS, FallLook, WATERFALLS, fall_look};
use dwell_world::falls::{DIRS, Fall};

use super::camera::Basis;
use super::flames::{quad, wrapped};

/// A sheet's segment (its liquid's index in `FALL_LOOKS` in the low byte).
pub const KIND_SHEET: u32 = 5;
/// The foam where a fall lands.
pub const KIND_FOAM: u32 = 6;
/// A spray puff.
pub const KIND_SPRAY: u32 = 7;

const G: f64 = 9.81;
/// The least speed a sheet is thrown out at (m/s), so a slow spill clears its wall.
const MIN_THROW_MS: f64 = 0.3;
/// How far off the lip's face the sheet starts (m), clear of the wall under it.
const OFF_WALL_M: f64 = 0.04;

/// A fall's look index, if its liquid has a falls look.
pub fn look_of(fall: &Fall) -> Option<usize> {
    let fluid = dwell_blocks::FLUIDS.get(usize::from(fall.fluid))?;
    fall_look(fluid.name)
}

/// A fall's lip, face-locally: the start of its lip edge (on the face it falls over), the unit
/// along the lip, and the unit out over it (x, z).
fn lip_edge(fall: &Fall) -> ([f64; 2], [f64; 2], [f64; 2]) {
    let [dx, dz] = DIRS[usize::from(fall.dir)];
    let (x, z) = (f64::from(fall.x), f64::from(fall.z));
    let start = match (dx, dz) {
        (1, _) => [x + 1.0, z],
        (-1, _) => [x, z],
        (_, 1) => [x, z + 1.0],
        _ => [x, z],
    };
    let along = if dx != 0 { [0.0, 1.0] } else { [1.0, 0.0] };
    (start, along, [f64::from(dx), f64::from(dz)])
}

/// A fall's curve: how far out (m) and how far down (m) its water is after `s` of the fall's time,
/// and that whole time (s) to its landing.
fn curve(fall: &Fall) -> (f64, f64) {
    let drop = (fall.lip - fall.land).max(0.0);
    ((2.0 * drop / G).sqrt(), fall.speed.max(MIN_THROW_MS))
}

/// The sheet's opacity by the fall's flow.
fn opacity(look: &FallLook, fall: &Fall) -> f64 {
    look.sheet_opacity_thin + (look.sheet_opacity_full - look.sheet_opacity_thin) * fall.strength()
}

fn hash(seed: u32, k: u32) -> f64 {
    let mut h = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(k.wrapping_mul(2_891_336_453));
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    f64::from(h & 0xffff) / 65535.0
}

/// A fall's seed (its lip's place and way).
pub fn seed_of(fall: &Fall) -> u32 {
    super::flames::cell_seed([fall.x, i32::from(fall.dir), fall.z])
}

/// The falls' quads (relative to `eye`, the spray facing the camera of `basis`) at `seconds`: each
/// fall's sheet, its foam and its spray (when `animated`: a CPU adapter's still sheets, its
/// `spray_cpu_count` puffs). The nearest `max_falls` within `draw_m`. Returns the vertices and the
/// falls drawn.
pub fn fall_quads(
    falls: &[Fall],
    eye: [f64; 3],
    basis: &Basis,
    seconds: f64,
    animated: bool,
) -> (Vec<f32>, usize) {
    let mut near: Vec<(f64, &Fall)> = falls
        .iter()
        .map(|f| {
            let p = f.lip_world();
            let d = ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2))
                .sqrt();
            (d, f)
        })
        .filter(|(d, _)| *d <= WATERFALLS.draw_m)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(WATERFALLS.max_falls as usize);
    let t = if animated { wrapped(seconds) } else { 0.0 };
    let mut out = Vec::new();
    let mut n = 0;
    for (_, fall) in near {
        let Some(li) = look_of(fall) else {
            continue;
        };
        let look = &FALL_LOOKS[li];
        let f = fall.face;
        let seed = seed_of(fall);
        let (start, along, outward) = lip_edge(fall);
        let w = f64::from(fall.width);
        let (time, throw) = curve(fall);
        // A face-local point (along the lip, out from it, height) relative to the eye.
        let at = |a: f64, o: f64, y: f64| -> [f64; 3] {
            let p = f.point([
                start[0] + along[0] * a + outward[0] * o,
                y,
                start[1] + along[1] * a + outward[1] * o,
            ]);
            [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]]
        };
        let point = |s: f64| (OFF_WALL_M + throw * s, fall.lip - 0.5 * G * s * s);
        // The sheet: its curve in segments, uv x across (m), y down its length (m).
        let segs = look.sheet_segments.max(1);
        let alpha = opacity(look, fall) as f32;
        let mut down = 0.0;
        for k in 0..segs {
            let (s0, s1) = (
                time * f64::from(k) / f64::from(segs),
                time * f64::from(k + 1) / f64::from(segs),
            );
            let (p0, p1) = (point(s0), point(s1));
            let len = (p1.0 - p0.0).hypot(p1.1 - p0.1);
            let speed = G * 0.5 * (s0 + s1) + throw;
            let d0 = down;
            quad(
                &mut out,
                |x, y| {
                    let (o, h) = if y < 0.5 { p0 } else { p1 };
                    at(x * w, o, h)
                },
                |x, y| [(x * w) as f32, (d0 + y * len) as f32],
                [t, alpha, (speed * look.sheet_scroll) as f32],
                li as u32 | KIND_SHEET << 8,
                seed,
            );
            down += len;
        }
        // The foam where it lands: a level patch over the sheet's foot.
        let s = fall.strength();
        let r = look.foam_radius_m * s + 0.25;
        let (o_land, _) = point(time);
        let lift = 0.02;
        quad(
            &mut out,
            |x, y| {
                at(
                    -r + x * (w + 2.0 * r),
                    o_land - r + y * 2.0 * r,
                    fall.land + lift,
                )
            },
            |x, y| [(2.0 * x - 1.0) as f32, (2.0 * y - 1.0) as f32],
            [t, (look.foam_opacity * (0.3 + 0.7 * s)) as f32, w as f32],
            li as u32 | KIND_FOAM << 8,
            seed,
        );
        // The spray: puffs rising from the landing and falling back, each on its own cycle.
        let count = if animated {
            look.spray_count
        } else {
            look.spray_cpu_count
        };
        let puffs = (f64::from(count) * w * s).round() as u32;
        for i in 0..puffs {
            let phase = (seconds / look.spray_life_s + hash(seed, 4 * i)).fract();
            let a = hash(seed, 4 * i + 1) * w;
            let o = o_land + (hash(seed, 4 * i + 2) - 0.3) * 0.8;
            let rise = look.spray_height_m * (0.5 + 0.5 * hash(seed, 4 * i + 3));
            let h = fall.land + rise * (std::f64::consts::PI * phase).sin();
            let size = look.spray_size_m * (0.6 + 0.8 * phase);
            let fade = (std::f64::consts::PI * phase).sin();
            let c = at(a, o, h);
            quad(
                &mut out,
                |x, y| {
                    std::array::from_fn(|k| {
                        c[k] + (basis.right[k] * (2.0 * x - 1.0) + basis.up[k] * (2.0 * y - 1.0))
                            * size
                    })
                },
                |x, y| [(2.0 * x - 1.0) as f32, (2.0 * y - 1.0) as f32],
                [t, (look.spray_opacity * fade) as f32, 0.0],
                li as u32 | KIND_SPRAY << 8,
                seed.wrapping_add(i),
            );
        }
        n += 1;
    }
    (out, n)
}

/// The falls' shading in WGSL: `fall_colour(kind, look, uv, rest, rel, seed)`, premultiplied and
/// lit as the water's surface is.
pub fn wgsl() -> String {
    let lin = |c: u32| -> String {
        let [r, g, b] = [16, 8, 0]
            .map(|k| dwell_look::srgb_to_linear(f64::from((c >> k) & 0xff) / 255.0) as f32);
        format!("vec3f({r:?}, {g:?}, {b:?})")
    };
    let list = |v: Vec<String>, none: &str| {
        if v.is_empty() {
            none.to_string()
        } else {
            v.join(", ")
        }
    };
    let sheets = list(
        FALL_LOOKS.iter().map(|l| lin(l.sheet_color)).collect(),
        "vec3f(1.0)",
    );
    let foams = list(
        FALL_LOOKS.iter().map(|l| lin(l.foam_color)).collect(),
        "vec3f(1.0)",
    );
    let sprays = list(
        FALL_LOOKS.iter().map(|l| lin(l.spray_color)).collect(),
        "vec3f(1.0)",
    );
    let streaks = list(
        FALL_LOOKS
            .iter()
            .map(|l| format!("{:?}", l.sheet_streaks_per_m as f32))
            .collect(),
        "1.0",
    );
    let n = FALL_LOOKS.len().max(1);
    format!(
        "const FALL_SHEETS = array<vec3f, {n}>({sheets});
const FALL_FOAMS = array<vec3f, {n}>({foams});
const FALL_SPRAYS = array<vec3f, {n}>({sprays});
const FALL_STREAKS = array<f32, {n}>({streaks});
fn fall_hash(p: vec2f) -> f32 {{
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}}
fn fall_noise(p: vec2f) -> f32 {{
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(fall_hash(i), fall_hash(i + vec2f(1.0, 0.0)), u.x),
        mix(fall_hash(i + vec2f(0.0, 1.0)), fall_hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}}
// A waterfall's piece (render/waterfalls.rs): kind 5 a sheet segment (uv x across in m, y down its
// length in m; rest x the time, y its opacity, z how fast its streaks run down), 6 its foam (uv −1 to
// 1; rest y the opacity, z the sheet's width), 7 a spray puff (uv −1 to 1; rest y its opacity). Lit
// as the water's surface is, premultiplied, never glowing.
fn fall_colour(kind: u32, look: u32, uv: vec2f, rest: vec3f, rel: vec3f) -> vec4f {{
    let k = min(look, {last}u);
    var c: vec3f;
    var a: f32;
    if (kind == 5u) {{
        let v = uv.y - rest.x * rest.z;
        let s = FALL_STREAKS[k];
        let streak = smoothstep(0.55, 0.9, fall_noise(vec2f(uv.x * s, v * 0.6)));
        let body = 0.75 + 0.25 * fall_noise(vec2f(uv.x * s * 0.5 + 7.0, v * 0.25));
        c = mix(FALL_SHEETS[k], FALL_FOAMS[k], 0.6 * streak);
        a = clamp(rest.y * body + 0.2 * streak, 0.0, 1.0);
    }} else if (kind == 6u) {{
        let d = length(uv);
        let churn = fall_noise(uv * 3.0 + vec2f(rest.x * 0.7, -rest.x * 0.5));
        a = (1.0 - smoothstep(0.45, 1.0, d)) * rest.y * (0.55 + 0.45 * churn);
        c = FALL_FOAMS[k];
    }} else {{
        a = (1.0 - smoothstep(0.3, 1.0, length(uv))) * rest.y;
        c = FALL_SPRAYS[k];
    }}
    if (a < 0.01) {{
        return vec4f(0.0);
    }}
    let y = frame.eye.x + rel.y;
    let up = face_up(face_at(y));
    var l = terrain_light(1.0, vec2f(1.0, 0.0));
    l.direct = shadow_light(rel, up, l.direct);
    let lit = lambert_lit(c, up, y, l);
    return vec4f(lit * a, a);
}}
",
        last = n - 1,
    )
}

#[cfg(test)]
mod tests {
    use super::super::flames::VERTEX_FLOATS;
    use super::*;
    use dwell_world::FaceFrame;

    fn fall(f: FaceFrame) -> Fall {
        Fall {
            face: f,
            x: 10,
            z: -4,
            dir: 0,
            width: 2,
            lip: 104.0,
            land: 100.0,
            speed: 0.6,
            flow: 0.5,
            fluid: 0,
        }
    }

    fn basis() -> Basis {
        Basis {
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            back: [0.0, 0.0, 1.0],
        }
    }

    dwell_world::both_faces!(
        fn a_sheet_hangs_from_the_lip_to_the_landing_beyond_the_face(f: FaceFrame) {
            let fall = fall(f);
            let segs = FALL_LOOKS[0].sheet_segments as usize;
            let eye = f.point([0.0, 100.0, 0.0]);
            let (v, n) = fall_quads(&[fall], eye, &basis(), 1.0, false);
            assert_eq!(n, 1);
            // a CPU adapter: the sheet's segments and the foam, no spray
            assert_eq!(v.len(), (segs + 1) * 6 * VERTEX_FLOATS);
            let sheet = &v[..segs * 6 * VERTEX_FLOATS];
            let local: Vec<[f64; 3]> = sheet
                .chunks_exact(VERTEX_FLOATS)
                .map(|c| {
                    f.point([
                        f64::from(c[0]) + eye[0],
                        f64::from(c[1]) + eye[1],
                        f64::from(c[2]) + eye[2],
                    ])
                })
                .collect();
            let (lo, hi) = local
                .iter()
                .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
            assert!(
                (hi - 104.0).abs() < 1e-3 && (lo - 100.0).abs() < 1e-3,
                "{lo}..{hi}"
            );
            for p in &local {
                assert!(p[0] >= 11.0 && p[0] < 12.0, "beyond the face x = 11: {p:?}");
                assert!(
                    p[2] >= -4.0 - 1e-6 && p[2] <= -2.0 + 1e-6,
                    "along the lip: {p:?}"
                );
            }
            // the top lies on the lip, the foot thrown out
            let top = local
                .iter()
                .filter(|p| p[1] > 103.99)
                .map(|p| p[0])
                .fold(f64::MAX, f64::min);
            let foot = local
                .iter()
                .filter(|p| p[1] < 100.01)
                .map(|p| p[0])
                .fold(0.0, f64::max);
            assert!(top < 11.1 && foot > top + 0.3, "{top} → {foot}");
        }
    );

    #[test]
    fn spray_and_motion_only_on_a_gpu_and_far_falls_are_not_drawn() {
        // face-a-only: counts and the time, not where things are
        let f = fall(FaceFrame::A);
        let segs = FALL_LOOKS[0].sheet_segments as usize;
        let eye = [0.0, 100.0, 0.0];
        let (v, _) = fall_quads(&[f], eye, &basis(), 1.0, true);
        let puffs = v.len() / (6 * VERTEX_FLOATS) - segs - 1;
        let want = (f64::from(FALL_LOOKS[0].spray_count) * 2.0 * f.strength()).round() as usize;
        assert_eq!(puffs, want);
        assert!(v[5] > 0.0, "the time moves the streaks");
        let (still, _) = fall_quads(&[f], eye, &basis(), 1.0, false);
        assert_eq!(still[5], 0.0, "a CPU adapter's sheet is still");
        let far = [10_000.0, 0.0, 0.0];
        assert_eq!(fall_quads(&[f], far, &basis(), 1.0, true).1, 0);
    }

    #[test]
    fn a_stronger_fall_is_more_opaque() {
        // face-a-only: the opacity by the flow
        let look = &FALL_LOOKS[0];
        let thin = Fall {
            flow: 0.0,
            ..fall(FaceFrame::A)
        };
        let full = Fall {
            flow: 10.0,
            ..fall(FaceFrame::A)
        };
        assert!((opacity(look, &thin) - look.sheet_opacity_thin).abs() < 1e-9);
        assert!((opacity(look, &full) - look.sheet_opacity_full).abs() < 1e-9);
    }
}
