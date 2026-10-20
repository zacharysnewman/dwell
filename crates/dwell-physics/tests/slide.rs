//! Sliding on steep ground (PLAYER_CONTROLLER.md §6.8, owner 2026-10-15: "make the player slide on
//! steep >44° slopes, give the player some control"): ground steeper than `slide.min_angle` slides
//! the player down under gravity less friction, up to a terminal speed; steering across the slope,
//! braking that slows but never stops or climbs, a crouched slide, jumping off with the momentum,
//! and no launch or hop at the bottom or at a crest. 27° slopes stay walkable. Runs in the local
//! frame like every suite (far from the origin and on face B too); the last test is face B in the
//! world frame.

mod common;

use common::*;
use dwell_blocks::{StateId, require};
use dwell_physics::controller::{State, events};
use dwell_physics::{DVec3, Vec3};
use dwell_world::ramps::{RampFacing, ramp_state, steep_pieces};
use dwell_world::rows_for_m;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn grass() -> StateId {
    require("dwell:grass")
}

/// A wide 45° ramp (a rotated static Tier 1 slab, 16 m wide in x) rising toward +z from the floor
/// (y = 0) at z ≈ 5.29 to y ≈ 3.18 at z ≈ 8.47: `ramp_y(z)` is its surface.
fn wide_ramp(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.boxed(
        v(0.5, 0.0, 6.0),
        v(8.0, 0.5, 4.0),
        euler(-45.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
}

fn ramp_y(z: f32) -> f32 {
    // The slab's top face passes through its centre + 0.5 m along the normal (0, cos 45°, −sin 45°).
    let h = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    h + (z - (6.0 - h))
}

/// A long 45° ramp (24 m along the slope) for the terminal speed: its surface `long_ramp_y(z)`.
fn long_ramp(w: &mut PlayerTestWorld) {
    w.floor_at(0, 32);
    w.boxed(
        v(0.5, 0.0, 12.0),
        v(4.0, 0.5, 12.0),
        euler(-45.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
}

fn long_ramp_y(z: f32) -> f32 {
    let h = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    h + (z - (12.0 - h))
}

/// Situation P of the Shape Playground: one-wide stairs of grass 1, 2, 3, 4, 5 m high along +x at
/// z = 0 on a floor of cubes (top y = 0), each topped by a stored 45° ramp rising toward +x (the
/// half-height grid's pieces, HALF_HEIGHT_GRID.md §3; under Phase 18 the rules shaped them): one
/// even 45° slope.
fn stairs_height_m(x: i32, z: i32) -> i32 {
    if z == 0 && (0..5).contains(&x) {
        x + 1
    } else {
        0
    }
}

/// The column's cells from the bottom (face-local): grass, its top metre the ramp's pieces (by
/// name: the caller hangs them on face B).
fn stairs_column(x: i32) -> Vec<&'static str> {
    let rows = rows_for_m(f64::from(stairs_height_m(x, 0)));
    let top = steep_pieces().expect("a cell height with 45° ramps");
    (0..rows)
        .map(|y| {
            let from_top = (rows - 1 - y) as usize;
            if from_top < top.len() {
                top[top.len() - 1 - from_top]
            } else {
                "full"
            }
        })
        .collect()
}

fn stairs_state(piece: &str, hanging: bool) -> StateId {
    if piece == "full" {
        grass()
    } else {
        ramp_state("dwell:grass", piece, RampFacing::West, hanging).expect("grass ramps")
    }
}

fn stairs(w: &mut PlayerTestWorld) {
    w.fill(-6, -1, -6, 10, -1, 6, cube_block());
    for x in 0..5 {
        for (y, piece) in stairs_column(x).into_iter().enumerate() {
            w.set(x, y as i32, 0, stairs_state(piece, false));
        }
    }
}

/// The stairs' surface height over (x, z) (local frame): the step below, plus the rise across.
fn stairs_surface(x: f32, z: f32) -> f32 {
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let h = stairs_height_m(cx, cz);
    if h == 0 {
        return 0.0;
    }
    (h - 1) as f32 + (x - cx as f32)
}

/// Ticks in the air rising (a hop or a launch) and the fastest upward speed.
#[derive(Debug, Default)]
struct Rise {
    rising_airborne: i32,
    max_up: f32,
}

impl Rise {
    fn watch(&mut self, w: &PlayerTestWorld, e: u32) {
        let up = w.vel(e).y;
        self.max_up = self.max_up.max(up);
        if !w.c(e).ground.grounded && up > 0.5 {
            self.rising_airborne += 1;
        }
    }
}

const MIN_ANGLE: f32 = 44.0;

#[test]
fn standing_on_a_45_degree_ramp_slides_down_and_never_rests_on_it() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    let e = w.spawn(v(0.5, ramp_y(7.5) + 0.05, 7.5));
    let mut slid = 0;
    let mut rise = Rise::default();
    let mut resting_on_steep = 0;
    for t in 0..ticks(3.0) {
        w.step(1);
        rise.watch(&w, e);
        let c = w.c(e);
        if c.state == State::Sliding {
            slid += 1;
        }
        let steep = c.ground.grounded && c.ground.slope_angle > MIN_ANGLE;
        if t > 6 && steep && w.vel(e).length() < 0.5 {
            resting_on_steep += 1;
        }
    }
    assert!(slid > 10, "slid for {slid} ticks");
    assert_eq!(resting_on_steep, 0, "never at rest on a 45° face");
    assert!(w.pos(e).z < 5.0, "slid off the ramp: {}", w.pos(e));
    assert!(approx(w.feet(e), 0.0, 0.02), "on the floor: {}", w.feet(e));
    assert_eq!(rise.rising_airborne, 0, "{rise:?}");
    // And comes to rest on the flat floor.
    w.step(ticks(2.0));
    assert!(w.horizontal_speed(e) < 0.01, "{}", w.horizontal_speed(e));
    assert_ne!(w.c(e).state, State::Sliding);
}

#[test]
fn standing_on_single_block_stairs_slides_down_them() {
    // Owner (2026-10-15): "Stairs will be their own thing later on. Ramps steeper than 44° will
    // slide." A player put on the third stair's ramp slides to the floor.
    let mut w = PlayerTestWorld::new();
    stairs(&mut w);
    let e = w.spawn_yaw(v(2.5, stairs_surface(2.5, 0.5) + 0.05, 0.5), 90.0);
    let mut rise = Rise::default();
    let mut slid = 0;
    for _ in 0..ticks(3.0) {
        w.step(1);
        rise.watch(&w, e);
        slid += i32::from(w.c(e).state == State::Sliding);
    }
    assert!(slid > 10, "slid for {slid} ticks");
    assert!(w.pos(e).x < 0.0, "down the stairs: {}", w.pos(e));
    assert!(approx(w.feet(e), 0.0, 0.02), "on the floor: {}", w.feet(e));
    assert!(rise.rising_airborne <= 2, "{rise:?}");
    assert_eq!(w.count(e, events::JUMPED), 0);
}

#[test]
fn walking_or_running_up_a_45_degree_ramp_fails() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    let walker = w.spawn(v(-2.5, 0.0, 3.0));
    let runner = w.spawn(v(3.5, 0.0, 3.0));
    w.set_input(move |_, h| input(0.0, 1.0, h == runner, false, false, 0.0, 0.0));
    let (mut walker_top, mut runner_top) = (0.0f32, 0.0f32);
    for _ in 0..ticks(3.0) {
        w.step(1);
        walker_top = walker_top.max(w.feet(walker));
        runner_top = runner_top.max(w.feet(runner));
    }
    // Momentum carries the capsule's round bottom a little way onto the foot of the ramp at most.
    assert!(walker_top < 0.6, "walker reached {walker_top}");
    assert!(runner_top < 0.8, "runner reached {runner_top}");
    assert!(w.pos(walker).z < 6.0 && w.pos(runner).z < 6.0);
}

#[test]
fn walking_up_single_block_stairs_fails() {
    let mut w = PlayerTestWorld::new();
    stairs(&mut w);
    let e = w.spawn_yaw(v(-2.5, 0.02, 0.5), 90.0);
    w.set_input(|_, _| input(0.0, 1.0, false, false, false, 90.0, 0.0));
    let top = w.max_feet(e, ticks(3.0));
    assert!(top < 1.0, "never tops the first ramp: {top}");
    assert_eq!(w.count(e, events::JUMPED), 0);
}

#[test]
fn steering_moves_the_slider_across_the_slope_with_reduced_acceleration() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    // Facing up the ramp (+z), right is −x.
    let steerer = w.spawn(v(-1.5, ramp_y(8.0) + 0.05, 8.0));
    let coaster = w.spawn(v(3.5, ramp_y(8.0) + 0.05, 8.0));
    w.set_input(move |_, h| mv(if h == steerer { 1.0 } else { 0.0 }, 0.0));
    w.step(6);
    let early = w.vel(steerer).x;
    // Walking accelerates at 50 m/s² (5 m/s within 0.1 s); steering on a slide is slower.
    assert!(early < -0.3, "steering: {early}");
    assert!(early > -3.5, "with reduced acceleration: {early}");
    w.step(ticks(0.5));
    let (s, c) = (w.pos(steerer), w.pos(coaster));
    assert!(s.x < -2.3, "moved across: {s}");
    assert!((c.x - 3.5).abs() < 0.05, "no drift without input: {c}");
    assert!(s.z < 7.0 && c.z < 7.0, "both slid down: {s} {c}");
}

