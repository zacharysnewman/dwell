//! The controller on the bifacial world (BIFACIAL_WORLD.md §3, §4, §6; PLAYER_CONTROLLER.md §11):
//! down is toward the midplane, so a face-B player stands upside down; the flip band at the
//! midplane; crossing between the faces. Geometry is in world coordinates: these tests are about
//! the faces, so they ignore `DWELL_ORIGIN_X` and `DWELL_FACE` (the other suites run mirrored).

mod common;

use common::*;
use dwell_physics::controller::events as ev;
use dwell_physics::{DVec3, Vec3};
use dwell_world::{FaceFrame, MIDPLANE_M};

/// A player on face B hangs under a slab whose lower face is at y = −4,098.
const SLAB_BOTTOM: i32 = -4098;
const MID: f64 = MIDPLANE_M;

fn build_slab(w: &mut PlayerTestWorld) {
    for z in -8..8 {
        for x in -8..8 {
            for y in SLAB_BOTTOM..SLAB_BOTTOM + 2 {
                w.set_world(x, y, z, cube_block());
            }
        }
    }
}

/// A 9 × 9 block of rock across the midplane (y −2,064 … −2,033) with a 1 × 1 shaft through it.
fn build_shaft(w: &mut PlayerTestWorld) {
    for z in -4..=4 {
        for x in -4..=4 {
            if x == 0 && z == 0 {
                continue;
            }
            for y in -2064..=-2033 {
                w.set_world(x, y, z, cube_block());
            }
        }
    }
}

