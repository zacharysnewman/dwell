//! A defeated player stands up at the resurrection shrine nearest where it fell, on that face
//! (docs/SHRINES.md; owner, 2026-11-12: "You respawn at the nearest shrine when you die"), in every
//! game mode; the server readies the terrain round that shrine while the player kneels. Humans
//! never die (WORK_QUEUE item 55): a defeated player or NPC kneels first (CHARACTERS.md §11.13).

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

dwell_world::both_faces!(
    /// WORK_QUEUE item 55 (owner, 2026-11-13: "Humans won't die in this game (general design
    /// rule)"): health 0 is a defeat, never the old death. The player kneels where it fell (others
    /// see it there, `DEAD` being the defeat on the wire), keeps its things (owner, 2026-11-13:
    /// "Keep everything"), and stands up at the shrine nearest where it fell after its defeat's
    /// kneel and fade (shared/defeat), nothing left where it knelt.
    fn a_defeated_player_kneels_where_it_fell_then_stands_at_the_shrine(f: FaceFrame) {
        let mut fx = Fixture::new(terrain("adventure"));
        fx.join(1, &Client::new(1));
        fx.join(2, &Client::new(2));
        let world = f.point([2_600.5, 140.0, -2_300.5]);
        fx.move_to(1, world[0], world[1], world[2]);
        fx.move_to(2, world[0] + 2.0, world[1], world[2]);
        fx.server.step();
        fx.server.give(1, "dwell:stone_axe", 1);
        let knelt = fx.server.feet_of(1).expect("joined");
        let want = expected(f, 1, knelt);
        let kept = fx.server.inventory_of(1).expect("joined").clone();
        fx.server.kill_player(1);
        assert_eq!(fx.server.health_of(1), 0);
        let down = dwell_sim::server::defeat_ticks(dwell_protocol::defeat::PLAYER);
        assert_eq!(
            down,
            (dwell_protocol::defeat::DEFAULT.down_s() * 60.0).round() as u32
        );
        let (mut stood, mut seen_kneeling) = (None, 0);
        for t in 0..down + 60 {
            fx.server.step();
            let out: Vec<Message> = fx
                .server
                .take_outbox()
                .into_iter()
                .filter_map(|o| Message::decode(&o.bytes))
                .collect();
            for m in out {
                match m {
                    Message::PhysicsSnapshot(s) if stood.is_none() => {
                        // Player 2's view of player 1.
                        let Some(r) = s.remotes.iter().find(|r| r.player_id == 1) else {
                            continue;
                        };
                        assert!(r.flags & player_flags::DEAD != 0, "kneeling at {t}");
                        let off = (0..3)
                            .map(|k| (r.position[k] - knelt[k]).abs())
                            .fold(0.0, f64::max);
                        assert!(off < 0.01, "where it fell: {:?}", r.position);
                        seen_kneeling += 1;
                    }
                    Message::PlayerEvent(e) if e.kind == PlayerEventKind::Respawn => {
                        stood = Some((t, e.position));
                    }
                    _ => {}
                }
            }
            if stood.is_none() {
                assert!(fx.server.loose_items().is_empty(), "nothing left at {t}");
            }
            if stood.is_some() {
                break;
            }
        }
        assert!(seen_kneeling > 10, "seen kneeling {seen_kneeling} times");
        let (t, at) = stood.expect("stood up");
        assert!(t + 1 >= down && t <= down + 1, "at tick {t} of {down}");
        let got = f.point(at);
        assert!(
            (got[0] - want[0]).abs() < 1e-6 && (got[2] - want[2]).abs() < 1e-6,
            "{got:?} vs {want:?}"
        );
        assert_eq!(FaceFrame::at_height(at[1]).face, f.face);
        assert_eq!(fx.server.health_of(1), players::MAX_HEALTH as i32);
        // It kept its things: nothing was left where it knelt.
        assert!(fx.server.loose_items().is_empty());
        assert_eq!(fx.server.inventory_of(1), Some(&kept));
    }
);

