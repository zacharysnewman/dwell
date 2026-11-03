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

fn log() -> u16 {
    dwell_blocks::require("dwell:log[axis=y]")
}

fn place(f: &mut Fixture, cell: [i32; 3], face: u8, m: u16) {
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell,
            face,
            material: m,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
}

fn steps(f: &mut Fixture, n: u32) {
    for _ in 0..n {
        f.server.step();
    }
}

/// Player 1 lays a build platform on the ground beside it (a block of the world's grid), puts a
/// log on its top (a held body), a log on that log (it joins), and a log beside the first over
/// the open ground, as a drag's fill does (it joins too): returns the body's spawn.
fn build_on_a_platform(f: &mut Fixture) -> EntitySpawn {
    join_streaming(f, 1);
    steps(f, 30);
    f.take_all();
    place(f, [1, G - 1, 1], 2, platform());
    assert_eq!(
        f.server.world().get_voxel(1, G, 1),
        platform(),
        "the platform is a grid block"
    );
    // A platform on a platform: ground too (red before: it started a body).
    steps(f, 6);
    place(f, [1, G, 1], 2, platform());
    assert_eq!(
        f.server.world().get_voxel(1, G + 1, 1),
        platform(),
        "stacked, in the grid"
    );
    assert!(
        spawns(&f.take_all()[&1]).is_empty(),
        "the platforms start no body"
    );
    steps(f, 6);
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [1, G + 1, 1],
            face: 2,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    f.take_all();
    assert_eq!(
        f.server.world().get_voxel(1, G + 1, 1),
        0,
        "the second one broken"
    );
    steps(f, 6);
    place(f, [1, G, 1], 2, log());
    let out = f.take_all();
    let spawn = spawns(&out[&1])
        .pop()
        .expect("a log on the platform starts a body");
    assert_eq!(
        (spawn.kind, spawn.cells.len()),
        (1, 1),
        "a built body of one cell"
    );
    assert_eq!(
        f.server.world().get_voxel(1, G + 1, 1),
        0,
        "not in the grid"
    );
    steps(f, 6);
    // A log on its top: a block placed into the cell over it (the world's), as a click against the
    // log's top sends it (the client aims at the body; the server's rule is the cell's).
    f.send(
        1,
        Message::BodyEditRequest(BodyEditRequest {
            body: spawn.id,
            cell: [0, 0, 0],
            face: 2,
            action: BlockEditAction::Place,
            material: log(),
        }),
    );
    f.server.step();
    steps(f, 6);
    // Beside the first log, over the ground past the platform's edge: a drag's one-cell fill.
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Fill,
            min: [2, G + 1, 1],
            size: [1, 1, 1],
            material: log(),
            from: 0,
            cells: Vec::new(),
        }),
    );
    f.server.step();
    let out = f.take_all();
    let mut joined: Vec<[i16; 3]> = out[&1]
        .iter()
        .filter_map(|m| match m {
            Message::BodyEdit(e) if e.body == spawn.id => Some(e.cells[0].offset),
            _ => None,
        })
        .collect();
    joined.sort_unstable();
    assert_eq!(
        joined,
        vec![[0, 1, 0], [1, 0, 0]],
        "the log on it and the one beside it join"
    );
    assert_eq!(
        f.server.world().get_voxel(2, G + 1, 1),
        0,
        "the fill went into the body"
    );
    spawn
}

#[test]
fn blocks_built_on_a_platform_are_one_held_body_until_the_platform_goes() {
    let mut f = Fixture::new(flat());
    let raft = build_on_a_platform(&mut f);
    let held = f.server.body_pose(raft.id).expect("the body");
    // Held: an overhang (the log beside it over open air) doesn't tip it, nor does time.
    steps(&mut f, 120);
    let (at, turn) = f.server.body_pose(raft.id).expect("still a body");
    assert!(
        (at - held.0).length() < 1e-9 && turn.angle_between(held.1) < 1e-9,
        "held still"
    );
    // A player joining sees it whole.
    join_streaming(&mut f, 2);
    f.server.step();
    let seen = spawns(&f.take_all()[&2]);
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].id, seen[0].kind, seen[0].cells.len()),
        (raft.id, 1, 3)
    );
    // The platform broken from under it: it moves (the overhang tips it off), and never settles.
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [1, G, 1],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    steps(&mut f, 60 * 4);
    assert_eq!(
        f.server.world().get_voxel(1, G, 1),
        0,
        "the platform is gone"
    );
    let (at, _) = f
        .server
        .body_pose(raft.id)
        .expect("still a body: it never settles");
    assert!(
        (at - held.0).length() > 0.2,
        "it moved once let go: {at:?} from {:?}",
        held.0
    );
}

#[test]
fn a_held_body_is_saved_with_the_world_and_comes_back_held() {
    let db = WorldDb::in_memory().expect("db");
    let config = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..flat()
    };
    let (raft, held) = {
        let mut f = Fixture::new(config());
        let raft = build_on_a_platform(&mut f);
        let held = f.server.body_pose(raft.id).expect("the body");
        f.server.save_now();
        f.server.flush_store();
        (raft, held)
    };
    assert_eq!(db.bodies().len(), 1, "saved");
    let mut f = Fixture::new(config());
    join_streaming(&mut f, 1);
    f.server.step();
    let seen = spawns(&f.take_all()[&1]);
    assert_eq!(seen.len(), 1, "it came back");
    assert_eq!(
        (seen[0].id, seen[0].kind, seen[0].cells.len()),
        (raft.id, 1, 3)
    );
    // Held again on its platform: the overhang doesn't tip it.
    steps(&mut f, 120);
    let (at, _) = f.server.body_pose(raft.id).expect("a body");
    assert!(
        (at - held.0).length() < 1e-6,
        "held where it was: {at:?} vs {:?}",
        held.0
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
