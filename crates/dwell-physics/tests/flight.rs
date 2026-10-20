//! Creative flight (PLAYER_CONTROLLER.md §6.7): an exclusive controller mode while the fly input is
//! held. The server's flight policy is in the netcode suite.

mod common;

use common::*;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, events as ev, fly_speed_factor};
use dwell_physics::net::{dequantize_input, quantize_input};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::{InputFrame, input_buttons, players::FLY_SPEED_MAX_LEVEL};
use dwell_world::MIDPLANE_Y;

fn fly(x: f32, y: f32, run: bool, up: bool, down: bool) -> Input {
    Input {
        fly: true,
        ..input(x, y, run, up, down, 0.0, 0.0)
    }
}

#[test]
fn flying_holds_the_player_in_the_air_and_letting_go_falls() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.0, 10.0, 0.0));
    w.set_input(|t, _| {
        if t < 60 {
            fly(0.0, 0.0, false, false, false)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(60);
    assert!(w.c(e).fly.flying);
    assert_eq!(w.c(e).state, State::Flying);
    assert_eq!(w.count(e, ev::FLY_STARTED), 1);
    assert!(approx(w.feet(e), 10.0, 0.01), "{}", w.feet(e));
    w.step(ticks(2.0));
    assert!(!w.c(e).fly.flying);
    assert_eq!(w.count(e, ev::FLY_ENDED), 1);
    assert!(w.c(e).ground.grounded);
    assert!(w.feet(e).abs() < 0.05, "{}", w.feet(e));
}

#[test]
fn jump_rises_crouch_sinks_move_flies_along_the_yaw() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::new(0.0, 8.0, 0.0));
    w.set_input(|_, _| fly(0.0, 0.0, false, true, false));
    w.step(ticks(1.0));
    assert!(w.feet(e) > 15.0, "{}", w.feet(e));
    assert!(!w.c(e).crouching, "flight keeps the standing shape");
    w.set_input(|_, _| fly(0.0, 0.0, false, false, true));
    let top = w.feet(e);
    w.step(ticks(0.5));
    assert!(w.feet(e) < top - 3.0);
    w.set_input(|_, _| fly(0.0, 1.0, false, false, false)); // yaw 0: +Z
    w.step(ticks(1.0));
    assert!(w.vel(e).z > 10.0, "{}", w.vel(e));
    assert!(w.vel(e).y.abs() < 0.5, "{}", w.vel(e));
}

#[test]
fn flying_around_the_outside_through_the_midplane_to_face_b() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn_at(
        DVec3::new(
            f64::from(world_cell_x(0)),
            f64::from(MIDPLANE_Y) + 30.0,
            0.0,
        ),
        0.0,
    );
    assert_eq!(w.c(e).face, dwell_world::FaceFrame::A);
    // Down (toward the midplane) until across, then up in the new frame (away from it). The input
    // closure can't see the world, so the face is read between ticks.
    let mut face = w.c(e).face;
    let (mut flips, mut lowest) = (0, f64::from(MIDPLANE_Y));
    for _ in 0..ticks(6.0) {
        let across = w.c(e).face.is_b();
        // Raw world input: `input_for` would mirror x, which this test doesn't use.
        w.set_input(move |_, _| fly(0.0, 0.0, false, across, !across));
        w.step(1);
        if w.c(e).face != face {
            flips += 1;
            face = w.c(e).face;
        }
        lowest = lowest.min(w.players.position(&w.physics, e).y);
        assert!(w.c(e).fly.flying);
    }
    assert!(flips >= 1);
    assert!(
        lowest < f64::from(MIDPLANE_Y) - 30.0,
        "well out onto face B's side: {lowest}"
    );
}

fn speed_at(feet: f32, level: u8, run: bool) -> f32 {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::new(0.0, feet, 0.0));
    w.set_input(move |_, _| Input {
        fly_speed: level,
        ..fly(0.0, 1.0, run, false, false)
    });
    w.step(ticks(1.5));
    w.horizontal_speed(e)
}

