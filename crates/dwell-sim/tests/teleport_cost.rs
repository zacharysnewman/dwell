//! A teleport's cost on the server's tick (ARCHITECTURE.md §6.3): a player at the browser's test
//! point (`?seed=0&at=2364.5,142,7443.5`) moved 200 m at once, as `__dwell.teleport` does, with
//! the server's jobs elsewhere (the browser's server-jobs worker, answered between ticks).

mod common;

use common::*;
use dwell_physics::terrain::{COLLISION_CELLS_PER_TICK, TerrainCollision};
use dwell_protocol::*;
use dwell_sim::jobs::JobRunner;
use dwell_sim::server::{ServerConfig, TICK_PHASES};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, FaceFrame, GENERATOR_TERRAIN, chunk_hash};
use dwell_worldgen::generator_for;

/// The worst tick after the teleports: its wall time, the terrain phase's, and the most
/// collider cells one tick set.
#[derive(Debug, Default)]
struct Worst {
    tick_ms: f64,
    terrain_ms: f64,
    cells: u64,
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
    let terrain = TICK_PHASES
        .iter()
        .position(|&n| n == "terrain")
        .expect("a phase");
    let mut worst = Worst::default();
    let mut x = at[0];
    // Four teleports of 200 m along +x (into the hills: buried chunks of full cubes, the
    // collider's dearest), 4 s apart, after 4 s at the join; then 4 s for the collider to settle.
    for t in 0..240 * 6 {
        if (240..240 * 5).contains(&t) && t % 240 == 0 {
            x += 200.0;
        }
        fx.move_to(w.player_id, x, at[1], at[2]);
        let clock = std::time::Instant::now();
        fx.server.step();
        let tick_ms = clock.elapsed().as_secs_f64() * 1e3;
        while let Some(job) = fx.server.take_job() {
            let done = runner.run(&job).expect("a job");
            fx.server.deliver_job(&done);
        }
        let _ = fx.take(1);
        let ms = fx.server.tick_phases();
        let counts = fx.server.tick_counts();
        let cells = counts
            .iter()
            .find(|c| c.0 == "collisionCells")
            .expect("a count")
            .1;
        if t >= 240 {
            worst.tick_ms = worst.tick_ms.max(tick_ms);
            worst.terrain_ms = worst.terrain_ms.max(ms[terrain].1);
            worst.cells = worst.cells.max(cells);
        }
    }
    let caught_up = fx
        .server
        .sim()
        .2
        .terrain()
        .is_some_and(TerrainCollision::caught_up);
    assert!(caught_up, "the collider never caught up with its chunks");
    println!("{f:?}: {worst:?}");
    worst
}

dwell_world::both_faces!(
    fn a_200_m_teleport_puts_its_chunks_into_the_collider_over_several_ticks(f: FaceFrame) {
        // Before: every cell of the chunks round the player went into Parry's voxels on the
        // teleport's tick, 65 536 for two buried chunks (4 ms each natively, 10-17 ms ticks;
        // several times that in the browser). Now those near the player at once, the rest a
        // little a tick.
        let worst = teleports(f);
        assert!(
            worst.cells <= COLLISION_CELLS_PER_TICK as u64 + 4096,
            "{worst:?}"
        );
    }
);
