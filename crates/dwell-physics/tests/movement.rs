//! Ported PPC suites Phase1Skeleton and Phase2Movement (PLAYER_CONTROLLER.md §10) on voxel
//! geometry, with Dwell's default config.

mod common;
use common::*;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, accelerate, events};
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
    let body = &r.bodies[w.players.body(e).rapier()];
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
    assert!(approx(
        (i.move_x * i.move_x + i.move_y * i.move_y).sqrt(),
        1.0,
        1e-5
    ));
}

#[test]
fn a_heavy_cluster_hitting_the_player_pushes_it() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.dynamic_box(
        v(-2.5, 0.9, 0.5),
        Vec3::splat(0.5),
        2000.0,
        v(10.0, 0.0, 0.0),
        true,
    );
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
    w.boxed(
        v(0.0, -1.0, 0.0),
        v(5.0, 0.5, 5.0),
        euler(0.0, 0.0, 60.0),
        Vec3::ZERO,
        0.0,
    );
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
    w.fill(-2, 2, -2, 2, 2, 2, cube_block()); // underside at 2.0
    let e = w.spawn(v(0.5, 0.15, 0.5)); // head at 1.95
    w.step(1);
    assert!(w.c(e).ground.ceiling_blocked);
}

#[test]
fn an_adjacent_voxel_wall_is_detected() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(1, 0, -3, 1, 3, 3, cube_block()); // face at x = 1
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
    assert!(
        approx(w.horizontal_speed(e), 2.5, 0.05),
        "{}",
        w.horizontal_speed(e)
    );
    w.step(ticks(1.0));
    assert!(
        approx(w.horizontal_speed(e), 5.0, 0.01),
        "{}",
        w.horizontal_speed(e)
    );
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
    let dirs = [
        mv(0.0, 1.0),
        mv(0.0, -1.0),
        mv(1.0, 0.0),
        mv(-1.0, 0.0),
        mv(
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        ),
    ];
    let m = PlayerControllerConfig::default().movement;
    for run in [false, true] {
        for yaw in [0.0f32, 37.0, 90.0] {
            for (d, dir) in dirs.iter().enumerate() {
                let mut w = PlayerTestWorld::new();
                w.floor_at(0, 40);
                let e = w.spawn(v(0.5, 0.0, 0.5));
                let i = dwell_physics::controller::Input {
                    run,
                    look_yaw: yaw,
                    ..*dir
                };
                w.set_input(move |_, _| i);
                w.step(ticks(0.5));
                let start = w.pos(e);
                w.step(ticks(2.0));
                let moved = w.pos(e) - start;
                let distance = (moved.x * moved.x + moved.z * moved.z).sqrt();
                let expected = 2.0 * if run { m.run_speed } else { m.walk_speed };
                assert!(
                    approx(distance, expected, 0.01),
                    "run {run} yaw {yaw} dir {d}: {distance}"
                );
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
    w.set_input(|t, _| {
        if t < ticks(1.0) {
            mv(0.0, 1.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(ticks(1.0) + ticks(0.5));
    assert!(w.horizontal_speed(e) < 0.05);
    assert_eq!(w.c(e).state, State::Idle);
}

#[test]
fn a_walk_stops_within_a_third_of_a_second_of_letting_go() {
    // Owner, 2026-11-02: walking friction 25 % higher (`deceleration` 12 → 15 m/s²): from the walk
    // speed (5 m/s) it stops in 1/3 s (it took 0.42 s before: red).
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.0, -12.0));
    let release = ticks(1.0);
    w.set_input(move |t, _| {
        if t < release {
            mv(0.0, 1.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    w.step(release);
    assert!(approx(w.horizontal_speed(e), 5.0, 0.05), "walking");
    w.step(ticks(0.36));
    assert!(
        w.horizontal_speed(e) < 0.05,
        "stopped: {}",
        w.horizontal_speed(e)
    );
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
    w.fill(-3, 0, 3, 3, 3, 3, cube_block()); // face at z = 3
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    assert!(approx(w.pos(e).z, 2.7, 0.02), "{}", w.pos(e).z);
}

#[test]
fn a_wall_of_leaves_is_walked_through() {
    // docs/FELLING.md (owner, 2026-10-27): "leaves should no longer be solid to a player". The
    // same wall as `a_voxel_wall_blocks_movement`, of leaves: the player walks on through it, and
    // a canopy under the feet holds no one up.
    let leaves = dwell_blocks::require("dwell:leaves");
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 0, 3, 3, 3, 3, leaves);
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    assert!(
        w.pos(e).z > 4.0,
        "walked through the leaves: {}",
        w.pos(e).z
    );
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 4, -3, 3, 4, 3, leaves); // a canopy 4 m up
    let e = w.spawn(v(0.5, 5.0, 0.5));
    w.step(ticks(2.0));
    // `pos` is the capsule's centre, 0.9 m over the feet: on the floor, not on the canopy at 5 m.
    assert!(
        w.pos(e).y < 1.5,
        "fell through the canopy to the floor: {}",
        w.pos(e).y
    );
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
    w.fill(-3, 0, 2, 3, 0, 12, cube_block());
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
            w.fill(-3, 0, 3, 3, 0, 20, cube_block());
            let e = w.spawn(v(0.5, 0.0, 0.0));
            w.set_input(move |t, _| {
                input(
                    0.0,
                    1.0,
                    run,
                    t >= jump_tick && t < jump_tick + 3,
                    false,
                    0.0,
                    0.0,
                )
            });
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
        assert!(
            worst <= limit,
            "run {run}: {} m/s jumping at {worst_jump}",
            worst * 60.0
        );
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
            w.fill(0, 0, 3, 0, 0, 3, cube_block());
            let e = w.spawn(v(0.5, 0.0, 0.0));
            w.set_input(move |t, _| {
                input(
                    0.0,
                    1.0,
                    run,
                    t >= jump_tick && t < jump_tick + 3,
                    false,
                    0.0,
                    0.0,
                )
            });
            let apex = w.max_feet(e, jump_tick + ticks(1.5));
            if apex > highest {
                highest = apex;
                at = jump_tick;
            }
        }
        assert!(
            highest <= cfg.jump.height + 0.05,
            "run {run}: apex {highest} jumping at {at}"
        );
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
                w.set(x, y, z, cube_block());
            }
        }
    }
    let e = w.spawn_yaw(v(0.5, 0.0, 0.5), 45.0);
    w.set_input(|t, _| input(0.0, 1.0, true, (20..23).contains(&t), false, 45.0, 0.0));
    let apex = w.max_feet(e, ticks(3.0));
    assert_eq!(w.count(e, events::JUMPED), 1);
    assert!(
        apex <= PlayerControllerConfig::default().jump.height + 0.05,
        "{apex}"
    );
    assert!(approx(w.feet(e), 1.0, 0.02), "{}", w.feet(e));
}

#[test]
fn air_control_scales_airborne_acceleration() {
    let mut w = PlayerTestWorld::new();
    let e = w.spawn(v(0.0, 20.0, 0.0));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(0.25));
    assert!(
        approx(w.horizontal_speed(e), 2.5, 0.05),
        "{}",
        w.horizontal_speed(e)
    );
}

/// Runs (or walks) off a 1 m ledge along +z, then in the air holds forward with run `air_run`.
/// The horizontal speed at the edge, and the slowest and fastest in the air until landing.
fn off_a_ledge_then(run: bool, air_run: bool) -> (f32, f32, f32) {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 64);
    w.fill(-4, 0, -40, 4, 1, -10, cube_block());
    let e = w.spawn(v(0.5, 2.0, -30.0));
    w.set_input(move |_, _| input(0.0, 1.0, run, false, false, 0.0, 0.0));
    while w.c(e).ground.grounded || w.pos(e).z < -9.0 {
        w.step(1);
    }
    w.set_input(move |_, _| input(0.0, 1.0, air_run, false, false, 0.0, 0.0));
    let at = w.horizontal_speed(e);
    let (mut slowest, mut fastest) = (f32::MAX, 0.0f32);
    w.step(1);
    while !w.c(e).ground.grounded {
        slowest = slowest.min(w.horizontal_speed(e));
        fastest = fastest.max(w.horizontal_speed(e));
        w.step(1);
    }
    (at, slowest, fastest)
}

