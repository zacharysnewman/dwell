//! First-person eye height smoothing (PLAYER_CONTROLLER.md §9). The controller moves the body in
//! jumps the camera must not show: onto or down a step in one tick (step-up, ground snap), and by
//! up to a body height when crouching or standing in mid-air (the head stays put, the feet move).
//! Each jump is folded into an offset that then decays, so the eye always moves continuously.
//! A step's offset decays along a virtual ramp ([`StepSmoother`]): at the speed a walker climbing
//! a ramp of the step view's slope would rise, so a step feels like a short ramp and a stair like
//! one long one, while the body itself steps (the sim is unchanged). [`EyeSmoother`] runs once per
//! sim tick; [`EyeCamera`] feeds it client states and interpolates its last two results for each
//! drawn frame. Heights are measured along the player's up (BIFACIAL_WORLD.md §6): on face B the
//! head points toward −y, so `up height = face × y`; the smoothing is the same either way, and
//! starts afresh when the player crosses to the other face. There the body turns over, so the eye
//! goes from above the body's centre to below it: it swings across over the camera's settling
//! time (`face_flip.rs`) rather than jumping by most of a body height.

use dwell_physics::config::StepView;
use dwell_protocol::controller_flags;
use dwell_world::FaceFrame;

use crate::face_flip::{FLIP_SECONDS, ease};

/// Crouch offset decay rate (1/s).
const CROUCH_RATE: f64 = 12.0;
/// A grounded per-tick feet change larger than this is a step, not riding a lift or a slope.
const STEP_MIN: f64 = 0.1;
/// A crouch offset larger than this is a teleport (respawn), not a crouch.
const CROUCH_MAX: f64 = 2.0;
/// A drawn body's step offset decays at this speed (m/s) in the air ([`StepSmoother::follow`]).
const AIR_SPEED: f64 = 6.0;
/// Steps further apart than this (m walked) are each a lone step, not one stair.
const STAIR_GAP: f64 = 3.0;

/// The step view's tunables (`PlayerControllerConfig::step_view`), in f64.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepRamp {
    pub slope: f64,
    pub max_slope: f64,
    pub min_speed: f64,
}

impl From<StepView> for StepRamp {
    fn from(v: StepView) -> Self {
        Self {
            slope: f64::from(v.slope),
            max_slope: f64::from(v.max_slope),
            min_speed: f64::from(v.min_speed),
        }
    }
}

impl Default for StepRamp {
    fn default() -> Self {
        dwell_physics::config::PlayerControllerConfig::default()
            .step_view
            .into()
    }
}

/// One tick of a body's feet, for [`StepSmoother`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepSample {
    /// Feet height along the body's up (drawn position).
    pub feet: f64,
    pub grounded: bool,
    /// Horizontal speed (m/s).
    pub speed: f64,
    /// The largest grounded one-tick feet change that is a step (larger is a teleport).
    pub max_step: f64,
    pub ramp: StepRamp,
}

/// A step the smoother saw this tick: the feet moved `height` (signed, + up) in one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub height: f64,
}

/// Smooths a body's steps (step-ups and the ground snap down a step) into a virtual ramp: the
/// offset to add to the feet so the drawn height glides. Used by the first-person eye and meant
/// for a drawn body too (a third-person character's root), one per body. Client-only.
#[derive(Clone, Debug, Default)]
pub struct StepSmoother {
    offset: f64,
    /// The current virtual ramp's slope (rise per m walked).
    slope: f64,
    /// Distance walked since the last step, and that step's direction (+1 up, −1 down).
    since: f64,
    last_sign: f64,
    /// The last step's height (unsigned).
    height: f64,
    /// On a stair, the last tread: the step's height and the distance walked to it.
    tread: Option<(f64, f64)>,
    last: Option<StepSample>,
}

impl StepSmoother {
    pub fn new() -> Self {
        Self::default()
    }

