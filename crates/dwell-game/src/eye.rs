//! First-person eye height smoothing (PLAYER_CONTROLLER.md §9). The controller moves the body in
//! jumps the camera must not show: onto or down a step in one tick (step-up, ground snap), and by
//! up to a body height when crouching or standing in mid-air (the head stays put, the feet move).
//! Each jump is folded into an offset that then decays, so the eye always moves continuously.
//! [`EyeSmoother`] runs once per sim tick; [`EyeCamera`] feeds it client states and interpolates its
//! last two results for each drawn frame. Heights are measured along the player's up
//! (BIFACIAL_WORLD.md §6): on face B the head points toward −y, so `up height = face × y`; the
//! smoothing is the same either way, and starts afresh when the player crosses to the other face.

use dwell_protocol::controller_flags;

use crate::FaceSign;

/// Step offset decay: a fixed speed (m/s, PPC SmoothSteps) plus a share of the offset (1/s).
const STEP_SPEED: f64 = 4.0;
const STEP_RATE: f64 = 6.0;
/// Crouch offset decay rate (1/s).
const CROUCH_RATE: f64 = 12.0;
/// A grounded per-tick feet change larger than this is a step, not riding a lift or a slope.
const STEP_MIN: f64 = 0.1;
/// A crouch offset larger than this is a teleport (respawn), not a crouch.
const CROUCH_MAX: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeSample {
    /// Feet height along the player's up (drawn position).
    pub feet: f64,
    pub crouched: bool,
    pub grounded: bool,
    /// Vertical velocity along the player's up.
    pub velocity_y: f64,
    pub eye_height: f64,
    pub crouch_eye_height: f64,
    pub max_step_height: f64,
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
    step: f64,
    crouch: f64,
    last: Option<EyeSample>,
}

impl EyeSmoother {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances one tick of `dt` seconds; returns the camera's eye height for that tick.
    pub fn tick(&mut self, s: EyeSample, dt: f64) -> f64 {
        let step_decay = (STEP_SPEED + self.step.abs() * STEP_RATE) * dt;
        self.step = if self.step.abs() <= step_decay {
            0.0
        } else {
            self.step - self.step.signum() * step_decay
        };
        self.crouch *= (-CROUCH_RATE * dt).exp();

        if let Some(last) = &self.last {
            let d_feet = s.feet - last.feet;
            if s.crouched != last.crouched {
                // Everything but the tick's own motion is the crouch.
                self.crouch -= eye_of(&s) - eye_of(last) - s.velocity_y * dt;
            } else if s.grounded
                && last.grounded
                && d_feet.abs() > STEP_MIN
                && d_feet.abs() <= s.max_step_height + 0.05
            {
                self.step -= d_feet;
            }
        }
        if self.step.abs() > s.max_step_height + 0.05 {
            self.step = 0.0;
        }
        if self.crouch.abs() > CROUCH_MAX {
            self.crouch = 0.0;
        }
        self.last = Some(s);
        eye_of(&s) + self.step + self.crouch
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
    pub face: FaceSign,
    pub eye_height: f64,
    pub crouch_eye_height: f64,
    pub max_step_height: f64,
}

/// Feet height from a state's own half height: the capsule centre moves when crouching.
pub fn eye_sample_of(s: &EyeState) -> EyeSample {
    let face = f64::from(s.face);
    EyeSample {
        feet: face * (s.position[1] + s.render_offset[1]) - s.half_height,
        crouched: s.controller_flags & controller_flags::CROUCHING != 0,
        grounded: s.controller_flags & controller_flags::GROUNDED != 0,
        velocity_y: face * s.velocity[1],
        eye_height: s.eye_height,
        crouch_eye_height: s.crouch_eye_height,
        max_step_height: s.max_step_height,
    }
}

/// The first-person camera's eye height: smoothed per tick, interpolated per frame.
#[derive(Clone, Debug)]
pub struct EyeCamera {
    smoother: EyeSmoother,
    previous: Option<f64>,
    current: f64,
    face: FaceSign,
}

impl Default for EyeCamera {
    fn default() -> Self {
        Self {
            smoother: EyeSmoother::new(),
            previous: None,
            current: 0.0,
            face: 1,
        }
    }
}

impl EyeCamera {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances one sim tick of `dt` seconds with the newly predicted state.
    pub fn tick(&mut self, s: &EyeState, dt: f64) {
        if s.face != self.face {
            // Across the midplane: heights along the up restart on the other side.
            self.smoother = EyeSmoother::new();
            self.previous = None;
            self.face = s.face;
        }
        // Along the player's up, back to a world height.
        let eye = f64::from(s.face) * self.smoother.tick(eye_sample_of(s), dt);
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
            eye_height: 1.62,
            crouch_eye_height: 0.8,
            max_step_height: 0.55,
        }
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
        fn smooths_a_step_up_and_a_step_down_alike() {
            for rise in [0.5, -0.5] {
                let samples: Vec<_> = std::iter::repeat_n(0.0, 5)
                    .chain(std::iter::repeat_n(rise, 30))
                    .map(sample)
                    .collect();
                let eyes = run(&samples);
                assert!(largest_tick_change(&eyes) < 0.12);
                assert_close(*eyes.last().unwrap(), 1.62 + rise, 6);
            }
        }

        #[test]
        fn keeps_up_with_running_down_a_staircase() {
            let samples: Vec<_> = (0..60)
                .map(|i| sample(-0.5 * (f64::from(i) / 7.5).floor()))
                .collect();
            let eyes = run(&samples);
            assert!(largest_tick_change(&eyes) < 0.12);
            for (s, e) in samples.iter().zip(&eyes) {
                assert!((e - eye_of(s)).abs() < 0.55);
            }
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
                velocity: [0.0, o.vy, 0.0],
                controller_flags: (if o.crouched {
                    controller_flags::CROUCHING
                } else {
                    0
                }) | (if o.grounded {
                    controller_flags::GROUNDED
                } else {
                    0
                }),
                face: 1,
                eye_height: 1.62,
                crouch_eye_height: 0.8,
                max_step_height: 0.55,
            }
        }
        fn state(feet: f64) -> EyeState {
            state_with(feet, STAND)
        }

        /// The state turned upside down (face B): heights negated, head down.
        fn mirrored(s: &EyeState) -> EyeState {
            EyeState {
                position: [s.position[0], -s.position[1], s.position[2]],
                render_offset: [s.render_offset[0], -s.render_offset[1], s.render_offset[2]],
                velocity: [s.velocity[0], -s.velocity[1], s.velocity[2]],
                controller_flags: s.controller_flags | controller_flags::FACE_B,
                face: -1,
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
                    assert_close(*d, -u, 6);
                }
            }
        }

        #[test]
        fn starts_afresh_across_the_midplane() {
            let mut camera = EyeCamera::new();
            camera.tick(&state(0.0), DT);
            camera.tick(&state(0.5), DT); // a step on face A: smoothed
            camera.tick(&mirrored(&state(0.0)), DT);
            // The first tick on face B is drawn as it is (the eye at the feet's up-height 0 + 1.62 below).
            assert_close(camera.draw(0.0), -1.62, 6);
        }
    }
}
