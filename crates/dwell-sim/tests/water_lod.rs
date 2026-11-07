//! The LOD's work for chunks the water rewrites (Phase 53 G5 follow-up, docs/WATER_SURFACE_GRID.md
//! §2d): a flood at seed 0's dam site must not spike the server's tick.

mod common;

use std::time::{Duration, Instant};

use common::*;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;

/// Seed 0's dam site (the bench's `?at=2364.5,142,7443.5`), as the browser's local server runs:
/// no worker threads, the LOD on the tick.
fn dam_site() -> ServerConfig {
    ServerConfig {
        world_seed: 0,
        generator_version: dwell_world::GENERATOR_TERRAIN,
        spawn: Some([2364.5, 142.0, 7443.5]),
        worldgen_threads: 0,
        lod_threads: Some(0),
        pregen_radius_chunks: 3,
        water: dwell_sim::water::host::Placement::Thread,
        ..ServerConfig::default()
    }
}

fn region(op: RegionEditOp, min: [i32; 3], size: [u8; 3], material: u16) -> Message {
    Message::RegionEdit(RegionEdit {
        op,
        min,
        size,
        material,
        from: 0,
        cells: Vec::new(),
    })
}

/// The bench's `dam-flood`: a stone wall 31 m across the river 4 m ahead and a pit 31 × 29 m dug
/// beside it, from the player's feet.
fn dam_and_pit(f: &mut Fixture) {
    let stone = dwell_blocks::require("dwell:stone");
    let (x, y, z) = (2364, 284, 7443); // the feet's cell (y in ½ m rows)
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x - 15, y - 12, z + 4],
            [31, 19, 1],
            stone,
        ),
    );
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x - 15, y - 12, z - 31],
            [31, 10, 29],
            0,
        ),
    );
}

