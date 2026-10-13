//! Ported PPC suites Phase1Skeleton and Phase2Movement (PLAYER_CONTROLLER.md §10) on voxel
//! geometry, with Dwell's default config.

mod common;
use common::*;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::events;
use dwell_physics::query::GroundRef;
use dwell_physics::{DQuat, Vec3};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

#[test]
fn spawn_configures_a_locked_gravity_free_frictionless_dynamic_capsule() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::ZERO);
    w.step(1);
    let r = w.physics.rapier();
    let body = &r.bodies[w.players.body(e)];
    assert!(body.is_dynamic());
    assert_eq!(body.gravity_scale(), 0.0);
    assert!(body.activation().time_until_sleep == f64::MAX || !body.is_sleeping());
    assert!((body.mass() - 80.0).abs() < 0.08);
    let collider = &r.colliders[body.colliders()[0]];
    assert_eq!(collider.friction(), 0.0);
    let cap = collider.shape().as_capsule().expect("a capsule");
    assert!((cap.radius - 0.3).abs() < 1e-6);
    assert!((cap.half_height() + cap.radius - 0.9).abs() < 1e-6);
}

#[test]
fn stands_still_on_the_floor_without_drift() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    let start = w.pos(e);
    w.step(120);
    assert!((w.pos(e) - start).length() < 0.01, "{:?}", w.pos(e) - start);
    assert!(w.c(e).ground.grounded);
    assert_eq!(w.c(e).state, State::Idle);
}

#[test]
fn input_reaches_the_controller_and_move_is_clamped() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(Vec3::ZERO);
    w.set_input(|_, _| input(1.0, 1.0, true, false, false, 90.0, -10.0));
    w.step(3);
    let i = w.c(e).input;
    assert!(i.run);
    assert_eq!((i.look_yaw, i.look_pitch), (90.0, -10.0));
    assert!(approx((i.move_x * i.move_x + i.move_y * i.move_y).sqrt(), 1.0, 1e-5));
}

#[test]
fn a_heavy_cluster_hitting_the_player_pushes_it() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.dynamic_box(v(-2.5, 0.9, 0.5), Vec3::splat(0.5), 2000.0, v(10.0, 0.0, 0.0), true);
    w.step(30);
    assert!(w.pos(e).x > 1.0, "{:?}", w.pos(e));
}

#[test]
fn standing_on_the_floor_is_grounded_on_terrain() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.step(2);
    let g = w.c(e).ground;
    assert!(g.grounded);
    assert_eq!(g.ground, GroundRef::Terrain);
    assert!(g.slope_angle < 1.0);
    assert!(g.gap.abs() < 0.01, "{}", g.gap);
}

#[test]
fn floating_high_is_not_grounded() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 3.0, 0.5));
    w.step(2);
    assert!(!w.c(e).ground.grounded);
}

#[test]
fn too_steep_ground_is_not_walkable() {
    let mut w = PlayerTestWorld::new();
    w.boxed(v(0.0, -1.0, 0.0), v(5.0, 0.5, 5.0), euler(0.0, 0.0, 60.0), Vec3::ZERO, 0.0);
    let e = w.spawn(v(0.0, 0.5, 0.0));
    w.step(2);
    let g = w.c(e).ground;
    assert!(!g.grounded);
    assert!(matches!(g.ground, GroundRef::Tier1Body(_)));
    assert!(approx(g.slope_angle, 60.0, 0.05), "{}", g.slope_angle);
}

#[test]
fn a_low_voxel_ceiling_is_detected() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-2, 2, -2, 2, 2, 2, stone()); // underside at 2.0
    let e = w.spawn(v(0.5, 0.15, 0.5)); // head at 1.95
    w.step(1);
    assert!(w.c(e).ground.ceiling_blocked);
}

#[test]
fn an_adjacent_voxel_wall_is_detected() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(1, 0, -3, 1, 3, 3, stone()); // face at x = 1
    let e = w.spawn(v(0.65, 0.0, 0.5));
    w.step(2);
    let g = w.c(e).ground;
    assert!(g.touching_wall);
    assert!(approx(g.wall_normal.x, -1.0, 1e-5));
}

#[test]
fn walk_accelerates_to_walk_speed() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, -8.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(3);
    assert!(approx(w.horizontal_speed(e), 2.5, 0.05), "{}", w.horizontal_speed(e));
    w.step(ticks(1.0));
    assert!(approx(w.horizontal_speed(e), 5.0, 0.01), "{}", w.horizontal_speed(e));
    assert!(w.pos(e).z > -3.5);
    assert_eq!(w.c(e).state, State::Walking);
}

