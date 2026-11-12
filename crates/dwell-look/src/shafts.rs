//! Light shafts, the volumetric glow rays (ADR 0064): the sun's (and faintly the moon's) shafts
//! in the air, a radial blur toward the light's place on the screen over the open sky, and the
//! sun's shafts under a fluid's surface, gathered along each view ray from the surface's lit cells
//! down the sun's refracted direction; drawn at a reduced resolution by the Light shafts setting's
//! tier (`shared/look/shafts.json`, generated into `shafts.gen.rs`). The renderer packs
//! [`shaft_uniforms`] into the frame block.

use dwell_world::FaceFrame;

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("shafts.gen.rs");
}
pub use generated::{
    DEFAULT_FLUID_SHAFTS, DEFAULT_LIGHT_SHAFTS, FLUID_SHAFTS, HIGH, LIGHT_KINDS, LIGHT_SHAFTS, LOW,
    MAX_SHAFT_LIGHTS, MOON_SHAFTS, SUN_SHAFTS,
};

/// The frame block's vec4s for the lights' shafts (`frame.torch_shafts`): a header and two per
/// light.
pub const LIGHT_SHAFT_VEC4S: usize = 1 + 2 * MAX_SHAFT_LIGHTS;

/// A tier of the Light shafts setting: the shafts' buffer against the drawing buffer (each axis)
/// and the samples a pixel takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaftTier {
    pub scale: f64,
    pub steps: u32,
    /// The most placed and held lights drawn with shafts, and the samples toward each.
    pub lights: u32,
    pub light_steps: u32,
}

/// The placed and held lights' shafts (`lights`): torches and lamps in caves, at night, indoors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightShaftTuning {
    /// Lights nearer the eye than this fade out, none within half of it (a torch in your own
    /// hand), m.
    pub near_m: f64,
    /// The farthest a light may be from the eye (fading out over its last half), m.
    pub range_m: f64,
    /// The share of the way to the light sampled; each twelfth of the way's weight on the last.
    pub reach: f64,
    pub decay: f64,
    /// How far past the screen's edge (half screen heights) a light may be, faded.
    pub offscreen: f64,
    /// The share of the strength drawn by full day (all of it at night).
    pub day_share: f64,
    /// The most a halo spans on the screen (half screen heights).
    pub max_disc: f64,
}

/// A light source's shafts by its name (`lights.kinds`, `lights.default`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightShafts {
    /// The light source's name (shared/blocks `lights`; empty for the default).
    pub light: &'static str,
    /// The glow's brightness (× the light's colour; 0: none).
    pub strength: f64,
    /// The glowing halo's radius round the flame (m).
    pub disc_m: f64,
}

/// Light source `light`'s shafts (the default when it has none of its own).
pub fn light_shafts(light: &str) -> &'static LightShafts {
    LIGHT_KINDS
        .iter()
        .find(|l| l.light == light)
        .unwrap_or(&DEFAULT_LIGHT_SHAFTS)
}

/// A placed or held light the shafts may come from: its flame (world), its colour (sRGB, 0–1)
/// and its light source's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaftLight {
    pub pos: [f64; 3],
    pub color: [f64; 3],
    pub light: &'static str,
}

/// A light picked for this frame's shafts: its screen point (NDC), its depth along the view (m),
/// its halo's radius (half screen heights), and its glow's colour × weight (sRGB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickedLight {
    pub ndc: [f64; 2],
    pub depth: f64,
    pub disc: f64,
    pub glow: [f64; 3],
    /// Its distance from the eye (m), what the nearest are picked by.
    pub dist: f64,
}

