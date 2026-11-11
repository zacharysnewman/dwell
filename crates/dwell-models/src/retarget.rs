//! A skinned model driven by the characters rig's pose (docs/CHARACTERS.md §11.6, ADR 0070
//! amendment): the same clips, times and blends the rigid body and the first-person arms play
//! (`dwell_characters::animator`), turned onto the model's joints by its retarget map
//! (models.json `retarget`).
//!
//! For a mapped bone `b` on joint `j`: the bone's turn from its rest frame (in the character's
//! frame, `R_b · R_b,rest⁻¹`) is put on the joint's rest frame turned by the map's `rest`
//! correction (`K_j`, the A pose onto the rig's straight-down rest), so `G_j = Y⁻¹ · R_b ·
//! R_b,rest⁻¹ · Y · K_j · G_j,rest` (Y the model's yaw). Joints the map leaves out keep their rest
//! turn under their parent. The root bone's joint takes the pose's root offset, scaled by the two
//! hip heights. Translations otherwise stay at rest: the model keeps its own proportions.

use dwell_characters::Character;
use dwell_characters::anim::{Pose, bone_transforms};

use crate::anim::{self, Trs};
use crate::glb::Model;
use crate::math::{Mat4, Quat};
use crate::{ModelData, Retarget};

/// A rotation as columns.
pub type M3 = [[f32; 3]; 3];

const I3: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..3).map(|k| a[k][r] * b[c][k]).sum()))
}

fn transpose(a: &M3) -> M3 {
    std::array::from_fn(|c| std::array::from_fn(|r| a[r][c]))
}

fn apply(a: &M3, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| a[0][r] * v[0] + a[1][r] * v[1] + a[2][r] * v[2])
}

fn len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// A 4 × 4's rotation (its columns made unit; the skins' scales are uniform) and its scale.
fn rotation(m: &Mat4) -> (M3, f32) {
    let s = len([m[0][0], m[0][1], m[0][2]]).max(1e-9);
    (
        std::array::from_fn(|c| std::array::from_fn(|r| m[c][r] / s)),
        s,
    )
}

fn quat_m3(q: Quat) -> M3 {
    let m = crate::math::trs([0.0; 3], q, [1.0; 3]);
    rotation(&m).0
}

/// A rotation's unit quaternion (x, y, z, w).
fn m3_quat(m: &M3) -> Quat {
    // R[row][col] = m[col][row].
    let r = |row: usize, col: usize| m[col][row];
    let tr = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            0.25 * s,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        [
            (r(0, 1) + r(1, 0)) / s,
            0.25 * s,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            0.25 * s,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
    q.map(|v| v / n)
}

/// A rig bone's rest and posed frames in the character's frame (feet at the origin, facing +z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigBone {
    pub rest: M3,
    pub posed: M3,
}

/// The characters rig's pose as the retarget reads it: each bone's frames by name, the root
/// offset (metres) and the root bone's rest height (metres over the feet).
pub struct RigPose<'a> {
    pub bone: &'a dyn Fn(&str) -> Option<RigBone>,
    pub root: [f32; 3],
    pub hip_m: f32,
}

/// `character`'s pose as a [`RigPose`] (the frames from [`bone_transforms`]), handed to `f`.
pub fn with_rig<T>(
    character: &Character,
    pose: &Pose,
    root_bone: &str,
    f: impl FnOnce(&RigPose) -> T,
) -> T {
    let rest = bone_transforms(character, &Pose::REST);
    let posed = bone_transforms(character, pose);
    let bone = |name: &str| {
        let i = character.bone(name)?;
        Some(RigBone {
            rest: rest[i].m,
            posed: posed[i].m,
        })
    };
    let hip_m = character.bone(root_bone).map_or(1.0, |i| rest[i].t[1]);
    f(&RigPose {
        bone: &bone,
        root: pose.root,
        hip_m,
    })
}

