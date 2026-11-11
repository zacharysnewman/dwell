//! Inventories and block drops through the server (docs/adr/0066-inventory-and-drops.md): in
//! adventure breaking drops the block's items, which fall and rest on the ground (both faces) and
//! go to a player who walks near; placing uses an item up; death drops everything; creative is
//! unlimited and drops nothing; inventories and loose items are saved with the world.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::inventory::{SLOTS, Stack};
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row): stone below it, grass on top at x = 2.
const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    }
}

/// A server in `mode` on a stone ground in face `f`'s frame, a grass block at (2, BASE − 1, 0),
/// one player joined (whose client said `Hello` when `hello`).
fn scene_with(f: FaceFrame, config: ServerConfig, hello: bool) -> Fixture {
    let mut fx = Fixture::new(config);
    let stone = f.state(require("dwell:stone"));
    for x in -6..=6 {
        for z in -6..=6 {
            for r in -4..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    let [x, y, z] = f.cell([2, BASE - 1, 0]);
    fx.server
        .world()
        .set_voxel(x, y, z, f.state(require("dwell:grass")));
    let welcome = fx.join(1, &Client::new(1));
    assert_ne!(welcome.flags & welcome_flags::INVENTORY, 0, "announced");
    if hello {
        fx.send(1, Message::InventoryAction(InventoryAction::Hello));
    }
    // Spawns spread players out: this one stands at (0.5, 0.5).
    fx.move_to_local(f, 0.5, m_of_row(BASE), 0.5);
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

fn scene(f: FaceFrame, mode: &str) -> Fixture {
    scene_with(f, config(f, mode), true)
}

fn edit(fx: &mut Fixture, f: FaceFrame, action: BlockEditAction, local: [i32; 3], material: u16) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action,
            cell: f.cell(local),
            face: f.cell_face(2) as u8,
            material,
            rows: 1,
            down: false,
            point: None,
        }),
    );
    for _ in 0..3 {
        fx.server.step();
    }
}

fn step(fx: &mut Fixture, ticks: u32) {
    for _ in 0..ticks {
        fx.server.step();
    }
}

dwell_world::both_faces!(
    fn breaking_grass_in_adventure_drops_dirt_that_rests_and_is_picked_up(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        edit(&mut fx, f, BlockEditAction::Break, [2, BASE - 1, 0], 0);
        let dirt: Vec<_> = fx
            .server
            .loose_items()
            .iter()
            .filter(|it| it.stack.item == "dwell:dirt")
            .cloned()
            .collect();
        assert_eq!(dirt.len(), 1, "grass drops one dirt");
        let id = dirt[0].id;
        // It falls out of the broken cell onto the stone below and rests on its top.
        step(&mut fx, 60);
        let it = fx
            .server
            .loose_items()
            .iter()
            .find(|it| it.id == id)
            .expect("still there: the player is 2 m away")
            .clone();
        assert!(it.resting);
        let local = f.point(it.position);
        assert!(
            (local[1] - m_of_row(BASE - 1)).abs() < 1e-9,
            "on the stone's top: {local:?}"
        );
        // The client was told about it.
        let sent = fx.take(1);
        assert!(sent.iter().any(
            |(m, _)| matches!(m, Message::ItemDrops(d) if d.items.iter().any(|i| i.id == id))
        ));
        // Walking near takes it.
        fx.move_to_local(f, 2.5, m_of_row(BASE - 1), 0.5);
        step(&mut fx, 5);
        assert!(fx.server.loose_items().iter().all(|it| it.id != id));
        let inv = fx.server.inventory_of(1).unwrap();
        assert_eq!(inv.count_of("dwell:dirt"), 1);
        let sent = fx.take(1);
        assert!(sent.iter().any(
            |(m, _)| matches!(m, Message::Inventory(i) if i.inventory.count_of("dwell:dirt") == 1
                && i.flags & inventory_flags::USES_ITEMS != 0)
        ));
        assert!(
            sent.iter()
                .any(|(m, _)| matches!(m, Message::ItemDrops(d) if d.gone.contains(&id)))
        );
    }
);

dwell_world::both_faces!(
    fn placing_uses_an_item_in_adventure_and_is_refused_without_one(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        let stone = f.state(require("dwell:stone"));
        let at = [-2, BASE, 0];
        let on = [-2, BASE - 1, 0];
        edit(&mut fx, f, BlockEditAction::Place, on, stone);
        let [x, y, z] = f.cell(at);
        assert_eq!(fx.server.world().get_voxel(x, y, z), 0, "refused: no stone");
        assert_eq!(fx.server.give(1, "dwell:stone", 2), 0);
        edit(&mut fx, f, BlockEditAction::Place, on, stone);
        assert_eq!(fx.server.world().get_voxel(x, y, z), stone);
        assert_eq!(
            fx.server.inventory_of(1).unwrap().count_of("dwell:stone"),
            1
        );
    }
);

