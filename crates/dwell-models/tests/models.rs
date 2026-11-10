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

// Owner, 2026-11-10: "The animal models are too smooth": every model is baked at each model-detail
// level (ADR 0070 §10), fewer triangles level by level on the same skin and clips, its colours in
// COLOR_0 and its normals flat (one a face: a triangle's three corners share it).
#[test]
fn every_model_has_each_detail_level_flat_shaded_on_one_skin() {
    use dwell_models::DETAIL_LEVELS;
    for m in MODELS {
        let model = read("", m.glb);
        assert_eq!(model.level_count(), DETAIL_LEVELS.len(), "{}", m.name);
        let mut last = usize::MAX;
        for k in 0..model.level_count() {
            let l = model.level(k);
            let tris = l.indices.len() / 3;
            assert!(
                tris > 50 && tris < last,
                "{} level {k}: {tris} triangles",
                m.name
            );
            last = tris;
            assert_eq!(l.positions.len(), l.colors.len(), "{} level {k}", m.name);
            let mut colours: Vec<[u32; 3]> = l
                .colors
                .iter()
                .map(|c| c.map(|v| (v * 255.0).round() as u32))
                .collect();
            colours.sort_unstable();
            colours.dedup();
            assert!(
                colours.len() > 1,
                "{} level {k}: one colour (COLOR_0 lost?)",
                m.name
            );
            assert!(
                l.joints
                    .iter()
                    .flatten()
                    .all(|&j| usize::from(j) < model.skin.len()),
                "{} level {k}",
                m.name
            );
            for t in l.indices.chunks_exact(3) {
                let n = l.normals[t[0] as usize];
                for &i in &t[1..] {
                    assert!(
                        near(l.normals[i as usize], n),
                        "{} level {k}: smooth",
                        m.name
                    );
                }
            }
        }
    }
}

// Owner, 2026-11-10: "we need a deer model with the antlers taken off for a female deer". The doe
// is the deer's FBX without the antlers: no vertices above the ears, the same skeleton and clips.
#[test]
fn the_doe_is_the_deer_without_antlers_on_the_same_skin_and_clips() {
    let find = |n: &str| MODELS.iter().find(|m| m.name == n).unwrap();
    let (deer, doe) = (read("", find("deer").glb), read("", find("doe").glb));
    let joints = |m: &glb::Model| -> Vec<String> {
        m.skin.iter().map(|&j| m.nodes[j].name.clone()).collect()
    };
    assert_eq!(joints(&deer), joints(&doe));
    let clips =
        |m: &glb::Model| -> Vec<String> { m.clips.iter().map(|c| c.name.clone()).collect() };
    assert_eq!(clips(&deer), clips(&doe));
    // The ears reach 1.61 (model units, +y up); the antlers rise to 1.97.
    for k in 0..deer.level_count() {
        let above = |m: &glb::Model| m.level(k).positions.iter().filter(|p| p[1] > 1.65).count();
        assert!(
            above(&deer) > 20,
            "level {k}: the deer's antlers: {}",
            above(&deer)
        );
        assert_eq!(above(&doe), 0, "level {k}: the doe has antler vertices");
    }
    assert!(doe.positions.len() < deer.positions.len());
}

// Owner, 2026-11-10: "use the dog recolored. Make it twice as big for default too." The wolf is the
// dog's mesh and clips, its colours greys (none of the dog's white or blue), drawn at twice its size.
#[test]
fn the_wolf_is_the_dog_recoloured_grey_at_twice_its_size() {
    let find = |n: &str| MODELS.iter().find(|m| m.name == n).unwrap();
    let (dog_data, wolf_data) = (find("dog"), find("wolf"));
    let (dog, wolf) = (read("", dog_data.glb), read("", wolf_data.glb));
    assert_eq!(dog.positions, wolf.positions, "the dog's mesh");
    let clips =
        |m: &glb::Model| -> Vec<String> { m.clips.iter().map(|c| c.name.clone()).collect() };
    assert_eq!(clips(&dog), clips(&wolf));
    assert_eq!(wolf_data.scale, 2.0 * dog_data.scale);
    let rgb = |c: &[f32; 3]| c.map(|v| (v * 255.0).round() as i32);
    // The dog's coat (its commonest colour) and its collar's blue are gone; most of it is grey.
    let dog_rgb: Vec<_> = dog.colors.iter().map(rgb).collect();
    let wolf_rgb: Vec<_> = wolf.colors.iter().map(rgb).collect();
    let coat = *dog_rgb
        .iter()
        .max_by_key(|c| dog_rgb.iter().filter(|d| d == c).count())
        .unwrap();
    assert!(!wolf_rgb.contains(&coat), "the dog's coat {coat:?}");
    let blue = dog_rgb
        .iter()
        .find(|c| c[2] > c[0] + 60)
        .expect("the dog's collar");
    assert!(!wolf_rgb.contains(blue));
    let grey = wolf_rgb
        .iter()
        .filter(|c| c.iter().max().unwrap() - c.iter().min().unwrap() < 40)
        .count();
    assert!(
        grey * 10 > wolf_rgb.len() * 9,
        "grey {grey} of {}",
        wolf_rgb.len()
    );
}
