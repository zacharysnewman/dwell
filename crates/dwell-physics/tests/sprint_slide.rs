//! The sprint slide (PLAYER_CONTROLLER.md §6.9, owner 2026-10-19: "when sprinting, if you crouch,
//! you'll continue to slide forward … You should still be in 'crouch' (height etc.) while
//! sliding"): crouching while sprinting on the ground slides on in the direction of travel with the
//! crouch capsule, slowing with friction (less downhill, more uphill) until the crouch speed or the
//! crouch's release; limited steering, a jump out with the momentum, a cooldown. Runs in the local
//! frame like every suite (far from the origin and on face B too).

mod common;

use common::*;
use dwell_physics::Vec3;
use dwell_physics::controller::{State, events};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// Runs forward (+z) from z = −30 on a wide floor for `run_ticks`, then holds crouch (still
/// running and moving forward) until `release` (a tick; `i32::MAX` never).
fn sprint_then_crouch(w: &mut PlayerTestWorld, run_ticks: i32, release: i32) -> u32 {
    w.floor_at(0, 48);
    let e = w.spawn(v(0.5, 0.0, -30.0));
    w.set_input(move |t, _| {
        input(
            0.0,
            1.0,
            true,
            false,
            t >= run_ticks && t < release,
            0.0,
            0.0,
        )
    });
    e
}

/// Ticks the sprint slide lasted (from its start) and the distance covered meanwhile.
#[derive(Debug, Default)]
struct SlideRun {
    ticks: i32,
    distance: f32,
    started: i32,
    ended: i32,
}

fn measure(w: &mut PlayerTestWorld, e: u32, seconds: f32) -> SlideRun {
    let mut r = SlideRun::default();
    for _ in 0..ticks(seconds) {
        let before = w.pos(e);
        w.step(1);
        if w.c(e).sprint_slide.active {
            r.ticks += 1;
            let d = w.pos(e) - before;
            r.distance += (d.x * d.x + d.z * d.z).sqrt();
        }
    }
    r.started = w.count(e, events::SPRINT_SLIDE_STARTED);
    r.ended = w.count(e, events::SPRINT_SLIDE_ENDED);
    r
}

#[test]
fn crouching_while_sprinting_starts_a_slide_with_the_sprints_momentum() {
    let mut w = PlayerTestWorld::new();
    let e = sprint_then_crouch(&mut w, 60, i32::MAX);
    w.step(60);
    let run = w.horizontal_speed(e);
    assert!(approx(run, 8.0, 0.05), "running at {run}");
    w.step(2);
    let c = w.c(e);
    assert!(c.sprint_slide.active, "sliding");
    assert_eq!(c.state, State::Sliding);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 1);
    // The sprint's speed plus the start boost, along the run.
    let speed = w.horizontal_speed(e);
    assert!(speed > run + 1.0, "{speed}");
    let vel = w.vel(e);
    assert!(vel.z > 0.99 * speed && vel.x.abs() < 0.01, "{vel}");
}

#[test]
fn crouching_from_a_walk_or_a_standing_sprint_does_not_slide() {
    // A walk (run not held).
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 48);
    let e = w.spawn(v(0.5, 0.0, -30.0));
    w.set_input(|t, _| input(0.0, 1.0, false, false, t >= 60, 0.0, 0.0));
    w.step(60);
    assert!(approx(w.horizontal_speed(e), 5.0, 0.05));
    w.step(30);
    assert!(!w.c(e).sprint_slide.active);
    assert_eq!(w.c(e).state, State::Crouching);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 0);
    assert!(
        approx(w.horizontal_speed(e), 1.6, 0.05),
        "{}",
        w.horizontal_speed(e)
    );
    // Run held, but crouch pressed before the player got up to speed.
    let mut w = PlayerTestWorld::new();
    let e = sprint_then_crouch(&mut w, 3, i32::MAX);
    w.step(30);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 0);
    assert_eq!(w.c(e).state, State::Crouching);
}