/// Every node's local transform with the map's joints turned to the rig's pose (the rest pose
/// elsewhere): what [`anim::globals`] and [`anim::joint_matrices`] take in place of a clip's pose.
pub fn pose(model: &Model, data: &ModelData, map: &Retarget, rig: &RigPose) -> Vec<Trs> {
    let rest = anim::rest(model);
    let rest_g = anim::globals(model, &rest);
    let n = model.nodes.len();
    let yaw = quat_m3(crate::math::axis_angle([0.0, 1.0, 0.0], data.yaw));
    let yaw_t = transpose(&yaw);
    // Each mapped node's target rotation in the model's space.
    let mut target: Vec<Option<M3>> = vec![None; n];
    for b in map.bones {
        let (Some(&node), Some(r)) = (model.skin.get(b.joint), (rig.bone)(b.bone)) else {
            continue;
        };
        let delta = mul(&r.posed, &transpose(&r.rest));
        let k = dwell_characters::math::euler(b.rest);
        let g_rest = rotation(&rest_g[node]).0;
        target[node] = Some(mul(
            &mul(&mul(&yaw_t, &delta), &yaw),
            &mul(&k, &g_rest),
        ));
    }
    let root_node = map
        .bones
        .iter()
        .find(|b| b.bone == map.root)
        .and_then(|b| model.skin.get(b.joint).copied());
    // Parents before children.
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = model.nodes[i].parent {
            i = p;
            d += 1;
        }
        d
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| depth(i));
    let mut out = rest.clone();
    let mut global: Vec<M3> = vec![I3; n];
    for i in order {
        let parent = model.nodes[i].parent;
        let (pg, ps) = parent.map_or((I3, 1.0), |p| (global[p], rotation(&rest_g[p]).1));
        let local = match target[i] {
            Some(g) => mul(&transpose(&pg), &g),
            None => quat_m3(rest[i].r),
        };
        out[i].r = m3_quat(&local);
        global[i] = mul(&pg, &local);
        if Some(i) == root_node {
            // The root offset: metres in the character's frame, the hips' height ratio, into the
            // parent's units.
            let hip = rest_g[i][3][1] * data.scale;
            let k = if rig.hip_m > 1e-6 { hip / rig.hip_m } else { 1.0 };
            let off = apply(&yaw_t, rig.root.map(|v| v * k / data.scale));
            let local_off = apply(&transpose(&pg), off.map(|v| v / ps));
            for (t, o) in out[i].t.iter_mut().zip(local_off) {
                *t += o;
            }
        }
    }
    out
}

