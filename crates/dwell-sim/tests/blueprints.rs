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

/// Stone under `BASE` and air over it across x in `xs`, z in `zs` (face-local).
fn ground_at(fx: &mut Fixture, f: FaceFrame, xs: std::ops::Range<i32>, zs: std::ops::Range<i32>) {
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    let air = dwell_blocks::require("dwell:air");
    for x in xs {
        for z in zs.clone() {
            for r in -2..24 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server
                    .world()
                    .set_voxel(x, y, z, if r < 0 { stone } else { air });
            }
        }
    }
}

both_faces!(
    fn a_town_site_near_a_player_is_founded_once_its_plan_s_blueprints_on_the_ground(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let rules = dwell_world::blueprint::Rules {
            towns: dwell_world::blueprint::TownRules {
                spacing_m: 256,
                ..SHIPPED.towns
            },
            ..SHIPPED
        };
        let flat: dwell_worldgen::TownGround = std::sync::Arc::new(|_, _, _| Some(m_of_row(BASE)));
        let seed = flat_seed();
        let g = |_: i32, _: i32| Some(m_of_row(BASE));
        let site = rules.towns.sites_near(seed, f, 0.0, 0.0, 600.0, &g)[0];
        let layout = rules.towns.layout(&site);
        {
            let mut fx = Fixture::new(config(f, Some(&db)));
            fx.server.set_blueprint_rules(rules);
            fx.server.set_town_ground(Some(flat.clone()));
            ground_at(
                &mut fx,
                f,
                site.x - 24..site.x + 24,
                site.z - 24..site.z + 24,
            );
            fx.join(1, &Client::new(1));
            stand(
                &mut fx,
                f,
                1,
                f64::from(site.x) + 20.5,
                f64::from(site.z) + 20.5,
            );
            step(&mut fx, 2 * 60 + 2);
            let list = fx.server.blueprint_list();
            assert_eq!(list.len(), layout.len(), "the plan's blueprints");
            for (b, (name, [x, z], turn)) in list.iter().zip(&layout) {
                assert_eq!(&b.name, name);
                assert_eq!(
                    (b.face, b.at, b.turn),
                    (u8::from(f.is_b()), [*x, BASE, *z], *turn)
                );
            }
            assert_eq!(
                fx.server.towns_founded(),
                vec![(u8::from(f.is_b()), site.cell[0], site.cell[1])]
            );
            step(&mut fx, 2 * 60 + 2);
            assert_eq!(
                fx.server.blueprint_list().len(),
                layout.len(),
                "founded once"
            );
            fx.server.save_now();
            fx.server.flush_store();
        }
        let mut fx = Fixture::new(config(f, Some(&db)));
        fx.server.set_blueprint_rules(rules);
        fx.server.set_town_ground(Some(flat));
        ground_at(
            &mut fx,
            f,
            site.x - 24..site.x + 24,
            site.z - 24..site.z + 24,
        );
        fx.join(1, &Client::new(1));
        stand(
            &mut fx,
            f,
            1,
            f64::from(site.x) + 20.5,
            f64::from(site.z) + 20.5,
        );
        step(&mut fx, 2 * 60 + 2);
        assert_eq!(
            fx.server.blueprint_list().len(),
            layout.len(),
            "not founded again"
        );
        assert_eq!(fx.server.towns_founded().len(), 1);
    }
);

