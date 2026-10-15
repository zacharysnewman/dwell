//! Motion effects (docs/MOTION_EFFECTS.md, Phase 29): the field of view widening with the camera's
//! speed, and the wind streaks the renderer draws in a screen-space pass. Pure: the game feeds in the
//! camera's velocity and the controller's state each drawn frame and reads back the FOV boost and
//! the wind; nothing here writes simulation state. The tunables are data (`shared/look/motion.json`,
//! generated into [`MOTION`] by `shared/look/gen.mjs`); every function takes them as a parameter so
//! tests can change them.

use dwell_protocol::PlayerState;

#[rustfmt::skip]
mod generated {
    include!("motion.gen.rs");
}
pub use generated::MOTION;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FovTunables {
    /// Degrees added at run speed (eased in from walk speed).
    pub sprint_boost_deg: f64,
    /// Degrees added at `full_mps` and above (log-scaled from run speed).
    pub flight_boost_deg: f64,
    pub full_mps: f64,
    /// Critically damped spring rates (1/s) toward a wider and a narrower target.
    pub widen_rate: f64,
    pub settle_rate: f64,
    /// The boosted view's caps (degrees); a view already wider than these is never narrowed.
    pub max_horizontal_deg: f64,
    pub max_vertical_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindTunables {
    /// Where the streaks start, as a fraction of the way from walk to run speed.
    pub start: f64,
    pub sprint_intensity: f64,
    pub full_mps: f64,
    pub rise_rate: f64,
    pub fall_rate: f64,
    pub streaks: u32,
    pub flight_streaks: u32,
    pub length: f64,
    pub width_px: f64,
    pub opacity: f64,
    /// sRGB, 0–1.
    pub color: [f64; 3],
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub vignette: f64,
    pub flow_speed: f64,
    /// The edge blur toward the vanishing point: a fraction of the distance to it, at full wind.
    pub blur: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionTunables {
    pub fov: FovTunables,
    pub wind: WindTunables,
    /// By `PlayerState` id.
    pub state_gain: [f64; 10],
    /// A camera jump this much farther than its velocity explains is a teleport: the effects reset.
    pub teleport_m: f64,
}

/// The controller's ground speeds the effects are scaled to (dwell-physics' `movement`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speeds {
    pub walk: f64,
    pub run: f64,
}

/// The player's choices (the settings menu): effects on or off, and the FOV boost's strength 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSettings {
    pub enabled: bool,
    pub fov_strength: f64,
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            fov_strength: 1.0,
        }
    }
}

fn smoothstep(t: f64) -> f64 {
    let x = t.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 0 at `from` m/s, 1 at `to`, on a log scale (flight spans 10 m/s to thousands of km/s).
fn log_ramp(speed: f64, from: f64, to: f64) -> f64 {
    if speed <= from || to <= from {
        return 0.0;
    }
    ((speed / from).ln() / (to / from).ln()).clamp(0.0, 1.0)
}

fn state_gain(t: &MotionTunables, state: PlayerState) -> f64 {
    t.state_gain[state as usize]
}

/// Degrees the vertical FOV widens by at `speed` (m/s) in `state`, before smoothing.
pub fn target_fov_boost(speed: f64, state: PlayerState, s: Speeds, t: &MotionTunables) -> f64 {
    let f = &t.fov;
    let sprint = smoothstep((speed - s.walk) / (s.run - s.walk).max(1e-6));
    let flight = smoothstep(log_ramp(speed, s.run, f.full_mps));
    let boost =
        f.sprint_boost_deg * sprint + (f.flight_boost_deg - f.sprint_boost_deg).max(0.0) * flight;
    boost * state_gain(t, state)
}

/// The wind's intensity 0–1 at `speed` in `state`, before smoothing.
pub fn target_wind(speed: f64, state: PlayerState, s: Speeds, t: &MotionTunables) -> f64 {
    let w = &t.wind;
    let start = s.walk + (s.run - s.walk) * w.start;
    let sprint = smoothstep((speed - start) / (s.run - start).max(1e-6));
    let flight = smoothstep(log_ramp(speed, s.run, w.full_mps));
    let v = w.sprint_intensity * sprint + (1.0 - w.sprint_intensity) * flight;
    v * state_gain(t, state)
}

/// A critically damped spring: no overshoot, exact for any step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub value: f64,
    pub velocity: f64,
}

