//! Region edits through the server (ARCHITECTURE.md §6.5, structure sandbox S3): validation (box
//! size, the cell rate, reach, permission, unbreakable blocks), broadcast as ordinary
//! VoxelModifications with per-chunk revisions, persistence's dirty chunks, and an undo (the
//! box's earlier states pasted back) restoring the world.

mod common;

use common::*;
use dwell_blocks::STATES;
use dwell_protocol::*;
use dwell_sim::server::{EditPolicy, ServerConfig, SessionId};
use dwell_world::region::{RegionCheck, read_box};
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

/// The flat world's surface: its top voxel is G − 1.
const G: i32 = FLAT_TOP_Y;

fn join_streaming(f: &mut Fixture, id: SessionId) -> Welcome {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
    w
}

fn region(op: RegionEditOp, min: [i32; 3], size: [u8; 3], material: u16) -> RegionEdit {
    RegionEdit {
        op,
        min,
        size,
        material,
        from: 0,
        cells: Vec::new(),
    }
}

fn fill(min: [i32; 3], size: [u8; 3], material: u16) -> Message {
    Message::RegionEdit(region(RegionEditOp::Fill, min, size, material))
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
fn a_fill_of_tall_grass_keeps_only_the_plants_that_stand_on_grass() {
    // Tall grass needs support (block data `placeOn`, `needsSupport`): a 3 × 3 × 3 fill on the
    // flat world's grass keeps the bottom layer; the layers above (standing on tall grass) and the
    // cells over a stone block go, in the same modification. Clearing the ground takes the rest.
    let tall = dwell_blocks::require("dwell:tall_grass");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    f.send(1, fill([-2, G - 1, 3], [1, 1, 1], M.stone));
    f.server.step();
    for _ in 0..30 {
        f.server.step();
    }
    f.send(1, fill([-2, G, 2], [3, 3, 3], tall));
    f.server.step();
    for x in -2..1 {
        for z in 2..5 {
            let want = if [x, z] == [-2, 3] { M.air } else { tall };
            assert_eq!(f.server.world().get_voxel(x, G, z), want, "{x} 0 {z}");
            for y in 1..3 {
                assert_eq!(
                    f.server.world().get_voxel(x, y + G, z),
                    M.air,
                    "{x} {y} {z}"
                );
            }
        }
    }
    for _ in 0..30 {
        f.server.step();
    }
    f.send(1, fill([-2, G - 1, 2], [3, 1, 3], M.air));
    f.server.step();
    for x in -2..1 {
        for z in 2..5 {
            assert_eq!(f.server.world().get_voxel(x, G, z), M.air, "{x} 0 {z}");
        }
    }
}

#[test]
fn a_fill_of_ladders_keeps_those_hung_on_a_wall() {
    // A ladder hangs on the block behind it (block data `needsSupport: behind`), not on what is
    // under it: a column filled against a stone wall stays (drag building's ladders, docs/ITEMS.md
    // §5); one with no wall behind goes. Red before: the region pass asked every cell to stand
    // on the one under it, and a ladder doesn't stand on a ladder.
    let north = (0..STATES.len() as u16)
        .find(|&id| {
            let s = dwell_blocks::info(id);
            s.look == "ladder" && s.placeable && s.facing == dwell_blocks::Facing::North
        })
        .expect("a ladder facing north");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // The wall: stone at z = 3, four cells up; a north-facing ladder at z = 2 hangs on it.
    f.send(1, fill([0, G, 3], [1, 4, 1], M.stone));
    f.server.step();
    for _ in 0..30 {
        f.server.step();
    }
    f.send(1, fill([0, G + 1, 2], [1, 3, 1], north));
    f.server.step();
    for y in 1..4 {
        assert_eq!(
            f.server.world().get_voxel(0, G + y, 2),
            north,
            "hung, row {y}"
        );
    }
    for _ in 0..30 {
        f.server.step();
    }
    // No wall behind: none kept.
    f.send(1, fill([4, G + 1, 2], [1, 3, 1], north));
    f.server.step();
    for y in 1..4 {
        assert_eq!(
            f.server.world().get_voxel(4, G + y, 2),
            M.air,
            "no wall, row {y}"
        );
    }
}

#[test]
fn the_welcome_announces_region_edits() {
    let mut f = Fixture::new(flat());
    let w = join_streaming(&mut f, 1);
    assert_ne!(w.flags & welcome_flags::REGION_EDITS, 0);
}

#[test]
fn a_fill_reaches_every_client_as_one_modification_and_marks_its_chunks_dirty() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    join_streaming(&mut f, 2);
    f.server.step();
    f.take_all();
    // A 4 × 2 × 3 slab ahead of player 1 (at about (−1, G, −0.25)), across chunks x −1 and 0.
    f.send(1, fill([-2, G, 2], [4, 2, 3], M.stone));
    f.server.step();
    for x in -2..2 {
        for y in G..G + 2 {
            for z in 2..5 {
                assert_eq!(f.server.world().get_voxel(x, y, z), M.stone, "{x} {y} {z}");
            }
        }
    }
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(
        (stats.region_edits_applied, stats.region_cells_changed),
        (1, 24)
    );
    let out = f.take_all();
    for id in [1, 2] {
        let mods = modifications(&out[&id]);
        assert_eq!(mods.len(), 1, "session {id}");
        assert_eq!(mods[0].chunks.len(), 2);
        assert!(mods[0].chunks.iter().all(|c| c.revision == 1));
        let cells: usize = mods[0].chunks.iter().map(|c| c.changes.len()).sum();
        assert_eq!(cells, 24);
    }
    assert_eq!(f.server.save_stats().dirty, 2);
}