#[test]
fn run_reaches_run_speed() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, -12.0));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    w.step(ticks(1.0));
    assert!(approx(w.horizontal_speed(e), 8.0, 0.01));
    assert_eq!(w.c(e).state, State::Running);
}

#[test]
fn every_direction_moves_at_the_same_speed() {
    let dirs = [mv(0.0, 1.0), mv(0.0, -1.0), mv(1.0, 0.0), mv(-1.0, 0.0), mv(0.7071, 0.7071)];
    let m = PlayerControllerConfig::default().movement;
    for run in [false, true] {
        for yaw in [0.0f32, 37.0, 90.0] {
            for (d, dir) in dirs.iter().enumerate() {
                let mut w = PlayerTestWorld::new();
                w.floor_at(0, 40);
                let e = w.spawn(v(0.5, 0.0, 0.5));
                let i = dwell_physics::controller::Input { run, look_yaw: yaw, ..*dir };
                w.set_input(move |_, _| i);
                w.step(ticks(0.5));
                let start = w.pos(e);
                w.step(ticks(2.0));
                let moved = w.pos(e) - start;
                let distance = (moved.x * moved.x + moved.z * moved.z).sqrt();
                let expected = 2.0 * if run { m.run_speed } else { m.walk_speed };
                assert!(approx(distance, expected, 0.01), "run {run} yaw {yaw} dir {d}: {distance}");
            }
        }
    }
}

#[test]
fn movement_is_relative_to_camera_yaw() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(-8.0, 0.0, 0.5));
    w.set_input(|_, _| input(0.0, 1.0, false, false, false, 90.0, 0.0));
    w.step(ticks(1.0));
    assert!(w.pos(e).x > -4.0);
    assert!(approx(w.pos(e).z, 0.5, 0.1));
}

#[test]
fn releasing_input_decelerates_to_a_stop() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, -12.0));
    w.set_input(|t, _| if t < ticks(1.0) { mv(0.0, 1.0) } else { mv(0.0, 0.0) });
    w.step(ticks(1.0) + ticks(0.5));
    assert!(w.horizontal_speed(e) < 0.05);
    assert_eq!(w.c(e).state, State::Idle);
}

#[test]
fn reversing_brakes_faster_than_releasing() {
    let after = |brake: dwell_physics::controller::Input| {
        let mut w = PlayerTestWorld::new();
        w.floor();
        let e = w.spawn(v(0.5, 0.0, -12.0));
        w.set_input(move |t, _| if t < ticks(1.0) { mv(0.0, 1.0) } else { brake });
        w.step(ticks(1.0) + ticks(0.1));
        w.vel(e).z
    };
    assert!(after(mv(0.0, -1.0)) < after(mv(0.0, 0.0)) - 0.5);
}

#[test]
fn a_voxel_wall_blocks_movement() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 0, 3, 3, 3, 3, stone()); // face at z = 3
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    assert!(approx(w.pos(e).z, 2.7, 0.02), "{}", w.pos(e).z);
}

#[test]
fn diagonal_input_is_not_faster() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(-8.0, 0.0, -8.0));
    w.set_input(|_, _| mv(1.0, 1.0));
    w.step(ticks(1.0));
    assert!(approx(w.horizontal_speed(e), 5.0, 0.01));
}

#[test]
fn the_slope_limit_leaves_room_for_45_degrees() {
    assert!(PlayerControllerConfig::default().probes.max_slope_angle >= 49.9);
}

#[test]
fn walks_up_a_half_step_without_leaving_the_ground() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.half_step(-3, 0, 2, 3, 12);
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    let mut airborne = 0;
    for _ in 0..ticks(1.5) {
        w.step(1);
        airborne += i32::from(!w.c(e).ground.grounded);
    }
    assert!(w.pos(e).z > 4.0, "{:?}", w.pos(e));
    assert!(approx(w.feet(e), 0.5, 0.03), "{}", w.feet(e));
    assert_eq!(airborne, 0);
}

#[test]
fn a_full_block_needs_a_jump() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 0, 2, 3, 0, 12, stone());
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(1.5));
    assert!(w.pos(e).z < 1.75);
    assert!(w.feet(e) < 0.05);
}

