//! Chunk requests (ARCHITECTURE.md §6.6): beyond the view, the chunks a client wants to draw at
//! full detail are streamed on request within the render radius — generated, air or explicit like
//! the view's own, with their edits — and unloaded once they leave that radius.

mod common;

use common::*;
use dwell_protocol::world::RENDER_RADIUS_CHUNKS;
use dwell_protocol::*;
use dwell_sim::server::SessionId;
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, generate_flat_chunk, local_index};

/// The chunk row holding the flat world's surface row (`FLAT_TOP_Y`): 2 on ½ m cells.
const GC: i32 = dwell_world::FLAT_TOP_Y / dwell_world::CHUNK_SIZE;

const RENDER_RADIUS: i32 = RENDER_RADIUS_CHUNKS as i32;
/// The flat world's surface: feet height on its ground.
const TOP: f64 = dwell_world::FLAT_TOP_M;

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(dwell_world::chunk_hash(&v)));
}

/// Streams the view (the player stands at the origin chunk of the flat world).
fn stream_view(f: &mut Fixture) {
    for _ in 0..20 {
        f.server.step();
    }
    f.take_all();
}

fn unloaded(unloads: &[ChunkUnload], c: ChunkCoordNet) -> bool {
    unloads.iter().any(|u| u.coords.contains(&c))
}

#[test]
fn chunks_beyond_the_view_arrive_once_after_the_view() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    stream_view(&mut f);
    let ground = ChunkCoord::new(8, GC - 1, 0); // 8 chunks out: beyond the view (3), within the render radius
    let sky = ChunkCoord::new(8, GC + 2, 0);
    const { assert!(RENDER_RADIUS >= 8) };
    f.send(
        1,
        Message::ChunkRequest(vec![[8, GC - 1, 0], [8, GC + 2, 0], [8, GC - 1, 0]]),
    );
    let chunks = f.chunks(1, 3, None);
    assert_eq!(chunks.len(), 2);
    assert_eq!(coord_of(&chunks[0]), ground);
    assert_eq!(chunks[0].form, ChunkForm::Generated);
    assert_eq!(coord_of(&chunks[1]), sky);
    assert_eq!(f.server.stream_stats_of(1).unwrap().requested_sent, 2);
    // Asking again for a chunk the client has sends nothing.
    f.send(1, Message::ChunkRequest(vec![[8, GC - 1, 0]]));
    assert!(f.chunks(1, 3, None).is_empty());
}

#[test]
fn beyond_the_render_radius_or_before_the_worldgen_check_are_dropped() {
    let mut f = Fixture::new(flat());
    f.join(1, &Client::new(1));
    f.send(1, Message::ChunkRequest(vec![[4, GC - 1, 0]])); // no check yet
    assert_eq!(f.server.stream_stats_of(1).unwrap().requests_dropped, 1);
    join_streaming(&mut f, 2);
    stream_view(&mut f);
    let far = RENDER_RADIUS + 2;
    f.send(2, Message::ChunkRequest(vec![[far, 0, 0], [0, 0, far]]));
    assert!(f.chunks(2, 3, None).is_empty());
    assert_eq!(f.server.stream_stats_of(2).unwrap().requests_dropped, 2);
}

#[test]
fn a_modified_chunk_arrives_explicitly_and_its_edits_follow() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    join_streaming(&mut f, 2);
    stream_view(&mut f);
    // Player 2 builds 6 chunks away from the origin, where player 1 does not stream by view.
    f.move_to(2, 6.0 * 32.0 + 0.5, TOP, 0.5);
    for _ in 0..20 {
        f.server.step();
    }
    f.take_all();
    f.send(
        2,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [6 * 32, FLAT_TOP_Y - 1, 2],
            face: 2,
            material: M.stone,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    assert_eq!(f.server.world().get_voxel(6 * 32, FLAT_TOP_Y, 2), M.stone);
    f.take_all();

    f.send(1, Message::ChunkRequest(vec![[6, GC, 0]]));
    let chunks = f.chunks(1, 2, None);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].form, ChunkForm::Explicit);
    assert_eq!(chunks[0].revision, 1);
    assert_eq!(chunks[0].voxels[local_index(0, 0, 2)], M.stone);

    // The next edit there reaches player 1 too.
    f.send(
        2,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [6 * 32 + 1, FLAT_TOP_Y - 1, 2],
            face: 2,
            material: M.sand,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    let mut edit_seen = false;
    for (m, _) in f.take(1) {
        if let Message::VoxelModification(m) = m {
            edit_seen |= m.chunks.iter().any(|c| c.coord == [6, GC, 0]);
        }
    }
    assert!(edit_seen);
}

#[test]
fn a_view_chunk_asked_for_stays_when_the_view_moves_on() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    stream_view(&mut f);
    // A ground chunk the view streamed (the player stands near the origin chunk).
    f.send(
        1,
        Message::ChunkRequest(vec![[1, GC - 1, 0], [-1, GC - 1, 0]]),
    );
    assert!(f.chunks(1, 2, None).is_empty()); // held already: nothing to send
    let mut unloads = Vec::new();
    f.move_to(1, 8.0 * 32.0 + 0.5, TOP, 0.5);
    f.chunks(1, 5, Some(&mut unloads));
    assert!(!unloaded(&unloads, [-1, GC - 1, 0])); // 9 chunks behind, within the render radius
    assert!(unloaded(&unloads, [-2, GC - 1, 0])); // its neighbour, never asked for, left with the view
}

#[test]
fn kept_while_within_the_render_radius_unloaded_beyond_it() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    stream_view(&mut f);
    f.send(1, Message::ChunkRequest(vec![[6, GC - 1, 0]]));
    assert_eq!(f.chunks(1, 2, None).len(), 1);
    // Three chunks the other way (the view moves on, the requested chunk stays)…
    let mut unloads = Vec::new();
    f.move_to(1, -3.0 * 32.0 + 0.5, TOP, 0.5);
    f.chunks(1, 5, Some(&mut unloads));
    assert!(!unloads.is_empty()); // the view moved: its far side left
    assert!(!unloaded(&unloads, [6, GC - 1, 0])); // 9 chunks from the view's center, far beyond view + margin
    // …then beyond the render radius and its margin.
    f.move_to(1, -f64::from(RENDER_RADIUS - 2) * 32.0 + 0.5, TOP, 0.5);
    f.chunks(1, 5, Some(&mut unloads));
    assert!(unloaded(&unloads, [6, GC - 1, 0]));
}
