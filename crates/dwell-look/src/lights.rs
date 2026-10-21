//! Dynamic lights (docs/LIGHTS.md §3, ADR 0047): the smooth point lights of light sources (the
//! blocks' `light`, `dwell_blocks::LightInfo`) — a torch in your hand, in other players' hands and,
//! with Torch shadows on (Phase 47 T2), placed. They are drawn by the renderer only: the flood light
//! (`dwell_world::light`) never sees a held light, and a placed one reaches it through its block's
//! `emits`. This is the platform-free part: the tunables, the falloff, the flicker, the choice of the
//! nearest lights a frame and the uniform block they are packed into; the shader's `point_lights`
//! (render/lights.rs) mirrors [`irradiance`].
//!
//! Point lights have no up: nothing here asks which face it is on (a light on face B is placed by
//! its holder's `FaceFrame` and shades as on face A).

#[path = "lights.gen.rs"]
mod generated;
pub use generated::LIGHTS;

/// The most lights the shader's array holds.
pub const MAX_SHADER_LIGHTS: usize = 16;
/// Floats in the uniform block: a header vec4 (x the count) and two vec4s per light.
pub const LIGHT_FLOATS: usize = 4 + MAX_SHADER_LIGHTS * 8;

/// `shared/look/lights.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightTunables {
    /// The most drawn a frame, nearest first.
    pub max_lights: usize,
    /// A light farther than this from the eye is left out (m).
    pub cull_m: f64,
    /// The falloff's curve: a light's share `d` metres out is `x / (curve − (curve − 1)·x)` of
    /// `x = 1 − d / radius`, the flood light's block-light curve over its levels (a level a
    /// metre), so a held torch lights as a placed one does.
    pub curve: f64,
    /// How far round a surface the light wraps: `(n·l + wrap) / (1 + wrap)`, so a surface edge-on
    /// to a flame isn't black.
    pub wrap: f64,
    /// The most a glowing surface gives off as the tone curve's input (after the exposure): past
    /// it the glow is scaled down whole, so its hue holds (a flame stays orange, not cream).
    pub glow_peak: f64,
}

/// One dynamic light this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// World position (the flame).
    pub pos: [f64; 3],
    /// Linear colour.
    pub color: [f64; 3],
    /// Irradiance at the flame.
    pub strength: f64,
    pub radius_m: f64,
    /// The share of its strength a flicker takes at most, and its noise's cycles a second.
    pub flicker: f64,
    pub flicker_speed: f64,
    /// Its own flicker's phase (a stable id: the holder's, the cell's).
    pub seed: u32,
    /// May cast shadows (Torch shadows, T2).
    pub shadows: bool,
}

impl PointLight {
    /// A light source's light at `pos`, flickering with phase `seed`.
    pub fn of(source: &dwell_blocks::LightInfo, pos: [f64; 3], seed: u32) -> Self {
        Self {
            pos,
            color: crate::hex(source.color).map(crate::srgb_to_linear),
            strength: f64::from(source.strength),
            radius_m: f64::from(source.radius_m),
            flicker: f64::from(source.flicker),
            flicker_speed: f64::from(source.flicker_speed),
            seed,
            shadows: source.shadows,
        }
    }
}

/// The light's share at distance `d` (m) of a light of `radius`: 1 at the flame, 0 at the radius,
/// `x / (curve − (curve − 1)·x)` of `x = 1 − d / radius` between (the flood light's curve).
pub fn falloff(d: f64, radius: f64, curve: f64) -> f64 {
    if d >= radius {
        return 0.0;
    }
    let x = 1.0 - d / radius;
    x / (curve - (curve - 1.0) * x)
}

/// What a self-lit surface of linear colour `c` (its colour × its `emissive`) gives off, in the
/// scene's light at `exposure` (the frame's, × the time of day's): the tone curve sees `c` itself
/// whatever the exposure (the light is the source: owner, 2026-10-21, "the flame should stay
/// illuminated regardless of brightness"), scaled down whole so its brightest channel is
/// `LIGHTS.glow_peak` at most, keeping its hue. The shader's `glow` mirrors it.
pub fn glow(c: [f64; 3], exposure: f64) -> [f64; 3] {
    let peak = c[0].max(c[1]).max(c[2]);
    let k = if peak > LIGHTS.glow_peak {
        LIGHTS.glow_peak / peak
    } else {
        1.0
    };
    c.map(|v| v * k * 0.6 / exposure.max(1e-6))
}

