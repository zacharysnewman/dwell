//! Chunk streaming's cost on the server's tick (ARCHITECTURE.md §6.3): a player flying across
//! chunk boundaries on the generated terrain, asking for chunks beyond the view as the client's LOD
//! does. Whether a chunk is all air needs its chunk column's sky floor, a chunk's column heights
//! (~1 ms natively, a few in the browser): computed on the tick for every new column it was 5–17
//! ms of a tick natively at a join and at each chunk boundary, and 50–640 ms a second in the
//! browser. Now off the tick (the server-jobs worker, a thread) or within a budget (inline).

mod common;

use std::collections::HashSet;

use common::*;
use dwell_protocol::world::VIEW_RADIUS_CHUNKS;
use dwell_protocol::*;
use dwell_sim::jobs::JobRunner;
use dwell_sim::server::{ServerConfig, TICK_PHASES};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{BUILD_VOLUME, Chunk, ChunkCoord, FaceFrame, GENERATOR_TERRAIN, chunk_hash};
use dwell_worldgen::generator_for;

fn phase(name: &str) -> usize {
    TICK_PHASES
        .iter()
        .position(|&n| n == name)
        .expect("a phase")
}

/// The worst ticks of a flight's streaming and generation phases (ms).
#[derive(Debug)]
struct Flight {
    stream: f64,
    worldgen: f64,
}

/// Joins a player at the browser's test point (`?seed=0&at=2364.5,142,7443.5`) on face `f` and
/// flies it +x at 20 m/s for 6 s, asking for the chunks round it within 5 chunks (a few a tick,
/// nearest first), then holds it still; asserts the whole view round where it stops arrived.
/// `remote`: the server's jobs run elsewhere (the browser's server-jobs worker), else on
/// `threads` worldgen threads (the dedicated server's), or inline with none.
fn fly(f: FaceFrame, remote: bool, threads: usize) -> Flight {
    let seed = 0;
    let at = f.point([2364.5, 142.0, 7443.5]);
    let mut fx = Fixture::new(ServerConfig {
        world_seed: seed,
        generator_version: GENERATOR_TERRAIN,
        spawn: Some(at),
        join_at_spawn: true,
        max_players: 1,
        lod_threads: Some(0),
        jobs_remote: remote,
        worldgen_threads: threads,
        store: Some(WorldStore::new(WorldDb::in_memory().expect("opens"), false)),
        ..ServerConfig::default()
    });
    let mut runner = JobRunner::for_world(GENERATOR_TERRAIN, seed);
    let w = fx.join(1, &Client::new(1));
    let mut v = Chunk::default();
    generator_for(w.generator_version, w.world_seed)(
        ChunkCoord::from_array(w.verification_chunk),
        &mut v,
    );
    fx.send(1, Message::WorldgenCheck(chunk_hash(&v)));
    fx.server.tick_phases();
    let (stream, worldgen) = (phase("stream"), phase("worldgen"));
    let mut flight = Flight {
        stream: 0.0,
        worldgen: 0.0,
    };
    let (mut have, mut asked) = (HashSet::new(), HashSet::new());
    let mut x = at[0];
    let fly_ticks = 120 + 6 * 60;
    for t in 0..fly_ticks + 4 * 60 {
        // Held in the air (no fall), moving once the view round the join has had its time.
        if (120..fly_ticks).contains(&t) {
            x += 20.0 / 60.0;
        }
        if fx.server.player_handle_of(w.player_id).is_some() {
            fx.move_to(w.player_id, x, at[1], at[2]);
        }
        let center = dwell_world::chunk_of_pos([x, at[1], at[2]]);
        let mut ask = Vec::new();
        'ask: for r in 4..=5 {
            for dz in -r..=r {
                for dx in -r..=r {
                    let c = ChunkCoord::new(center.x + dx, center.y - 1, center.z + dz);
                    if asked.insert(c) {
                        ask.push(c.to_array());
                        if ask.len() == 3 {
                            break 'ask;
                        }
                    }
                }
            }
        }
        if !ask.is_empty() {
            fx.send(1, Message::ChunkRequest(ask));
        }
        fx.server.step();
        if threads > 0 {
            // The threads keep time with the ticks, as at 60 Hz they would (a few ms apart here).
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        if remote {
            while let Some(job) = fx.server.take_job() {
                let done = runner.run(&job).expect("a job");
                fx.server.deliver_job(&done);
            }
        }
        for (m, _) in fx.take(1) {
            match m {
                Message::ChunkData(c) => {
                    have.insert(coord_of(&c));
                }
                Message::ChunkUnload(u) => {
                    for c in u.coords {
                        have.remove(&ChunkCoord::from_array(c));
                    }
                }
                _ => {}
            }
        }
        let ms = fx.server.tick_phases();
        flight.stream = flight.stream.max(ms[stream].1);
        flight.worldgen = flight.worldgen.max(ms[worldgen].1);
    }
    let center = dwell_world::chunk_of_pos([x, at[1], at[2]]);
    let r = VIEW_RADIUS_CHUNKS as i32;
    for dy in -r..=r {
        for dz in -r..=r {
            for dx in -r..=r {
                let c = ChunkCoord::new(center.x + dx, center.y + dy, center.z + dz);
                if dx * dx + dy * dy + dz * dz <= r * r + r && BUILD_VOLUME.chunk_may_hold(c) {
                    assert!(have.contains(&c), "{c:?} in view never arrived");
                }
            }
        }
    }
    let stats = fx.server.stream_stats_of(1).expect("a session");
    assert!(stats.requested_sent > 100, "{stats:?}");
    println!("{f:?} remote {remote}, threads {threads}: {flight:?}, {stats:?}");
    flight
}

dwell_world::both_faces!(
    fn flying_across_chunk_boundaries_streams_within_a_few_ms_a_tick(f: FaceFrame) {
        // Inline (no thread or worker): the sky floors are computed in the generation phase
        // within its budget, the streaming only asks.
        let inline = fly(f, false, 0);
        assert!(
            inline.stream < 2.0,
            "a tick streamed for {:.1} ms",
            inline.stream
        );
        assert!(
            inline.worldgen < 12.0,
            "a tick's generation took {:.1} ms",
            inline.worldgen
        );
        // As the browser runs it (the server-jobs worker): none of it on the tick.
        let remote = fly(f, true, 0);
        assert!(
            remote.stream < 2.0,
            "a tick streamed for {:.1} ms",
            remote.stream
        );
        // As the dedicated server runs it (worldgen threads): a thread of its own.
        let threads = fly(f, false, 1);
        assert!(
            threads.stream < 2.0 && threads.worldgen < 2.0,
            "{threads:?}"
        );
    }
);
