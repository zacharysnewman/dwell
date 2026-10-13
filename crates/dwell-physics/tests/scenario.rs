//! Ported PPC suites CharacterStacking, GroundedConsistency, StepSmoothness, Phase8Hardening and
//! GoldenTrace on voxel geometry, plus Dwell additions (doorways, auto-jump, edge guard, voxel
//! edits under the player, chunk seams), the 64-player performance gate, and the golden trace,
//! which is also the native↔WASM divergence check: it holds the trace bit-exactly in the origin
//! frame, and CI runs it under `wasm32-wasip1` too (ADR 0010).

mod common;

use std::fmt::Write as _;

use common::*;
use dwell_blocks::require;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, events as ev};
use dwell_physics::query::GroundRef;
use dwell_physics::{DQuat, Vec3};

// --- stacking --------------------------------------------------------------------------------

/// Player 0 stands on the floor, player 1 on player 0's head; player 0 strafes right (−X) from
/// t = 30.
fn stack(carried: bool) -> (PlayerTestWorld, PlayerHandle, PlayerHandle) {
    let mut config = PlayerControllerConfig::default();
    config.movement.carried_by_characters = carried;
    let mut w = PlayerTestWorld::with_config(config);
    w.floor();
    let bottom = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    let top = w.spawn(Vec3::new(0.5, 1.8, 0.5));
    w.set_input(move |t, h| {
        if h == bottom && t >= 30 {
            mv(1.0, 0.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    (w, bottom, top)
}

#[test]
fn standing_on_a_head_is_grounded_on_that_player() {
    let (mut w, _, top) = stack(false);
    w.step(25);
    assert!(w.c(top).ground.grounded);
    assert!(matches!(w.c(top).ground.ground, GroundRef::Player(_)));
    assert!(approx(w.feet(top), 1.8, 0.02), "{}", w.feet(top));
}

#[test]
fn the_player_below_walks_out_from_under_you() {
    let (mut w, bottom, top) = stack(false);
    w.step(60);
    assert!(w.pos(bottom).x < 0.0);
    assert!(approx(w.pos(top).x, 0.5, 0.1), "{}", w.pos(top));
}

#[test]
fn opting_in_carries_you_along() {
    let (mut w, _, top) = stack(true);
    w.step(60);
    assert!(w.pos(top).x < 0.0, "{}", w.pos(top));
}

// --- grounded consistency --------------------------------------------------------------------

#[test]
fn state_is_jumping_from_the_take_off_tick() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.set_input(|t, _| input(0.0, 0.0, false, t == 10, false, 0.0, 0.0));
    w.step(10);
    assert_eq!(w.count(e, ev::JUMPED), 0);
    w.step(1);
    assert_eq!(w.count(e, ev::JUMPED), 1);
    assert!(!w.c(e).ground.grounded);
    assert_eq!(w.c(e).state, State::Jumping);
}

#[test]
fn jumping_off_a_moving_platform_stops_its_carry_at_once() {
    let mut w = PlayerTestWorld::new();
    w.boxed(
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(20.0, 0.5, 20.0),
        DQuat::IDENTITY,
        Vec3::new(3.0, 0.0, 0.0),
        0.0,
    );
    let e = w.spawn(Vec3::ZERO);
    w.set_input(|t, _| input(0.0, 0.0, false, t == 60, false, 0.0, 0.0));
    w.step(61); // take-off tick: the jump inherits the platform's motion
    assert!(
        approx(w.c(e).platform.base_velocity.x, 3.0, 0.03),
        "{}",
        w.c(e).platform.base_velocity
    );
    w.step(1);
    assert_eq!(w.c(e).platform.base_velocity.length(), 0.0);
}

// --- step smoothness -------------------------------------------------------------------------

#[derive(Default, Debug)]
struct StairRun {
    arrived: bool,
    seconds: f32,
    max_rise: f32,
    max_drop: f32,
    final_feet: f32,
    airborne: i32,
}

/// `steps` steps of `tread` cells along +Z from z = 2, then a landing; 0.5 m steps (half steps are
/// static boxes) or 1 m ones. Up: from the floor walking +Z; down: from the landing walking −Z.
fn run_stairs(steps: i32, tread: i32, up: bool, run: bool, halves_only: bool) -> StairRun {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 24);
    let rise = if halves_only { 0.5 } else { 1.0 };
    for i in 0..steps {
        for z in 2 + tread * i..2 + tread * steps + 4 {
            // Step i is i + 1 half-cells high: full cubes below, a half step on top when odd.
            let halves = if halves_only { i + 1 } else { 2 * (i + 1) };
            for h in 0..halves / 2 {
                w.fill(-3, h, z, 3, h, z, stone());
            }
            if halves % 2 == 1 {
                w.half_step(-3, halves / 2, z, 3, z);
            }
        }
    }
    let top = rise * steps as f32;
    let end = 2.0 + (tread * steps) as f32;
    let (start_z, target_z) = if up {
        (0.0, end + 1.0)
    } else {
        (end + 1.5, 0.5)
    };
    let e = w.spawn(Vec3::new(0.5, if up { 0.0 } else { top }, start_z));
    let arrived = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = arrived.clone();
    w.set_input(move |_, _| {
        if flag.get() {
            mv(0.0, 0.0)
        } else {
            input(
                0.0,
                if up { 1.0 } else { -1.0 },
                run,
                false,
                false,
                0.0,
                0.0,
            )
        }
    });
    w.step(3);
    let mut r = StairRun::default();
    let mut previous = w.feet(e);
    for tick in 1..=240 {
        if arrived.get() {
            break;
        }
        w.step(1);
        let feet = w.feet(e);
        r.max_rise = r.max_rise.max(feet - previous);
        r.max_drop = r.max_drop.max(previous - feet);
        previous = feet;
        if !w.c(e).ground.grounded {
            r.airborne += 1;
        }
        let z = w.pos(e).z;
        if if up { z >= target_z } else { z <= target_z } {
            arrived.set(true);
            r.arrived = true;
            r.seconds = tick as f32 / 60.0;
        }
    }
    w.step(60);
    r.final_feet = w.feet(e);
    r
}

#[test]
fn half_step_stairs_up_and_down_stay_grounded_at_full_speed() {
    for up in [true, false] {
        for run in [false, true] {
            let r = run_stairs(4, 1, up, run, true);
            let ctx = format!("up {up} run {run}: {r:?}");
            assert!(r.arrived, "{ctx}");
            assert_eq!(r.airborne, 0, "{ctx}");
            assert!(
                approx(r.final_feet, if up { 2.0 } else { 0.0 }, 0.01),
                "{ctx}"
            );
            let distance = 7.0; // up: 2 + 4 + 1 m; down: 4 + 1.5 + 1.5 m
            assert!(
                r.seconds <= distance / if run { 8.0 } else { 5.0 } + 0.2,
                "{ctx}"
            );
        }
    }
}

#[test]
fn stepping_down_a_stairs_corner_gains_no_momentum() {
    // Diagonally down half-block stairs: the ground snap drives the capsule into the corner of the
    // step being left, and the contact's sideways push-out is not a kick.
    for x in [-9.5f32, -8.5, -7.2] {
        for yaw in [160.0f32, 180.0, 200.0, 220.0] {
            for run in [false, true] {
                let mut w = PlayerTestWorld::with(
                    PlayerControllerConfig::default(),
                    dwell_world::flat_generator(),
                );
                // Half, block, block and a half, two blocks: 0.5 m per step (x −10..−8, z 6..10).
                w.half_step(-10, 0, 6, -8, 6);
                w.fill(-10, 0, 7, -8, 0, 10, stone());
                w.half_step(-10, 1, 8, -8, 8);
                w.fill(-10, 1, 9, -8, 1, 10, stone());
                let e = w.spawn_yaw(Vec3::new(x, 2.0, 10.5), yaw);
                w.set_input(move |_, _| input(0.0, 1.0, run, false, false, yaw, 0.0));
                let (mut external, mut driven) = (0.0f32, 0.0f32);
                for _ in 0..90 {
                    w.step(1);
                    external = external.max(w.c(e).horizontal.external.length());
                    driven = driven.max(w.c(e).horizontal.contribution.length());
                }
                let ctx = format!("x {x} yaw {yaw} run {run}: external {external} driven {driven}");
                assert!(external < 0.05, "{ctx}");
                assert!(driven <= if run { 8.0 } else { 5.0 } + 0.01, "{ctx}");
            }
        }
    }
}

#[test]
fn a_one_metre_ledge_is_a_fall_not_a_step() {
    let r = run_stairs(1, 3, false, false, false);
    assert!(r.airborne > 5, "{r:?}");
    assert!(approx(r.final_feet, 0.0, 0.01), "{r:?}");
}

// --- voxel geometry --------------------------------------------------------------------------

fn doorway() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-3, 0, 4, 3, 3, 4, stone());
    w.fill(0, 0, 4, 0, 1, 4, air()); // doorway at x = 0
    w.set_input(|_, _| mv(0.0, 1.0));
    w
}

