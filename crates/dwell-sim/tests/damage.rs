//! Block damage through the server (docs/BLOCK_DAMAGE.md, ADR 0078): a block takes hits to break by
//! the tool held, from its `damage` data; a hit short of the count leaves it standing and is
//! remembered; the hits and the destroy drop the block's drops; bare hands can't break stone; the
//! same in creative and adventure; the damage is saved with the world.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row): stone below it, dirt on top at x = 2.
const BASE: i32 = FLAT_TOP_Y + 40;
const STONE: [i32; 3] = [-2, BASE - 1, 0];
const DIRT: [i32; 3] = [2, BASE - 1, 0];

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        one_hit_breaks: false,
        ..flat()
    }
}

fn scene_with(f: FaceFrame, config: ServerConfig) -> Fixture {
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
    let [x, y, z] = f.cell(DIRT);
    fx.server
        .world()
        .set_voxel(x, y, z, f.state(require("dwell:dirt")));
    fx.join(1, &Client::new(1));
    fx.send(1, Message::InventoryAction(InventoryAction::Hello));
    fx.move_to_local(f, 0.5, m_of_row(BASE), 0.5);
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

/// Holds `item`: in adventure given and selected, in creative told as the hotbar's tool.
fn hold(fx: &mut Fixture, mode: &str, item: &str) {
    if mode == "adventure" {
        fx.server.give(1, item, 1);
        let slot = fx
            .server
            .inventory_of(1)
            .unwrap()
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.item == item))
            .expect("given") as u8;
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot }),
        );
    } else {
        fx.send(
            1,
            Message::Chop(ChopAction::Held {
                tool: item.into(),
                vertical: false,
            }),
        );
    }
    for _ in 0..2 {
        fx.server.step();
    }
}

fn hit(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: f.cell(local),
            face: f.cell_face(2) as u8,
            material: 0,
            rows: 1,
            down: false,
            point: None,
        }),
    );
    // Within the edit rate (10 a second): one hit every 7 ticks.
    for _ in 0..7 {
        fx.server.step();
    }
}

fn block_at(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) -> &'static str {
    let [x, y, z] = f.cell(local);
    dwell_blocks::block_of(fx.server.world().get_voxel(x, y, z)).name
}

/// Hits `local` until it breaks (at most 20): the hits it took.
fn hits_to_break(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) -> u32 {
    let was = block_at(fx, f, local);
    for n in 1..=20 {
        hit(fx, f, local);
        if block_at(fx, f, local) != was {
            return n;
        }
    }
    panic!("{was} at {local:?} still stands after 20 hits");
}

fn loose(fx: &Fixture, item: &str) -> u32 {
    fx.server
        .loose_items()
        .iter()
        .filter(|it| it.stack.item == item)
        .map(|it| u32::from(it.stack.count))
        .sum()
}

dwell_world::both_faces!(
    fn hits_to_break_come_from_the_tool_held_in_both_modes(f: FaceFrame) {
        // Owner, 2026-11-10: stone tier about 5, metal about 3, the wrong tool or bare hands
        // about 10–12; the same in creative and adventure.
        for mode in ["adventure", "creative"] {
            let mut fx = scene_with(f, config(f, mode));
            hold(&mut fx, mode, "dwell:stone_pickaxe");
            assert_eq!(hits_to_break(&mut fx, f, STONE), 5, "{mode}: stone pickaxe");
            hold(&mut fx, mode, "dwell:metal_shovel");
            assert_eq!(hits_to_break(&mut fx, f, DIRT), 3, "{mode}: metal shovel");
            hold(&mut fx, mode, "dwell:metal_pickaxe");
            assert_eq!(
                hits_to_break(&mut fx, f, [-3, BASE - 1, 0]),
                3,
                "{mode}: metal pickaxe"
            );
        }
        let mut fx = scene_with(f, config(f, "adventure"));
        empty_hand(&mut fx);
        assert_eq!(hits_to_break(&mut fx, f, DIRT), 11, "adventure: bare hands");
    }
);

/// Adventure: selects an empty inventory slot (a bare hand).
fn empty_hand(fx: &mut Fixture) {
    let slot = fx
        .server
        .inventory_of(1)
        .unwrap()
        .slots
        .iter()
        .position(|s| s.is_none())
        .expect("an empty slot") as u8;
    fx.send(
        1,
        Message::InventoryAction(InventoryAction::Select { slot }),
    );
    for _ in 0..2 {
        fx.server.step();
    }
}

