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
    f.send(1, fill([-2, 31, 3], [1, 1, 1], M.stone));
    f.server.step();
    for _ in 0..30 {
        f.server.step();
    }
    f.send(1, fill([-2, 32, 2], [3, 3, 3], tall));
    f.server.step();
    for x in -2..1 {
        for z in 2..5 {
            let want = if [x, z] == [-2, 3] { M.air } else { tall };
            assert_eq!(f.server.world().get_voxel(x, 32, z), want, "{x} 0 {z}");
            for y in 1..3 {
                assert_eq!(
                    f.server.world().get_voxel(x, y + 32, z),
                    M.air,
                    "{x} {y} {z}"
                );
            }
        }
    }
    for _ in 0..30 {
        f.server.step();
    }
    f.send(1, fill([-2, 31, 2], [3, 1, 3], M.air));
    f.server.step();
    for x in -2..1 {
        for z in 2..5 {
            assert_eq!(f.server.world().get_voxel(x, 32, z), M.air, "{x} 0 {z}");
        }
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
    f.send(1, fill([200, G, 0], [2, 2, 2], M.stone));
    f.server.step();
    let s = f.server.stats_of(1).unwrap();
    assert_eq!(s.last_region_check, RegionCheck::OutOfReach);
    assert_eq!(s.region_edits_rejected, 1);
    assert_eq!(f.server.world().get_voxel(200, G, 0), M.air);

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
fn only_a_brush_world_announces_brushes() {
    let mut f = Fixture::new(flat());
    assert_eq!(join_streaming(&mut f, 1).flags & welcome_flags::BRUSHES, 0);
    let mut f = Fixture::new(sandbox());
    let w = join_sandbox(&mut f, 1);
    assert_ne!(w.flags & welcome_flags::BRUSHES, 0);
    assert_ne!(w.flags & welcome_flags::REGION_EDITS, 0);
}

#[test]
fn brush_strokes_reach_far_in_the_sandbox_only() {
    // 100 m from the spawn: beyond the region reach (64 m), within the sandbox's brush reach.
    let far = fill([100, 35, 0], [2, 2, 2], M.stone);
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.send(1, far.clone());
    f.server.step();
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(stats.last_region_check, RegionCheck::OutOfReach);
    assert_eq!(f.server.world().get_voxel(100, 35, 0), M.air);

    let mut f = Fixture::new(sandbox());
    join_sandbox(&mut f, 1);
    f.server.step();
    f.send(1, far);
    f.server.step();
    assert_eq!(f.server.stats_of(1).unwrap().region_edits_applied, 1);
    assert_eq!(f.server.world().get_voxel(100, 35, 0), M.stone);
    // Still bounded: 200 m is out of reach.
    f.send(1, fill([200, 35, 0], [1, 1, 1], M.stone));
    f.server.step();
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(stats.last_region_check, RegionCheck::OutOfReach);
}

#[test]
fn a_brush_paste_keeps_its_keep_cells_and_the_unbreakable_floor() {
    let mut f = Fixture::new(sandbox());
    join_sandbox(&mut f, 1);
    f.server.step();
    let floor = f.server.world().get_voxel(4, 31, 4);
    assert!(
        STATES[usize::from(floor)].indestructible,
        "the sandbox floor"
    );
    f.server.world().set_voxel(5, 32, 4, M.sand);
    // A 3 × 2 × 1 paste over the floor: stone on the cell above it, keep everywhere else.
    let keep = region_edit::KEEP as u16;
    f.send(
        1,
        Message::RegionEdit(RegionEdit {
            cells: vec![keep, keep, keep, M.stone, keep, keep],
            ..region(RegionEditOp::Paste, [4, 31, 4], [3, 2, 1], 0)
        }),
    );
    f.server.step();
    let stats = f.server.stats_of(1).unwrap();
    assert_eq!(
        (stats.region_edits_applied, stats.region_cells_changed),
        (1, 1)
    );
    let w = f.server.world();
    assert_eq!(w.get_voxel(4, 32, 4), M.stone);
    assert_eq!(w.get_voxel(5, 32, 4), M.sand);
    assert_eq!(w.get_voxel(4, 31, 4), floor);
}