    /// The offset to add to the feet (m, along the up): negative while a step up is being climbed.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// The per-frame form, for a drawn body whose feet come interpolated (a remote player: its
    /// step-up is spread over a snapshot interval, so no single frame shows it): on the ground the
    /// drawn feet follow the real ones at no more than the steepest virtual ramp's speed
    /// (`max(min_speed, max_slope × speed)`, so walkable slopes are followed exactly and a step is
    /// taken as a 45° ramp); in the air the offset left decays at `AIR_SPEED`; further than two
    /// steps off is a teleport. Returns a step when one starts (the offset leaves zero).
    pub fn follow(&mut self, s: StepSample, dt: f64) -> Option<Step> {
        let r = s.ramp;
        let was = self.offset;
        let mut offset = match &self.last {
            Some(last) if s.grounded && last.grounded => was + last.feet - s.feet,
            _ => was,
        };
        if offset.abs() > 2.0 * s.max_step {
            offset = 0.0;
        }
        let speed = if s.grounded {
            (r.max_slope * s.speed).max(r.min_speed)
        } else {
            AIR_SPEED
        };
        let decay = speed * dt;
        offset = if offset.abs() <= decay {
            0.0
        } else {
            offset - offset.signum() * decay
        };
        self.offset = offset;
        self.last = Some(s);
        (was.abs() < STEP_MIN && offset.abs() >= STEP_MIN).then_some(Step { height: -offset })
    }