#[test]
fn fits_through_a_one_by_two_doorway() {
    let mut w = doorway();
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.0));
    w.step(ticks(2.0));
    assert!(w.pos(e).z > 7.0, "{}", w.pos(e));
    assert!(approx(w.feet(e), 0.0, 0.01), "{}", w.feet(e));
}

#[test]
fn a_doorway_still_blocks_a_player_aimed_at_its_frame() {
    let mut w = doorway();
    let e = w.spawn(Vec3::new(1.2, 0.0, 0.0)); // lined up with the wall beside the doorway
    w.step(ticks(1.5));
    assert!(w.pos(e).z < 3.75, "{}", w.pos(e));
}

#[test]
fn walking_across_chunk_seams_in_every_direction_never_loses_the_ground() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    // Diagonal and axis runs that cross the x = ±32 and z = ±32 seams.
    w.set_input(|t, _| input(0.0, 1.0, true, false, false, ((t / 120) * 45) as f32, 0.0));
    let (mut airborne, mut max_lift) = (0, 0.0f32);
    for _ in 0..120 * 8 {
        w.step(1);
        if !w.c(e).ground.grounded {
            airborne += 1;
        }
        max_lift = max_lift.max(w.feet(e).abs());
    }
    assert_eq!(airborne, 0);
    assert!(
        max_lift < 0.003,
        "sub-millimetre ripples at most: {max_lift}"
    );
}