#[test]
fn friction_slows_the_slide_to_the_crouch_speed_where_it_ends() {
    let mut w = PlayerTestWorld::new();
    let e = sprint_then_crouch(&mut w, 60, i32::MAX);
    w.step(61);
    let start = w.horizontal_speed(e);
    let mut last = start;
    let mut ticks_sliding = 0;
    while w.c(e).sprint_slide.active && ticks_sliding < 600 {
        w.step(1);
        let s = w.horizontal_speed(e);
        assert!(s <= last + 1e-3, "speeding up on flat ground: {last} → {s}");
        last = s;
        ticks_sliding += 1;
    }
    // (start − crouch speed) / friction: 1.32 s at the defaults.
    let s = w.config.sprint_slide;
    let expected = (start - w.config.crouch.speed) / s.friction;
    let took = ticks_sliding as f32 / 60.0;
    assert!(
        (took - expected).abs() < 0.1,
        "{took} s, expected {expected} s"
    );
    assert_eq!(w.count(e, events::SPRINT_SLIDE_ENDED), 1);
    // Crouch still held: the player crouch-walks on.
    w.step(30);
    assert_eq!(w.c(e).state, State::Crouching);
    assert!(
        approx(w.horizontal_speed(e), 1.6, 0.05),
        "{}",
        w.horizontal_speed(e)
    );
}

/// A wide, long gentle slope (a static Tier 1 slab tilted `degrees`, rising toward +z) with its
/// surface `surface_y(z)`; the player spawns on it at `z` facing `yaw`, sprints for 0.7 s along it
/// and crouches. Returns the slide.
fn slide_on_slope(degrees: f32, z: f32, yaw: f32) -> SlideRun {
    let mut w = PlayerTestWorld::new();
    let a = degrees.to_radians();
    let (cy, cz) = (0.0f32, 0.0f32);
    w.boxed(
        v(0.5, cy, cz),
        v(10.0, 0.5, 40.0),
        euler(f64::from(-degrees), 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
    let surface_y = |z: f32| cy + 0.5 * a.cos() + (z - (cz - 0.5 * a.sin())) * a.tan();
    let e = w.spawn_yaw(v(0.5, surface_y(z) + 0.05, z), yaw);
    let run = ticks(0.7) + 20;
    w.set_input(move |t, _| {
        input(
            0.0,
            f32::from(u8::from(t >= 20)),
            true,
            false,
            t >= run,
            yaw,
            0.0,
        )
    });
    w.step(run);
    // (Uphill the body's own velocity is lower: each tick's step-up nudge pays part of it, §4.)
    let own = w.c(e).horizontal.current.length();
    assert!(approx(own, 8.0, 0.1), "{own}");
    measure(&mut w, e, 3.0)
}

#[test]
fn a_slide_lasts_longer_downhill_and_shorter_uphill() {
    let mut w = PlayerTestWorld::new();
    let e = sprint_then_crouch(&mut w, 60, i32::MAX);
    w.step(60);
    let flat = measure(&mut w, e, 3.0);
    // 15° slopes: downhill the slope's pull beats friction, uphill it adds to it.
    let down = slide_on_slope(15.0, 25.0, 180.0);
    let up = slide_on_slope(15.0, -25.0, 0.0);
    eprintln!("flat {flat:?}\ndown {down:?}\nup {up:?}");
    for r in [&flat, &down, &up] {
        assert_eq!(r.started, 1, "{r:?}");
    }
    assert!(down.ticks > flat.ticks + 30 && down.distance > flat.distance * 2.0);
    assert!(up.ticks < flat.ticks - 20 && up.distance < flat.distance * 0.7);
}

#[test]
fn the_slide_keeps_the_crouch_capsule_throughout() {
    let mut w = PlayerTestWorld::new();
    let e = sprint_then_crouch(&mut w, 60, i32::MAX);
    w.step(61);
    let mut slid = 0;
    while w.c(e).sprint_slide.active {
        assert!(w.c(e).crouching);
        assert!(approx(w.players.half_height(e), 0.45, 1e-5));
        assert!(approx(w.feet(e), 0.0, 0.02), "feet {}", w.feet(e));
        assert!(approx(w.head(e), 0.9, 0.02), "head {}", w.head(e));
        w.step(1);
        slid += 1;
    }
    assert!(slid > 30, "{slid}");
}

#[test]
fn releasing_crouch_under_a_low_ceiling_stays_crouched_and_stands_once_clear() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 48);
    // A 1-tall crawlspace over z 2..7.
    w.fill(-3, 1, 2, 3, 1, 6, cube_block());
    let e = w.spawn(v(0.5, 0.0, -10.0));
    let (crouch, release) = (60, 105);
    w.set_input(move |t, _| {
        input(
            0.0,
            1.0,
            true,
            false,
            (crouch..release).contains(&t),
            0.0,
            0.0,
        )
    });
    w.step(crouch + 2);
    assert!(w.c(e).sprint_slide.active);
    w.step(release - crouch - 2);
    let z = w.pos(e).z;
    assert!(
        (2.4..6.6).contains(&z),
        "released at z {z}: not under the roof"
    );
    let mut stood_under = false;
    for _ in 0..ticks(5.0) {
        w.step(1);
        let z = w.pos(e).z;
        stood_under |= z > 2.35 && z < 6.65 && !w.c(e).crouching;
    }
    assert!(!stood_under, "stood up under the roof");
    assert!(!w.c(e).sprint_slide.active);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_ENDED), 1);
    assert!(w.pos(e).z > 7.5, "{}", w.pos(e).z);
    assert!(!w.c(e).crouching);
    assert!(approx(w.feet(e), 0.0, 0.02), "{}", w.feet(e));
}

