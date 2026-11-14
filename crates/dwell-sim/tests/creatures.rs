//! Creatures through the server (docs/adr/0073-creatures.md): the fixture creature spawns near a
//! joined player by data alone, a client that said `CreaturesHello` is told of it (and only that
//! one), it wanders on the ground, and it despawns when the player goes far (both faces); NPCs'
//! part goes only to clients with the `creatures.npcs` capability.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::*;
use dwell_sim::creatures::Table;
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn scene(f: FaceFrame) -> Fixture {
    scene_with(f, Table::FIXTURE, false)
}

/// The scene with `table`'s creatures; client 2 also says hello, with the `creatures.npcs`
/// capability when `npcs`.
fn scene_with(f: FaceFrame, table: Table, npcs: bool) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        ..flat()
    });
    fx.server.set_creature_table(table);
    let stone = f.state(require("dwell:stone"));
    // A stone floor over the whole chunk column the player stands in (the fixture's spawnChunks 0).
    for x in -2..34 {
        for z in -2..34 {
            for r in -2..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    fx.join(1, &Client::new(1));
    fx.join(2, &Client::new(2));
    fx.send(1, Message::InventoryAction(InventoryAction::Hello));
    fx.send(1, Message::CreaturesHello);
    if npcs {
        let caps = dwell_protocol::capabilities::Capabilities::this_client();
        assert!(caps.has(capability_names::CREATURES_NPCS));
        fx.send(2, Message::Capabilities(caps));
        fx.send(2, Message::CreaturesHello);
    }
    let h = fx.server.player_handle_of(1).expect("a live player");
    for id in [1u16, 2] {
        let h = fx.server.player_handle_of(id).unwrap_or(h);
        let (_, physics, players) = fx.server.sim();
        let half = players.half_height(h) as f64;
        let c = f.point([8.5, m_of_row(BASE) + half, 8.5]);
        players.teleport(
            physics,
            h,
            dwell_physics::DVec3::from(c),
            dwell_physics::Vec3::ZERO,
        );
    }
    fx.take_all();
    fx
}

fn told(msgs: &[Message]) -> Vec<CreatureNet> {
    msgs.iter()
        .filter_map(|m| match m {
            Message::Creatures(c) => Some(c.creatures.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

dwell_world::both_faces!(
    fn the_fixture_creature_spawns_wanders_is_told_and_despawns(f: FaceFrame) {
        let mut fx = scene(f);
        for _ in 0..61 {
            fx.server.step();
        }
        assert_eq!(
            fx.server.creatures().len(),
            3,
            "the fixture's group, by data alone"
        );
        let start: Vec<_> = fx.server.creatures().iter().map(|c| c.position).collect();
        let all = fx.take_all();
        let one = told(all.get(&1).map_or(&[][..], |v| v.as_slice()));
        assert!(
            one.iter().any(|c| c.model == "penguin"),
            "the hello client is told"
        );
        assert!(
            told(all.get(&2).map_or(&[][..], |v| v.as_slice())).is_empty(),
            "the other is not"
        );
        for _ in 0..60 * 20 {
            fx.server.step();
        }
        let moved = fx
            .server
            .creatures()
            .iter()
            .zip(&start)
            .any(|(c, s)| (c.position[0] - s[0]).abs() + (c.position[2] - s[2]).abs() > 0.5);
        assert!(moved, "they wander");
        for c in fx.server.creatures() {
            let l = f.point(c.position);
            assert!((l[1] - m_of_row(BASE)).abs() < 1e-6, "on the floor: {l:?}");
        }
        fx.take_all();
        for id in [1u16, 2] {
            fx.move_to(id, 900.0, f.y(m_of_row(BASE)), 900.0);
        }
        fx.server.step();
        assert!(
            fx.server.creatures().is_empty(),
            "despawned far from every player"
        );
        let gone = fx.take_all();
        assert!(
            gone.get(&1).is_some_and(|v| v
                .iter()
                .any(|m| matches!(m, Message::Creatures(c) if c.gone.len() == 3))),
            "the client is told they are gone"
        );
    }
);

// WORK_QUEUE item 38: NPCs ride the creature messages; their part (kind, pose, palette variant)
// goes only to a client with the `creatures.npcs` capability, an older client (1) reading them
// as plain creatures.
dwell_world::both_faces!(
    fn npcs_are_told_with_their_part_only_to_clients_with_the_capability(f: FaceFrame) {
        let mut fx = scene_with(f, Table::NPC_FIXTURE, true);
        for _ in 0..61 {
            fx.server.step();
        }
        let n = fx.server.creatures().len();
        assert!(n >= 1, "the fixture NPC spawns by data alone");
        let all = fx.take_all();
        let msgs = |id: u32| -> Vec<CreaturesMessage> {
            all.get(&id)
                .map_or(&[][..], |v| v.as_slice())
                .iter()
                .filter_map(|m| match m {
                    Message::Creatures(c) => Some(c.clone()),
                    _ => None,
                })
                .collect()
        };
        let (old, new) = (msgs(1), msgs(2));
        assert!(
            old.iter()
                .any(|m| m.creatures.iter().any(|c| c.model == "peasant"))
        );
        assert!(
            old.iter().all(|m| m.npcs.is_none()),
            "no part for an older client"
        );
        // WORK_QUEUE item 39: the animals' poses' part, likewise, only to a client with
        // `creatures.poses` (here client 2, whose capabilities are this build's).
        assert!(
            old.iter().all(|m| m.poses.is_none()),
            "no poses for an older client"
        );
        assert!(
            new.iter().all(|m| m.poses.is_some()),
            "the poses' part for a client with the capability"
        );
        assert!(
            dwell_protocol::capabilities::Capabilities::this_client()
                .has(capability_names::CREATURES_POSES)
        );
        let part: Vec<NpcNet> = new
            .iter()
            .filter_map(|m| m.npcs.clone())
            .flatten()
            .collect();
        assert!(!part.is_empty());
        for p in &part {
            let c = fx
                .server
                .creatures()
                .iter()
                .find(|c| c.id == p.id)
                .expect("its creature");
            assert_eq!(p.kind, "test:person");
            assert_eq!(p.variant, c.variant);
        }
    }
);

#[test]
fn the_wire_poses_are_the_ecologys_idle_poses_in_order() {
    // both-faces: no world geometry, the pose names only.
    assert_eq!(
        dwell_protocol::CREATURE_POSES,
        dwell_ecology::IdlePose::NAMES
    );
}