/// Server ticks' wall times (ms) over `ticks` ticks paced at 60 Hz, after the dam.
fn flood_ticks(ticks: u32, remote: bool) -> Vec<f64> {
    let mut f = Fixture::new(ServerConfig {
        jobs_remote: remote,
        ..dam_site()
    });
    let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_TERRAIN, 0);
    f.join(1, &Client::new(1));
    for _ in 0..120 {
        f.server.step();
        f.take_all();
    }
    dam_and_pit(&mut f);
    let t = Instant::now();
    f.server.step();
    println!(
        "the edits' tick: {:.2} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    let stone = dwell_blocks::require("dwell:stone");
    assert_eq!(
        f.server.world().get_voxel(2364, 284 - 4, 7447),
        stone,
        "the wall stands"
    );
    let start = Instant::now();
    let mut ms = Vec::new();
    for i in 0..ticks {
        let (g0, n0) = {
            let w = f.server.world();
            (w.generated_on_access(), w.loaded_chunks())
        };
        let t = Instant::now();
        f.server.step();
        let el = t.elapsed().as_secs_f64() * 1000.0;
        ms.push(el);
        if el > 5.0 {
            let w = f.server.world();
            println!(
                "  tick {i}: {el:.1} ms, generated {} on access, chunks {} -> {}",
                w.generated_on_access() - g0,
                n0,
                w.loaded_chunks()
            );
        }
        f.take_all();
        // the LOD worker's part, off the tick
        while let Some(job) = f.server.take_job() {
            let done = runner.run(&job).expect("a job");
            f.server.deliver_job(&done);
        }
        let due = start + Duration::from_secs_f64(f64::from(i + 1) / 60.0);
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
    }
    ms
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test water_lod measure -- --ignored --nocapture"]
fn measure_the_dam_sites_flood_ticks() {
    // face-a-only: a measurement at seed 0's dam site, on face A.
    for remote in [false, true] {
        println!(
            "LOD jobs {}:",
            if remote {
                "in their worker"
            } else {
                "on the tick"
            }
        );
        let ms = flood_ticks(600, remote);
        for (s, sec) in ms.chunks(60).enumerate() {
            let max = sec.iter().copied().fold(0.0, f64::max);
            let mean = sec.iter().sum::<f64>() / sec.len() as f64;
            println!("second {s}: tick mean {mean:.2} ms, max {max:.2} ms");
        }
    }
}

#[test]
#[ignore = "a measurement: the LOD generator's time per section level at the dam site"]
fn measure_the_lod_generator_by_level() {
    // face-a-only: a measurement at seed 0's dam site, on face A.
    let g = dwell_worldgen::lod_generator_for(dwell_world::GENERATOR_TERRAIN, 0);
    let c = dwell_world::chunk_of(2364, 284, 7443);
    let mut s = dwell_world::lod::lod_of_chunk(c);
    let mut cells = vec![0u16; dwell_world::lod::VOLUME];
    // warm the world map
    g(dwell_world::lod::parent(s), &mut cells);
    for _ in 0..dwell_world::lod::MAX_LEVEL {
        s = dwell_world::lod::parent(s);
        let t = Instant::now();
        g(s, &mut cells);
        println!(
            "level {}: {:.2} ms",
            s.level,
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// The flood of `water_thread.rs` on the flat world (a lake 61 × 57 four rows deep, its near wall cut
/// 20 wide), with the LOD as the browser's local server has it: on the tick (`remote` false) or in
/// its worker (`remote`, the jobs run by a `LodRunner` between ticks here).
fn flat_flood(remote: bool, ticks: u32) -> Fixture {
    let mut f = Fixture::new(ServerConfig {
        lod_threads: Some(0),
        jobs_remote: remote,
        ..flat()
    });
    let w = f.join(1, &Client::new(1));
    let mut v = dwell_world::Chunk::default();
    dwell_world::generate_flat_chunk(
        dwell_world::ChunkCoord::from_array(w.verification_chunk),
        &mut v,
    );
    f.send(1, Message::WorldgenCheck(dwell_world::chunk_hash(&v)));
    f.server.step();
    f.take_all();
    let g = dwell_world::FLAT_TOP_Y;
    let stone = dwell_blocks::require("dwell:stone");
    let w = f.server.world();
    for x in -31..=31 {
        for z in 3..=61 {
            for y in g..g + 4 {
                let wall = x == -31 || x == 31 || z == 3 || z == 61;
                w.set_voxel(x, y, z, if wall { stone } else { dwell_world::M.water });
            }
        }
    }
    f.server.step();
    f.take_all();
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [-10, g, 3],
            [20, 4, 1],
            dwell_world::M.air,
        ),
    );
    let mut runner = dwell_sim::jobs::JobRunner::new(
        dwell_world::flat_generator(),
        dwell_worldgen::lod_generator_for(dwell_world::GENERATOR_FLAT, 0),
    );
    for _ in 0..ticks {
        f.server.step();
        f.take_all();
        while let Some(job) = f.server.take_job() {
            let done = runner.run(&job).expect("a job");
            assert!(f.server.deliver_job(&done));
        }
    }
    f
}

/// Every section's content (revisions apart).
fn contents(f: &Fixture) -> Vec<(dwell_world::lod::LodCoord, Vec<u8>)> {
    let mut out: Vec<_> = f
        .server
        .lod()
        .sections()
        .iter()
        .map(|(c, s)| (*c, s.encoded.clone()))
        .collect();
    out.sort_by_key(|(c, _)| (c.level, c.i, c.j, c.k));
    out
}

#[test]
fn a_floods_lod_is_computed_once_a_hold_from_kept_bases() {
    // face-a-only: the LOD's sections are world volumes (no up or down); the flood is the flat
    // world's, face A's.
    // Owner, 2026-11-06: a flood's first seconds spiked the browser's server tick. Every water tick
    // rewrote the flooding chunks and each one's LOD section was computed again from the
    // generator, on the tick. Now a section the water rewrites is computed at most once a hold
    // (2 s) and from its kept generated base. Red before: ~a recompute per section per water tick
    // (10 a second), each running the generator.
    let f = flat_flood(false, 600);
    let lod = f.server.lod();
    let sections = lod.sections().len() as u64;
    let jobs = lod.generator_runs + lod.base_hits;
    assert!(sections > 0, "the flood wrote LOD sections");
    assert!(
        lod.generator_runs <= sections + 4,
        "the generator ran once a section: {} runs for {sections} sections",
        lod.generator_runs
    );
    // 10 s of flood: at most one computation a 2 s hold per section, and a few for edits
    assert!(
        jobs <= sections * 7,
        "{jobs} computations for {sections} sections over 10 s"
    );
}

#[test]
fn the_lod_after_a_flood_matches_a_recompute_on_the_tick_and_in_its_worker() {
    // face-a-only: the LOD's sections are world volumes (no up or down); the flood is the flat
    // world's, face A's.
    // Held sections and kept bases change when a section is computed, not what it holds: after a
    // flood, the sections are those computed from scratch from the world as it is.
    let mut tick = flat_flood(false, 360);
    tick.server.drain_lod();
    let mut worker = flat_flood(true, 360);
    worker.server.drain_lod();
    // from scratch: every modified chunk's section dirty, computed with nothing kept
    let mut scratch = dwell_sim::lod_propagation::LodPropagation::new(
        dwell_worldgen::lod_generator_for(dwell_world::GENERATOR_FLAT, 0),
        0,
    );
    let world = tick.server.world();
    let modified: Vec<dwell_world::ChunkCoord> = world
        .chunks()
        .filter(|(_, c)| c.revision() > 0)
        .map(|(c, _)| *c)
        .collect();
    for &c in &modified {
        scratch.mark_chunk(c);
    }
    scratch.drain(&[], &mut |c| {
        dwell_sim::lod_propagation::modified_voxels(world.find(c))
    });
    let mut expect: Vec<_> = scratch
        .sections()
        .iter()
        .map(|(c, s)| (*c, s.encoded.clone()))
        .collect();
    expect.sort_by_key(|(c, _)| (c.level, c.i, c.j, c.k));
    let on_tick = contents(&tick);
    assert!(!expect.is_empty());
    assert!(on_tick == expect, "the tick's LOD is the world's");
    // the worker's flood is the same flood (the water inline, deterministic)
    assert!(
        contents(&worker) == on_tick,
        "the worker's LOD is the tick's"
    );
}

#[test]
#[ignore = "a measurement: LOD revisions written over a dam's minutes"]
fn measure_the_lod_churn_of_a_dam() {
    // face-a-only: a measurement at seed 0's dam site.
    let mut f = Fixture::new(ServerConfig {
        water: dwell_sim::water::host::Placement::Inline,
        ..dam_site()
    });
    f.join(1, &Client::new(1));
    for _ in 0..120 {
        f.server.step();
        f.take_all();
    }
    let stone = dwell_blocks::require("dwell:stone");
    let (x, y, z) = (2364, 284, 7443);
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x - 15, y - 12, z + 4],
            [31, 19, 1],
            stone,
        ),
    );
    let mut last = f.server.lod().last_revision();
    for minute_tenth in 0..30 {
        for _ in 0..600 {
            f.server.step();
            f.take_all();
        }
        let now = f.server.lod().last_revision();
        println!(
            "{:>4} s: {} LOD sections written, {} dirty, {} held",
            (minute_tenth + 1) * 10,
            now - last,
            f.server.lod().dirty().len(),
            f.server.lod().held()
        );
        last = now;
    }
}

