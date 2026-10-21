//! Motes (docs/BIFACIAL_WORLD.md §12.1): soft glowing particles drifting and swirling in a portal's
//! shaft round the midplane crossing (owner, 2026-10-21: "add some blue magical particles at the
//! transition zone"). Each is a camera-facing disc that gives off its colour whatever the light
//! (self-lit, as a flame's embers), pulsing gently at its own phase.
//!
//! This is the platform-free part: the looks (`shared/look/motes.json`, generated into
//! `motes.gen.rs`), where a portal's motes are at a time ([`portal_motes`], written once face-local
//! and crossed into the world through `FaceFrame`: half of them on each face's side of the
//! midplane), the disc's CPU twin ([`mote_shade`]) and its WGSL ([`wgsl`]). The renderer
//! (dwell-client render/flames.rs) draws them with the flames' embers.

use dwell_world::FaceFrame;

#[rustfmt::skip]
#[path = "motes.gen.rs"]
mod generated;
pub use generated::{MOTE_TUNABLES, MOTES};

/// `shared/look/motes.json`'s tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoteTunables {
    /// A portal's motes are drawn while the eye is within this of its crossing point (m).
    pub draw_m: f64,
    /// The most drawn a frame.
    pub max_motes: usize,
}

/// A mote look (`motes` in `shared/look/motes.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoteLook {
    /// Namespaced name, e.g. `dwell:portal` (a portal's `motes`).
    pub name: &'static str,
    /// sRGB `0xRRGGBB`: the motes' colour and their centres'.
    pub color: u32,
    pub core: u32,
    /// How many a portal has (half on each face's side), and how many a CPU adapter draws.
    pub count: u32,
    pub cpu_count: u32,
    /// A mote's radius (m).
    pub size_m: f32,
    /// How far from the axis they drift (m), and how far either side of the midplane.
    pub radius_m: f32,
    pub height_m: f32,
    /// Turns a second round the axis (each mote between half and all of it, half the other way).
    pub swirl: f32,
    /// Rising and falling along the shaft (m/s).
    pub drift: f32,
    /// A slow sideways wander (m).
    pub wobble_m: f32,
    /// Brightness (the colour × this).
    pub glow: f32,
    /// The pulse: its rate and depth (0 steady, 1 to dark).
    pub pulse_hz: f32,
    pub pulse_depth: f32,
}

/// A mote look's index by name.
pub fn find(name: &str) -> Option<usize> {
    MOTES.iter().position(|m| m.name == name)
}

/// One mote at a time: its place (world, metres) and brightness (0–1, × the look's glow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mote {
    pub at: [f64; 3],
    pub brightness: f32,
}

