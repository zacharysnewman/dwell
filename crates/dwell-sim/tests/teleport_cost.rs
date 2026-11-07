//! A teleport's cost on the server's tick (ARCHITECTURE.md §6.3): a player at the browser's test
//! point (`?seed=0&at=2364.5,142,7443.5`) moved 200 m at once, as `__dwell.teleport` does, with
//! the server's jobs elsewhere (the browser's server-jobs worker, answered between ticks).

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::jobs::JobRunner;
use dwell_sim::server::{ServerConfig, TICK_PHASES};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, FaceFrame, GENERATOR_TERRAIN, chunk_hash};
use dwell_worldgen::generator_for;

/// The worst tick after the teleports: its wall time, its phases' sum and the worst phase.
#[derive(Debug, Default)]
struct Worst {
    tick_ms: f64,
    phases_ms: f64,
    phase: &'static str,
    phase_ms: f64,
    tick: u32,
}

fn teleports(f: FaceFrame) -> Worst {
    let seed = 0;
    let at = f.point([2364.5, 142.0, 7443.5]);
    let mut fx = Fixture::new(ServerConfig {
        world_seed: seed,
        generator_version: GENERATOR_TERRAIN,
        spawn: Some(at),
        join_at_spawn: true,
        max_players: 1,
        lod_threads: Some(0),
        jobs_remote: true,
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
    let mut worst = Worst::default();
    let mut x = at[0];
    for t in 0..240 * 4 {
        // Four teleports of 200 m, 4 s apart, after 4 s at the join.
        let moved = t >= 240 && t % 240 == 0;
        if moved {
            x += 200.0;
        }
        if let Some(h) = fx.server.player_handle_of(w.player_id) {
            let _ = h;
            fx.move_to(w.player_id, x, at[1], at[2]);
        }
        let clock = std::time::Instant::now();
        fx.server.step();
        let tick_ms = clock.elapsed().as_secs_f64() * 1e3;
        while let Some(job) = fx.server.take_job() {
            let done = runner.run(&job).expect("a job");
            fx.server.deliver_job(&done);
        }
        let _ = fx.take(1);
        let ms = fx.server.tick_phases();
        if t >= 240 && tick_ms > worst.tick_ms {
            let (k, p) = ms
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
                .expect("phases");
            worst = Worst {
                tick_ms,
                phases_ms: ms.iter().map(|p| p.1).sum(),
                phase: TICK_PHASES[k],
                phase_ms: p.1,
                tick: t,
            };
        }
        if moved || tick_ms > 8.0 {
            println!("tick {t}: {tick_ms:.1} ms, counts {:?}, {ms:?}", fx.server.tick_counts());
        }
    }
    println!("{f:?}: {worst:?}");
    worst
}

dwell_world::both_faces!(
    fn a_200_m_teleport_costs_no_tick_over_16_ms(f: FaceFrame) {
        let worst = teleports(f);
        assert!(worst.tick_ms < 16.0, "{worst:?}");
    }
);
