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

/// A piece of `cells` (centres relative to its origin, face-local; 1 m × `half_y` × 1 m boxes of
/// `mass` kg each), at `origin` (face-local), turned by `rotation` (face-local), moving at
/// `velocity` (face-local).
fn moving_piece(
    w: &mut PlayerTestWorld,
    cells: &[Vec3],
    half_y: f64,
    mass: f64,
    origin: Vec3,
    rotation: dwell_physics::DQuat,
    velocity: Vec3,
) -> dwell_physics::BodyHandle {
    let cells: Vec<PieceCell> = cells
        .iter()
        .map(|c| {
            let c = mirror_vec(*c);
            PieceCell {
                center: DVec3::new(f64::from(c.x), f64::from(c.y), f64::from(c.z)),
                half: DVec3::new(0.5, half_y, 0.5),
                mass,
                collide: dwell_physics::Collide::All,
                hull: None,
                joined: 0,
            }
        })
        .collect();
    let piece = w.physics.add_piece(to_world(origin), &cells, false);
    w.physics.set_rotation(piece, mirror_rotation(rotation));
    let v = mirror_vec(velocity);
    w.physics.set_velocity(
        piece,
        DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z)),
    );
    piece
}

#[test]
fn a_piece_coming_down_on_a_standing_player_pins_them_not_squeezes_them_out() {
    // BUGS.md #52: a felled trunk's slanted underside came down on its chopper and the
    // frictionless capsule, pinched between it and the ground, could only be squeezed out
    // sideways, at the descent over the slant's slope (a 10° slab at 6 m/s: ~34 m/s, flung tens
    // of metres). A piece pressing a player down grips them (`physics::PinchGrip`): they stay put
    // or are carried at the piece's own speed. Red before: thrown far.
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.step(5);
    let start = w.pos(e);
    // A 6 m × ½ m × 2 m slab of 1.2 t tilted 6°, its underside a little over the player's head.
    let cells: Vec<Vec3> = (0..6)
        .flat_map(|x| (0..2).map(move |z| Vec3::new(x as f32 - 2.5, 0.0, z as f32 - 0.5)))
        .collect();
    moving_piece(
        &mut w,
        &cells,
        0.25,
        100.0,
        Vec3::new(1.5, 2.4, 0.5),
        euler(0.0, 0.0, 6.0),
        Vec3::new(0.0, -6.0, 0.0),
    );
    let mut far: f32 = 0.0;
    let mut fastest: f32 = 0.0;
    for _ in 0..ticks(2.0) {
        w.step(1);
        let d = w.pos(e) - start;
        far = far.max((d.x * d.x + d.z * d.z).sqrt());
        fastest = fastest.max(w.horizontal_speed(e));
    }
    println!("squeezed {far} m, {fastest} m/s");
    assert!(
        far < 1.0 && fastest < 4.0,
        "the slab squeezed the player {far} m out, at {fastest} m/s"
    );
}

#[test]
fn a_piece_moving_into_a_standing_player_throws_them_as_hard_as_physics_says() {
    // BUGS.md #52, the owner (2026-11-08): "we shouldn't be capping player kick like that". The
    // pinch's grip is for contacts from above: a 900 kg log swung sideways into a player at
    // 10 m/s (an inelastic hit: momentum kept, 900 · 10 / 980 ≈ 9.2 m/s) throws them at about
    // its speed, uncapped.
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.step(5);
    let cells: Vec<Vec3> = (-1..=1).map(|z| Vec3::new(0.0, 0.0, z as f32)).collect();
    moving_piece(
        &mut w,
        &cells,
        0.5,
        300.0,
        Vec3::new(-2.5, 1.0, 0.5),
        dwell_physics::DQuat::IDENTITY,
        Vec3::new(10.0, 0.0, 0.0),
    );
    let mut fastest: f32 = 0.0;
    for _ in 0..ticks(0.5) {
        w.step(1);
        fastest = fastest.max(w.vel(e).x);
    }
    println!("thrown at {fastest} m/s");
    assert!(
        (7.0..11.0).contains(&fastest),
        "a 10 m/s log threw the player at {fastest} m/s"
    );
}
