//! Ported PPC suites Phase5Platform and Phase6Climb (ladders as voxels), plus Dwell's swim layer.
//! Moving platforms are Tier 1 bodies.

mod common;
use common::*;
use dwell_blocks::require;
use dwell_physics::query::GroundRef;
use dwell_physics::{DQuat, Vec3, controller::events as ev};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
const PLATFORM_HALF: Vec3 = Vec3::new(3.0, 0.5, 3.0);
fn look(x: f32, y: f32, yaw: f32, pitch: f32) -> dwell_physics::controller::Input {
    input(x, y, false, false, false, yaw, pitch)
}

#[test]
fn rides_a_moving_kinematic_platform() {
    let mut w = PlayerTestWorld::new();
    let platform = w.boxed(
        v(0.0, -0.5, 0.0),
        PLATFORM_HALF,
        DQuat::IDENTITY,
        v(3.0, 0.0, 0.0),
        0.0,
    );
    let e = w.spawn(Vec3::ZERO);
    w.step(ticks(0.5));
    let settled = w.pos(e) - w.body_position(platform);
    w.step(ticks(2.0));
    assert!(w.body_position(platform).x > 5.0);
    assert!(((w.pos(e) - w.body_position(platform)) - settled).length() < 0.03);
    assert!(w.c(e).ground.grounded);
    assert!(matches!(w.c(e).ground.ground, GroundRef::Tier1Body(_)));
}

#[test]
fn rides_a_dynamic_platform_using_its_body_velocity() {
    let mut w = PlayerTestWorld::new();
    let platform = w.dynamic_box(
        v(0.0, -0.5, 0.0),
        PLATFORM_HALF,
        1000.0,
        v(2.0, 0.0, 0.0),
        true,
    );
    let e = w.spawn(Vec3::ZERO);
    w.step(ticks(0.5));
    let settled = w.pos(e) - w.body_position(platform);
    w.step(ticks(2.0));
    assert!(
        w.body_position(platform).x > 4.0,
        "{:?}",
        w.body_position(platform)
    );
    assert!(((w.pos(e) - w.body_position(platform)) - settled).length() < 0.05);
    assert!(
        approx(w.c(e).platform.base_velocity.x, 2.0, 0.05),
        "{:?}",
        w.c(e).platform.base_velocity
    );
}

#[test]
fn walking_on_a_moving_platform_is_relative_to_it() {
    let mut w = PlayerTestWorld::new();
    w.boxed(
        v(0.0, -0.5, 0.0),
        v(10.0, 0.5, 10.0),
        DQuat::IDENTITY,
        v(0.0, 0.0, 3.0),
        0.0,
    );
    let e = w.spawn(Vec3::ZERO);
    w.set_input(|_, _| mv(-1.0, 0.0)); // strafe left: +X at yaw 0
    w.step(ticks(0.8));
    assert!(approx(w.vel(e).x, 5.0, 0.02), "{:?}", w.vel(e));
    assert!(approx(w.vel(e).z, 3.0, 0.03), "{:?}", w.vel(e));
}

#[test]
fn carried_around_a_rotating_platform() {
    let mut w = PlayerTestWorld::new();
    w.boxed(
        v(0.0, -0.5, 0.0),
        v(5.0, 0.5, 5.0),
        DQuat::IDENTITY,
        Vec3::ZERO,
        90.0,
    );
    let e = w.spawn(v(0.0, 0.0, 2.0));
    w.step(3);
    let yaw_delta = w.c(e).platform.yaw_delta;
    w.step(ticks(1.0) - 3);
    let p = w.pos(e);
    assert!(approx((p.x * p.x + p.z * p.z).sqrt(), 2.0, 0.08), "{p:?}");
    assert!(p.x > 1.0, "{p:?}");
    assert!(approx(yaw_delta, 1.5, 0.05), "{yaw_delta}");
}

#[test]
fn rides_an_elevator_up_and_down_staying_grounded() {
    for speed in [2.0f32, -2.0] {
        let mut w = PlayerTestWorld::new();
        w.floor_at(-30, 16);
        let lift = w.boxed(
            v(0.0, -0.5, 0.0),
            PLATFORM_HALF,
            DQuat::IDENTITY,
            v(0.0, speed, 0.0),
            0.0,
        );
        let e = w.spawn(Vec3::ZERO);
        let mut ungrounded = 0;
        for _ in 0..ticks(2.0) {
            w.step(1);
            ungrounded += i32::from(!w.c(e).ground.grounded);
        }
        let top = w.body_position(lift).y + 0.5;
        assert!(
            approx(w.feet(e) - top, 0.0, 0.04),
            "speed {speed}: {}",
            w.feet(e) - top
        );
        assert!(ungrounded <= 2, "speed {speed}: {ungrounded}");
    }
}

