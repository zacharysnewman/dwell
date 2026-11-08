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
pub use generated::{DEFAULT_FLUID_SHAFTS, FLUID_SHAFTS, HIGH, LOW, MOON_SHAFTS, SUN_SHAFTS};

/// A tier of the Light shafts setting: the shafts' buffer against the drawing buffer (each axis)
/// and the samples a pixel takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaftTier {
    pub scale: f64,
    pub steps: u32,
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
    [h[0], (1.0 - h[0] * h[0] - h[1] * h[1]).max(0.0).sqrt(), h[1]]
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
        [
            tier.steps as f32,
            f(l.reach),
            f(l.decay),
            f(l.disc_radius),
        ],
        [f(color[0]), f(color[1]), f(color[2]), f(window.window_edge)],
        window_vec,
    ]
}

fn rgb(c: u32) -> [f64; 3] {
    [16, 8, 0].map(|s| f64::from((c >> s) & 0xff) / 255.0)
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
    fn view(f: FaceFrame, forward_local: [f64; 3], up_local: [f64; 3], sun_local: [f64; 3]) -> ShaftView<'static> {
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
        }
    }

    #[test]
    fn a_direction_ahead_lands_at_the_screen_centre_and_behind_is_none() {
        assert_eq!(screen_point(FORWARD, RIGHT, UP, FORWARD, [1.0, 1.0]), Some([0.0, 0.0]));
        let p = screen_point(unit([1.0, 0.5, -1.0]), RIGHT, UP, FORWARD, [1.0, 0.5]).unwrap();
        assert!((p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9, "{p:?}");
        assert_eq!(screen_point([0.0, 0.0, 1.0], RIGHT, UP, FORWARD, [1.0, 1.0]), None);
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
                &view(FaceFrame::A, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, -1.0]),
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
            let on = shaft_uniforms(Some(HIGH), &view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, -1.0]))[0][2];
            let near = shaft_uniforms(Some(HIGH), &view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.1, 0.3, -1.0]))[0][2];
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
            assert!(u[0][2] > 0.0 && u[0][2] as f64 <= MOON_SHAFTS.strength + 1e-6, "{}", u[0][2]);
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
            assert_eq!(u[1], [LOW.steps as f32, water.reach_m as f32, water.fade_m as f32, water.cell_m as f32]);
            let mut night = view(f, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.3, -0.8, -0.2]);
            night.under = Some(water);
            assert_eq!(shaft_uniforms(Some(LOW), &night)[0][2], 0.0);
        }
    );

    #[test]
    fn off_draws_none_but_keeps_the_window() {
        let mut v = view(FaceFrame::A, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.3, -1.0]);
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
        assert!(LOW.steps < HIGH.steps);
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
}
