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
    // Within the edit rate (10 a second), past a tool's contact (WORK_QUEUE item 25) and the
    // hand's punch rate (WORK_QUEUE item 17).
    for _ in 0..(7 + contact_ticks("dwell:stone_pickaxe")).max(punch_ticks()) {
        fx.server.step();
    }
}

/// The ticks from a hit's request to its tool's contact (`contactS` in shared/tools/tools.json).
fn contact_ticks(item: &str) -> u32 {
    (chopping::break_contact_s(Some(item)) * f64::from(SIM_HZ) - 1e-9).ceil() as u32
}

/// The ticks between punches at the hand's rate (`breaking.hand.repeatS`), rounded up.
fn punch_ticks() -> u32 {
    let r = chopping::HAND_REPEAT_S.expect("the hand repeats on hold");
    (r * f64::from(SIM_HZ) - 1e-9).ceil() as u32
}

fn send_hit(fx: &mut Fixture, f: FaceFrame, local: [i32; 3]) {
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
}

fn wear_of(fx: &Fixture, item: &str) -> u16 {
    fx.server
        .inventory_of(1)
        .unwrap()
        .slots
        .iter()
        .flatten()
        .find(|s| s.item == item)
        .map_or(0, |s| s.wear)
}

dwell_world::both_faces!(
    fn a_tool_hit_lands_at_its_swings_contact_not_when_it_arrives(f: FaceFrame) {
        // Owner, 2026-11-12: "the gashes in blocks from tools show up on click instead of when the
        // animation lines up with the block. We want that timing." The damage, the mark and the
        // wear land at the pickaxe's contact (its data), never on the request's arrival.
        for (tool, cell) in [
            ("dwell:stone_pickaxe", STONE),
            ("dwell:wooden_shovel", DIRT),
        ] {
            let mut fx = scene_with(f, config(f, "adventure"));
            hold(&mut fx, "adventure", tool);
            let n = contact_ticks(tool);
            assert!(n > 5, "{tool}: a contact after the click ({n} ticks)");
            fx.take_all();
            send_hit(&mut fx, f, cell);
            // The step that reads the request, and `n` - 1 ticks after it.
            for _ in 0..n {
                fx.server.step();
            }
            assert!(
                fx.server.damage_of(f.cell(cell)).is_none(),
                "{tool}: no mark before contact"
            );
            assert_eq!(wear_of(&fx, tool), 0, "{tool}: no wear before contact");
            assert!(
                !fx.take(1)
                    .iter()
                    .any(|(m, _)| matches!(m, Message::Damage(_))),
                "{tool}: no damage sent before contact"
            );
            fx.server.step();
            assert_eq!(
                fx.server.damage_of(f.cell(cell)).map(|d| d.hits),
                Some(1),
                "{tool}: the hit at contact"
            );
            assert_eq!(wear_of(&fx, tool), 1, "{tool}: one wear, at contact");
            for _ in 0..30 {
                fx.server.step();
            }
            assert_eq!(
                fx.server.damage_of(f.cell(cell)).map(|d| d.hits),
                Some(1),
                "{tool}: once"
            );
            assert_eq!(wear_of(&fx, tool), 1, "{tool}: worn once");
        }
    }
);

/// Says `hits.at_contact`: this client sends a tool's hit at its swing's contact, aimed then.
fn says_hits_at_contact(fx: &mut Fixture) {
    fx.send(
        1,
        Message::Capabilities(capabilities::Capabilities::of([
            capability_names::HITS_AT_CONTACT,
        ])),
    );
    fx.server.step();
}

