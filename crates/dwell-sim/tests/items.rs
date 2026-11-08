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