#[test]
fn sprint_changes_nothing_in_the_air() {
    // Owner (2026-11-05): "Sprint shouldn't change in air control or speed … we want a true
    // physics controller." Letting go of sprint in the air steered the speed from 8 down to the
    // walk speed (5 m/s over ~0.2 s); pressing it in the air sped a walk up to 8.
    let (at, slowest, fastest) = off_a_ledge_then(true, false);
    assert!(at > 7.9, "ran off the edge at {at}");
    assert!(
        slowest > at - 0.01 && fastest < at + 0.01,
        "let go of sprint: {slowest}..{fastest} of {at} m/s"
    );
    let (at, slowest, fastest) = off_a_ledge_then(false, true);
    assert!(approx(at, 5.0, 0.05), "walked off the edge at {at}");
    assert!(
        slowest > at - 0.01 && fastest < at + 0.01,
        "pressed sprint: {slowest}..{fastest} of {at} m/s"
    );
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
    assert!(
        approx(w.vel(e).x, 6.0 * (-0.5f32).exp(), 0.08),
        "{}",
        w.vel(e).x
    );
    let _ = DQuat::IDENTITY;
}

/// A 1-block wall from z = 3 across the floor; the player walks (or runs) at it from z = 0 and
/// presses jump once at `jump_tick`, holding forward throughout. Returns the gap between the
/// capsule's front and the wall when jump was pressed, and the feet height 1.5 s later.
fn jump_at_a_block(run: bool, jump_tick: i32) -> (f32, f32) {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    w.fill(-3, 0, 3, 3, 0, 20, cube_block());
    let e = w.spawn(v(0.5, 0.0, 0.0));
    w.set_input(move |t, _| {
        input(
            0.0,
            1.0,
            run,
            t >= jump_tick && t < jump_tick + 3,
            false,
            0.0,
            0.0,
        )
    });
    w.step(jump_tick);
    let gap = 3.0 - (w.pos(e).z + PlayerControllerConfig::default().body.radius);
    w.step(ticks(1.5));
    (gap, w.feet(e))
}