#[test]
fn jumping_off_a_moving_platform_keeps_its_momentum() {
    let mut w = PlayerTestWorld::new();
    w.boxed(
        v(0.0, -0.5, 0.0),
        PLATFORM_HALF,
        DQuat::IDENTITY,
        v(4.0, 0.0, 0.0),
        0.0,
    );
    let e = w.spawn(Vec3::ZERO);
    w.set_input(|t, _| input(0.0, 0.0, false, (40..42).contains(&t), false, 0.0, 0.0));
    w.step(44);
    assert!(!w.c(e).ground.grounded);
    assert!(w.vel(e).x > 3.8 && w.vel(e).x < 4.05, "{:?}", w.vel(e));
}

#[test]
fn an_explosion_pushes_nearby_players_only() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let near = w.spawn(v(2.0, 0.0, 0.5));
    let far = w.spawn(v(12.0, 0.0, 0.5));
    w.step(2);
    w.explosion(5, v(0.0, 1.0, 0.5), 6.0, 12.0, 0.5);
    w.step(12);
    assert!(w.pos(near).x > 2.5);
    assert!(approx(w.pos(far).x, 12.0, 0.002));
}

#[test]
fn a_removed_body_underfoot_starts_a_fall() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(-10, 16);
    let b = w.static_box(v(0.0, -0.5, 0.0), PLATFORM_HALF);
    let e = w.spawn(Vec3::ZERO);
    w.step(10);
    assert!(w.c(e).ground.grounded);
    w.physics.remove_body(b);
    w.step(10);
    assert!(!w.c(e).ground.grounded);
    assert_eq!(w.c(e).state, State::Falling);
}

/// A 3 m ledge (x −2..2, z 2..12) with a ladder column (x = 0, z = 1, y 0..2) facing north,
/// mounted on the ledge's face.
fn ladder_world() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-2, 0, 2, 2, 2, 12, stone());
    w.fill(0, 0, 1, 0, 2, 1, require("dwell:ladder[facing=north]"));
    w
}

#[test]
fn walking_into_a_ladder_grabs_it_and_holds_height() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| if t < 20 { mv(0.0, 1.0) } else { mv(0.0, 0.0) });
    w.step(30);
    let y = w.pos(e).y;
    w.step(60);
    assert_eq!(w.count(e, ev::CLIMB_STARTED), 1);
    assert!(w.c(e).climb.climbing);
    assert_eq!(w.c(e).state, State::Climbing);
    assert!(approx(w.pos(e).y, y, 0.01));
}

#[test]
fn forward_climbs_up_and_snaps_to_the_ladder_face() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(45);
    assert!(w.c(e).climb.climbing);
    assert!(approx(w.vel(e).y, 3.0, 0.03), "{:?}", w.vel(e));
    assert!(approx(w.pos(e).z, 1.70, 0.02), "{}", w.pos(e).z);
}

#[test]
fn looking_down_climbs_down() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| {
        if t < 40 {
            mv(0.0, 1.0)
        } else {
            look(0.0, 1.0, 0.0, -45.0)
        }
    });
    w.step(48);
    assert!(w.c(e).climb.climbing);
    assert!(approx(w.vel(e).y, -3.0, 0.03), "{:?}", w.vel(e));
}

#[test]
fn glancing_down_slightly_still_climbs_up() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| look(0.0, 1.0, 0.0, if t < 20 { 0.0 } else { -10.0 }));
    w.step(40);
    assert!(approx(w.vel(e).y, 3.0, 0.03), "{:?}", w.vel(e));
}

#[test]
fn strafing_moves_along_the_ladder() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| if t < 30 { mv(0.0, 1.0) } else { mv(1.0, 0.0) });
    w.step(33);
    assert!(w.c(e).climb.climbing);
    assert!(approx(w.vel(e).x, -3.0, 0.03), "{:?}", w.vel(e));
}

#[test]
fn jumping_off_launches_away_and_does_not_re_grab() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| {
        input(
            0.0,
            if t < 40 { 1.0 } else { 0.0 },
            false,
            (40..42).contains(&t),
            false,
            0.0,
            0.0,
        )
    });
    w.step(41);
    assert!(!w.c(e).climb.climbing);
    assert_eq!(w.count(e, ev::CLIMB_ENDED), 1);
    assert!(w.vel(e).z < -2.0 && w.vel(e).y > 3.0, "{:?}", w.vel(e));
    w.step(60);
    assert_eq!(w.count(e, ev::CLIMB_STARTED), 1);
}

