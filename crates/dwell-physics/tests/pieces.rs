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
            collides: true,
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
fn a_player_cannot_push_a_piece_too_heavy() {
    // A 1.5 t log (over felling.json's pushableMass) stays where it lies.
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
            collides: true,
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