/// Every modified section's content against a from-scratch computation of the server's world.
fn lod_matches_world(f: &mut Fixture, generator: dwell_world::lod::LodGenerator) -> Vec<String> {
    let mut scratch = dwell_sim::lod_propagation::LodPropagation::new(generator, 0);
    let world = f.server.world();
    let modified: Vec<dwell_world::ChunkCoord> = world
        .chunks()
        .filter(|(_, c)| c.revision() > 0)
        .map(|(c, _)| *c)
        .collect();
    for &c in &modified {
        scratch.mark_chunk(c);
    }
    scratch.drain(&[], &mut |c| {
        dwell_sim::lod_propagation::modified_voxels(world.find(c))
    });
    let mut bad = Vec::new();
    for (c, s) in scratch.sections() {
        match f.server.lod().find(*c) {
            Some(have) if have.encoded == s.encoded => {}
            Some(_) => bad.push(format!("{c:?} differs")),
            None => bad.push(format!("{c:?} missing")),
        }
    }
    bad
}

#[test]
#[ignore = "a probe: a world flooded and saved, reopened with the LOD jobs in their worker"]
fn probe_a_reopened_flooded_worlds_lod() {
    // face-a-only: seed 0's dam site, on face A.
    let dir = std::env::temp_dir().join(format!("dwell-lod-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("w.dwellworld");
    let store = || {
        dwell_storage::WorldStore::new(dwell_storage::WorldDb::open(&path).expect("opens"), false)
    };
    let generator = dwell_worldgen::lod_generator_for(dwell_world::GENERATOR_TERRAIN, 0);
    {
        let mut f = Fixture::new(ServerConfig {
            store: Some(store()),
            ..dam_site()
        });
        f.join(1, &Client::new(1));
        for _ in 0..120 {
            f.server.step();
            f.take_all();
        }
        dam_and_pit(&mut f);
        for _ in 0..1800 {
            f.server.step();
            f.take_all();
        }
        f.server.save_now();
        f.server.flush_store();
        println!(
            "first session: {} sections",
            f.server.lod().sections().len()
        );
    }
    let mut f = Fixture::new(ServerConfig {
        store: Some(store()),
        jobs_remote: true,
        ..dam_site()
    });
    let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_TERRAIN, 0);
    f.join(1, &Client::new(1));
    let mut queue: std::collections::VecDeque<Vec<u8>> = Default::default();
    for t in 0..3600 {
        f.server.step();
        f.take_all();
        while let Some(job) = f.server.take_job() {
            queue.push_back(job);
        }
        // the worker answers a job a tick, late
        if t % 2 == 0
            && let Some(job) = queue.pop_front()
        {
            let done = runner.run(&job).expect("an answer");
            f.server.deliver_job(&done);
        }
        if t % 600 == 599 {
            let lod = f.server.lod();
            println!(
                "{} s: {} sections, {} dirty, {} in flight, {} queued",
                (t + 1) / 60,
                lod.sections().len(),
                lod.dirty().len(),
                lod.in_flight().len(),
                queue.len()
            );
        }
    }
    let bad = lod_matches_world(&mut f, generator);
    println!(
        "{} sections differ from the world's: {:?}",
        bad.len(),
        &bad[..bad.len().min(10)]
    );
}
