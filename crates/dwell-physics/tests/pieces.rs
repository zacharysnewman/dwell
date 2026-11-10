//! Falling pieces and players (docs/FELLING.md §4): players push light pieces and not heavy ones.
//! In the face-local frame (`common`), so `DWELL_FACE=b` runs it on face B.

mod common;
use common::*;
use dwell_physics::{DVec3, PieceCell, Vec3};

/// A log lying across the player's path (three 1 m cells along x at z = 0, on the floor: too tall
/// to step onto), each cell `mass` kg; `dominant` as the server sets it for a piece too heavy to push.
fn log_across(w: &mut PlayerTestWorld, mass: f64, dominant: bool) -> dwell_physics::BodyHandle {
    let cells: Vec<PieceCell> = (-1..=1)
        .map(|x| PieceCell {
            center: DVec3::new(f64::from(x), 0.0, 0.0),
            half: DVec3::new(0.5, 0.5, 0.5),
            mass,
            collide: dwell_physics::Collide::All,
            hull: None,
            joined: 0,
        })
        .collect();
    w.physics
        .add_piece(to_world(Vec3::new(0.5, 0.5, 0.5)), &cells, dominant)
}

/// How far the log moved while the player walked into it for two seconds.
fn pushed(mass: f64, dominant: bool) -> f64 {
    let mut w = PlayerTestWorld::new();
    w.floor();
    // The player first: terrain collision is loaded round players.
    let _p = w.spawn(Vec3::new(0.5, 0.0, -2.0));
    w.step(5);
    let log = log_across(&mut w, mass, dominant);
    w.step(30);
    let start = w.physics.position(log);
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    (w.physics.position(log) - start).length()
}

#[test]
fn a_player_pushes_a_light_piece() {
    let d = pushed(5.0, false);
    assert!(d > 0.3, "a 15 kg log moved {d} m");
}

#[test]
fn a_player_pushes_a_held_light_piece_by_its_push() {
    // Docs/BUILT_BODIES.md §6 (owner, 2026-11-02: pushing a floating raft jittered): the server
    // holds slow pieces from players' contacts (`dominant`) and the controller's push moves them,
    // as the prediction does its stand-ins. Red before: a held piece never moved.
    let d = pushed(5.0, true);
    assert!(d > 0.3, "a held 15 kg log moved {d} m");
}

/// The log's speed (m/s, along the push) over the last second of three the player walked into it.
fn pushed_speed(total_mass: f64) -> f64 {
    let mut w = PlayerTestWorld::new();
    w.floor();
    let _p = w.spawn(Vec3::new(0.5, 0.0, -2.0));
    w.step(5);
    let log = log_across(&mut w, total_mass / 3.0, true);
    w.step(30);
    w.set_input(|_, _| mv(0.0, 1.0));
    w.step(ticks(2.0));
    let start = to_local(w.physics.position(log));
    w.step(ticks(1.0));
    f64::from(to_local(w.physics.position(log)).z - start.z)
}

#[test]
fn a_player_slides_a_log_on_the_ground_at_a_walk() {
    // Owner, 2026-11-03: a single log (a 1 m log block, 700 kg) slides on land at about 0.5 m/s.
    // Red before: the 800 N push was under its friction, and it stayed put.
    let v = pushed_speed(700.0);
    assert!((0.4..0.6).contains(&v), "a 700 kg log slid at {v} m/s");
}

#[test]
fn a_player_cannot_push_a_piece_too_heavy() {
    // A 1.5 t log held (as the server holds slow pieces) stays where it lies: the push
    // (`max_push_force`) is less than its friction on the ground.
    let d = pushed(500.0, true);
    assert!(d < 0.02, "a 1.5 t log moved {d} m");
}

#[test]
fn a_player_standing_on_a_sliding_piece_rides_it() {
    // A 5 × 1 × 4 m slab on the floor, the player on top, the slab kept sliding +x at 2 m/s (a
    // rolling log's steady motion; a sudden shove the player catches up with, as on a cart): the
    // player, without input, goes as far as the slab does.
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 1.0, 0.5));
    let cells: Vec<PieceCell> = (-2..=2)
        .map(|x| PieceCell {
            center: DVec3::new(f64::from(x), 0.0, 0.0),
            half: DVec3::new(0.5, 0.5, 2.0),
            mass: 100.0,
            collide: dwell_physics::Collide::All,
            hull: None,
            joined: 0,
        })
        .collect();
    let slab = w
        .physics
        .add_piece(to_world(Vec3::new(0.5, 0.5, 0.5)), &cells, false);
    w.step(30);
    let push = to_world(Vec3::new(2.0, 0.0, 0.0)) - to_world(Vec3::ZERO);
    let slide = |w: &mut PlayerTestWorld, ticks: i32| {
        for _ in 0..ticks {
            w.physics.set_velocity(slab, push);
            w.step(1);
        }
    };
    slide(&mut w, 30);
    let (p0, s0) = (w.pos(e), w.physics.position(slab));
    slide(&mut w, 30);
    let slid = to_local(w.physics.position(slab)) - to_local(s0);
    let rode = w.pos(e) - p0;
    assert!(slid.x > 0.8, "the slab slid {slid}");
    assert!(
        (rode.x - slid.x).abs() < 0.05,
        "rode {rode} on a slab that slid {slid}"
    );
    assert!(w.c(e).ground.grounded);
}

#[test]
fn a_felled_octagonal_trunk_falls_and_settles_on_a_flat_side() {
    // Phase 55 T4 (owner, 2026-11-09: "everything log/tree should be octagonal"): a felled trunk's
    // logs as one octagonal prism (`merged_boxes`; `piece_hull` doesn't give it yet), dropped on the floor, still
    // falls and comes to rest, lying on one of its flat sides (face-local: DWELL_FACE=b mirrors it).
    let mut w = PlayerTestWorld::new();
    w.floor();
    let _p = w.spawn(Vec3::new(0.5, 0.0, -2.5));
    w.step(5);
    let log = dwell_blocks::require("dwell:log[axis=x]");
    let cells: Vec<PieceCell> = (-2..2)
        .map(|x| PieceCell {
            center: DVec3::new(f64::from(x), 0.0, 0.0),
            half: DVec3::new(0.5, 0.25, 0.5),
            mass: 350.0,
            collide: dwell_physics::Collide::All,
            hull: Some(log),
            joined: 0,
        })
        .collect();
    let trunk = w
        .physics
        .add_piece(to_world(Vec3::new(0.5, 1.5, 0.5)), &cells, false);
    w.step(ticks(4.0));
    let at = to_local(w.physics.position(trunk));
    let speed = w.physics.velocity(trunk).length();
    assert!(at.y < 1.0, "it fell: {at:?}");
    assert!(speed < 0.05, "it settled: {speed} m/s at {at:?}");
    // On a flat side: its centre a cell's half (less the skin) over the floor, not on a corner.
    assert!(
        (f64::from(at.y) - 0.24).abs() < 0.03,
        "it lies on a flat side: centre {} m up",
        at.y
    );
}
