//! Built bodies (docs/BUILT_BODIES.md, ADR 0052, Phase 51 B2): the build platform starts a body
//! instead of going in the world's grid; blocks placed against it join it; it never settles; a
//! player joining is sent it; it is saved with the world and comes back as it was.
//! face-a-only: the flat test world is face A's; a body has no up (its own grid), and its gravity
//! is the face's, tested with the pieces (dwell-physics).

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, chunk_hash, generate_flat_chunk};

const G: i32 = FLAT_TOP_Y;

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

fn spawns(out: &[Message]) -> Vec<EntitySpawn> {
    out.iter()
        .filter_map(|m| match m {
            Message::EntitySpawn(e) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

fn platform() -> u16 {
    dwell_blocks::require("dwell:build_platform")
}

/// Player 1 places a build platform on the ground beside it, then a log on the platform's top:
/// returns the body's spawn.
fn build_a_raft(f: &mut Fixture) -> EntitySpawn {
    join_streaming(f, 1);
    for _ in 0..30 {
        f.server.step();
    }
    f.take_all();
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [1, G - 1, 1],
            face: 2,
            material: platform(),
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    let out = f.take_all();
    let spawn = spawns(&out[&1]).pop().expect("the platform starts a body");
    assert_eq!(
        (spawn.kind, spawn.cells.len()),
        (1, 1),
        "a built body of one cell"
    );
    assert_eq!(spawn.cells[0].state, platform());
    assert_eq!(
        f.server.world().get_voxel(1, G, 1),
        0,
        "not in the world's grid"
    );
    // Let it come to rest on the ground, then put a log on its top (+Y).
    for _ in 0..60 {
        f.server.step();
    }
    f.take_all();
    f.send(
        1,
        Message::BodyEditRequest(BodyEditRequest {
            body: spawn.id,
            cell: [0, 0, 0],
            face: 2,
            action: BlockEditAction::Place,
            material: dwell_blocks::require("dwell:log[axis=y]"),
        }),
    );
    f.server.step();
    let out = f.take_all();
    let edit = out[&1]
        .iter()
        .find_map(|m| match m {
            Message::BodyEdit(e) => Some(e.clone()),
            _ => None,
        })
        .expect("the log joins the body");
    assert_eq!(edit.body, spawn.id);
    assert_eq!(edit.cells.len(), 1);
    assert_eq!(edit.cells[0].offset, [0, 1, 0], "on its top");
    spawn
}

#[test]
fn a_build_platform_starts_a_body_that_takes_blocks_and_never_settles() {
    let mut f = Fixture::new(flat());
    let raft = build_a_raft(&mut f);
    // Ten seconds at rest: a natural piece would have settled (2 s); this one stays a body.
    for _ in 0..(60 * 10) {
        f.server.step();
    }
    let out = f.take_all();
    assert!(
        !out.get(&1)
            .into_iter()
            .flatten()
            .any(|m| matches!(m, Message::EntityDespawn(id) if *id == raft.id)),
        "it never settles"
    );
    assert_eq!(
        f.server.world().get_voxel(1, G, 1),
        0,
        "nothing written into the grid"
    );
    // A player joining now is sent it, with its two cells.
    join_streaming(&mut f, 2);
    f.server.step();
    let out = f.take_all();
    let seen = spawns(&out[&2]);
    assert_eq!(seen.len(), 1, "the joiner sees the body");
    assert_eq!(
        (seen[0].id, seen[0].kind, seen[0].cells.len()),
        (raft.id, 1, 2)
    );
}

#[test]
fn a_built_body_is_saved_with_the_world_and_comes_back() {
    let db = WorldDb::in_memory().expect("db");
    let config = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..flat()
    };
    let raft = {
        let mut f = Fixture::new(config());
        let raft = build_a_raft(&mut f);
        f.server.save_now();
        f.server.flush_store();
        raft
    };
    assert_eq!(db.bodies().len(), 1, "saved");
    // A new server over the same world: a player joining is sent the raft, both its cells.
    let mut f = Fixture::new(config());
    join_streaming(&mut f, 1);
    f.server.step();
    let out = f.take_all();
    let seen = spawns(&out[&1]);
    assert_eq!(seen.len(), 1, "the raft came back");
    let mut cells: Vec<([i16; 3], u16)> =
        seen[0].cells.iter().map(|c| (c.offset, c.state)).collect();
    cells.sort_unstable();
    assert_eq!(
        (seen[0].id, seen[0].kind, cells),
        (
            raft.id,
            1,
            vec![
                ([0, 0, 0], platform()),
                ([0, 1, 0], dwell_blocks::require("dwell:log[axis=y]"))
            ]
        )
    );
    // Where it rested: on the ground (its reference cell's centre a quarter metre up).
    let y = seen[0].origin[1];
    assert!(
        (y - (f64::from(G) * 0.5 + 0.25)).abs() < 0.05,
        "at rest on the ground: {y}"
    );
}

#[test]
fn a_wooden_raft_floats_and_a_stone_one_sinks() {
    // docs/BUILT_BODIES.md §6 (owner, 2026-11-02: built objects "float in water"): each cell is
    // pushed up by the water it displaces, by the water's density. A raft of logs (700 kg/m³, under
    // water's 1000) floats with about 70 % of it under; one of stone (2400) sinks to the bottom.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // A pool 12 × 12, four rows (2 m) deep, full of water, its surface level with the ground.
    let (water, air) = (dwell_blocks::require("dwell:water"), 0);
    for x in 4..16 {
        for z in 4..16 {
            for y in G - 4..G {
                f.server.world().set_voxel(x, y, z, water);
            }
            f.server.world().set_voxel(x, G, z, air);
        }
    }
    let surface = f64::from(G) * 0.5;
    let raft = |m: u16| -> Vec<([i32; 3], u16)> {
        (0..3)
            .flat_map(|x| (0..3).map(move |z| ([x, 0, z], m)))
            .collect()
    };
    let log = f.server.add_body(
        1,
        raft(dwell_blocks::require("dwell:log[axis=y]")),
        [6.5, surface + 1.0, 6.5],
    );
    let stone = f.server.add_body(
        1,
        raft(dwell_blocks::require("dwell:stone")),
        [11.5, surface + 1.0, 11.5],
    );
    for _ in 0..(60 * 12) {
        f.server.step();
    }
    let (at, turn) = f.server.body_pose(log).expect("the log raft");
    // Floating: its cells' centres 0.1 m under the surface (70 % of a ½ m cell under), level.
    assert!(
        (at.y - (surface - 0.1)).abs() < 0.06,
        "the log raft floats: its centre {:.3} m, the surface {surface}",
        at.y
    );
    assert!(
        turn.angle_between(dwell_physics::DQuat::IDENTITY) < 0.1,
        "level: {turn:?}"
    );
    let (at, _) = f.server.body_pose(stone).expect("the stone raft");
    let bottom = f64::from(G - 4) * 0.5 + 0.25;
    assert!(
        (at.y - bottom).abs() < 0.06,
        "the stone raft sank: {:.3} m (bottom {bottom})",
        at.y
    );
}
