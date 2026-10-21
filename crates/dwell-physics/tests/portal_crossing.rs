//! Through a portal (docs/BIFACIAL_WORLD.md §6, `dwell-worldgen` `portals.rs`; owner 2026-10-21):
//! on a real world's terrain, a player climbs down a portal's ladder from face A, holding forward
//! and looking down, crosses the midplane on the ladder, and climbs on up to face B's opening.
//! Geometry is in world coordinates (the generated world itself has both faces).

mod common;

use common::*;
use dwell_physics::DVec3;
use dwell_world::{Face, FaceFrame, GENERATOR_TERRAIN, m_of_row};
use dwell_worldgen::BifacialTerrain;

#[test]
fn a_player_climbs_down_a_portal_from_face_a_and_up_to_face_b() {
    // both-faces: the climb crosses from face A to face B.
    let seed = 0;
    let g = BifacialTerrain::new(seed);
    let p = g.portals();
    let portal = p.sites[0];
    let (lx, lz) = p.ladder_column(portal);
    let top_a = g.local(Face::A).ladder_top(p, portal);
    let top_b = g.local(Face::B).ladder_top(p, portal);
    let mut w = PlayerTestWorld::world_frame_of(
        dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
        Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
    );
    // In the shaft, one column in front of the ladder and 2 m under its top, facing its wall.
    let [wx, wz] = p.rules.wall;
    let start = DVec3::new(
        f64::from(lx - wx) + 0.5,
        m_of_row(top_a) - 2.0,
        f64::from(lz - wz) + 0.5,
    );
    let yaw0 = f64::from(wx).atan2(f64::from(wz)).to_degrees();
    let h = w.spawn_at(start, yaw0 as f32);
    assert_eq!(w.c(h).face, FaceFrame::A);
    // Walk into the ladder: it holds the player.
    let y0 = yaw0 as f32;
    w.set_input(move |_, _| input(0.0, 1.0, false, false, false, y0, 0.0));
    w.step(20);
    assert!(w.c(h).climb.climbing, "the ladder didn't hold");
    // Look down and hold forward; across the midplane the client continues the look on face B
    // (`FaceFrame::look_across`), which reads as a look up face B's ladder.
    let (mut yaw, mut pitch, mut face) = (yaw0, -80.0f64, FaceFrame::A);
    let mut crossed_at = None;
    let goal = m_of_row(top_b) - 1.5;
    let mut reached = false;
    let mut took = 0;
    for t in 0..ticks(150.0) {
        took = t;
        if w.c(h).face != face {
            face = w.c(h).face;
            (yaw, pitch) = FaceFrame::look_across(yaw, pitch);
            crossed_at.get_or_insert(t);
        }
        let (y, pt) = (yaw as f32, pitch as f32);
        w.set_input(move |_, _| input(0.0, 1.0, false, false, false, y, pt));
        w.step(1);
        assert!(w.c(h).climb.climbing, "tick {t}: let go of the ladder");
        let feet = w.players.feet(&w.physics, h);
        // Face-local height on face B once across.
        if w.c(h).face == FaceFrame::B && FaceFrame::B.y(feet) >= goal {
            reached = true;
            break;
        }
    }
    // The whole climb, from face A's opening to face B's, at the ladder's speed.
    let length = (m_of_row(top_a) - 2.0) + goal;
    let speed = length / (f64::from(took) / 60.0);
    assert!(
        (0.8 * f64::from(w.config.climb.speed)..=1.2 * f64::from(w.config.climb.speed)).contains(&speed),
        "{length} m in {took} ticks: {speed} m/s"
    );
    assert!(crossed_at.is_some(), "never crossed the midplane");
    assert_eq!(w.c(h).face, FaceFrame::B);
    assert!(
        reached,
        "stopped at face B height {} of {goal}",
        FaceFrame::B.y(w.players.feet(&w.physics, h))
    );
}