#[test]
fn a_block_removed_under_the_feet_starts_a_fall() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(-10, 16);
    w.fill(0, -1, 0, 0, -1, 0, stone());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.step(10);
    assert!(w.c(e).ground.grounded);
    w.remove(0, -1, 0);
    w.step(10);
    assert!(!w.c(e).ground.grounded);
    assert!(w.vel(e).y < -1.0);
    w.step(ticks(2.0));
    assert!(approx(w.feet(e), -10.0, 0.002), "{}", w.feet(e));
}

#[test]
fn a_block_placed_in_front_is_collided_with_in_the_same_tick() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(5);
    w.fill(-2, 0, 3, 2, 2, 3, stone());
    w.step(ticks(1.5));
    assert!(approx(w.pos(e).z, 2.7, 0.01), "{}", w.pos(e));
}

#[test]
fn auto_jump_climbs_a_one_block_step_when_enabled() {
    for auto_jump in [false, true] {
        let mut w = PlayerTestWorld::with_config(if auto_jump {
            PlayerControllerConfig::touch()
        } else {
            PlayerControllerConfig::default()
        });
        w.floor();
        w.fill(-3, 0, 3, 3, 0, 10, stone());
        let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
        w.set_input(|_, _| mv(0.0, 1.0));
        w.step(ticks(2.0));
        if auto_jump {
            assert!(approx(w.feet(e), 1.0, 0.01), "{}", w.feet(e));
            assert!(w.pos(e).z > 5.0);
        } else {
            assert!(approx(w.feet(e), 0.0, 0.01), "{}", w.feet(e));
        }
    }
}

#[test]
fn auto_jump_does_not_jump_at_a_two_block_wall() {
    let mut w = PlayerTestWorld::with_config(PlayerControllerConfig::touch());
    w.floor();
    w.fill(-3, 0, 3, 3, 1, 3, stone());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(1.5));
    assert_eq!(w.count(e, ev::JUMPED), 0);
}