#[test]
fn an_undo_pastes_the_box_back_as_it_was() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    let (min, size) = ([-3, G - 2, 1], [5u8, 4, 4]);
    let dims = size.map(i32::from);
    let before = read_box(f.server.world(), min, dims).expect("a generated world reads");
    // Hollow out a box that reaches into the ground, then paste the earlier states back.
    f.send(
        1,
        Message::RegionEdit(region(RegionEditOp::Hollow, min, size, M.sand)),
    );
    f.server.step();
    let after = read_box(f.server.world(), min, dims).expect("reads");
    assert_ne!(after, before);
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            cells: before.clone(),
            ..region(RegionEditOp::Paste, min, size, 0)
        }),
    );
    f.server.step();
    assert_eq!(read_box(f.server.world(), min, dims), Some(before));
    assert_eq!(f.server.stats_of(1).unwrap().region_edits_applied, 2);
}

#[test]
fn an_oversized_box_is_a_malformed_message() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let side = region_edit::MAX_SIDE as u8 + 1;
    f.send(1, fill([0, G, 2], [side, 1, 1], M.stone));
    let out = f.take_all().remove(&1).unwrap_or_default();
    assert!(
        out.iter()
            .any(|m| matches!(m, Message::Reject(r) if r.reason == RejectReason::Malformed)),
        "{out:?}"
    );
    assert_eq!(f.server.world().get_voxel(0, G, 2), M.air);
}

#[test]
fn rate_limited_by_cells_a_burst_then_cells_per_second() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    let side = region_edit::MAX_SIDE as u8;
    let full = region_edit::MAX_SIDE.pow(3);
    let burst = region_edit::BURST_CELLS / full;
    assert!((1..=region_edit::MAX_QUEUED).contains(&burst));
    // Clearing the air above the ground: valid and cheap to apply, but each costs its volume.
    for _ in 0..burst {
        f.send(1, fill([-16, 1, -16], [side; 3], M.air));
    }
    f.server.step();
    assert_eq!(
        f.server.stats_of(1).unwrap().region_edits_applied,
        burst as u32
    );
    f.send(1, fill([-16, 1, -16], [side; 3], M.air));
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().region_edits_rejected, 1);
    // One box's worth of cells later, one more.
    let ticks = (full * SIM_HZ as usize).div_ceil(region_edit::CELLS_PER_SECOND);
    for _ in 0..ticks {
        f.server.step();
    }
    f.send(1, fill([-16, 1, -16], [side; 3], M.air));
    f.server.step();
    assert_eq!(
        f.server.stats_of(1).unwrap().region_edits_applied,
        burst as u32 + 1
    );
}

#[test]
fn more_than_max_queued_in_a_tick_are_dropped() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    for x in 0..region_edit::MAX_QUEUED as i32 + 2 {
        f.send(1, fill([x, G, 2], [1, 1, 1], M.dirt));
    }
    f.server.step();
    let s = f.server.stats_of(1).unwrap();
    assert_eq!(s.region_edits_applied, region_edit::MAX_QUEUED as u32);
    assert_eq!(s.region_edits_rejected, 2);
}

#[test]
fn out_of_reach_or_not_permitted_changes_nothing() {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.send(1, fill([300, G, 0], [2, 2, 2], M.stone)); // past creative's 256 m
    f.server.step();
    let s = f.server.stats_of(1).unwrap();
    assert_eq!(s.last_region_check, RegionCheck::OutOfReach);
    assert_eq!(s.region_edits_rejected, 1);
    assert_eq!(f.server.world().get_voxel(300, G, 0), M.air);

    let mut f = Fixture::new(ServerConfig {
        edits: EditPolicy::Nobody,
        ..flat()
    });
    join_streaming(&mut f, 1);
    f.server.step();
    f.send(1, fill([0, G, 2], [2, 2, 2], M.stone));
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().region_edits_applied, 0);
    assert_eq!(f.server.world().get_voxel(0, G, 2), M.air);
}