#[test]
fn speed_grows_with_height_with_a_terrain_cap_up_to_the_top_levels() {
    let f = PlayerControllerConfig::default().fly;
    let low = speed_at(4.0, 0, true);
    assert!(
        approx(
            low,
            f.speed * f.run_factor * (1.0 + 4.0 / f.boost_height),
            0.05
        ),
        "{low}"
    );
    let band = speed_at(3000.0, 0, true);
    assert!(approx(band, f.terrain_speed, 0.02), "{band}");
    assert!(
        speed_at(20000.0, 0, true) > 10_000.0,
        "above the terrain: exponential climb"
    );
    // Far above the world the height-based speed is the top level's (the body's speed limit is
    // set from that).
    let top = f.speed * f.run_factor * fly_speed_factor(FLY_SPEED_MAX_LEVEL as u8);
    let high = speed_at(200_000.0, 0, true);
    assert!(approx(high, top, 0.02), "{high} {top}");
}

#[test]
fn flight_is_not_bounded_by_the_build_sphere() {
    // ADR 0025 D3: the sphere bounds building, not movement. Up through its top (50 km over the
    // centre) and on: no ceiling holds the player.
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::new(0.0, 49_000.0, 0.0));
    w.set_input(|_, _| fly(0.0, 0.0, false, true, false));
    w.step(ticks(3.0));
    assert!(w.c(e).fly.flying);
    assert!(w.feet(e) > 60_000.0, "{}", w.feet(e));
    // Sideways past the rim too.
    let e = w.spawn(Vec3::new(49_000.0, 1_000.0, 0.0));
    w.set_input(|_, _| Input {
        fly_speed: FLY_SPEED_MAX_LEVEL as u8,
        ..fly(1.0, 0.0, false, false, false)
    });
    w.step(ticks(3.0));
    let x = w.pos(e).x;
    assert!(!(38_000.0..=60_000.0).contains(&x), "{x}");
}

#[test]
fn the_speed_level_is_a_true_minimum_speed() {
    let f = PlayerControllerConfig::default().fly;
    assert_eq!(fly_speed_factor(0), 1.0);
    assert_eq!(fly_speed_factor(1), 2.0f32.sqrt());
    assert_eq!(fly_speed_factor(20), 1024.0);
    // The top level crosses the 100 km world in about 9 s (Phase 16: re-checked for 50 km).
    let top = fly_speed_factor(FLY_SPEED_MAX_LEVEL as u8);
    assert_eq!((FLY_SPEED_MAX_LEVEL, top), (20, 1024.0));
    assert_eq!(fly_speed_factor(255), top);
    assert!(100_000.0 / (f.speed * top) < 10.0);

    // A level slower than the height gives changes nothing (level 0 is the old behaviour).
    assert_eq!(speed_at(20000.0, 10, false), speed_at(20000.0, 0, false));
    // Above the terrain band a higher level flies faster: level 20 is 11 m/s × 2^10.
    assert!(approx(speed_at(20000.0, 20, false), f.speed * 1024.0, 0.05));
    // Near the ground a low level helps: level 8 is 11 m/s × 16.
    assert!(approx(speed_at(4.0, 8, false), f.speed * 16.0, 0.05));
    // The band's cap limits only the height-based speed: the level's minimum holds there too...
    assert!(approx(speed_at(100.0, 20, false), f.speed * 1024.0, 0.05));
    assert!(approx(speed_at(3000.0, 20, false), f.speed * 1024.0, 0.05));
    // ...while a level under the cap leaves the capped height-based speed as it was.
    assert!(approx(speed_at(3000.0, 8, false), f.terrain_speed, 0.02));
}

#[test]
fn the_speed_level_travels_in_the_buttons_and_is_capped() {
    let mut i = Input {
        fly: true,
        fly_speed: 17,
        ..Input::default()
    };
    let back = dequantize_input(&quantize_input(&i, 1));
    assert_eq!(back.fly_speed, 17);
    assert!(back.fly);
    i.fly_speed = 200;
    assert_eq!(
        dequantize_input(&quantize_input(&i, 1)).fly_speed,
        FLY_SPEED_MAX_LEVEL as u8
    );
    let f = InputFrame {
        buttons: input_buttons::FLY_SPEED,
        ..InputFrame::default()
    }; // level 63
    assert_eq!(dequantize_input(&f).fly_speed, FLY_SPEED_MAX_LEVEL as u8);
}

#[test]
fn diving_at_the_bands_top_speed_stops_on_the_ground() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.0, 600.0, 0.0));
    w.set_input(|_, _| fly(0.0, 0.0, true, false, true));
    w.step(ticks(4.0));
    assert!(w.feet(e).abs() < 0.05, "{}", w.feet(e));
    assert!(w.c(e).fly.flying);
}
