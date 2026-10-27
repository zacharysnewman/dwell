//! The day/night cycle (docs/DAY_NIGHT.md, ADR 0034): the world clock, the sun's path, and the sky
//! keyframes over the day, from `shared/look/daynight.json` (generated into `daynight.gen.rs`).
//!
//! - **Time.** World time is the server's tick (saved in the world file's `meta` as `world_tick`
//!   and sent in every snapshot), so every client derives the same time of day from it.
//! - **The sun's path.** A great circle through the rising and setting points, culminating at the
//!   data's noon elevation over face A. Its *solar angle* is 0° rising, 90° at noon, 180° setting
//!   and 270° at its lowest; it moves unevenly so daylight lasts `day_s` of the day. The moon is
//!   always opposite the sun (BIFACIAL_WORLD.md §6). Face B shares the sky: its solar angle is face
//!   A's plus 180° (when the sun sets on face A it rises on face B).
//! - **Keyframes.** Each sets the sky, the lights, the fog and the exposure at one solar angle; any
//!   angle is a monotone cubic (no overshoot, no corners) through the keyframes around it, colours
//!   in Oklab. A face's look is the keyframes at its own solar angle.

use crate::{Rgb, srgb_to_linear};
use dwell_world::FaceFrame;

#[rustfmt::skip]
#[allow(clippy::all)]
mod data {
    include!("daynight.gen.rs");
}
pub use data::DAY_NIGHT;

