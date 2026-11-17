//! PRIORITY 0 (owner, 2026-11-16: "it's absolutely mandatory that the server never drops below
//! 60fps"): the server's tick on the owner's world (seed 3218026833, where they saw the stutter,
//! BUGS.md #194), a player moving at a sprint through it (`TICK_FLY=1`: flying at 362 m/s), its jobs
//! answered off the tick as the browser's are. Prints the worst ticks with their phases and fails a
//! tick over `TICK_BUDGET_MS` (default 8 natively). `TICK_GROWTH_SPEED` (default 60: a world day
//! every 20 s) sets how fast trees and grass grow.

mod common;

use common::*;
use dwell_protocol::players::FLY_SPEED_MAX_LEVEL;
use dwell_protocol::*;
use dwell_sim::jobs::JobRunner;
use dwell_sim::server::{ServerConfig, TICK_PHASES};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, GENERATOR_TERRAIN, chunk_hash};
use dwell_worldgen::generator_for;

/// How much slower the browser's jobs worker (WASM) runs a job than natively.
const WASM_SLOWDOWN: f64 = 2.5;

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test tick_budget -- --ignored --nocapture"]
fn the_owners_world_at_a_sprint() {
    let seed = 3_218_026_833;
    let at = [-3356.5, 34.0, -3763.7];
    let mut fx = Fixture::new(ServerConfig {
        world_seed: seed,
        generator_version: GENERATOR_TERRAIN,
        spawn: std::env::var("TICK_AT_OWNER").is_ok().then_some(at),
        join_at_spawn: true,
        max_players: 1,
        lod_threads: Some(0),
        jobs_remote: true,
        // As the browser's local world (dwell-client local.rs): no worldgen threads, saving often.
        worldgen_threads: 0,
        autosave_seconds: 10,
        // A world day every 20 s: grass and trees grow as in an old world.
        growth_speed: std::env::var("TICK_GROWTH_SPEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60.0),
        store: Some(WorldStore::new(WorldDb::in_memory().expect("opens"), false)),
        ..ServerConfig::default()
    });
    // As the browser's local world (dwell-client local.rs, under wasm32): the phone's collision
    // budgets (shared/render/streaming.json) and no generation on the reach.
    fx.server.set_collision_budgets(
        1024,
        dwell_physics::terrain::CollisionAhead {
            chunks_per_tick: 1,
            ..Default::default()
        },
    );
    fx.server.set_reach_generates(0);
    let mut runner = JobRunner::for_world(GENERATOR_TERRAIN, seed);
    let w = fx.join(1, &Client::new(1));
    let mut v = Chunk::default();
    generator_for(w.generator_version, w.world_seed)(
        ChunkCoord::from_array(w.verification_chunk),
        &mut v,
    );
    fx.send(1, Message::WorldgenCheck(chunk_hash(&v)));
    type Tick<'a> = (f64, usize, Vec<(&'a str, f64)>);
    let mut ticks: Vec<Tick> = Vec::new();
    let mut pending = std::collections::VecDeque::new();
    let mut finished: std::collections::VecDeque<(f64, Vec<u8>)> =
        std::collections::VecDeque::new();
    let mut worker_free_ms = 0.0f64;
    let mut on_tick = 0;
    let mut heading: f64 = 0.0;
    // `TICK_FLY`: flying straight at the top speed level (362 m/s; owner, 2026-11-16: "big hitches
    // while flying at 362 m/s"), climbing gently, instead of sprinting.
    let fly = std::env::var("TICK_FLY").is_ok();
    let secs = std::env::var("TICK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90usize);
    for t in 0..60 * secs {
        // After 5 s at the join: sprinting forward through the controller, jumping every
        // 1.3 s, turning slowly (as the browser's stutter-run scenario).
        if t > 300 {
            let input = if fly {
                dwell_physics::controller::Input {
                    move_y: 1.0,
                    run: true,
                    fly: true,
                    fly_speed: FLY_SPEED_MAX_LEVEL as u8,
                    // As the bench's stutter-fly: 3° up for 20 s, then level turned 90°.
                    look_pitch: if t < 300 + 1200 { 3.0 } else { 0.0 },
                    look_yaw: if t < 300 + 1200 { 0.0 } else { 90.0 },
                    ..Default::default()
                }
            } else {
                heading += 0.15;
                dwell_physics::controller::Input {
                    move_y: 1.0,
                    run: true,
                    jump: t % 80 < 8,
                    look_yaw: heading as f32,
                    ..Default::default()
                }
            };
            let m = Message::PlayerInput(PlayerInput {
                last_snapshot_tick: 0,
                inputs: vec![dwell_physics::net::quantize_input(&input, (t - 300) as u32)],
            });
            fx.server.on_datagram(1, &m.encode());
        }
        let clock = std::time::Instant::now();
        fx.server.step();
        let tick_ms = clock.elapsed().as_secs_f64() * 1e3;
        // The browser's jobs worker: one thread beside the tick, each job taking its native time
        // times WASM's slowdown; an answer reaches the tick once the worker has finished it.
        let now_ms = t as f64 * 1000.0 / 60.0;
        while let Some(job) = fx.server.take_job() {
            pending.push_back(job);
        }
        while worker_free_ms <= now_ms {
            let Some(job) = pending.pop_front() else {
                break;
            };
            let clock = std::time::Instant::now();
            let done = runner.run(&job).expect("a job");
            worker_free_ms = worker_free_ms.max(now_ms - 1000.0 / 60.0)
                + clock.elapsed().as_secs_f64() * 1e3 * WASM_SLOWDOWN;
            finished.push_back((worker_free_ms, done));
        }
        while finished.front().is_some_and(|(at, _)| *at <= now_ms) {
            let (_, done) = finished.pop_front().expect("finished");
            fx.server.deliver_job(&done);
        }
        let _ = fx.take(1);
        let ms = fx.server.tick_phases();
        let made = fx
            .server
            .tick_counts()
            .iter()
            .find(|c| c.0 == "generatedOnTick")
            .map_or(0, |c| c.1);
        on_tick += made;
        if t > 300 {
            let mut phases: Vec<(&str, f64)> = ms
                .iter()
                .filter(|p| p.1 > 0.5)
                .map(|p| (p.0, p.1))
                .collect();
            phases.sort_by(|a, b| b.1.total_cmp(&a.1));
            ticks.push((tick_ms, t, phases));
        }
    }
    let _ = TICK_PHASES;
    let mut sorted: Vec<_> = ticks.iter().collect();
    sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
    let n = ticks.len();
    let mut all: Vec<f64> = ticks.iter().map(|t| t.0).collect();
    all.sort_by(f64::total_cmp);
    eprintln!(
        "GENERATED ON TICK {on_tick}; TICKS {n}: p50 {:.2} p99 {:.2} max {:.2} ms; over 8 ms: {}, over 16.7 ms: {}",
        all[n / 2],
        all[n * 99 / 100],
        all[n - 1],
        all.iter().filter(|&&v| v > 8.0).count(),
        all.iter().filter(|&&v| v > 16.7).count()
    );
    eprintln!("END feet {:?}", fx.server.feet_of(w.player_id));
    eprintln!("GRASS REGENERATED {}", fx.server.grass_regenerated());
    for (ms, t, phases) in sorted.iter().take(12) {
        eprintln!("WORST {ms:.2} ms at tick {t}: {phases:?}");
    }
    // The budget (priority 0): no tick over `TICK_BUDGET_MS` natively (default 8; the browser runs
    // ~2–3× slower, so 8 natively is ~16–24 ms there at worst).
    let budget: f64 = std::env::var("TICK_BUDGET_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8.0);
    assert!(
        all[n - 1] <= budget,
        "a tick of {:.2} ms, over the {budget} ms budget: {:?}",
        all[n - 1],
        sorted[0]
    );
}