/// The lights whose shafts a frame draws at `tier`: those of a kind with a strength, in front of
/// the camera, within the range and not too far off the screen, weighed by their kind's strength,
/// the near, range and screen fades and the time of day, the nearest `tier.lights` (ties by
/// position, so the choice is stable).
pub fn pick_shaft_lights(tier: ShaftTier, v: &ShaftView) -> Vec<PickedLight> {
    let t = &LIGHT_SHAFTS;
    let night = 1.0 - smooth(-0.1, 0.1, v.face.vector(v.sun)[1]);
    let day = t.day_share + (1.0 - t.day_share) * night;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut out: Vec<(PickedLight, [f64; 3])> = v
        .lights
        .iter()
        .filter_map(|l| {
            let kind = light_shafts(l.light);
            let rel: [f64; 3] = std::array::from_fn(|k| l.pos[k] - v.eye[k]);
            let dist = dot(rel, rel).sqrt();
            let z = dot(rel, v.forward);
            if kind.strength <= 0.0 || dist >= t.range_m || z <= 0.05 {
                return None;
            }
            let ndc = [
                dot(rel, v.right) / z / v.tan[0],
                dot(rel, v.up) / z / v.tan[1],
            ];
            let past =
                ((ndc[0].abs() - 1.0).max(0.0) * v.aspect).max((ndc[1].abs() - 1.0).max(0.0));
            let screen = if t.offscreen > 0.0 {
                1.0 - smooth(0.0, t.offscreen, past)
            } else if past > 0.0 {
                0.0
            } else {
                1.0
            };
            let range = 1.0 - smooth(0.5 * t.range_m, t.range_m, dist);
            let near = if t.near_m > 0.0 {
                smooth(0.5 * t.near_m, t.near_m, dist)
            } else {
                1.0
            };
            let weight = kind.strength * screen * range * near * day;
            (weight > 0.0).then(|| {
                (
                    PickedLight {
                        ndc,
                        depth: z,
                        disc: (kind.disc_m / (z * v.tan[1])).min(t.max_disc),
                        glow: l.color.map(|c| c * weight),
                        dist,
                    },
                    l.pos,
                )
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.0.dist.total_cmp(&b.0.dist).then(
            a.1[0]
                .total_cmp(&b.1[0])
                .then(a.1[1].total_cmp(&b.1[1]))
                .then(a.1[2].total_cmp(&b.1[2])),
        )
    });
    out.truncate((tier.lights as usize).min(MAX_SHAFT_LIGHTS));
    out.into_iter().map(|(p, _)| p).collect()
}

/// Each sample's weight on the one before at `tier`: the data's decay is per twelfth of the way,
/// so Low's fewer samples weigh the way as High's do.
pub fn sample_decay(tier: ShaftTier) -> f64 {
    LIGHT_SHAFTS
        .decay
        .powf(12.0 / f64::from(tier.light_steps.max(1)))
}

/// The frame block's lights' shafts (`frame.torch_shafts`): header x the count (0: none), y the
/// samples toward each, z the reach, w the decay; per light its NDC point, depth (m) and halo
/// radius (half screen heights), then its glow (sRGB × weight). `tier` None (Off): none.
pub fn light_shaft_uniforms(
    tier: Option<ShaftTier>,
    v: &ShaftView,
) -> [[f32; 4]; LIGHT_SHAFT_VEC4S] {
    let mut u = [[0.0f32; 4]; LIGHT_SHAFT_VEC4S];
    let Some(tier) = tier else {
        return u;
    };
    let picked = pick_shaft_lights(tier, v);
    let f = |x: f64| x as f32;
    u[0] = [
        picked.len() as f32,
        tier.light_steps as f32,
        f(LIGHT_SHAFTS.reach),
        f(sample_decay(tier)),
    ];
    for (i, p) in picked.iter().enumerate() {
        u[1 + 2 * i] = [f(p.ndc[0]), f(p.ndc[1]), f(p.depth), f(p.disc)];
        u[2 + 2 * i] = [f(p.glow[0]), f(p.glow[1]), f(p.glow[2]), 0.0];
    }
    u
}

/// A light's shafts in the air (`air.sun`, `air.moon`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirShafts {
    /// The glow's brightness (× the light's colour; 0: none).
    pub strength: f64,
    /// The share of the way to the light a pixel samples.
    pub reach: f64,
    /// Each sample's weight against the one before.
    pub decay: f64,
    /// Half screen heights round the light whose open sky glows.
    pub disc_radius: f64,
    /// The light's height over the horizon where the shafts are full (degrees).
    pub elevation_deg: f64,
    /// How far past the screen's edge (half screen heights) the light may be, faded.
    pub offscreen: f64,
}

/// A fluid's shafts under its surface and its Snell's window (`fluids`, `default`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidShafts {
    /// The registry fluid's name (empty for the default).
    pub fluid: &'static str,
    pub strength: f64,
    /// How far along the view ray they gather (m).
    pub reach_m: f64,
    /// The depth under the surface over which they fade out (m).
    pub fade_m: f64,
    /// The surface cells' size (m) and drift (m/s).
    pub cell_m: f64,
    pub speed: f64,
    /// Snell's window: its glow's colour (sRGB `0xRRGGBB`), how much of it is over the sky, and
    /// the share of its rim softened.
    pub window_color: u32,
    pub window_glow: f64,
    pub window_edge: f64,
}