/// [`pose`] from `character`'s `pose` (its frames from [`bone_transforms`]).
pub fn pose_from(
    model: &Model,
    data: &ModelData,
    map: &Retarget,
    character: &Character,
    rig_pose: &Pose,
) -> Vec<Trs> {
    with_rig(character, rig_pose, map.root, |rig| {
        pose(model, data, map, rig)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glb;

    fn close(a: &M3, b: &M3, eps: f32) -> bool {
        (0..3).all(|c| (0..3).all(|r| (a[c][r] - b[c][r]).abs() < eps))
    }

    #[test]
    fn quaternions_round_trip() {
        for q in [
            crate::math::axis_angle([0.0, 1.0, 0.0], 30.0),
            crate::math::axis_angle([0.6, 0.0, 0.8], 170.0),
            crate::math::axis_angle([1.0, 0.0, 0.0], -90.0),
        ] {
            assert!(close(&quat_m3(m3_quat(&quat_m3(q))), &quat_m3(q), 1e-5));
        }
    }

    #[test]
    fn the_rest_pose_retargets_to_the_corrected_rest() {
        // With the rig at rest the joints keep their rest frames turned by `rest` alone.
        let (data, model) = crate::player();
        let map = data.retarget.expect("the Peasant is retargeted");
        let ch = dwell_characters::player();
        let p = pose_from(model, data, &map, ch, &Pose::REST);
        let g = anim::globals(model, &p);
        let r = anim::globals(model, &anim::rest(model));
        for b in map.bones {
            let node = model.skin[b.joint];
            let want = mul(&dwell_characters::math::euler(b.rest), &rotation(&r[node]).0);
            assert!(close(&rotation(&g[node]).0, &want, 1e-4), "{}", b.bone);
        }
    }

    #[test]
    fn every_mapped_bone_is_the_player_characters() {
        let (data, model) = crate::player();
        let map = data.retarget.unwrap();
        let ch = dwell_characters::player();
        assert_eq!(ch.rig, map.rig);
        for b in map.bones {
            assert!(ch.bone(b.bone).is_some(), "{}", b.bone);
            assert!(b.joint < model.skin.len());
        }
    }

    #[test]
    fn a_retarget_map_is_added_by_data_alone() {
        // The fixture's two-bone model maps the rig's hips and right arm (fixtures/models.json).
        let data = crate::find(crate::fixture::MODELS, "two_bone").unwrap();
        let map = data.retarget.expect("the fixture's map");
        assert_eq!(map.bones.len(), 2);
        let model = glb::parse(crate::fixture::PLAYER_GLB).unwrap();
        let ch = dwell_characters::player();
        let arm = ch.bone("arm_r").unwrap();
        let mut rig = Pose::REST;
        rig.set(arm, [-90.0, 0.0, 0.0]);
        let p = pose_from(&model, data, &map, ch, &rig);
        let g = anim::globals(&model, &p);
        // The tip (pointing up at rest, `rest` turning it to hang down) swings forward with the arm.
        let tip = model.skin[1];
        let dir = apply(&rotation(&g[tip]).0, [0.0, 1.0, 0.0]);
        let want = apply(&dwell_characters::math::euler([-90.0, 0.0, 0.0]), [0.0, -1.0, 0.0]);
        for k in 0..3 {
            assert!((dir[k] - want[k]).abs() < 1e-4, "{dir:?} {want:?}");
        }
    }


    /// The tracked limbs: the rig's end bone and its chain's first, and the Peasant's joints.
    const TRACKED: [(&str, &str, &str, &str); 4] = [
        ("hand_l", "arm_l", "hand.L", "upper_arm.L"),
        ("hand_r", "arm_r", "hand.R", "upper_arm.R"),
        ("foot_l", "thigh_l", "foot.L", "thigh.L"),
        ("foot_r", "thigh_r", "foot.R", "thigh.R"),
    ];

    /// The shadow-vs-body alignment as numbers: for each clip at sampled times, the retargeted
    /// Peasant's hands and feet against the rig's. `limb`: the end's direction and reach from its
    /// shoulder or hip, as a share of the limb's length (the pose itself, proportions aside);
    /// `body`: the end's position with the Peasant scaled to the rig by the hips' heights, as a
    /// share of the rig's height (what the proportions add).
    #[test]
    fn the_retargeted_hands_and_feet_track_the_rigs() {
        let (data, model) = crate::player();
        let map = data.retarget.unwrap();
        let ch = dwell_characters::player();
        let rest_t = bone_transforms(ch, &Pose::REST);
        let hips = ch.bone("hips").unwrap();
        let rest_g = anim::globals(model, &anim::rest(model));
        let hip_node = model.skin[map.bones.iter().find(|b| b.bone == "hips").unwrap().joint];
        // Metres per model unit, then the Peasant brought to the rig's size by the hips' heights.
        let k = rest_t[hips].t[1] / (rest_g[hip_node][3][1] * data.scale);
        let node = |n: &str| model.nodes.iter().position(|x| x.name == n).unwrap();
        let at = |g: &[Mat4], n: &str| {
            let m = g[node(n)][3];
            [m[0], m[1], m[2]].map(|v| v * data.scale * k)
        };
        let sub = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| a[i] - b[i]) };
        let (mut limb, mut body) = ((0.0f32, String::new()), (0.0f32, String::new()));
        for role in [
            "idle", "walk", "sprint", "crouch", "crouch_walk", "jump", "fall", "land", "swim",
            "climb", "slide", "fly", "break", "place",
        ] {
            let Some(c) = ch.clip(role) else { continue };
            let clip = dwell_characters::TABLES.clip(c);
            for s in 0..8 {
                let t = clip.duration * s as f32 / 8.0;
                let rig = dwell_characters::anim::sample(clip, t);
                let bt = bone_transforms(ch, &rig);
                let g = anim::globals(model, &pose_from(model, data, &map, ch, &rig));
                let gr = anim::globals(model, &anim::rest(model));
                for (end, first, j_end, j_first) in TRACKED {
                    let (e, f) = (ch.bone(end).unwrap(), ch.bone(first).unwrap());
                    let want = sub(bt[e].t, bt[f].t);
                    let want_len = len(sub(rest_t[e].t, rest_t[f].t));
                    let got = sub(at(&g, j_end), at(&g, j_first));
                    let got_len = len(sub(at(&gr, j_end), at(&gr, j_first)));
                    let miss =
                        len(std::array::from_fn(|i| got[i] / got_len - want[i] / want_len));
                    if miss > limb.0 {
                        limb = (miss, format!("{role} t={t:.2} {end}"));
                    }
                    let miss = len(sub(at(&g, j_end), bt[e].t)) / ch.height;
                    if miss > body.0 {
                        body = (miss, format!("{role} t={t:.2} {end}"));
                    }
                }
            }
        }
        eprintln!(
            "worst limb miss {:.1} % of the limb ({}); worst body miss {:.1} % of the height ({})",
            100.0 * limb.0,
            limb.1,
            100.0 * body.0,
            body.1
        );
        // The pose carries over: each hand and foot within 10 % of its limb's length.
        assert!(limb.0 < 0.10, "{:.3}: {}", limb.0, limb.1);
        // With the Peasant's proportions, within 15 % of the height (its arms are shorter and its
        // shoulders lower than the rig's, so arms overhead miss most).
        assert!(body.0 < 0.15, "{:.3}: {}", body.0, body.1);
    }
}
