//! Gravity per grid and far flight (MOON_WORLD.md §7, M-7): a player on a grid with a lower surface
//! gravity (the moon's ⅙) takes off at the same speed and rises 1/scale as high; far from every
//! grid the server raises the flight speed. Both faces through the mirror suite
//! (`DWELL_FACE=b cargo test -p dwell-physics`).

mod common;

use common::*;
use dwell_physics::Vec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::Input;

const MOON: f32 = 0.1667;

/// The highest the feet rise over the floor after one jump, under `scale` × the config's gravity.
fn jump_height(scale: f32) -> (f32, i32) {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.players.set_gravity_scale(e, scale);
    w.step(ticks(0.5));
    let ground = w.feet(e);
    let start = w.tick;
    w.set_input(move |t, _| input(0.0, 0.0, false, t - start < 3, false, 0.0, 0.0));
    let mut peak = ground;
    let mut landed = None;
    for t in 0..ticks(12.0) {
        w.step(1);
        peak = peak.max(w.feet(e));
        if t > 5 && landed.is_none() && w.c(e).ground.grounded {
            landed = Some(t);
        }
    }
    (peak - ground, landed.expect("lands"))
}

#[test]
fn a_jump_under_a_sixth_of_gravity_rises_six_times_as_high_and_lasts_six_times_as_long() {
    let cfg = PlayerControllerConfig::default();
    let (home, home_air) = jump_height(1.0);
    let (moon, moon_air) = jump_height(MOON);
    // Home: the config's jump height.
    assert!(approx(home, cfg.jump.height, 0.06), "home {home} m");
    // The moon: the same take-off speed, ⅙ g: 1/0.1667 × the height (v² / 2g).
    let want = cfg.jump.height / MOON;
    assert!(
        (moon - want).abs() < 0.03 * want,
        "moon {moon} m, want {want} (home {home})"
    );
    let ratio = moon_air as f32 / home_air as f32;
    assert!((ratio - 1.0 / MOON).abs() < 0.4, "air time ×{ratio}");
}

#[test]
fn gravity_scale_one_is_the_config_to_the_bit() {
    // Setting the home grid's scale changes nothing.
    let run = |set: bool| {
        let mut w = PlayerTestWorld::new();
        w.floor_at(0, 40);
        let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
        if set {
            w.players.set_gravity_scale(e, 1.0);
        }
        w.set_input(|t, _| input(0.3, 1.0, t % 50 < 25, t % 40 == 0, false, t as f32, 0.0));
        (0..ticks(4.0))
            .map(|_| {
                w.step(1);
                w.pos(e).to_array().map(f32::to_bits)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(false), run(true));
}

#[test]
fn far_flight_raises_the_flight_speed_and_zero_hands_back_the_sliders() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::new(0.0, 50.0, 0.0));
    w.set_input(|_, _| Input {
        fly: true,
        ..mv(0.0, 1.0)
    });
    w.step(ticks(2.0));
    let normal = w.horizontal_speed(e);
    assert!(normal < 30.0, "slider speed {normal}");
    let physics = &mut w.physics;
    w.players.set_far_fly_speed(physics, e, 12_000.0);
    w.step(ticks(4.0));
    let far = w.horizontal_speed(e);
    assert!(far > 11_000.0, "far flight {far} m/s");
    let physics = &mut w.physics;
    w.players.set_far_fly_speed(physics, e, 0.0);
    w.step(ticks(4.0));
    assert!(
        w.horizontal_speed(e) < normal * 1.2,
        "slows back to the slider's"
    );
}
