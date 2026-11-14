//! Your first-person arms drawn from the player's skinned model (docs/CHARACTERS.md §5, §11.10;
//! models.json `firstPerson`): the arm's triangles picked from the one body mesh by skin weight,
//! posed from the first-person arms' clips (the rig's `firstPerson.bones` chain in view space,
//! `dwell_characters::anim::arm_transforms`) through the model's retarget map, the arm's shoulder
//! put on the chain's first bone and the arm scaled to the rig's reach.

use dwell_characters::Character;
use dwell_characters::anim::{Pose, arm_transforms, bone_transforms};

use crate::anim;
use crate::glb::Model;
use crate::math::{self as mm, Mat4};
use crate::retarget::{self, RigBone, RigPose};
use crate::{FirstPerson, ModelData};

/// Detail level `level`'s triangles (as indices) whose three vertices each have at least
/// `fp.weight` of their skin weight on `fp.joints`: the arm, cut cleanly where its weights end.
pub fn arm_triangles(model: &Model, level: usize, fp: &FirstPerson) -> Vec<u32> {
    let m = model.level(level);
    let on = |v: u32| {
        let v = v as usize;
        let w: f32 = (0..4)
            .filter(|&k| fp.joints.contains(&usize::from(m.joints[v][k])))
            .map(|k| m.weights[v][k])
            .sum();
        w >= fp.weight
    };
    m.indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&v| on(v)))
        .flatten()
        .copied()
        .collect()
}

/// Your own body in first person (models.json `firstPerson.body`, docs/CHARACTERS.md §5.1, §11.12):
/// level `level`'s triangles less every one with a vertex that has at least `body.weight` of its
/// skin weight on `body.hide` or on the arm's `fp.joints` (the arms view draws it). Empty when the
/// model draws no body.
pub fn body_triangles(model: &Model, level: usize, fp: &FirstPerson) -> Vec<u32> {
    let Some(body) = fp.body else {
        return Vec::new();
    };
    let m = model.level(level);
    let hidden = |v: u32| {
        let v = v as usize;
        let w: f32 = (0..4)
            .filter(|&k| {
                let j = usize::from(m.joints[v][k]);
                body.hide.contains(&j) || fp.joints.contains(&j)
            })
            .map(|k| m.weights[v][k])
            .sum();
        w >= body.weight
    };
    m.indices
        .chunks_exact(3)
        .filter(|t| !t.iter().any(|&v| hidden(v)))
        .flatten()
        .copied()
        .collect()
}

/// The vertices `tris` use, each once, in order.
pub fn vertices_of(tris: &[u32]) -> Vec<u32> {
    let mut v = tris.to_vec();
    v.sort_unstable();
    v.dedup();
    v
}

/// Level `level`'s vertices `verts` skinned by `joints` (each skin joint's matrix: its global
/// times its inverse bind, in whatever space the caller wants them).
pub fn skin_points(model: &Model, level: usize, joints: &[Mat4], verts: &[u32]) -> Vec<[f32; 3]> {
    let m = model.level(level);
    verts
        .iter()
        .map(|&v| {
            let v = v as usize;
            let mut p = [0.0f32; 3];
            for k in 0..4 {
                let w = m.weights[v][k];
                if w > 0.0
                    && let Some(j) = joints.get(usize::from(m.joints[v][k]))
                {
                    let q = mm::point(j, m.positions[v]);
                    for c in 0..3 {
                        p[c] += q[c] * w;
                    }
                }
            }
            p
        })
        .collect()
}