/// A surface of albedo `albedo` lit to `lit` (linear) with `emissive` `shine`: its lit colour
/// replaced by up to all of its self-lit glow (`shine` 1 or more: fully self-lit, unaffected by
/// the sun, ambient, flood light, corner occlusion and shadows). The shaders' last step for
/// terrain, cutouts and characters mirrors it.
pub fn emitted(lit: [f64; 3], albedo: [f64; 3], shine: f64, exposure: f64) -> [f64; 3] {
    let g = glow(albedo.map(|v| v * shine), exposure);
    std::array::from_fn(|k| lit[k] * (1.0 - shine.min(1.0)) + g[k])
}

/// The share of a light reaching a surface at `cos` to it, wrapped by `wrap`.
pub fn wrapped(cos: f64, wrap: f64) -> f64 {
    ((cos + wrap) / (1.0 + wrap)).max(0.0)
}

fn hash01(seed: u32, i: i64) -> f64 {
    let mut h = u64::from(seed) ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth value noise in [0, 1] at `x` (cycles), its own per `seed`.
fn noise(seed: u32, x: f64) -> f64 {
    let i = x.floor();
    let f = x - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash01(seed, i as i64);
    let b = hash01(seed, i as i64 + 1);
    a + (b - a) * s
}

/// The flicker's factor on a light's strength at time `t` (s): in [1 − amount, 1], two octaves of
/// smooth noise at `speed` and twice it, so a torch wavers rather than strobes. Deterministic in
/// (`t`, `seed`); `amount` 0 is steady.
pub fn flicker(t: f64, seed: u32, amount: f64, speed: f64) -> f64 {
    if amount <= 0.0 {
        return 1.0;
    }
    let n = 0.65 * noise(seed, t * speed) + 0.35 * noise(seed ^ 0x5bd1_e995, t * speed * 2.0);
    1.0 - amount * n
}

/// The lights to draw this frame: those within `cull_m` of the eye, nearest first (ties by seed,
/// so the choice is stable), at most `max`.
pub fn pick(lights: &[PointLight], eye: [f64; 3], max: usize, cull_m: f64) -> Vec<PointLight> {
    let dist2 = |l: &PointLight| (0..3).map(|k| (l.pos[k] - eye[k]).powi(2)).sum::<f64>();
    let mut out: Vec<(f64, PointLight)> = lights
        .iter()
        .map(|l| (dist2(l), *l))
        .filter(|(d, _)| *d <= cull_m * cull_m)
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.seed.cmp(&b.1.seed)));
    out.truncate(max.min(MAX_SHADER_LIGHTS));
    out.into_iter().map(|(_, l)| l).collect()
}

/// The uniform block (`PointLights` in the shader): x of the header the count; per light its
/// position relative to the eye and radius, then its colour × strength × flicker at `t`.
pub fn pack(lights: &[PointLight], eye: [f64; 3], t: f64) -> [f32; LIGHT_FLOATS] {
    let mut u = [0.0f32; LIGHT_FLOATS];
    let n = lights.len().min(MAX_SHADER_LIGHTS);
    u[0] = n as f32;
    for (i, l) in lights.iter().take(n).enumerate() {
        let o = 4 + i * 8;
        let k = l.strength * flicker(t, l.seed, l.flicker, l.flicker_speed);
        u[o..o + 8].copy_from_slice(&[
            (l.pos[0] - eye[0]) as f32,
            (l.pos[1] - eye[1]) as f32,
            (l.pos[2] - eye[2]) as f32,
            l.radius_m as f32,
            (l.color[0] * k) as f32,
            (l.color[1] * k) as f32,
            (l.color[2] * k) as f32,
            0.0,
        ]);
    }
    u
}

