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
    assert_eq!(lake.inflow(), 0.0, "a closed lake has no inflow");
}

#[test]
fn a_lake_cut_open_drains_on_after_its_player_leaves() {
    // face-a-only: the server's flat world is face A's; the basins' both faces are tested in
    // `water::tests`.
    // Owner, 2026-11-03: water an edit set going changes whether or not anyone is near (two
    // players far apart on one river). A lake cut open, its player leaving at once (no one near): the lake
    // drains on (its chunks kept loaded, not evicted) and settles. Red before: it waited as left.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
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
    for _ in 0..7 {
        f.server.step();
    }
    let lake_chunk = dwell_world::chunk_of(0, G, 5);
    f.server.on_disconnected(1);
    for _ in 0..1200 {
        f.server.step();
        f.take_all();
    }
    assert!(
        f.server.world().find(lake_chunk).is_some(),
        "the lake's chunk kept loaded"
    );
    let w = f.server.world();
    for x in -2..=2 {
        for z in 3..=8 {
            assert_eq!(w.get_voxel(x, G + 3, z), M.air, "drained at {x}, {z}");
            assert_eq!(
                w.get_voxel(x, G + 2, z),
                M.water,
                "below the cut at {x}, {z}"
            );
        }
    }
    let lake = f.server.water().basin_at([0, G, 5]).expect("the lake");
    assert!(!lake.active, "settled");
    assert!(
        f.server.water().wanted_chunks().is_empty(),
        "no chunks wanted once settled"
    );
}

/// Owner, 2026-11-03: a block set at the bottom of any river flooded it without end, or drained
/// it. In a generated world (seed 1), a stone set in the bed of two rivers' steps changes no other
/// cell within 48 m over six hours of water time: the bodies found are at rest as generated, their
/// natural outlets passing the world map's flow (docs/WATER_FLOW.md §2.2); a ramp at the water's
/// edge may take its flooded state. Red before: one river
/// rose without end (an inflow measured off its shore's gaps, 61 m³/s, and the pools below merged
/// into it), the other drained.
#[test]
fn a_block_set_in_a_generated_rivers_bed_changes_nothing_else() {
    // face-a-only: the generated world's face A; the basins' both faces are tested in
    // `water::tests`.
    use dwell_world::{GENERATOR_TERRAIN, VoxelWorld};
    const SEED: u64 = 1;
    const R: i32 = 48;
    let runoff = dwell_worldgen::runoff_for(GENERATOR_TERRAIN, SEED).expect("a world map");
    // Each a river's bed cell, its water the lowest of a step (found by a search of seed 1).
    for bed in [[-5432, 217, -4184], [-5860, 208, -4161]] {
        let mut w = VoxelWorld::new(
            dwell_worldgen::generator_for(GENERATOR_TERRAIN, SEED),
            Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, SEED)),
        );
        let (lo, hi) = (bed[1] - 24, bed[1] + 24);
        for x in (bed[0] - R..=bed[0] + R).step_by(8) {
            for z in (bed[2] - R..=bed[2] + R).step_by(8) {
                for y in (lo..=hi).step_by(8) {
                    w.get_voxel(x, y, z);
                }
            }
        }
        assert_eq!(
            w.get_voxel(bed[0], bed[1], bed[2]),
            M.water,
            "a river at {bed:?}"
        );
        assert_ne!(
            w.get_voxel(bed[0], bed[1] - 1, bed[2]),
            M.water,
            "its bed under {bed:?}"
        );
        let snapshot = |w: &VoxelWorld| -> Vec<u16> {
            let mut v = Vec::new();
            for x in bed[0] - R..=bed[0] + R {
                for z in bed[2] - R..=bed[2] + R {
                    for y in lo..=hi {
                        v.push(w.get_voxel_if_loaded([x, y, z]));
                    }
                }
            }
            v
        };
        let stone = dwell_blocks::require("dwell:stone");
        w.set_voxel(bed[0], bed[1], bed[2], stone);
        let before = snapshot(&w);
        let runoff = runoff.clone();
        let mut water = dwell_sim::water::Water::with_runoff(std::sync::Arc::new(
            move |f: dwell_world::FaceFrame, x, z| runoff(f.face, x, z),
        ));
        water.touch([(bed, M.water)]);
        let player = [
            f64::from(bed[0]) + 0.5,
            dwell_world::m_of_row(bed[1]) + 2.0,
            f64::from(bed[2]) + 0.5,
        ];
        // Six hours of water time at a 20-minute day (7.2 s a water tick).
        for _ in 0..3000 {
            water.tick(&mut w, &[player], 7.2);
        }
        let after = snapshot(&w);
        // A ramp at the water's edge may take its flooded state (the edge fix): nothing else.
        let flooded = |a: u16, b: u16| dwell_blocks::with_property(a, "flooded", "true") == Some(b);
        let changed = before
            .iter()
            .zip(&after)
            .filter(|&(&a, &b)| a != b && !flooded(a, b))
            .count();
        assert_eq!(changed, 0, "{changed} cells changed near {bed:?}");
    }
}
