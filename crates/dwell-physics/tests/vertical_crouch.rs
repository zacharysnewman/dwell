//! Ported PPC suites Phase3Vertical and Phase4Crouch on voxel geometry. Default config: jump
//! velocity √(2 · 20 · 1.25) ≈ 7.07 m/s, apex 1.25 m.

mod common;
use common::*;
use dwell_physics::Vec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::events as ev;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn jump_at(t: i32, from: i32, to: i32) -> dwell_physics::controller::Input {
    input(0.0, 0.0, false, t >= from && t < to, false, 0.0, 0.0)
}

#[test]
fn falls_and_lands_on_the_floor() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 3.0, 0.5));
    w.step(5);
    assert_eq!(w.c(e).state, State::Falling);
    w.step(ticks(1.5));
    assert!(w.c(e).ground.grounded);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
    assert_eq!(w.c(e).state, State::Idle);
    assert_eq!(w.count(e, ev::LANDED), 1);
}

#[test]
fn jump_reaches_the_configured_apex() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| jump_at(t, 5, 8));
    let apex = w.max_feet(e, ticks(1.5));
    assert!(approx(apex, 1.25, 0.05), "{apex}");
    assert_eq!(w.count(e, ev::JUMPED), 1);
    assert!(w.c(e).ground.grounded);
}

#[test]
fn holding_jump_does_not_bunny_hop() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| jump_at(t, 5, i32::MAX));
    w.step(ticks(3.0));
    assert_eq!(w.count(e, ev::JUMPED), 1);
}

#[test]
fn a_press_just_before_landing_is_buffered() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 1.25, 0.5));
    w.set_input(|t, _| jump_at(t, 15, 17));
    w.step(ticks(1.0));
    assert_eq!(w.count(e, ev::JUMPED), 1);
}

#[test]
fn a_press_too_early_is_forgotten() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 1.25, 0.5));
    w.set_input(|t, _| jump_at(t, 3, 5));
    w.step(ticks(1.0));
    assert_eq!(w.count(e, ev::JUMPED), 0);
}

/// A ledge ending at z = 2 above a deep floor; the player walks off it at 5 m/s.
fn ledge(jump_tick: i32) -> (PlayerTestWorld, u32) {
    let mut w = PlayerTestWorld::new();
    w.fill(-4, -1, -12, 3, -1, 1, cube_block());
    w.floor_at(-20, 16);
    let e = w.spawn(v(0.5, 0.0, -2.0));
    w.set_input(move |t, _| {
        input(
            0.0,
            1.0,
            false,
            jump_tick >= 0 && t >= jump_tick && t < jump_tick + 2,
            false,
            0.0,
            0.0,
        )
    });
    (w, e)
}

fn tick_when_ungrounded() -> i32 {
    let (mut w, e) = ledge(-1);
    for i in 0..300 {
        w.step(1);
        if !w.c(e).ground.grounded {
            return i;
        }
    }
    -1
}

#[test]
fn coyote_time_allows_a_jump_just_after_walking_off() {
    let off = tick_when_ungrounded();
    assert!(off > 0);
    let (mut w, e) = ledge(off + 3);
    w.step(off + 6);
    assert_eq!(w.count(e, ev::JUMPED), 1);
    assert!(w.vel(e).y > 5.0);
}

#[test]
fn coyote_time_lasts_0_2_s() {
    let off = tick_when_ungrounded();
    assert!(off > 0);
    let (mut w, e) = ledge(off + 11);
    w.step(off + 14);
    assert_eq!(w.count(e, ev::JUMPED), 1);
    assert!(w.vel(e).y > 3.0);
}

#[test]
fn coyote_time_expires() {
    let off = tick_when_ungrounded();
    let (mut w, e) = ledge(off + 14);
    w.step(off + 20);
    assert_eq!(w.count(e, ev::JUMPED), 0);
}

#[test]
fn a_second_press_right_after_a_jump_is_not_a_double_jump() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| {
        input(
            0.0,
            0.0,
            false,
            (5..7).contains(&t) || (12..14).contains(&t),
            false,
            0.0,
            0.0,
        )
    });
    let apex = w.max_feet(e, ticks(1.5));
    assert_eq!(w.count(e, ev::JUMPED), 1);
    assert!(approx(apex, 1.25, 0.05), "{apex}");
}

#[test]
fn a_voxel_ceiling_stops_the_jump() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 2, -3, 3, 2, 3, cube_block());
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| jump_at(t, 5, 8));
    let apex = w.max_feet(e, ticks(1.0));
    w.step(ticks(1.0));
    assert!(apex > 0.1 && apex < 0.25, "{apex}");
    assert!(w.c(e).ground.grounded);
}

#[test]
fn a_launch_kick_while_grounded_launches() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.kick(5, e, v(0.0, 15.0, 0.0));
    let apex = w.max_feet(e, ticks(2.0));
    assert!(approx(apex, 15.0 * 15.0 / 40.0, 0.05), "{apex}");
}

#[test]
fn a_jump_during_a_launch_keeps_the_larger_velocity() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| jump_at(t, 8, 10));
    w.kick(5, e, v(0.0, 15.0, 0.0));
    assert!(w.max_feet(e, ticks(2.0)) > 5.0);
}