/// The irradiance (linear RGB) the packed lights give a surface at `rel` (relative to the eye) with
/// unit normal `n`: the wrapped cosine × the falloff (`LIGHTS`' curve and wrap), summed. The
/// shader's `point_lights` mirrors it.
pub fn irradiance(u: &[f32; LIGHT_FLOATS], rel: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for i in 0..(u[0] as usize).min(MAX_SHADER_LIGHTS) {
        let o = 4 + i * 8;
        let to = [
            f64::from(u[o]) - rel[0],
            f64::from(u[o + 1]) - rel[1],
            f64::from(u[o + 2]) - rel[2],
        ];
        let d = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        let cos = if d > 1e-4 {
            wrapped(
                (to[0] * n[0] + to[1] * n[1] + to[2] * n[2]) / d,
                LIGHTS.wrap,
            )
        } else {
            1.0
        };
        let w = cos * falloff(d, f64::from(u[o + 3]), LIGHTS.curve);
        for (k, s) in sum.iter_mut().enumerate() {
            *s += f64::from(u[o + 4 + k]) * w;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::{FaceFrame, both_faces};

    fn torch() -> &'static dwell_blocks::LightInfo {
        dwell_blocks::light("dwell:torch").expect("the build has the torch light")
    }

    #[test]
    fn the_falloff_is_one_at_the_flame_ends_at_the_radius_and_falls_as_the_block_light() {
        let c = LIGHTS.curve;
        assert_eq!(falloff(0.0, 10.0, c), 1.0);
        assert_eq!(falloff(10.0, 10.0, c), 0.0);
        assert_eq!(falloff(12.0, 10.0, c), 0.0);
        let mut last = f64::INFINITY;
        for k in 0..100 {
            let f = falloff(f64::from(k) * 0.1, 10.0, c);
            assert!(f < last, "{k}");
            last = f;
        }
        // A light of radius 15 falls as the flood light's block light from level 15, a level a
        // metre, on the same curve.
        let block = dwell_world::light::LIGHT.block_curve;
        let level = |l: f64| (l / 15.0) / (block - (block - 1.0) * (l / 15.0));
        for d in [0.0, 3.0, 7.5, 12.0] {
            assert!((falloff(d, 15.0, block) - level(15.0 - d)).abs() < 1e-12);
        }
        assert_eq!(LIGHTS.curve, block, "the data keeps them alike");
        // Wrapped: edge-on still lit, facing away none.
        assert!(wrapped(0.0, LIGHTS.wrap) > 0.0);
        assert_eq!(wrapped(-1.0, LIGHTS.wrap), 0.0);
        assert_eq!(wrapped(1.0, LIGHTS.wrap), 1.0);
    }

    #[test]
    fn the_flicker_wavers_inside_its_amount_and_is_deterministic() {
        let (amount, speed) = (0.2, 6.0);
        let mut lo = 1.0f64;
        let mut hi = 0.0f64;
        let mut biggest_step = 0.0f64;
        let mut prev = flicker(0.0, 7, amount, speed);
        for k in 1..6000 {
            let t = f64::from(k) / 600.0;
            let f = flicker(t, 7, amount, speed);
            assert_eq!(f, flicker(t, 7, amount, speed));
            assert!((1.0 - amount..=1.0).contains(&f), "{f}");
            biggest_step = biggest_step.max((f - prev).abs());
            prev = f;
            lo = lo.min(f);
            hi = hi.max(f);
        }
        assert!(hi - lo > amount * 0.4, "it moves: {lo}..{hi}");
        assert!(
            biggest_step < amount * 0.1,
            "smoothly at 600 Hz: {biggest_step}"
        );
        assert_ne!(
            flicker(1.3, 7, amount, speed),
            flicker(1.3, 8, amount, speed),
            "its own phase per seed"
        );
        assert_eq!(flicker(1.3, 7, 0.0, speed), 1.0, "steady without flicker");
    }

    #[test]
    fn the_nearest_lights_within_reach_are_picked_at_most_the_budget() {
        let at = |x: f64, seed| PointLight::of(torch(), [x, 0.0, 0.0], seed);
        let lights: Vec<_> = (0..30).map(|k| at(f64::from(30 - k) * 3.0, k)).collect();
        let eye = [0.0; 3];
        let picked = pick(&lights, eye, LIGHTS.max_lights, LIGHTS.cull_m);
        assert_eq!(picked.len(), LIGHTS.max_lights.min(MAX_SHADER_LIGHTS));
        let xs: Vec<f64> = picked.iter().map(|l| l.pos[0]).collect();
        assert_eq!(xs[0], 3.0, "the nearest first");
        assert!(xs.windows(2).all(|w| w[0] <= w[1]));
        assert!(
            pick(&lights, eye, 64, 10.0)
                .iter()
                .all(|l| l.pos[0] <= 10.0),
            "none past the cull"
        );
        assert!(
            pick(&lights, eye, 64, LIGHTS.cull_m).len() <= MAX_SHADER_LIGHTS,
            "the shader's array"
        );
        // Ties by seed: the order doesn't depend on the input's.
        let mut rev = lights.clone();
        rev.reverse();
        let same = [at(5.0, 2), at(5.0, 1)];
        assert_eq!(pick(&same, eye, 1, 64.0)[0].seed, 1);
        assert_eq!(pick(&rev, eye, 4, 64.0), pick(&lights, eye, 4, 64.0));
    }

    #[test]
    fn a_light_source_added_by_data_alone_lights_a_surface() {
        // The fixture's `test:lantern` (shared/blocks/fixtures/test.json), by its data alone.
        let lantern = dwell_blocks::fixture::LIGHTS
            .iter()
            .find(|l| l.name == "test:lantern")
            .expect("the fixture has test:lantern");
        let lamp = dwell_blocks::fixture::STATES
            .iter()
            .find(|s| s.name == "test:lamp")
            .expect("the fixture has test:lamp");
        assert_eq!(lamp.light.map(|l| l.name), Some("test:lantern"));
        let l = PointLight::of(lantern, [0.0, 2.0, 0.0], 1);
        let u = pack(&[l], [0.0; 3], 0.0);
        let e = irradiance(&u, [0.0; 3], [0.0, 1.0, 0.0]);
        assert!(e[2] > e[0], "bluish: {e:?}");
        assert!(e[2] > 0.1);
        // Past its radius (6 m), nothing.
        let far = irradiance(&u, [0.0, -5.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(far, [0.0; 3]);
    }

    #[test]
    fn a_surface_facing_away_takes_none_and_the_held_light_never_needs_the_flood_light() {
        let l = PointLight::of(torch(), [0.0, 1.0, 0.0], 3);
        let u = pack(&[l], [0.0; 3], 0.0);
        let up = irradiance(&u, [0.0; 3], [0.0, 1.0, 0.0]);
        let down = irradiance(&u, [0.0; 3], [0.0, -1.0, 0.0]);
        assert!(up[0] > 0.5, "{up:?}");
        assert_eq!(down, [0.0; 3]);
        assert!(up[0] > up[2], "warm");
    }

    both_faces!(
        fn a_torch_over_the_ground_lights_it_alike_on_both_faces(f: FaceFrame) {
            // A torch 1.5 m over the ground, 3 m in front: the ground under it (face-local up
            // normal) as face A sees it.
            let ground = f.point([10.0, 4.0, -3.0]);
            let flame = f.point([10.0, 5.5, 0.0]);
            let eye = f.point([10.0, 5.6, -3.0]);
            let u = pack(&[PointLight::of(torch(), flame, 1)], eye, 2.0);
            let rel = std::array::from_fn(|k| ground[k] - eye[k]);
            let e = irradiance(&u, rel, f.vector([0.0, 1.0, 0.0]));
            let a = FaceFrame::A;
            let ga = a.point([10.0, 4.0, -3.0]);
            let ea = a.point([10.0, 5.6, -3.0]);
            let ua = pack(
                &[PointLight::of(torch(), a.point([10.0, 5.5, 0.0]), 1)],
                ea,
                2.0,
            );
            let ra = std::array::from_fn(|k| ga[k] - ea[k]);
            let ref_e = irradiance(&ua, ra, [0.0, 1.0, 0.0]);
            for k in 0..3 {
                assert!((e[k] - ref_e[k]).abs() < 1e-6, "{e:?} vs {ref_e:?}");
            }
            assert!(e[0] > 0.0);
        }
    );
}
