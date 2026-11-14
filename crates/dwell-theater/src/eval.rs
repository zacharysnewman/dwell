//! A path evaluated at a path time into a camera pose (THEATER_MODE.md §3.4). Positions are world
//! metres; looks, orbits and offsets are face-local through [`FaceFrame`], so a path written on
//! face A runs mirrored on face B and one that crosses the midplane keeps its world direction.

use dwell_world::FaceFrame;

use crate::path::{Aim, Keyframe, Move, Path, TargetRef};

/// Where the targets a path names are now.
pub trait Targets {
    /// A target's position (world metres; a character's or creature's eye or centre), `None` when
    /// it isn't known (gone, too far): the camera then holds its last aim.
    fn position(&self, target: &TargetRef) -> Option<[f64; 3]>;
}

/// Only fixed points.
pub struct NoTargets;
impl Targets for NoTargets {
    fn position(&self, target: &TargetRef) -> Option<[f64; 3]> {
        match target {
            TargetRef::Point(p) => Some(*p),
            _ => None,
        }
    }
}

/// A camera pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// World metres.
    pub position: [f64; 3],
    /// The look (a world unit vector).
    pub direction: [f64; 3],
    /// The look as `placeCamera` takes it: degrees, face-local to [`Pose::frame`] (the held
    /// `up` face's, else the position's face).
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    /// Vertical field of view (degrees); `None`: the viewer's setting.
    pub fov_deg: Option<f64>,
    /// The time of day to show (0–1); `None`: the world's.
    pub time_of_day: Option<f64>,
    /// The face whose up the camera holds wherever it is (a keyframe's `up`); `None`: the face
    /// the position is over (`"auto"`). From the other face's side it is that face's view, the
    /// world upside down.
    pub up: Option<FaceFrame>,
}

impl Pose {
    /// A pose at `position` looking along the world `direction`.
    pub fn new(position: [f64; 3], direction: [f64; 3]) -> Self {
        let d = normalize(direction).unwrap_or([0.0, 0.0, 1.0]);
        let local = FaceFrame::at_height(position[1]).vector(d);
        let yaw_deg = local[0].atan2(local[2]).to_degrees();
        let pitch_deg = local[1].clamp(-1.0, 1.0).asin().to_degrees();
        Self {
            position,
            direction: d,
            yaw_deg,
            pitch_deg,
            fov_deg: None,
            time_of_day: None,
            up: None,
        }
    }
    /// The frame the camera's up and its yaw and pitch are in.
    pub fn frame(&self) -> FaceFrame {
        self.up
            .unwrap_or_else(|| FaceFrame::at_height(self.position[1]))
    }
    /// The same pose with its up held to `up`'s face (`None`: the position's): the direction is
    /// kept and the yaw and pitch given in that frame.
    pub fn with_up(self, up: Option<FaceFrame>) -> Self {
        let f = up.unwrap_or_else(|| FaceFrame::at_height(self.position[1]));
        let local = f.vector(self.direction);
        Self {
            yaw_deg: local[0].atan2(local[2]).to_degrees(),
            pitch_deg: local[1].clamp(-1.0, 1.0).asin().to_degrees(),
            up,
            ..self
        }
    }
}

/// The segment at path time `t` (clamped to the path) and how far along it is (0–1, uneased).
pub fn segment_at(path: &Path, t: f64) -> (usize, f64) {
    let k = &path.keyframes;
    let t = t.clamp(k[0].t, k[k.len() - 1].t);
    let i = k.partition_point(|kf| kf.t <= t).saturating_sub(1);
    let i = i.min(k.len() - 2);
    let (a, b) = (k[i].t, k[i + 1].t);
    (i, ((t - a) / (b - a)).clamp(0.0, 1.0))
}

/// The playback rate (path seconds per real second) at path time `t`.
pub fn speed_at(path: &Path, t: f64) -> f64 {
    let (i, u) = segment_at(path, t);
    let k = &path.keyframes;
    lerp(k[i].speed, k[i + 1].speed, u)
}