/// Fluid `fluid`'s shafts (the default when it has none of its own).
pub fn fluid_shafts(fluid: &str) -> &'static FluidShafts {
    FLUID_SHAFTS
        .iter()
        .find(|l| l.fluid == fluid)
        .unwrap_or(&DEFAULT_FLUID_SHAFTS)
}

/// The Light shafts setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaftQuality {
    Off,
    Low,
    High,
}

impl ShaftQuality {
    /// From the settings store's name (`off`, `low`, `high`); anything else is None.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The tier it draws (None: no shafts pass).
    pub fn tier(self) -> Option<ShaftTier> {
        match self {
            Self::Off => None,
            Self::Low => Some(LOW),
            Self::High => Some(HIGH),
        }
    }
}

/// The shafts' buffer for a drawing buffer of `size` at `tier`: each axis scaled, rounded up, at
/// least one pixel.
pub fn buffer_size((w, h): (u32, u32), tier: ShaftTier) -> (u32, u32) {
    let s = |v: u32| ((f64::from(v) * tier.scale).ceil() as u32).max(1);
    (s(w), s(h))
}

/// Where direction `dir` (unit, world) shows on the screen of a camera with axes `right`, `up`,
/// `forward` and half-FOV tangents `tan` (x, y): its NDC point (may be past ±1), or None when it
/// is behind the camera.
pub fn screen_point(
    dir: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    forward: [f64; 3],
    tan: [f64; 2],
) -> Option<[f64; 2]> {
    let d = |a: [f64; 3]| a[0] * dir[0] + a[1] * dir[1] + a[2] * dir[2];
    let z = d(forward);
    if z <= 1e-3 {
        return None;
    }
    Some([d(right) / z / tan[0], d(up) / z / tan[1]])
}

/// How strongly a light's air shafts show (0–1, before `strength`): faded in over its height
/// above the face's horizon (`rise`, the sine of its elevation: none at or under the horizon,
/// full at `elevation_deg`), and out as its screen point (`ndc`, None behind the camera) leaves
/// the screen (full inside it, none `offscreen` half screen heights past its edge; `aspect` the
/// screen's width over its height).
pub fn air_weight(l: &AirShafts, rise: f64, ndc: Option<[f64; 2]>, aspect: f64) -> f64 {
    let Some([x, y]) = ndc else {
        return 0.0;
    };
    let full = l.elevation_deg.to_radians().sin();
    let height = smooth(0.0, full, rise);
    // Past the edge, in half screen heights (x scaled by the aspect).
    let past = ((x.abs() - 1.0).max(0.0) * aspect).max((y.abs() - 1.0).max(0.0));
    let screen = if l.offscreen > 0.0 {
        1.0 - smooth(0.0, l.offscreen, past)
    } else if past > 0.0 {
        0.0
    } else {
        1.0
    };
    height * screen
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The sun's direction seen from under a fluid of index of refraction `ior` (the WGSL
/// `sun_through`, face-local: +y up): its slant from up shrunk by 1 / ior.
pub fn sun_through(sun: [f64; 3], ior: f64) -> [f64; 3] {
    let k = 1.0 / ior.max(1.0);
    let h = [sun[0] * k, sun[2] * k];
    [
        h[0],
        (1.0 - h[0] * h[0] - h[1] * h[1]).max(0.0).sqrt(),
        h[1],
    ]
}

/// What the frame needs to draw the shafts: the camera's axes (world), half-FOV tangents and the
/// screen's aspect, the eye's face, the sun and moon (unit, world) with their colours (sRGB,
/// 0–1), and the fluid the eye is under (None: in the air).
pub struct ShaftView<'a> {
    pub right: [f64; 3],
    pub up: [f64; 3],
    pub forward: [f64; 3],
    pub tan: [f64; 2],
    pub aspect: f64,
    pub face: FaceFrame,
    pub sun: [f64; 3],
    pub moon: [f64; 3],
    pub sun_color: [f64; 3],
    pub moon_color: [f64; 3],
    pub under: Option<&'a FluidShafts>,
    /// The eye (world) and the placed and held lights near it.
    pub eye: [f64; 3],
    pub lights: &'a [ShaftLight],
}

