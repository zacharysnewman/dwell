//! Water flow through the server (docs/WATER_FLOW.md, W1): a player's edit reaches the basins, the
//! water ticks in game time, and its changes go out as modifications.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::SessionId;
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

const G: i32 = FLAT_TOP_Y;

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

#[test]
fn breaking_a_lakes_wall_drains_its_top_row_and_clients_see_it() {
    // face-a-only: the server's flat world is face A's; the basins' both faces are tested in
    // `water::tests`.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // A walled lake four rows deep in front of the player (x −2–2, z 3–8), its near wall at z 2.
    let stone = dwell_blocks::require("dwell:stone");
    let w = f.server.world();
    for x in -3..=3 {
        for z in 2..=9 {
            for y in G..G + 4 {
                let wall = x == -3 || x == 3 || z == 2 || z == 9;
                w.set_voxel(x, y, z, if wall { stone } else { M.water });
            }
        }
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [0, G + 3, 2],
            face: 5,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let mut drained = Vec::new();
    for _ in 0..240 {
        f.server.step();
        for (_, out) in f.take_all() {
            for m in out {
                if let Message::VoxelModification(v) = m
                    && v.reason == VoxelModificationReason::Rebake
                {
                    drained.extend(v.chunks.iter().flat_map(|c| c.changes.clone()));
                }
            }
        }
    }
    let w = f.server.world();
    for x in -2..=2 {
        for z in 3..=8 {
            assert_eq!(
                w.get_voxel(x, G + 3, z),
                M.air,
                "the top row drained at {x}, {z}"
            );
            assert_eq!(
                w.get_voxel(x, G + 2, z),
                M.water,
                "the row below the cut stays at {x}, {z}"
            );
        }
    }
    assert!(
        drained.iter().filter(|c| c.material == M.air).count() >= 30,
        "the client was sent the drained cells ({} changes)",
        drained.len()
    );
    let lake = f
        .server
        .water()
        .basin_at([0, G, 5])
        .expect("the lake is a basin");
    assert_eq!(lake.inflow, 0.0, "a closed lake has no inflow");
}