#[test]
fn a_jump_out_of_a_slide_keeps_its_momentum() {
    let jump_distance = |sprint: bool| {
        let mut w = PlayerTestWorld::new();
        w.floor_at(0, 48);
        let e = w.spawn(v(0.5, 0.0, -30.0));
        let jump = 75;
        w.set_input(move |t, _| {
            input(
                0.0,
                1.0,
                sprint,
                (jump..jump + 2).contains(&t),
                t >= 60,
                0.0,
                0.0,
            )
        });
        w.step(jump);
        let at = w.horizontal_speed(e);
        w.step(1);
        assert!(!w.c(e).ground.grounded);
        let z0 = w.pos(e).z;
        let mut min_speed = f32::MAX;
        while !w.c(e).ground.grounded {
            min_speed = min_speed.min(w.horizontal_speed(e));
            w.step(1);
        }
        assert_eq!(w.count(e, events::JUMPED), 1);
        if sprint {
            // Crouch still held: it slides on where it lands (owner, 2026-10-30).
            assert!(w.c(e).sprint_slide.active);
            assert_eq!(w.count(e, events::SPRINT_SLIDE_ENDED), 1);
            assert!(
                min_speed > at - 0.05,
                "kept {min_speed} of {at} m/s in the air"
            );
        }
        w.pos(e).z - z0
    };
    let slide = jump_distance(true);
    let crouch_walk = jump_distance(false);
    // Over the 0.7 s jump: about 7.7 m/s against the crouch speed (1.6 m/s).
    assert!(
        slide > 5.0 && slide > 3.0 * crouch_walk,
        "{slide} m against {crouch_walk} m"
    );
}

