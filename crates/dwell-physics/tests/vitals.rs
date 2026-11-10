//! Stamina and oxygen in the controller (PLAYER_CONTROLLER.md §6.12, ADR 0069). Like every suite
//! here, run on face A and, with `DWELL_FACE=b`, on its mirror image.

mod common;
use common::*;
use dwell_blocks::require;
use dwell_physics::Vec3;
use dwell_physics::controller::{State, VITALS, Vitals, VitalsRules};

const ADVENTURE: VitalsRules = VitalsRules {
    stamina: true,
    oxygen: true,
};

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A 6 m deep pool (x −4..4, z −4..4) in a floor whose top is at y = 0.
fn pool(w: &mut PlayerTestWorld) {
    w.fill(-10, -8, -10, 10, -1, 10, cube_block());
    w.fill(-4, -6, -4, 4, -1, 4, require("dwell:water"));
}

fn exhausted() -> Vitals {
    Vitals {
        stamina: 0.0,
        exhausted: true,
        rest_ticks: 0,
        ..Vitals::default()
    }
}

#[test]
fn sprinting_drains_stamina_and_exhausted_a_player_cannot_sprint() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 128);
    w.players.set_vitals(ADVENTURE);
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    w.step(ticks(2.0));
    assert_eq!(w.c(e).state, State::Running);
    let spent = VITALS.max_stamina - w.c(e).vitals.stamina;
    // About 2 s of the sprint rate (the first ticks accelerate at a walk).
    let rate = VITALS.drain(dwell_physics::controller::Activity::Sprint);
    assert!(spent > rate * 1.6 && spent < rate * 2.05, "{spent}");
    // Run it out: exhausted, the run input no longer runs.
    w.step(ticks(VITALS.max_stamina / rate));
    assert!(w.c(e).vitals.exhausted, "{:?}", w.c(e).vitals);
    w.step(5);
    assert_eq!(w.c(e).state, State::Walking);
    let speed = w.horizontal_speed(e);
    assert!(speed < 5.5, "{speed} m/s while exhausted");
}

#[test]
fn creative_never_drains_or_exhausts() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 128);
    let e = w.spawn(v(0.5, 0.0, 0.5));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    w.step(ticks(14.0));
    assert_eq!(w.c(e).state, State::Running);
    assert_eq!(w.c(e).vitals, Vitals::default());
}

#[test]
fn a_rested_swimmer_floats_and_an_exhausted_one_sinks() {
    for tired in [false, true] {
        let mut w = PlayerTestWorld::new();
        pool(&mut w);
        w.players.set_vitals(ADVENTURE);
        let e = w.spawn(v(0.5, -2.0, 0.5));
        w.step(ticks(2.0));
        assert!(w.c(e).swim.swimming);
        let start = w.feet(e);
        if tired {
            w.players.controller_mut(e).vitals = exhausted();
        }
        // Holding swim-up too: exhausted, it is ignored.
        w.set_input(|_, _| input(0.0, 0.0, false, true, false, 0.0, 0.0));
        w.step(ticks(1.0));
        let feet = w.feet(e);
        if tired {
            assert!(feet < start - 0.8, "sank from {start} to {feet}");
            assert!(w.c(e).vitals.exhausted);
        } else {
            assert!(feet > start, "floated from {start} to {feet}");
        }
    }
}

#[test]
fn an_exhausted_swimmer_rests_back_past_the_share_and_floats_again() {
    let mut w = PlayerTestWorld::new();
    pool(&mut w);
    w.players.set_vitals(ADVENTURE);
    let e = w.spawn(v(0.5, -2.0, 0.5));
    w.step(ticks(1.0));
    w.players.controller_mut(e).vitals = exhausted();
    // It sinks resting: back to the share in the delay plus the share's regen, not before.
    let back = VITALS.regen_delay_seconds + VITALS.recover_at() / VITALS.regen_per_second;
    let mut t = 0;
    while w.c(e).vitals.exhausted && t < ticks(back + 1.0) {
        w.step(1);
        t += 1;
    }
    let c = w.c(e).vitals;
    assert!(!c.exhausted && c.stamina >= VITALS.recover_at() - 0.1, "{c:?}");
    assert!((t - ticks(back)).abs() <= 2, "{t} ticks to recover");
    // Treading now, it spends again from the share without locking (25 → 24 % is fine).
    w.step(ticks(1.0));
    let c = w.c(e).vitals;
    assert!(c.stamina < VITALS.recover_at() + 1.0 && !c.exhausted, "{c:?}");
    let rising = w.vel(e).y;
    assert!(rising > -0.2, "treading holds it up: {rising}");
}

#[test]
fn air_drains_with_the_head_under_and_refills_on_surfacing() {
    let mut w = PlayerTestWorld::new();
    pool(&mut w);
    w.players.set_vitals(ADVENTURE);
    let e = w.spawn(v(0.5, -5.5, 0.5));
    // Dive and stay down (crouch descends).
    w.set_input(|_, _| input(0.0, 0.0, false, false, true, 0.0, 0.0));
    w.step(ticks(3.0));
    let o = w.c(e).vitals.oxygen;
    assert!(
        (o - (VITALS.oxygen_seconds - 3.0)).abs() < 0.1,
        "{o} s of air left"
    );
    // Up and out of it: it fills again.
    w.set_input(|_, _| input(0.0, 0.0, false, true, false, 0.0, 0.0));
    w.step(ticks(4.0));
    assert!(w.c(e).swim.submerged < VITALS.head_fraction);
    assert_eq!(w.c(e).vitals.oxygen, VITALS.oxygen_seconds);
}
