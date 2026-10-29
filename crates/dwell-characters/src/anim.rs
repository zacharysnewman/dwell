//! Clips sampled, layered and cross-faded (rig.mjs `sample`, `layer`, `crossfade`), and a pose's bone
//! transforms (rig.mjs `pivots`).

use crate::math::Affine;
use crate::types::{Character, Clip, Ease, Key, NO_PARENT};

/// Bones a rig may have (the quadruped has 21).
pub const MAX_BONES: usize = 24;

/// A pose: each bone's Euler XYZ (degrees) and the root offset (metres), with which of them it keys
/// (a layer replaces only those).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub rot: [[f32; 3]; MAX_BONES],
    /// Bit per bone.
    pub keyed: u32,
    pub root: [f32; 3],
    pub root_keyed: bool,
}

/// `pose` with `item`'s turns added (data `pose`: a block carried on an upturned palm), each a
/// turn on top of whatever the clips put there.
pub fn with_item(pose: &Pose, item: Option<&crate::Item>) -> Pose {
    let mut out = *pose;
    for &(bone, turn) in item.map_or(&[][..], |i| i.pose) {
        if let Some(r) = out.rot.get_mut(bone) {
            for k in 0..3 {
                r[k] += turn[k];
            }
            out.keyed |= 1 << bone;
        }
    }
    out
}

impl Default for Pose {
    fn default() -> Self {
        Self::REST
    }
}

impl Pose {
    /// Every bone at rest, keying nothing.
    pub const REST: Pose = Pose {
        rot: [[0.0; 3]; MAX_BONES],
        keyed: 0,
        root: [0.0; 3],
        root_keyed: false,
    };
    pub fn keys(&self, bone: usize) -> bool {
        self.keyed & (1 << bone) != 0
    }
    pub fn set(&mut self, bone: usize, r: [f32; 3]) {
        self.rot[bone] = r;
        self.keyed |= 1 << bone;
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], u: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * u)
}

fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    0.5 * (2.0 * p1
        + (-p0 + p2) * u
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u * u * u)
}

/// One channel's value at `t` (rig.mjs `channel`).
fn channel(keys: &[Key], t: f32, clip: &Clip) -> [f32; 3] {
    let n = keys.len();
    if n == 1 {
        return keys[0].v;
    }
    let d = clip.duration;
    let i = keys.iter().rposition(|k| k.t <= t);
    let (a, b, ta, tb) = match i {
        None if !clip.looping => return keys[0].v,
        None => (n - 1, 0, keys[n - 1].t - d, keys[0].t),
        Some(i) if i == n - 1 && !clip.looping => return keys[n - 1].v,
        Some(i) if i == n - 1 => (n - 1, 0, keys[n - 1].t, keys[0].t + d),
        Some(i) => (i, i + 1, keys[i].t, keys[i + 1].t),
    };
    let u = if tb > ta { (t - ta) / (tb - ta) } else { 0.0 };
    let (va, vb) = (keys[a].v, keys[b].v);
    match clip.ease {
        Ease::Step => va,
        Ease::Linear => lerp3(va, vb, u),
        Ease::Smooth => lerp3(va, vb, u * u * (3.0 - 2.0 * u)),
        Ease::Spline => {
            let at = |k: isize| {
                let k = if clip.looping {
                    k.rem_euclid(n as isize)
                } else {
                    k.clamp(0, n as isize - 1)
                };
                keys[k as usize].v
            };
            let p0 = at(a as isize - 1);
            let p3 = at(b as isize + 1);
            std::array::from_fn(|j| catmull(p0[j], va[j], vb[j], p3[j], u))
        }
    }
}

/// A clip's pose at `t` seconds (loops wrap; one-shots hold their ends).
pub fn sample(clip: &Clip, t: f32) -> Pose {
    let d = clip.duration;
    let t = if clip.looping {
        t.rem_euclid(d)
    } else {
        t.clamp(0.0, d)
    };
    let mut p = Pose::REST;
    for c in clip.channels {
        if c.bone < MAX_BONES {
            p.set(c.bone, channel(c.keys, t, clip));
        }
    }
    if !clip.root.is_empty() {
        p.root = channel(clip.root, t, clip);
        p.root_keyed = true;
    }
    p
}

/// Lays `over` on `base` at `weight` (0–1): each bone it keys moves toward its value.
pub fn layer(base: &mut Pose, over: &Pose, weight: f32) {
    for b in 0..MAX_BONES {
        if over.keys(b) {
            base.rot[b] = lerp3(base.rot[b], over.rot[b], weight);
            base.keyed |= 1 << b;
        }
    }
    if over.root_keyed {
        base.root = lerp3(base.root, over.root, weight);
        base.root_keyed = true;
    }
}

/// From `a` to `b` (0 → a, 1 → b) over every bone either keys: a bone one doesn't key is at rest in
/// it, so switching base clips never leaves a bone behind.
pub fn crossfade(a: &Pose, b: &Pose, u: f32) -> Pose {
    let mut p = Pose::REST;
    p.keyed = a.keyed | b.keyed;
    for k in 0..MAX_BONES {
        p.rot[k] = lerp3(a.rot[k], b.rot[k], u);
    }
    p.root = lerp3(a.root, b.root, u);
    p.root_keyed = a.root_keyed || b.root_keyed;
    p
}