dwell_world::both_faces!(
    fn in_creative_anything_but_a_tool_breaks_any_block_in_one_hit(f: FaceFrame) {
        // Owner, 2026-11-11: "I should be able to break any block while holding anything or
        // nothing with the exception of tools" (creative's `instantBreaks`).
        let mut fx = scene_with(f, config(f, "creative"));
        assert_eq!(hits_to_break(&mut fx, f, STONE), 1, "bare hands: stone");
        assert_eq!(hits_to_break(&mut fx, f, DIRT), 1, "bare hands: dirt");
        // Holding a block (the hotbar tells no tool, or names a non-tool item).
        hold(&mut fx, "creative", "");
        assert_eq!(hits_to_break(&mut fx, f, [-3, BASE - 1, 0]), 1, "a block");
        hold(&mut fx, "creative", "dwell:dirt");
        assert_eq!(
            hits_to_break(&mut fx, f, [-4, BASE - 1, 0]),
            1,
            "dirt named"
        );
        // A tool keeps its hits.
        hold(&mut fx, "creative", "dwell:stone_pickaxe");
        assert_eq!(
            hits_to_break(&mut fx, f, [-5, BASE - 1, 0]),
            5,
            "stone pickaxe"
        );
    }
);

dwell_world::both_faces!(
    fn bare_hands_and_the_wrong_tool_do_nothing_to_stone(f: FaceFrame) {
        // Adventure (creative breaks anything without a tool, above).
        let mut fx = scene_with(f, config(f, "adventure"));
        empty_hand(&mut fx);
        for _ in 0..12 {
            hit(&mut fx, f, STONE);
        }
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:stone", "bare hands");
        // Creative with a tool that lists none for stone (a shovel) can't either.
        let mut fx = scene_with(f, config(f, "creative"));
        hold(&mut fx, "creative", "dwell:metal_shovel");
        for _ in 0..12 {
            hit(&mut fx, f, STONE);
        }
        assert_eq!(
            block_at(&mut fx, f, STONE),
            "dwell:stone",
            "creative: a shovel"
        );
        // Adventure starts holding an axe: the wrong tool for stone, which lists none for it.
        let mut fx = scene_with(f, config(f, "adventure"));
        for _ in 0..12 {
            hit(&mut fx, f, STONE);
        }
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:stone", "an axe");
        // And the hits left no damage: a pickaxe still takes its five.
        hold(&mut fx, "adventure", "dwell:stone_pickaxe");
        assert_eq!(hits_to_break(&mut fx, f, STONE), 5);
    }
);

dwell_world::both_faces!(
    fn the_hits_and_the_destroy_drop_the_blocks_drops(f: FaceFrame) {
        let mut fx = scene_with(f, config(f, "adventure"));
        hold(&mut fx, "adventure", "dwell:stone_pickaxe");
        for _ in 0..4 {
            hit(&mut fx, f, STONE);
        }
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:stone");
        // Owner, 2026-11-11: a ½ m stone cell breaks into 8 rock shards, half (4) over the hits
        // (one each of the four before the last), the rest on the destroy.
        assert_eq!(loose(&fx, "dwell:rock_shard"), 4);
        assert_eq!(loose(&fx, "dwell:stone"), 0, "no longer the block itself");
        hit(&mut fx, f, STONE);
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:air");
        assert_eq!(
            loose(&fx, "dwell:rock_shard"),
            8,
            "the cell's 8 pieces, once"
        );
    }
);

dwell_world::both_faces!(
    fn dirt_breaks_into_eight_clumps_and_may_turn_up_a_rock(f: FaceFrame) {
        // Owner, 2026-11-11: 8 dirt clumps a cell, half over the hits; rocks still found at random.
        let mut rocks = 0;
        for _ in 0..12 {
            let mut fx = scene_with(f, config(f, "adventure"));
            hold(&mut fx, "adventure", "dwell:wooden_shovel");
            let n = hits_to_break(&mut fx, f, DIRT);
            assert_eq!(n, 5, "a stone-tier shovel");
            assert_eq!(loose(&fx, "dwell:dirt_clump"), 8);
            assert_eq!(loose(&fx, "dwell:dirt"), 0);
            rocks += loose(&fx, "dwell:rock");
        }
        assert!(
            rocks > 0,
            "the random rock (chance 0.5) turned up in 12 cells"
        );
    }
);

