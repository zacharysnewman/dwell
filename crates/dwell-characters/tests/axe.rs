//! The stone axe in first person (docs/adr/0067-stone-axe-and-chopping.md, the owner's 2026-11-08
//! playtest: "the axe is held backwards, the blade faces the player"): the blade's edge points away
//! from the eye in both holds, the two holds differ, and the edge leads the head through each
//! swing's strike.
//!
//! face-independent: the arms are posed in view space (+x left, +y up, +z ahead) and turned into
//! the world by the camera's basis (`arms_frame`), the same on both faces.

use dwell_characters::anim::{arm_transforms, sample, with_item};
use dwell_characters::{TABLES, View, mesh, player};

/// The centre (view space) of the held item's part `id` in the arms role's clip, `t` s in.
fn part_centre(item: &str, role: &str, id: &str, t: f32) -> [f32; 3] {
    let ch = player();
    let fp = ch.first_person.expect("arms view");
    let it = ch.item(item).unwrap_or_else(|| panic!("item {item}"));
    let clip = ch
        .arms_clip(role)
        .unwrap_or_else(|| panic!("role {role}"));
    let bones = arm_transforms(ch, &with_item(&sample(TABLES.clip(clip), t), Some(it)));
    let part = it
        .parts
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("part {id} of {item}"));
    let m = mesh::build(std::slice::from_ref(part), View::Arms, fp.bones);
    let mut c = [0.0; 3];
    for (p, &b) in m.positions.iter().zip(&m.bones) {
        let v = bones[usize::from(b)].apply(*p);
        for k in 0..3 {
            c[k] += v[k] / m.positions.len() as f32;
        }
    }
    c
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    a.map(|v| v / l)
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The blade's direction: from the lashing on the handle out to the blade's edge.
fn blade(item: &str, role: &str, t: f32) -> [f32; 3] {
    norm(sub(
        part_centre(item, role, "axe_blade", t),
        part_centre(item, role, "axe_lashing", t),
    ))
}

/// The axe's two holds and their swings: (held item, idle role, swing role).
const HOLDS: [(&str, &str, &str); 2] = [
    ("stone_axe", "idle", "chop_level"),
    ("stone_axe_vertical", "idle", "chop_vertical"),
];

#[test]
fn the_axe_blade_faces_away_from_the_eye_in_both_holds() {
    for (item, idle, _) in HOLDS {
        let d = blade(item, idle, 0.0);
        assert!(
            d[2] > 0.3,
            "{item}: the blade points away from the eye (+z ahead), not at it: {d:?}"
        );
    }
}

#[test]
fn the_two_holds_differ() {
    let a = blade(HOLDS[0].0, HOLDS[0].1, 0.0);
    let b = blade(HOLDS[1].0, HOLDS[1].1, 0.0);
    assert!(
        dot(a, b) < 0.8,
        "the level and vertical holds turn the axe visibly: {a:?} vs {b:?}"
    );
}

#[test]
fn the_edge_leads_the_head_through_the_strike() {
    for (item, _, swing) in HOLDS {
        let ch = player();
        let clip = TABLES.clip(ch.arms_clip(swing).expect("swing clip"));
        // The strike: the fastest stretch of the head's path.
        let n = 60;
        let dt = clip.duration / n as f32;
        let (mut best, mut at) = (0.0, 0.0);
        for i in 0..n {
            let t = i as f32 * dt;
            let v = sub(
                part_centre(item, swing, "axe_blade", t + dt),
                part_centre(item, swing, "axe_blade", t),
            );
            let s = dot(v, v).sqrt();
            if s > best {
                (best, at) = (s, t + dt / 2.0);
            }
        }
        let v = norm(sub(
            part_centre(item, swing, "axe_blade", at + dt / 2.0),
            part_centre(item, swing, "axe_blade", at - dt / 2.0),
        ));
        let d = blade(item, swing, at);
        assert!(
            dot(v, d) > 0.4,
            "{item} {swing}: at the strike ({at:.2} s) the edge leads: motion {v:?}, blade {d:?}"
        );
    }
}