#[test]
fn a_town_site_with_no_ground_yet_waits_and_a_world_without_a_height_field_has_none() {
    // face-a-only: the waiting and the off switch, no geometry.
    let f = FaceFrame::A;
    let mut fx = Fixture::new(config(f, None));
    fx.join(1, &Client::new(1));
    // No town ground (a test generator, the flat world): nothing founded.
    step(&mut fx, 2 * 60 + 2);
    assert!(fx.server.towns_founded().is_empty());
    let rules = dwell_world::blueprint::Rules {
        towns: dwell_world::blueprint::TownRules {
            spacing_m: 256,
            ..SHIPPED.towns
        },
        ..SHIPPED
    };
    fx.server.set_blueprint_rules(rules);
    fx.server
        .set_town_ground(Some(std::sync::Arc::new(|_, _, _| Some(m_of_row(BASE)))));
    let g = |_: i32, _: i32| Some(m_of_row(BASE));
    let site = rules.towns.sites_near(flat_seed(), f, 0.0, 0.0, 600.0, &g)[0];
    stand(
        &mut fx,
        f,
        1,
        f64::from(site.x) + 0.5,
        f64::from(site.z) + 0.5,
    );
    step(&mut fx, 2 * 60 + 2);
    assert!(
        fx.server.towns_founded().is_empty(),
        "no ground there yet: it waits"
    );
    assert!(fx.server.blueprint_list().is_empty());
    // The ground there: founded.
    ground_at(
        &mut fx,
        f,
        site.x - 24..site.x + 24,
        site.z - 24..site.z + 24,
    );
    // (The player fell through the empty world meanwhile: stood on the new ground.)
    stand(
        &mut fx,
        f,
        1,
        f64::from(site.x) + 20.5,
        f64::from(site.z) + 20.5,
    );
    step(&mut fx, 2 * 60 + 2);
    assert_eq!(fx.server.towns_founded().len(), 1);
}

/// The seed the fixture's server runs with.
fn flat_seed() -> u64 {
    config(FaceFrame::A, None).world_seed
}

// --- B5: the storehouse and the builders' trips (docs/BLUEPRINTS.md §9) ---

/// Fast builders (a piece every 0.05 s), as the hut test's.
fn fast() -> dwell_world::blueprint::Rules {
    dwell_world::blueprint::Rules {
        builder: dwell_world::blueprint::BuilderRules {
            pace_s: 0.05,
            ..SHIPPED.builder
        },
        ..SHIPPED
    }
}

/// Fills every cell of the world's blueprint `b` with its block (a built storehouse).
fn build_all(fx: &mut Fixture, b: &Blueprint) {
    for c in Ghost::of(b).cells {
        fx.server
            .world()
            .set_voxel(c.world[0], c.world[1], c.world[2], c.state);
    }
}

/// A built storehouse at face-local (16, BASE, 8) (its box x 14..18, z 7..10) and the hut's
/// blueprint at (8, BASE, 8) (x 6..10, z 6..10); the hut's id and the storehouse's box.
fn storehouse_and_hut(fx: &mut Fixture, f: FaceFrame) -> (u32, Blueprint) {
    let sid = fx
        .server
        .place_blueprint(blueprint(f, "dwell:bp_storehouse", [16, BASE, 8], 0))
        .expect("the storehouse placed");
    fx.server.add_storehouse(sid).expect("a storehouse");
    let store = fx.server.blueprint_list()[0].clone();
    build_all(fx, &store);
    // The hut waits for the storehouse: no builder until it is built (checked twice a second).
    let id = fx
        .server
        .place_blueprint(blueprint(f, "dwell:bp_hut", [8, BASE, 8], 0))
        .expect("placed");
    step(fx, 31);
    assert_eq!(fx.server.storehouses().len(), 1);
    assert!(fx.server.storehouses()[0].3, "built: open");
    assert_eq!(
        fx.server.blueprint_list().len(),
        1,
        "the storehouse's blueprint went"
    );
    (id, fx.server.blueprint_list()[0].clone())
}

/// The flat distance from a creature's face-local feet to the storehouse's box (x 14..19, z 7..11).
fn to_store(f: FaceFrame, at: [f64; 3]) -> f64 {
    let p = f.point(at);
    let dx = (14.0 - p[0]).max(p[0] - 19.0).max(0.0);
    let dz = (7.0 - p[2]).max(p[2] - 11.0).max(0.0);
    dx.hypot(dz)
}

