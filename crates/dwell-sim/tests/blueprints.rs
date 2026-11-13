//! Blueprints through the server (WORK_QUEUE item 56, docs/BLUEPRINTS.md): placed by a client
//! with `blueprints.world` and sent to every client with it (and none without), saved with the
//! world and back at the next join, removed once every cell holds its block, and built by a
//! builder NPC over ticks, foundation up, skipping a cell a player filled with something else.
//! Both faces: the record is face-local and the ghost goes through `FaceFrame`.

mod common;

use common::*;
use dwell_protocol::blueprints::{Blueprint, BlueprintsMessage};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::blueprint::{Ghost, SHIPPED, built, record_of};
use dwell_world::{FLAT_TOP_Y, FaceFrame, both_faces, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame, db: Option<&std::sync::Arc<WorldDb>>) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        store: db.map(|db| WorldStore::new(db.clone(), false)),
        ..flat()
    }
}

fn step(fx: &mut Fixture, n: u32) {
    for _ in 0..n {
        fx.server.step();
    }
}

fn caps() -> Message {
    Message::Capabilities(capabilities::Capabilities::this_client())
}

/// A stone floor two rows deep under `BASE` and air over it, so the blueprint's cells are loaded.
fn ground(fx: &mut Fixture, f: FaceFrame) {
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    let air = dwell_blocks::require("dwell:air");
    for x in -4..24 {
        for z in -4..24 {
            for r in -2..20 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server
                    .world()
                    .set_voxel(x, y, z, if r < 0 { stone } else { air });
            }
        }
    }
}

