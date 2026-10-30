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
#[test]
fn players_walk_through_tall_grass() {
    // A cross-look plant has shape empty in its data: no collider, nothing to stand on.
    let grass = require("dwell:tall_grass");
    assert!(!dwell_blocks::info(grass).solid);
    let walk = |m| {
        let mut w = PlayerTestWorld::new();
        w.floor();
        w.fill(-2, 0, 1, 2, 1, 3, m);
        let e = w.spawn(v(0.5, 0.0, -0.5));
        w.set_input(|_, _| mv(0.0, 1.0));
        w.step(90);
        (w.pos(e), w.c(e).climb.climbing)
    };
    let (through, climbing) = walk(grass);
    assert!(through.z > 5.0, "walked through: {through:?}");
    assert!(through.y < 1.5, "stayed on the floor: {through:?}");
    assert!(!climbing);
    // The same patch as stone stops the player.
    let (stopped, _) = walk(cube_block());
    assert!(stopped.z < 1.0, "{stopped:?}");
}

fn ladder_world() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-2, 0, 2, 2, 2, 12, cube_block());
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

/// Off a ladder but still in its cells, a jump grabs it again (owner, 2026-10-21: before, the
/// column let go of was not re-grabbed until the player left it, jump or not).
#[test]
fn a_jump_in_a_ladders_cells_after_getting_off_grabs_it_again() {
    for hold in [true, false] {
        let mut w = ladder_world();
        let e = w.spawn(v(0.5, 0.0, -0.5));
        // Up, then down to the floor looking down (it lets go there), still pushing into the
        // ladder or standing, then jump.
        w.set_input(move |t, _| {
            let (y, pitch) = if t < 50 {
                (1.0, 0.0)
            } else if t < 50 + ticks(1.5) {
                (1.0, -45.0)
            } else {
                (if hold { 1.0 } else { 0.0 }, 0.0)
            };
            let jump = (50 + ticks(2.0)..50 + ticks(2.0) + 2).contains(&t);
            input(0.0, y, false, jump, false, 0.0, pitch)
        });
        w.step(50 + ticks(2.0) - 5);
        assert_eq!(w.count(e, ev::CLIMB_ENDED), 1, "hold {hold}");
        assert!(!w.c(e).climb.climbing && w.c(e).ground.grounded);
        assert!(
            w.pos(e).z > 1.0,
            "still in the ladder's cells: {:?}",
            w.pos(e)
        );
        // Walking into it doesn't grab it: the column let go of stays let go of.
        assert_eq!(w.count(e, ev::CLIMB_STARTED), 1);
        w.step(10);
        assert_eq!(
            w.count(e, ev::CLIMB_STARTED),
            2,
            "hold {hold}: the jump grabs it"
        );
        if hold {
            w.step(20);
            assert!(w.c(e).climb.climbing, "and climbs it");
            assert!(w.feet(e) > 0.5, "{}", w.feet(e));
        } else {
            w.step(20);
            assert!(w.c(e).climb.climbing, "and holds it, standing still");
        }
    }
}

/// A jump off a ladder (pressed while climbing) is not one that grabs it again: tapping jump
/// again as the player leaves, or on landing, doesn't pull them back on (which could leave them
/// stuck on it).
#[test]
fn jumping_off_near_the_floor_and_tapping_jump_again_does_not_re_grab() {
    let mut w = ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    // Grab it at the floor (holding still), jump off, then tap jump every few ticks.
    w.set_input(|t, _| {
        let y = if t < 20 { 1.0 } else { 0.0 };
        let jump = t == 30 || (t > 32 && t % 4 == 0);
        input(0.0, y, false, jump, false, 0.0, 0.0)
    });
    w.step(31);
    assert!(!w.c(e).climb.climbing);
    assert_eq!(w.count(e, ev::CLIMB_ENDED), 1);
    w.step(ticks(1.5));
    assert_eq!(w.count(e, ev::CLIMB_STARTED), 1, "not re-grabbed");
    assert!(w.pos(e).z < 1.0, "left the ladder: {:?}", w.pos(e));
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
    w.fill(-8, -3, -8, 8, -1, 12, cube_block());
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
    w.fill(-8, -2, -8, 8, -2, 12, cube_block());
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

/// Regression (owner, 2026-10-29): walking forward off a ½ m ledge onto the one-cell ladder on its
/// side, looking down the whole time, climbed down it and never let go at the bottom. A ½ m
/// descent never leaves the ground, so the climb never saw a landing to let go on.
#[test]
fn walking_down_a_half_metre_ladder_lets_go_at_the_bottom() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    // A ½ m ledge (one row of cells) from z = 2 on, a ladder cell on its −z side.
    for x in -2..=2 {
        for z in 2..=12 {
            w.set_cell(x, 0, z, cube_block());
        }
    }
    w.set_cell(0, 0, 1, require("dwell:ladder[facing=north]"));
    let e = w.spawn_yaw(v(0.5, 0.5, 3.0), 180.0);
    w.set_input(|_, _| look(0.0, 1.0, 180.0, -45.0));
    w.step(ticks(3.0));
    assert!(
        !w.c(e).climb.climbing,
        "still on the ladder at {:?}",
        w.pos(e)
    );
    assert!(approx(w.feet(e), 0.0, 0.01), "on the floor: {}", w.feet(e));
    assert!(
        w.pos(e).z < 0.5,
        "walked on past the ladder: {:?}",
        w.pos(e)
    );
}

