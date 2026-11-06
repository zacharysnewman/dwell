//! The water on its own thread (Phase 53 G5, ADR 0062, docs/WATER_SURFACE_GRID.md §2d): the server
//! sends the water host what changed and applies what comes back a tick later; physics never waits
//! for the water.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_sim::water::host::{CoreConfig, Placement, WaterHost};
use dwell_storage::{WORLD_EXTENSION, WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

const G: i32 = FLAT_TOP_Y;

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

/// A walled lake four rows deep in front of the player (x −2–2, z 3–8), its near wall broken at
/// the top row: it drains over the flat ground.
fn cut_lake(f: &mut Fixture) {
    join_streaming(f, 1);
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
}

fn on(placement: Placement) -> ServerConfig {
    ServerConfig {
        water: placement,
        ..flat()
    }
}

/// The cells about the lake.
fn cells(f: &mut Fixture) -> Vec<u16> {
    let w = f.server.world();
    let mut out = Vec::new();
    for x in -8..=8 {
        for z in -6..=16 {
            for y in G - 1..G + 5 {
                out.push(w.get_voxel_if_loaded([x, y, z]));
            }
        }
    }
    out
}

#[test]
fn a_server_with_its_water_on_a_thread_makes_the_same_water_as_inline() {
    // face-a-only: the server's flat world is face A's; the host's both faces are tested in
    // `water::tests` (`the_water_on_its_own_thread_runs_as_inline`).
    let run = |placement| {
        let mut f = Fixture::new(on(placement));
        assert_eq!(f.server.water_placement(), placement);
        cut_lake(&mut f);
        for _ in 0..240 {
            f.server.step();
            // each tick's water waited on: the thread then answers exactly as inline does
            assert!(f.server.wait_water(Duration::from_secs(20)));
            f.take_all();
        }
        cells(&mut f)
    };
    let inline = run(Placement::Inline);
    let thread = run(Placement::Thread);
    assert!(
        inline.iter().filter(|&&m| m == M.air).count() > 0,
        "the lake drained some"
    );
    assert!(inline == thread, "the same water either way");
}

#[test]
fn a_water_write_overtaken_by_an_edit_is_dropped() {
    // face-a-only: the server's flat world is face A's.
    // ADR 0062: the water's writes reach the world a tick after it made them; a cell an edit made
    // solid meanwhile keeps the edit (the edit is on its way to the water). Without the check the
    // write clobbered the block.
    let mut f = Fixture::new(flat());
    cut_lake(&mut f);
    let stone = dwell_blocks::require("dwell:stone");
    let mut target = None;
    for _ in 0..240 {
        f.server.step();
        f.take_all();
        let w = f
            .server
            .pending_water_writes()
            .into_iter()
            .find(|w| dwell_blocks::info(w.state).liquid);
        if let Some(w) = w {
            target = Some(w.cell);
            break;
        }
    }
    let cell = target.expect("the water writes water");
    // an edit the water hasn't seen yet sets a block where it is about to write
    f.server.world().set_voxel(cell[0], cell[1], cell[2], stone);
    f.server.step();
    assert_eq!(
        f.server.world().get_voxel(cell[0], cell[1], cell[2]),
        stone,
        "the block stands at {cell:?}"
    );
    assert!(f.server.water_stats().1 >= 1, "the write was dropped");
}

#[test]
fn the_physics_tick_does_not_wait_for_the_water() {
    // face-a-only: the server's flat world is face A's.
    // ADR 0062 (owner, 2026-11-06: slowing the water whenever much of it moves "is not really
    // acceptable"): water slower than any tick (each run held 250 ms here, as a dammed river's
    // could take) doesn't hold the server's tick; inline, it does.
    let slow = CoreConfig {
        slow: Duration::from_millis(250),
        ..CoreConfig::thread()
    };
    let worst = |placement| {
        let mut f = Fixture::new(flat());
        f.server.set_water_host(WaterHost::with_config(
            placement,
            dwell_sim::water::Water::new(),
            slow,
        ));
        cut_lake(&mut f);
        let mut worst = Duration::ZERO;
        for _ in 0..60 {
            let t = Instant::now();
            f.server.step();
            worst = worst.max(t.elapsed());
            f.take_all();
        }
        (worst, f)
    };
    let (inline, _) = worst(Placement::Inline);
    assert!(
        inline >= Duration::from_millis(250),
        "inline, a tick waits for the water ({inline:?})"
    );
    let (thread, mut f) = worst(Placement::Thread);
    assert!(
        thread < Duration::from_millis(125),
        "on its thread, no tick waits for it: the worst took {thread:?}"
    );
    // and the water still runs, behind
    assert!(f.server.wait_water(Duration::from_secs(20)));
    let (stats, _) = f.server.water_stats();
    assert!(stats.runs > 0 && stats.max_busy_ms >= 250.0, "{stats:?}");
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dwell-water-thread-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The report's `free_m3`.
fn free_m3(report: &str) -> f64 {
    let at = report.find("\"free_m3\":").expect("free_m3") + 10;
    let end = report[at..].find(',').expect("a field after") + at;
    report[at..end].parse().expect("a number")
}

#[test]
fn the_waters_record_is_saved_and_restored_across_its_thread() {
    // face-a-only: the server's flat world is face A's; the record's both faces are tested in
    // `water::tests` (`the_record_survives_the_host`).
    let dir = Scratch::new("record");
    let path = dir.0.join(format!("w{WORLD_EXTENSION}"));
    let store = || WorldStore::new(WorldDb::open(&path).expect("opens"), false);
    let before = {
        let mut f = Fixture::new(ServerConfig {
            store: Some(store()),
            ..on(Placement::Thread)
        });
        cut_lake(&mut f);
        for _ in 0..120 {
            f.server.step();
            f.take_all();
        }
        // closing: a fresh record, waited for
        f.server.save_now();
        f.server.flush_store();
        let r = f.server.water_report([0, G, 5], 4).expect("a report");
        let v = free_m3(&r);
        assert!(v > 1.0, "free water to save: {r}");
        v
    };
    let mut f = Fixture::new(ServerConfig {
        store: Some(store()),
        ..on(Placement::Thread)
    });
    let r = f.server.water_report([0, G, 5], 4).expect("a report");
    let after = free_m3(&r);
    assert!(
        (after - before).abs() < 1e-3,
        "the free water came back: {before} → {after}"
    );
}