#[test]
fn a_jump_always_clears_a_one_block_step() {
    // Owner (2026-10-14): "usually the player can't make a 1 block high jump". Walking up to a
    // block and then jumping, pressed against it, fell back down every time: the wall stopping the
    // drive was absorbed as an external push (Rapier's contact clustering hid the contacts), which
    // cancels air control. From up to 2.4 m away, and from against the block, every press clears it.
    for run in [false, true] {
        let mut failed = Vec::new();
        for jump_tick in 0..=70 {
            let (gap, feet) = jump_at_a_block(run, jump_tick);
            if gap <= 2.4 && feet < 0.95 {
                failed.push(format!("tick {jump_tick} gap {gap:.2} → feet {feet:.2}"));
            }
        }
        assert!(failed.is_empty(), "run {run}: {failed:?}");
    }
}

#[test]
fn crouching_against_a_wall_never_moves_the_player_backwards() {
    // Owner (2026-10-14): crouch-jumping up against a wall moved the player backwards with forward
    // held: the absorbed "push" from the wall outlasted the slower crouched drive.
    for jump in [false, true] {
        let mut w = PlayerTestWorld::new();
        w.floor_at(0, 40);
        w.fill(-3, 0, 3, 3, 2, 20, cube_block());
        let e = w.spawn(v(0.5, 0.0, 0.0));
        w.set_input(move |t, _| {
            input(
                0.0,
                1.0,
                false,
                jump && (60..63).contains(&t),
                t >= 60,
                0.0,
                0.0,
            )
        });
        w.step(60);
        let against = w.pos(e).z;
        let mut lowest = against;
        for _ in 0..ticks(1.5) {
            w.step(1);
            lowest = lowest.min(w.pos(e).z);
        }
        assert!(
            lowest > against - 0.02,
            "jump {jump}: from z {against:.3} back to {lowest:.3}"
        );
    }
}

// --- the walking rule: input only adds, up to the target (owner, 2026-11-06) --------------------

/// Run along +z for 1 s, jump on the last tick, then `air` for `seconds` in the air: the horizontal
/// velocity (x, z) at the end, and whether it stayed in the air throughout.
fn running_jump_then(air: Input, seconds: f32) -> (Vec3, bool) {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.05, 0.5));
    let go = ticks(1.0);
    w.set_input(move |t, _| {
        if t < go {
            input(0.0, 1.0, true, t == go - 1, false, 0.0, 0.0)
        } else {
            air
        }
    });
    w.step(go);
    assert!(approx(w.horizontal_speed(e), 8.0, 0.05), "ran at 8 m/s");
    let mut airborne = true;
    for _ in 0..ticks(seconds) {
        w.step(1);
        airborne &= !w.c(e).ground.grounded;
    }
    let vel = w.vel(e);
    (Vec3::new(vel.x, 0.0, vel.z), airborne)
}

#[test]
fn letting_go_in_the_air_keeps_the_speed() {
    // Owner (2026-11-06): input "shouldn't slow you down to reach the target speed". Until 0.49.17
    // air control braked a running jump with nothing held: 8 → 6.5 m/s in 0.5 s.
    let (vel, airborne) = running_jump_then(Input::default(), 0.5);
    assert!(airborne);
    assert!(approx(vel.length(), 8.0, 0.01), "{vel}");
}

