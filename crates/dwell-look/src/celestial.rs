//! The celestial model (docs/proposals/MOON_WORLD.md §6, M-4): the sun, the home disc and the moon
//! as bodies, all pure functions of the world clock, so the server and every client place them
//! alike with no message.
//!
//! - **The home disc** is at rest; positions here are offsets from its centre (the midplane's
//!   centre, world axes, metres).
//! - **The sun** is today's direction exactly ([`DayNight::sun_direction`] at the clock's solar
//!   angle, DAY_NIGHT.md §3): the clock, the path and both faces' days are unchanged.
//! - **The sky frame** turns with the sun: its axes are toward the sun, a quarter turn on along the
//!   sun's path, and the path's axis. The stars are fixed in it (DAY_NIGHT.md §3).
//! - **The moon** goes round a circle `moon_orbit.distance_m` out, inclined `inclination_deg` to the
//!   sun's path, once a month (`moon.cycle_days`, new moon to new moon) against the sky's daily
//!   turn, so it rises later each day. Its ascending node sits `node_deg` along the path from the sun
//!   at day 0 and turns once against the sun every `node_turn_days` (eclipse seasons). The month's
//!   count starts at the world's phase (`WorldSettings::moon_phase`, an optional tag; absent, the
//!   data's `moon.start_phase`).
//! - **Tidally locked** (owner, 2026-11-12): the moon grid's up (face A's) always points at the home
//!   disc's centre, so home sees face A at every phase; face B is the far side.
//! - **Phases from geometry** on a domed disc (owner, 2026-11-12, changing Q3: "You mentioned a dome
//!   something, maybe do that"): the flat face's normals blended `moon.domeBias` toward a sphere's,
//!   so the phases read as crescents, halves and gibbous with the baked craters on top; the lit
//!   share is that disc's ([`disc_lit`]), full opposite the sun, 0 at new.
//! - **Moonlight on the ground** is today's moonlight × (`moon.night_floor` + (1 − floor) × the lit
//!   share × how much of the disc is over the face's horizon): darker than today's 55% floor on a
//!   moonless night, never black (owner: "A darker floor but a minimum still.").
//! - **The look** (owner: "the moon should look bright"): the lit part is as bright as today's moon
//!   (`moon.color`); its bloom and glow are today's × the lit share ([`Celestial::glow`]).
//!
//! Drawn by M-2: `DayNight::state` places the sky's moon, its lit share and its glow from this.

use crate::daynight::{Clock, DAY_NIGHT, DayNight};
use dwell_protocol::world_settings::WorldSettings;
use dwell_world::FaceFrame;

pub type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    a.map(|v| v * k)
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// Grid cells a side over the disc for [`disc_lit`]'s average.
const DISC_CELLS: usize = 24;

/// The lit share of the moon's face seen, by the cosine `cos_a` between its flat normal and the
/// sun: the disc's mean N·L with each point's normal blended `dome` toward a sphere's
/// (`moon.domeBias`, the sky's `MOON_DOME`: 0 the flat disc, n·s; 1 a ball, (1 + cos a) / 2), over
/// the same at full, so a full moon is 1 and a new one 0. The drawn moon (sky.wgsl `dwell_moon`)
/// blends its normals the same way, so the halo and the moonlight follow what is drawn.
pub fn disc_lit(cos_a: f64, dome: f64) -> f64 {
    let c = cos_a.clamp(-1.0, 1.0);
    let s = (1.0 - c * c).max(0.0).sqrt();
    let mean = |sun: V3| {
        let (mut sum, mut n) = (0.0, 0usize);
        for j in 0..DISC_CELLS {
            for i in 0..DISC_CELLS {
                let x = (i as f64 + 0.5) / DISC_CELLS as f64 * 2.0 - 1.0;
                let y = (j as f64 + 0.5) / DISC_CELLS as f64 * 2.0 - 1.0;
                let r2 = x * x + y * y;
                if r2 > 1.0 {
                    continue;
                }
                let ball = [x, y, (1.0 - r2).sqrt()];
                let nrm = unit(std::array::from_fn(|k| {
                    dome * ball[k] + (1.0 - dome) * [0.0, 0.0, 1.0][k]
                }));
                sum += dot(nrm, sun).max(0.0);
                n += 1;
            }
        }
        sum / n.max(1) as f64
    };
    let full = mean([0.0, 0.0, 1.0]).max(1e-9);
    (mean([s, 0.0, c]) / full).clamp(0.0, 1.0)
}