#[test]
fn creative_places_without_items_and_breaking_drops_nothing() {
    // face-a-only: the mode's gate is read before anything face-dependent.
    let f = FaceFrame::A;
    let mut fx = scene(f, "creative");
    let stone = require("dwell:stone");
    edit(&mut fx, f, BlockEditAction::Place, [-2, BASE - 1, 0], stone);
    let check = fx.server.stats_of(1).unwrap().last_edit_check;
    assert_eq!(fx.server.world().get_voxel(-2, BASE, 0), stone, "{check:?}");
    edit(&mut fx, f, BlockEditAction::Break, [2, BASE - 1, 0], 0);
    assert_eq!(fx.server.world().get_voxel(2, BASE - 1, 0), 0);
    assert!(fx.server.loose_items().is_empty(), "nothing drops");
    assert!(fx.server.inventory_of(1).unwrap().is_empty());
}

#[test]
fn a_full_inventory_leaves_the_item_on_the_ground_and_currency_still_goes_to_the_wallet() {
    // face-a-only: pickup reads distances, the same on both faces.
    let f = FaceFrame::A;
    let mut fx = scene(f, "adventure");
    for _ in 0..SLOTS {
        fx.server.give(1, "dwell:sand", 64);
    }
    assert_eq!(fx.server.give(1, "dwell:sand", 1), 1, "full");
    let feet = fx.server.feet_of(1).unwrap();
    let dirt = fx.server.drop_item(Stack::new("dwell:dirt", 3), feet);
    fx.server.drop_item(Stack::new("dwell:gem_large", 2), feet);
    step(&mut fx, 60);
    let left: Vec<_> = fx.server.loose_items().iter().map(|it| it.id).collect();
    assert_eq!(left, [dirt], "the dirt stays; the gems were taken");
    assert_eq!(fx.server.inventory_of(1).unwrap().money, 10);
}

