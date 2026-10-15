//! Block edits through the server (ARCHITECTURE.md §6.5, Phase 3d): the server's validation of
//! edit requests (§11), VoxelModification broadcasts with per-chunk revisions, and resync.

mod common;

use common::*;
use dwell_protocol::players::BLOCK_EDIT_INTERVAL_MS;
use dwell_protocol::*;
use dwell_sim::server::{EditPolicy, ServerConfig, SessionId};
use dwell_world::edit::EditCheck;
use dwell_world::{Chunk, ChunkCoord, M, chunk_hash, chunk_of, generate_flat_chunk, local_index};

fn brk(cell: [i32; 3], face: u8) -> Message {
    Message::BlockEditRequest(BlockEditRequest {
        action: BlockEditAction::Break,
        cell,
        face,
        material: 0,
    })
}
fn place(cell: [i32; 3], face: u8, m: u16) -> Message {
    Message::BlockEditRequest(BlockEditRequest {
        action: BlockEditAction::Place,
        cell,
        face,
        material: m,
    })
}

/// Joins a streaming client (its worldgen check passes) to a flat world.
fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

fn modifications(messages: &[Message]) -> Vec<VoxelModification> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::VoxelModification(v) => Some(v.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_placed_block_reaches_every_client_streaming_its_chunk_one_revision_each() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    join_streaming(&mut f, 2);
    f.server.step(); // chunks around both players
    f.take_all();

    // Player 1 stands at (−1, 0, −0.25): two blocks on the ground ahead, in one tick.
    f.send(1, place([-1, -1, 2], 2, M.stone));
    f.send(1, place([0, -1, 2], 2, M.sand));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(-1, 0, 2), M.stone);
    assert_eq!(f.server.world().get_voxel(0, 0, 2), M.sand);
    assert_eq!(
        f.server
            .world()
            .find(ChunkCoord::new(0, 0, 0))
            .unwrap()
            .revision(),
        1
    );
    assert_eq!(
        f.server
            .world()
            .find(ChunkCoord::new(-1, 0, 0))
            .unwrap()
            .revision(),
        1
    );
    let out = f.take_all();
    for id in [1, 2] {
        let mods = modifications(&out[&id]);
        assert_eq!(mods.len(), 1, "session {id}");
        assert_eq!(mods[0].reason, VoxelModificationReason::Edit);
        // x = −1 lies in chunk −1, x = 0 in chunk 0: one entry per chunk, each at revision 1.
        assert_eq!(mods[0].chunks.len(), 2);
        assert_eq!(mods[0].chunks[0].coord, [-1, 0, 0]);
        assert_eq!(mods[0].chunks[0].revision, 1);
        assert_eq!(
            mods[0].chunks[0].changes,
            vec![VoxelChange {
                index: local_index(31, 0, 2) as u16,
                material: M.stone
            }]
        );
        assert_eq!(mods[0].chunks[1].coord, [0, 0, 0]);
        assert_eq!(mods[0].chunks[1].revision, 1);
        assert_eq!(
            mods[0].chunks[1].changes,
            vec![VoxelChange {
                index: local_index(0, 0, 2) as u16,
                material: M.sand
            }]
        );
    }

    // Breaking it again: revision 2.
    for _ in 0..10 {
        f.server.step();
    }
    f.take_all();
    f.send(2, brk([0, 0, 2], 2));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(0, 0, 2), M.air);
    let out = f.take_all();
    let mods = modifications(&out[&1]);
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].chunks.len(), 1);
    assert_eq!(mods[0].chunks[0].revision, 2);
    assert_eq!(f.server.stats_of(2).unwrap().edits_applied, 1);
}

#[test]
fn tall_grass_goes_on_grass_not_stone_and_with_the_block_under_it() {
    let tall = dwell_blocks::require("dwell:tall_grass");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let settle = |f: &mut Fixture| {
        for _ in 0..12 {
            f.server.step();
        }
        f.take_all()
    };
    // On the flat world's grass: placed. On a stone block: refused (its data's placeOn).
    f.send(1, place([0, -1, 2], 2, tall));
    f.send(1, place([-1, -1, 2], 2, M.stone));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(0, 0, 2), tall);
    settle(&mut f);
    f.send(1, place([-1, 0, 2], 2, tall));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(-1, 1, 2), M.air);
    assert_eq!(
        f.server.stats_of(1).unwrap().last_edit_check,
        EditCheck::NoSupport
    );
    settle(&mut f);
    // Dig the ground in front, then break the grass block under the plant from that side: the
    // plant goes in the same modification, in the same chunk's changes.
    f.send(1, brk([0, -1, 1], 2));
    f.server.step();
    settle(&mut f);
    f.send(1, brk([0, -1, 2], 5));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(0, -1, 2), M.air);
    assert_eq!(
        f.server.world().get_voxel(0, 0, 2),
        M.air,
        "the plant went with it"
    );
    let out = f.take_all();
    let mods = modifications(&out[&1]);
    assert_eq!(mods.len(), 1);
    let changes: Vec<VoxelChange> = mods[0]
        .chunks
        .iter()
        .flat_map(|c| c.changes.clone())
        .collect();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(changes.iter().all(|c| c.material == M.air));
    // Placing into a plant replaces it.
    f.send(1, place([1, -1, 2], 2, tall));
    f.server.step();
    settle(&mut f);
    f.send(1, place([1, 0, 2], 2, M.sand));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(1, 0, 2), M.sand);
    assert_eq!(f.server.world().get_voxel(1, 1, 2), M.air);
}