    /// Advances one tick of `dt` seconds; returns the step taken this tick, if any.
    pub fn tick(&mut self, s: StepSample, dt: f64) -> Option<Step> {
        let r = s.ramp;
        if self.slope == 0.0 {
            self.slope = r.slope;
        }
        self.since += s.speed * dt;
        let mut step = None;
        if let Some(last) = &self.last {
            let d = s.feet - last.feet;
            if s.grounded && last.grounded && d.abs() > STEP_MIN && d.abs() <= s.max_step {
                let sign = d.signum();
                // A stair: the slope from this step and the distance since the one before.
                // Over the last two treads when there were two: a tick's rounding of one tread's
                // length is noise.
                let stair = sign == self.last_sign && self.since < STAIR_GAP;
                self.slope = if stair {
                    let (rise, run) = match self.tread {
                        Some((h, l)) => (d.abs() + h, self.since + l),
                        None => (d.abs(), self.since),
                    };
                    (rise / run.max(1e-6)).clamp(r.slope, r.max_slope)
                } else {
                    r.slope
                };
                self.tread = stair.then_some((d.abs(), self.since));
                self.offset -= d;
                self.since = 0.0;
                self.last_sign = sign;
                self.height = d.abs();
                step = Some(Step { height: d });
            }
        }
        // Along the virtual ramp (from the step's own tick, so the eye never pauses on a stair): the rise a walker at this speed would make on it.
        // Further behind than one step (a stair steeper than the ramp so far), it catches up in
        // proportion, so the lag stays about a step.
        let behind = 1.0 + 0.5 * (self.offset.abs() / self.height.max(STEP_MIN) - 1.0).max(0.0);
        let speed = (self.slope * s.speed).max(r.min_speed) * behind;
        let decay = speed * dt;
        self.offset = if self.offset.abs() <= decay {
            0.0
        } else {
            self.offset - self.offset.signum() * decay
        };
        // Never more than a step behind: on a stair steeper than the steepest ramp the eye keeps
        // up at the step's height.
        self.offset = self.offset.clamp(-2.0 * s.max_step, 2.0 * s.max_step);
        self.last = Some(s);
        step
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeSample {
    /// Feet height along the player's up (drawn position).
    pub feet: f64,
    pub crouched: bool,
    pub grounded: bool,
    /// Vertical velocity along the player's up.
    pub velocity_y: f64,
    /// Horizontal speed (m/s).
    pub speed: f64,
    pub eye_height: f64,
    pub crouch_eye_height: f64,
    pub max_step_height: f64,
    pub ramp: StepRamp,
}

pub fn eye_of(s: &EyeSample) -> f64 {
    s.feet
        + if s.crouched {
            s.crouch_eye_height
        } else {
            s.eye_height
        }
}

#[derive(Clone, Debug, Default)]
pub struct EyeSmoother {
    step: StepSmoother,
    crouch: f64,
    last: Option<EyeSample>,
}

impl EyeSmoother {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances one tick of `dt` seconds; returns the camera's eye height for that tick.
    pub fn tick(&mut self, s: EyeSample, dt: f64) -> f64 {
        self.crouch *= (-CROUCH_RATE * dt).exp();
        let crouch_changed = self.last.is_some_and(|l| l.crouched != s.crouched);
        if let Some(last) = &self.last
            && crouch_changed
        {
            // Everything but the tick's own motion is the crouch.
            self.crouch -= eye_of(&s) - eye_of(last) - s.velocity_y * dt;
        }
        self.step.tick(
            StepSample {
                feet: s.feet,
                // A crouch moves the feet in mid-air (or on the ground it doesn't): not a step.
                grounded: s.grounded && !crouch_changed,
                speed: s.speed,
                max_step: s.max_step_height + 0.05,
                ramp: s.ramp,
            },
            dt,
        );
        if self.crouch.abs() > CROUCH_MAX {
            self.crouch = 0.0;
        }
        self.last = Some(s);
        eye_of(&s) + self.step.offset() + self.crouch
    }
}

/// The parts of a predicted client state the camera reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeState {
    pub position: [f64; 3],
    pub render_offset: [f64; 3],
    pub half_height: f64,
    pub velocity: [f64; 3],
    pub controller_flags: u8,
    pub face: FaceFrame,
    pub eye_height: f64,
    pub crouch_eye_height: f64,
    pub max_step_height: f64,
    pub ramp: StepRamp,
}

/// Feet height from a state's own half height: the capsule centre moves when crouching.
pub fn eye_sample_of(s: &EyeState) -> EyeSample {
    let f = s.face;
    let [px, py, pz] = s.position;
    let [ox, oy, oz] = s.render_offset;
    let at = f.point([px + ox, py + oy, pz + oz]);
    let v = f.vector(s.velocity);
    EyeSample {
        feet: at[1] - s.half_height,
        crouched: s.controller_flags & controller_flags::CROUCHING != 0,
        grounded: s.controller_flags & controller_flags::GROUNDED != 0,
        velocity_y: v[1],
        speed: v[0].hypot(v[2]),
        eye_height: s.eye_height,
        crouch_eye_height: s.crouch_eye_height,
        max_step_height: s.max_step_height,
        ramp: s.ramp,
    }
}
/// The first-person camera's eye height: smoothed per tick, interpolated per frame.
#[derive(Clone, Debug)]
pub struct EyeCamera {
    smoother: EyeSmoother,
    previous: Option<f64>,
    current: f64,
    face: FaceFrame,
    /// Across the midplane: the eye's world height then less the new face's eye (m), and the
    /// seconds since; it eases out with the camera's turn.
    swing: Option<(f64, f64)>,
}

impl Default for EyeCamera {
    fn default() -> Self {
        Self {
            smoother: EyeSmoother::new(),
            previous: None,
            current: 0.0,
            face: FaceFrame::A,
            swing: None,
        }
    }
}

impl EyeCamera {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances one sim tick of `dt` seconds with the newly predicted state.
    pub fn tick(&mut self, s: &EyeState, dt: f64) {
        let crossed = s.face != self.face;
        if crossed {
            // Across the midplane: heights along the up restart on the other side.
            self.smoother = EyeSmoother::new();
            self.face = s.face;
        }
        // Along the player's up, back to a world height.
        let fresh = s.face.y(self.smoother.tick(eye_sample_of(s), dt));
        if crossed && self.previous.is_some() {
            // From the eye drawn now (mid-swing, if the body swings back), not a jump.
            self.swing = Some((self.current - fresh, 0.0));
        } else if let Some((offset, t)) = self.swing {
            self.swing = (t + dt < FLIP_SECONDS).then_some((offset, t + dt));
        }
        let eye = fresh
            + self
                .swing
                .map_or(0.0, |(offset, t)| offset * (1.0 - ease(t)));
        self.previous = Some(match self.previous {
            None => eye,
            Some(_) => self.current,
        });
        self.current = eye;
    }

