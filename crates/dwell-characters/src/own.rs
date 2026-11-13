//! Your own body in first person (CHARACTERS.md §5.1): the full body from the neck down, playing the
//! clips others see, drawn in the scene under the arms view. The data (`firstPerson.body`) names the
//! parts it leaves out (the head and its hat, the neck, both arms: the arms view draws yours) and how
//! it keeps clear of the eye: leaned back from its soles (`lean`), `back` behind it at the eye's
//! height, `look_down` more looking down, and further whenever a vertex would come within `clear` of
//! the eye. The soles stay where they stand, on the ground and on their shadow (owner, 2026-11-13:
//! the feet "seemed to be floating" when the whole body was moved back off them). All of it is in
//! the body's own frame (+y up, +z ahead, the feet at the origin), so it is the same on both faces.

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

/// How far back (m, along the body's −z, at the eye's height) your body is drawn this frame: the
/// second of [`keep_clear`] over `mesh` posed by `bones` (in the body's frame). `pitch` is degrees,
/// up positive.
pub fn shift(
    body: &FirstPersonBody,
    mesh: &Mesh,
    bones: &[Affine; MAX_BONES],
    eye: [f32; 3],
    pitch: f32,
) -> f32 {
    keep_clear(body, posed(mesh, bones), eye, pitch, "idle").1
}

/// Your own body's place in its frame this frame, and how far back that is at the eye's height:
/// [`keep_clear`] over `mesh` posed by `bones`, playing `role`.
pub fn place(
    body: &FirstPersonBody,
    mesh: &Mesh,
    bones: &[Affine; MAX_BONES],
    eye: [f32; 3],
    pitch: f32,
    role: &str,
) -> (Affine, f32) {
    keep_clear(body, posed(mesh, bones), eye, pitch, role)
}

fn posed<'a>(
    mesh: &'a Mesh,
    bones: &'a [Affine; MAX_BONES],
) -> impl Iterator<Item = [f32; 3]> + 'a {
    mesh.positions
        .iter()
        .zip(&mesh.bones)
        .map(|(p, &b)| bones[usize::from(b)].apply(*p))
}

/// How your body (its posed vertices `points`, in the body's frame; a skinned body's too,
/// CHARACTERS.md §11.12) is kept clear of `eye` (in the body's frame), and how far back that is at
/// the eye's height: leaned back from its soles ([`lean`]) by `back` plus `look_down` × the sine of
/// how far down you look, so the soles stay where they stand (moved back whole instead when it is
/// off its feet, its lowest point `OFF_GROUND_M` up: a swim stroke); then, only for a pose that still
/// brings a vertex within `clear` of the eye (a crouch under a lagging eye, a slide), moved back
/// whole as much more as clears it.
pub fn keep_clear(
    body: &FirstPersonBody,
    points: impl IntoIterator<Item = [f32; 3]>,
    eye: [f32; 3],
    pitch: f32,
    role: &str,
) -> (Affine, f32) {
    let down = (-pitch.to_radians().sin()).max(0.0);
    let base = body.back + body.look_down * down;
    let points: Vec<[f32; 3]> = points.into_iter().collect();
    // Planted on its soles (the lowest point at the feet) it leans; off them (the feet tucked in a
    // jump) or in a role that stands on nothing (swimming, climbing, flying) there is no ground to
    // stand on, and it moves back whole, eased between.
    let low = points.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    let w = if stands(role) {
        ((OFF_GROUND_M - low) / (OFF_GROUND_M - ON_GROUND_M)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let leaned = Affine::translation([0.0, 0.0, -(1.0 - w) * base]).then(&lean(w * base, eye));
    let c2 = body.clear * body.clear;
    let mut more = 0.0f32;
    for v in points {
        let v = leaned.apply(v);
        let (dx, dy, dz) = (v[0] - eye[0], v[1] - eye[1], v[2] - eye[2]);
        let r2 = dx * dx + dy * dy;
        if r2 < c2 {
            // Moved back by m more, the vertex is at dz − m along z: clear once dz − m ≤ −√(c² − r²).
            more = more.max(dz + (c2 - r2).sqrt());
        }
    }
    (
        Affine::translation([0.0, 0.0, -more]).then(&leaned),
        base + more,
    )
}

/// Whether a body playing `role` stands on the ground (not swimming, climbing or flying).
pub fn stands(role: &str) -> bool {
    !matches!(role, "swim" | "climb" | "fly")
}

/// Your body's lowest point at most this high over its feet (m): planted, it leans from its soles.
const ON_GROUND_M: f32 = 0.04;
/// And at least this high: off the ground, it moves back whole.
const OFF_GROUND_M: f32 = 0.12;

/// Your body leaned back by `s` (m at the eye's height) in its own frame: each point moved along −z
/// by `s` × its height over the soles ÷ the eye's, so the soles stay where they stand (on the
/// ground, a ramp or a slope, as another player's do: owner, 2026-11-13, the first-person feet
/// "seemed to be floating" when the whole body was moved back off them and its shadow).
pub fn lean(s: f32, eye: [f32; 3]) -> Affine {
    let k = s / eye[1].max(0.5);
    Affine {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, -k], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
    }
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
            for name in [
                "head",
                "arm_l",
                "forearm_l",
                "hand_l",
                "arm_r",
                "forearm_r",
                "hand_r",
            ] {
                let bone = ch.bone(name).unwrap() as u8;
                assert!(!m.bones.contains(&bone), "{}: {name}", l.name);
            }
            for id in [
                "neck",
                "head",
                "hat_brim",
                "hood_brow",
                "upper_arm_l",
                "knuckles_l",
            ] {
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
        let (placed, s) = place(&b, &m, &bones, chest, 0.0, "idle");
        assert!(s > b.back + 0.1, "{s}");
        let leaned = placed;
        for (p, &k) in m.positions.iter().zip(&m.bones) {
            let v = leaned.apply(bones[usize::from(k)].apply(*p));
            let d = (v[0] - chest[0])
                .hypot(v[1] - chest[1])
                .hypot(v[2] - chest[2]);
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