#[test]
fn edge_guard_keeps_a_crouched_player_on_a_ledge() {
    for guard in [false, true] {
        let mut config = PlayerControllerConfig::default();
        config.movement.edge_guard = guard;
        let mut w = PlayerTestWorld::with_config(config);
        w.floor_at(-5, 16);
        w.fill(-4, -1, -4, 3, -1, 2, stone()); // the ledge ends at z = 3
        let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
        w.set_input(|_, _| input(0.0, 1.0, false, false, true, 0.0, 0.0));
        w.step(ticks(3.0));
        if guard {
            assert!(approx(w.feet(e), 0.0, 0.01), "{}", w.feet(e));
            let z = w.pos(e).z;
            assert!(
                z > 2.9 && z < 3.3,
                "leans out as far as the probe ring allows: {z}"
            );
        } else {
            assert!(w.feet(e) < -4.0);
        }
    }
}

// --- the five-player scenario (PPC Phase8Hardening / GoldenTrace) ----------------------------

const SCENARIO_TICKS: i32 = 600;
const SCENARIO_PLAYERS: usize = 5;

/// Floor, ramp, half step, crawlspace roof, ladder to a ledge, pool, a moving and turning platform
/// and an explosion at tick 300.
fn scenario_world() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 24);
    w.boxed(
        Vec3::new(-6.0, 0.0, 6.0),
        Vec3::new(2.0, 0.5, 5.0),
        euler(-20.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    ); // ramp
    w.half_step(4, 0, 6, 7, 10);
    w.fill(-1, 1, 7, 1, 1, 9, stone()); // crawlspace roof
    w.fill(10, 0, 2, 14, 2, 6, stone()); // ledge
    w.fill(12, 0, 1, 12, 2, 1, require("dwell:ladder[facing=north]"));
    w.fill(-16, -1, -12, -10, -1, -6, air()); // pool pit
    w.fill(-16, -3, -12, -10, -2, -6, stone());
    w.fill(-15, -2, -11, -11, -1, -7, require("dwell:water"));
    w.boxed(
        Vec3::new(-12.0, 0.25, 0.0),
        Vec3::new(2.0, 0.25, 2.0),
        DQuat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
        30.0,
    );
    w.explosion(300, Vec3::new(0.0, 1.0, 6.0), 8.0, 10.0, 0.5);
    w.spawn(Vec3::new(-6.0, 0.0, 0.0)); // ramp
    w.spawn(Vec3::new(0.5, 0.0, 4.0)); // crawlspace
    w.spawn(Vec3::new(12.5, 0.0, -1.0)); // ladder
    w.spawn(Vec3::new(-12.0, 0.5, 0.0)); // platform
    w.spawn(Vec3::new(5.5, 0.0, 3.0)); // half step
    w
}

/// Different deterministic input scripts per player, exercising every action.
fn scenario_input(tick: i32, player: PlayerHandle) -> Input {
    let p = player as i32;
    let yaw = ((tick * (p + 1)) % 360) as f32;
    match (tick / 40 + p) % 6 {
        0 => input(
            0.0,
            1.0,
            false,
            false,
            false,
            if p == 2 { 0.0 } else { yaw },
            0.0,
        ),
        1 => input(1.0, 1.0, true, false, false, yaw, 0.0),
        2 => input(0.0, 1.0, false, tick % 40 < 3, false, 0.0, 0.0),
        3 => input(-1.0, 0.0, false, false, true, 0.0, 0.0),
        4 => input(0.0, 1.0, false, false, false, 0.0, -45.0),
        _ => input(0.0, 0.0, false, tick % 7 == 0, false, 0.0, 0.0),
    }
}

/// One row per tick and player: tick, player, state, then position and velocity (local frame) as
/// shortest round-trip decimals, so equal text is equal bits.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    tick: i32,
    player: u32,
    state: i32,
    values: [f32; 6],
}

fn record_trace() -> Vec<Row> {
    let mut w = scenario_world();
    w.set_input(scenario_input);
    let mut rows = Vec::new();
    for tick in 0..SCENARIO_TICKS {
        w.step(1);
        for h in w.players.handles() {
            let (p, v) = (w.pos(h), w.vel(h));
            rows.push(Row {
                tick,
                player: h,
                state: w.c(h).state as i32,
                values: [p.x, p.y, p.z, v.x, v.y, v.z],
            });
        }
    }
    rows
}

fn golden_path() -> String {
    format!("{}/tests/golden/scenario.trace", env!("CARGO_MANIFEST_DIR"))
}