/// The pose at path time `t`, unsmoothed (a tracking move's raw position: [`crate::Player`]
/// smooths it frame by frame).
pub fn pose_at(path: &Path, t: f64, targets: &dyn Targets) -> Pose {
    let (i, u) = segment_at(path, t);
    let (a, b) = (&path.keyframes[i], &path.keyframes[i + 1]);
    let seg = &path.segments[i];
    let e = seg.easing.apply(u);
    let fov = match (a.fov_deg, b.fov_deg) {
        (Some(x), Some(y)) => Some(lerp(x, y, e)),
        (x, _) => x,
    };
    let mut pose = match &seg.mv {
        // A cut holds the first keyframe until the next one's time.
        Move::Cut if u < 1.0 => aimed(a.position, a, targets),
        Move::Cut => aimed(b.position, b, targets),
        Move::Linear | Move::Ease | Move::Pan | Move::Dolly { .. } | Move::Crane { .. } => {
            let p = match &seg.mv {
                Move::Pan => a.position,
                Move::Crane { up_easing } => {
                    let h = lerp3(a.position, b.position, e);
                    let up = up_easing.apply(u);
                    [h[0], lerp(a.position[1], b.position[1], up), h[2]]
                }
                _ => lerp3(a.position, b.position, e),
            };
            let at = match &seg.mv {
                Move::Dolly { at: Some(t), .. } => targets.position(t),
                _ => None,
            };
            let d = match at {
                Some(s) => sub(s, p),
                None => slerp(aim_dir(a, p, targets), aim_dir(b, p, targets), e),
            };
            Pose::new(p, d)
        }
        Move::Orbit { around, degrees } => {
            let c = targets.position(around).unwrap_or(a.position);
            let f = FaceFrame::at_height(c[1]);
            let la = f.vector(sub(a.position, c));
            let lb = f.vector(sub(b.position, c));
            let (ra, rb) = (la[0].hypot(la[2]), lb[0].hypot(lb[2]));
            let ang_a = la[0].atan2(la[2]);
            // To the next keyframe's angle the shortest way, or the way round (whole turns
            // added) nearest `degrees`.
            let base = wrap_pi(lb[0].atan2(lb[2]) - ang_a);
            let sweep = match degrees {
                Some(d) => {
                    let tau = std::f64::consts::TAU;
                    base + ((d.to_radians() - base) / tau).round() * tau
                }
                None => base,
            };
            let ang = ang_a + sweep * e;
            let r = lerp(ra, rb, e);
            let h = lerp(la[1], lb[1], e);
            let p = add(c, f.vector([r * ang.sin(), h, r * ang.cos()]));
            Pose::new(p, sub(c, p))
        }
        Move::Track { target, offset, .. } => match targets.position(target) {
            Some(tp) => {
                let p = add(tp, FaceFrame::at_height(tp[1]).vector(*offset));
                Pose::new(p, sub(tp, p))
            }
            None => aimed(lerp3(a.position, b.position, e), a, targets),
        },
    };
    if let Move::Dolly { vertigo: true, at } = &seg.mv {
        // The dolly zoom: the subject's size held from the first keyframe.
        let subject = at.as_ref().or(match &a.aim {
            Aim::At(t) => Some(t),
            Aim::Look { .. } => None,
        });
        if let (Some(s), Some(f0)) = (subject.and_then(|s| targets.position(s)), a.fov_deg) {
            let (d0, d) = (
                crate::path::dist(a.position, s),
                crate::path::dist(pose.position, s),
            );
            let half = ((f0.to_radians() / 2.0).tan() * d0 / d.max(1e-3)).atan();
            pose.fov_deg = Some((2.0 * half).to_degrees().clamp(5.0, 150.0));
        } else {
            pose.fov_deg = fov;
        }
    } else {
        pose.fov_deg = fov;
    }
    pose.time_of_day = time_of_day(a, b, seg.days, u);
    // The up is the segment's first keyframe's, the next one's from its time (a change of `up`
    // turns the camera over at that keyframe, as crossing the midplane does under "auto").
    pose.with_up(if u >= 1.0 { b.up } else { a.up })
}

/// The time of day between two keyframes: forward from `a`'s to `b`'s (through midnight when `b`'s
/// is earlier) plus `days`, linear in path time; held when only `a` sets it.
fn time_of_day(a: &Keyframe, b: &Keyframe, days: u32, u: f64) -> Option<f64> {
    let (x, y) = (a.time_of_day?, b.time_of_day.unwrap_or(a.time_of_day?));
    let span = (y - x).rem_euclid(1.0) + f64::from(days);
    Some((x + span * u).rem_euclid(1.0))
}

fn aimed(p: [f64; 3], k: &Keyframe, targets: &dyn Targets) -> Pose {
    Pose::new(p, aim_dir(k, p, targets))
}

/// A keyframe's aim from `p` as a world unit vector.
fn aim_dir(k: &Keyframe, p: [f64; 3], targets: &dyn Targets) -> [f64; 3] {
    match &k.aim {
        Aim::Look { yaw_deg, pitch_deg } => k.look_frame().look(*yaw_deg, *pitch_deg),
        Aim::At(t) => targets
            .position(t)
            .and_then(|s| normalize(sub(s, p)))
            .unwrap_or([0.0, 0.0, 1.0]),
    }
}

pub(crate) fn lerp(a: f64, b: f64, u: f64) -> f64 {
    a + (b - a) * u
}
pub(crate) fn lerp3(a: [f64; 3], b: [f64; 3], u: f64) -> [f64; 3] {
    std::array::from_fn(|k| lerp(a[k], b[k], u))
}
pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a[k] + b[k])
}
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn normalize(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(v, v).sqrt();
    (n > 1e-9).then(|| v.map(|c| c / n))
}
fn wrap_pi(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    let a = (a + PI).rem_euclid(TAU) - PI;
    if a <= -PI { a + TAU } else { a }
}
/// Spherical interpolation of two unit vectors (the look turns at an even rate).
fn slerp(a: [f64; 3], b: [f64; 3], u: f64) -> [f64; 3] {
    let c = dot(a, b).clamp(-1.0, 1.0);
    let th = c.acos();
    if th < 1e-6 {
        return normalize(lerp3(a, b, u)).unwrap_or(a);
    }
    if (std::f64::consts::PI - th).abs() < 1e-6 {
        // Opposite looks: turn about the up axis.
        let mid = normalize([-a[2], 0.0, a[0]]).unwrap_or([1.0, 0.0, 0.0]);
        return if u < 0.5 {
            slerp(a, mid, u * 2.0)
        } else {
            slerp(mid, b, u * 2.0 - 1.0)
        };
    }
    let s = th.sin();
    let (wa, wb) = (((1.0 - u) * th).sin() / s, (u * th).sin() / s);
    std::array::from_fn(|k| a[k] * wa + b[k] * wb)
}
