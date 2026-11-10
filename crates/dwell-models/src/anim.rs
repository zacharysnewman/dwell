//! Clips on a skinned model (docs/CHARACTERS.md §11.3): sampling a clip into each node's local
//! transform, blending two poses, the joint matrices the skinning shader takes, and the [`Player`]
//! that turns a role into its clip and cross-fades when the role changes.

use crate::ModelData;
use crate::glb::{Clip, Model, Path};
use crate::math::{self, Mat4, Quat};

/// Seconds a change of role cross-fades over.
pub const BLEND_S: f32 = 0.2;

/// A node's local transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub t: [f32; 3],
    pub r: Quat,
    pub s: [f32; 3],
}

/// Every node at rest (its own transform in the file).
pub fn rest(model: &Model) -> Vec<Trs> {
    model
        .nodes
        .iter()
        .map(|n| Trs {
            t: n.t,
            r: n.r,
            s: n.s,
        })
        .collect()
}

/// The keys around `time` and how far between them (0..1; step keys hold the earlier).
fn keys(times: &[f32], time: f32, linear: bool) -> (usize, usize, f32) {
    let last = times.len().saturating_sub(1);
    if times.is_empty() || time <= times[0] {
        return (0, 0, 0.0);
    }
    if time >= times[last] {
        return (last, last, 0.0);
    }
    let i = times.partition_point(|&t| t <= time).saturating_sub(1);
    let j = (i + 1).min(last);
    let span = times[j] - times[i];
    let f = if linear && span > 0.0 {
        (time - times[i]) / span
    } else {
        0.0
    };
    (i, j, f)
}

/// `clip` at `time` (seconds, held at its ends) over the rest pose.
pub fn sample(model: &Model, clip: &Clip, time: f32) -> Vec<Trs> {
    let mut out = rest(model);
    for c in &clip.channels {
        let Some(n) = out.get_mut(c.node) else {
            continue;
        };
        let (i, j, f) = keys(&c.times, time, c.linear);
        match c.path {
            Path::Rotation => {
                let at = |k: usize| -> Quat { std::array::from_fn(|q| c.values[k * 4 + q]) };
                if c.values.len() >= (j + 1) * 4 {
                    n.r = math::nlerp(at(i), at(j), f);
                }
            }
            Path::Translation | Path::Scale => {
                let at = |k: usize| -> [f32; 3] { std::array::from_fn(|q| c.values[k * 3 + q]) };
                if c.values.len() >= (j + 1) * 3 {
                    let v = math::lerp3(at(i), at(j), f);
                    if c.path == Path::Translation {
                        n.t = v;
                    } else {
                        n.s = v;
                    }
                }
            }
        }
    }
    out
}

/// `a` blended toward `b` by `w` (0 all `a`, 1 all `b`).
pub fn blend(a: &[Trs], b: &[Trs], w: f32) -> Vec<Trs> {
    a.iter()
        .zip(b)
        .map(|(a, b)| Trs {
            t: math::lerp3(a.t, b.t, w),
            r: math::nlerp(a.r, b.r, w),
            s: math::lerp3(a.s, b.s, w),
        })
        .collect()
}

/// Each node's transform to the model's space (through its parents).
pub fn globals(model: &Model, pose: &[Trs]) -> Vec<Mat4> {
    let mut out: Vec<Option<Mat4>> = vec![None; model.nodes.len()];
    fn at(model: &Model, pose: &[Trs], out: &mut [Option<Mat4>], i: usize) -> Mat4 {
        if let Some(m) = out[i] {
            return m;
        }
        let p = pose[i];
        let local = math::trs(p.t, p.r, p.s);
        let m = match model.nodes[i].parent {
            Some(parent) => math::mul(&at(model, pose, out, parent), &local),
            None => local,
        };
        out[i] = Some(m);
        m
    }
    (0..model.nodes.len())
        .map(|i| at(model, pose, &mut out, i))
        .collect()
}

/// The skinning matrices: each joint's bind space to the model's space under `pose`.
pub fn joint_matrices(model: &Model, pose: &[Trs]) -> Vec<Mat4> {
    let g = globals(model, pose);
    model
        .skin
        .iter()
        .zip(&model.inverse_bind)
        .map(|(&j, ib)| math::mul(&g[j], ib))
        .collect()
}

/// Vertex `v` skinned by `joints` (what the vertex shader does: the weighted sum of its four
/// joints' matrices).
pub fn skin_point(model: &Model, joints: &[Mat4], v: usize) -> [f32; 3] {
    let mut out = [0.0; 3];
    for k in 0..4 {
        let w = model.weights[v][k];
        if w == 0.0 {
            continue;
        }
        let p = math::point(&joints[usize::from(model.joints[v][k])], model.positions[v]);
        for (o, p) in out.iter_mut().zip(p) {
            *o += p * w;
        }
    }
    out
}

/// One clip playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playing {
    pub role: &'static str,
    pub clip: usize,
    pub time: f32,
    pub speed: f32,
}

/// A model's role → clip player (docs/CHARACTERS.md §11.3): the role's clip (or the model's
/// fallback's) at its speed, and when the clip changes the old one fading out over [`BLEND_S`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub now: Playing,
    /// The clip fading out and how much of the fade is left (seconds).
    pub fading: Option<(Playing, f32)>,
}

impl Player {
    pub fn new(data: &ModelData, model: &Model, role: &'static str) -> Self {
        Self {
            now: Self::playing(data, model, role),
            fading: None,
        }
    }

    fn playing(data: &ModelData, model: &Model, role: &'static str) -> Playing {
        let rc = data.role(role);
        Playing {
            role,
            clip: model.clip(rc.clip).unwrap_or(0),
            time: 0.0,
            speed: rc.speed,
        }
    }

    /// Advances `dt` seconds playing `role`.
    pub fn update(&mut self, data: &ModelData, model: &Model, role: &'static str, dt: f32) {
        if role != self.now.role {
            let next = Self::playing(data, model, role);
            if next.clip == self.now.clip {
                // The same clip at another speed: keep its phase.
                self.now = Playing {
                    time: self.now.time,
                    ..next
                };
            } else {
                self.fading = Some((self.now, BLEND_S));
                self.now = next;
            }
        }
        self.now.time += dt * self.now.speed;
        if let Some((p, left)) = &mut self.fading {
            p.time += dt * p.speed;
            *left -= dt;
            if *left <= 0.0 {
                self.fading = None;
            }
        }
    }

    /// The share of the clip playing now (1 once the fade is done).
    pub fn weight(&self) -> f32 {
        self.fading
            .map_or(1.0, |(_, left)| (1.0 - left / BLEND_S).clamp(0.0, 1.0))
    }

    /// The pose this moment.
    pub fn pose(&self, model: &Model) -> Vec<Trs> {
        let clip = |p: &Playing| model.clips.get(p.clip);
        // Clips loop.
        let at = |c: &Clip, t: f32| sample(model, c, t.rem_euclid(c.duration.max(1e-3)));
        let now = clip(&self.now).map_or_else(|| rest(model), |c| at(c, self.now.time));
        match self.fading.and_then(|(p, _)| clip(&p).map(|c| (p, c))) {
            Some((p, c)) => blend(&at(c, p.time), &now, self.weight()),
            None => now,
        }
    }
}