#[test]
fn stepping_up_never_moves_the_player_faster_than_its_speed() {
    let m = PlayerControllerConfig::default().movement;
    for run in [false, true] {
        let limit = (if run { m.run_speed } else { m.walk_speed }) / 60.0 * 1.02;
        let (mut worst, mut worst_jump, mut onto) = (0.0f32, -1, 0);
        for jump_tick in (0..=60).step_by(2) {
            let mut w = PlayerTestWorld::new();
            w.floor_at(0, 40);
            w.fill(-3, 0, 3, 3, 0, 20, stone());
            let e = w.spawn(v(0.5, 0.0, 0.0));
            w.set_input(move |t, _| input(0.0, 1.0, run, t >= jump_tick && t < jump_tick + 3, false, 0.0, 0.0));
            let mut prev = w.pos(e);
            for _ in 0..jump_tick + ticks(1.5) {
                w.step(1);
                let p = w.pos(e);
                let moved = ((p.x - prev.x).powi(2) + (p.z - prev.z).powi(2)).sqrt();
                if moved > worst {
                    worst = moved;
                    worst_jump = jump_tick;
                }
                prev = p;
            }
            onto += i32::from(w.feet(e) > 0.95);
        }
        assert!(onto > 10, "run {run}: onto {onto}");
        assert!(worst <= limit, "run {run}: {} m/s jumping at {worst_jump}", worst * 60.0);
    }
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    w.half_step(-3, 0, 3, 3, 20);
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    let mut prev = w.pos(e);
    let mut worst = 0.0f32;
    for _ in 0..ticks(1.5) {
        w.step(1);
        worst = worst.max(w.pos(e).z - prev.z);
        prev = w.pos(e);
    }
    assert!(w.feet(e) > 0.45);
    assert!(worst * 60.0 <= m.walk_speed * 1.02, "{}", worst * 60.0);
}

#[test]
fn clearing_a_blocks_top_edge_does_not_launch() {
    let cfg = PlayerControllerConfig::default();
    for run in [false, true] {
        let (mut highest, mut at) = (0.0f32, -1);
        for jump_tick in (0..=60).step_by(3) {
            let mut w = PlayerTestWorld::new();
            w.floor_at(0, 40);
            w.fill(0, 0, 3, 0, 0, 3, stone());
            let e = w.spawn(v(0.5, 0.0, 0.0));
            w.set_input(move |t, _| input(0.0, 1.0, run, t >= jump_tick && t < jump_tick + 3, false, 0.0, 0.0));
            let apex = w.max_feet(e, jump_tick + ticks(1.5));
            if apex > highest {
                highest = apex;
                at = jump_tick;
            }
        }
        assert!(highest <= cfg.jump.height + 0.05, "run {run}: apex {highest} jumping at {at}");
    }
}

#[test]
fn one_jump_climbs_one_step_of_a_diagonal_staircase() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    for z in 0..30 {
        for x in 0..30 {
            let height = x.min(z) - 2;
            for y in 0..height {
                w.set(x, y, z, stone());
            }
        }
    }
    let e = w.spawn_yaw(v(0.5, 0.0, 0.5), 45.0);
    w.set_input(|t, _| input(0.0, 1.0, true, (20..23).contains(&t), false, 45.0, 0.0));
    let apex = w.max_feet(e, ticks(3.0));
    assert_eq!(w.count(e, events::JUMPED), 1);
    assert!(apex <= PlayerControllerConfig::default().jump.height + 0.05, "{apex}");
    assert!(approx(w.feet(e), 1.0, 0.02), "{}", w.feet(e));
}

#[test]
fn air_control_scales_airborne_acceleration() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(v(0.0, 20.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(0.25));
    assert!(approx(w.horizontal_speed(e), 2.5, 0.05), "{}", w.horizontal_speed(e));
}

#[test]
fn a_ground_kick_is_absorbed_then_decays() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(-8.0, 0.0, 0.5));
    w.kick(5, e, v(6.0, 0.0, 0.0));
    w.step(8);
    let after = w.vel(e).x;
    w.step(ticks(0.5));
    assert!(after > 4.0 && after < 6.1, "{after}");
    assert!(w.vel(e).x < 0.1);
    assert!(w.pos(e).x > -7.5);
}

#[test]
fn an_air_kick_keeps_its_momentum() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(v(0.0, 30.0, 0.0));
    w.kick(5, e, v(6.0, 0.0, 0.0));
    w.step(8 + 60);
    assert!(approx(w.vel(e).x, 6.0, 0.01), "{}", w.vel(e).x);
}

#[test]
fn air_drag_decays_an_air_kick_unity_parity() {
    let mut w = PlayerTestWorld::with_config(PlayerControllerConfig::unity_parity());
    let e = w.spawn(v(0.0, 30.0, 0.0));
    w.kick(5, e, v(6.0, 0.0, 0.0));
    w.step(8 + 60);
    assert!(approx(w.vel(e).x, 6.0 * (-0.5f32).exp(), 0.08), "{}", w.vel(e).x);
    let _ = DQuat::IDENTITY;
}