#[test]
fn braking_slows_the_slide_but_never_stops_it_or_climbs() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    let braker = w.spawn(v(-3.0, ramp_y(8.0) + 0.05, 8.0));
    let coaster = w.spawn(v(3.0, ramp_y(8.0) + 0.05, 8.0));
    // Facing up the ramp, holding forward: up the slope.
    w.set_input(move |_, h| mv(0.0, if h == braker { 1.0 } else { 0.0 }));
    let mut min_z = w.pos(braker).z;
    let mut climbed = 0.0f32;
    let mut stopped = 0;
    let mut slower = true;
    for t in 0..ticks(2.5) {
        w.step(1);
        let z = w.pos(braker).z;
        let on_ramp = z > 5.6;
        if on_ramp {
            climbed = climbed.max(z - min_z);
        }
        min_z = min_z.min(z);
        if t > 12 && on_ramp && w.vel(braker).length() < 0.3 {
            stopped += 1;
        }
        if t > 12 && w.pos(coaster).z > 5.6 && w.vel(braker).z < w.vel(coaster).z - 0.01 {
            slower = false;
        }
    }
    assert!(climbed < 0.01, "braking never climbs: {climbed}");
    assert_eq!(stopped, 0, "braking never stops the slide");
    assert!(slower, "the braker slides slower");
    assert!(
        w.pos(braker).z < 5.3,
        "but still reaches the bottom: {}",
        w.pos(braker)
    );
}