/// Puts player `id` standing at face-local (x, z) on the floor.
fn stand(fx: &mut Fixture, f: FaceFrame, id: u16, x: f64, z: f64) {
    let h = fx.server.player_handle_of(id).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = players.half_height(h) as f64;
    let c = f.point([x, m_of_row(BASE) + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(c),
        dwell_physics::Vec3::ZERO,
    );
}

fn blueprint(f: FaceFrame, name: &str, at: [i32; 3], turn: u8) -> Blueprint {
    let s = dwell_world::structure::load(name).expect("a test blueprint");
    record_of(&s, f, at, turn).expect("fits a record")
}

/// The `Blueprints` messages each session was sent since the last call.
fn sent(fx: &mut Fixture, ids: &[SessionId]) -> Vec<Vec<BlueprintsMessage>> {
    let mut all = fx.take_all();
    ids.iter()
        .map(|id| {
            all.remove(id)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|m| match m {
                    Message::Blueprints(m) => Some(m),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// The cells of the world's blueprint `b` holding their block, and those left.
fn progress(fx: &mut Fixture, b: &Blueprint) -> (usize, usize) {
    let g = Ghost::of(b);
    let world = fx.server.world();
    let done = g
        .cells
        .iter()
        .filter(|c| built(world.get_voxel(c.world[0], c.world[1], c.world[2]), c.state))
        .count();
    (done, g.cells.len() - done)
}

both_faces!(
    fn a_placed_blueprint_goes_to_every_capable_client_and_is_saved_with_the_world(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let placed = {
            let mut fx = Fixture::new(config(f, Some(&db)));
            fx.join(1, &Client::new(1));
            fx.join(2, &Client::new(2));
            fx.join(3, &Client::new(3));
            fx.send(1, caps());
            fx.send(2, caps());
            step(&mut fx, 3);
            // A world from before blueprints: an empty list to each capable client, nothing to 3.
            let empty = vec![BlueprintsMessage::All(vec![])];
            assert_eq!(sent(&mut fx, &[1, 2, 3]), [empty.clone(), empty, vec![]]);
            fx.send(
                1,
                Message::Blueprints(BlueprintsMessage::Place(blueprint(
                    f,
                    "dwell:bp_well",
                    [40, BASE, -12],
                    1,
                ))),
            );
            step(&mut fx, 3);
            let list = fx.server.blueprint_list();
            assert_eq!(list.len(), 1);
            let b = list[0].clone();
            assert!(b.id > 0);
            assert_eq!(
                (b.face, b.at, b.turn),
                (u8::from(f.is_b()), [40, BASE, -12], 1)
            );
            let added = vec![BlueprintsMessage::Added(b.clone())];
            assert_eq!(sent(&mut fx, &[1, 2, 3]), [added.clone(), added, vec![]]);
            fx.server.save_now();
            fx.server.flush_store();
            b
        };
        let mut fx = Fixture::new(config(f, Some(&db)));
        fx.join(3, &Client::new(3));
        fx.send(3, caps());
        step(&mut fx, 3);
        assert_eq!(
            sent(&mut fx, &[3]),
            [vec![BlueprintsMessage::All(vec![placed.clone()])]]
        );
        // Removed by a client: everyone is told.
        fx.send(3, Message::Blueprints(BlueprintsMessage::Remove(placed.id)));
        step(&mut fx, 3);
        assert!(fx.server.blueprint_list().is_empty());
        assert_eq!(
            sent(&mut fx, &[3]),
            [vec![BlueprintsMessage::Removed(placed.id)]]
        );
    }
);

#[test]
fn a_client_without_the_capability_cant_place_one_and_the_world_holds_its_most() {
    // face-a-only: the capability exchange and the limit, no geometry.
    let f = FaceFrame::A;
    let mut fx = Fixture::new(config(f, None));
    fx.join(1, &Client::new(1));
    fx.join(2, &Client::new(2));
    fx.send(2, caps());
    step(&mut fx, 2);
    let wall = |x| {
        Message::Blueprints(BlueprintsMessage::Place(blueprint(
            f,
            "dwell:bp_wall",
            [x, BASE, 0],
            0,
        )))
    };
    fx.send(1, wall(0));
    step(&mut fx, 2);
    assert!(fx.server.blueprint_list().is_empty());
    fx.server
        .set_blueprint_rules(dwell_world::blueprint::Rules {
            per_world: 2,
            ..SHIPPED
        });
    for x in [0, 10, 20] {
        fx.send(2, wall(x));
    }
    step(&mut fx, 2);
    assert_eq!(fx.server.blueprint_list().len(), 2);
    // Outside the world: refused.
    fx.server.set_blueprint_rules(SHIPPED);
    fx.send(2, wall(50_000_000));
    step(&mut fx, 2);
    assert_eq!(fx.server.blueprint_list().len(), 2);
}

both_faces!(
    fn a_blueprint_whose_cells_all_hold_their_blocks_is_removed(f: FaceFrame) {
        let mut fx = Fixture::new(config(f, None));
        ground(&mut fx, f);
        fx.join(1, &Client::new(1));
        fx.send(1, caps());
        // No builders: this is the players' own work.
        fx.server
            .set_blueprint_rules(dwell_world::blueprint::Rules {
                builder: dwell_world::blueprint::BuilderRules {
                    per_blueprint: 0,
                    ..SHIPPED.builder
                },
                ..SHIPPED
            });
        let id = fx
            .server
            .place_blueprint(blueprint(f, "dwell:bp_wall", [8, BASE, 8], 0))
            .expect("placed");
        let b = fx.server.blueprint_list()[0].clone();
        step(&mut fx, 40);
        assert_eq!(fx.server.blueprint_list().len(), 1, "not built yet");
        for c in Ghost::of(&b).cells {
            fx.server
                .world()
                .set_voxel(c.world[0], c.world[1], c.world[2], c.state);
        }
        fx.take_all();
        step(&mut fx, 40);
        assert!(fx.server.blueprint_list().is_empty());
        assert_eq!(sent(&mut fx, &[1]), [vec![BlueprintsMessage::Removed(id)]]);
    }
);

both_faces!(
    fn a_builder_builds_the_hut_over_ticks_foundation_up_skipping_an_occupied_cell(f: FaceFrame) {
        let mut fx = Fixture::new(config(f, None));
        ground(&mut fx, f);
        fx.join(1, &Client::new(1));
        fx.send(1, Message::CreaturesHello);
        fx.send(1, caps());
        stand(&mut fx, f, 1, 8.5, 16.5);
        fx.server
            .set_blueprint_rules(dwell_world::blueprint::Rules {
                builder: dwell_world::blueprint::BuilderRules {
                    pace_s: 0.05,
                    ..SHIPPED.builder
                },
                ..SHIPPED
            });
        let at = [8, BASE, 8];
        let id = fx
            .server
            .place_blueprint(blueprint(f, "dwell:bp_hut", at, 0))
            .expect("placed");
        let b = fx.server.blueprint_list()[0].clone();
        // A player's dirt where a wall cell goes.
        let occupied = f.cell([6, BASE + 2, 6]);
        let dirt = f.state(dwell_blocks::require("dwell:dirt"));
        fx.server
            .world()
            .set_voxel(occupied[0], occupied[1], occupied[2], dirt);
        let total = Ghost::of(&b).cells.len();
        step(&mut fx, 2);
        let builders = fx.server.builders_of(id);
        assert_eq!(builders.len(), 1, "a builder came");
        let kind = {
            let c = fx
                .server
                .creatures()
                .iter()
                .find(|c| c.id == builders[0])
                .expect("the builder");
            c.kind
        };
        assert_eq!(
            dwell_sim::creatures::Table::SHIPPED.creatures[kind].name,
            "dwell:builder"
        );
        assert!(
            dwell_sim::creatures::Table::SHIPPED.creatures[kind]
                .npc
                .is_some_and(|n| n.held.is_none()),
            "no sword"
        );
        let mut last = 0;
        let mut floor_done_at = None;
        let mut first_roof = None;
        for t in 0..60 * 240 {
            step(&mut fx, 1);
            let (done, _) = progress(&mut fx, &b);
            assert!(done >= last, "progress never goes back");
            last = done;
            let g = Ghost::of(&b);
            let world = fx.server.world();
            let rows: Vec<(i32, bool)> = g
                .cells
                .iter()
                .map(|c| {
                    let now = world.get_voxel(c.world[0], c.world[1], c.world[2]);
                    (c.local[1], built(now, c.state))
                })
                .collect();
            let row_done = |row: i32| rows.iter().filter(|r| r.0 == row).all(|r| r.1);
            let any_at = |row: i32| rows.iter().any(|r| r.0 == row && r.1);
            if floor_done_at.is_none() && row_done(BASE) {
                floor_done_at = Some(t);
            }
            if first_roof.is_none() && any_at(BASE + 5) {
                first_roof = Some(t);
            }
            if done == total - 1 {
                break;
            }
        }
        assert_eq!(last, total - 1, "every cell but the occupied one built");
        assert!(
            floor_done_at.unwrap() < first_roof.unwrap(),
            "the floor before the roof"
        );
        let world = fx.server.world();
        assert_eq!(world.get_voxel(occupied[0], occupied[1], occupied[2]), dirt);
        // The occupied cell cleared and filled by its builder: the blueprint completes and goes.
        let air = dwell_blocks::require("dwell:air");
        fx.server
            .world()
            .set_voxel(occupied[0], occupied[1], occupied[2], air);
        step(&mut fx, 60 * 20);
        assert!(fx.server.blueprint_list().is_empty(), "completed");
        assert!(fx.server.builders_of(id).is_empty());
        assert!(
            fx.server.creatures().iter().all(|c| c.id != builders[0]),
            "its builder went"
        );
    }
);