fn format_trace(rows: &[Row]) -> String {
    let mut s = String::new();
    for r in rows {
        let [px, py, pz, vx, vy, vz] = r.values;
        writeln!(
            s,
            "{} {} {} {px:?} {py:?} {pz:?} {vx:?} {vy:?} {vz:?}",
            r.tick, r.player, r.state
        )
        .unwrap();
    }
    s
}

fn parse_trace(text: &str) -> Vec<Row> {
    text.lines()
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let v = |i: usize| f[i].parse::<f32>().expect("float");
            Row {
                tick: f[0].parse().unwrap(),
                player: f[1].parse().unwrap(),
                state: f[2].parse().unwrap(),
                values: [v(3), v(4), v(5), v(6), v(7), v(8)],
            }
        })
        .collect()
}

#[test]
fn the_five_player_scenario_is_deterministic_across_runs() {
    let (a, b) = (record_trace(), record_trace());
    assert_eq!(a.len(), SCENARIO_TICKS as usize * SCENARIO_PLAYERS);
    assert!(a == b);
}

#[test]
fn the_five_player_scenario_matches_the_golden_trace() {
    let actual = record_trace();
    let path = golden_path();
    if std::env::var("DWELL_UPDATE_GOLDEN").as_deref() == Ok("1") {
        assert!(
            origin_x() == 0 && !face_b(),
            "record the golden in the origin frame"
        );
        std::fs::write(&path, format_trace(&actual)).expect("write golden");
        return;
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {path}; run with DWELL_UPDATE_GOLDEN=1"));
    let expected = parse_trace(&text);
    assert_eq!(expected.len(), actual.len());
    if origin_x() == 0 && !face_b() {
        // Bit-exact: natively this pins the behaviour, under wasm32-wasip1 it is the native↔WASM
        // divergence check (enhanced-determinism, libm).
        for (e, a) in expected.iter().zip(&actual) {
            assert!(
                e == a,
                "diverged at tick {} player {}:\n  golden {e:?}\n  actual {a:?}",
                e.tick,
                e.player
            );
        }
        return;
    }
    // Far from the origin and on face B the world coordinates differ, so positions round
    // differently: the controller's decisions (every state) must match, positions within a bound.
    let (mut worst, mut worst_row) = (0.0f32, String::new());
    for (e, a) in expected.iter().zip(&actual) {
        assert_eq!((e.tick, e.player), (a.tick, a.player));
        assert_eq!(
            e.state, a.state,
            "state differs at tick {} player {}: {e:?} vs {a:?}",
            e.tick, e.player
        );
        for k in 0..3 {
            let d = (e.values[k] - a.values[k]).abs();
            if d > worst {
                worst = d;
                worst_row = format!("{e:?} vs {a:?}");
            }
        }
    }
    assert!(
        worst <= 0.001,
        "max deviation from golden: {worst} m: {worst_row}"
    );
}

// --- performance -----------------------------------------------------------------------------

#[test]
fn sixty_four_players_controller_passes_cost_under_1_ms_per_tick() {
    const PLAYERS: i32 = 64;
    const TICKS: i32 = 300;
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 32);
    for p in 0..PLAYERS {
        w.spawn(Vec3::new(
            (p % 8) as f32 * 3.0 - 12.0,
            0.0,
            (p / 8) as f32 * 3.0 - 12.0,
        ));
    }
    w.set_input(scenario_input);
    w.step(30); // warm up (terrain collision built)
    w.input = None;
    let mut controller = std::time::Duration::ZERO;
    for _ in 0..TICKS {
        for h in w.players.handles() {
            let i = w.input_for(scenario_input(w.tick, h));
            w.players.set_input(h, i);
        }
        let start = std::time::Instant::now();
        w.players.tick(&mut w.world, &mut w.physics);
        controller += start.elapsed();
        w.physics.step(1.0 / 60.0);
        w.tick += 1;
    }
    let per_tick = controller.as_secs_f64() * 1000.0 / f64::from(TICKS);
    eprintln!("64 players: controller passes {per_tick:.3} ms per tick");
    // Optimised builds hold the budget; unoptimised ones only catch pathological regressions.
    let budget = if cfg!(debug_assertions) { 25.0 } else { 1.0 };
    assert!(per_tick < budget, "{per_tick} ms per tick");
}