dwell_world::both_faces!(
    fn dying_in_adventure_drops_everything_where_the_player_died(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        fx.server.give(1, "dwell:dirt", 70);
        fx.server.give(1, "dwell:stick", 3);
        fx.server.give(1, "dwell:gem", 7);
        let feet = fx.server.feet_of(1).unwrap();
        fx.server.kill_player(1);
        let loose = fx.server.loose_items().to_vec();
        // The starting axe, two stacks of dirt and the sticks (the gems stay in the wallet).
        assert_eq!(loose.len(), 4, "a stack a slot: {loose:?}");
        let total: u32 = loose.iter().map(|it| u32::from(it.stack.count)).sum();
        assert_eq!(total, 74);
        step(&mut fx, 60);
        for it in fx.server.loose_items() {
            let (a, b) = (f.point(it.position), f.point(feet));
            assert!(it.resting);
            assert!((a[1] - b[1]).abs() < 1e-9, "on the ground: {a:?} {b:?}");
            assert!(((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() < 0.5);
        }
        let inv = fx.server.inventory_of(1).unwrap();
        assert_eq!(inv.count_of("dwell:dirt") + inv.count_of("dwell:stick"), 0);
        assert_eq!(inv.money, 7, "the wallet is kept");
    }
);

#[test]
fn inventories_and_loose_items_are_saved_with_the_world() {
    // face-a-only: a byte format.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "adventure")
    };
    {
        let mut fx = scene_with(f, cfg(), true);
        fx.server.give(1, "dwell:dirt", 5);
        fx.server.give(1, "dwell:gem", 4);
        fx.server
            .drop_item(Stack::new("dwell:rock", 2), [4.5, m_of_row(BASE), 4.5]);
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut fx = scene_with(f, cfg(), true);
    let inv = fx.server.inventory_of(1).unwrap();
    assert_eq!(inv.count_of("dwell:dirt"), 5);
    assert_eq!(inv.money, 4);
    let loose = fx.server.loose_items();
    assert_eq!(loose.len(), 1);
    assert_eq!(loose[0].stack, Stack::new("dwell:rock", 2));
    step(&mut fx, 1);
}

#[test]
fn a_client_that_never_says_hello_is_sent_no_inventory_messages() {
    // face-a-only: a wire rule.
    let f = FaceFrame::A;
    let mut fx = scene_with(f, config(f, "adventure"), false);
    edit(&mut fx, f, BlockEditAction::Break, [2, BASE - 1, 0], 0);
    fx.server.give(1, "dwell:dirt", 1);
    step(&mut fx, 30);
    assert!(!fx.server.loose_items().is_empty());
    assert!(
        fx.take(1)
            .iter()
            .all(|(m, _)| !matches!(m, Message::Inventory(_) | Message::ItemDrops(_)))
    );
}

trait MoveLocal {
    fn move_to_local(&mut self, f: FaceFrame, x: f64, feet: f64, z: f64);
}

impl MoveLocal for Fixture {
    /// Puts player 1's feet at a face-local point (its capsule toward face `f`'s up).
    fn move_to_local(&mut self, f: FaceFrame, x: f64, feet: f64, z: f64) {
        let h = self.server.player_handle_of(1).expect("a live player");
        let (_, physics, players) = self.server.sim();
        let half = f64::from(players.half_height(h));
        let centre = f.point([x, feet + half, z]);
        players.teleport(
            physics,
            h,
            dwell_physics::DVec3::from(centre),
            dwell_physics::Vec3::ZERO,
        );
    }
}

#[test]
fn an_adventure_player_starts_with_an_axe_and_a_creative_one_with_nothing() {
    // Owner (2026-11-08): "Adventure starts with an axe." (face-a-only: no face involved)
    let adv = scene(FaceFrame::A, "adventure");
    assert_eq!(
        adv.server
            .inventory_of(1)
            .unwrap()
            .count_of("dwell:stone_axe"),
        1
    );
    let cre = scene(FaceFrame::A, "creative");
    assert!(cre.server.inventory_of(1).unwrap().is_empty());
}

// --- Ground items (docs/adr/0071-ground-items.md) ---

/// The ground items' scene: [`config`]'s with its ground and a stone wall 6 rows tall across
/// x = 4 generated (the scatter's rules read the terrain as generated, so a built wall grows no
/// rocks): the wall's feet are a cliff base.
fn ground_config(f: FaceFrame, mode: &str) -> ServerConfig {
    let stone = f.state(require("dwell:stone"));
    ServerConfig {
        generator_override: Some(std::sync::Arc::new(
            move |c: dwell_world::ChunkCoord, chunk: &mut dwell_world::Chunk| {
                let n = dwell_world::CHUNK_SIZE;
                for ly in 0..n {
                    for lz in 0..n {
                        for lx in 0..n {
                            let [x, y, z] = f.cell([c.x * n + lx, c.y * n + ly, c.z * n + lz]);
                            let inside = (-6..=6).contains(&x) && (-6..=6).contains(&z);
                            let ground = (BASE - 4..BASE).contains(&y);
                            let wall = x == 4 && (BASE..BASE + 6).contains(&y);
                            if inside && (ground || wall) {
                                chunk.set(lx, ly, lz, stone);
                            }
                        }
                    }
                }
            },
        )),
        ..config(f, mode)
    }
}

fn ground_scene(f: FaceFrame, mode: &str) -> Fixture {
    scene_with(f, ground_config(f, mode), true)
}

/// The cells (world) beside the generated wall where the shipped scatter put a rock.
fn wall_and_rocks(fx: &mut Fixture, f: FaceFrame) -> Vec<[i32; 3]> {
    let mut found = Vec::new();
    for x in [2, 3, 5, 6] {
        for z in -6..=6 {
            let cell = f.cell([x, BASE, z]);
            if let Some(p) = fx.server.ground_item_at(cell) {
                assert_eq!(p.cell, cell);
                found.push(cell);
            }
        }
    }
    assert!(
        !found.is_empty(),
        "the scatter put rocks at the wall's feet"
    );
    found
}

/// The break action aimed at the ground item in `cell` (ADR 0071 amendment, 2026-11-09): the
/// client sends it as `InventoryAction::Pick`.
fn hit(fx: &mut Fixture, cell: [i32; 3]) {
    fx.send(1, Message::InventoryAction(InventoryAction::Pick { cell }));
    step(fx, 2);
}

fn loose_rocks(fx: &Fixture) -> usize {
    fx.server
        .loose_items()
        .iter()
        .filter(|it| it.stack.item == "dwell:rock")
        .count()
}

dwell_world::both_faces!(
    /// Hitting a rock within reach knocks it loose (owner, 2026-11-09: "if you hit the rock or
    /// stick they drop as the spinning item that auto picks up"): a loose rock where it lay, not
    /// straight into the inventory, and gone from the scatter; walking over it collects it,
    /// stacking with a dropped rock. A second hit of it, one out of reach, or one at nothing is
    /// refused.
    fn hitting_a_ground_rock_knocks_it_loose_and_walking_over_collects_it(f: FaceFrame) {
        let mut fx = ground_scene(f, "adventure");
        fx.send(1, Message::InventoryAction(InventoryAction::GroundHello));
        step(&mut fx, 1);
        let rocks = wall_and_rocks(&mut fx, f);
        fx.server.give(1, "dwell:rock", 1);
        fx.take_all();
        let near = *rocks
            .iter()
            .find(|c| f.cell(**c)[0] < 4)
            .expect("one on the near side");
        let lay = fx.server.ground_item_at(near).unwrap().point();
        hit(&mut fx, near);
        assert_eq!(
            fx.server.inventory_of(1).unwrap().count_of("dwell:rock"),
            1,
            "not straight into the inventory"
        );
        assert_eq!(loose_rocks(&fx), 1, "a loose rock");
        let it = fx
            .server
            .loose_items()
            .iter()
            .find(|it| it.stack.item == "dwell:rock")
            .unwrap();
        let d = (0..3)
            .map(|k| (it.position[k] - lay[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(d < 1.0, "where it lay: {d} m off");
        assert!(fx.server.ground_item_at(near).is_none());
        assert!(fx.server.ground_gone().cells.contains(&near));
        assert!(
            fx.take(1)
                .iter()
                .any(|(m, _)| matches!(m, Message::GroundGone(c) if c == &vec![near])),
            "the client is told"
        );
        hit(&mut fx, near);
        assert_eq!(loose_rocks(&fx), 1, "a second hit knocks nothing");
        // Walking over it collects it.
        let l = f.cell(near);
        fx.move_to_local(
            f,
            f64::from(l[0]) + 0.5,
            m_of_row(BASE),
            f64::from(l[2]) + 0.5,
        );
        step(&mut fx, 60);
        let inv = fx.server.inventory_of(1).unwrap();
        assert_eq!(inv.count_of("dwell:rock"), 2, "collected");
        assert_eq!(
            inv.slots
                .iter()
                .flatten()
                .filter(|s| s.item == "dwell:rock")
                .count(),
            1,
            "one stack with the dropped rock"
        );
        // Out of reach: the player 40 m off.
        let other = rocks.iter().copied().find(|c| *c != near);
        fx.move_to_local(f, -40.5, m_of_row(BASE + 40), 0.5);
        if let Some(other) = other {
            hit(&mut fx, other);
            assert_eq!(loose_rocks(&fx), 0);
            assert!(fx.server.ground_item_at(other).is_some());
        }
        // Nothing lies here: refused.
        hit(&mut fx, f.cell([0, BASE, 0]));
        assert_eq!(loose_rocks(&fx), 0);
    }
);

dwell_world::both_faces!(
    /// In creative, where breaking drops nothing (ADR 0066), a ground rock hit simply vanishes:
    /// gone from the scatter, no loose rock, nothing in the inventory.
    fn hitting_a_ground_rock_in_creative_makes_it_vanish(f: FaceFrame) {
        let mut fx = ground_scene(f, "creative");
        let rocks = wall_and_rocks(&mut fx, f);
        let near = *rocks
            .iter()
            .find(|c| f.cell(**c)[0] < 4)
            .expect("one on the near side");
        hit(&mut fx, near);
        assert!(fx.server.ground_gone().cells.contains(&near));
        assert!(fx.server.ground_item_at(near).is_none());
        assert_eq!(loose_rocks(&fx), 0, "nothing dropped");
        step(&mut fx, 60);
        let inv = fx.server.inventory_of(1).unwrap();
        assert_eq!(inv.count_of("dwell:rock"), 0, "nothing taken");
    }
);

dwell_world::both_faces!(
    /// Breaking the block under a ground rock knocks it loose: a loose rock where it lay
    /// (adventure), and it does not come back when the cell is filled again.
    fn breaking_the_ground_under_a_rock_knocks_it_loose(f: FaceFrame) {
        let mut fx = ground_scene(f, "adventure");
        let rocks = wall_and_rocks(&mut fx, f);
        let cell = *rocks
            .iter()
            .find(|c| f.cell(**c)[0] == 3)
            .or(rocks.first())
            .unwrap();
        let l = f.cell(cell);
        let before = fx.server.loose_items().len();
        edit(
            &mut fx,
            f,
            BlockEditAction::Break,
            [l[0], l[1] - 1, l[2]],
            0,
        );
        let loose = fx.server.loose_items();
        assert!(
            loose.iter().any(|it| it.stack.item == "dwell:rock"),
            "{loose:?}"
        );
        assert!(loose.len() > before);
        let stone = f.state(require("dwell:stone"));
        let [x, y, z] = f.cell([l[0], l[1] - 1, l[2]]);
        fx.server.world().set_voxel(x, y, z, stone);
        assert!(fx.server.ground_item_at(cell).is_none(), "stays gone");
    }
);

#[test]
fn hit_ground_items_stay_gone_across_a_reload() {
    // face-a-only: a byte format (both faces' hits are pinned above).
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..ground_config(f, "creative")
    };
    let cell;
    {
        let mut fx = scene_with(f, cfg(), true);
        let rocks = wall_and_rocks(&mut fx, f);
        cell = *rocks.iter().find(|c| c[0] < 4).expect("near side");
        hit(&mut fx, cell);
        assert!(
            fx.server.ground_gone().cells.contains(&cell),
            "creative's hit takes it too"
        );
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut fx = scene_with(f, cfg(), true);
    wall_and_rocks(&mut fx, f);
    assert!(fx.server.ground_gone().cells.contains(&cell));
    assert!(fx.server.ground_item_at(cell).is_none());
}

#[test]
fn ground_messages_go_only_to_a_client_that_said_ground_hello() {
    // face-a-only: a wire rule.
    let f = FaceFrame::A;
    let mut fx = ground_scene(f, "adventure");
    fx.server.give(1, "dwell:dirt", 1);
    step(&mut fx, 2);
    let got = fx.take(1);
    assert!(got.iter().any(|(m, _)| matches!(m, Message::Inventory(i)
        if i.flags & inventory_flags::GROUND != 0)));
    let rocks = wall_and_rocks(&mut fx, f);
    let cell = *rocks.iter().find(|c| c[0] < 4).expect("near side");
    hit(&mut fx, cell);
    assert!(
        fx.take(1)
            .iter()
            .all(|(m, _)| !matches!(m, Message::GroundGone(_)))
    );
    fx.send(1, Message::InventoryAction(InventoryAction::GroundHello));
    step(&mut fx, 1);
    assert!(
        fx.take(1)
            .iter()
            .any(|(m, _)| matches!(m, Message::GroundGone(c) if c == &vec![cell]))
    );
}

dwell_world::both_faces!(
    /// The scatter's rules read the terrain as generated (owner, 2026-11-09: "keep sticks under
    /// cleared leaves"): a rock at the generated wall's foot stays when the wall is broken away.
    fn a_ground_rock_stays_when_its_cliff_is_broken_away(f: FaceFrame) {
        let mut fx = ground_scene(f, "creative");
        let rocks = wall_and_rocks(&mut fx, f);
        for z in -6..=6 {
            for r in 0..6 {
                let [x, y, z] = f.cell([4, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, 0);
            }
        }
        for cell in rocks {
            assert!(fx.server.ground_item_at(cell).is_some(), "{cell:?} stays");
        }
    }
);

dwell_world::both_faces!(
    /// The server's ground items follow the biomes' densities (ADR 0074 B5): rocks ×0 where a
    /// biome names no rocks leaves none at the wall's feet, nor one to knock loose; ×1 everywhere
    /// is the scatter as without densities.
    fn the_servers_ground_items_follow_the_biomes_densities(f: FaceFrame) {
        use dwell_world::ground::{GroundDensity, KINDS};
        let mut fx = ground_scene(f, "adventure");
        step(&mut fx, 1);
        let rocks = wall_and_rocks(&mut fx, f);
        let even = |rock: f32| -> dwell_worldgen::GroundDensityFn {
            std::sync::Arc::new(move |_| GroundDensity {
                corners: KINDS
                    .iter()
                    .map(|k| {
                        [if k.name == "dwell:ground_rock" {
                            rock
                        } else {
                            1.0
                        }; 4]
                    })
                    .collect(),
            })
        };
        fx.server.set_ground_density(Some(even(1.0)));
        for &cell in &rocks {
            assert!(fx.server.ground_item_at(cell).is_some(), "{cell:?} at ×1");
        }
        fx.server.set_ground_density(Some(even(0.0)));
        for &cell in &rocks {
            assert_eq!(fx.server.ground_item_at(cell), None, "{cell:?} at ×0");
        }
        fx.server.set_ground_density(None);
        assert!(fx.server.ground_item_at(rocks[0]).is_some());
    }
);
