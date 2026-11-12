//! The terrain collider's per-tick budget (docs/BUGS.md #72 round 6): the client's prediction on
//! a phone puts fewer cells in a tick outside the player's reach, and the player moves exactly as
//! with the server's budget.

mod common;

use common::*;
use dwell_physics::DVec3;
use dwell_world::GENERATOR_TERRAIN;

/// Runs (sprints +X) from the seed-0 spawn for `ticks`, with `budget` cells a tick outside the
/// reach (None: the default); returns each tick's feet and the most cells a tick put in.
fn run(budget: Option<usize>, ticks: i32) -> (Vec<DVec3>, u64) {
    let seed = 0;
    let mut w = PlayerTestWorld::world_frame_of(
        dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
        Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
    );
    if let Some(b) = budget {
        w.players.set_collision_cells_per_tick(b);
    }
    let sp = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, seed);
    let h = w.spawn_at(DVec3::new(sp[0], sp[1], sp[2]), 90.0);
    w.set_input(move |_, _| input(0.0, 1.0, true, false, false, 90.0, 0.0));
    let (mut feet, mut most) = (Vec::new(), 0);
    for _ in 0..ticks {
        let before = w.players.terrain().expect("terrain").cells();
        w.step(1);
        most = most.max(w.players.terrain().expect("terrain").cells() - before);
        feet.push(w.players.position(&w.physics, h));
    }
    (feet, most)
}

// face-a-only: a budget of cells, the same on both faces; the run is on face A's spawn.
#[test]
fn a_smaller_collision_budget_bounds_a_ticks_cells_and_moves_the_player_the_same() {
    let budget = 1024;
    let (want, most_default) = run(None, 600);
    let (got, most) = run(Some(budget), 600);
    // Within reach every cell goes in at once: a few thousand at most over the budget.
    assert!(
        most <= budget as u64 + 3000,
        "a tick put {most} cells in (default {most_default})"
    );
    assert!(
        most_default > most,
        "the default budget put in more ({most_default})"
    );
    let moved = (want[want.len() - 1] - want[0]).length();
    assert!(moved > 32.0, "ran {moved} m: across a chunk");
    for (i, (a, b)) in want.iter().zip(&got).enumerate() {
        assert_eq!(a, b, "tick {i}: the player moved differently");
    }
}