#[test]
fn reaching_the_floor_keeps_the_momentum_without_a_hop_then_friction_stops_it() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    let e = w.spawn(v(0.5, ramp_y(8.0) + 0.05, 8.0));
    let holder = w.spawn(v(4.5, ramp_y(8.0) + 0.05, 8.0));
    // `holder` faces down the ramp (yaw 180°) and holds forward from the bottom on.
    w.set_input(move |_, h| {
        if h == holder {
            input(0.0, 1.0, false, false, false, 180.0, 0.0)
        } else {
            mv(0.0, 0.0)
        }
    });
    let mut rise = Rise::default();
    let mut bottom_speed = None;
    let mut holder_bottom = None;
    let mut holder_peak = 0.0f32;
    let mut airborne_on_flat = 0;
    let mut stop_tick = None;
    let mut bottom_tick = None;
    for t in 0..ticks(4.0) {
        w.step(1);
        rise.watch(&w, e);
        rise.watch(&w, holder);
        if std::env::var("DWELL_DEBUG").is_ok() {
            for h in [e, holder] {
                let c = w.c(h);
                eprintln!(
                    "t {t} p{h} {} v {} {:?} slope {} cur {} ext {}",
                    w.pos(h),
                    w.vel(h),
                    c.state,
                    c.ground.slope_angle,
                    c.horizontal.current,
                    c.horizontal.external
                );
            }
        }
        if w.pos(e).z < 4.9 {
            bottom_speed.get_or_insert(w.horizontal_speed(e));
            bottom_tick.get_or_insert(t);
            airborne_on_flat += i32::from(!w.c(e).ground.grounded);
            if stop_tick.is_none() && w.horizontal_speed(e) < 0.01 {
                stop_tick = Some(t);
            }
        }
        // The speed it left the slope at: its last tick sliding.
        if w.c(holder).state == State::Sliding {
            holder_bottom = Some(w.horizontal_speed(holder));
        } else if w.pos(holder).z < 4.9 {
            holder_peak = holder_peak.max(w.horizontal_speed(holder));
        }
    }
    let bottom_speed = bottom_speed.expect("reached the floor");
    assert!(
        bottom_speed > 3.0,
        "keeps the slide's speed: {bottom_speed}"
    );
    assert_eq!(rise.rising_airborne, 0, "no launch or hop: {rise:?}");
    assert!(rise.max_up < 0.5, "{rise:?}");
    assert_eq!(airborne_on_flat, 0);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
    // Ground friction stops it, over more than a few ticks.
    let stop_tick = stop_tick.expect("stops");
    assert!(
        w.pos(e).z < 4.5,
        "coasted on past the foot (z 5.29): {}",
        w.pos(e)
    );
    let coast = stop_tick - bottom_tick.expect("reached the floor");
    assert!(coast > ticks(0.3), "coasted for {coast} ticks");
    // Holding forward at the bottom adds no boost on top of the slide's speed.
    let holder_bottom = holder_bottom.expect("holder reached the floor");
    assert!(
        holder_peak < holder_bottom + 0.3,
        "{holder_peak} vs {holder_bottom}"
    );
}