/// The four vec4s of the frame block's shafts (`frame.shafts`, `shafts_cfg`, `shafts_color`,
/// `shafts_window` in the WGSL). shafts: in the air x, y the light's NDC point; under a fluid y
/// the cells' drift (m/s); z the weight (strength × fades; 0: none drawn); w the mode (0 none, 1
/// air, 2 under a fluid). cfg: x the samples; in the air y the reach (share of the way), z the
/// decay, w the disc's radius (half screen heights); under a fluid y the reach (m), z the fade
/// depth (m), w the cells' size (m). color: rgb the light's colour (sRGB, added over the finished
/// picture), w the window's softened rim (a share). window: rgb the glow's colour (sRGB), w how
/// much of the window it is. `tier` None (Off): weight 0, mode 0; the window stays (it is a look,
/// not a shaft).
pub fn shaft_uniforms(tier: Option<ShaftTier>, v: &ShaftView) -> [[f32; 4]; 4] {
    let f = |x: f64| x as f32;
    let window = v.under.copied().unwrap_or(DEFAULT_FLUID_SHAFTS);
    let wc = rgb(window.window_color);
    let window_vec = [f(wc[0]), f(wc[1]), f(wc[2]), f(window.window_glow)];
    let off = [
        [0.0; 4],
        [0.0; 4],
        [0.0, 0.0, 0.0, f(window.window_edge)],
        window_vec,
    ];
    let Some(tier) = tier else {
        return off;
    };
    let rise = |d: [f64; 3]| v.face.vector(d)[1];
    if let Some(u) = v.under {
        let weight = u.strength * smooth(-0.1, 0.1, rise(v.sun));
        if weight <= 0.0 {
            return off;
        }
        return [
            [0.0, f(u.speed), f(weight), 2.0],
            [tier.steps as f32, f(u.reach_m), f(u.fade_m), f(u.cell_m)],
            [
                f(v.sun_color[0]),
                f(v.sun_color[1]),
                f(v.sun_color[2]),
                f(window.window_edge),
            ],
            window_vec,
        ];
    }
    // The sun by day; the moon (faintly) when the sun is down.
    let sun_rise = rise(v.sun);
    let (l, dir, color) = if sun_rise > 0.0 {
        (&SUN_SHAFTS, v.sun, v.sun_color)
    } else {
        (&MOON_SHAFTS, v.moon, v.moon_color)
    };
    let ndc = screen_point(dir, v.right, v.up, v.forward, v.tan);
    let weight = l.strength * air_weight(l, rise(dir), ndc, v.aspect);
    let Some(p) = ndc.filter(|_| weight > 0.0) else {
        return off;
    };
    [
        [f(p[0]), f(p[1]), f(weight), 1.0],
        [tier.steps as f32, f(l.reach), f(l.decay), f(l.disc_radius)],
        [f(color[0]), f(color[1]), f(color[2]), f(window.window_edge)],
        window_vec,
    ]
}

fn rgb(c: u32) -> [f64; 3] {
    [16, 8, 0].map(|s| f64::from((c >> s) & 0xff) / 255.0)
}

/// The order of the 3 × 3 tile of sample offsets ([`shaft_jitter`]): each of the nine strata of a
/// step once, neighbours far apart.
pub const SHAFT_TILE: [u32; 9] = [0, 5, 7, 8, 3, 1, 4, 6, 2];

/// The shafts pass's per-texel offset (0–1) for its samples (`shafts_jitter`'s twin), at the
/// texel `p` of the shafts' buffer: the 3 × 3 tile's stratum (BUGS.md #97: was an interleaved
/// gradient, whose diagonal pattern showed). Any 3 × 3 block of texels holds every stratum once,
/// so the composite's box ([`shaft_blur`]) is the glow of nine times the steps where the scene is
/// smooth: interleaved sampling, no history needed.
pub fn shaft_jitter(p: [i32; 2]) -> f64 {
    let k = (p[0].rem_euclid(3) + 3 * p[1].rem_euclid(3)) as usize;
    (f64::from(SHAFT_TILE[k]) + 0.5) / 9.0
}

/// The composite's filter over the shafts' buffer (`with_shafts`): the box's half width in texels
/// (1: 3 × 3, the jitter's tile). The shafts are a screen-space blur toward the light, the same
/// in front of a silhouette as beside it, so the box needs no depth test.
pub const SHAFT_BLUR: i32 = 1;