both_faces!(
    fn a_builder_fetches_from_the_storehouse_carrying_at_most_ten_and_finishes_it(f: FaceFrame) {
        let mut fx = Fixture::new(config(f, None));
        ground(&mut fx, f);
        fx.join(1, &Client::new(1));
        fx.send(1, Message::CreaturesHello);
        fx.send(1, caps());
        stand(&mut fx, f, 1, 8.5, 18.5);
        fx.server.set_blueprint_rules(fast());
        let (id, hut) = storehouse_and_hut(&mut fx, f);
        let total = Ghost::of(&hut).cells.len();
        let cap = SHIPPED.builder_job().carry_max;
        let mut builder = None;
        let (mut last_done, mut last_count) = (0, 0u32);
        let (mut loads, mut most) = (0, 0u32);
        let mut away = false;
        let mut done_at = None;
        for t in 0..60 * 400 {
            step(&mut fx, 1);
            if fx.server.blueprint_list().is_empty() {
                done_at = Some(t);
                break;
            }
            let Some(&b) = builder.as_ref().or(fx.server.builders_of(id).first()) else {
                continue;
            };
            if builder.is_none() {
                // Watched from here (it may have begun while the storehouse opened).
                builder = Some(b);
                last_done = progress(&mut fx, &hut).0;
                last_count = fx.server.builder_load(b).unwrap().iter().map(|l| l.1).sum();
                continue;
            }
            let load = fx.server.builder_load(b).expect("its load");
            let count: u32 = load.iter().map(|l| l.1).sum();
            let at = fx
                .server
                .creatures()
                .iter()
                .find(|c| c.id == b)
                .expect("the builder")
                .position;
            let (done, _) = progress(&mut fx, &hut);
            assert!(count <= cap, "at most {cap} carried: {count}");
            most = most.max(count);
            if count > last_count {
                // Loaded only at the storehouse.
                assert!(
                    to_store(f, at) <= 3.0,
                    "t {t}: loaded {:.1} m from the storehouse at {:?}",
                    to_store(f, at),
                    f.point(at)
                );
                assert_eq!(
                    done, last_done,
                    "no piece placed on a loading tick: t {t} count {last_count} -> {count}"
                );
                if away {
                    loads += 1;
                }
                away = false;
            }
            if done > last_done {
                // Every piece placed came out of the load, one for one.
                assert_eq!(
                    last_count - count,
                    (done - last_done) as u32,
                    "places only what it carried"
                );
                away = true;
            }
            (last_done, last_count) = (done, count);
        }
        assert!(done_at.is_some(), "the hut was finished");
        assert_eq!(most, cap, "full loads of {cap}");
        assert!(
            loads + 1 >= total.div_ceil(cap as usize),
            "{loads} trips back for {total} cells"
        );
    }
);

both_faces!(
    fn the_carried_stack_sent_matches_the_builder_s_load(f: FaceFrame) {
        let mut fx = Fixture::new(config(f, None));
        ground(&mut fx, f);
        fx.join(1, &Client::new(1));
        fx.send(1, Message::CreaturesHello);
        fx.send(1, caps());
        stand(&mut fx, f, 1, 8.5, 18.5);
        fx.server.set_blueprint_rules(fast());
        let (id, _) = storehouse_and_hut(&mut fx, f);
        // Until it carries something.
        let mut b = None;
        for _ in 0..60 * 60 {
            step(&mut fx, 1);
            if let Some(&c) = fx.server.builders_of(id).first()
                && fx.server.builder_load(c).is_some_and(|l| !l.is_empty())
            {
                b = Some(c);
                break;
            }
        }
        let b = b.expect("a builder with a load");
        // Each send carries its load as it was when sent: the creatures go out before the
        // builders work in a tick, so the load at the end of the tick before.
        let names = |l: &[(dwell_blocks::StateId, u32)]| -> Vec<(String, u8)> {
            l.iter()
                .map(|&(s, n)| (dwell_blocks::info(s).name.to_string(), n as u8))
                .collect()
        };
        fx.take_all();
        let mut before = names(&fx.server.builder_load(b).unwrap());
        let mut checked = 0;
        for _ in 0..60 {
            step(&mut fx, 1);
            let sent: Vec<CarryNet> = fx
                .take_all()
                .remove(&1)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|m| match m {
                    Message::Creatures(c) => c.carries,
                    _ => None,
                })
                .flatten()
                .collect();
            let now = names(&fx.server.builder_load(b).unwrap());
            for c in sent {
                assert_eq!(c.id, b, "only the builder carries");
                assert!(c.items == before || c.items == now, "the stack is the load");
                checked += 1;
            }
            before = now;
        }
        assert!(checked >= 5, "its load sent ({checked})");
    }
);

