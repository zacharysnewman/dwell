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

fn dist(a: &Mat4, b: &Mat4) -> f32 {
    (0..3).map(|k| (a[3][k] - b[3][k]).powi(2)).sum::<f32>().sqrt()
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
) -> Option<(Vec<Mat4>, Mat4)> {
    data.first_person?;
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
    let globals = anim::globals(model, &retarget::pose(model, data, &map, &rig));
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
    Some((globals, place))
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
            assert!(!arm.is_empty() && arm.len() % 3 == 0, "level {level}");
            assert!(arm.len() * 5 < all, "level {level}: the arm is a small part of the body");
        }
        // Every arm vertex lies on the right of the body (x < 0 faces +z: the rig's right is −x)
        // and above the waist: nothing of the legs, the chest or the left arm is in it.
        let m = model.level(0);
        let rest = anim::globals(model, &anim::rest(model));
        let shoulder = model.skin[data.hand.unwrap().joint];
        for &v in &arm_triangles(model, 0, &fp) {
            let p = m.positions[v as usize];
            assert!(p[0] * rest[shoulder][3][0] > 0.0, "vertex {v} at {p:?} is on the other side");
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
                let (g, place) = pose(model, data, ch, &p).unwrap();
                let skinned = mm::point(&mm::mul(&place, &g[hand_node]), [0.0; 3]);
                let rigid = arm_transforms(ch, &p)[hand_bone].t;
                let d = (0..3)
                    .map(|k| (skinned[k] - rigid[k]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                worst = worst.max(d);
            }
        }
        assert!(worst < 0.06, "the skinned hand is {worst} m from the rigid hand");
    }
}