fn close(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn a_face_b_player_stands_upside_down_under_the_ground() {
    let mut w = PlayerTestWorld::world_frame();
    build_slab(&mut w);
    let h = w.spawn_at(DVec3::new(0.5, f64::from(SLAB_BOTTOM), 0.5), 0.0);
    assert_eq!(w.c(h).face, FaceFrame::B);
    w.step(60);
    assert!(w.c(h).ground.grounded);
    assert_eq!(w.c(h).state, State::Idle);
    // The capsule hangs from the slab: feet at its lower face, the head 1.8 m below.
    let (feet, head, y) = (
        w.players.feet(&w.physics, h),
        w.players.head(&w.physics, h),
        w.players.position(&w.physics, h).y,
    );
    assert!(close(feet, f64::from(SLAB_BOTTOM), 1e-5), "{feet}");
    assert!(close(head, f64::from(SLAB_BOTTOM) - 1.8, 1e-5), "{head}");
    assert!(close(y, f64::from(SLAB_BOTTOM) - 0.9, 1e-5), "{y}");
}

#[test]
fn a_face_b_player_falls_toward_the_midplane_and_lands_on_a_floor_above() {
    let mut w = PlayerTestWorld::world_frame();
    // A floor at y −3,000: a player 20 m below it falls up and lands on it, head away from it.
    for z in -8..8 {
        for x in -8..8 {
            w.set_world(x, -3000, z, cube_block());
        }
    }
    let y0 = -3000.0 - 20.0;
    let h = w.spawn_at(DVec3::new(0.5, y0, 0.5), 0.0);
    assert_eq!(w.c(h).face, FaceFrame::B);
    for _ in 0..240 {
        if w.c(h).ground.grounded {
            break;
        }
        w.step(1);
        let vy = w.players.velocity(&w.physics, h).y;
        assert!(vy >= -1e-4, "never away from the midplane: {vy}");
    }
    assert!(w.c(h).ground.grounded);
    assert_eq!(w.count(h, ev::LANDED), 1);
    // Free fall of 20 m at 20 m/s²: ~28 m/s on landing, as on face A.
    assert!(
        approx(w.c(h).landed_speed, (2.0f32 * 20.0 * 20.0).sqrt(), 0.05),
        "{}",
        w.c(h).landed_speed
    );
    assert!(close(w.players.feet(&w.physics, h), -3000.0, 1e-4));
}

#[test]
fn jumping_on_face_b_rises_toward_minus_y_by_the_same_height() {
    let mut w = PlayerTestWorld::world_frame();
    build_slab(&mut w);
    let h = w.spawn_at(DVec3::new(0.5, f64::from(SLAB_BOTTOM), 0.5), 0.0);
    w.step(30);
    assert!(w.c(h).ground.grounded);
    let start = w.players.feet(&w.physics, h);
    let t0 = w.tick;
    w.set_input(move |tick, _| input(0.0, 0.0, false, tick - t0 < 3, false, 0.0, 0.0));
    let mut lowest = start;
    for _ in 0..90 {
        w.step(1);
        lowest = lowest.min(w.players.feet(&w.physics, h));
    }
    let height = (start - lowest) as f32;
    assert!(approx(height, w.config.jump.height, 0.05), "{height}");
    assert!(w.c(h).ground.grounded);
}

#[test]
fn strafing_right_is_toward_the_players_right_on_either_face() {
    // Facing +Z (yaw 0) an upright player's right is −X; an upside-down player's, +X.
    for face_b in [false, true] {
        let mut w = PlayerTestWorld::world_frame();
        if face_b {
            build_slab(&mut w);
        } else {
            for z in -8..8 {
                for x in -8..8 {
                    w.set_world(x, -1, z, cube_block());
                }
            }
        }
        let h = w.spawn_at(
            DVec3::new(0.5, if face_b { f64::from(SLAB_BOTTOM) } else { 0.0 }, 0.5),
            0.0,
        );
        w.step(30);
        let x0 = w.players.position(&w.physics, h).x;
        w.set_input(|_, _| mv(1.0, 0.0));
        w.step(30);
        let dx = w.players.position(&w.physics, h).x - x0;
        assert!(dx.abs() > 1.0, "face_b {face_b}: {dx}");
        assert_eq!(dx < 0.0, !face_b, "face_b {face_b}: {dx}");
    }
}

#[test]
fn the_flip_band_accelerates_a_body_falling_down_a_shaft_across_the_midplane() {
    for side in [1.0, -1.0] {
        let mut w = PlayerTestWorld::world_frame();
        build_shaft(&mut w);
        let h = w.spawn_at(DVec3::new(0.5, MID + side * 20.0, 0.5), 0.0);
        assert_eq!(w.c(h).face, FaceFrame::of_sign(side));
        let (mut flips, mut face, mut fastest_in_band) = (0, w.c(h).face, 0.0f64);
        for _ in 0..60 * 4 {
            w.step(1);
            if (w.players.position(&w.physics, h).y - MID).abs() < 4.0 {
                fastest_in_band =
                    fastest_in_band.max(w.players.velocity(&w.physics, h).y.abs().into());
            }
            if w.c(h).face != face {
                flips += 1;
                face = w.c(h).face;
            }
        }
        // Free fall over 20 m reaches 28 m/s; the band's acceleration adds to it.
        assert!(flips >= 1, "side {side}");
        assert!(fastest_in_band > 30.0, "side {side}: {fastest_in_band}");
    }
}

#[test]
fn crossing_the_midplane_by_swimming_switches_the_face() {
    let mut w = PlayerTestWorld::world_frame();
    build_shaft(&mut w);
    let h = w.spawn_at(DVec3::new(0.5, MID + 6.0, 0.5), 0.0);
    // Press down (crouch: toward the midplane) until across it; on the far side "down" points
    // back, so the player presses up (jump) to go on — away from the midplane.
    assert_eq!(w.c(h).face, FaceFrame::A);
    let (mut flips, mut face, mut lowest) = (0, w.c(h).face, MID);
    for _ in 0..60 * 3 {
        let across = w.c(h).face.is_b();
        w.set_input(move |_, _| input(0.0, 0.0, false, across, !across, 0.0, 0.0));
        w.step(1);
        if w.c(h).face != face {
            flips += 1;
            face = w.c(h).face;
        }
        lowest = lowest.min(w.players.position(&w.physics, h).y);
    }
    // Out of the band the pull is back toward the midplane, so the player may come back across.
    assert!(flips >= 1);
    // On, into the far side of the band (3 m/s for the best part of three seconds).
    assert!(lowest < MID - 3.0, "{lowest}");
}

#[test]
fn crossing_keeps_the_motion() {
    let mut w = PlayerTestWorld::world_frame();
    build_shaft(&mut w);
    let h = w.spawn_at(DVec3::new(0.5, MID + 0.5, 0.5), 0.0);
    w.step(1);
    w.players
        .add_velocity(&mut w.physics, h, Vec3::new(0.0, -20.0, 0.0));
    let (mut before, mut after) = (0.0f32, 0.0f32);
    let face = w.c(h).face;
    for _ in 0..60 * 2 {
        if after != 0.0 {
            break;
        }
        before = w.players.velocity(&w.physics, h).y;
        w.step(1);
        if w.c(h).face != face {
            after = w.players.velocity(&w.physics, h).y;
        }
    }
    assert!(after != 0.0);
    // Still moving the same way, at about the same speed: not reversed, not lost, by the switch.
    assert!(before < 0.0 && after < 0.0, "{before} {after}");
    assert!((after - before).abs() < 1.5, "{before} {after}");
}