/// What a viewer at home sees of the moon at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonView {
    /// The moon's face toward the viewer (A from anywhere at home: it is tidally locked).
    pub face: FaceFrame,
    /// The share of that face lit by the sun (n·s, 0 when the sun is behind it): 0 new, 1 full.
    pub lit: f64,
    /// The phase by how far the moon is behind the sun along the sun's path, seen from the viewer:
    /// 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter.
    pub phase: f64,
    /// Toward the moon's centre from the viewer (unit, world).
    pub direction: V3,
}

/// The sun, the home disc and the moon at any world time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Celestial {
    /// The cycle's data with the world's clock.
    pub day: DayNight,
    /// The month's phase at day 0 (0 new, 0.5 full).
    pub start_phase: f64,
}

impl Celestial {
    /// The model for a cycle; `start_phase` the world's (None: the data's `moon.start_phase`).
    pub fn new(day: DayNight, start_phase: Option<f64>) -> Self {
        Self {
            day,
            start_phase: start_phase.unwrap_or(day.moon.start_phase).rem_euclid(1.0),
        }
    }

    /// The model for a world: its clock and its moon phase (ADR 0054), this build's data otherwise.
    pub fn for_world(s: &WorldSettings) -> Self {
        let clock = Clock {
            day_s: f64::from(s.clock.day_s),
            night_s: f64::from(s.clock.night_s),
            start_hour: f64::from(s.clock.start_hour),
        };
        Self::new(DayNight { clock, ..DAY_NIGHT }, s.moon_phase.map(f64::from))
    }

    /// Face A's solar angle at `world_s` (DAY_NIGHT.md §3, unchanged).
    pub fn solar_deg(&self, world_s: f64) -> f64 {
        self.day.solar_deg(self.day.time_of_day(world_s))
    }

    /// Toward the sun (unit, world) at `world_s`: today's path.
    pub fn sun_direction(&self, world_s: f64) -> V3 {
        self.day.sun_direction(self.solar_deg(world_s))
    }

    /// The month's count at `world_s` (0 new, 0.5 full): the phase the orbit is placed by.
    pub fn month_phase(&self, world_s: f64) -> f64 {
        (self.start_phase + self.day.days(world_s) / self.day.moon.cycle_days).rem_euclid(1.0)
    }

    /// The sky frame's axes in the world: toward the sun, a quarter turn on along its path, the
    /// path's axis (right-handed).
    fn sky_frame(&self, world_s: f64) -> [V3; 3] {
        let a = self.solar_deg(world_s);
        [
            self.day.sun_direction(a),
            self.day.sun_direction(a + 90.0),
            self.day.sky_axis(),
        ]
    }

    /// The moon's place on its orbit in the sky frame: (toward it, along the orbit, the orbit's
    /// normal), unit.
    fn orbit_local(&self, world_s: f64) -> [V3; 3] {
        let o = self.day.moon_orbit;
        let days = self.day.days(world_s);
        let turn = if o.node_turn_days > 0.0 {
            360.0 * days / o.node_turn_days
        } else {
            0.0
        };
        let node = (o.node_deg - turn).to_radians();
        // Against the daily turn: 0 at new moon (toward the sun), 180° at full.
        let lon = (-360.0 * self.month_phase(world_s)).to_radians();
        let u = lon - node;
        let (si, ci) = o.inclination_deg.to_radians().sin_cos();
        let (sn, cn) = node.sin_cos();
        // Tilted by the inclination about the node's line, then turned to the node.
        let place = |[x, y, z]: V3| {
            let (y1, z1) = (y * ci - z * si, y * si + z * ci);
            [x * cn - y1 * sn, x * sn + y1 * cn, z1]
        };
        let (su, cu) = u.sin_cos();
        [
            place([cu, su, 0.0]),
            place([-su, cu, 0.0]),
            place([0.0, 0.0, 1.0]),
        ]
    }