#[test]
fn climbing_down_to_the_floor_lets_go() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| if t < 50 { mv(0.0, 1.0) } else { mv(0.0, -1.0) });
    w.step(50 + ticks(1.5));
    assert_eq!(w.count(e, ev::CLIMB_ENDED), 1);
    assert!(!w.c(e).climb.climbing);
    assert!(w.c(e).ground.grounded);
}

#[test]
fn climbing_over_the_top_lands_on_the_ledge() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(3.0));
    assert!(!w.c(e).climb.climbing);
    assert!(w.c(e).ground.grounded);
    assert!(approx(w.feet(e), 3.0, 0.01), "{}", w.feet(e));
    assert!(w.pos(e).z > 2.3);
}

#[test]
fn standing_on_the_ledge_above_a_ladder_does_not_grab_it() {
    let mut w = ladder_world();
    let e = w.spawn_yaw(v(0.5, 3.0, 2.3), 180.0);
    w.step(30);
    assert!(!w.c(e).climb.climbing);
    assert!(w.c(e).ground.grounded);
}

#[test]
fn walking_off_the_ledge_onto_the_ladder_climbs_down_it() {
    let mut w = ladder_world();
    let e = w.spawn_yaw(v(0.5, 3.0, 3.0), 180.0);
    w.set_input(|_, _| look(0.0, 1.0, 180.0, -45.0));
    let mut fastest_fall = 0.0f32;
    for _ in 0..ticks(3.0) {
        w.step(1);
        if !w.c(e).ground.grounded {
            fastest_fall = fastest_fall.max(-w.vel(e).y);
        }
    }
    assert!(w.count(e, ev::CLIMB_STARTED) >= 1);
    assert!(approx(w.feet(e), 0.0, 0.01), "{}", w.feet(e));
    assert!(fastest_fall < 4.0, "{fastest_fall}");
}

/// A 2 m deep pool (x −3..3, z 0..8) in a floor whose top is at y = 0.
fn pool(w: &mut PlayerTestWorld) {
    w.fill(-8, -3, -8, 8, -1, 12, stone());
    w.fill(-3, -2, 0, 3, -1, 8, require("dwell:water"));
}

#[test]
fn deep_water_starts_swimming_and_buoyancy_floats() {
    let mut w = PlayerTestWorld::new();
    pool(&mut w);
    let e = w.spawn(v(0.5, 0.0, -1.0));
    w.set_input(|t, _| if t < 30 { mv(0.0, 1.0) } else { mv(0.0, 0.0) });
    w.step(ticks(4.0));
    assert!(w.c(e).swim.swimming);
    assert_eq!(w.c(e).state, State::Swimming);
    assert_eq!(w.count(e, ev::SWIM_STARTED), 1);
    assert!(
        approx(w.c(e).swim.submerged, 0.7, 0.08),
        "{}",
        w.c(e).swim.submerged
    );
}

#[test]
fn crouch_dives_and_look_direction_swimming_moves() {
    let mut w = PlayerTestWorld::new();
    pool(&mut w);
    let e = w.spawn(v(0.5, -2.0, 4.0));
    w.set_input(|t, _| {
        if t < 60 {
            input(0.0, 0.0, false, false, true, 0.0, 0.0)
        } else {
            mv(0.0, 1.0)
        }
    });
    w.step(60);
    assert!(w.c(e).swim.swimming);
    assert!(w.feet(e) < -1.9, "{}", w.feet(e));
    let z = w.pos(e).z;
    w.step(30);
    assert!(w.pos(e).z > z + 0.5);
}

#[test]
fn jumping_at_the_edge_climbs_out_of_the_pool() {
    let mut w = PlayerTestWorld::new();
    pool(&mut w);
    let e = w.spawn(v(0.5, -1.2, 7.0));
    w.set_input(|t, _| {
        if t < 60 {
            input(0.0, 1.0, false, true, false, 0.0, 0.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(ticks(3.0));
    assert!(!w.c(e).swim.swimming);
    assert!(w.count(e, ev::SWIM_ENDED) >= 1);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
    assert!(w.pos(e).z > 9.0);
}

#[test]
fn shallow_water_is_walked_through() {
    let mut w = PlayerTestWorld::new();
    w.fill(-8, -2, -8, 8, -2, 12, stone());
    w.fill(-8, -1, -8, 8, -1, 12, require("dwell:water"));
    let e = w.spawn(v(0.5, -1.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(1.0));
    assert!(!w.c(e).swim.swimming);
    assert!(
        approx(w.horizontal_speed(e), 5.0, 0.02),
        "{}",
        w.horizontal_speed(e)
    );
}
