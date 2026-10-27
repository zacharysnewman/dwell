//! The bedrock between the faces is unbreakable (docs/BIFACIAL_WORLD.md §2, `shared/worldgen/
//! midplane.json`; owner 2026-10-21): in a portal's shaft, where a player can reach it, the server
//! refuses a break of the shaft wall's bedrock and a region edit leaves it standing. The client
//! predicts an edit by the server's own check run on its world (`dwell-client` `game.rs`
//! `predicted_changes`: `check_block_edit`), so the same check is asked from the player's eye here:
//! it refuses too, and nothing is predicted.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_world::edit::{DEFAULT_REACH, EditCheck, check_block_edit};
use dwell_world::{Chunk, ChunkCoord, Face, GENERATOR_TERRAIN, M, chunk_hash, m_of_row};
use dwell_worldgen::BifacialTerrain;

const SEED: u64 = 0;

fn terrain() -> ServerConfig {
    ServerConfig {
        generator_version: GENERATOR_TERRAIN,
        world_seed: SEED,
        ..ServerConfig::default()
    }
}

fn join_terrain(f: &mut Fixture, id: SessionId) -> Welcome {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    dwell_worldgen::generator_for(GENERATOR_TERRAIN, w.world_seed)(
        ChunkCoord::from_array(w.verification_chunk),
        &mut v,
    );
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
    w
}

#[test]
fn the_server_refuses_to_break_the_bedrock_and_a_region_edit_leaves_it() {
    // face-a-only: the player stands on face A's side of the midplane in the shaft; the bedrock is
    // one block (`indestructible`) on both faces, checked by the same code.
    let bedrock = dwell_blocks::require(dwell_worldgen::portals::MIDPLANE.bedrock);
    let g = BifacialTerrain::new(SEED);
    let portal = g.portal_sites()[0];
    let mut f = Fixture::new(terrain());
    let w = join_terrain(&mut f, 1);
    f.server.step();
    // In the shaft at the midplane, the wall's bedrock 3 columns west (the rim's column) at row 1.
    let target = [portal.x - 3, 1, portal.z];
    assert_eq!(
        f.server.world().get_voxel(target[0], target[1], target[2]),
        bedrock
    );
    assert_eq!(
        f.server.world().get_voxel(portal.x - 2, 1, portal.z),
        M.air,
        "the shaft is open"
    );
    let feet = m_of_row(1);
    let eye = [
        f64::from(portal.x) + 0.5,
        feet + 1.62,
        f64::from(portal.z) + 0.5,
    ];
    let request = BlockEditRequest {
        action: BlockEditAction::Break,
        cell: target,
        face: 0, // its +x face, toward the shaft
        material: 0,
        rows: 1,
        down: false,
    };
    // The client's prediction: the server's check from the eye, refused.
    let out = check_block_edit(f.server.world(), &request, eye, &[], DEFAULT_REACH);
    assert_eq!(out.check, EditCheck::Unbreakable);
    // The server: refused, and the bedrock stays.
    f.move_to(w.player_id, eye[0], feet, eye[2]);
    f.send(1, Message::BlockEditRequest(request));
    f.server.step();
    assert_eq!(
        f.server.stats_of(1).unwrap().last_edit_check,
        EditCheck::Unbreakable
    );
    assert_eq!(
        f.server.world().get_voxel(target[0], target[1], target[2]),
        bedrock
    );
    // A region clear over the wall: the bedrock is left as it is.
    f.move_to(w.player_id, eye[0], feet, eye[2]);
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Fill,
            min: [portal.x - 4, 0, portal.z - 1],
            size: [2, 2, 3],
            material: M.air,
            from: 0,
            cells: Vec::new(),
        }),
    );
    for _ in 0..4 {
        f.server.step();
    }
    for z in portal.z - 1..=portal.z + 1 {
        for x in portal.x - 4..=portal.x - 3 {
            for y in 0..2 {
                assert_eq!(f.server.world().get_voxel(x, y, z), bedrock, "{x} {y} {z}");
            }
        }
    }
    // Face B's bedrock below it is the same block.
    let below = g.local(Face::B).ground_y(portal.x - 3, portal.z);
    assert!(below.is_some());
    assert_eq!(
        f.server.world().get_voxel(target[0], -1, target[2]),
        bedrock
    );
}