impl Spring {
    /// Steps `dt` seconds toward `target` at `rate` (1/s; it settles in about 5 / rate).
    pub fn step(&mut self, target: f64, rate: f64, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let x = self.value - target;
        let e = (-rate * dt).exp();
        let temp = (self.velocity + rate * x) * dt;
        self.velocity = (self.velocity - rate * temp) * e;
        self.value = target + (x + temp) * e;
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Horizontal field of view (degrees) that a vertical one gives at an aspect ratio (width / height).
pub fn horizontal_fov(vertical_deg: f64, aspect: f64) -> f64 {
    (2.0 * ((vertical_deg.to_radians() / 2.0).tan() * aspect).atan()).to_degrees()
}
/// The vertical field of view that gives a horizontal one.
fn vertical_for_horizontal(horizontal_deg: f64, aspect: f64) -> f64 {
    (2.0 * ((horizontal_deg.to_radians() / 2.0).tan() / aspect).atan()).to_degrees()
}

/// The vertical FOV a frame draws with: `base` (the screen's unboosted FOV) widened by `boost`
/// degrees, but never past the caps — the horizontal one, then the vertical — unless the base view
/// already is (a cap never narrows the view).
pub fn effective_fov(base: f64, boost: f64, aspect: f64, t: &MotionTunables) -> f64 {
    let wanted = base + boost.max(0.0);
    let by_width = vertical_for_horizontal(t.fov.max_horizontal_deg, aspect);
    let cap = by_width.min(t.fov.max_vertical_deg).max(base);
    wanted.min(cap)
}

/// Where the direction of travel projects on the screen (NDC, ±1 at the edges) and +1 when moving
/// into the view or −1 when moving away (streaks then flow toward the point). `dir` is in view space:
/// x right, y up, z forward. Sideways travel puts the point far off the screen, clamped to `reach`.
pub fn vanishing_point(dir: [f64; 3], tan_x: f64, tan_y: f64) -> ([f64; 2], f64) {
    const REACH: f64 = 6.0;
    let sign = if dir[2] >= 0.0 { 1.0 } else { -1.0 };
    let z = dir[2].abs().max(1e-6);
    let p = [dir[0] / z / tan_x, dir[1] / z / tan_y];
    let len = (p[0] * p[0] + p[1] * p[1]).sqrt();
    let k = if len > REACH { REACH / len } else { 1.0 };
    ([p[0] * k, p[1] * k], sign)
}

/// One drawn frame's inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionInput {
    /// The camera's eye (world, m) and velocity (m/s): the predicted player's, flight included.
    pub eye: [f64; 3],
    pub velocity: [f64; 3],
    pub state: PlayerState,
    pub dt: f64,
    /// The player's own eyes are the camera (alive, no tool camera).
    pub first_person: bool,
    /// The camera is turning over across the midplane this frame (the somersault started).
    pub turned_over: bool,
    /// The game menu is open: nothing changes.
    pub paused: bool,
}

/// What the renderer draws with this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotionOutput {
    /// Degrees added to the vertical FOV (before the caps, `effective_fov`).
    pub fov_boost_deg: f64,
    /// 0–1.
    pub wind: f64,
    /// The flight layer 0–1 (the wind beyond sprint intensity).
    pub flight: f64,
    /// Unit world direction of travel (zero when still).
    pub direction: [f64; 3],
    /// Seconds of animation (wrapped).
    pub time: f64,
}

/// The effects' smoothed state between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotionFx {
    pub fov: Spring,
    pub wind: Spring,
    last_eye: Option<[f64; 3]>,
    time: f64,
    direction: [f64; 3],
}