    fn in_world(f: &[V3; 3], v: V3) -> V3 {
        std::array::from_fn(|i| f[0][i] * v[0] + f[1][i] * v[1] + f[2][i] * v[2])
    }

    /// Toward the moon's centre from the home disc's centre (unit, world).
    pub fn moon_direction(&self, world_s: f64) -> V3 {
        let f = self.sky_frame(world_s);
        unit(Self::in_world(&f, self.orbit_local(world_s)[0]))
    }

    /// The moon's centre from the home disc's centre (m, world): the moon grid's origin (M-3).
    pub fn moon_position(&self, world_s: f64) -> V3 {
        scale(self.moon_direction(world_s), self.day.moon_orbit.distance_m)
    }

    /// The moon grid's axes in the world (x, y, z; orthonormal, right-handed), tidally locked: y,
    /// its face A's up, points at the home disc's centre; x along its orbit.
    pub fn moon_axes(&self, world_s: f64) -> [V3; 3] {
        let f = self.sky_frame(world_s);
        let [toward, along, _] = self.orbit_local(world_s).map(|v| Self::in_world(&f, v));
        let y = scale(unit(toward), -1.0);
        let x = unit(sub(along, scale(y, dot(along, y))));
        [x, y, cross(x, y)]
    }

    /// The moon seen from `viewer` (an offset from the home disc's centre, world axes, m) at
    /// `world_s`.
    pub fn view(&self, world_s: f64, viewer: V3) -> MoonView {
        let sun = self.sun_direction(world_s);
        let moon = self.moon_position(world_s);
        let up_a = self.moon_axes(world_s)[1];
        let back = sub(viewer, moon); // from the moon toward the viewer
        // The moon grid's own faces: the side of its midplane the viewer is on, its up for A,
        // reversed for B (as the home grid's `FaceFrame::up`).
        let side = dot(back, up_a);
        let face = FaceFrame::of_sign(side);
        let normal = if side < 0.0 { scale(up_a, -1.0) } else { up_a };
        let direction = unit(scale(back, -1.0));
        // The moon's angle behind the sun along its path, seen from the viewer (the sky frame's
        // longitude, so the moon's latitude off the path never flips it near new or full).
        let f = self.sky_frame(world_s);
        let lon = dot(direction, f[1]).atan2(dot(direction, f[0]));
        let phase = -lon / std::f64::consts::TAU;
        MoonView {
            face,
            lit: disc_lit(dot(normal, sun), self.day.moon.dome_bias),
            phase: phase.rem_euclid(1.0),
            direction,
        }
    }

    /// The share of today's moonlight on face `face` at `viewer` (an offset from the home disc's
    /// centre): the night floor, plus the rest × the lit share × the share of the disc over the
    /// face's horizon.
    pub fn moonlight(&self, world_s: f64, viewer: V3, face: FaceFrame) -> f64 {
        let v = self.view(world_s, viewer);
        let elevation = dot(v.direction, face.up()).clamp(-1.0, 1.0).asin();
        let over = (0.5 + 0.5 * elevation / self.day.moon.radius_deg.to_radians()).clamp(0.0, 1.0);
        let floor = self.day.moon.night_floor;
        floor + (1.0 - floor) * v.lit * over
    }

    /// The moon's halo seen in `v`: today's (bloom, glow) strengths × the lit share, so a full moon
    /// glows as today's and a new one not at all (the keyframes' `moonGlow` multiplies both, as
    /// today). The lit part's own brightness is today's `moon.color`; the bake shapes the surface,
    /// not its overall brightness.
    pub fn glow(&self, v: &MoonView) -> (f64, f64) {
        (self.day.moon.bloom * v.lit, self.day.moon.glow * v.lit)
    }
}

#[cfg(test)]
mod tests;