#[test]
fn a_cooldown_stops_slides_being_chained() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 64);
    let e = w.spawn(v(0.5, 0.0, -60.0));
    // Slide at 60, release at 70, crouch again at 76 (inside the cooldown), release at 82, and
    // again at 140 (after it).
    let crouched = |t: i32| (60..70).contains(&t) || (76..82).contains(&t) || t >= 140;
    w.set_input(move |t, _| input(0.0, 1.0, true, false, crouched(t), 0.0, 0.0));
    w.step(72);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 1);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_ENDED), 1);
    w.step(4);
    assert!(
        w.horizontal_speed(e) > 7.0,
        "still sprinting: {}",
        w.horizontal_speed(e)
    );
    w.step(2);
    assert!(
        !w.c(e).sprint_slide.active,
        "chained a slide inside the cooldown"
    );
    assert_eq!(w.c(e).state, State::Crouching);
    w.step(140 - 78 + 2);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 2);
    assert!(w.c(e).sprint_slide.active);
}

#[test]
fn steering_turns_a_slide_at_a_limited_rate() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 48);
    let e = w.spawn(v(0.5, 0.0, -30.0));
    // Sprint along +z, slide, then look 90° to the side holding forward for half a second.
    w.set_input(|t, _| {
        input(
            0.0,
            1.0,
            true,
            false,
            t >= 60,
            if t >= 62 { 90.0 } else { 0.0 },
            0.0,
        )
    });
    w.step(62 + 30);
    assert!(w.c(e).sprint_slide.active);
    let vel = w.vel(e);
    let turned = vel.x.atan2(vel.z).to_degrees();
    // 90 °/s for 0.5 s.
    let rate = w.config.sprint_slide.steer_rate;
    assert!((turned - rate * 0.5).abs() < 4.0, "turned {turned}°");
}

#[test]
fn a_wall_stops_a_slide() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 48);
    w.fill(-4, 0, 0, 4, 1, 0, cube_block());
    let e = w.spawn(v(0.5, 0.0, -10.0));
    w.set_input(|t, _| input(0.0, 1.0, true, false, t >= 60, 0.0, 0.0));
    w.step(62);
    assert!(w.c(e).sprint_slide.active);
    w.step(60);
    assert!(!w.c(e).sprint_slide.active);
    assert!(w.pos(e).z < -0.29 && w.pos(e).z > -0.4, "{}", w.pos(e).z);
    // Against the wall the crouched player pushes at walking pace, with no stored momentum.
    let c = w.c(e);
    assert!(
        c.horizontal.external.length() < 0.05,
        "{}",
        c.horizontal.external
    );
}

// --- Owner, 2026-10-30: crouching in the air, sliding on landing, jumping off a steep slope ---

/// Runs +z on a 2 m platform (z < −9) and off its edge at the run speed: returns the player once
/// airborne past the edge, `crouch` held from then on.
fn run_off_a_ledge(w: &mut PlayerTestWorld, crouch: bool) -> u32 {
    w.floor_at(0, 64);
    w.fill(-4, 0, -40, 4, 1, -10, cube_block());
    let e = w.spawn(v(0.5, 2.0, -30.0));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    for _ in 0..ticks(6.0) {
        w.step(1);
        if !w.c(e).ground.grounded && w.pos(e).z > -9.0 {
            break;
        }
    }
    assert!(!w.c(e).ground.grounded, "off the edge");
    w.set_input(move |_, _| input(0.0, 1.0, true, false, crouch, 0.0, 0.0));
    e
}

#[test]
fn crouching_in_the_air_keeps_the_speed_you_had() {
    // Owner: "crouching shouldn't slow your in air velocity". Before, the air control steered the
    // speed toward the crouch speed (1.6 m/s) for the rest of the fall.
    let mut w = PlayerTestWorld::new();
    let e = run_off_a_ledge(&mut w, true);
    let at = w.horizontal_speed(e);
    assert!(at > 7.0, "running off the edge: {at}");
    let mut slowest = f32::MAX;
    w.step(1);
    while !w.c(e).ground.grounded {
        slowest = slowest.min(w.horizontal_speed(e));
        w.step(1);
    }
    assert!(w.c(e).crouching);
    assert!(
        slowest > at - 0.1,
        "kept {slowest} of {at} m/s crouched in the air"
    );
}