/// The ladder grabs only a body within `climb.grab_reach` of its plate (owner, 2026-10-29: "the
/// pull … should only stick out a little from the ladder itself"), not one anywhere in its cell:
/// standing in the cell well out from the plate holds the floor; walking into the plate grabs.
/// Red before: the whole cell grabbed.
#[test]
fn a_ladder_grabs_only_near_its_plate() {
    let mut w = ladder_world();
    // The plate is on the cell's +z side (z ≈ 1.9 to 2.0); the body's front at z 1.5.
    let e = w.spawn(v(0.5, 0.0, 1.2));
    w.step(30);
    assert!(!w.c(e).climb.climbing, "grabbed from {:?}", w.pos(e));
    assert!(w.c(e).ground.grounded);
    // Walking on into it grabs it.
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(30);
    assert!(w.c(e).climb.climbing, "not grabbed at {:?}", w.pos(e));
}

/// Standing at the very top of a ladder — on the ledge's edge, the body over the ladder's column —
/// holds the ledge (owner, 2026-10-29: "be able to stand at the top of the ladder").
#[test]
fn standing_at_the_ledges_edge_over_a_ladder_does_not_grab_it() {
    let mut w = ladder_world();
    let e = w.spawn_yaw(v(0.5, 3.0, 2.05), 180.0);
    w.step(60);
    assert!(!w.c(e).climb.climbing, "grabbed at {:?}", w.pos(e));
    assert!(w.c(e).ground.grounded);
    assert!(approx(w.feet(e), 3.0, 0.01), "{}", w.feet(e));
}

/// A wall 5 m tall with a ladder up its −z side to 3 m: the ladder ends partway up.
fn short_ladder_world() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-2, 0, 2, 2, 4, 12, cube_block());
    w.fill(0, 0, 1, 0, 2, 1, require("dwell:ladder[facing=north]"));
    w
}

/// The top of a ladder is solid to stand on (owner, 2026-10-29: "stand on top of the ladder … it's
/// solid on top"; only the ladder itself, its plate's top): climbed to the top of a ladder ending
/// partway up a wall, the player lets go and stands on it, holding there without being pulled back
/// on. Red before: with nothing under them they fell to the floor.
#[test]
fn a_ladders_top_is_solid_to_stand_on() {
    let mut w = short_ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| {
        if t < ticks(2.5) {
            mv(0.0, 1.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(ticks(4.0));
    assert!(!w.c(e).climb.climbing, "still climbing at {:?}", w.pos(e));
    assert!(w.c(e).ground.grounded, "not standing at {:?}", w.pos(e));
    assert!(
        approx(w.feet(e), 3.0, 0.02),
        "on the ladder's top: {}",
        w.feet(e)
    );
    // Standing there: no pull back onto it.
    w.step(60);
    assert!(!w.c(e).climb.climbing);
    assert!(approx(w.feet(e), 3.0, 0.02), "{}", w.feet(e));
}

/// From the top of a ladder, looking down and walking forward (out over it) climbs down it.
#[test]
fn from_a_ladders_top_looking_down_and_walking_forward_climbs_down() {
    let mut w = short_ladder_world();
    let e = w.spawn(v(0.5, 0.0, -0.5));
    w.set_input(|t, _| {
        if t < ticks(2.5) {
            mv(0.0, 1.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(ticks(4.0));
    assert!(approx(w.feet(e), 3.0, 0.02), "on top first: {}", w.feet(e));
    // Turn round (yaw 180: facing −z, out over the ladder), look down, walk forward.
    w.set_input(|_, _| look(0.0, 1.0, 180.0, -45.0));
    let mut fastest_fall = 0.0f32;
    for _ in 0..ticks(3.0) {
        w.step(1);
        if !w.c(e).climb.climbing && !w.c(e).ground.grounded {
            fastest_fall = fastest_fall.max(-w.vel(e).y);
        }
    }
    assert!(
        w.count(e, ev::CLIMB_STARTED) >= 2,
        "grabbed it again on the way down"
    );
    assert!(
        approx(w.feet(e), 0.0, 0.02),
        "down at the floor: {}",
        w.feet(e)
    );
    assert!(fastest_fall < 4.0, "climbed, not fell: {fastest_fall}");
}