#[test]
fn walks_up_and_down_a_20_degree_ramp_staying_grounded() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.boxed(
        v(0.5, 0.0, 6.0),
        v(2.0, 0.5, 5.0),
        euler(-20.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
    let e = w.spawn(v(0.5, 0.0, -1.0));
    let turn = ticks(1.4);
    w.set_input(move |t, _| mv(0.0, if t < turn { 1.0 } else { -1.0 }));
    let mut ungrounded = 0;
    let (mut at_start, mut at_end) = (Vec3::ZERO, Vec3::ZERO);
    for i in 0..ticks(2.8) {
        w.step(1);
        ungrounded += i32::from(!w.c(e).ground.grounded);
        if i == ticks(1.2) {
            at_start = w.pos(e);
        }
        if i == ticks(1.35) {
            at_end = w.pos(e);
        }
    }
    let climbed = (at_end - at_start) / 0.15;
    assert!(approx(climbed.z, 5.0, 0.03), "{climbed:?}");
    assert!(
        approx(climbed.y, 5.0 * 20f32.to_radians().tan(), 0.15),
        "{climbed:?}"
    );
    assert_eq!(ungrounded, 0);
    assert!(approx(w.feet(e), 0.0, 0.03), "{}", w.feet(e));
}

#[test]
fn crouching_on_the_ground_keeps_the_feet_planted() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| input(0.0, 0.0, false, false, t >= 5, 0.0, 0.0));
    w.step(20);
    assert!(w.c(e).crouching);
    assert!(approx(w.players.half_height(e), 0.45, 1e-5));
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
    assert_eq!(w.c(e).state, State::Crouching);
    assert_eq!(w.count(e, ev::CROUCH_CHANGED), 1);
}

#[test]
fn releasing_crouch_stands_back_up() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| input(0.0, 0.0, false, false, (5..20).contains(&t), 0.0, 0.0));
    w.step(40);
    assert!(!w.c(e).crouching);
    assert!(approx(w.players.half_height(e), 0.9, 1e-5));
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
}

#[test]
fn crouch_walking_is_slower() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, -8.0));
    w.set_input(|_, _| input(0.0, 1.0, false, false, true, 0.0, 0.0));
    w.step(ticks(1.0));
    assert!(
        approx(w.horizontal_speed(e), 1.6, 0.02),
        "{}",
        w.horizontal_speed(e)
    );
}

/// A 1-tall crawlspace: roof blocks at y = 1 over z = 2..4.
fn crawlspace(w: &mut PlayerTestWorld) {
    w.floor();
    w.fill(-3, 1, 2, 3, 1, 4, cube_block());
}

#[test]
fn standing_the_player_cannot_pass_under_a_1_m_roof() {
    let mut w = PlayerTestWorld::new();
    crawlspace(&mut w);
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    assert!(w.pos(e).z < 1.75, "{}", w.pos(e).z);
}

#[test]
fn crouched_the_player_crawls_through_and_stands_once_clear() {
    let mut w = PlayerTestWorld::new();
    crawlspace(&mut w);
    let e = w.spawn(v(0.5, 0.0, 0.0));
    let release = ticks(2.4);
    w.set_input(move |t, _| input(0.0, 1.0, false, false, t < release, 0.0, 0.0));
    let mut stayed = true;
    for _ in 0..ticks(4.5) {
        w.step(1);
        let z = w.pos(e).z;
        if z > 2.35 && z < 4.65 && !w.c(e).crouching {
            stayed = false;
        }
    }
    assert!(w.pos(e).z > 6.0, "{}", w.pos(e).z);
    assert!(stayed);
    assert!(!w.c(e).crouching);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
}

#[test]
fn mid_air_crouch_tucks_the_legs_without_moving_the_head() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|t, _| input(0.0, 0.0, false, (2..4).contains(&t), t >= 12, 0.0, 0.0));
    w.step(12);
    let before = w.head(e);
    w.step(1);
    assert!(w.c(e).crouching);
    let d = w.head(e) - before;
    assert!(d > -0.01 && d < 0.15, "{d}");
}

#[test]
fn a_crouch_jump_lifts_the_feet_higher() {
    let max_feet = |crouch: bool| {
        let mut w = PlayerTestWorld::new();
        w.floor();
        let e = w.spawn(v(0.5, 0.0, 0.5));
        w.set_input(move |t, _| {
            input(
                0.0,
                0.0,
                false,
                (2..4).contains(&t),
                crouch && t >= 10,
                0.0,
                0.0,
            )
        });
        w.max_feet(e, ticks(1.0))
    };
    assert!(max_feet(true) > max_feet(false) + 0.8);
}

#[test]
fn mid_air_boost_adds_height() {
    let apex = |boost: f32| {
        let mut config = PlayerControllerConfig::default();
        config.crouch.mid_air_boost = boost;
        let mut w = PlayerTestWorld::with_config(config);
        w.floor();
        let e = w.spawn(v(0.5, 0.0, 0.5));
        w.set_input(|t, _| input(0.0, 0.0, false, (2..4).contains(&t), t >= 10, 0.0, 0.0));
        w.max_feet(e, ticks(1.5))
    };
    assert!(apex(3.0) > apex(0.0) + 0.5);
}

#[test]
fn landing_crouched_keeps_a_centred_capsule_then_stands_from_the_feet() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    let release = ticks(2.0);
    w.set_input(move |t, _| {
        input(
            0.0,
            0.0,
            false,
            (2..4).contains(&t),
            t >= 10 && t < release,
            0.0,
            0.0,
        )
    });
    w.step(release - 5);
    assert!(w.c(e).crouching);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
    w.step(20);
    assert!(!w.c(e).crouching);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
}