#[test]
fn landing_with_crouch_held_while_moving_slides() {
    // Owner: "sliding should continue from midair … falling when crouch is held on landing", with
    // horizontal speed (a fall straight down doesn't slide).
    let mut w = PlayerTestWorld::new();
    let e = run_off_a_ledge(&mut w, true);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 0);
    while !w.c(e).ground.grounded {
        w.step(1);
    }
    w.step(2);
    assert!(w.c(e).sprint_slide.active, "slid on landing");
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 1);
    // Not crouching: lands running.
    let mut w = PlayerTestWorld::new();
    let e = run_off_a_ledge(&mut w, false);
    w.step(ticks(1.0));
    assert!(!w.c(e).sprint_slide.active);
}

#[test]
fn a_jump_out_of_a_slide_with_crouch_held_slides_on_where_it_lands() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 64);
    let e = w.spawn(v(0.5, 0.0, -50.0));
    let jump = 75;
    w.set_input(move |t, _| {
        input(
            0.0,
            1.0,
            true,
            (jump..jump + 2).contains(&t),
            t >= 60,
            0.0,
            0.0,
        )
    });
    w.step(jump + 2);
    assert!(!w.c(e).ground.grounded, "jumped");
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 1);
    while !w.c(e).ground.grounded {
        w.step(1);
    }
    let landed = w.horizontal_speed(e);
    w.step(2);
    assert!(w.c(e).sprint_slide.active, "slid on landing");
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 2);
    // A continuation: no second start boost.
    assert!(
        w.horizontal_speed(e) <= landed + 0.05,
        "{} after {landed}",
        w.horizontal_speed(e)
    );
}

#[test]
fn falling_straight_down_with_crouch_held_does_not_slide() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 16);
    let e = w.spawn(v(0.5, 3.0, 0.5));
    w.set_input(|_, _| input(0.0, 0.0, true, false, true, 0.0, 0.0));
    w.step(ticks(2.0));
    assert!(w.c(e).ground.grounded);
    assert_eq!(w.count(e, events::SPRINT_SLIDE_STARTED), 0);
}

#[test]
fn a_jump_off_a_steep_slope_adds_the_jump_along_its_face() {
    // Owner: jumping from a steep slope "should be angled out along the slope face angle" and add
    // to the velocity the player has ("forces are additive"), not replace it. A 45° face (steeper
    // than `slide.min_angle`, 44°: it slides) rising to +z: its normal leans to −z.
    let degrees = 45.0f32;
    let mut w = PlayerTestWorld::new();
    let a = degrees.to_radians();
    w.boxed(
        v(0.5, 0.0, 0.0),
        v(10.0, 0.5, 40.0),
        euler(f64::from(-degrees), 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
    let surface_y = |z: f32| 0.5 * a.cos() + (z + 0.5 * a.sin()) * a.tan();
    let e = w.spawn(v(0.5, surface_y(10.0) + 0.05, 10.0));
    let jump = 40;
    w.set_input(move |t, _| input(0.0, 0.0, false, t == jump, false, 0.0, 0.0));
    w.step(jump);
    assert!(w.c(e).sliding(44.0), "sliding down the face");
    let before = w.vel(e);
    w.step(1);
    let after = w.vel(e);
    assert_eq!(w.count(e, events::JUMPED), 1);
    let j = dwell_physics::config::PlayerControllerConfig::default().jump_velocity();
    let n = Vec3::new(0.0, a.cos(), -a.sin());
    let added = after - before;
    // Along the face's normal (gravity takes a tick's 0.33 m/s off y).
    assert!(
        (added - n * j).length() < 0.6,
        "added {added:?}, the jump along the normal is {:?} (before {before:?})",
        n * j
    );
    assert!(
        after.z < before.z - 0.5 * j * a.sin(),
        "out from the face: {after:?}"
    );
}