/// How long a day is, and the clock at world tick 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Seconds of daylight and of night on face A (face B has them the other way round).
    pub day_s: f64,
    pub night_s: f64,
    /// The 24-hour clock at world time 0.
    pub start_hour: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPath {
    /// Where the sun rises over face A (yaw degrees: 0 = +Z, 90 = +X).
    pub rise_yaw_deg: f64,
    /// Its height at noon over face A's horizon.
    pub noon_elevation_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLook {
    pub radius_deg: f64,
    pub horizon_scale: f64,
    pub limb_darkening: f64,
    pub bloom_deg: f64,
    pub bloom: f64,
    pub glow_deg: f64,
    pub glow: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonLook {
    pub radius_deg: f64,
    pub cycle_days: f64,
    pub start_phase: f64,
    pub color: Rgb,
    pub maria: f64,
    pub earthshine: f64,
    pub bloom_deg: f64,
    pub bloom: f64,
    pub glow_deg: f64,
    pub glow: f64,
    pub glow_color: Rgb,
    pub new_moon_light: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarLook {
    pub cells: f64,
    pub density: f64,
    pub size_px: f64,
    pub brightness: f64,
    pub twinkle: f64,
    pub twinkle_hz: f64,
    pub horizon_fade: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyShape {
    pub gradient_power: f64,
    pub band_height: f64,
    pub band_spread: f64,
    /// Below the horizon, the other face's sky fades in from this elevation sine…
    pub behind_from: f64,
    /// …to this one (fully the other face's from here down), for an eye this high (m) or more
    /// above the midplane; nearer it the viewer's horizon haze fades out (docs/DAY_NIGHT.md §13).
    pub behind_to: f64,
    pub behind_height_m: f64,
}

/// The eye's adaptation to the face it is on (docs/DAY_NIGHT.md §13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeLook {
    /// The time constant (s) the exposure and saturation ease toward the eye's face with.
    pub adapt_s: f64,
}

/// The look at one solar angle (colours sRGB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub name: &'static str,
    /// The solar angle (degrees): 0 rising, 90 noon, 180 setting, 270 lowest.
    pub sun: f64,
    pub sky_zenith: Rgb,
    pub sky_horizon: Rgb,
    pub sun_disc: Rgb,
    pub glow: Rgb,
    pub glow_strength: f64,
    pub band: Rgb,
    pub band_strength: f64,
    pub sun_light: Rgb,
    pub sun_intensity: f64,
    pub moon_light: Rgb,
    pub moon_intensity: f64,
    pub hemi_sky: Rgb,
    pub hemi_ground: Rgb,
    pub hemi_intensity: f64,
    pub fog_density: f64,
    pub exposure: f64,
    pub stars: f64,
    pub moon_glow: f64,
    /// The lit scene's colour saturation (1 as is; lower by night, as eyes see moonlight).
    pub saturation: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayNight {
    pub clock: Clock,
    pub sun_path: SunPath,
    pub sun: SunLook,
    pub moon: MoonLook,
    pub stars: StarLook,
    pub sky: SkyShape,
    pub eye: EyeLook,
    pub keyframes: &'static [Keyframe],
}

/// A keyframe's values interpolated: what a face looks like at its solar angle.
pub type Sample = Keyframe;

const COLORS: usize = 9;
const SCALARS: usize = 10;
const CHANNELS: usize = COLORS * 3 + SCALARS;

fn colors(k: &Keyframe) -> [Rgb; COLORS] {
    [
        k.sky_zenith,
        k.sky_horizon,
        k.sun_disc,
        k.glow,
        k.band,
        k.sun_light,
        k.moon_light,
        k.hemi_sky,
        k.hemi_ground,
    ]
}
fn scalars(k: &Keyframe) -> [f64; SCALARS] {
    [
        k.glow_strength,
        k.band_strength,
        k.sun_intensity,
        k.moon_intensity,
        k.hemi_intensity,
        k.fog_density,
        k.exposure,
        k.stars,
        k.moon_glow,
        k.saturation,
    ]
}

/// A keyframe as interpolation channels: its colours in Oklab, then its scalars.
fn channels(k: &Keyframe) -> [f64; CHANNELS] {
    let mut c = [0.0; CHANNELS];
    for (i, rgb) in colors(k).iter().enumerate() {
        c[i * 3..i * 3 + 3].copy_from_slice(&srgb_to_oklab(*rgb));
    }
    c[COLORS * 3..].copy_from_slice(&scalars(k));
    c
}

fn from_channels(c: &[f64; CHANNELS], name: &'static str, sun: f64) -> Sample {
    let col = |i: usize| oklab_to_srgb([c[i * 3], c[i * 3 + 1], c[i * 3 + 2]]);
    let s = |i: usize| c[COLORS * 3 + i].max(0.0);
    Keyframe {
        name,
        sun,
        sky_zenith: col(0),
        sky_horizon: col(1),
        sun_disc: col(2),
        glow: col(3),
        band: col(4),
        sun_light: col(5),
        moon_light: col(6),
        hemi_sky: col(7),
        hemi_ground: col(8),
        glow_strength: s(0),
        band_strength: s(1),
        sun_intensity: s(2),
        moon_intensity: s(3),
        hemi_intensity: s(4),
        fog_density: s(5),
        exposure: s(6),
        stars: s(7).min(1.0),
        moon_glow: s(8),
        saturation: s(9),
    }
}

/// Two samples mixed in Oklab (`t` = 0 gives `a`).
pub fn mix_samples(a: &Sample, b: &Sample, t: f64) -> Sample {
    let (ca, cb) = (channels(a), channels(b));
    let c: [f64; CHANNELS] = std::array::from_fn(|i| ca[i] + (cb[i] - ca[i]) * t);
    from_channels(&c, if t < 0.5 { a.name } else { b.name }, a.sun)
}

// Oklab (Björn Ottosson, 2020) from and to sRGB (0–1).
pub fn srgb_to_oklab(c: Rgb) -> Rgb {
    let [r, g, b] = c.map(srgb_to_linear);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

pub fn oklab_to_srgb(c: Rgb) -> Rgb {
    let [ll, a, b] = c;
    let l = ll + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m = ll - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s = ll - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    let lin = [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ];
    lin.map(|v| linear_to_srgb(v.clamp(0.0, 1.0)))
}

pub fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A monotone cubic's tangent at a point between slopes `d0` (over `h0`) and `d1` (over `h1`)
/// (Fritsch–Butland, as PCHIP): flat where the data turns, so the curve never overshoots.
fn pchip_tangent(h0: f64, d0: f64, h1: f64, d1: f64) -> f64 {
    if d0 * d1 <= 0.0 {
        return 0.0;
    }
    let (w0, w1) = (2.0 * h1 + h0, h1 + 2.0 * h0);
    (w0 + w1) / (w0 / d0 + w1 / d1)
}

fn wrap360(deg: f64) -> f64 {
    deg.rem_euclid(360.0)
}

impl DayNight {
    /// The keyframe with the strongest sun (noon): the sun's glow colour for the clouds' silver
    /// lining.
    pub fn brightest(&self) -> &Keyframe {
        self.keyframes
            .iter()
            .max_by(|a, b| a.sun_intensity.total_cmp(&b.sun_intensity))
            .expect("at least one keyframe")
    }

    /// The length of a whole day and night (s).
    pub fn day_length_s(&self) -> f64 {
        self.clock.day_s + self.clock.night_s
    }

    /// Days since world time 0 (continuous), counted from midnight, with the start hour added.
    pub fn days(&self, world_s: f64) -> f64 {
        world_s / self.day_length_s() + self.clock.start_hour / 24.0
    }

    /// The time of day (0 midnight, 0.5 noon on face A) at `world_s`.
    pub fn time_of_day(&self, world_s: f64) -> f64 {
        self.days(world_s).rem_euclid(1.0)
    }

    /// The in-game day number at `world_s` (day 0 is the world's first).
    pub fn day_number(&self, world_s: f64) -> i64 {
        self.days(world_s).floor() as i64
    }

    /// Face A's solar angle (degrees, 0–360) at time of day `t`: the hour angle 2πu + k·sin 2πu
    /// (u = t − ½), so the sun moves slower by day and faster by night and is up for `day_s`.
    pub fn solar_deg(&self, t: f64) -> f64 {
        let f = self.clock.day_s / self.day_length_s();
        let k = (std::f64::consts::FRAC_PI_2 - std::f64::consts::PI * f)
            / (std::f64::consts::PI * f).sin();
        let u = std::f64::consts::TAU * (t - 0.5);
        wrap360((u + k * u.sin()).to_degrees() + 90.0)
    }

    /// The horizontal unit vectors the path is built on: toward sunrise, and toward the side the sun
    /// culminates on (a quarter turn right of sunrise).
    fn path_basis(&self) -> (Rgb, Rgb) {
        let yaw = self.sun_path.rise_yaw_deg.to_radians();
        let rise = [yaw.sin(), 0.0, yaw.cos()];
        let side = [
            (yaw + std::f64::consts::FRAC_PI_2).sin(),
            0.0,
            (yaw + std::f64::consts::FRAC_PI_2).cos(),
        ];
        (rise, side)
    }

    /// The direction of noon (unit, world).
    fn noon(&self) -> Rgb {
        let (_, side) = self.path_basis();
        let el = self.sun_path.noon_elevation_deg.to_radians();
        [side[0] * el.cos(), el.sin(), side[2] * el.cos()]
    }

    /// Toward the sun (unit, world) at face A's solar angle `solar_deg`.
    pub fn sun_direction(&self, solar_deg: f64) -> Rgb {
        let (rise, _) = self.path_basis();
        let noon = self.noon();
        let a = solar_deg.to_radians();
        std::array::from_fn(|i| rise[i] * a.cos() + noon[i] * a.sin())
    }

    /// The axis the sky turns about (unit): the sun's path's normal, so turning sunrise by the
    /// solar angle about it gives the sun.
    pub fn sky_axis(&self) -> Rgb {
        let (r, n) = (self.path_basis().0, self.noon());
        [
            r[1] * n[2] - r[2] * n[1],
            r[2] * n[0] - r[0] * n[2],
            r[0] * n[1] - r[1] * n[0],
        ]
    }

    /// The moon's phase (0 new, 0.5 full) after `days` days: one cycle per `cycle_days`.
    pub fn moon_phase(&self, days: f64) -> f64 {
        (self.moon.start_phase + days / self.moon.cycle_days).rem_euclid(1.0)
    }

    /// The share of the moon's disc lit at `phase`.
    pub fn moon_lit(phase: f64) -> f64 {
        0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()
    }

    /// The keyframes around `solar_deg` and how far between the two it lies:
    /// (index before, index after, 0–1).
    pub fn segment(&self, solar_deg: f64) -> (usize, usize, f64) {
        let k = self.keyframes;
        let n = k.len();
        let x = wrap360(solar_deg);
        // The last keyframe at or before x (wrapping: before the first is the last).
        let i = k.iter().rposition(|f| f.sun <= x).unwrap_or(n - 1);
        let j = (i + 1) % n;
        let start = k[i].sun;
        let span = wrap360(k[j].sun - start).max(1e-9);
        let span = if n == 1 { 360.0 } else { span };
        (i, j, (wrap360(x - start) / span).clamp(0.0, 1.0))
    }

    /// A face's look at its solar angle: the keyframes interpolated by a monotone cubic over the
    /// angle (cyclic), colours in Oklab.
    pub fn sample(&self, solar_deg: f64) -> Sample {
        let (i, j, t) = self.segment(solar_deg);
        let name = if t < 0.5 {
            self.keyframes[i].name
        } else {
            self.keyframes[j].name
        };
        from_channels(&self.sample_channels(solar_deg), name, wrap360(solar_deg))
    }

    /// [`Self::sample`]'s channels (colours in Oklab, then the scalars) before they are turned
    /// back into colours.
    fn sample_channels(&self, solar_deg: f64) -> [f64; CHANNELS] {
        let k = self.keyframes;
        let n = k.len();
        if n == 1 {
            return channels(&k[0]);
        }
        let (i, j, t) = self.segment(solar_deg);
        let (prev, next) = ((i + n - 1) % n, (j + 1) % n);
        let gap = |a: usize, b: usize| wrap360(k[b].sun - k[a].sun).max(1e-9);
        let (h0, h1, h2) = (gap(prev, i), gap(i, j), gap(j, next));
        let (c0, c1, c2, c3) = (
            channels(&k[prev]),
            channels(&k[i]),
            channels(&k[j]),
            channels(&k[next]),
        );
        let (t2, t3) = (t * t, t * t * t);
        let (h00, h10, h01, h11) = (
            2.0 * t3 - 3.0 * t2 + 1.0,
            t3 - 2.0 * t2 + t,
            -2.0 * t3 + 3.0 * t2,
            t3 - t2,
        );
        std::array::from_fn(|ch| {
            let (y0, y1, y2, y3) = (c0[ch], c1[ch], c2[ch], c3[ch]);
            let (d0, d1, d2) = ((y1 - y0) / h0, (y2 - y1) / h1, (y3 - y2) / h2);
            let m1 = pchip_tangent(h0, d0, h1, d1);
            let m2 = pchip_tangent(h1, d1, h2, d2);
            h00 * y1 + h10 * h1 * m1 + h01 * y2 + h11 * h1 * m2
        })
    }

    /// Everything the renderer and the HUD need at world time `world_s`: `time` replaces the time
    /// of day and `moon` the moon's phase (dev switches, `?time=`, `?moon=`).
    pub fn state(&self, world_s: f64, time: Option<f64>, moon: Option<f64>) -> DayState {
        let day = self.day_number(world_s);
        let time_of_day = time.map_or_else(|| self.time_of_day(world_s), |t| t.rem_euclid(1.0));
        let days = day as f64 + time_of_day;
        let solar_deg = self.solar_deg(time_of_day);
        let sun = self.sun_direction(solar_deg);
        let moon_phase = moon.map_or_else(|| self.moon_phase(days), |m| m.rem_euclid(1.0));
        DayState {
            time_of_day,
            day,
            solar_deg,
            sun,
            moon: sun.map(|c| -c),
            axis: self.sky_axis(),
            moon_phase,
            moon_lit: Self::moon_lit(moon_phase),
            a: self.sample(solar_deg),
            b: self.sample(solar_deg + 180.0),
        }
    }
}

/// The cycle at one moment, for both faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayState {
    /// 0 midnight, 0.5 noon (face A's clock).
    pub time_of_day: f64,
    pub day: i64,
    /// Face A's solar angle (degrees); face B's is 180° on.
    pub solar_deg: f64,
    /// Toward the sun and the moon (unit, world).
    pub sun: Rgb,
    pub moon: Rgb,
    /// The sky's turning axis (the stars turn about it by the solar angle).
    pub axis: Rgb,
    /// 0 new, 0.5 full; and the share of its disc lit.
    pub moon_phase: f64,
    pub moon_lit: f64,
    /// Each face's look.
    pub a: Sample,
    pub b: Sample,
}

impl Default for DayState {
    fn default() -> Self {
        DAY_NIGHT.state(0.0, None, None)
    }
}

impl DayState {
    /// The look for a viewer `weight_a` of the way to face A (1 on face A, 0 on face B; the
    /// renderer eases it across the flip band so the sky doesn't jump when the player crosses).
    pub fn viewer(&self, weight_a: f64) -> Sample {
        if weight_a >= 1.0 {
            self.a
        } else if weight_a <= 0.0 {
            self.b
        } else {
            mix_samples(&self.b, &self.a, weight_a)
        }
    }

    /// The 24-hour clock, "HH:MM".
    pub fn clock_text(&self) -> String {
        let minutes = (self.time_of_day * 24.0 * 60.0).floor() as i64;
        format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
    }

    /// The F3 overlay's line.
    pub fn debug_line(&self) -> String {
        let (i, j, t) = DAY_NIGHT.segment(self.solar_deg);
        let names = DAY_NIGHT.keyframes;
        format!(
            "time {} (t {:.3}) · day {} · sun {:.0}° (face B {:.0}°) · sky {} → {} {:.0}% · moon phase {:.2}, {:.0}% lit",
            self.clock_text(),
            self.time_of_day,
            self.day,
            self.solar_deg,
            wrap360(self.solar_deg + 180.0),
            names[i].name,
            names[j].name,
            t * 100.0,
            self.moon_phase,
            self.moon_lit * 100.0
        )
    }

    /// Face `face`'s look.
    pub fn of(&self, face: FaceFrame) -> &Sample {
        if face.is_b() { &self.b } else { &self.a }
    }

    /// The `DaySky` uniform block (`DAY_SKY_WGSL`) for a viewer on `face`: that face's sky in the
    /// viewer's slots and the other face's in `other_*`, which the shaders choose between per ray by
    /// the air it crosses, not by the viewer (docs/DAY_NIGHT.md §13); the scene's saturation
    /// `adapted_a` of the way to face A's (the eye's adaptation, [`EyeAdaptation`]); `seconds` into
    /// the session (the twinkle), with pixels `pixel_angle` radians across.
    pub fn pack(
        &self,
        face: FaceFrame,
        adapted_a: f64,
        seconds: f64,
        pixel_angle: f64,
    ) -> [f32; DAY_SKY_FLOATS] {
        let v = self.of(face);
        let o = self.of(face.other());
        let saturation = self.viewer(adapted_a).saturation;
        let moon_light =
            DAY_NIGHT.moon.new_moon_light + (1.0 - DAY_NIGHT.moon.new_moon_light) * self.moon_lit;
        let mut u = [0.0f32; DAY_SKY_FLOATS];
        let mut i = 0;
        let mut put = |v: [f64; 4]| {
            for (k, x) in v.iter().enumerate() {
                u[i + k] = *x as f32;
            }
            i += 4;
        };
        let with = |c: Rgb, w: f64| [c[0], c[1], c[2], w];
        let lit = |c: Rgb, k: f64| {
            let l = c.map(srgb_to_linear);
            [l[0] * k, l[1] * k, l[2] * k, 0.0]
        };
        put(with(self.sun, self.solar_deg.to_radians()));
        put(with(self.moon, self.moon_phase));
        put(with(self.axis, self.solar_deg.to_radians()));
        put(with(v.sky_zenith, 0.0));
        put(with(v.sky_horizon, saturation));
        put(with(v.glow, v.glow_strength));
        put(with(v.band, v.band_strength));
        put(with(v.sun_disc, v.moon_glow * self.moon_lit.max(0.25)));
        put([
            v.stars,
            seconds.rem_euclid(3600.0),
            pixel_angle,
            self.moon_lit,
        ]);
        for s in [&self.a, &self.b] {
            put(lit(s.sun_light, s.sun_intensity));
            put(lit(s.moon_light, s.moon_intensity * moon_light));
            put(lit(s.hemi_sky, s.hemi_intensity));
            put(lit(s.hemi_ground, s.hemi_intensity));
        }
        // The other face's sky, for the rays that cross into its air: down a shaft, or past the
        // rim (docs/BUGS.md 10, 13); y its fog density (× the fog data's).
        put(with(o.sky_zenith, 0.0));
        put(with(o.sky_horizon, o.saturation));
        put(with(o.glow, o.glow_strength));
        put(with(o.band, o.band_strength));
        put(with(o.sun_disc, o.moon_glow * self.moon_lit.max(0.25)));
        put([o.stars, fog_density(o), 0.0, 0.0]);
        u
    }
}

/// A face's fog density this moment: the fog data's, times its time of day's (at most 1).
pub fn fog_density(s: &Sample) -> f64 {
    (crate::fog::FOG.density * s.fog_density).min(1.0)
}

/// The eye's adaptation to the face it is on (docs/DAY_NIGHT.md §13): how far toward face A's
/// exposure and saturation it is (1 face A's, 0 face B's), easing toward its face's with the
/// data's time constant (`eye.adaptS`) as it crosses the midplane, instead of switching there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeAdaptation {
    weight_a: Option<f64>,
}

impl EyeAdaptation {
    /// Steps `dt_s` seconds toward `target_a` (the eye's face: 1 face A, 0 face B, between across
    /// the flip band) and returns the weight. The first step takes the target as it is.
    pub fn step(&mut self, target_a: f64, dt_s: f64) -> f64 {
        let target = target_a.clamp(0.0, 1.0);
        let tau = DAY_NIGHT.eye.adapt_s;
        let w = match self.weight_a {
            Some(w) if tau > 0.0 && dt_s.is_finite() => {
                w + (target - w) * -(-dt_s.max(0.0) / tau).exp_m1()
            }
            _ => target,
        };
        self.weight_a = Some(w);
        w
    }
}

/// Floats in the `DaySky` uniform block.
pub const DAY_SKY_FLOATS: usize = 23 * 4;

/// The uniform block the renderer's frame carries (`frame.day`): the cycle's state for this frame.
/// Sky colours are sRGB (the sky is drawn in output colour); lights are linear × intensity. Clouds
/// and water take their light from it.
pub const DAY_SKY_WGSL: &str = r"
struct DaySky {
    // xyz toward the sun (world); w face A's solar angle (rad)
    sun: vec4f,
    // xyz toward the moon; w its phase (0 new, 0.5 full)
    moon: vec4f,
    // xyz the sky's turning axis; w the stars' turn (rad)
    axis: vec4f,
    // The viewer's face's sky (sRGB): the zenith and horizon colours (horizon.a: the scene's saturation, the eye's adaptation's), the sun's glow and the horizon band
    // toward it (rgb, a strength), the sun's disc (rgb; a the moon's glow strength).
    zenith: vec4f,
    horizon: vec4f,
    glow: vec4f,
    band: vec4f,
    disc: vec4f,
    // x stars' visibility, y seconds (the twinkle), z a pixel's angle (rad), w the moon's lit share
    stars: vec4f,
    // Each face's lights (linear × intensity): the sun, the moon, the hemisphere's sky and ground.
    sun_a: vec4f,
    moon_a: vec4f,
    sky_a: vec4f,
    ground_a: vec4f,
    sun_b: vec4f,
    moon_b: vec4f,
    sky_b: vec4f,
    ground_b: vec4f,
    // The other face's sky (as zenith … disc above; other_stars.x its stars' visibility, y its fog
    // density): what a ray that crosses into its air shows, through a shaft or past the rim.
    other_zenith: vec4f,
    other_horizon: vec4f,
    other_glow: vec4f,
    other_band: vec4f,
    other_disc: vec4f,
    other_stars: vec4f,
};
";

#[cfg(test)]
mod tests {
    use super::*;

    const D: DayNight = DAY_NIGHT;

    fn dist(a: Rgb, b: Rgb) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
    }
    fn near(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} vs {b} (±{tol})");
    }
    fn elevation_deg(v: Rgb) -> f64 {
        v[1].asin().to_degrees()
    }
    /// The largest difference between two samples' channels.
    fn gap(a: &Sample, b: &Sample) -> f64 {
        let (ca, cb) = (channels(a), channels(b));
        (0..CHANNELS)
            .map(|i| (ca[i] - cb[i]).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_block_carries_the_other_faces_sky_for_below_the_horizon() {
        // both-faces: packs for each face and compares their slots.
        // docs/BUGS.md 10: from face A at noon, face B's night seen through a portal was drawn in
        // face A's daytime colours (a white patch). The block carries the other face's sky.
        let day = D.state(0.0, Some(0.5), Some(0.5));
        let u = day.pack(FaceFrame::A, 1.0, 0.0, 0.001);
        let other = 17 * 4;
        let zenith = &u[other..other + 3];
        for (k, z) in zenith.iter().enumerate() {
            assert!(
                (f64::from(*z) - day.b.sky_zenith[k]).abs() < 1e-6,
                "{zenith:?}"
            );
        }
        assert!(
            f64::from(u[other + 5 * 4]) > 0.5,
            "face B's stars show: {}",
            u[other + 20]
        );
        assert!(day.a.stars < 0.5, "and face A's don't at noon");
        // From face B the slots swap: its own sky in the viewer's, face A's in the other's.
        let b = day.pack(FaceFrame::B, 0.0, 0.0, 0.001);
        assert_eq!(&b[3 * 4..3 * 4 + 3], &u[other..other + 3]);
        assert_eq!(&b[other..other + 3], &u[3 * 4..3 * 4 + 3]);
    }

    #[test]
    fn the_skies_are_the_faces_own_not_a_blend_by_the_eye() {
        // docs/BUGS.md 13 (owner, 2026-10-27): crossing the midplane swapped the sky's colours as
        // the eye's flip-band weight moved, so the same ray looked different from either side of
        // it. Each face's sky is now its own whatever the eye's adaptation; only the saturation
        // (horizon.a) follows the eye.
        let day = D.state(0.0, Some(0.45), Some(0.5));
        let other = 17 * 4;
        for adapted in [0.0, 0.3, 0.5, 1.0] {
            let u = day.pack(FaceFrame::A, adapted, 0.0, 0.001);
            for k in 0..3 {
                near(f64::from(u[3 * 4 + k]), day.a.sky_zenith[k], 1e-6);
                near(f64::from(u[other + k]), day.b.sky_zenith[k], 1e-6);
            }
            near(
                f64::from(u[4 * 4 + 3]),
                day.viewer(adapted).saturation,
                1e-6,
            );
        }
        // The other face's fog density rides in other_stars.y.
        let u = day.pack(FaceFrame::A, 1.0, 0.0, 0.001);
        near(f64::from(u[other + 5 * 4 + 1]), fog_density(&day.b), 1e-6);
    }

    #[test]
    fn the_eye_adapts_to_its_face_over_the_datas_time() {
        // docs/BUGS.md 13: the exposure jumped at the midplane. It eases over eye.adaptS.
        let tau = D.eye.adapt_s;
        assert!(tau > 0.0);
        let mut eye = EyeAdaptation::default();
        assert_eq!(
            eye.step(1.0, 0.016),
            1.0,
            "the first frame takes its face as it is"
        );
        let mut w = 1.0;
        let mut t = 0.0;
        while t < tau - 1e-9 {
            let next = eye.step(0.0, 0.01);
            assert!(next <= w, "it only eases one way: {next} after {w}");
            w = next;
            t += 0.01;
        }
        near(w, (-1.0f64).exp(), 0.01);
        for _ in 0..1000 {
            w = eye.step(0.0, 0.01);
        }
        assert!(w < 1e-3, "and arrives: {w}");
        // A hidden tab's long frame lands; nothing goes past the target.
        assert_eq!(eye.step(1.0, 1e9), 1.0);
        assert_eq!(eye.step(0.0, f64::NAN), 0.0);
    }

    #[test]
    fn the_clock_runs_from_world_time() {
        // A 30-minute day: 20 minutes of daylight, 10 of night (owner, TREE_GROWTH.md).
        assert_eq!(D.day_length_s(), 1800.0);
        assert_eq!((D.clock.day_s, D.clock.night_s), (1200.0, 600.0));
        near(D.time_of_day(0.0), D.clock.start_hour / 24.0, 1e-12);
        near(D.time_of_day(1800.0), D.time_of_day(0.0), 1e-12);
        near(
            D.time_of_day(900.0),
            (D.clock.start_hour / 24.0 + 0.5).rem_euclid(1.0),
            1e-12,
        );
        assert_eq!(D.day_number(0.0), 0);
        // Day 1 starts at the first midnight.
        let to_midnight = (1.0 - D.clock.start_hour / 24.0) * 1800.0;
        assert_eq!(D.day_number(to_midnight - 0.01), 0);
        assert_eq!(D.day_number(to_midnight + 0.01), 1);
        assert_eq!(D.day_number(to_midnight + 1800.0 * 9.5), 10);
    }

    #[test]
    fn the_sun_is_up_for_the_days_share_of_it() {
        // Sunrise, noon, sunset and midnight where the clock puts them.
        let f = D.clock.day_s / D.day_length_s();
        let wrapped = |t: f64| (D.solar_deg(t) + 180.0).rem_euclid(360.0) - 180.0;
        near(wrapped(0.5 - f / 2.0), 0.0, 1e-9);
        near(D.solar_deg(0.5), 90.0, 1e-9);
        near(D.solar_deg(0.5 + f / 2.0), 180.0, 1e-9);
        near(D.solar_deg(0.0), 270.0, 1e-9);
        // Daylight for 2/3 of the day, moving smoothly and always forward.
        let n = 36_000;
        let mut up = 0;
        let mut last = D.solar_deg(0.0);
        for i in 1..=n {
            let t = f64::from(i) / f64::from(n);
            let a = D.solar_deg(t);
            let step = (a - last).rem_euclid(360.0);
            assert!(step > 0.0 && step < 0.1, "t {t}: {last} → {a}");
            last = a;
            if D.sun_direction(a)[1] > 0.0 {
                up += 1;
            }
        }
        near(f64::from(up) / f64::from(n), f, 1e-3);
    }

    #[test]
    fn the_sun_and_moon_follow_the_path() {
        let k = D.sky_axis();
        for deg in [0.0, 37.0, 90.0, 180.0, 233.0, 270.0] {
            let s = D.sun_direction(deg);
            near((s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt(), 1.0, 1e-12);
            // The axis is the path's normal: the sun is always square to it.
            near(s[0] * k[0] + s[1] * k[1] + s[2] * k[2], 0.0, 1e-12);
        }
        near(
            elevation_deg(D.sun_direction(90.0)),
            D.sun_path.noon_elevation_deg,
            1e-9,
        );
        near(elevation_deg(D.sun_direction(0.0)), 0.0, 1e-9);
        near(
            elevation_deg(D.sun_direction(270.0)),
            -D.sun_path.noon_elevation_deg,
            1e-9,
        );
        let rise = D.sun_direction(0.0);
        near(
            rise[0].atan2(rise[2]).to_degrees(),
            D.sun_path.rise_yaw_deg,
            1e-9,
        );
        // The sun is up in the morning; the moon is opposite.
        let morning = D.state(0.0, Some(0.3), None);
        assert!(morning.sun[1] > 0.3);
        for i in 0..3 {
            near(morning.moon[i], -morning.sun[i], 1e-12);
        }
        // Face B's solar angle is face A's + 180°: its sky turns over when face A's does.
        assert_eq!(morning.b, D.sample(morning.solar_deg + 180.0));
    }

    #[test]
    fn the_moon_goes_through_its_phases() {
        assert_eq!(D.moon.cycle_days, 8.0);
        near(D.moon_phase(0.0), D.moon.start_phase, 1e-12);
        near(D.moon_phase(8.0), D.moon_phase(0.0), 1e-12);
        near(
            D.moon_phase(4.0),
            (D.moon.start_phase + 0.5).rem_euclid(1.0),
            1e-12,
        );
        near(DayNight::moon_lit(0.5), 1.0, 1e-12);
        near(DayNight::moon_lit(0.0), 0.0, 1e-12);
        near(DayNight::moon_lit(0.25), 0.5, 1e-12);
        // Driven by the day count: a day moves the phase on by 1/8.
        let a = D.state(0.0, Some(0.0), None).moon_phase;
        let b = D.state(1800.0, Some(0.0), None).moon_phase;
        near((b - a).rem_euclid(1.0), 1.0 / 8.0, 1e-9);
        assert_eq!(D.state(0.0, None, Some(0.25)).moon_phase, 0.25);
    }

    #[test]
    fn keyframes_are_hit_exactly_and_continuous_everywhere() {
        for k in D.keyframes {
            let s = D.sample(k.sun);
            for (a, b) in colors(&s).iter().zip(colors(k)) {
                assert!(dist(*a, b) < 1e-6, "{}: {a:?} vs {b:?}", k.name);
            }
            for (a, b) in scalars(&s).iter().zip(scalars(k)) {
                near(*a, b, 1e-9);
            }
            // No jump arriving at or leaving a keyframe.
            assert!(gap(&D.sample(k.sun - 1e-6), &s) < 1e-4, "{}", k.name);
            assert!(gap(&D.sample(k.sun + 1e-6), &s) < 1e-4, "{}", k.name);
        }
        // Across 360°/0° and across midnight in time, too.
        assert!(gap(&D.sample(359.999_999), &D.sample(0.0)) < 1e-4);
        let before = D.state(0.0, Some(0.999_999), None);
        let after = D.state(0.0, Some(0.0), None);
        assert!(gap(&before.a, &after.a) < 1e-3 && gap(&before.b, &after.b) < 1e-3);
        // Over the whole day in small steps, every channel moves a little at a time: no pops.
        let n = 20_000;
        let mut last = D.sample(0.0);
        for i in 1..=n {
            let deg = 360.0 * f64::from(i) / f64::from(n);
            let s = D.sample(deg);
            assert!(gap(&s, &last) < 0.02, "at {deg}°");
            last = s;
        }
    }

    #[test]
    fn interpolation_never_overshoots_its_keyframes() {
        // A monotone cubic: between two keyframes each channel stays between their values.
        let k = D.keyframes;
        for i in 0..k.len() {
            let j = (i + 1) % k.len();
            let (a, b) = (channels(&k[i]), channels(&k[j]));
            let span = (k[j].sun - k[i].sun).rem_euclid(360.0);
            for step in 1..50 {
                let s = D.sample_channels(k[i].sun + span * f64::from(step) / 50.0);
                for ch in 0..CHANNELS {
                    let (lo, hi) = (a[ch].min(b[ch]), a[ch].max(b[ch]));
                    assert!(
                        s[ch] >= lo - 1e-9 && s[ch] <= hi + 1e-9,
                        "{} → {} channel {ch}",
                        k[i].name,
                        k[j].name
                    );
                }
            }
        }
    }

    #[test]
    fn oklab_round_trips_and_mixes_without_mud() {
        for c in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.39, 0.6, 0.85],
            [0.9, 0.5, 0.2],
        ] {
            assert!(dist(oklab_to_srgb(srgb_to_oklab(c)), c) < 1e-6);
        }
        // Mixing orange and blue in Oklab keeps more lightness than raw sRGB's midpoint.
        let (o, b) = ([1.0, 0.55, 0.25], [0.3, 0.45, 0.85]);
        let ok: Rgb = std::array::from_fn(|i| 0.5 * (srgb_to_oklab(o)[i] + srgb_to_oklab(b)[i]));
        let raw: Rgb = std::array::from_fn(|i| 0.5 * (o[i] + b[i]));
        assert!(ok[0] > srgb_to_oklab(raw)[0]);
    }

    #[test]
    fn the_pack_fills_the_uniform_block() {
        let day = D.state(0.0, Some(0.5), None);
        let u = day.pack(FaceFrame::A, 1.0, 12.0, 0.002);
        assert_eq!(u.len(), DAY_SKY_FLOATS);
        assert_eq!(&u[0..3], &day.sun.map(|v| v as f32));
        // The viewer's sky; face A's sun light (linear × intensity), face B's none.
        assert!((u[12] - day.a.sky_zenith[0] as f32).abs() < 1e-6);
        assert!(u[36] > 1.0 && u[52] == 0.0);
        assert!((u[34] - 0.002).abs() < 1e-9);
    }

    #[test]
    fn the_debug_line_reads_the_clock() {
        let day = D.state(0.0, Some(0.5), Some(0.5));
        assert_eq!(day.clock_text(), "12:00");
        let line = day.debug_line();
        assert!(
            line.starts_with("time 12:00") && line.contains("sun 90°") && line.contains("100% lit"),
            "{line}"
        );
        assert_eq!(D.state(0.0, Some(0.0), None).clock_text(), "00:00");
    }
}
