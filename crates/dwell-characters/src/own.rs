//! Your own body in first person (CHARACTERS.md §5.1): the full body from the neck down, playing the
//! clips others see, drawn in the scene under the arms view. The data (`firstPerson.body`) names the
//! parts it leaves out (the head and its hat, the neck, both arms: the arms view draws yours) and how
//! it keeps clear of the eye: `back` behind it, `look_down` more looking down, and further back
//! whenever a vertex would come within `clear` of the eye. All of it is in the body's own frame
//! (+y up, +z ahead, the feet at the origin), so it is the same on both faces.

use crate::anim::MAX_BONES;
use crate::math::Affine;
use crate::mesh::{self, Mesh};
use crate::types::{FirstPersonBody, Part, View};

/// Whether your own body draws a part.
pub fn drawn(body: &FirstPersonBody, p: &Part) -> bool {
    p.view != View::Arms && !body.hide_bones.contains(&p.bone) && !body.hide_parts.contains(&p.id)
}

/// The mesh of `parts` as your own body draws it.
pub fn build(parts: &[Part], body: &FirstPersonBody) -> Mesh {
    let kept: Vec<Part> = parts.iter().copied().filter(|p| drawn(body, p)).collect();
    mesh::build(&kept, View::Body, &[])
}

/// How far back (m, along the body's −z) to draw your body this frame: `back`, plus `look_down` × the
/// sine of how far down you look, plus whatever more keeps every vertex of `mesh` (posed by `bones`,
/// in the body's frame) `clear` of `eye` (the eye in the body's frame). `pitch` is degrees, up
/// positive.
pub fn shift(
    body: &FirstPersonBody,
    mesh: &Mesh,
    bones: &[Affine; MAX_BONES],
    eye: [f32; 3],
    pitch: f32,
) -> f32 {
    let down = (-pitch.to_radians().sin()).max(0.0);
    let mut s = body.back + body.look_down * down;
    let c2 = body.clear * body.clear;
    for (p, &b) in mesh.positions.iter().zip(&mesh.bones) {
        let v = bones[usize::from(b)].apply(*p);
        let (dx, dy, dz) = (v[0] - eye[0], v[1] - eye[1], v[2] - eye[2]);
        let r2 = dx * dx + dy * dy;
        if r2 < c2 {
            // Moved back by s, the vertex is at dz − s along z: clear once dz − s ≤ −√(c² − r²).
            s = s.max(dz + (c2 - r2).sqrt());
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Pose, bone_transforms};
    use crate::{DEFAULT_LOOK, FIXTURE, TABLES, player};

    fn body() -> FirstPersonBody {
        player()
            .first_person
            .and_then(|f| f.body)
            .expect("the player draws its own body")
    }

    #[test]
    fn your_body_leaves_out_the_head_the_neck_and_the_arms() {
        let ch = player();
        let b = body();
        for l in ch.looks {
            let m = build(l.parts, &b);
            assert!(!m.parts.is_empty());
            for name in ["head", "arm_l", "forearm_l", "hand_l", "arm_r", "forearm_r", "hand_r"] {
                let bone = ch.bone(name).unwrap() as u8;
                assert!(!m.bones.contains(&bone), "{}: {name}", l.name);
            }
            for id in ["neck", "head", "hat_brim", "hood_brow", "upper_arm_l", "knuckles_l"] {
                assert!(!m.parts.contains(&id), "{}: {id}", l.name);
            }
            for id in ["torso", "thigh_l", "foot_r", "belt"] {
                assert!(m.parts.contains(&id), "{}: {id}", l.name);
            }
        }
    }

    #[test]
    fn the_shift_is_the_back_offset_more_looking_down_and_more_when_a_vertex_is_near() {
        let ch = player();
        let b = body();
        let m = build(ch.look(DEFAULT_LOOK).unwrap().parts, &b);
        let bones = bone_transforms(ch, &Pose::REST);
        // An eye high above the body: only the offsets.
        let high = [0.0, 5.0, 0.0];
        assert!((shift(&b, &m, &bones, high, 0.0) - b.back).abs() < 1e-6);
        assert!((shift(&b, &m, &bones, high, 30.0) - b.back).abs() < 1e-6);
        assert!((shift(&b, &m, &bones, high, -90.0) - (b.back + b.look_down)).abs() < 1e-5);
        // An eye at the chest: moved back until every vertex is clear.
        let chest = [0.0, 1.2, 0.0];
        let s = shift(&b, &m, &bones, chest, 0.0);
        assert!(s > b.back + 0.1, "{s}");
        for (p, &k) in m.positions.iter().zip(&m.bones) {
            let v = bones[usize::from(k)].apply(*p);
            let d = (v[0] - chest[0]).hypot(v[1] - chest[1]).hypot(v[2] - s - chest[2]);
            assert!(d >= b.clear - 1e-4, "{d}");
        }
    }

    /// The fixture's pauldron (a part on the arm, added by data alone) is hidden with the arm.
    #[test]
    fn a_part_added_by_data_on_a_hidden_bone_is_hidden() {
        let ch = FIXTURE.character(crate::PLAYER).unwrap();
        let b = ch.first_person.unwrap().body.unwrap();
        let parts = ch.look(DEFAULT_LOOK).unwrap().parts;
        assert!(parts.iter().any(|p| p.id == "test_pauldron_l"));
        assert!(!build(parts, &b).parts.contains(&"test_pauldron_l"));
        let _ = TABLES;
    }
}