fn hash(k: u32, salt: u32, seed: u32) -> f32 {
    let mut h = k
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(salt.wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h as f32 / 4_294_967_295.0
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How many motes of `look` are drawn: all, or a CPU adapter's few.
pub fn drawn_count(look: &MoteLook, animated: bool) -> u32 {
    if animated { look.count } else { look.cpu_count }
}

/// A portal's motes at `seconds`: the shaft's axis through world (x, z) = `axis`. Each mote circles the axis at its own rate and radius, rises and falls
/// along its band (face-local height 0 … `height_m`, wrapping and fading at both ends) and wanders
/// a little sideways; mote k is on face A's side for even k, face B's for odd. A CPU adapter
/// (`animated` false) draws `cpu_count` of them. `seed` is the portal's (each pulses apart).
pub fn portal_motes(
    look: &MoteLook,
    axis: [f64; 2],
    seconds: f64,
    animated: bool,
    seed: u32,
) -> Vec<Mote> {
    let t = seconds.rem_euclid(4096.0) as f32;
    let tau = std::f32::consts::TAU;
    (0..drawn_count(look, animated))
        .map(|k| {
            let face = if k % 2 == 0 {
                FaceFrame::A
            } else {
                FaceFrame::B
            };
            let h = |salt| hash(k, salt, seed);
            // Round the axis: radius (area-even), start angle, turning rate and sense.
            let r = look.radius_m * h(1).sqrt();
            let sense = if h(2) < 0.5 { -1.0 } else { 1.0 };
            let turn = tau * look.swirl * (0.5 + 0.5 * h(3)) * sense;
            let angle = tau * h(4) + turn * t;
            let wob = look.wobble_m * (t * (0.3 + 0.4 * h(5)) + tau * h(6)).sin();
            let rr = (r + wob).clamp(0.0, look.radius_m);
            // Along the band: a rise (or fall) that wraps, fading in and out at its ends.
            let span = look.height_m.max(1e-3);
            let dir = if h(7) < 0.5 { -1.0 } else { 1.0 };
            let s = (h(8) + dir * look.drift * (0.6 + 0.8 * h(9)) * t / span).rem_euclid(1.0);
            let fade = smooth(0.0, 0.15, s) * (1.0 - smooth(0.85, 1.0, s));
            let pulse =
                1.0 - look.pulse_depth * (0.5 + 0.5 * (tau * (look.pulse_hz * t + h(10))).sin());
            // Face-local (its height from the midplane) into the world.
            Mote {
                at: face.point([
                    axis[0] + f64::from(rr * angle.cos()),
                    f64::from(s * span),
                    axis[1] + f64::from(rr * angle.sin()),
                ]),
                brightness: (fade * pulse).clamp(0.0, 1.0),
            }
        })
        .collect()
}

/// A mote's disc at `r` (its distance from the centre over its size) and `v` (its brightness):
/// the colour (linear), the core's at the centre, fading to nothing at its edge.
pub fn mote_shade(look: &MoteLook, r: f32, v: f32) -> [f32; 3] {
    let a = (1.0 - smooth(0.0, 1.0, r)).powi(2);
    let core = (1.0 - smooth(0.0, 0.45, r)).powi(2);
    let (c, k) = (linear(look.color), linear(look.core));
    std::array::from_fn(|i| (c[i] + (k[i] - c[i]) * core) * a * v * look.glow)
}

fn linear(c: u32) -> [f32; 3] {
    crate::hex(c).map(|v| crate::srgb_to_linear(v) as f32)
}

/// The motes' WGSL: `mote_look` (one case per look) and `mote_shade`, mirroring [`mote_shade`].
pub fn wgsl() -> String {
    let v3 = |c: [f32; 3]| format!("vec3f({:?}, {:?}, {:?})", c[0], c[1], c[2]);
    let mut cases = String::new();
    for (i, m) in MOTES.iter().enumerate() {
        cases.push_str(&format!(
            "        case {i}u: {{ return MoteLook({}, {}, {:?}); }}\n",
            v3(linear(m.color)),
            v3(linear(m.core)),
            m.glow,
        ));
    }
    format!(
        r"
struct MoteLook {{
    color: vec3f,
    core: vec3f,
    glow: f32,
}};

fn mote_look(i: u32) -> MoteLook {{
    switch i {{
{cases}        default: {{ return MoteLook(vec3f(0.0), vec3f(0.0), 0.0); }}
    }}
}}

// A mote's disc at `uv` (−1 to 1 across) and brightness `v`: `mote_shade` in dwell-look motes.rs.
fn mote_shade(look: MoteLook, uv: vec2f, v: f32) -> vec3f {{
    let r = length(uv);
    let a = pow(1.0 - smoothstep(0.0, 1.0, r), 2.0);
    let core = pow(1.0 - smoothstep(0.0, 0.45, r), 2.0);
    return mix(look.color, look.core, core) * (a * v * look.glow);
}}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal() -> &'static MoteLook {
        &MOTES[find("dwell:portal").expect("the portal's motes")]
    }

    #[test]
    fn a_portals_motes_stay_in_its_shaft_round_the_midplane_on_both_faces() {
        let look = portal();
        let axis = [-4871.5, 20185.5];
        let (mut a, mut b) = (0, 0);
        for step in 0..200 {
            let t = f64::from(step) * 0.37;
            let motes = portal_motes(look, axis, t, true, 7);
            assert_eq!(motes.len(), look.count as usize);
            for m in &motes {
                let d = ((m.at[0] - axis[0]).powi(2) + (m.at[2] - axis[1]).powi(2)).sqrt();
                assert!(d <= f64::from(look.radius_m) + 1e-6, "{d}");
                assert!(
                    m.at[1].abs() <= f64::from(look.height_m) + 1e-6,
                    "{}",
                    m.at[1]
                );
                assert!((0.0..=1.0).contains(&m.brightness));
                // Each face's side through the face frame: face B's are below the midplane.
                if FaceFrame::at_height(m.at[1]) == FaceFrame::A {
                    a += 1;
                } else {
                    b += 1;
                }
            }
        }
        // Half on each side.
        let n = 200 * look.count as i32;
        assert!(
            (a - n / 2).abs() <= n / 20 && (b - n / 2).abs() <= n / 20,
            "{a} {b}"
        );
    }

    #[test]
    fn face_b_motes_are_face_a_motes_mirrored_through_the_face_frame() {
        // both-faces: a mote on face B's side is the mirror of the same face-local mote on face
        // A's (its height through `FaceFrame::B`), so both openings see the same swirl.
        let look = portal();
        let motes = portal_motes(look, [0.0, 0.0], 12.5, true, 3);
        for pair in motes.chunks(2) {
            if let [a, b] = pair {
                // Face-local heights in the band on both sides.
                assert!((0.0..=f64::from(look.height_m)).contains(&FaceFrame::A.y(a.at[1])));
                assert!((0.0..=f64::from(look.height_m)).contains(&FaceFrame::B.y(b.at[1])));
            }
        }
        // The mirror world's motes: each face-B mote is where face A's would be, mirrored.
        for (k, m) in motes.iter().enumerate().filter(|(k, _)| k % 2 == 1) {
            let a = FaceFrame::B.point(m.at);
            assert!(a[1] >= 0.0, "mote {k}: {a:?}");
        }
    }

    #[test]
    fn they_move_and_pulse_and_a_cpu_adapter_draws_its_few() {
        let look = portal();
        let a = portal_motes(look, [0.0, 0.0], 10.0, true, 1);
        let b = portal_motes(look, [0.0, 0.0], 10.5, true, 1);
        let moved = a.iter().zip(&b).filter(|(p, q)| p.at != q.at).count();
        assert!(moved > a.len() / 2, "{moved}");
        let pulse: Vec<f32> = (0..40)
            .map(|i| portal_motes(look, [0.0, 0.0], f64::from(i) * 0.1, true, 1)[0].brightness)
            .collect();
        let (lo, hi) = pulse
            .iter()
            .fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(hi - lo > 0.1, "{lo} {hi}");
        assert_eq!(
            portal_motes(look, [0.0, 0.0], 1.0, false, 1).len(),
            look.cpu_count as usize
        );
        // The same time and seed, the same motes; another portal's seed, other ones.
        assert_eq!(a, portal_motes(look, [0.0, 0.0], 10.0, true, 1));
        assert_ne!(a, portal_motes(look, [0.0, 0.0], 10.0, true, 2));
    }

    #[test]
    fn a_mote_is_bright_at_its_core_and_gone_at_its_edge() {
        let look = portal();
        let centre = mote_shade(look, 0.0, 1.0);
        let edge = mote_shade(look, 1.0, 1.0);
        assert!(centre.iter().all(|&c| c > 0.0));
        assert!(edge.iter().all(|&c| c.abs() < 1e-6));
        // Blue: its blue the strongest at its rim.
        let rim = mote_shade(look, 0.6, 1.0);
        assert!(rim[2] > rim[0] && rim[2] > rim[1], "{rim:?}");
    }

    #[test]
    fn the_wgsl_parses_and_validates() {
        let src = format!(
            "{}\n@fragment fn f(@location(0) uv: vec2f) -> @location(0) vec4f {{ return vec4f(mote_shade(mote_look(0u), uv, 1.0), 1.0); }}",
            wgsl()
        );
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }
}
