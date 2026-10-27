//! Felling through the server (docs/FELLING.md): cutting a trunk sends the cleared cells, the
//! piece's spawn and its poses; at rest it settles into blocks lying on the ground.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::SessionId;
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

/// The flat world's first air row.
const G: i32 = FLAT_TOP_Y;

fn log() -> u16 {
    dwell_blocks::parse_state("dwell:log[axis=y]").expect("log")
}
fn is_log(m: u16) -> bool {
    dwell_blocks::info(m).name.starts_with("dwell:log")
}

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

#[test]
fn a_cut_trunk_falls_tips_and_lies_on_the_ground_as_logs() {
    // face-a-only: the flat test world is face A's; the support rule has no up (tests in
    // dwell-world), and pieces fall by the physics' face gravity, tested there.
    fell_and_settle(false);
}

#[test]
fn a_piece_far_from_every_player_still_lands_on_the_ground() {
    // face-a-only: as above. The terrain collides only round bodies that need it: a trunk falling
    // with nobody near must still meet the ground, not fall through the world.
    fell_and_settle(true);
}

fn fell_and_settle(walk_away: bool) {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // A trunk 14 rows (7 m) tall at (1, 1), a few leaves round its top; leaning a little
    // (its top a cell over) so it tips one way.
    let (x, z) = (1, 1);
    {
        let w = f.server.world();
        for y in G..G + 14 {
            w.set_voxel(x, y, z, log());
        }
        w.set_voxel(x + 1, G + 13, z, log());
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            w.set_voxel(
                x + dx,
                G + 12,
                z + dz,
                dwell_blocks::require("dwell:leaves"),
            );
        }
    }
    // Player 1 (at (−1, ·, −0.25)) breaks the trunk one row above the ground, on its −x face.
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [x, G + 1, z],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    let out = f.take_all();
    let spawns: Vec<&EntitySpawn> = out[&1]
        .iter()
        .filter_map(|m| match m {
            Message::EntitySpawn(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(spawns.len(), 1, "one piece falls");
    assert_eq!(
        spawns[0].carriers, 13,
        "the trunk over the cut and its branch"
    );
    let collapse = out[&1].iter().any(|m| {
        matches!(m, Message::VoxelModification(v) if v.reason == VoxelModificationReason::Collapse)
    });
    assert!(collapse, "its cells cleared");
    assert_eq!(f.server.world().get_voxel(x, G + 5, z), M.air);
    assert!(
        is_log(f.server.world().get_voxel(x, G, z)),
        "the stump stands"
    );
    if walk_away {
        f.move_to(1, -200.0, f64::from(G) * dwell_world::CELL_H, -200.0);
    }
    // It falls and is seen falling; within 15 s it is at rest and settled.
    let mut posed = false;
    let mut despawned = false;
    for _ in 0..(60 * 15) {
        f.server.step();
        // Datagrams too (the snapshots): the whole outbox.
        for o in f
            .server
            .take_outbox()
            .into_iter()
            .filter(|o| o.session == 1)
        {
            match Message::decode(&o.bytes) {
                Some(Message::PhysicsSnapshot(s)) if !s.pieces.is_empty() => posed = true,
                Some(Message::EntityDespawn(id)) if id == spawns[0].id => despawned = true,
                _ => {}
            }
        }
        if despawned {
            break;
        }
    }
    assert!(posed, "snapshots carried its pose");
    assert!(despawned, "it settled");
    // It lies on the ground: logs along x or z on the first rows over the floor, none upright
    // over the stump's height, and about as many as fell (½ m rows lying as 1 m cells: half).
    let w = f.server.world();
    let mut lying = 0;
    let mut high = 0;
    for zz in z - 10..=z + 10 {
        for xx in x - 10..=x + 10 {
            for y in G..G + 14 {
                let m = w.get_voxel(xx, y, zz);
                if !is_log(m) || (xx, zz) == (x, z) && y == G {
                    continue;
                }
                if y > G + 3 {
                    high += 1;
                }
                if dwell_blocks::info(m).axis != dwell_blocks::Axis::Y {
                    lying += 1;
                }
            }
        }
    }
    assert_eq!(high, 0, "nothing left standing in the air");
    assert!(lying >= 5, "the trunk lies along x or z: {lying} cells");
}