/// The composite's shafts at texel `p` of a `w` × `h` buffer `field` (row by row): the box of
/// [`SHAFT_BLUR`] round it, clamped to the buffer (`with_shafts`'s taps, at texel centres).
pub fn shaft_blur(field: &[f64], w: usize, h: usize, p: [i32; 2]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for dy in -SHAFT_BLUR..=SHAFT_BLUR {
        for dx in -SHAFT_BLUR..=SHAFT_BLUR {
            let x = (p[0] + dx).clamp(0, w as i32 - 1) as usize;
            let y = (p[1] + dy).clamp(0, h as i32 - 1) as usize;
            sum += field[y * w + x];
            n += 1.0;
        }
    }
    sum / n
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: [f64; 3] = [1.0, 0.0, 0.0];
    const UP: [f64; 3] = [0.0, 1.0, 0.0];
    const FORWARD: [f64; 3] = [0.0, 0.0, -1.0];

    fn unit(v: [f64; 3]) -> [f64; 3] {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / l)
    }

    /// A view on face `f` looking face-locally along `forward_local` (the sun face-local too).
    fn view(
        f: FaceFrame,
        forward_local: [f64; 3],
        up_local: [f64; 3],
        sun_local: [f64; 3],
    ) -> ShaftView<'static> {
        let fw = unit(forward_local);
        let up = unit(up_local);
        // right = forward × up (face-local, right-handed), then mirrored by the face's frame.
        let right = [
            fw[1] * up[2] - fw[2] * up[1],
            fw[2] * up[0] - fw[0] * up[2],
            fw[0] * up[1] - fw[1] * up[0],
        ];
        ShaftView {
            right: f.vector(right),
            up: f.vector(up),
            forward: f.vector(fw),
            tan: [1.0, 0.75],
            aspect: 4.0 / 3.0,
            face: f,
            sun: f.vector(unit(sun_local)),
            moon: f.vector(unit([-sun_local[0], -sun_local[1], -sun_local[2]])),
            sun_color: [1.0, 0.9, 0.7],
            moon_color: [0.6, 0.7, 1.0],
            under: None,
            eye: [0.0; 3],
            lights: &[],
        }
    }

    const TORCH: [f64; 3] = [1.0, 0.6, 0.3];

    fn torch_at(f: FaceFrame, local: [f64; 3]) -> ShaftLight {
        ShaftLight {
            pos: f.point(local),
            color: TORCH,
            light: "dwell:torch",
        }
    }

    /// Face-local positions ahead of the eye (looking face-locally along −z), placed through `f`;
    /// the eye at the face's origin point.
    fn lights_view(f: FaceFrame, lights: &[ShaftLight], sun_local: [f64; 3]) -> ShaftView<'_> {
        let mut v = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], sun_local);
        v.eye = f.point([0.0; 3]);
        v.lights = lights;
        v
    }

    dwell_world::both_faces!(
        /// The nearest lights in range and on screen are picked, the tier's count at most;
        /// those behind, out of range or far off the screen are not.
        fn the_nearest_lights_in_range_on_screen_are_picked(f: FaceFrame) {
            let lights = [
                torch_at(f, [0.0, 1.0, -12.0]),
                torch_at(f, [0.5, 0.0, -4.0]),
                torch_at(f, [0.0, 0.0, 6.0]),   // behind
                torch_at(f, [0.0, 0.0, -30.0]), // out of range
                torch_at(f, [40.0, 0.0, -5.0]), // far off the screen's side
                torch_at(f, [-1.0, -0.5, -8.0]),
                torch_at(f, [0.0, 0.0, -0.3]), // in your hand: faded out
            ];
            let night = [0.0, -0.5, -1.0];
            let v = lights_view(f, &lights, night);
            let high = pick_shaft_lights(HIGH, &v);
            let dists: Vec<f64> = high
                .iter()
                .map(|p| (p.dist * 10.0).round() / 10.0)
                .collect();
            assert_eq!(dists, vec![4.0, 8.1, 12.0], "nearest first: {dists:?}");
            // Face-local up is the screen's up on both faces: the torch 1 m up is above centre.
            assert!(high[2].ndc[1] > 0.0 && high[2].ndc[0].abs() < 1e-9);
            assert!((high[0].depth - 4.0).abs() < 1e-9);
            assert_eq!(pick_shaft_lights(LOW, &v).len(), LOW.lights as usize);
            // Parity: face B's picks are face A's.
            let la: Vec<ShaftLight> = lights
                .iter()
                .map(|l| ShaftLight {
                    pos: f.point(l.pos),
                    ..*l
                })
                .collect();
            let a = pick_shaft_lights(HIGH, &lights_view(FaceFrame::A, &la, night));
            assert_eq!(high, a);
        }
    );

    dwell_world::both_faces!(
        /// Each light kind's strength is its data's: a fuse glows less than a torch, a light not
        /// listed not at all; by day a share of the night's.
        fn the_strength_is_the_kinds_by_name_and_fades_by_day(f: FaceFrame) {
            let at = |light| ShaftLight {
                light,
                ..torch_at(f, [0.0, 0.0, -5.0])
            };
            let night = [0.0, -0.5, -1.0];
            let glow = |light, sun| {
                let ls = [at(light)];
                pick_shaft_lights(HIGH, &lights_view(f, &ls, sun))
                    .first()
                    .map_or(0.0, |p| p.glow[0])
            };
            let torch = glow("dwell:torch", night);
            let fuse = glow("dwell:fuse", night);
            assert!(
                torch > 0.0 && fuse > 0.0 && fuse < torch,
                "{fuse} < {torch}"
            );
            assert_eq!(glow("test:unlisted", night), 0.0);
            let day = glow("dwell:torch", [0.0, 0.8, -1.0]);
            assert!(
                (day / torch - LIGHT_SHAFTS.day_share).abs() < 1e-9,
                "{day} / {torch}"
            );
            // The weight's ratio between kinds is their data's.
            let k = light_shafts("dwell:fuse").strength / light_shafts("dwell:torch").strength;
            assert!((fuse / torch - k).abs() < 1e-9);
        }
    );

    #[test]
    fn off_draws_no_light_shafts_and_the_header_counts_them() {
        let lights = [torch_at(FaceFrame::A, [0.0, 0.0, -5.0])];
        let v = lights_view(FaceFrame::A, &lights, [0.0, -0.5, -1.0]);
        assert_eq!(
            light_shaft_uniforms(None, &v),
            [[0.0; 4]; LIGHT_SHAFT_VEC4S]
        );
        let u = light_shaft_uniforms(Some(LOW), &v);
        assert_eq!(u[0][0], 1.0);
        assert_eq!(u[0][1], LOW.light_steps as f32);
        assert!((u[1][2] - 5.0).abs() < 1e-6, "its depth");
        assert!(u[2][0] > u[2][2], "a torch's warm glow");
    }

    #[test]
    fn low_weighs_the_way_as_high_and_a_close_halo_is_capped() {
        // The weight left at the light's end is the same on both tiers (Low read brighter when
        // the decay was per sample).
        let end = |t: ShaftTier| sample_decay(t).powi(t.light_steps as i32);
        assert!(
            (end(LOW) - end(HIGH)).abs() < 1e-9,
            "{} vs {}",
            end(LOW),
            end(HIGH)
        );
        let lights = [torch_at(FaceFrame::A, [0.0, 0.0, -1.6])];
        let v = lights_view(FaceFrame::A, &lights, [0.0, -0.5, -1.0]);
        let p = pick_shaft_lights(HIGH, &v);
        assert_eq!(p[0].disc, LIGHT_SHAFTS.max_disc);
    }

    #[test]
    fn a_light_kind_not_listed_takes_the_default_with_no_shafts() {
        assert_eq!(light_shafts("test:none").strength, 0.0);
        assert!(light_shafts("dwell:torch").strength > 0.0);
    }

    #[test]
    fn a_direction_ahead_lands_at_the_screen_centre_and_behind_is_none() {
        assert_eq!(
            screen_point(FORWARD, RIGHT, UP, FORWARD, [1.0, 1.0]),
            Some([0.0, 0.0])
        );
        let p = screen_point(unit([1.0, 0.5, -1.0]), RIGHT, UP, FORWARD, [1.0, 0.5]).unwrap();
        assert!(
            (p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9,
            "{p:?}"
        );
        assert_eq!(
            screen_point([0.0, 0.0, 1.0], RIGHT, UP, FORWARD, [1.0, 1.0]),
            None
        );
    }

    dwell_world::both_faces!(
        fn the_sun_ahead_and_up_draws_air_shafts_at_its_screen_point(f: FaceFrame) {
            let v = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, -1.0]);
            let u = shaft_uniforms(Some(HIGH), &v);
            assert_eq!(u[0][3], 1.0, "air mode");
            assert!(u[0][2] > 0.0, "drawn");
            assert!(u[0][0].abs() < 1e-6, "centred across");
            // Face-local up is the screen's up on both faces.
            assert!(u[0][1] > 0.0, "above the centre: {}", u[0][1]);
            assert_eq!(u[1][0], HIGH.steps as f32);
            // Parity: face B's uniforms are face A's.
            let a = shaft_uniforms(
                Some(HIGH),
                &view(
                    FaceFrame::A,
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.3, -1.0],
                ),
            );
            assert_eq!(u, a);
        }
    );

    dwell_world::both_faces!(
        fn the_shafts_fade_with_the_sun_behind_off_screen_or_down(f: FaceFrame) {
            // Behind the camera: none.
            let behind = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, 1.0]);
            assert_eq!(shaft_uniforms(Some(HIGH), &behind)[0][2], 0.0);
            // Far past the screen's side: none; just past it: some, less than on screen.
            let far = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [3.0, 0.3, -1.0]);
            assert_eq!(shaft_uniforms(Some(HIGH), &far)[0][2], 0.0);
            let on = shaft_uniforms(
                Some(HIGH),
                &view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, -1.0]),
            )[0][2];
            let near = shaft_uniforms(
                Some(HIGH),
                &view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.1, 0.3, -1.0]),
            )[0][2];
            assert!(near > 0.0 && near < on, "{near} vs {on}");
            // At the horizon: the sun's weight is none (the moon is under it too).
            let low = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]);
            assert_eq!(shaft_uniforms(Some(HIGH), &low)[0][2], 0.0);
        }
    );

    dwell_world::both_faces!(
        fn at_night_the_moon_draws_them_faintly(f: FaceFrame) {
            // The sun under the horizon ahead and down; the moon (opposite) behind and up: look at it.
            let v = view(f, [0.0, 0.3, 1.0], [0.0, 1.0, 0.0], [0.0, -0.3, -1.0]);
            let u = shaft_uniforms(Some(HIGH), &v);
            assert!(
                u[0][2] > 0.0 && u[0][2] as f64 <= MOON_SHAFTS.strength + 1e-6,
                "{}",
                u[0][2]
            );
            assert_eq!(u[2][0], 0.6_f32, "the moon's colour");
        }
    );

    dwell_world::both_faces!(
        fn under_water_the_shafts_follow_the_sun_by_day_only(f: FaceFrame) {
            let water = fluid_shafts("dwell:water");
            let mut v = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.3, 0.8, -0.2]);
            v.under = Some(water);
            let u = shaft_uniforms(Some(LOW), &v);
            assert_eq!(u[0][3], 2.0, "under mode");
            assert!((u[0][2] as f64 - water.strength).abs() < 1e-6);
            assert_eq!(
                u[1],
                [
                    LOW.steps as f32,
                    water.reach_m as f32,
                    water.fade_m as f32,
                    water.cell_m as f32
                ]
            );
            let mut night = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.3, -0.8, -0.2]);
            night.under = Some(water);
            assert_eq!(shaft_uniforms(Some(LOW), &night)[0][2], 0.0);
        }
    );

    #[test]
    fn off_draws_none_but_keeps_the_window() {
        let mut v = view(
            FaceFrame::A,
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.3, -1.0],
        );
        assert_eq!(shaft_uniforms(None, &v)[0], [0.0; 4]);
        let water = fluid_shafts("dwell:water");
        v.under = Some(water);
        let u = shaft_uniforms(None, &v);
        assert_eq!(u[0], [0.0; 4]);
        assert_eq!(u[3][3], water.window_glow as f32);
    }

    #[test]
    fn the_tiers_are_named_and_low_is_cheaper() {
        assert_eq!(ShaftQuality::from_name("high"), Some(ShaftQuality::High));
        assert_eq!(ShaftQuality::from_name("low"), Some(ShaftQuality::Low));
        assert_eq!(ShaftQuality::from_name("off"), Some(ShaftQuality::Off));
        assert_eq!(ShaftQuality::from_name("ultra"), None);
        assert_eq!(ShaftQuality::Off.tier(), None);
        assert_eq!(buffer_size((1920, 1080), HIGH), (960, 540));
        assert_eq!(buffer_size((1921, 1081), LOW), (481, 271));
        assert_eq!(buffer_size((1, 1), LOW), (1, 1));
        const { assert!(LOW.steps < HIGH.steps) };
    }

    #[test]
    fn a_fluid_not_listed_takes_the_default_with_no_shafts() {
        assert_eq!(fluid_shafts("test:none").strength, 0.0);
        assert!(fluid_shafts("dwell:water").strength > 0.0);
    }

    #[test]
    fn the_sun_through_the_surface_is_steeper_and_straight_up_stays_up() {
        let s = sun_through(unit([1.0, 0.2, 0.0]), 1.333);
        assert!(s[1] > 0.2 && (s[0] * s[0] + s[1] * s[1] + s[2] * s[2] - 1.0).abs() < 1e-9);
        assert_eq!(sun_through([0.0, 1.0, 0.0], 1.333), [0.0, 1.0, 0.0]);
    }

    /// `shafts_air`'s radial blur at texel `p` (shafts-buffer texels) toward the light at `light`,
    /// with the sample offset `j`, over a scene whose open sky is `open` (1 open, 0 geometry).
    fn air_glow(
        p: [f64; 2],
        light: [f64; 2],
        j: f64,
        steps: u32,
        open: &dyn Fn([f64; 2]) -> f64,
    ) -> f64 {
        let l = &SUN_SHAFTS;
        let radius = 40.0 * l.disc_radius;
        let delta = [
            (light[0] - p[0]) * l.reach / f64::from(steps),
            (light[1] - p[1]) * l.reach / f64::from(steps),
        ];
        let (mut sum, mut total, mut w) = (0.0, 0.0, 1.0);
        for k in 0..steps {
            let t = f64::from(k) + j;
            let q = [p[0] + delta[0] * t, p[1] + delta[1] * t];
            let r = ((q[0] - light[0]).hypot(q[1] - light[1]) / radius).min(1.0);
            sum += open(q) * (1.0 - r) * (1.0 - r) * w;
            total += w;
            w *= l.decay;
        }
        sum / total
    }

    #[test]
    fn the_composited_shafts_behind_a_tree_show_no_dither_pattern() {
        // BUGS.md #97: a low sun behind a tree (a trunk and a round crown on an 80 × 80 texel
        // buffer, the sun just above the crown). The pass's per-texel jitter offsets High's 24
        // samples; composited as it was (no filter over an interleaved-gradient jitter) the
        // jitter's pattern showed as a fine diagonal crosshatch. Against the converged glow (64
        // offsets a texel, through the same filter), the composite's error is under a quarter of the
        // raw pass's.
        let (w, h) = (80usize, 80usize);
        let light = [40.0, 22.0];
        let open = |q: [f64; 2]| {
            let trunk = (q[0] - 40.0).abs() < 3.0 && q[1] > 30.0;
            let crown = (q[0] - 40.0).hypot(q[1] - 34.0) < 12.0;
            let gaps = ((q[0] * 0.9).sin() * (q[1] * 1.1).sin()) > 0.55;
            if trunk || (crown && !gaps) { 0.0 } else { 1.0 }
        };
        let steps = HIGH.steps;
        let raw: Vec<f64> = (0..w * h)
            .map(|i| {
                let p = [(i % w) as i32, (i / w) as i32];
                let c = [f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.5];
                air_glow(c, light, shaft_jitter(p), steps, &open)
            })
            .collect();
        let truth: Vec<f64> = (0..w * h)
            .map(|i| {
                let c = [(i % w) as f64 + 0.5, (i / w) as f64 + 0.5];
                (0..64)
                    .map(|k| air_glow(c, light, (f64::from(k) + 0.5) / 64.0, steps, &open))
                    .sum::<f64>()
                    / 64.0
            })
            .collect();
        // The dither seen: the composite of the pass less the same composite of the converged glow
        // (the filter's own softening is the look, not the dither).
        let (mut err_raw, mut err_shown) = (0.0, 0.0);
        for i in 0..w * h {
            let p = [(i % w) as i32, (i / w) as i32];
            err_raw += (raw[i] - truth[i]).powi(2);
            err_shown += (shaft_blur(&raw, w, h, p) - shaft_blur(&truth, w, h, p)).powi(2);
        }
        let (err_raw, err_shown) = (
            (err_raw / (w * h) as f64).sqrt(),
            (err_shown / (w * h) as f64).sqrt(),
        );
        assert!(
            err_raw > 0.005,
            "the fixture shows no dither to remove: {err_raw}"
        );
        assert!(
            err_shown < 0.25 * err_raw,
            "the composited shafts keep the jitter's pattern: rms error {err_shown} against the raw pass's {err_raw}"
        );
    }
}
