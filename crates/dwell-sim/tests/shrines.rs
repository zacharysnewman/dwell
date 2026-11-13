//! A dead player stands up at the resurrection shrine nearest its death, on the face it died on
//! (docs/SHRINES.md; owner, 2026-11-12: "You respawn at the nearest shrine when you die"), in every
//! game mode; the server readies the terrain round that shrine while the player is dead.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, spawn_spread};
use dwell_world::{CELL_H, FaceFrame, GENERATOR_TERRAIN};
use dwell_worldgen::BifacialTerrain;

const SEED: u64 = 0;

fn terrain(mode: &str) -> ServerConfig {
    ServerConfig {
        generator_version: GENERATOR_TERRAIN,
        world_seed: SEED,
        mode: mode.into(),
        ..ServerConfig::default()
    }
}

/// Where the server stands player `id` up after a death at `death`: the shrine's feet, the
/// players' spread round them and half a metre over the floor, face-local.
fn expected(f: FaceFrame, id: u16, death: [f64; 3]) -> [f64; 3] {
    let g = BifacialTerrain::new(SEED);
    let (_, feet) = g
        .nearest_shrine(f.face, death[0], death[2])
        .expect("a shrine near");
    let [x, y, z] = f.point(feet);
    let [dx, dz] = spawn_spread(id);
    [x + dx, y + CELL_H, z + dz]
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6)
}

dwell_world::both_faces!(
    /// Deaths far apart on face `f` stand the player up at a different shrine each, the one nearest
    /// each death, on that face.
    fn a_death_respawns_at_the_shrine_nearest_it_on_its_face(f: FaceFrame) {
        let fx = Fixture::new(terrain("adventure"));
        let mut seen = Vec::new();
        for (x, z) in [(0.0, 0.0), (3_100.0, -1_700.0), (-4_250.5, 2_900.0)] {
            let death = f.point([x, 120.0, z]);
            let at = fx.server.respawn_point_after(1, death);
            assert!(close(f.point(at), expected(f, 1, death)), "{at:?}");
            assert_eq!(FaceFrame::at_height(at[1]), f, "on the death's face");
            seen.push((at[0].round() as i64, at[2].round() as i64));
        }
        seen.dedup();
        assert_eq!(seen.len(), 3, "three shrines: {seen:?}");
    }
);

dwell_world::both_faces!(
    /// The whole round in creative too (every mode respawns at shrines): a player killed far
    /// from the spawn on face `f` stands up at the shrine nearest its death, whose chunk the respawn
    /// preload streams while it is dead.
    fn a_killed_player_stands_up_at_the_nearest_shrine(f: FaceFrame) {
        let mut fx = Fixture::new(terrain("creative"));
        fx.join(1, &Client::new(1));
        let local = [2_600.5, 140.0, -2_300.5];
        let world = f.point(local);
        fx.move_to(1, world[0], world[1], world[2]);
        fx.server.step();
        let death = fx.server.feet_of(1).expect("joined");
        let want = expected(f, 1, death);
        fx.server.kill_player(1);
        let preload = fx.server.respawn_preload_of(1);
        let mut respawn_at = None;
        for _ in 0..(6 * 60) {
            fx.server.step();
            for (m, _) in fx.take(1) {
                if let Message::PlayerEvent(e) = m
                    && e.kind == PlayerEventKind::Respawn
                {
                    respawn_at = Some(e.position);
                }
            }
            if respawn_at.is_some() {
                break;
            }
        }
        if let Some(c) = preload {
            let at = dwell_world::chunk_of_pos(f.point(want));
            assert!((c.x - at.x).abs() <= 1 && (c.y - at.y).abs() <= 1 && (c.z - at.z).abs() <= 1);
        }
        let respawn_at = respawn_at.expect("a respawn");
        // Placed there: x and z exactly, the height as the body settles (within a cell).
        let got = f.point(respawn_at);
        assert!(
            (got[0] - want[0]).abs() < 1e-6 && (got[2] - want[2]).abs() < 1e-6,
            "{got:?}"
        );
        assert!((got[1] - want[1]).abs() <= CELL_H, "{got:?} vs {want:?}");
        assert_eq!(FaceFrame::at_height(respawn_at[1]).face, f.face);
    }
);
