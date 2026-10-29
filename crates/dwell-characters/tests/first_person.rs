//! The first-person arms as the player sees them (CHARACTERS.md §5): the walk's hand bob rate and
//! the hand's place and size on screen through the arms' own projection. Pins the owner's playtest
//! tuning (2026-10-20): the bob was 2–3x too fast, the hand ~50% too big and too far up and left.
//!
//! The arms are posed in view space (+x left, +y up, +z ahead) and turned into the world by the
//! camera's basis (`arms_frame`), so none of this depends on the face.

use dwell_characters::anim::{arm_transforms, sample, with_item};
use dwell_characters::animator::{Animator, MoveInput, Space};
use dwell_characters::{TABLES, View, mesh, player};
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::PlayerState;

/// The screen the projection is measured on (a typical 16:9 window).
const ASPECT: f32 = 16.0 / 9.0;

/// The hand bob's frequency (dips a second) while the controller moves at `speed` in `state`, read
/// from the arms' animator: the anchor's height (`Pose::root`) sampled at 240 Hz for 10 s, after a
/// second to finish the cross-fade into the gait.
fn bob_hz(state: PlayerState, speed: f32) -> f32 {
    let mut a = Animator::new(&TABLES, player(), Space::Arms);
    let input = MoveInput {
        state,
        velocity: [speed, 0.0, 0.0],
        ..MoveInput::default()
    };
    let dt = 1.0 / 240.0;
    for _ in 0..240 {
        a.update(&input, dt);
    }
    let ys: Vec<f32> = (0..2400).map(|_| a.update(&input, dt).root[1]).collect();
    let dips = ys
        .windows(3)
        .filter(|w| w[1] < w[0] && w[1] <= w[2])
        .count();
    dips as f32 / 10.0
}

fn speeds() -> (f32, f32) {
    let m = PlayerControllerConfig::default().movement;
    (m.walk_speed, m.run_speed)
}

#[test]
fn the_walk_bob_is_unhurried() {
    let (walk, _) = speeds();
    let hz = bob_hz(PlayerState::Walking, walk);
    // Was 6.25 Hz (a dip each 0.8 m, the legs' stride); the owner asked for ~2.5x slower.
    assert!(
        (2.0..=3.0).contains(&hz),
        "the walk bobs at {hz} Hz at {walk} m/s"
    );
}

#[test]
fn the_sprint_bobs_faster_than_the_walk_in_proportion_to_the_speed() {
    let (walk, run) = speeds();
    let w = bob_hz(PlayerState::Walking, walk);
    let s = bob_hz(PlayerState::Running, run);
    assert!(
        s > w,
        "the sprint ({s} Hz) bobs faster than the walk ({w} Hz)"
    );
    let ratio = (s / w) / (run / walk);
    assert!(
        (0.9..=1.1).contains(&ratio),
        "the sprint's bob ({s} Hz) and the walk's ({w} Hz) scale with the speed ({run} vs {walk} m/s)"
    );
    assert!((3.0..=5.0).contains(&s), "the sprint bobs at {s} Hz");
}

/// The screen box (NDC: x right, y up, −1..1, unclipped) of the hand and, with `held`, the held
/// block, in the arms role's clip `t` seconds in.
fn hand_box(role: &str, t: f32, held: bool) -> ([f32; 2], [f32; 2]) {
    let ch = player();
    let fp = ch.first_person.expect("the player has an arms view");
    let hand = ch.bone("hand_r").expect("the rig has hand_r");
    let clip = fp.clips.iter().find(|(r, _)| *r == role).expect("role").1;
    // As drawn: the held block's turns on the clip (it is carried on the upturned palm).
    let item = held.then(|| ch.item("block")).flatten();
    let bones = arm_transforms(ch, &with_item(&sample(TABLES.clip(clip), t), item));
    let f = 1.0 / (fp.fov.to_radians() / 2.0).tan();
    let mut meshes = vec![mesh::build(ch.looks[0].parts, View::Arms, fp.bones)];
    if held {
        let block = ch.item("block").expect("the player holds blocks");
        meshes.push(mesh::build(block.parts, View::Arms, fp.bones));
    }
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for m in &meshes {
        for (p, &b) in m.positions.iter().zip(&m.bones) {
            if usize::from(b) != hand {
                continue;
            }
            let v = bones[hand].apply(*p);
            assert!(v[2] > 0.1, "the hand is well ahead of the eye: {v:?}");
            let s = [-v[0] / v[2] * f / ASPECT, v[1] / v[2] * f];
            for k in 0..2 {
                lo[k] = lo[k].min(s[k]);
                hi[k] = hi[k].max(s[k]);
            }
        }
    }
    assert!(lo[0] < hi[0], "the hand has vertices");
    (lo, hi)
}

#[test]
fn the_held_block_and_hand_sit_in_the_lower_right_at_a_modest_size() {
    // Standing: was centre (0.35, −0.72), its top at −0.07 (the screen's middle) and 0.65 of the
    // screen's height; the owner asked for ~1/1.5 the size, lower and further right. That read
    // too small (0.43), so it is now ~0.55: between the two.
    let (lo, hi) = hand_box("idle", 0.0, true);
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let height = (hi[1] - lo[1]) / 2.0;
    assert!(
        (0.5..=0.6).contains(&height),
        "height {height} of the screen"
    );
    assert!(
        (0.45..=0.8).contains(&centre[0]),
        "centre {centre:?} right of the middle"
    );
    assert!(
        (-0.95..=-0.6).contains(&centre[1]),
        "centre {centre:?} below the middle"
    );
    assert!(
        hi[1] < -0.3,
        "the top ({}) stays well below the crosshair",
        hi[1]
    );
    assert!(
        lo[0] > 0.2,
        "the left edge ({}) stays right of the crosshair",
        lo[0]
    );
    assert!(
        hi[1] > -0.8 && lo[0] < 0.8,
        "and it is still in view: {lo:?}..{hi:?}"
    );
}

#[test]
fn the_hand_stays_low_right_through_the_gaits() {
    for role in ["idle", "walk", "sprint"] {
        for t in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
            for held in [false, true] {
                let (lo, hi) = hand_box(role, t, held);
                assert!(
                    hi[1] < -0.25,
                    "{role} {t} s: top {} below the crosshair",
                    hi[1]
                );
                assert!(
                    lo[0] > 0.15,
                    "{role} {t} s: left {} right of the crosshair",
                    lo[0]
                );
            }
        }
    }
}