fn dist(a: &Mat4, b: &Mat4) -> f32 {
    (0..3)
        .map(|k| (a[3][k] - b[3][k]).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// The arms posed by `pose` (a first-person clip's, with the held item's turns): every node's
/// model-space transform ([`anim::globals`]) and the matrix from model space to view space (+x
/// left, +y up, +z ahead, metres from the eye). None: the model draws no first-person arms, or the
/// character has none.
pub fn pose(
    model: &Model,
    data: &ModelData,
    character: &Character,
    pose: &Pose,
    item: Option<&str>,
) -> Option<(Vec<Mat4>, Mat4)> {
    let fp_data = data.first_person?;
    let map = data.retarget?;
    let chain = character.first_person?.bones;
    let (&first, &last) = (chain.first()?, chain.last()?);
    let rest = bone_transforms(character, &Pose::REST);
    let posed = arm_transforms(character, pose);
    // Only the chain's bones turn their joints: the view has no body (the shoulder, the spine
    // keep their rest).
    let bone = |name: &str| {
        let i = character.bone(name).filter(|i| chain.contains(i))?;
        Some(RigBone {
            rest: rest[i].m,
            posed: posed[i].m,
        })
    };
    let rig = RigPose {
        bone: &bone,
        root: [0.0; 3],
        hip_m: 1.0,
    };
    let mut local = retarget::pose(model, data, &map, &rig);
    // The hand closed on what it holds (models.json `grips`).
    if let Some(g) = item.and_then(|i| data.grip(i)) {
        crate::grip::apply(model, g, &mut local);
    }
    let globals = anim::globals(model, &local);
    let node_of = |b: usize| {
        let name = character.bones[b].name;
        let j = map.bones.iter().find(|m| m.bone == name)?.joint;
        model.skin.get(j).copied()
    };
    let (shoulder, hand) = (node_of(first)?, node_of(last)?);
    // The arm scaled so its rest reach (shoulder to hand) is the rig's: the hand, and the item
    // in it, land where the clips were fitted (clip_probe), whatever the model's proportions.
    let rest_g = anim::globals(model, &anim::rest(model));
    let reach_model = dist(&rest_g[shoulder], &rest_g[hand]) * data.scale;
    let reach_rig = (0..3)
        .map(|k| (rest[first].t[k] - rest[last].t[k]).powi(2))
        .sum::<f32>()
        .sqrt();
    let k = if reach_model > 1e-6 {
        reach_rig / reach_model
    } else {
        1.0
    };
    let s = data.scale * k;
    let at = posed[first].t;
    let p = globals[shoulder][3];
    let turn = mm::axis_angle([0.0, 1.0, 0.0], data.yaw);
    let place = mm::mul(
        &mm::trs(at, turn, [s; 3]),
        &mm::trs([-p[0], -p[1], -p[2]], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
    );
    // The whole arm turned about the rig's hand (`turn`): the elbow and the sleeve up into view.
    let hand_at = posed[last].t;
    let e = dwell_characters::math::euler(fp_data.turn);
    let turn: Mat4 = [
        [e[0][0], e[0][1], e[0][2], 0.0],
        [e[1][0], e[1][1], e[1][2], 0.0],
        [e[2][0], e[2][1], e[2][2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let about = mm::mul(
        &mm::trs(hand_at, [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
        &mm::mul(
            &turn,
            &mm::trs(hand_at.map(|v| -v), [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
        ),
    );
    Some((globals, mm::mul(&about, &place)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peasants_arm_is_cut_from_its_body_by_skin_weight() {
        let (data, model) = crate::player();
        let fp = data.first_person.expect("the Peasant draws your arms");
        for level in 0..model.level_count() {
            let all = model.level(level).indices.len();
            let arm = arm_triangles(model, level, &fp);
            assert!(
                !arm.is_empty() && arm.len().is_multiple_of(3),
                "level {level}"
            );
            assert!(
                arm.len() * 5 < all,
                "level {level}: the arm is a small part of the body"
            );
        }
        // Every arm vertex lies on the right of the body (x < 0 faces +z: the rig's right is −x)
        // and above the waist: nothing of the legs, the chest or the left arm is in it.
        let m = model.level(0);
        let rest = anim::globals(model, &anim::rest(model));
        let shoulder = model.skin[data.hand.unwrap().joint];
        for &v in &arm_triangles(model, 0, &fp) {
            let p = m.positions[v as usize];
            assert!(
                p[0] * rest[shoulder][3][0] > 0.0,
                "vertex {v} at {p:?} is on the other side"
            );
        }
    }

    /// The share of vertex `v`'s skin weight on `joints`.
    fn share(m: &crate::glb::View<'_>, v: u32, joints: &[usize]) -> f32 {
        let v = v as usize;
        (0..4)
            .filter(|&k| joints.contains(&usize::from(m.joints[v][k])))
            .map(|k| m.weights[v][k])
            .sum()
    }

    #[test]
    fn your_body_is_the_peasant_less_its_head_and_arms_by_skin_weight() {
        let (data, model) = crate::player();
        let fp = data.first_person.expect("the Peasant draws your arms");
        let body = fp.body.expect("and your own body");
        let joint = |name: &str| {
            let b = data.retarget.unwrap().bones.iter().find(|b| b.bone == name);
            b.unwrap().joint
        };
        let (head, arm_l) = (joint("head"), joint("arm_l"));
        assert!(body.hide.contains(&head) && body.hide.contains(&arm_l));
        for level in 0..model.level_count() {
            let m = model.level(level);
            let tris = body_triangles(model, level, &fp);
            // A third of the mesh: the head and hat are 40 % of it.
            assert!(
                tris.len().is_multiple_of(3) && tris.len() * 4 > m.indices.len(),
                "level {level}: {} of {}",
                tris.len(),
                m.indices.len()
            );
            // No vertex of it is the head's, the left arm's or the first-person arm's.
            for &v in &tris {
                for (what, joints) in [
                    ("head", &[head][..]),
                    ("left arm", &[arm_l]),
                    ("arm", fp.joints),
                ] {
                    assert!(
                        share(&m, v, joints) < body.weight,
                        "level {level}: a {what} vertex {v}"
                    );
                }
            }
            // The arms view's arm and the body share no triangle: never two arms.
            let arm = arm_triangles(model, level, &fp);
            let set: std::collections::HashSet<[u32; 3]> =
                tris.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
            assert!(
                arm.chunks_exact(3)
                    .all(|t| !set.contains(&[t[0], t[1], t[2]]))
            );
            // The legs and feet are all in it.
            let feet = [
                joint("foot_l"),
                joint("foot_r"),
                joint("shin_l"),
                joint("shin_r"),
            ];
            let leg_tris = m
                .indices
                .chunks_exact(3)
                .filter(|t| t.iter().all(|&v| share(&m, v, &feet) > 0.99))
                .count();
            let kept = tris
                .chunks_exact(3)
                .filter(|t| t.iter().all(|&v| share(&m, v, &feet) > 0.99))
                .count();
            assert!(
                leg_tris > 0 && kept == leg_tris,
                "level {level}: {kept} of {leg_tris} leg triangles"
            );
        }
        // Rest pose: nothing kept is above the shoulders (the head and its hat are out).
        let m = model.level(0);
        let rest = anim::globals(model, &anim::rest(model));
        let neck = rest[model.skin[joint("head")]][3][1];
        for &v in &body_triangles(model, 0, &fp) {
            assert!(
                m.positions[v as usize][1] < neck,
                "vertex {v} above the neck"
            );
        }
    }

    /// The fixture's two-joint model draws a body by data alone: everything off its arm joint.
    #[test]
    fn a_fixture_models_body_is_cut_by_data_alone() {
        let data = crate::fixture::MODELS
            .iter()
            .find(|m| m.name == "two_bone")
            .unwrap();
        let model = crate::glb::parse(crate::fixture::PLAYER_GLB).unwrap();
        let fp = data
            .first_person
            .expect("the fixture names a first-person arm and body");
        assert!(fp.body.is_some_and(|b| b.hide.is_empty()));
        let m = model.level(0);
        let tris = body_triangles(&model, 0, &fp);
        assert!(!tris.is_empty() && tris.len() < m.indices.len());
        let arm = fp.joints;
        for t in m.indices.chunks_exact(3) {
            let off = t.iter().all(|&v| share(&m, v, arm) < 0.5);
            let kept = tris.chunks_exact(3).any(|k| k == t);
            assert_eq!(off, kept, "triangle {t:?}");
        }
        // Skinned at rest, the kept vertices are where the mesh has them.
        let rest = anim::globals(&model, &anim::rest(&model));
        let joints: Vec<Mat4> = model
            .skin
            .iter()
            .zip(&model.inverse_bind)
            .map(|(&j, ib)| mm::mul(&rest[j], ib))
            .collect();
        let verts = vertices_of(&tris);
        for (p, &v) in skin_points(&model, 0, &joints, &verts).iter().zip(&verts) {
            let q = m.positions[v as usize];
            assert!((0..3).all(|k| (p[k] - q[k]).abs() < 1e-4), "{p:?} vs {q:?}");
        }
    }

    #[test]
    fn the_skinned_hand_follows_the_rigid_arms_hand_through_the_swings() {
        // The arm keeps the rig's reach: the Peasant's hand.R stays near the rigid hand bone (the
        // frame the held items and clip_probe's fits are in) in every first-person clip.
        let (data, model) = crate::player();
        let ch = dwell_characters::player();
        let hand_bone = ch.bone("hand_r").unwrap();
        let hand_node = model.skin[data.hand.unwrap().joint];
        let fp = ch.first_person.unwrap();
        let mut worst = 0.0f32;
        for &(_, clip) in fp.clips {
            let c = dwell_characters::TABLES.clip(clip);
            for n in 0..=8 {
                let t = c.duration * n as f32 / 8.0;
                let p = dwell_characters::anim::sample(c, t);
                let (g, place) = pose(model, data, ch, &p, None).unwrap();
                let skinned = mm::point(&mm::mul(&place, &g[hand_node]), [0.0; 3]);
                let rigid = arm_transforms(ch, &p)[hand_bone].t;
                let d = (0..3)
                    .map(|k| (skinned[k] - rigid[k]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                worst = worst.max(d);
            }
        }
        assert!(
            worst < 0.06,
            "the skinned hand is {worst} m from the rigid hand"
        );
    }
}
