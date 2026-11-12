//! Playing a path frame by frame (THEATER_MODE.md §3.5): path time advanced by the real frame time
//! times the keyframes' speed, and a tracking move's position smoothed exponentially, so the same
//! path plays the same at 30 or 144 frames a second.

use crate::eval::{Pose, Targets, pose_at, segment_at, speed_at, sub};
use crate::path::{Move, Path};

/// A path being played.
#[derive(Clone, Debug)]
pub struct Player {
    pub path: Path,
    /// Path seconds from the first keyframe's time.
    pub time: f64,
    pub playing: bool,
    /// The last pose given (where a tracking move eases from) and the raw pose it chased.
    last: Option<(Pose, Pose)>,
}

impl Player {
    pub fn new(path: Path) -> Self {
        Self {
            path,
            time: 0.0,
            playing: true,
            last: None,
        }
    }
    /// Whether the path has played to its end (never, looped).
    pub fn finished(&self) -> bool {
        !self.path.looped && self.time >= self.path.duration()
    }
    /// Jumps to path time `t` (seconds from the start); the next step's tracking starts afresh.
    pub fn seek(&mut self, t: f64) {
        self.time = t.clamp(0.0, self.path.duration());
        self.last = None;
    }

    /// Advances by `dt` real seconds (0 while paused) and gives the pose to draw.
    pub fn step(&mut self, dt: f64, targets: &dyn Targets) -> Pose {
        let dur = self.path.duration();
        let t0 = self.path.keyframes[0].t;
        let dt = if self.playing {
            dt.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.last.is_none() {
            // Start from the pose now; a track at its segment's start eases in from the keyframe.
            let t = t0 + self.time;
            let raw = pose_at(&self.path, t, targets);
            let (i, u) = segment_at(&self.path, t);
            let mut from = raw;
            if matches!(self.path.segments[i].mv, Move::Track { .. }) && u == 0.0 {
                from.position = self.path.keyframes[i].position;
            }
            self.last = Some((from, raw));
        }
        if dt > 0.0 {
            // Midpoint step of dτ/dt = speed(τ) (linear in τ between keyframes).
            let s0 = speed_at(&self.path, t0 + self.time);
            let mid = speed_at(&self.path, t0 + self.time + s0 * dt / 2.0);
            self.time += mid * dt;
            if self.time >= dur {
                if self.path.looped && dur > 0.0 {
                    self.time = self.time.rem_euclid(dur);
                } else {
                    self.time = dur;
                }
            }
        }
        let t = t0 + self.time;
        let raw = pose_at(&self.path, t, targets);
        let (i, _) = segment_at(&self.path, t);
        let pose = match (&self.path.segments[i].mv, self.last) {
            (
                Move::Track {
                    target,
                    smoothing_s,
                    ..
                },
                Some((last, last_raw)),
            ) if *smoothing_s > 0.0 && dt > 0.0 => {
                // Exponential smoothing, solved exactly for a goal moving straight through the
                // frame (from the last raw pose to this one): the same lag at every frame rate.
                let a = (-dt / smoothing_s).exp();
                let ramp = smoothing_s / dt * (1.0 - a);
                let p: [f64; 3] = std::array::from_fn(|k| {
                    let (y0, y1) = (last_raw.position[k], raw.position[k]);
                    y1 + (last.position[k] - y0) * a - (y1 - y0) * ramp
                });
                let aim = targets
                    .position(target)
                    .map_or(raw.direction, |tp| sub(tp, p));
                Pose {
                    fov_deg: raw.fov_deg,
                    time_of_day: raw.time_of_day,
                    ..Pose::new(p, aim)
                }
            }
            (Move::Track { .. }, Some((last, _))) if dt == 0.0 => last,
            _ => raw,
        };
        self.last = Some((pose, raw));
        pose
    }
}
