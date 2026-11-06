//! Built bodies (docs/BUILT_BODIES.md, ADR 0052, Phase 51 B2): the build platform starts a body
//! instead of going in the world's grid; blocks placed against it join it; it never settles; a
//! player joining is sent it; it is saved with the world and comes back as it was.
//! face-a-only: the flat test world is face A's; a body has no up (its own grid), and its gravity
//! is the face's, tested with the pieces (dwell-physics).

mod common;

use common::*;
use dwell_physics::DVec3;
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

#[test]
fn a_player_stands_steadily_on_a_floating_raft() {
    // Owner, 2026-11-02: "my character freaks out when standing on a floating object, hand and feet
    // animations twitch and spam". A raft afloat accelerates as it bobs (buoyancy, ~0.12 m/s a
    // tick): the controller read the player carried with it as launched off it every tick, so
    // never grounded (Falling). Settled on it, the player stays grounded. Red before: 0 of 180.
    let (mut f, raft) = floating_raft();
    let (at, _) = f.server.body_pose(raft).expect("the raft");
    f.move_to(1, 1.0, at.y + 0.25 + 0.02, 1.0);
    let h = f.server.player_handle_of(1).expect("the player");
    steps(&mut f, 60);
    let mut grounded = 0;
    for _ in 0..180 {
        f.server.step();
        let (_, _, players) = f.server.sim();
        grounded += i32::from(players.controller(h).ground.grounded);
    }
    assert!(
        grounded >= 175,
        "grounded on the raft {grounded} of 180 ticks"
    );
}

/// A 2 × 2 log raft (1,400 kg) settled afloat in a pool: the fixture, the raft's id.
fn floating_raft() -> (Fixture, u32) {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let water = dwell_blocks::require("dwell:water");
    for x in -6..8 {
        for z in -6..8 {
            for y in G - 6..G {
                f.server.world().set_voxel(x, y, z, water);
            }
        }
    }
    let surface = f64::from(G) * 0.5;
    let cells: Vec<([i32; 3], u16)> = (0..2)
        .flat_map(|x| (0..2).map(move |z| ([x, 0, z], log())))
        .collect();
    let raft = f.server.add_body(1, cells, [0.5, surface, 0.5]);
    steps(&mut f, 240);
    (f, raft)
}

#[test]
fn a_raft_let_go_in_water_glides_on() {
    // Owner, 2026-11-03: water resists as real water does, by the area facing the motion and the
    // speed squared, so a heavy raft let go glides. Red before: a drag of 1.2/s on its velocity
    // took 70% of its speed a second, and 1 m/s was 0.19 m/s two seconds on.
    let (mut f, raft) = floating_raft();
    let body = f.server.body_handle(raft).expect("the raft");
    {
        let (_, physics, _) = f.server.sim();
        let v = physics.velocity(body);
        physics.set_velocity(body, DVec3::new(1.0, v.y, 0.0));
    }
    steps(&mut f, 120);
    let (_, physics, _) = f.server.sim();
    let v = physics.velocity(body);
    let speed = DVec3::new(v.x, 0.0, v.z).length();
    assert!(
        (0.4..0.8).contains(&speed),
        "two seconds on the raft goes {speed} m/s"
    );
}

#[test]
fn a_player_on_a_floating_raft_presses_it_down() {
    // Owner, 2026-11-03: jumping on a log pushes it down, with a true force. An 80 kg player on a
    // 2 × 2 m raft sinks it by their weight's water (80 kg / (1000 kg/m³ · 4 m²) = 2 cm), and a
    // landing from a jump dips it further. Red before: the held raft ignored its rider.
    let (mut f, raft) = floating_raft();
    let (at, _) = f.server.body_pose(raft).expect("the raft");
    let unladen = at.y;
    f.move_to(1, 1.0, at.y + 0.25 + 0.02, 1.0);
    steps(&mut f, 240);
    let laden = f.server.body_pose(raft).expect("the raft").0.y;
    let dip = unladen - laden;
    assert!(
        (0.012..0.03).contains(&dip),
        "the rider sank the raft {dip} m"
    );
    // Dropped onto it from 2 m up.
    f.move_to(1, 1.0, laden + 0.25 + 2.0, 1.0);
    let mut lowest = laden;
    for _ in 0..90 {
        f.server.step();
        f.take_all();
        lowest = lowest.min(f.server.body_pose(raft).expect("the raft").0.y);
    }
    assert!(
        unladen - lowest > dip + 0.01,
        "the landing dipped the raft {} m (standing: {dip} m)",
        unladen - lowest
    );
}

#[test]
fn a_floating_raft_drifts_with_the_current() {
    // Phase 53 G3a (docs/WATER_SURFACE_GRID.md §2c): water slows a body toward the water's own
    // velocity, so in a 0.5 m/s current a raft let be comes up to it (a 1,400 kg raft slowly: by
    // the area it shows the water and the speed squared, as it glides on, above).
    let (mut f, raft) = floating_raft();
    let face = dwell_world::FaceFrame::A;
    for x in -6..8 {
        for z in -6..8 {
            let (key, i) = dwell_world::currents::current_key(face, x, z);
            let world = f.server.world();
            let mut cells = world
                .currents
                .get(key)
                .cloned()
                .unwrap_or_else(|| Box::new([[0.0; 2]; dwell_world::currents::CA]));
            cells[i] = [0.0, 0.5];
            world.currents.set(key, Some(cells));
        }
    }
    let body = f.server.body_handle(raft).expect("the raft");
    steps(&mut f, 600);
    let (_, physics, _) = f.server.sim();
    let v = physics.velocity(body);
    assert!(
        v.x.abs() < 0.05 && (0.3..0.51).contains(&v.z),
        "ten seconds on the raft goes {v:?} in a 0.5 m/s current along z"
    );
}