#[test]
fn holding_sideways_in_the_air_turns_without_slowing_or_speeding() {
    // Until 0.49.17 air control steered toward the held direction's velocity, taking the rest away:
    // a running jump with right held lost half its forward speed in 0.5 s (8 → 4.0 m/s). Now the
    // held direction gains speed, the total stays 8 m/s (the cap: no strafe-jumping), and the
    // forward part gives way only as the velocity turns.
    let right = input(-1.0, 0.0, true, false, false, 0.0, 0.0);
    let (vel, airborne) = running_jump_then(right, 0.5);
    assert!(airborne);
    assert!(approx(vel.length(), 8.0, 0.01), "the speed kept: {vel}");
    assert!(vel.x > 2.0 && vel.z > 6.0, "turned toward +x: {vel}");
}

#[test]
fn input_never_builds_speed_past_the_target_whatever_its_direction() {
    // Strafe-jumping (Quake): input held nearly across the velocity, turned with it each tick,
    // adds speed the "along the input" test never sees. The rule caps the velocity's length.
    let wish_at = |v: Vec3, degrees: f32| {
        let a = libm::atan2f(v.x, v.z) + degrees.to_radians();
        Vec3::new(libm::sinf(a), 0.0, libm::cosf(a))
    };
    for degrees in [30.0f32, 60.0, 80.0, 89.0, 95.0] {
        let mut vel = Vec3::new(0.0, 0.0, 8.0);
        for _ in 0..ticks(5.0) {
            vel = accelerate(vel, wish_at(vel, degrees), 5.0, 10.0 / 60.0);
        }
        assert!(vel.length() <= 8.0 + 1e-4, "{degrees}°: {}", vel.length());
    }
    // Up to the target it adds; at or past it, along the input, it adds nothing and takes nothing;
    // across it, it turns the velocity and keeps its speed.
    let forward = Vec3::new(0.0, 0.0, 1.0);
    assert!(approx(
        accelerate(Vec3::ZERO, forward, 5.0, 0.2).z,
        0.2,
        1e-6
    ));
    assert!(approx(
        accelerate(forward * 4.9, forward, 5.0, 0.2).z,
        5.0,
        1e-6
    ));
    assert_eq!(
        accelerate(forward * 12.0, forward, 5.0, 0.2),
        forward * 12.0
    );
    let turned = accelerate(forward * 12.0, Vec3::new(0.6, 0.0, 0.8), 5.0, 0.2);
    assert!(
        approx(turned.length(), 12.0, 1e-4) && turned.x > 0.1,
        "{turned}"
    );
}

#[test]
fn a_kick_and_walking_do_not_stack() {
    // Until 0.49.17 the player's own speed was added on top of a knockback's: kicked at 12 m/s
    // with run held, the body went 17 m/s on the ground and 15 (10 + 5) in the air. Now holding
    // forward adds nothing to a body already faster than the target.
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 0.05, 0.5));
    w.step(5);
    w.kick(6, e, v(0.0, 0.0, 12.0));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    // The kick lands on the speed the player had (1.7 m/s): 13.7, and from there it only falls.
    w.step(2);
    let kicked = w.horizontal_speed(e);
    assert!(kicked > 13.0, "{kicked}");
    for _ in 0..ticks(1.2) {
        let before = w.horizontal_speed(e);
        w.step(1);
        assert!(
            w.horizontal_speed(e) <= before + 1e-4,
            "ground: {before} → {}",
            w.horizontal_speed(e)
        );
    }
    assert!(
        approx(w.horizontal_speed(e), 8.0, 0.05),
        "friction to the run speed"
    );
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 30.0, 0.5));
    w.kick(0, e, v(0.0, 0.0, 10.0));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    w.step(2);
    let kicked = w.horizontal_speed(e);
    w.step(ticks(1.0));
    assert!(
        kicked < 10.2 && approx(w.horizontal_speed(e), kicked, 1e-4),
        "air: {kicked} → {} (until 0.49.17: 15)",
        w.horizontal_speed(e)
    );
}

#[test]
fn a_standing_jump_still_steers_up_to_the_walk_speed() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(v(0.5, 30.0, 0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(1.0));
    assert!(
        approx(w.horizontal_speed(e), 5.0, 0.01),
        "{}",
        w.horizontal_speed(e)
    );
}