both_faces!(
    fn a_new_town_builds_its_storehouse_first(f: FaceFrame) {
        let rules = dwell_world::blueprint::Rules {
            towns: dwell_world::blueprint::TownRules {
                spacing_m: 256,
                ..SHIPPED.towns
            },
            ..fast()
        };
        let flat: dwell_worldgen::TownGround = std::sync::Arc::new(|_, _, _| Some(m_of_row(BASE)));
        let g = |_: i32, _: i32| Some(m_of_row(BASE));
        let site = rules.towns.sites_near(flat_seed(), f, 0.0, 0.0, 600.0, &g)[0];
        let mut fx = Fixture::new(config(f, None));
        fx.server.set_blueprint_rules(rules);
        fx.server.set_town_ground(Some(flat));
        ground_at(
            &mut fx,
            f,
            site.x - 24..site.x + 24,
            site.z - 24..site.z + 24,
        );
        fx.join(1, &Client::new(1));
        stand(
            &mut fx,
            f,
            1,
            f64::from(site.x) + 20.5,
            f64::from(site.z) + 20.5,
        );
        step(&mut fx, 2 * 60 + 2);
        let list = fx.server.blueprint_list();
        assert_eq!(list[0].name, "dwell:bp_storehouse", "placed first");
        let store = list[0].clone();
        let stores = fx.server.storehouses();
        assert_eq!(stores.len(), 1);
        assert!(!stores[0].3, "not built yet");
        let mut opened = None;
        for t in 0..60 * 120 {
            step(&mut fx, 1);
            let open = fx.server.storehouses()[0].3;
            if !open {
                // Only the storehouse is being built: no other blueprint has a builder or a block.
                for b in &fx.server.blueprint_list()[1..] {
                    assert!(fx.server.builders_of(b.id).is_empty(), "{} waits", b.name);
                    assert_eq!(progress(&mut fx, b).0, 0, "{} untouched", b.name);
                }
            } else {
                opened = Some(t);
                break;
            }
        }
        assert!(opened.is_some(), "the storehouse was built");
        assert_eq!(progress(&mut fx, &store).1, 0);
        // Then the others get builders.
        step(&mut fx, 60);
        let others = fx.server.blueprint_list();
        assert!(
            others
                .iter()
                .any(|b| !fx.server.builders_of(b.id).is_empty()),
            "builders on the town's next blueprints"
        );
    }
);

both_faces!(
    fn a_save_keeps_the_storehouse_and_a_builder_s_load(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let (id, load) = {
            let mut fx = Fixture::new(config(f, Some(&db)));
            ground(&mut fx, f);
            fx.join(1, &Client::new(1));
            stand(&mut fx, f, 1, 8.5, 18.5);
            fx.server.set_blueprint_rules(fast());
            let (id, _) = storehouse_and_hut(&mut fx, f);
            let mut load = None;
            for _ in 0..60 * 60 {
                step(&mut fx, 1);
                if let Some(&c) = fx.server.builders_of(id).first()
                    && let Some(l) = fx.server.builder_load(c).filter(|l| !l.is_empty())
                {
                    load = Some(l);
                    break;
                }
            }
            fx.server.save_now();
            fx.server.flush_store();
            (id, load.expect("a load"))
        };
        let mut fx = Fixture::new(config(f, Some(&db)));
        ground(&mut fx, f);
        fx.server.set_blueprint_rules(fast());
        let stores = fx.server.storehouses();
        assert_eq!(stores.len(), 1, "the storehouse kept");
        assert!(stores[0].3, "built");
        let hut = fx.server.blueprint_list()[0].clone();
        let (before, _) = progress(&mut fx, &hut);
        fx.join(1, &Client::new(1));
        stand(&mut fx, f, 1, 8.5, 18.5);
        let mut b = None;
        for _ in 0..10 {
            step(&mut fx, 1);
            b = b.or(fx.server.builders_of(id).first().copied());
            if b.is_some() {
                break;
            }
        }
        let b = b.expect("its builder back");
        let now = fx.server.builder_load(b).expect("a builder");
        let (after, _) = progress(&mut fx, &hut);
        // Carrying what it carried (less any piece it put in on coming back).
        assert_eq!(now.len(), load.len(), "{now:?} against {load:?}");
        assert_eq!(now[0].0, load[0].0);
        let count = |l: &[(dwell_blocks::StateId, u32)]| l.iter().map(|i| i.1).sum::<u32>();
        assert_eq!(
            count(&now) + (after - before) as u32,
            count(&load),
            "carrying what it carried"
        );
    }
);
