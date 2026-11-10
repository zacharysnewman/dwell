//! Creatures through the server (docs/adr/0073-creatures.md): the fixture creature spawns near a
//! joined player by data alone, a client that said `CreaturesHello` is told of it (and only that
//! one), it wanders on the ground, and it despawns when the player goes far (both faces).

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::*;
use dwell_sim::creatures::Table;
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn scene(f: FaceFrame) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        ..flat()
    });
    fx.server.set_creature_table(Table::FIXTURE);
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
