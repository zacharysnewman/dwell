//! The skinned models (docs/CHARACTERS.md §11): the GLBs read, the skinning math, the role → clip
//! player and its blending, and the fixture model added by data alone.

use dwell_models::anim::{self, BLEND_S, Player};
use dwell_models::{MAX_JOINTS, MODELS, fixture, glb, player};

fn read(table: &str, path: &str) -> glb::Model {
    let root = env!("CARGO_MANIFEST_DIR");
    let bytes = std::fs::read(format!("{root}/../../shared/models/{table}{path}"))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    glb::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn every_model_reads_with_its_joints_and_clips() {
    for m in MODELS {
        let model = read("", m.glb);
        assert_eq!(model.skin.len(), m.joints, "{}", m.name);
        assert!(model.skin.len() <= MAX_JOINTS, "{}", m.name);
        assert!(!model.indices.is_empty(), "{}", m.name);
        assert_eq!(model.positions.len(), model.weights.len(), "{}", m.name);
        for r in m.roles {
            assert!(
                model.clip(r.clip).is_some(),
                "{}: role {} → {}",
                m.name,
                r.role,
                r.clip
            );
        }
        // The weights are normalised and every vertex is on the skin.
        for w in &model.weights {
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-3, "{}", m.name);
        }
    }
}

// Owner, 2026-11-10: "Chickens are showing up pure white." The chicken's and the kitty's GLBs had
// the palette colours in COLOR_1 behind the pack's own white COLOR_0, which is the layer the game
// reads. Every model's colours are its palette remap: more than one colour, none of them all white.
#[test]
fn every_model_is_coloured_not_plain_white() {
    for m in MODELS {
        let model = read("", m.glb);
        assert_eq!(model.colors.len(), model.positions.len(), "{}", m.name);
        let mut distinct: Vec<[u32; 3]> = model
            .colors
            .iter()
            .map(|c| c.map(|v| (v * 255.0).round() as u32))
            .collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() > 1,
            "{}: one colour only, {:?} — the palette colours aren't in COLOR_0",
            m.name,
            distinct
        );
    }
}

#[test]
fn the_player_model_is_the_peasant_standing_about_player_height() {
    let (data, model) = player();
    assert_eq!(data.name, "peasant");
    let joints = anim::joint_matrices(model, &anim::rest(model));
    let ys: Vec<f32> = (0..model.positions.len())
        .map(|v| anim::skin_point(model, &joints, v)[1] * data.scale)
        .collect();
    let (lo, hi) = ys
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
    assert!(lo.abs() < 0.1, "feet at {lo}");
    assert!((1.6..2.0).contains(&(hi - lo)), "height {}", hi - lo);
    assert!(model.clip("idle").is_some() && model.clip("walk").is_some());
}

#[test]
fn the_fixture_reads_its_joints_clips_and_colours() {
    let m = read("fixtures/", "two_bone.glb");
    assert_eq!(m.skin.len(), 2);
    assert_eq!(
        m.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["rest", "bend"]
    );
    assert_eq!(m.colors[0], [1.0, 0.0, 0.0]);
    assert_eq!(m.joints[1], [1, 0, 0, 0]);
    assert!((m.clips[1].duration - 1.0).abs() < 1e-6);
}

#[test]
fn a_joint_turned_moves_its_weighted_vertex_and_not_the_others() {
    let m = read("fixtures/", "two_bone.glb");
    // At rest every vertex is where the mesh put it.
    let rest = anim::joint_matrices(&m, &anim::rest(&m));
    for v in 0..3 {
        assert!(near(anim::skin_point(&m, &rest, v), m.positions[v]), "{v}");
    }
    // The tip turned 90° about +z: vertex 1 (2 m up, on the tip) swings to (−1, 1, 0); the root's
    // vertices stay.
    let bent = anim::joint_matrices(&m, &anim::sample(&m, &m.clips[1], 1.0));
    assert!(near(anim::skin_point(&m, &bent, 1), [-1.0, 1.0, 0.0]));
    assert!(near(anim::skin_point(&m, &bent, 0), [0.0, 0.0, 0.0]));
    assert!(near(anim::skin_point(&m, &bent, 2), [1.0, 0.0, 0.0]));
    // Halfway through the clip, halfway round (45°).
    let half = anim::joint_matrices(&m, &anim::sample(&m, &m.clips[1], 0.5));
    let s = std::f32::consts::FRAC_1_SQRT_2;
    assert!(near(anim::skin_point(&m, &half, 1), [-s, 1.0 + s, 0.0]));
}

#[test]
fn roles_map_to_their_clips_and_fall_back() {
    let data = &fixture::MODELS[0];
    assert_eq!(data.role("walk").clip, "bend");
    assert_eq!(data.role("sprint").speed, 2.0);
    // No `swim` clip: the fallback's.
    assert_eq!(data.role("swim").clip, "rest");
    let peasant = dwell_models::find(MODELS, "peasant").unwrap();
    assert_eq!(peasant.role("sprint").clip, "walk");
    assert_eq!(peasant.role("jump").clip, "idle");
}

#[test]
fn a_change_of_role_cross_fades_over_the_blend_time() {
    let data = &fixture::MODELS[0];
    let m = read("fixtures/", data.glb);
    let mut p = Player::new(data, &m, "walk");
    p.update(data, &m, "walk", 1.0);
    assert_eq!(p.weight(), 1.0);
    // Walk (bend, at its end: tip at 90°) → idle (rest): halfway through the fade the tip is at
    // 45°.
    p.update(data, &m, "idle", 0.0);
    assert_eq!(p.now.role, "idle");
    assert_eq!(p.weight(), 0.0);
    p.update(data, &m, "idle", BLEND_S / 2.0);
    assert!((p.weight() - 0.5).abs() < 1e-5);
    let fading = p.fading.unwrap().0;
    // The fading clip keeps its own clock.
    assert!((fading.time - (1.0 + BLEND_S / 2.0)).abs() < 1e-5);
    p.update(data, &m, "idle", BLEND_S);
    assert!(p.fading.is_none());
    let joints = anim::joint_matrices(&m, &p.pose(&m));
    assert!(near(anim::skin_point(&m, &joints, 1), [0.0, 2.0, 0.0]));
}

#[test]
fn the_same_clip_at_another_speed_keeps_its_phase_without_a_fade() {
    let data = &fixture::MODELS[0];
    let m = read("fixtures/", data.glb);
    let mut p = Player::new(data, &m, "walk");
    p.update(data, &m, "walk", 0.25);
    p.update(data, &m, "sprint", 0.25);
    assert!(p.fading.is_none());
    assert!((p.now.time - 0.75).abs() < 1e-5, "{}", p.now.time);
}

#[test]
fn a_bad_glb_fails_with_a_message() {
    assert!(glb::parse(b"nope").is_err());
    let mut bytes = std::fs::read(format!(
        "{}/../../shared/models/fixtures/two_bone.glb",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    bytes.truncate(40);
    assert!(glb::parse(&bytes).is_err());
}