#[test]
fn running_over_a_crest_onto_a_45_degree_ramp_slides_down_without_a_launch() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 24);
    // A platform 3 m high for z ≤ 2, and from its edge a 45° ramp down toward +z to the floor at
    // z = 5: a slab 5 m along the slope whose top face starts at the platform's edge.
    w.static_box(v(0.5, 1.5, -2.0), v(8.0, 1.5, 4.0));
    let h = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    let along = 2.5 * std::f32::consts::FRAC_1_SQRT_2;
    w.boxed(
        v(0.5, 3.0 - along - h, 2.0 + along - h),
        v(8.0, 0.5, 2.5),
        euler(45.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
    let e = w.spawn(v(0.5, 3.0, -2.0));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    let mut rise = Rise::default();
    let mut airborne_run = 0;
    let mut longest_airborne = 0;
    let mut peak = 0.0f32;
    for _ in 0..ticks(2.0) {
        w.step(1);
        rise.watch(&w, e);
        if w.c(e).ground.grounded {
            airborne_run = 0;
        } else {
            airborne_run += 1;
            longest_airborne = longest_airborne.max(airborne_run);
        }
        peak = peak.max(w.horizontal_speed(e));
    }
    assert!(w.pos(e).z > 6.0, "down the ramp: {}", w.pos(e));
    assert_eq!(rise.rising_airborne, 0, "{rise:?}");
    assert!(rise.max_up < 0.5, "{rise:?}");
    assert!(
        longest_airborne <= 3,
        "follows the ramp: {longest_airborne}"
    );
    let run = w.config.movement.run_speed;
    assert!(peak > run + 0.5, "the slide gained speed: {peak}");
    assert_eq!(w.count(e, events::JUMPED), 0);
}

#[test]
fn the_slide_reaches_its_terminal_speed_and_a_crouched_slide_is_faster_and_lower() {
    let mut w = PlayerTestWorld::new();
    long_ramp(&mut w);
    let z0 = 19.0;
    let standing = w.spawn(v(-1.0, long_ramp_y(z0) + 0.05, z0));
    let crouched = w.spawn(v(2.0, long_ramp_y(z0) + 0.05, z0));
    w.set_input(move |_, h| input(0.0, 0.0, false, false, h == crouched, 0.0, 0.0));
    let (mut top_standing, mut top_crouched) = (0.0f32, 0.0f32);
    let mut crouched_sliding = 0;
    for _ in 0..ticks(2.2) {
        w.step(1);
        // The speed along the slope while on it (not the landing on the floor).
        if w.c(standing).state == State::Sliding {
            top_standing = top_standing.max(w.vel(standing).length());
        }
        if w.c(crouched).state == State::Sliding && w.c(crouched).crouching {
            top_crouched = top_crouched.max(w.vel(crouched).length());
            crouched_sliding += 1;
        }
    }
    let terminal = w.config.slide.terminal_speed;
    assert!(
        top_standing > terminal - 0.3 && top_standing < terminal + 0.3,
        "{top_standing}"
    );
    assert!(top_crouched > top_standing + 0.5, "{top_crouched}");
    let crouched_terminal = terminal * w.config.slide.crouch_terminal_factor;
    assert!(top_crouched < crouched_terminal + 0.3, "{top_crouched}");
    assert!(crouched_sliding > 30, "{crouched_sliding}");
    assert!(approx(w.players.half_height(crouched), 0.45, 1e-5));
}

#[test]
fn jumping_off_a_slide_keeps_the_downhill_momentum() {
    let mut w = PlayerTestWorld::new();
    long_ramp(&mut w);
    let z0 = 14.0;
    let e = w.spawn(v(0.5, long_ramp_y(z0) + 0.05, z0));
    let jump_at = ticks(0.7);
    w.set_input(move |t, _| input(0.0, 0.0, false, t == jump_at, false, 0.0, 0.0));
    w.step(jump_at + 1);
    assert_eq!(w.count(e, events::JUMPED), 1);
    let takeoff = w.vel(e);
    assert!(takeoff.z < -3.0, "sliding down at take-off: {takeoff}");
    let mut min_ratio = f32::MAX;
    for _ in 0..10 {
        w.step(1);
        assert!(!w.c(e).ground.grounded);
        min_ratio = min_ratio.min(w.vel(e).z / takeoff.z);
    }
    // (The player's own part of it slows with air control, as after any jump without input.)
    assert!(min_ratio > 0.9, "kept the momentum in the air: {min_ratio}");
}

/// A wide 26.57° ramp (a rotated static Tier 1 slab, as `wide_ramp`): `gentle_y(z)` is its surface.
fn gentle_ramp(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.boxed(
        v(0.5, 0.0, 6.0),
        v(8.0, 0.5, 4.0),
        euler(-f64::from(GENTLE_DEG), 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
}

/// arctan ½: half a metre over a metre, the half-height grid's ramp (HALF_HEIGHT_GRID.md §3).
const GENTLE_DEG: f32 = 26.565_05;

fn gentle_y(z: f32) -> f32 {
    // The slab's top face passes through its centre + 0.5 m along its normal.
    let t = GENTLE_DEG.to_radians();
    0.5 / t.cos() + t.tan() * (z - 6.0)
}

#[test]
fn a_27_degree_slope_is_still_walked_up_and_rested_on() {
    let mut w = PlayerTestWorld::new();
    gentle_ramp(&mut w);
    let e = w.spawn(v(0.5, gentle_y(6.0) + 0.05, 6.0));
    w.step(ticks(0.5));
    let at = w.pos(e);
    w.step(ticks(2.0));
    assert!(
        (w.pos(e) - at).length() < 0.01,
        "rests on 27°: {at} → {}",
        w.pos(e)
    );
    assert!(w.c(e).ground.slope_angle > 20.0 && w.c(e).ground.slope_angle < MIN_ANGLE);
    assert_eq!(w.c(e).state, State::Idle);
    let before = w.feet(e);
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(1.0));
    assert!(
        w.feet(e) > before + 0.4,
        "walked up: {before} → {}",
        w.feet(e)
    );
    assert_ne!(w.c(e).state, State::Sliding);
}

#[test]
fn swimming_and_climbing_override_sliding() {
    let mut w = PlayerTestWorld::new();
    wide_ramp(&mut w);
    // Deep water over one part of the ramp; on another, a wall of ladder cells (x = 3) the climber
    // steers into from the ramp beside it (grabbing needs the ground two ticks running).
    w.fill(-6, 0, 5, -2, 6, 9, require("dwell:water"));
    w.fill(3, 0, 5, 3, 4, 9, require("dwell:ladder[facing=east]"));
    let swimmer = w.spawn(v(-4.0, ramp_y(7.5) + 0.05, 7.5));
    let climber = w.spawn(v(4.9, ramp_y(7.5) + 0.05, 7.5));
    let steer = ticks(0.4);
    w.set_input(move |t, h| mv(if h == climber && t < steer { 1.0 } else { 0.0 }, 0.0));
    w.step(ticks(0.5));
    assert_eq!(w.count(climber, events::CLIMB_STARTED), 1);
    let at = w.pos(climber);
    let mut slid = 0;
    for _ in 0..ticks(1.0) {
        w.step(1);
        slid += i32::from(w.c(swimmer).state == State::Sliding)
            + i32::from(w.c(climber).state == State::Sliding);
    }
    assert_eq!(slid, 0);
    assert_eq!(w.c(swimmer).state, State::Swimming);
    assert_eq!(w.c(climber).state, State::Climbing);
    assert!(
        (w.pos(climber).z - at.z).abs() < 0.1,
        "held on the ladder: {at} → {}",
        w.pos(climber)
    );
}

#[test]
fn a_face_b_player_slides_down_single_block_stairs_toward_the_midplane() {
    // The stairs of the test above, built in the world frame on face B: the mirror image below the
    // midplane, where up is −y. The slide follows the face's up, not +y.
    let mut w = PlayerTestWorld::world_frame();
    let row = dwell_world::mirror_y;
    for z in -6..=6 {
        for x in -6..=10 {
            w.world.set_voxel(x, row(-1), z, cube_block());
        }
    }
    for x in 0..5 {
        for (y, piece) in stairs_column(x).into_iter().enumerate() {
            w.world
                .set_voxel(x, row(y as i32), 0, stairs_state(piece, true));
        }
    }
    let local_feet = stairs_surface(2.5, 0.5) + 0.05;
    let e = w.spawn_at(DVec3::new(2.5, MIRROR - f64::from(local_feet), 0.5), 90.0);
    assert_eq!(w.c(e).face, -1);
    let mut slid = 0;
    for _ in 0..ticks(3.0) {
        w.step(1);
        slid += i32::from(w.c(e).state == State::Sliding);
    }
    let feet = MIRROR - w.players.feet(&w.physics, e);
    assert!(slid > 10, "slid for {slid} ticks");
    assert!(w.players.position(&w.physics, e).x < 0.0);
    assert!(feet.abs() < 0.02, "on the floor: {feet}");
}