dwell_world::both_faces!(
    fn a_hit_sent_at_its_contact_lands_on_arrival_checked_from_the_eye_then(f: FaceFrame) {
        // BUGS.md #111 (owner, 2026-11-12): "if I click on a block, and move my cursor to aim at
        // the block below it, I'll see and hear the tool hit, but it won't be on the block I'm
        // aiming at". A client with `hits.at_contact` sends the hit at the contact, naming what it
        // aims at then: the server lands it at once (no second wait), checked as any edit.
        for (tool, cell) in [
            ("dwell:stone_pickaxe", STONE),
            ("dwell:wooden_shovel", DIRT),
        ] {
            let mut fx = scene_with(f, config(f, "adventure"));
            says_hits_at_contact(&mut fx);
            hold(&mut fx, "adventure", tool);
            fx.take_all();
            send_hit(&mut fx, f, cell);
            fx.server.step();
            assert_eq!(
                fx.server.damage_of(f.cell(cell)).map(|d| d.hits),
                Some(1),
                "{tool}: landed on arrival (the client's contact)"
            );
            assert_eq!(wear_of(&fx, tool), 1, "{tool}: one wear, at the hit");
            for _ in 0..30 {
                fx.server.step();
            }
            assert_eq!(
                fx.server.damage_of(f.cell(cell)).map(|d| d.hits),
                Some(1),
                "{tool}: once"
            );
            assert_eq!(wear_of(&fx, tool), 1, "{tool}: worn once");
        }
        // Validated then: a cell the eye can't see (buried under the top stone) is refused,
        // landing nothing and wearing nothing.
        let mut fx = scene_with(f, config(f, "adventure"));
        says_hits_at_contact(&mut fx);
        hold(&mut fx, "adventure", "dwell:stone_pickaxe");
        let buried = [STONE[0], BASE - 3, STONE[2]];
        send_hit(&mut fx, f, buried);
        for _ in 0..30 {
            fx.server.step();
        }
        assert!(
            fx.server.damage_of(f.cell(buried)).is_none(),
            "out of sight"
        );
        assert_eq!(wear_of(&fx, "dwell:stone_pickaxe"), 0, "no wear");
    }
);

dwell_world::both_faces!(
    fn a_hit_whose_tool_is_put_away_before_contact_lands_nothing(f: FaceFrame) {
        for mode in ["adventure", "creative"] {
            let mut fx = scene_with(f, config(f, mode));
            hold(&mut fx, mode, "dwell:stone_pickaxe");
            send_hit(&mut fx, f, STONE);
            fx.server.step();
            // Switched to the shovel mid-swing.
            hold(&mut fx, mode, "dwell:wooden_shovel");
            for _ in 0..40 {
                fx.server.step();
            }
            assert!(
                fx.server.damage_of(f.cell(STONE)).is_none(),
                "{mode}: cancelled"
            );
            assert_eq!(wear_of(&fx, "dwell:stone_pickaxe"), 0, "{mode}: no wear");
            assert_eq!(block_at(&mut fx, f, STONE), "dwell:stone", "{mode}");
        }
    }
);

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

