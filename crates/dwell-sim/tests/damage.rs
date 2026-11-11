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
        let mut fx = scene_with(f, config(f, "creative"));
        assert_eq!(hits_to_break(&mut fx, f, DIRT), 11, "creative: bare hands");
    }
);

dwell_world::both_faces!(
    fn bare_hands_and_the_wrong_tool_do_nothing_to_stone(f: FaceFrame) {
        let mut fx = scene_with(f, config(f, "creative"));
        for _ in 0..12 {
            hit(&mut fx, f, STONE);
        }
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:stone", "bare hands");
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
        // Stone drops one stone: half of it (none) over the hits, the rest on the destroy.
        assert_eq!(loose(&fx, "dwell:stone"), 0);
        hit(&mut fx, f, STONE);
        assert_eq!(block_at(&mut fx, f, STONE), "dwell:air");
        assert_eq!(loose(&fx, "dwell:stone"), 1, "the block's drops, once");
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