dwell_world::both_faces!(
    fn each_hit_with_the_stone_axe_wears_it_and_creative_does_not(f: FaceFrame) {
        // Owner, 2026-11-11: "It should survive 32 hits of durability"; a break hit with it counts.
        assert_eq!(chopping::durability("dwell:stone_axe"), Some(32));
        let mut fx = scene_with(f, config(f, "adventure"));
        // Adventure starts holding the axe (slot 0).
        let wear = |fx: &Fixture| {
            fx.server.inventory_of(1).unwrap().slots[0]
                .as_ref()
                .map(|s| (s.item.clone(), s.wear))
        };
        assert_eq!(wear(&fx), Some(("dwell:stone_axe".into(), 0)));
        fx.take_all();
        let n = hits_to_break(&mut fx, f, DIRT);
        assert_eq!(n, 11, "the axe is no shovel");
        assert_eq!(
            wear(&fx),
            Some(("dwell:stone_axe".into(), 11)),
            "one per hit"
        );
        // A refused hit (stone with an axe) doesn't count.
        hit(&mut fx, f, STONE);
        assert_eq!(wear(&fx), Some(("dwell:stone_axe".into(), 11)));
        // The client is sent the wear.
        let sent = fx.take(1);
        assert!(sent.iter().any(|(m, _)| matches!(m, Message::Inventory(i)
            if i.inventory.slots[0].as_ref().is_some_and(|s| s.wear == 11))));
        // Creative (`creativeWear` false): the hotbar's tool doesn't wear.
        const { assert!(!chopping::CREATIVE_WEAR) };
        let mut fx = scene_with(f, config(f, "creative"));
        hold(&mut fx, "creative", "dwell:stone_axe");
        fx.server.give(1, "dwell:stone_axe", 1);
        hits_to_break(&mut fx, f, DIRT);
        let inv = fx.server.inventory_of(1).unwrap();
        assert!(inv.slots.iter().flatten().all(|s| s.wear == 0));
    }
);

#[test]
fn damage_persists_across_a_save_and_an_older_world_loads_undamaged() {
    // face-a-only: a byte format.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    assert_eq!(
        db.damage(),
        None,
        "a world from before damage has no record"
    );
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "creative")
    };
    {
        let mut fx = scene_with(f, cfg());
        hold(&mut fx, "creative", "dwell:stone_pickaxe");
        hit(&mut fx, f, STONE);
        hit(&mut fx, f, STONE);
        fx.server.save_now();
        fx.server.flush_store();
    }
    assert!(db.damage().is_some(), "saved");
    let mut fx = scene_with(f, cfg());
    hold(&mut fx, "creative", "dwell:stone_pickaxe");
    assert_eq!(hits_to_break(&mut fx, f, STONE), 3, "two of five hits kept");
    // A block never hit (an older world's every block) takes its full count.
    assert_eq!(hits_to_break(&mut fx, f, [-3, BASE - 1, 0]), 5);
}

#[test]
fn damage_and_its_struck_point_reach_a_client_that_draws_it_and_go_when_it_breaks() {
    // face-a-only: the message's cells are world-oriented; the drawing's faces are the mesher's
    // both-faces test (dwell-mesh damage_mesh).
    let f = FaceFrame::A;
    let mut fx = scene_with(f, config(f, "creative"));
    // A second client asks for damage (the Inventory's DAMAGE flag says the server keeps it).
    fx.join(2, &Client::new(2));
    fx.send(2, Message::DamageHello);
    for _ in 0..3 {
        fx.server.step();
    }
    fx.take_all();
    hold(&mut fx, "creative", "dwell:stone_pickaxe");
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: f.cell(STONE),
            face: 2,
            material: 0,
            rows: 1,
            down: false,
            point: Some([40, 255, 200]),
        }),
    );
    for _ in 0..7 {
        fx.server.step();
    }
    let d = fx.server.damage_of(f.cell(STONE)).expect("damaged");
    assert_eq!(d.marks[0].point, [40, 255, 200], "the struck point kept");
    let sent = fx.take(2);
    let view = sent
        .iter()
        .find_map(|(m, _)| match m {
            Message::Damage(d) => d.cells.iter().find(|(c, _)| *c == f.cell(STONE)).cloned(),
            _ => None,
        })
        .expect("player 2 was told")
        .1;
    assert_eq!(view.fraction, 51, "1 of 5 hits");
    assert_eq!(view.marks[0].point, [40, 255, 200]);
    // Player 1 never asked: nothing it might not read.
    assert!(
        !fx.take(1)
            .iter()
            .any(|(m, _)| matches!(m, Message::Damage(_)))
    );
    for _ in 0..4 {
        hit(&mut fx, f, STONE);
    }
    assert_eq!(block_at(&mut fx, f, STONE), "dwell:air");
    assert!(
        fx.take(2)
            .iter()
            .any(|(m, _)| matches!(m, Message::Damage(d) if d.gone.contains(&f.cell(STONE))))
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
