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
fn the_waters_modifications_reach_a_client_in_revision_order() {
    // face-a-only: the server's flat world is face A's; the basins' both faces are tested in
    // `water::tests`.
    // Owner, 2026-11-04: rising water "disappears and then reappears at a slightly higher level".
    // The water wrote its cells with `set_voxel`, a revision per cell, then sent them as one
    // modification: every water modification skipped revisions, so the client asked for the whole
    // chunk again and drew it dark until relit. Each chunk's modifications now follow one another.
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
    let mut seen: std::collections::HashMap<[i32; 3], u32> = std::collections::HashMap::new();
    let w = f.server.world();
    for x in -1..=0 {
        for y in -1..=1 {
            for z in -1..=0 {
                if let Some(c) = w.find(ChunkCoord::new(x, y, z)) {
                    seen.insert([x, y, z], c.revision());
                }
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
    let mut water = 0;
    for _ in 0..240 {
        f.server.step();
        for (_, out) in f.take_all() {
            for m in out {
                let Message::VoxelModification(v) = m else {
                    continue;
                };
                water += usize::from(v.reason == VoxelModificationReason::Rebake);
                for c in &v.chunks {
                    if let Some(last) = seen.get(&c.coord) {
                        assert_eq!(
                            c.revision,
                            last + 1,
                            "chunk {:?}: revision {} after {last} ({:?})",
                            c.coord,
                            c.revision,
                            v.reason
                        );
                    }
                    seen.insert(c.coord, c.revision);
                }
            }
        }
    }
    assert!(water > 0, "the water sent its changes");
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
/// it; 2026-11-04 (ADR 0057): generated water must rest by the one law, as any water does. In
/// generated worlds (seeds 0–2), a stone set in the bed of river pools picked along the world
/// map's rivers changes no other cell within 48 m over six hours of water time, and sends no change
/// down its pool graph (no pool nobody measured off its rest, ADR 0059): each pool's lip carries its river's flow with the pool a full row over its
/// sill (WATER_FLOW.md §17); a ramp at the water's edge may take its flooded state.
#[test]
fn a_block_set_in_a_generated_rivers_bed_changes_nothing_else() {
    // face-a-only: the generated world's face A; the basins' both faces are tested in
    // `water::tests`.
    use dwell_world::{Face, GENERATOR_TERRAIN, VoxelWorld};
    const R: i32 = 48;
    let liquid = |m: u16| dwell_blocks::try_info(m).is_some_and(|i| i.liquid);
    let mut checked = 0;
    for seed in [0u64, 1, 2] {
        let runoff = dwell_worldgen::runoff_for(GENERATOR_TERRAIN, seed).expect("a world map");
        let rivers = std::sync::Arc::new(
            dwell_worldgen::BifacialTerrain::for_generator(GENERATOR_TERRAIN, seed)
                .expect("terrain"),
        );
        let map = rivers.local(Face::A).map().clone();
        // Vertices along the map's rivers, inland, of small and medium flows.
        let mut picks: Vec<(i32, i32)> = Vec::new();
        for r in &map.rivers {
            for (i, v) in r.path.iter().enumerate() {
                if i % 40 == 20 && v.surface > 3.0 && (0.3..30.0).contains(&v.discharge) {
                    picks.push((v.x as i32, v.z as i32));
                }
            }
        }
        let mut sites = 0;
        for (px, pz) in picks {
            if sites == 4 {
                break;
            }
            let mut w = VoxelWorld::new(
                dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
                Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
            );
            // A column of the river two or more cells deep near the vertex: its bed cell.
            let mut bed = None;
            'find: for dx in -6..=6 {
                for dz in -6..=6 {
                    let (x, z) = (px + dx, pz + dz);
                    let Some(top) = (0..700).rev().find(|&y| {
                        let m = w.get_voxel(x, y, z);
                        m != dwell_world::M.air
                            && !dwell_blocks::try_info(m).is_some_and(|i| i.replaceable)
                    }) else {
                        continue;
                    };
                    if !liquid(w.get_voxel(x, top, z)) {
                        continue;
                    }
                    let mut b = top;
                    while liquid(w.get_voxel(x, b - 1, z)) {
                        b -= 1;
                    }
                    if top - b >= 1 {
                        bed = Some([x, b, z]);
                        break 'find;
                    }
                }
            }
            let Some(bed) = bed else {
                continue;
            };
            sites += 1;
            let (lo, hi) = (bed[1] - 24, bed[1] + 24);
            // Loaded well past what is checked: a river running on out of the loaded world
            // seeps away there, not a part of this check.
            const LOAD: i32 = R + 80;
            for x in (bed[0] - LOAD..=bed[0] + LOAD).step_by(8) {
                for z in (bed[2] - LOAD..=bed[2] + LOAD).step_by(8) {
                    for y in (lo - 16..=hi + 16).step_by(8) {
                        w.get_voxel(x, y, z);
                    }
                }
            }
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
            let was = w.get_voxel(bed[0], bed[1], bed[2]);
            w.set_voxel(bed[0], bed[1], bed[2], stone);
            let before = snapshot(&w);
            let runoff = runoff.clone();
            // With the map's rivers too, as the server's: a river at rest sends nothing down them.
            let mut water = dwell_sim::water::Water::with_runoff(std::sync::Arc::new(
                move |f: dwell_world::FaceFrame, x, z| runoff(f.face, x, z),
            ))
            .with_rivers(rivers.clone());
            water.touch([(bed, was)]);
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
            assert_eq!(
                water.off_rest(),
                0,
                "seed {seed}, bed {bed:?}: a river at rest sent changes down its pools"
            );
            // A ramp at the water's edge may take its flooded state: nothing else.
            let flooded =
                |a: u16, b: u16| dwell_blocks::with_property(a, "flooded", "true") == Some(b);
            let changed = before
                .iter()
                .zip(&after)
                .filter(|&(&a, &b)| a != b && !flooded(a, b))
                .count();
            assert_eq!(
                changed, 0,
                "seed {seed}: {changed} cells changed near {bed:?}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 5, "river pools checked: {checked}");
}
