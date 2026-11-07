//! A waterfall's push on players (docs/WATER_SURFACE_GRID.md §2e, PLAYER_CONTROLLER.md §6.4): in a
//! fall's sheet a player is pushed out from the lip and down, and can't jump or swim up it. The mirror
//! harness runs these on face B too (`DWELL_FACE=b`).

mod common;
use common::*;
use dwell_blocks::require;
use dwell_physics::Vec3;

/// A ledge 4 m high (x −6..=0, z 0..=2) carrying a river 1 m deep toward +x at `current` m/s, walled
/// along its sides; the floor below at 0 m. With `pool`, water 2 m deep below it (x 1..=6).
fn ledge(current: f32, pool: bool) -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.fill(-10, -1, -6, 10, -1, 8, cube_block());
    w.fill(-6, 0, 0, 0, 3, 2, cube_block());
    w.fill(-7, 0, -1, 0, 5, -1, cube_block());
    w.fill(-7, 0, 3, 0, 5, 3, cube_block());
    w.fill(-7, 0, 0, -7, 5, 2, cube_block());
    w.fill(-6, 4, 0, 0, 4, 2, require("dwell:water"));
    if pool {
        w.fill(1, 0, -1, 7, 1, -1, cube_block());
        w.fill(1, 0, 3, 7, 1, 3, cube_block());
        w.fill(7, 0, 0, 7, 1, 2, cube_block());
        w.fill(1, 0, 0, 6, 1, 2, require("dwell:water"));
    }
    let face = dwell_world::FaceFrame::at_height(to_world(Vec3::ZERO).y - 1.0);
    for x in -6..=0 {
        for z in 0..=2 {
            let (key, i) = dwell_world::currents::current_key(face, world_cell_x(x), z);
            let mut cells = w
                .world
                .currents
                .get(key)
                .cloned()
                .unwrap_or_else(|| Box::new([[0.0; 2]; dwell_world::currents::CA]));
            cells[i] = [current, 0.0];
            w.world.currents.set(key, Some(cells));
        }
    }
    w
}

#[test]
fn standing_under_a_fall_pushes_the_player_out_from_the_lip() {
    let out = |current: f32| {
        let mut w = ledge(current, false);
        let e = w.spawn(Vec3::new(1.35, 0.0, 1.5));
        w.set_input(|_, _| mv(0.0, 0.0));
        w.step(ticks(1.0));
        w.pos(e).x - 1.35
    };
    let still = out(0.0);
    assert!(still.abs() < 0.05, "no fall, no push: {still}");
    let pushed = out(0.8);
    assert!(pushed > 0.4, "pushed out {pushed} m in a second");
}

#[test]
fn a_fall_cant_be_jumped_up() {
    let peak = |current: f32| {
        let mut w = ledge(current, false);
        let e = w.spawn(Vec3::new(1.35, 0.0, 1.5));
        w.step(ticks(0.3));
        w.set_input(|_, _| input(0.0, 0.0, false, true, false, 0.0, 0.0));
        w.max_feet(e, ticks(1.0))
    };
    let free = peak(0.0);
    assert!(free > 0.8, "a jump with no fall: {free}");
    let under = peak(0.8);
    assert!(under < 0.15, "a jump under the fall: {under}");
}

#[test]
fn a_swimmer_below_a_fall_cant_swim_up_it() {
    let peak = |current: f32| {
        let mut w = ledge(current, true);
        let e = w.spawn(Vec3::new(1.5, 0.5, 1.5));
        w.step(ticks(0.5));
        assert!(w.c(e).swim.swimming);
        // swimming up: jump held, looking up
        w.set_input(|_, _| input(0.0, 1.0, false, true, false, -90.0, 80.0));
        w.max_feet(e, ticks(2.0))
    };
    let free = peak(0.0);
    let under = peak(0.8);
    assert!(
        under < free - 0.3,
        "held under the fall: feet peaked at {under}, {free} with no fall"
    );
}