/// Every bone's transform in the character's frame (feet at the origin, facing +z) for a pose.
pub fn bone_transforms(ch: &Character, pose: &Pose) -> [Affine; MAX_BONES] {
    let mut out = [Affine::default(); MAX_BONES];
    for (i, b) in ch.bones.iter().enumerate().take(MAX_BONES) {
        let local = if b.parent == NO_PARENT {
            Affine::translation(std::array::from_fn(|k| b.rest[k] + pose.root[k]))
        } else {
            Affine::translation(b.rest)
        }
        .then(&Affine::euler(pose.rot[i]));
        out[i] = if b.parent == NO_PARENT {
            local
        } else {
            out[b.parent].then(&local)
        };
    }
    out
}

/// The first-person arm chain's transforms in view space (+z ahead, +x left, the eye at the origin):
/// the chain's first bone at the anchor plus the pose's root offset (the bob), the rest under it.
/// Bones off the chain are left at identity (nothing of theirs is drawn).
pub fn arm_transforms(ch: &Character, pose: &Pose) -> [Affine; MAX_BONES] {
    let mut out = [Affine::default(); MAX_BONES];
    let Some(fp) = ch.first_person else {
        return out;
    };
    for (n, &i) in fp.bones.iter().enumerate() {
        let b = &ch.bones[i];
        out[i] = if n == 0 {
            Affine::translation(std::array::from_fn(|k| fp.anchor[k] + pose.root[k]))
                .then(&Affine::euler(pose.rot[i]))
        } else {
            out[b.parent].then(&Affine::translation(b.rest).then(&Affine::euler(pose.rot[i])))
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_items_turns_go_on_top_of_the_pose() {
        // Owner, 2026-10-28: a block is carried on the upturned palm; a torch keeps the grip.
        let ch = crate::player();
        let hand = ch.bone("hand_r").expect("a right hand");
        let block = ch.item("block").expect("the block item");
        let torch = ch.item("torch").expect("the torch item");
        let turn = block
            .pose
            .iter()
            .find(|(b, _)| *b == hand)
            .map(|(_, t)| *t)
            .expect("the block turns the hand palm up");
        let mut pose = Pose::REST;
        pose.rot[hand] = [10.0, 0.0, 0.0];
        let held = with_item(&pose, Some(block));
        assert_eq!(held.rot[hand], [10.0 + turn[0], turn[1], turn[2]]);
        assert!(held.keyed & (1 << hand) != 0);
        assert_eq!(
            with_item(&pose, Some(torch)),
            pose,
            "the torch keeps the grip"
        );
        assert_eq!(with_item(&pose, None), pose);
    }
    use crate::TABLES;

    fn clip(name: &str) -> &'static Clip {
        TABLES.clips.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn a_clip_hits_its_keys_and_loops_wrap() {
        let walk = clip("biped_walk");
        let c = walk.channels[0];
        for k in c.keys {
            let p = sample(walk, k.t);
            for j in 0..3 {
                assert!((p.rot[c.bone][j] - k.v[j]).abs() < 1e-3, "{k:?}");
            }
        }
        let a = sample(walk, 0.25);
        let b = sample(walk, 0.25 + walk.duration);
        assert_eq!(a, b);
    }

    #[test]
    fn a_one_shot_holds_its_last_key() {
        let jump = clip("biped_jump");
        assert_eq!(
            sample(jump, jump.duration),
            sample(jump, jump.duration + 3.0)
        );
    }

    #[test]
    fn a_layer_replaces_only_the_bones_it_keys_and_a_crossfade_takes_every_bone() {
        let p = crate::player();
        let mut pose = sample(clip("biped_walk"), 0.3);
        let thigh = p.bone("thigh_l").unwrap();
        let before = pose.rot[thigh];
        let hold = sample(clip("biped_hold"), 0.0);
        assert!(!hold.keys(thigh));
        layer(&mut pose, &hold, 1.0);
        assert_eq!(pose.rot[thigh], before);
        let fore = p.bone("forearm_r").unwrap();
        assert_eq!(pose.rot[fore], hold.rot[fore]);
        // Halfway from the walk to the rest pose: every bone the walk keys at half.
        let walk = sample(clip("biped_walk"), 0.3);
        let half = crossfade(&walk, &Pose::REST, 0.5);
        assert!((half.rot[thigh][0] - walk.rot[thigh][0] / 2.0).abs() < 1e-5);
    }

    #[test]
    fn the_rest_pose_stands_the_player_at_its_height() {
        let p = crate::player();
        let t = bone_transforms(p, &Pose::REST);
        let head = p.bone("head").unwrap();
        // The head's pivot: hips 0.92 + chest 0.12 + head 0.41.
        assert!((t[head].t[1] - 1.45).abs() < 1e-4, "{:?}", t[head].t);
        let arms = arm_transforms(p, &Pose::REST);
        let fp = p.first_person.unwrap();
        assert_eq!(arms[fp.bones[0]].t, fp.anchor);
    }
}