/// The registry's unbreakable (`indestructible`) blocks survive a fill and a paste. Covered with a
/// stand-in by `dwell_world::region`'s tests while the registry has none.
#[test]
fn unbreakable_blocks_are_left_as_they_are() {
    let Some(locked) = (0..STATES.len()).find(|&i| STATES[i].indestructible) else {
        eprintln!("no indestructible block in the registry: covered by dwell-world's region tests");
        return;
    };
    let locked = locked as u16;
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.server.world().set_voxel(0, G, 2, locked);
    f.send(1, fill([-1, G, 2], [3, 1, 1], M.stone));
    f.server.step();
    assert_eq!(f.server.world().get_voxel(0, G, 2), locked);
    assert_eq!(f.server.world().get_voxel(1, G, 2), M.stone);
}

/// The structure sandbox (generator 100): the world type `shared/brushes/brushes.json` lets brush.
fn sandbox() -> ServerConfig {
    ServerConfig {
        generator_version: dwell_world::GENERATOR_SANDBOX,
        ..flat()
    }
}

fn join_sandbox(f: &mut Fixture, id: SessionId) -> Welcome {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    let generate = dwell_worldgen::generator_for(dwell_world::GENERATOR_SANDBOX, w.world_seed);
    generate(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
    w
}

#[test]
fn every_world_announces_brushes() {
    // Owner, 2026-11-06: brushes in every world, not just the sandbox (every world is creative).
    let mut f = Fixture::new(flat());
    let w = join_streaming(&mut f, 1);
    assert_ne!(w.flags & welcome_flags::BRUSHES, 0);
    assert_ne!(w.flags & welcome_flags::REGION_EDITS, 0);
    let mut f = Fixture::new(sandbox());
    let w = join_sandbox(&mut f, 1);
    assert_ne!(w.flags & welcome_flags::BRUSHES, 0);
}

#[test]
fn brush_strokes_reach_256_m_in_every_world() {
    // 200 m from the spawn: beyond the old region reach (64 m) and the sandbox's old 128 m, within
    // creative's 256 m (owner, 2026-11-06).
    for sandboxed in [false, true] {
        let mut f = Fixture::new(if sandboxed { sandbox() } else { flat() });
        if sandboxed {
            join_sandbox(&mut f, 1);
        } else {
            join_streaming(&mut f, 1);
        }
        f.server.step();
        f.send(1, fill([200, G + 3, 0], [2, 2, 2], M.stone));
        f.server.step();
        let stats = f.server.stats_of(1).unwrap();
        assert_eq!(
            stats.last_region_check,
            RegionCheck::Ok,
            "sandbox {sandboxed}"
        );
        assert_eq!(f.server.world().get_voxel(200, G + 3, 0), M.stone);
        // Still bounded: 300 m is out of reach.
        f.send(1, fill([300, G + 3, 0], [1, 1, 1], M.stone));
        f.server.step();
        let stats = f.server.stats_of(1).unwrap();
        assert_eq!(stats.last_region_check, RegionCheck::OutOfReach);
        assert_eq!(f.server.world().get_voxel(300, G + 3, 0), M.air);
    }
}

/// Seed 0's generated terrain at the bench's dam site (`?at=2364.5,142,7443.5`), as the browser's
/// local server runs it.
fn terrain() -> ServerConfig {
    ServerConfig {
        world_seed: 0,
        generator_version: dwell_world::GENERATOR_TERRAIN,
        spawn: Some([2364.5, 142.0, 7443.5]),
        worldgen_threads: 0,
        lod_threads: Some(0),
        pregen_radius_chunks: 1,
        ..ServerConfig::default()
    }
}

#[test]
fn a_creative_brush_stroke_lands_200_m_off_in_a_terrain_world_and_undoes() {
    // face-a-only: the server's reach and KEEP cells don't depend on the face; the dam site is A's.
    // Before brushes came to every world, a terrain world announced none and refused a region
    // edit past 65 m.
    let mut f = Fixture::new(terrain());
    let w = f.join(1, &Client::new(1));
    assert_ne!(
        w.flags & welcome_flags::BRUSHES,
        0,
        "terrain worlds take brushes"
    );
    for _ in 0..10 {
        f.server.step();
        f.take_all();
    }
    // A brush paste (KEEP around one stone cell) 200 m east, 10 m over the spawn: air there.
    let (x, y, z) = (2364 + 200, 284 + 20, 7443);
    let before: Vec<u16> = (0..3)
        .map(|i| f.server.world().get_voxel(x + i, y, z))
        .collect();
    let keep = region_edit::KEEP as u16;
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            cells: vec![keep, M.stone, keep],
            ..region(RegionEditOp::Paste, [x, y, z], [3, 1, 1], 0)
        }),
    );
    f.server.step();
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(stats.last_region_check, RegionCheck::Ok);
    assert_eq!(f.server.world().get_voxel(x + 1, y, z), M.stone);
    assert_eq!(f.server.world().get_voxel(x, y, z), before[0]);
    // The undo: the earlier state pasted back over the same box.
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            cells: vec![keep, before[1], keep],
            ..region(RegionEditOp::Paste, [x, y, z], [3, 1, 1], 0)
        }),
    );
    f.server.step();
    assert_eq!(f.server.world().get_voxel(x + 1, y, z), before[1]);
}