    /// Eye height (world y) drawn `alpha` ∈ [0, 1) of the way from the previous tick to the current.
    pub fn draw(&self, alpha: f64) -> f64 {
        let previous = self.previous.unwrap_or(self.current);
        previous + (self.current - previous) * alpha
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_close;

    const DT: f64 = 1.0 / 60.0;

    fn sample(feet: f64) -> EyeSample {
        EyeSample {
            feet,
            crouched: false,
            grounded: true,
            velocity_y: 0.0,
            speed: WALK,
            eye_height: 1.62,
            crouch_eye_height: 0.8,
            max_step_height: 0.55,
            ramp: StepRamp::default(),
        }
    }

    /// The default walk and run speeds (m/s).
    const WALK: f64 = 5.0;
    const RUN: f64 = 8.0;

    /// Feet heights over a stair of `n` steps of `rise` (signed) and `tread` m, walked at `speed`
    /// from 1 m before the first step: each step is taken in one tick, as the controller does.
    fn stair(n: i32, rise: f64, tread: f64, speed: f64) -> Vec<EyeSample> {
        let ticks = ((f64::from(n) * tread + 3.0) / (speed * DT)) as i32;
        (0..ticks)
            .map(|t| {
                let walked = f64::from(t) * speed * DT - 1.0;
                let steps = if walked < 0.0 {
                    0
                } else {
                    ((walked / tread).floor() as i32 + 1).min(n)
                };
                EyeSample {
                    speed,
                    ..sample(rise * f64::from(steps))
                }
            })
            .collect()
    }

    /// The eye's vertical speed per tick (m/s).
    fn speeds(eyes: &[f64]) -> Vec<f64> {
        eyes.windows(2).map(|w| (w[1] - w[0]) / DT).collect()
    }

    /// Runs `samples` through a smoother; returns the eye per tick.
    fn run(samples: &[EyeSample]) -> Vec<f64> {
        let mut eye = EyeSmoother::new();
        samples.iter().map(|s| eye.tick(*s, DT)).collect()
    }

    fn largest_tick_change(eyes: &[f64]) -> f64 {
        eyes.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(f64::NEG_INFINITY, f64::max)
    }

    fn max(v: &[f64]) -> f64 {
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }

    mod smoother {
        use super::*;

        #[test]
        fn follows_the_feet_exactly_on_flat_ground() {
            let eyes = run(&[sample(0.0), sample(0.0), sample(0.0)]);
            assert_eq!(eyes, vec![1.62, 1.62, 1.62]);
        }

        #[test]
        fn a_lone_step_is_climbed_and_descended_like_a_half_metre_ramp() {
            // Owner, 2026-10-20: "treat steps like half ramps, so smooth transition between
            // them". The body steps in one tick; the eye rises (or falls) at the speed a walker
            // on a ramp of the step view's slope (0.5, a ½ m ramp) would: 2.5 m/s walking, 4 m/s
            // running, from the tick of the step until it is level with the feet, and never
            // faster. Before, every step was a kick: the eye left at 7 m/s and slowed.
            let slope = StepRamp::default().slope;
            for speed in [WALK, RUN] {
                for rise in [0.5, -0.5] {
                    let samples: Vec<_> = std::iter::repeat_n(0.0, 10)
                        .chain(std::iter::repeat_n(rise, 60))
                        .map(|f| EyeSample { speed, ..sample(f) })
                        .collect();
                    let eyes = run(&samples);
                    let ramp = slope * speed;
                    let v = speeds(&eyes);
                    let at = format!("speed {speed} rise {rise}: {v:.2?}");
                    assert!(v.iter().all(|s| s.abs() <= ramp + 1e-9), "{at}");
                    // A constant speed along the ramp, from the step to the top.
                    let climb = (rise.abs() / ramp / DT).floor() as usize;
                    // (The eye moves on the step's own tick, v[9].)
                    for s in &v[9..9 + climb - 1] {
                        assert_close(s * rise.signum(), ramp, 6);
                    }
                    assert_close(eyes[9 + climb + 1], 1.62 + rise, 6);
                }
            }
        }

        #[test]
        fn a_stair_is_climbed_and_descended_like_one_ramp() {
            // The virtual ramp's slope is the stair's own (a step's height over the distance
            // walked since the step before, 0.5 to 1.0), so the eye moves on every tick from the
            // first step to the last, at about the stair's slope × speed, and never more than a
            // step behind the feet. Before, it rose in bursts and stood still between steps.
            for tread in [1.0, 0.75, 0.6] {
                for speed in [WALK, RUN] {
                    for rise in [0.5, -0.5] {
                        let samples = stair(6, rise, tread, speed);
                        let eyes = run(&samples);
                        let v = speeds(&eyes);
                        let at = format!("tread {tread} speed {speed} rise {rise}: {v:.2?}");
                        let stair_speed = (0.5 / tread) * speed;
                        let first = samples.iter().position(|s| s.feet != 0.0).unwrap();
                        let last = samples
                            .iter()
                            .position(|s| s.feet == samples.last().unwrap().feet)
                            .unwrap();
                        for s in &v[first..last] {
                            assert!(s * rise.signum() > 0.35 * stair_speed, "{at}");
                            assert!(s.abs() < 1.6 * stair_speed, "{at}");
                        }
                        for (s, e) in samples.iter().zip(&eyes) {
                            assert!((e - eye_of(s)).abs() <= 0.75, "{at}");
                        }
                        assert_close(*eyes.last().unwrap(), eye_of(samples.last().unwrap()), 6);
                    }
                }
            }
        }

        /// A drawn body's feet per frame (240 Hz) through `follow`; returns the drawn feet.
        fn follow(feet: &[(f64, bool)], speed: f64) -> (Vec<f64>, usize) {
            let dt = DT / 4.0;
            let mut s = StepSmoother::new();
            let mut steps = 0;
            let drawn = feet
                .iter()
                .map(|&(f, grounded)| {
                    let step = s.follow(
                        StepSample {
                            feet: f,
                            grounded,
                            speed,
                            max_step: 0.85,
                            ramp: StepRamp::default(),
                        },
                        dt,
                    );
                    steps += usize::from(step.is_some());
                    f + s.offset()
                })
                .collect();
            (drawn, steps)
        }

        #[test]
        fn a_drawn_body_takes_an_interpolated_step_as_a_ramp_and_follows_slopes_exactly() {
            // A remote player's step-up arrives spread over a snapshot interval (here 50 ms,
            // 12 frames); the drawn body rises at no more than the steepest ramp's speed (45° at
            // walking speed: 5 m/s) and starts one step. A 26.6° slope is followed exactly.
            let ramp = StepRamp::default();
            let step: Vec<_> = (0..120)
                .map(|i| (0.5 * (f64::from(i - 20) / 12.0).clamp(0.0, 1.0), true))
                .collect();
            let (drawn, steps) = follow(&step, WALK);
            assert_eq!(steps, 1);
            let dt = DT / 4.0;
            for w in drawn.windows(2) {
                assert!((w[1] - w[0]) / dt <= ramp.max_slope * WALK + 1e-6);
            }
            assert_close(*drawn.last().unwrap(), 0.5, 6);
            let slope: Vec<_> = (0..120)
                .map(|i| (0.5 * WALK * DT / 4.0 * f64::from(i), true))
                .collect();
            let (drawn, steps) = follow(&slope, WALK);
            assert_eq!(steps, 0);
            for (d, (f, _)) in drawn.iter().zip(&slope) {
                assert_close(*d, *f, 9);
            }
            // Jumping mid-step: the offset decays in the air; the fall itself is drawn as it is.
            let jump: Vec<_> = step[..26]
                .iter()
                .copied()
                .chain((0..60).map(|i| (0.5 + 0.02 * f64::from(i), false)))
                .collect();
            let (drawn, _) = follow(&jump, WALK);
            assert_close(*drawn.last().unwrap(), jump.last().unwrap().0, 9);
        }

        #[test]
        fn a_step_taken_standing_still_settles_at_the_least_speed() {
            let samples: Vec<_> = std::iter::repeat_n(0.0, 5)
                .chain(std::iter::repeat_n(0.5, 40))
                .map(|f| EyeSample {
                    speed: 0.0,
                    ..sample(f)
                })
                .collect();
            let eyes = run(&samples);
            let least = StepRamp::default().min_speed;
            assert!(largest_tick_change(&eyes) <= least * DT + 1e-9);
            assert_close(eyes[5 + (0.5 / (least * DT)) as usize], 2.12, 6);
        }

        #[test]
        fn does_not_smooth_falls_jumps_or_lifts() {
            let fall: Vec<_> = (0..20)
                .map(|i| EyeSample {
                    grounded: false,
                    ..sample(-0.2 * f64::from(i))
                })
                .collect();
            for (e, s) in run(&fall).iter().zip(&fall) {
                assert_close(*e, eye_of(s), 6);
            }
            let lift: Vec<_> = (0..20).map(|i| sample(0.05 * f64::from(i))).collect();
            for (e, s) in run(&lift).iter().zip(&lift) {
                assert_close(*e, eye_of(s), 6);
            }
        }

        #[test]
        fn lowers_the_eye_smoothly_when_crouching_on_the_ground() {
            let crouched = EyeSample {
                crouched: true,
                ..sample(0.0)
            };
            let samples: Vec<_> = std::iter::once(sample(0.0))
                .chain(std::iter::repeat_n(crouched, 40))
                .collect();
            let eyes = run(&samples);
            assert!(largest_tick_change(&eyes) < 0.2);
            assert_close(eyes[1], 1.62, 1);
            assert_close(*eyes.last().unwrap(), 0.8, 2);
        }

        #[test]
        fn crouching_in_mid_air_moves_the_eye_only_continuously() {
            // Falling at 3 m/s; on tick 5 the feet jump up by the height difference.
            let vy = -3.0;
            let samples: Vec<_> = (0..40)
                .map(|i| {
                    let crouched = i >= 5;
                    EyeSample {
                        grounded: false,
                        crouched,
                        velocity_y: vy,
                        ..sample(10.0 + vy * DT * f64::from(i) + if crouched { 0.9 } else { 0.0 })
                    }
                })
                .collect();
            let eyes = run(&samples);
            // Never more than the fall itself plus a little eye settling per tick.
            assert!(largest_tick_change(&eyes) < vy.abs() * DT + 0.02);
            assert_close(*eyes.last().unwrap(), eye_of(samples.last().unwrap()), 2);
        }

        #[test]
        fn a_quick_crouch_tap_dips_and_recovers_without_a_jump() {
            let crouched = EyeSample {
                crouched: true,
                ..sample(0.0)
            };
            let samples: Vec<_> = [sample(0.0), crouched, crouched]
                .into_iter()
                .chain(std::iter::repeat_n(sample(0.0), 40))
                .collect();
            let eyes = run(&samples);
            assert!(largest_tick_change(&eyes) < 0.2);
            assert_close(*eyes.last().unwrap(), 1.62, 2);
        }

        #[test]
        fn a_teleport_is_not_smoothed() {
            let eyes = run(&[
                sample(0.0),
                EyeSample {
                    crouched: true,
                    ..sample(30.0)
                },
            ]);
            assert_close(eyes[1], 30.8, 6);
        }
    }

    // Drawn camera eye over whole client states, 4 frames per tick like a 240 Hz display: the bugs
    // players saw (a snap down each stair, a jolt crouching mid-air, a dip releasing a crouch) were
    // in how the per-frame camera was drawn from the ticks.
    mod camera {
        use super::*;

        const FRAMES: usize = 4;

        #[derive(Clone, Copy)]
        struct Opts {
            crouched: bool,
            grounded: bool,
            vy: f64,
        }
        const STAND: Opts = Opts {
            crouched: false,
            grounded: true,
            vy: 0.0,
        };

        fn state_with(feet: f64, o: Opts) -> EyeState {
            let half = if o.crouched { 0.45 } else { 0.9 };
            EyeState {
                position: [0.0, feet + half, 0.0],
                render_offset: [0.0; 3],
                half_height: half,
                velocity: [0.0, o.vy, WALK],
                controller_flags: (if o.crouched {
                    controller_flags::CROUCHING
                } else {
                    0
                }) | (if o.grounded {
                    controller_flags::GROUNDED
                } else {
                    0
                }),
                face: FaceFrame::A,
                eye_height: 1.62,
                crouch_eye_height: 0.8,
                max_step_height: 0.55,
                ramp: StepRamp::default(),
            }
        }
        fn state(feet: f64) -> EyeState {
            state_with(feet, STAND)
        }

        /// The state turned upside down (face B): heights negated, head down.
        fn mirrored(s: &EyeState) -> EyeState {
            on(FaceFrame::B, s)
        }

        /// A face-A (face-local) state placed on face `f` through its frame.
        fn on(f: FaceFrame, s: &EyeState) -> EyeState {
            if !f.is_b() {
                return *s;
            }
            EyeState {
                position: f.point(s.position),
                render_offset: f.vector(s.render_offset),
                velocity: f.vector(s.velocity),
                controller_flags: s.controller_flags
                    | dwell_physics::net::face_bit(f, controller_flags::FACE_B),
                face: FaceFrame::B,
                ..*s
            }
        }

        /// The eye drawn on every frame while the states are ticked in turn.
        fn frames(states: &[EyeState]) -> Vec<f64> {
            let mut camera = EyeCamera::new();
            let mut out = Vec::new();
            for s in states {
                camera.tick(s, DT);
                for f in 0..FRAMES {
                    out.push(camera.draw(f as f64 / FRAMES as f64));
                }
            }
            out
        }

        fn hold(n: usize, s: EyeState) -> Vec<EyeState> {
            vec![s; n]
        }

        fn cat(parts: &[Vec<EyeState>]) -> Vec<EyeState> {
            parts.concat()
        }

        #[test]
        fn walking_down_a_stair_lowers_the_eye_smoothly() {
            let eyes = frames(&cat(&[hold(5, state(0.5)), hold(40, state(0.0))]));
            assert!(largest_tick_change(&eyes) < 0.05);
            assert_close(*eyes.last().unwrap(), 1.62, 4);
        }

        #[test]
        fn walking_up_a_stair_raises_the_eye_smoothly() {
            let eyes = frames(&cat(&[hold(5, state(0.0)), hold(40, state(0.5))]));
            assert!(largest_tick_change(&eyes) < 0.05);
            assert_close(*eyes.last().unwrap(), 2.12, 4);
        }

        dwell_world::both_faces!(
            /// The virtual ramp on either face (BIFACIAL_WORLD.md §11): a ½ m stair walked on
            /// face `f` draws, face-local, the eye face A draws, and it rises on every frame from
            /// the first step to the last at no more than the steepest ramp's speed.
            fn a_stair_draws_the_virtual_ramp_on_either_face(f: FaceFrame) {
                let local: Vec<EyeState> = (0..60)
                    .map(|t| state(0.5 * f64::from((t / 12).min(4))))
                    .collect();
                let a = frames(&local);
                let here = frames(&local.iter().map(|s| on(f, s)).collect::<Vec<_>>());
                for (h, e) in here.iter().zip(&a) {
                    assert_close(f.y(*h), *e, 6);
                }
                let ramp = StepRamp::default();
                // Face-local per-frame rises, from the first step (tick 12) to the last (48).
                for w in a[12 * FRAMES..48 * FRAMES].windows(2) {
                    let rise = (w[1] - w[0]) / (DT / FRAMES as f64);
                    assert!(rise > 0.0 && rise <= ramp.max_slope * WALK + 1e-6, "{rise}");
                }
            }
        );

        #[test]
        fn crouching_in_mid_air_does_not_jolt_the_eye() {
            let vy = -3.0;
            let states: Vec<_> = (0..40)
                .map(|i| {
                    state_with(
                        10.0 + vy * DT * f64::from(i) + if i >= 5 { 0.9 } else { 0.0 },
                        Opts {
                            grounded: false,
                            crouched: i >= 5,
                            vy,
                        },
                    )
                })
                .collect();
            let eyes = frames(&states);
            // Per frame: the fall itself plus a little eye settling.
            assert!(largest_tick_change(&eyes) < (vy.abs() * DT) / FRAMES as f64 + 0.02);
        }

        #[test]
        fn a_quick_crouch_tap_on_the_ground_dips_and_recovers_without_a_snap() {
            let crouched = Opts {
                crouched: true,
                ..STAND
            };
            let eyes = frames(&cat(&[
                hold(5, state(0.0)),
                hold(2, state_with(0.0, crouched)),
                hold(40, state(0.0)),
            ]));
            assert!(largest_tick_change(&eyes) < 0.05);
            assert!(max(&eyes) < 1.62 + 1e-6);
            assert_close(*eyes.last().unwrap(), 1.62, 2);
        }

        #[test]
        fn a_fall_is_drawn_as_it_happens() {
            let falling = Opts {
                grounded: false,
                ..STAND
            };
            let states: Vec<_> = (0..20)
                .map(|i| state_with(5.0 - 0.1 * f64::from(i), falling))
                .collect();
            let eyes = frames(&states);
            assert_close(
                eyes[eyes.len() - FRAMES],
                eye_of(&sample(5.0 - 0.1 * 18.0)),
                6,
            );
        }

        #[test]
        fn on_face_b_the_eye_is_the_mirror_image() {
            let crouched = Opts {
                crouched: true,
                ..STAND
            };
            let scenarios: Vec<Vec<EyeState>> = vec![
                cat(&[
                    hold(5, state(0.0)),
                    hold(5, state(0.5)),
                    hold(5, state(1.0)),
                ]),
                cat(&[
                    hold(5, state(0.0)),
                    hold(2, state_with(0.0, crouched)),
                    hold(30, state(0.0)),
                ]),
                (0..20)
                    .map(|i| {
                        state_with(
                            5.0 - 0.1 * f64::from(i),
                            Opts {
                                grounded: false,
                                crouched: false,
                                vy: -6.0,
                            },
                        )
                    })
                    .collect(),
            ];
            for states in scenarios {
                let up = frames(&states);
                let down = frames(&states.iter().map(mirrored).collect::<Vec<_>>());
                assert_eq!(down.len(), up.len());
                for (d, u) in down.iter().zip(&up) {
                    assert_close(*d, FaceFrame::B.y(*u), 6);
                }
            }
        }

        // both-faces: crosses from face A to face B.
        #[test]
        fn starts_afresh_across_the_midplane_and_the_eye_swings_across() {
            let mut camera = EyeCamera::new();
            camera.tick(&state(0.0), DT);
            camera.tick(&state(0.5), DT); // a step on face A: smoothed
            let before = camera.draw(1.0);
            camera.tick(&mirrored(&state(0.0)), DT);
            // The first tick on face B is drawn where the eye was: the body turns over, the eye
            // does not jump to the far side of it.
            assert_close(camera.draw(0.0), before, 6);
            assert_close(camera.draw(1.0), before, 6);
            // It swings across, never by more than a body height in a tick, and settles where
            // face B's eye is (the feet's up-height 0 + 1.62 below), the smoothing started afresh.
            let mut last = camera.draw(1.0);
            for _ in 0..(FLIP_SECONDS / DT).ceil() as usize {
                camera.tick(&mirrored(&state(0.0)), DT);
                let now = camera.draw(1.0);
                assert!(now <= last + 1e-9 && last - now < 0.25, "{last} → {now}");
                last = now;
            }
            assert_close(camera.draw(1.0), FaceFrame::B.y(1.62), 6);
        }
    }
}