dwell_world::both_faces!(
    /// WORK_QUEUE item 55: an NPC brought to 0 health is defeated, not killed: it stays (no drops,
    /// not removed), kneels still and unhurt where it fell for its kind's kneel (the villager's
    /// override, shared/defeat), sent kneeling (pose 5) to a client with `humans.defeat` and
    /// standing (0) to one without; then it stands up whole at the shrine nearest where it fell, on
    /// that face (pose 6), and wanders again.
    fn a_defeated_npc_kneels_then_stands_at_the_nearest_shrine(f: FaceFrame) {
        let mut fx = Fixture::new(terrain("adventure"));
        fx.join(1, &Client::new(1));
        let world = f.point([2_600.5, 140.0, -2_300.5]);
        fx.move_to(1, world[0], world[1], world[2]);
        fx.server.step();
        let feet = fx.server.feet_of(1).expect("joined");
        let at = {
            let l = f.point(feet);
            f.point([l[0] + 3.0, l[1], l[2]])
        };
        let ids = fx.server.spawn_npcs_dev("dwell:villager", at, 1, 0.0, 0);
        let id = ids[0];
        let knelt = fx
            .server
            .creatures()
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .position;
        let hurt = fx
            .server
            .creatures_mut()
            .hurt(id, 1e6, 0.0, feet)
            .expect("hurt");
        assert!(hurt.defeated && !hurt.killed, "{hurt:?}");
        assert!(
            fx.server.creatures_mut().hurt(id, 1.0, 0.0, feet).is_none(),
            "unhurt while kneeling"
        );
        let table = fx.server.creatures_mut().table;
        let c = fx.server.creatures().iter().find(|c| c.id == id).unwrap();
        assert_eq!(c.npc_net_for(&table, true, true).unwrap().pose, 5);
        assert_eq!(c.npc_net_for(&table, true, false).unwrap().pose, 0);
        // A second player waits at the shrine, so the NPC standing up there isn't despawned as
        // far from every player.
        let shrine = BifacialTerrain::new(SEED)
            .nearest_shrine(f.face, knelt[0], knelt[2])
            .expect("a shrine")
            .1;
        fx.join(2, &Client::new(2));
        let by = f.point(shrine);
        let by = f.point([by[0] - 3.0, by[1] + 1.0, by[2]]);
        fx.move_to(2, by[0], by[1], by[2]);
        let down = (dwell_protocol::defeat::of("dwell:villager").down_s() * 60.0).round() as u32;
        assert!(down > (dwell_protocol::defeat::DEFAULT.down_s() * 60.0) as u32);
        for t in 0..down - 1 {
            fx.server.step();
            let c = fx
                .server
                .creatures()
                .iter()
                .find(|c| c.id == id)
                .expect("kept");
            let (a, b) = (f.point(c.position), f.point(knelt));
            assert!(
                (a[0] - b[0]).hypot(a[2] - b[2]) < 0.05,
                "still where it fell at {t}: {a:?} vs {b:?}"
            );
        }
        for _ in 0..3 {
            fx.server.step();
        }
        let c = fx
            .server
            .creatures()
            .iter()
            .find(|c| c.id == id)
            .expect("kept");
        let (got, want) = (f.point(c.position), f.point(shrine));
        assert!(
            (got[0] - want[0]).hypot(got[2] - want[2]) < 2.0,
            "{got:?} by the shrine {want:?}"
        );
        assert_eq!(FaceFrame::at_height(c.position[1]).face, f.face);
        assert_eq!(c.npc_net_for(&table, true, true).unwrap().pose, 6);
        assert!(c.health > 0.0);
        let stand = (dwell_protocol::defeat::STAND_S * 60.0).round() as u32;
        for _ in 0..stand + 2 {
            fx.server.step();
        }
        let c = fx
            .server
            .creatures()
            .iter()
            .find(|c| c.id == id)
            .expect("kept");
        assert!(c.defeat.is_none(), "standing again");
        assert!(c.npc_net_for(&table, true, true).unwrap().pose < 5);
    }
);