#[test]
fn a_brush_paste_keeps_its_keep_cells_and_the_unbreakable_floor() {
    let mut f = Fixture::new(sandbox());
    join_sandbox(&mut f, 1);
    f.server.step();
    let floor = f.server.world().get_voxel(4, G - 1, 4);
    assert!(
        STATES[usize::from(floor)].indestructible,
        "the sandbox floor"
    );
    f.server.world().set_voxel(5, G, 4, M.sand);
    // A 3 × 2 × 1 paste over the floor: stone on the cell above it, keep everywhere else.
    let keep = region_edit::KEEP as u16;
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            cells: vec![keep, keep, keep, M.stone, keep, keep],
            ..region(RegionEditOp::Paste, [4, G - 1, 4], [3, 2, 1], 0)
        }),
    );
    f.server.step();
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(
        (stats.region_edits_applied, stats.region_cells_changed),
        (1, 1)
    );
    let w = f.server.world();
    assert_eq!(w.get_voxel(4, G, 4), M.stone);
    assert_eq!(w.get_voxel(5, G, 4), M.sand);
    assert_eq!(w.get_voxel(4, G - 1, 4), floor);
}

#[test]
fn every_edit_is_acknowledged_after_its_changes_with_the_refused_ones_named() {
    // Edit prediction (§6.5): block and region edits are numbered together in arrival order; after
    // the tick's VoxelModification, an EditAck says how far the server got and which it refused.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let block = |cell: [i32; 3]| {
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell,
            face: 2,
            material: 0,
            rows: 1,
            down: false,
            point: None,
        })
    };
    f.send(1, fill([3, G, 3], [2, 1, 1], M.stone)); // 1: applied
    f.send(1, block([200, G - 1, 0])); // 2: out of reach, refused
    f.send(1, fill([300, G, 3], [2, 1, 1], M.stone)); // 3: out of reach, refused
    f.server.step();
    let got = f.take_all().remove(&1).unwrap_or_default();
    let ack_at = got
        .iter()
        .position(|m| matches!(m, Message::EditAck(_)))
        .expect("an ack");
    let modified_at = got
        .iter()
        .position(|m| matches!(m, Message::VoxelModification(_)))
        .expect("the fill's modification");
    assert!(modified_at < ack_at, "the changes come first");
    let Message::EditAck(ack) = &got[ack_at] else {
        unreachable!()
    };
    assert_eq!(ack.through, 3);
    assert_eq!(ack.rejected, vec![2, 3]);
    // A tick with no edits sends no ack; a refused edit alone still gets one.
    f.server.step();
    assert!(
        !f.take_all()
            .remove(&1)
            .unwrap_or_default()
            .iter()
            .any(|m| matches!(m, Message::EditAck(_)))
    );
    f.send(1, block([200, G - 1, 0]));
    f.server.step();
    let got = f.take_all().remove(&1).unwrap_or_default();
    assert!(got.contains(&Message::EditAck(EditAck {
        through: 4,
        rejected: vec![4],
    })));
}

#[test]
fn the_edit_log_records_each_block_change_with_what_it_was_and_became() {
    // Owner, 2026-11-04: a "block edits" log in the block dump, so a reported world can be
    // replayed. A fill of two stone cells is logged per cell (tick, cell, from, to, reason);
    // the water's own changes are not.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    assert_eq!(f.server.edit_log_json(), "[]");
    f.send(1, fill([0, G, 2], [2, 1, 1], M.stone));
    f.server.step();
    let log: serde_json::Value = serde_json::from_str(&f.server.edit_log_json()).unwrap();
    let rows = log.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{log}");
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r["cell"], serde_json::json!([i as i32, G, 2]));
        assert_eq!(r["from"], "dwell:air");
        assert!(r["to"].as_str().unwrap().starts_with("dwell:stone"), "{r}");
        assert!(r["tick"].as_u64().is_some() && r["reason"].is_string());
    }
}
