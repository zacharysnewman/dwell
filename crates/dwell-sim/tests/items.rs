//! Inventories and block drops through the server (docs/adr/0066-inventory-and-drops.md): in
//! adventure breaking drops the block's items, which fall and rest on the ground (both faces) and
//! go to a player who walks near; placing uses an item up; a defeat keeps everything; creative is
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
    // Past a held tool's contact (the starting shovel's, 0.26 s: the hit lands then).
    for _ in 0..20 {
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
            .filter(|it| it.stack.item == "dwell:dirt_clump")
            .cloned()
            .collect();
        // Owner, 2026-11-15: a ½ m grass cell breaks into 4 dirt clumps.
        assert_eq!(dirt.len(), 1, "one stack of clumps");
        assert_eq!(dirt[0].stack.count, 4, "grass drops 4 dirt clumps");
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
        assert_eq!(inv.count_of("dwell:dirt_clump"), 4);
        let sent = fx.take(1);
        assert!(sent.iter().any(
            |(m, _)| matches!(m, Message::Inventory(i) if i.inventory.count_of("dwell:dirt_clump") == 4
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
    fn a_defeat_in_adventure_keeps_everything_and_drops_nothing(f: FaceFrame) {
        // Owner, 2026-11-13: "Keep everything" (WORK_QUEUE item 55): before, the hotbar and the
        // inventory dropped where the player knelt when it stood up at the shrine.
        let mut fx = scene(f, "adventure");
        fx.server.give(1, "dwell:dirt", 70);
        fx.server.give(1, "dwell:stick", 3);
        fx.server.give(1, "dwell:gem", 7);
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot: 1 }),
        );
        step(&mut fx, 1);
        let before = fx.server.inventory_of(1).unwrap().clone();
        assert!(
            before.count_of("dwell:dirt") == 70 && before.held == 1,
            "{before:?}"
        );
        let feet = fx.server.feet_of(1).unwrap();
        let shrine = fx.server.respawn_point_after(1, feet);
        fx.server.kill_player(1);
        assert_eq!(fx.server.health_of(1), 0, "defeated");
        let down = dwell_sim::server::defeat_ticks(dwell_protocol::defeat::PLAYER);
        step(&mut fx, down + 60);
        assert!(fx.server.health_of(1) > 0, "stood up");
        let stood = f.point(fx.server.feet_of(1).unwrap());
        let at = f.point(shrine);
        let off = ((stood[0] - at[0]).powi(2) + (stood[2] - at[2]).powi(2)).sqrt();
        assert!(off < 1.0, "at the shrine: {stood:?} {at:?}");
        assert!(
            fx.server.loose_items().is_empty(),
            "nothing drops: {:?}",
            fx.server.loose_items()
        );
        assert_eq!(fx.server.inventory_of(1).unwrap(), &before, "kept exactly");
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
        fx.server.drop_item(
            Stack::new("dwell:rock_shard", 2),
            [4.5, m_of_row(BASE), 4.5],
        );
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut fx = scene_with(f, cfg(), true);
    let inv = fx.server.inventory_of(1).unwrap();
    assert_eq!(inv.count_of("dwell:dirt"), 5);
    assert_eq!(inv.money, 4);
    let loose = fx.server.loose_items();
    assert_eq!(loose.len(), 1);
    assert_eq!(loose[0].stack, Stack::new("dwell:rock_shard", 2));
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
fn an_adventure_player_starts_with_a_wooden_shovel_and_a_creative_one_with_nothing() {
    // Owner (2026-11-14): "Perhaps you could start with a wooden shovel instead of an axe since
    // dirt drops rocks." (2026-11-08 it was an axe.) (face-a-only: no face involved)
    let adv = scene(FaceFrame::A, "adventure");
    let inv = adv.server.inventory_of(1).unwrap();
    assert_eq!(inv.count_of("dwell:wooden_shovel"), 1);
    assert_eq!(inv.count_of("dwell:stone_axe"), 0, "no axe: it is combined");
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
        .filter(|it| it.stack.item == "dwell:rock_shard")
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
        fx.server.give(1, "dwell:rock_shard", 1);
        fx.take_all();
        let near = *rocks
            .iter()
            .find(|c| f.cell(**c)[0] < 4)
            .expect("one on the near side");
        let lay = fx.server.ground_item_at(near).unwrap().point();
        hit(&mut fx, near);
        assert_eq!(
            fx.server
                .inventory_of(1)
                .unwrap()
                .count_of("dwell:rock_shard"),
            1,
            "not straight into the inventory"
        );
        assert_eq!(loose_rocks(&fx), 1, "a loose rock");
        let it = fx
            .server
            .loose_items()
            .iter()
            .find(|it| it.stack.item == "dwell:rock_shard")
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
        assert_eq!(inv.count_of("dwell:rock_shard"), 2, "collected");
        assert_eq!(
            inv.slots
                .iter()
                .flatten()
                .filter(|s| s.item == "dwell:rock_shard")
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
        assert_eq!(inv.count_of("dwell:rock_shard"), 0, "nothing taken");
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
            loose.iter().any(|it| it.stack.item == "dwell:rock_shard"),
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

// --- Combining (owner, 2026-11-11) ---

fn combine(fx: &mut Fixture, name: &str) {
    let recipe = dwell_protocol::inventory::recipe_id(name);
    fx.send(
        1,
        Message::InventoryAction(InventoryAction::Combine { recipe }),
    );
    step(fx, 2);
}

#[test]
fn the_server_combines_pieces_into_slabs_and_stone_tools() {
    // face-a-only: inventories have no face.
    let mut fx = scene(FaceFrame::A, "adventure");
    let count = |fx: &Fixture, item: &str| fx.server.inventory_of(1).unwrap().count_of(item);
    // The server says it combines.
    fx.server.give(1, "dwell:stick", 1);
    step(&mut fx, 2);
    let sent = fx.take(1);
    assert!(sent.iter().any(|(m, _)| matches!(m, Message::Inventory(i)
        if i.flags & inventory_flags::COMBINE != 0)));
    fx.server.give(1, "dwell:stick", 3);
    // 4 rock shards make a stone block (owner, 2026-11-15): refused with 3, made with 4.
    fx.server.give(1, "dwell:rock_shard", 3);
    combine(&mut fx, "dwell:stone_slab");
    assert_eq!(count(&fx, "dwell:stone"), 0, "refused: 3 shards");
    assert_eq!(count(&fx, "dwell:rock_shard"), 3, "nothing taken");
    fx.server.give(1, "dwell:rock_shard", 3);
    combine(&mut fx, "dwell:stone_slab");
    assert_eq!(count(&fx, "dwell:stone"), 1);
    assert_eq!(count(&fx, "dwell:rock_shard"), 2);
    // Dirt clumps and wood chunks.
    fx.server.give(1, "dwell:dirt_clump", 4);
    fx.server.give(1, "dwell:wood", 4);
    combine(&mut fx, "dwell:dirt_slab");
    combine(&mut fx, "dwell:log_slab");
    assert_eq!(
        (count(&fx, "dwell:dirt"), count(&fx, "dwell:dirt_clump")),
        (1, 0)
    );
    assert_eq!((count(&fx, "dwell:log"), count(&fx, "dwell:wood")), (1, 0));
    // Stone tools: 2 sticks, 2 rock shards and 2 grass fibers; refused without the fibers.
    combine(&mut fx, "dwell:stone_pickaxe");
    assert_eq!(
        count(&fx, "dwell:stone_pickaxe"),
        0,
        "refused: no grass fiber"
    );
    fx.server.give(1, "dwell:rock_shard", 2);
    fx.server.give(1, "dwell:grass_fiber", 4);
    combine(&mut fx, "dwell:stone_pickaxe");
    combine(&mut fx, "dwell:stone_axe");
    assert_eq!(count(&fx, "dwell:stone_pickaxe"), 1);
    assert_eq!(
        count(&fx, "dwell:stone_axe"),
        1,
        "the made one (none to start)"
    );
    assert_eq!(count(&fx, "dwell:stick"), 0);
    assert_eq!(count(&fx, "dwell:rock_shard"), 0);
    assert_eq!(count(&fx, "dwell:grass_fiber"), 0);
    // An unknown recipe id changes nothing.
    fx.send(
        1,
        Message::InventoryAction(InventoryAction::Combine { recipe: 7 }),
    );
    step(&mut fx, 2);
    assert_eq!(count(&fx, "dwell:stone_axe"), 1);
}

#[test]
fn the_server_combines_the_shovels_and_sand_clumps_into_sand() {
    // face-a-only: inventories have no face.
    // Owner, 2026-11-14: "Shovel should get a 4 wood sticks recipe 2 grass fiber." and the stone
    // shovel "just make it 2 stone like the others"; "sand should have sub chunks. And let's leave
    // them all at 8 actually."
    let mut fx = scene(FaceFrame::A, "adventure");
    let count = |fx: &Fixture, item: &str| fx.server.inventory_of(1).unwrap().count_of(item);
    fx.server.give(1, "dwell:stick", 3);
    fx.server.give(1, "dwell:grass_fiber", 2);
    combine(&mut fx, "dwell:wooden_shovel");
    assert_eq!(
        count(&fx, "dwell:wooden_shovel"),
        1,
        "refused: 3 sticks (the start's only)"
    );
    fx.server.give(1, "dwell:stick", 1);
    combine(&mut fx, "dwell:wooden_shovel");
    assert_eq!(
        count(&fx, "dwell:wooden_shovel"),
        2,
        "4 sticks and 2 fibres"
    );
    assert_eq!(
        (count(&fx, "dwell:stick"), count(&fx, "dwell:grass_fiber")),
        (0, 0)
    );
    fx.server.give(1, "dwell:stick", 2);
    fx.server.give(1, "dwell:rock_shard", 2);
    fx.server.give(1, "dwell:grass_fiber", 2);
    combine(&mut fx, "dwell:stone_shovel");
    assert_eq!(count(&fx, "dwell:stone_shovel"), 1);
    assert_eq!(
        count(&fx, "dwell:rock_shard"),
        0,
        "2 shards, as the stone axe and pickaxe"
    );
    fx.server.give(1, "dwell:sand_clump", 3);
    combine(&mut fx, "dwell:sand_slab");
    assert_eq!(count(&fx, "dwell:sand"), 0, "refused: 3 clumps");
    fx.server.give(1, "dwell:sand_clump", 1);
    combine(&mut fx, "dwell:sand_slab");
    assert_eq!(
        (count(&fx, "dwell:sand"), count(&fx, "dwell:sand_clump")),
        (1, 0)
    );
}

dwell_world::both_faces!(
    /// Sand breaks into 4 sand clumps, as dirt into dirt clumps (owner, 2026-11-14; 4 since
    /// 2026-11-15).
    fn breaking_sand_in_adventure_drops_4_sand_clumps(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        set_local(&mut fx, f, [2, BASE - 1, 0], "dwell:sand");
        edit(&mut fx, f, BlockEditAction::Break, [2, BASE - 1, 0], 0);
        assert_eq!(loose_count(&fx, "dwell:sand_clump"), 4);
    }
);

// --- What rests on a destroyed block drops with it (WORK_QUEUE item 58, BUGS.md #178) ---
// Owner, 2026-11-14: "When you destroy a block with grass, rock, sticks on it, those should drop
// too." and "Essentially a general rule of blocks or items sitting on a block should be dropped
// when that block is destroyed." One rule however the block goes (a break, a region's clearing, a
// blast, a fall): a block that needs the cell under it (`needsSupport`, `placeOn`) breaks and drops
// its drops, up the column, and a ground item on its top is knocked loose.

fn set_local(fx: &mut Fixture, f: FaceFrame, local: [i32; 3], state: &str) {
    let [x, y, z] = f.cell(local);
    let s = f.state(dwell_blocks::parse_state(state).unwrap_or_else(|e| panic!("{state}: {e}")));
    fx.server.world().set_voxel(x, y, z, s);
}

fn loose_count(fx: &Fixture, item: &str) -> u32 {
    fx.server
        .loose_items()
        .iter()
        .filter(|it| it.stack.item == item)
        .map(|it| u32::from(it.stack.count))
        .sum()
}

fn local_state(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) -> u16 {
    let [x, y, z] = f.cell(local);
    fx.server.world().get_voxel(x, y, z)
}

/// Clears one cell (face-local) with a region fill of air, as a drag's or a tool's region does.
fn clear_region(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) {
    fx.send(
        1,
        Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Fill,
            min: f.cell(local),
            size: [1, 1, 1],
            material: 0,
            from: 0,
            cells: Vec::new(),
        }),
    );
    step(fx, 3);
}

/// `test:moss` (shared/blocks/fixtures), registered from its data as a server mod would: a block
/// that stands on the cell under it, and on itself, added by data alone.
fn moss() -> u16 {
    static ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *ID.get_or_init(|| {
        dwell_blocks::state("test:moss").unwrap_or_else(|| {
            let mut row = *dwell_blocks::fixture::STATES
                .iter()
                .find(|s| s.name == "test:moss")
                .expect("the fixture has test:moss");
            row.shape_index = dwell_blocks::info(require("dwell:tall_grass")).shape_index;
            dwell_blocks::register(
                dwell_blocks::Origin::Server,
                dwell_blocks::NewBlock {
                    name: "test:moss".into(),
                    properties: Vec::new(),
                    states: vec![row],
                },
            )
            .expect("registers")
        })
    })
}

dwell_world::both_faces!(
    /// Breaking a block drops what stands on it: tall grass (its fibre), a torch (itself, both its
    /// cells gone) and a sapling, each where it stood; nothing is left standing on air.
    fn the_blocks_standing_on_a_broken_block_drop_with_it(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        set_local(&mut fx, f, [2, BASE, 0], "dwell:tall_grass[stage=3]");
        set_local(&mut fx, f, [-2, BASE - 1, 2], "dwell:grass");
        set_local(&mut fx, f, [-2, BASE, 2], "dwell:oak_sapling");
        set_local(&mut fx, f, [0, BASE, -3], "dwell:torch");
        set_local(&mut fx, f, [0, BASE + 1, -3], "dwell:torch_upper");
        edit(&mut fx, f, BlockEditAction::Break, [2, BASE - 1, 0], 0);
        edit(&mut fx, f, BlockEditAction::Break, [-2, BASE - 1, 2], 0);
        edit(&mut fx, f, BlockEditAction::Break, [0, BASE - 1, -3], 0);
        let air = f.state(0);
        for c in [
            [2, BASE, 0],
            [-2, BASE, 2],
            [0, BASE, -3],
            [0, BASE + 1, -3],
        ] {
            assert_eq!(local_state(&mut fx, f, c), air, "{c:?} cleared");
        }
        assert!(
            loose_count(&fx, "dwell:grass_fiber") >= 1,
            "the tuft's fibre"
        );
        assert_eq!(loose_count(&fx, "dwell:oak_sapling"), 1, "the sapling");
        assert_eq!(loose_count(&fx, "dwell:torch"), 1, "the torch, once");
    }
);

dwell_world::both_faces!(
    /// A stand of tall grass up to 2 m (WORK_QUEUE item 58 g): cutting any cell cuts the cells over
    /// it, each dropping its fibre ("Cutting grass that's taller than 1 gives +1 plant fiber each"),
    /// and the seed only from the tip; what is under the cut stands.
    fn cutting_a_stand_takes_the_cells_over_it_and_drops_a_fibre_a_cell(f: FaceFrame) {
        for (cut, gone) in [(0, 4), (1, 3), (2, 2)] {
            let mut fx = scene(f, "adventure");
            set_local(&mut fx, f, [2, BASE - 1, 0], "dwell:grass");
            for k in 0..3 {
                set_local(
                    &mut fx,
                    f,
                    [2, BASE + k, 0],
                    "dwell:tall_grass[stage=stalk]",
                );
            }
            set_local(&mut fx, f, [2, BASE + 3, 0], "dwell:tall_grass[stage=3]");
            edit(&mut fx, f, BlockEditAction::Break, [2, BASE + cut, 0], 0);
            let tall = dwell_blocks::require("dwell:tall_grass");
            for k in 0..4 {
                let s = local_state(&mut fx, f, [2, BASE + k, 0]);
                let stands = s != f.state(0)
                    && dwell_blocks::block_of(f.state(s)).name == "dwell:tall_grass";
                assert_eq!(
                    stands,
                    k < cut,
                    "cut at {cut}: cell {k} ({s}, tall grass {tall})"
                );
            }
            assert_eq!(
                loose_count(&fx, "dwell:grass_fiber"),
                gone,
                "cut at {cut}: a fibre a cell; loose {:?}",
                fx.server.loose_items()
            );
            assert!(
                loose_count(&fx, "dwell:grass_seed") <= 1,
                "a seed from the tip only"
            );
        }
    }
);

dwell_world::both_faces!(
    /// A block added by data alone (`test:moss`: `needsSupport: below`, standing on itself too)
    /// gets the rule, up its column: three of it on a stone drop three when the stone goes, by a
    /// break or by a region's clearing.
    fn a_data_only_block_and_a_column_of_it_drop_when_their_ground_goes(f: FaceFrame) {
        let moss = moss();
        for by_region in [false, true] {
            let mut fx = scene(f, "adventure");
            for r in 0..3 {
                let [x, y, z] = f.cell([1, BASE + r, 2]);
                fx.server.world().set_voxel(x, y, z, f.state(moss));
            }
            if by_region {
                clear_region(&mut fx, f, [1, BASE - 1, 2]);
            } else {
                edit(&mut fx, f, BlockEditAction::Break, [1, BASE - 1, 2], 0);
            }
            for r in 0..3 {
                assert_eq!(
                    local_state(&mut fx, f, [1, BASE + r, 2]),
                    f.state(0),
                    "region {by_region}: moss {r} cleared"
                );
            }
            assert_eq!(
                loose_count(&fx, "test:moss"),
                3,
                "region {by_region}: the column dropped cell by cell; ground {}, loose {:?}",
                local_state(&mut fx, f, [1, BASE - 1, 2]),
                fx.server.loose_items()
            );
        }
    }
);

dwell_world::both_faces!(
    /// A region's clearing drops what stood on what it cleared: tall grass's fibre, and the
    /// ground rock lying on it knocked loose (red before: the tuft vanished, the rock floated).
    fn a_regions_clearing_drops_what_stood_on_it(f: FaceFrame) {
        let mut fx = ground_scene(f, "adventure");
        let rocks = wall_and_rocks(&mut fx, f);
        let rock = f.cell(rocks[0]);
        set_local(&mut fx, f, [-2, BASE, -2], "dwell:tall_grass[stage=3]");
        set_local(&mut fx, f, [-2, BASE - 1, -2], "dwell:grass");
        clear_region(&mut fx, f, [-2, BASE - 1, -2]);
        assert_eq!(local_state(&mut fx, f, [-2, BASE, -2]), f.state(0));
        assert!(
            loose_count(&fx, "dwell:grass_fiber") >= 1,
            "the tuft's fibre"
        );
        // Sand under it: its own drop is no rock shard (grass's sometimes is), so the count is
        // the knocked-loose rock's alone.
        let under = [rock[0], rock[1] - 1, rock[2]];
        set_local(&mut fx, f, under, "dwell:sand");
        let before = loose_count(&fx, "dwell:rock_shard");
        clear_region(&mut fx, f, under);
        assert!(
            fx.server.ground_item_at(rocks[0]).is_none(),
            "not left floating"
        );
        assert_eq!(
            loose_count(&fx, "dwell:rock_shard"),
            before + 1,
            "knocked loose"
        );
    }
);

dwell_world::both_faces!(
    /// A blast that takes the ground under a ground rock knocks it loose (red before: it floated
    /// over the crater).
    fn a_blast_under_a_ground_rock_knocks_it_loose(f: FaceFrame) {
        use dwell_world::explosives::{EXPLOSIVES, lit_state};
        let mut fx = ground_scene(f, "adventure");
        let rocks = wall_and_rocks(&mut fx, f);
        let rock = f.cell(rocks[0]);
        let (i, e) = EXPLOSIVES
            .iter()
            .enumerate()
            .find(|(_, e)| e.name == "dwell:dynamite")
            .expect("dynamite");
        let under = f.cell([rock[0], rock[1] - 1, rock[2]]);
        fx.server.world().set_voxel(
            under[0],
            under[1],
            under[2],
            f.state(lit_state(e).expect("lit")),
        );
        fx.server.light_fuse(under, i, 0.0);
        step(&mut fx, 5);
        assert!(
            fx.server.ground_item_at(rocks[0]).is_none(),
            "not left floating"
        );
        assert!(loose_count(&fx, "dwell:rock_shard") >= 1, "knocked loose");
    }
);

#[test]
fn the_town_blocks_combine_from_wood_shards_sticks_and_fibre() {
    // face-a-only: inventories have no face.
    // Owner, 2026-11-14: "Can we make planks assembled from 8 wood pieces from trees." / "8 stone
    // chunks for a stone bricks. Thatch roof comes from 6 sticks, 2 plant fibre"; 2026-11-17: "Let's
    // make the planks and stone brick 4 for 4" and thatch "2 sticks 2 grass fiber, 4 blocks".
    let mut fx = scene(FaceFrame::A, "adventure");
    let count = |fx: &Fixture, item: &str| fx.server.inventory_of(1).unwrap().count_of(item);
    fx.server.give(1, "dwell:wood", 7);
    combine(&mut fx, "dwell:planks_slab");
    combine(&mut fx, "dwell:planks_slab");
    assert_eq!(
        (count(&fx, "dwell:planks"), count(&fx, "dwell:wood")),
        (4, 3),
        "4 wood 4 planks, the 3 left refused"
    );
    fx.server.give(1, "dwell:rock_shard", 4);
    combine(&mut fx, "dwell:stone_brick_slab");
    assert_eq!(
        (
            count(&fx, "dwell:stone_brick"),
            count(&fx, "dwell:rock_shard")
        ),
        (4, 0)
    );
    fx.server.give(1, "dwell:stick", 2);
    fx.server.give(1, "dwell:grass_fiber", 1);
    combine(&mut fx, "dwell:thatch_slab");
    assert_eq!(count(&fx, "dwell:thatch"), 0, "refused: one fibre");
    fx.server.give(1, "dwell:grass_fiber", 1);
    combine(&mut fx, "dwell:thatch_slab");
    assert_eq!(
        (
            count(&fx, "dwell:thatch"),
            count(&fx, "dwell:stick"),
            count(&fx, "dwell:grass_fiber")
        ),
        (4, 0, 0)
    );
}

#[test]
fn a_cannon_and_a_ships_wheel_are_crafted() {
    // face-a-only: inventory counts, no geometry. Owner, 2026-11-18: a cannon "16 rock shards + 4
    // wood"; a ship's wheel "6 wood + 4 sticks".
    let mut fx = scene(FaceFrame::A, "adventure");
    let count = |fx: &Fixture, item: &str| fx.server.inventory_of(1).unwrap().count_of(item);
    fx.server.give(1, "dwell:rock_shard", 15);
    fx.server.give(1, "dwell:wood", 10);
    combine(&mut fx, "dwell:cannon");
    assert_eq!(count(&fx, "dwell:cannon"), 0, "refused: 15 shards");
    fx.server.give(1, "dwell:rock_shard", 1);
    combine(&mut fx, "dwell:cannon");
    assert_eq!(
        (
            count(&fx, "dwell:cannon"),
            count(&fx, "dwell:rock_shard"),
            count(&fx, "dwell:wood")
        ),
        (1, 0, 6)
    );
    fx.server.give(1, "dwell:stick", 4);
    combine(&mut fx, "dwell:helm");
    assert_eq!(
        (
            count(&fx, "dwell:helm"),
            count(&fx, "dwell:wood"),
            count(&fx, "dwell:stick")
        ),
        (1, 0, 0)
    );
}