#[test]
fn rate_limited_a_burst_of_three_then_one_per_block_edit_interval() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    for x in -3..=1 {
        f.send(1, place([x, -1, 2], 2, M.dirt));
    }
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().edits_applied, 3);
    assert_eq!(f.server.stats_of(1).unwrap().edits_rejected, 2);
    let interval = BLOCK_EDIT_INTERVAL_MS * i64::from(SIM_HZ) / 1000;
    for _ in 0..interval {
        f.server.step();
    }
    f.send(1, place([0, -1, 2], 2, M.dirt));
    f.send(1, place([1, -1, 2], 2, M.dirt));
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().edits_applied, 4);
}

#[test]
fn refused_by_the_edit_policy_nobody_may_edit() {
    let mut f = Fixture::new(ServerConfig {
        edits: EditPolicy::Nobody,
        ..flat()
    });
    join_streaming(&mut f, 1);
    f.send(1, place([-1, -1, 2], 2, M.dirt));
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().edits_applied, 0);
    assert_eq!(f.server.world().get_voxel(-1, 0, 2), M.air);
}

#[test]
fn refused_by_the_edit_policy_only_ops_may_edit() {
    let mut f = Fixture::new(ServerConfig {
        edits: EditPolicy::Ops,
        ops: vec![Client::new(2).public_key],
        ..flat()
    });
    join_streaming(&mut f, 1);
    join_streaming(&mut f, 2);
    f.send(1, place([-1, -1, 2], 2, M.dirt));
    f.send(2, place([0, -1, 2], 2, M.dirt));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(-1, 0, 2), M.air);
    assert_eq!(f.server.world().get_voxel(0, 0, 2), M.dirt);
}

#[test]
fn resync_re_sends_chunks_the_client_has_with_their_current_revision() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.send(1, place([0, -1, 2], 2, M.sand));
    f.server.step();
    f.take_all();
    f.send(
        1,
        Message::ChunkResync(vec![[0, 0, 0], [0, -1, 0], [50, 0, 0]]),
    ); // the last one was never streamed
    let out = f.take_all().remove(&1).unwrap_or_default();
    assert_eq!(out.len(), 2, "{out:?}");
    let Message::ChunkData(edited) = &out[0] else {
        panic!("{:?}", out[0])
    };
    assert_eq!(edited.form, ChunkForm::Explicit);
    assert_eq!(edited.revision, 1);
    assert_eq!(edited.voxels[local_index(0, 0, 2)], M.sand);
    let Message::ChunkData(other) = &out[1] else {
        panic!("{:?}", out[1])
    };
    assert_eq!(other.form, ChunkForm::Generated);
    assert_eq!(f.server.stats_of(1).unwrap().resyncs, 2);
}

#[test]
fn an_edited_chunk_the_air_test_calls_empty_is_kept_and_streamed_explicitly() {
    let mut f = Fixture::new(ServerConfig {
        world_seed: 5,
        ..ServerConfig::default()
    });
    let w = f.join(1, &Client::new(1));
    let h = f.server.player_handle_of(w.player_id).unwrap();
    let spawn = {
        let (_, physics, players) = f.server.sim();
        players.position(physics, h)
    };
    let x = spawn.x.floor() as i32;
    let z = spawn.z.floor() as i32;
    let y = 4000; // open sky
    let sky = chunk_of(x, y, z + 2);
    // A floating block (tests put it directly), then the player next to it breaks it.
    f.server.world().set_voxel(x, y, z + 2, M.stone);
    f.move_to(
        w.player_id,
        f64::from(x) + 0.5,
        f64::from(y) - 1.0,
        f64::from(z) + 0.5,
    );
    f.send(1, Message::WorldgenCheck(0)); // full-chunk mode
    let chunks = f.chunks(1, 20, None);
    let it = chunks
        .iter()
        .find(|m| coord_of(m) == sky)
        .expect("the sky chunk streamed");
    assert_eq!(it.form, ChunkForm::Explicit);
    assert_eq!(it.revision, 1);
    assert!(chunks.iter().filter(|m| m.form == ChunkForm::Air).count() > 0);

    f.send(1, brk([x, y, z + 2], 5));
    f.server.step();
    assert_eq!(
        f.server.stats_of(w.player_id).unwrap().last_edit_check,
        EditCheck::Ok
    );
    assert_eq!(f.server.world().get_voxel(x, y, z + 2), M.air);
    assert_eq!(f.server.world().find(sky).unwrap().revision(), 2); // modified: kept, not read as air
}