impl MotionFx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops everything: the next frame starts from an unwidened view and no wind.
    pub fn reset(&mut self) {
        self.fov.reset();
        self.wind.reset();
    }

    /// Advances one drawn frame and returns what to draw.
    pub fn frame(
        &mut self,
        input: &MotionInput,
        settings: MotionSettings,
        speeds: Speeds,
        t: &MotionTunables,
    ) -> MotionOutput {
        if input.paused {
            return self.output(t);
        }
        let v = input.velocity;
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        // A teleport or respawn: the eye moved much farther than its velocity carried it.
        let jumped = self.last_eye.is_some_and(|e| {
            let d: [f64; 3] = std::array::from_fn(|i| input.eye[i] - e[i] - v[i] * input.dt);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() > t.teleport_m
        });
        self.last_eye = Some(input.eye);
        if jumped || input.turned_over || !input.first_person || !settings.enabled {
            self.reset();
        }
        if speed > 1e-3 {
            self.direction = v.map(|c| c / speed);
        }
        let on = input.first_person && settings.enabled;
        let (fov_target, wind_target) = if on {
            (
                target_fov_boost(speed, input.state, speeds, t)
                    * settings.fov_strength.clamp(0.0, 1.0),
                target_wind(speed, input.state, speeds, t),
            )
        } else {
            (0.0, 0.0)
        };
        let dt = input.dt.clamp(0.0, 0.25);
        let rate =
            |target: f64, value: f64, up: f64, down: f64| if target > value { up } else { down };
        self.fov.step(
            fov_target,
            rate(
                fov_target,
                self.fov.value,
                t.fov.widen_rate,
                t.fov.settle_rate,
            ),
            dt,
        );
        self.wind.step(
            wind_target,
            rate(
                wind_target,
                self.wind.value,
                t.wind.rise_rate,
                t.wind.fall_rate,
            ),
            dt,
        );
        self.time = (self.time + dt) % 1000.0;
        self.output(t)
    }

    fn output(&self, t: &MotionTunables) -> MotionOutput {
        let wind = self.wind.value.clamp(0.0, 1.0);
        let s = t.wind.sprint_intensity;
        MotionOutput {
            fov_boost_deg: self.fov.value.max(0.0),
            wind,
            flight: if s < 1.0 {
                ((wind - s) / (1.0 - s)).clamp(0.0, 1.0)
            } else {
                0.0
            },
            direction: self.direction,
            time: self.time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEEDS: Speeds = Speeds {
        walk: 5.0,
        run: 8.0,
    };

    fn input(speed: f64, state: PlayerState) -> MotionInput {
        MotionInput {
            eye: [0.0; 3],
            velocity: [0.0, 0.0, speed],
            state,
            dt: 1.0 / 60.0,
            first_person: true,
            turned_over: false,
            paused: false,
        }
    }

    /// Runs `seconds` of frames at a constant velocity (the eye moving with it).
    fn run(
        fx: &mut MotionFx,
        i: &mut MotionInput,
        seconds: f64,
        t: &MotionTunables,
    ) -> MotionOutput {
        let mut out = MotionOutput::default();
        for _ in 0..(seconds * 60.0) as usize {
            for k in 0..3 {
                i.eye[k] += i.velocity[k] * i.dt;
            }
            out = fx.frame(i, MotionSettings::default(), SPEEDS, t);
        }
        out
    }

    #[test]
    fn the_target_fov_is_zero_at_walk_speed_and_grows_to_sprint_then_flight() {
        let t = &MOTION;
        let at = |v: f64| target_fov_boost(v, PlayerState::Running, SPEEDS, t);
        assert_eq!(at(0.0), 0.0);
        assert_eq!(at(SPEEDS.walk), 0.0);
        assert!((at(SPEEDS.run) - t.fov.sprint_boost_deg).abs() < 1e-9);
        assert!(at(6.5) > 0.0 && at(6.5) < t.fov.sprint_boost_deg);
        // Log-scaled beyond run speed: 100 m/s is well short of the flight boost, the full speed reaches it.
        let mid = at(100.0);
        assert!(
            mid > t.fov.sprint_boost_deg && mid < t.fov.flight_boost_deg,
            "{mid}"
        );
        assert!((at(t.fov.full_mps) - t.fov.flight_boost_deg).abs() < 1e-9);
        assert_eq!(at(1e9), at(t.fov.full_mps));
        // Monotonic.
        let mut last = 0.0;
        for v in (0..4000).map(|i| f64::from(i) * 0.5) {
            assert!(at(v) >= last - 1e-12);
            last = at(v);
        }
    }

    #[test]
    fn states_gate_the_targets_by_their_gain() {
        let t = &MOTION;
        assert_eq!(target_fov_boost(8.0, PlayerState::Climbing, SPEEDS, t), 0.0);
        assert_eq!(target_wind(8.0, PlayerState::Climbing, SPEEDS, t), 0.0);
        assert!(target_wind(30.0, PlayerState::Falling, SPEEDS, t) > 0.0);
        assert!(
            target_wind(30.0, PlayerState::Flying, SPEEDS, t)
                > target_wind(8.0, PlayerState::Running, SPEEDS, t)
        );
    }

    #[test]
    fn a_tunable_changed_by_data_changes_the_effect() {
        let mut t = MOTION;
        let before = target_fov_boost(SPEEDS.run, PlayerState::Running, SPEEDS, &t);
        t.fov.sprint_boost_deg = 4.0;
        assert_eq!(
            target_fov_boost(SPEEDS.run, PlayerState::Running, SPEEDS, &t),
            4.0
        );
        assert_ne!(before, 4.0);
        t.state_gain[PlayerState::Running as usize] = 0.5;
        assert_eq!(
            target_fov_boost(SPEEDS.run, PlayerState::Running, SPEEDS, &t),
            2.0
        );
        t.wind.start = 0.0;
        assert!(target_wind(SPEEDS.walk + 0.5, PlayerState::Running, SPEEDS, &t) > 0.0);
        t.wind.start = 0.9;
        assert_eq!(
            target_wind(SPEEDS.walk + 0.5, PlayerState::Running, SPEEDS, &t),
            0.0
        );
    }

    #[test]
    fn the_spring_is_critically_damped_and_widens_faster_than_it_settles() {
        let mut s = Spring::default();
        let mut peak: f64 = 0.0;
        for _ in 0..600 {
            s.step(10.0, 8.0, 1.0 / 60.0);
            peak = peak.max(s.value);
        }
        assert!(
            (s.value - 10.0).abs() < 1e-6 && peak <= 10.0 + 1e-9,
            "no overshoot"
        );
        // Exact for any step: one big step lands where many small ones do.
        let (mut a, mut b) = (Spring::default(), Spring::default());
        a.step(1.0, 5.0, 0.5);
        for _ in 0..500 {
            b.step(1.0, 5.0, 0.001);
        }
        assert!(
            (a.value - b.value).abs() < 1e-9,
            "{} vs {}",
            a.value,
            b.value
        );
        // Widening to 90 % takes less time than settling back to 10 %.
        let t = &MOTION;
        let mut fx = MotionFx::new();
        let mut i = input(SPEEDS.run, PlayerState::Running);
        let (mut up, mut down) = (0, 0);
        while fx.fov.value < 0.9 * t.fov.sprint_boost_deg {
            run(&mut fx, &mut i, 1.0 / 60.0, t);
            up += 1;
        }
        i.velocity = [0.0; 3];
        while fx.fov.value > 0.1 * t.fov.sprint_boost_deg {
            run(&mut fx, &mut i, 1.0 / 60.0, t);
            down += 1;
        }
        assert!(up < down, "widen {up} frames, settle {down}");
        assert!(up <= 36, "widens within 0.6 s: {up}");
    }

    #[test]
    fn the_caps_bound_the_horizontal_and_vertical_view_but_never_narrow_it() {
        let t = &MOTION;
        let wide = 16.0 / 9.0;
        let base = 67.0;
        let v = effective_fov(base, 40.0, wide, t);
        assert!((horizontal_fov(v, wide) - t.fov.max_horizontal_deg).abs() < 1e-6);
        // A tall screen stops at the vertical cap.
        let v = effective_fov(75.0, 60.0, 0.4, t);
        assert!((v - t.fov.max_vertical_deg).abs() < 1e-9);
        // Under the caps the boost is added whole; none leaves the base as it is.
        assert!((effective_fov(base, 5.0, wide, t) - (base + 5.0)).abs() < 1e-9);
        assert_eq!(effective_fov(base, 0.0, wide, t), base);
        assert_eq!(effective_fov(base, -3.0, wide, t), base);
        // A base already past the caps (a very wide screen) is kept, not narrowed.
        let mut low = MOTION;
        low.fov.max_horizontal_deg = 90.0;
        assert_eq!(effective_fov(base, 10.0, wide, &low), base);
    }

    #[test]
    fn the_effects_reset_on_a_teleport_a_respawn_and_the_turn_over() {
        let t = &MOTION;
        let settled = || {
            let mut fx = MotionFx::new();
            let mut i = input(SPEEDS.run, PlayerState::Running);
            run(&mut fx, &mut i, 2.0, t);
            assert!(fx.fov.value > 0.9 * t.fov.sprint_boost_deg);
            assert!(fx.wind.value > 0.0);
            (fx, i)
        };
        // Teleport: the eye jumps far beyond what the velocity explains.
        let (mut fx, mut i) = settled();
        i.eye[0] += 500.0;
        let out = fx.frame(&i, MotionSettings::default(), SPEEDS, t);
        assert!(out.fov_boost_deg < 0.5, "{}", out.fov_boost_deg);
        // Moving fast is no teleport.
        let (mut fx, mut i) = settled();
        i.velocity = [0.0, 0.0, 1e6];
        i.eye[2] += 1e6 / 60.0;
        let out = fx.frame(&i, MotionSettings::default(), SPEEDS, t);
        assert!(out.fov_boost_deg > 0.9 * t.fov.sprint_boost_deg);
        // Respawn (third person while dead, then back): nothing carried over.
        let (mut fx, mut i) = settled();
        i.first_person = false;
        let out = fx.frame(&i, MotionSettings::default(), SPEEDS, t);
        assert!(out.fov_boost_deg < 0.5 && out.wind < 0.05);
        // The turn-over across the midplane.
        let (mut fx, mut i) = settled();
        i.turned_over = true;
        let out = fx.frame(&i, MotionSettings::default(), SPEEDS, t);
        assert!(out.fov_boost_deg < 0.5 && out.wind < 0.05);
    }

    #[test]
    fn nothing_changes_while_the_menu_is_open_and_settings_scale_or_stop_it() {
        let t = &MOTION;
        let mut fx = MotionFx::new();
        let mut i = input(SPEEDS.run, PlayerState::Running);
        let before = run(&mut fx, &mut i, 0.2, t);
        i.paused = true;
        i.velocity = [0.0; 3];
        for _ in 0..120 {
            assert_eq!(fx.frame(&i, MotionSettings::default(), SPEEDS, t), before);
        }
        // Half strength settles at half the boost; off is no effect at all.
        let mut fx = MotionFx::new();
        let mut i = input(SPEEDS.run, PlayerState::Running);
        let half = MotionSettings {
            enabled: true,
            fov_strength: 0.5,
        };
        for _ in 0..240 {
            fx.frame(&i, half, SPEEDS, t);
        }
        assert!((fx.fov.value - 0.5 * t.fov.sprint_boost_deg).abs() < 1e-3);
        i.velocity = [0.0, 0.0, 500.0];
        let off = MotionSettings {
            enabled: false,
            fov_strength: 1.0,
        };
        let out = fx.frame(&i, off, SPEEDS, t);
        assert_eq!((out.fov_boost_deg, out.wind), (0.0, 0.0));
    }

    #[test]
    fn the_vanishing_point_follows_the_direction_of_travel() {
        let (tx, ty) = (1.2, 0.7);
        assert_eq!(vanishing_point([0.0, 0.0, 1.0], tx, ty), ([0.0, 0.0], 1.0));
        // Falling while looking ahead: the point drops below the centre.
        let (p, s) = vanishing_point([0.0, -0.35, 0.94], tx, ty);
        assert!(p[1] < -0.4 && p[0] == 0.0 && s == 1.0);
        // Strafing right: far off to the right, clamped.
        let (p, _) = vanishing_point([1.0, 0.0, 0.0], tx, ty);
        assert!((p[0] - 6.0).abs() < 1e-9);
        // Backing away: the point ahead, streaks flowing in.
        let (p, s) = vanishing_point([0.2, 0.0, -1.0], tx, ty);
        assert!(p[0] > 0.0 && s == -1.0);
    }

    #[test]
    fn the_generated_tunables_match_the_data() {
        let json = include_str!("../../../shared/look/motion.json");
        assert!(json.contains(&format!(
            "\"sprintBoostDeg\": {}",
            MOTION.fov.sprint_boost_deg
        )));
        assert!(json.contains(&format!("\"streaks\": {}", MOTION.wind.streaks)));
    }
}