dwell_world::both_faces!(
    fn a_metal_tool_breaks_in_about_half_the_hits_with_bigger_bites(f: FaceFrame) {
        // Owner, 2026-11-12: "Metal tools break double sized bigger chunks off and reach 50%
        // broken faster, which is when the blocks break. Also have double durability."
        // A mark's chunk is its hit's bite (BLOCK_DAMAGE.md §10): 0.5 × the cell ÷ the hits, in
        // sixteenths of a 1-in-8 bite: 26 for 5 hits, 43 for 3 (the metal tool's 5/3 the stone's).
        for (stone_tool, metal_tool, cell) in [
            ("dwell:stone_pickaxe", "dwell:metal_pickaxe", STONE),
            ("dwell:wooden_shovel", "dwell:metal_shovel", DIRT),
        ] {
            let mut fx = scene_with(f, config(f, "adventure"));
            hold(&mut fx, "adventure", stone_tool);
            hit(&mut fx, f, cell);
            let d = fx.server.damage_of(f.cell(cell)).expect("damaged").clone();
            assert_eq!(d.marks[0].chunk, 26, "{stone_tool}: a 5-hit bite");
            let s = 1 + hits_to_break(&mut fx, f, cell);
            assert_eq!(
                wear_of(&fx, stone_tool),
                s as u16,
                "{stone_tool}: a wear a hit"
            );

            let mut fx = scene_with(f, config(f, "adventure"));
            hold(&mut fx, "adventure", metal_tool);
            hit(&mut fx, f, cell);
            let d = fx.server.damage_of(f.cell(cell)).expect("damaged").clone();
            assert_eq!(d.marks[0].chunk, 43, "{metal_tool}: a 3-hit bite");
            // Half-way to breaking after half as many hits.
            assert!(
                f64::from(d.hits) / f64::from(d.need) >= 0.3,
                "{metal_tool}: {} of {}",
                d.hits,
                d.need
            );
            let m = 1 + hits_to_break(&mut fx, f, cell);
            assert!(
                m <= s.div_ceil(2) && 2 * m + 1 >= s,
                "{metal_tool}: {m} hits against {stone_tool}'s {s}"
            );
            assert_eq!(
                wear_of(&fx, metal_tool),
                m as u16,
                "{metal_tool}: a wear a hit"
            );
            assert_eq!(
                chopping::durability(metal_tool),
                chopping::durability(stone_tool).map(|h| 2 * h),
                "{metal_tool}: double durability"
            );
        }
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
        // Each hit's piece is a real 3D shard (BLOCK_DAMAGE.md §6.2, owner: "both"): the loose item
        // is drawn by its look, the angular `shard`, no bigger than the mark the hit carves.
        let piece = dwell_protocol::inventory::item("dwell:rock_shard")
            .and_then(|d| d.look)
            .expect("a rock shard's look is data");
        assert_eq!(piece.shape, dwell_protocol::inventory::LookShape::Shard);
        let mark = dwell_protocol::damage::look_of("dwell:stone").expect("stone's mark is data");
        assert!(
            piece.size.iter().all(|&s| s <= mark.size) && piece.size[1] <= mark.depth,
            "the piece fits the chunk it came out of: {:?} in {} × {}",
            piece.size,
            mark.size,
            mark.depth
        );
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

dwell_world::both_faces!(
    fn a_worn_tool_dropped_at_death_and_picked_up_keeps_its_wear(f: FaceFrame) {
        // A dead player's axe fell as a loose item with no wear: picked back up it was a fresh
        // 32-hit axe. The loose item carries the slot's wear.
        let mut fx = scene_with(f, config(f, "adventure"));
        assert_eq!(hits_to_break(&mut fx, f, DIRT), 11);
        fx.server.kill_player(1);
        // Defeated, it keeps its things while it kneels (CHARACTERS.md §11.13) and leaves them
        // where it knelt when it stands up at the shrine (here the spawn).
        let loose_axes = |fx: &Fixture| -> Vec<u16> {
            fx.server
                .loose_items()
                .iter()
                .filter(|it| it.stack.item == "dwell:stone_axe")
                .map(|it| it.stack.wear)
                .collect()
        };
        assert!(loose_axes(&fx).is_empty(), "nothing dropped while kneeling");
        let ticks = dwell_sim::server::defeat_ticks(dwell_protocol::defeat::PLAYER) + SIM_HZ;
        for _ in 0..ticks {
            fx.server.step();
        }
        assert_eq!(loose_axes(&fx), [11], "the dropped axe keeps its wear");
        // The player stands up where it knelt (the spawn) and walks over its drops.
        fx.move_to_local(f, 0.5, m_of_row(BASE), 0.5);
        for _ in 0..60 {
            fx.server.step();
        }
        let inv = fx.server.inventory_of(1).expect("respawned");
        let worn: Vec<_> = inv
            .slots
            .iter()
            .flatten()
            .filter(|s| s.item == "dwell:stone_axe")
            .map(|s| s.wear)
            .collect();
        assert_eq!(worn, [11], "picked up with 21 hits left, not a fresh axe");
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
    for _ in 0..7 + contact_ticks("dwell:stone_pickaxe") {
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

#[test]
fn a_marks_chunk_reaches_only_a_client_that_lists_damage_chunks() {
    // face-a-only: the message's chunk bytes; the carve's size is the mesher's both-faces test.
    let f = FaceFrame::A;
    let mut fx = scene_with(f, config(f, "creative"));
    fx.join(2, &Client::new(2));
    fx.send(2, Message::DamageHello);
    fx.join(3, &Client::new(3));
    fx.send(
        3,
        Message::Capabilities(dwell_protocol::capabilities::Capabilities::this_client()),
    );
    fx.send(3, Message::DamageHello);
    for _ in 0..3 {
        fx.server.step();
    }
    fx.take_all();
    hold(&mut fx, "creative", "dwell:metal_pickaxe");
    hit(&mut fx, f, STONE);
    // One take for every client (`take` keeps only one session's messages).
    let mut sent = fx.take_all();
    let mut seen = |id| {
        sent.remove(&id)
            .unwrap_or_default()
            .into_iter()
            .find_map(|m| match m {
                Message::Damage(d) if !d.cells.is_empty() => Some(d),
                _ => None,
            })
            .expect("told")
    };
    let older = seen(2);
    assert!(!older.chunks, "an older client's message is as before");
    assert_eq!(older.cells[0].1.marks[0].chunk, 16, "read as ×1");
    let newer = seen(3);
    assert!(newer.chunks);
    assert_eq!(
        newer.cells[0].1.marks[0].chunk, 43,
        "the metal pickaxe's 3-hit bite"
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

dwell_world::both_faces!(
    fn punches_at_the_hands_rate_land_and_break_dirt_after_its_hand_hits(f: FaceFrame) {
        // Owner, 2026-11-13: "You should also be able to hold 'punch' to dig with your hand." A
        // held punch is a hit every `repeatS` (`breaking.hand`): each lands, and the dirt breaks
        // after its hits by hand.
        let mut fx = scene_with(f, config(f, "adventure"));
        empty_hand(&mut fx);
        let need = damage::hits_to_break("dwell:dirt", None).expect("dirt breaks by hand");
        assert!(need > 1);
        let before = fx.server.stats_of(1).unwrap();
        let cell = f.cell(DIRT);
        for k in 1..=need {
            assert_eq!(block_at(&mut fx, f, DIRT), "dwell:dirt", "before punch {k}");
            send_hit(&mut fx, f, DIRT);
            for _ in 0..punch_ticks() {
                fx.server.step();
            }
            if k < need {
                assert_eq!(
                    fx.server.damage_of(cell).map(|d| u32::from(d.hits)),
                    Some(k),
                    "punch {k} landed"
                );
            }
        }
        assert_ne!(
            block_at(&mut fx, f, DIRT),
            "dwell:dirt",
            "broken by punch {need}"
        );
        let after = fx.server.stats_of(1).unwrap();
        assert_eq!(after.edits_rejected, before.edits_rejected, "none refused");
    }
);

dwell_world::both_faces!(
    fn punches_faster_than_the_hands_rate_are_refused(f: FaceFrame) {
        // The server takes a punch no faster than the hand's data rate (one spare for the
        // network's bunching): a client punching at twice it lands about half.
        let mut fx = scene_with(f, config(f, "adventure"));
        empty_hand(&mut fx);
        let every = punch_ticks().div_ceil(2).max(7);
        let seconds = 2.0;
        let sends = (seconds * f64::from(SIM_HZ) / f64::from(every)) as u32;
        let before = fx.server.stats_of(1).unwrap();
        for _ in 0..sends {
            send_hit(&mut fx, f, DIRT);
            for _ in 0..every {
                fx.server.step();
            }
        }
        let landed = fx
            .server
            .damage_of(f.cell(DIRT))
            .map_or(0, |d| u32::from(d.hits));
        let r = chopping::HAND_REPEAT_S.unwrap();
        let most = (f64::from(sends * every) / f64::from(SIM_HZ) / r).floor() as u32 + 2;
        assert!(
            landed <= most && landed < sends,
            "{landed} of {sends} punches landed, at most {most}"
        );
        let after = fx.server.stats_of(1).unwrap();
        assert_eq!(
            after.edits_rejected - before.edits_rejected,
            sends - landed,
            "the rest refused"
        );
        assert_eq!(block_at(&mut fx, f, DIRT), "dwell:dirt");
    }
);
