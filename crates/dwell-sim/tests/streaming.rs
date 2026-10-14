//! Terrain streaming (ARCHITECTURE.md §6.3, Phases 3b–3c): the verification check, Generated vs.
//! Explicit vs. Air chunks, the spherical view, nearest-first order, the bandwidth budget,
//! unloading, eviction, and the worldgen pool.

mod common;

use std::collections::{BTreeSet, HashSet};
use std::time::Duration;

use common::*;
use dwell_physics::controller::Input;
use dwell_physics::net::quantize_input;
use dwell_protocol::world::{MAX_CHUNKS_PER_TICK, VIEW_RADIUS_CHUNKS};
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_sim::worldgen_pool::WorldgenPool;
use dwell_world::{
    CHUNK_SIZE, Chunk, ChunkCoord, GENERATOR_FLAT, GENERATOR_TERRAIN, M, MAX_CHUNK_Y, MIDPLANE_Y,
    MIN_CHUNK_Y, WORLD_RADIUS, chunk_hash, chunk_of,
};
use dwell_worldgen::{TerrainGenerator, air_test_for, generator_for};

const R: i32 = VIEW_RADIUS_CHUNKS as i32;

/// A player's body centre (world metres).
fn position_of(f: &mut Fixture, player_id: u16) -> Option<dwell_physics::DVec3> {
    let h = f.server.player_handle_of(player_id)?;
    let (_, physics, players) = f.server.sim();
    Some(players.position(physics, h))
}

#[test]
fn nothing_is_sent_before_the_clients_worldgen_check() {
    let mut f = Fixture::new(flat());
    f.join(1, &Client::new(1));
    assert!(f.chunks(1, 30, None).is_empty());
}

#[test]
fn a_matching_verification_hash_gets_generated_chunks_nearest_first() {
    let mut f = Fixture::new(ServerConfig {
        world_seed: 99,
        ..ServerConfig::default()
    });
    let welcome = f.join(1, &Client::new(1));
    // The client generates the verification chunk itself and reports its hash.
    let mut verification = Chunk::default();
    let vc = ChunkCoord::from_array(welcome.verification_chunk);
    generator_for(welcome.generator_version, welcome.world_seed)(vc, &mut verification);
    f.send(1, Message::WorldgenCheck(chunk_hash(&verification)));

    let chunks = f.chunks(1, 60, None);
    assert!(!chunks.is_empty());
    // Streaming centres on the player's chunk (players are spread around the spawn point).
    let position = position_of(&mut f, welcome.player_id).expect("a live player");
    let spawn = chunk_of(
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    );
    assert_eq!(coord_of(&chunks[0]), spawn);
    let mut previous = 0;
    let mut ordered = true;
    let air = air_test_for(GENERATOR_TERRAIN, 99);
    let mut air_chunks = 0;
    for c in &chunks {
        // Chunks the generator leaves empty travel as Air; the rest as Generated.
        let expected = if air(coord_of(c)) {
            ChunkForm::Air
        } else {
            ChunkForm::Generated
        };
        assert_eq!(c.form, expected, "{:?}", c.coord);
        air_chunks += i32::from(c.form == ChunkForm::Air);
        let (dx, dy, dz) = (
            c.coord[0] - spawn.x,
            c.coord[1] - spawn.y,
            c.coord[2] - spawn.z,
        );
        let d = dx * dx + dy * dy + dz * dz;
        ordered = ordered && d >= previous;
        previous = d;
    }
    assert!(ordered);
    assert!(air_chunks > 0); // the sky above the spawn
    // The whole view: a sphere of VIEW_RADIUS_CHUNKS.
    let mut expected = 0;
    for y in -R..=R {
        for z in -R..=R {
            for x in -R..=R {
                if x * x + y * y + z * z <= R * R + R
                    && spawn.y + y >= MIN_CHUNK_Y
                    && spawn.y + y <= MAX_CHUNK_Y
                {
                    expected += 1;
                }
            }
        }
    }
    assert_eq!(chunks.len(), expected);
    assert_eq!(f.server.stream_stats_of(1).unwrap().streamed, expected);
    // Every chunk once.
    let unique: HashSet<ChunkCoord> = chunks.iter().map(coord_of).collect();
    assert_eq!(unique.len(), chunks.len());
}

#[test]
fn a_mismatched_hash_or_0_gets_every_chunk_explicitly_as_generated() {
    for hash in [0u64, 12345] {
        let mut f = Fixture::new(ServerConfig {
            world_seed: 7,
            view_radius_chunks: 2,
            ..ServerConfig::default()
        });
        f.join(1, &Client::new(1));
        f.send(1, Message::WorldgenCheck(hash));
        let chunks = f.chunks(1, 120, None);
        assert!(chunks.len() > 20, "hash {hash}: {}", chunks.len());
        let generate = generator_for(GENERATOR_TERRAIN, 7);
        let mut explicit_chunks = 0;
        for c in &chunks {
            assert_eq!(c.revision, 0, "hash {hash}");
            let mut expected = Chunk::default();
            generate(coord_of(c), &mut expected);
            if c.form == ChunkForm::Air {
                // empty sky needs no payload in either mode
                assert!(expected.voxels().iter().all(|&v| v == M.air), "hash {hash}");
                continue;
            }
            assert_eq!(c.form, ChunkForm::Explicit, "hash {hash}");
            explicit_chunks += 1;
            assert!(
                c.voxels.iter().eq(expected.voxels().iter()),
                "hash {hash}: {:?}",
                c.coord
            );
        }
        assert!(explicit_chunks > 10, "hash {hash}: {explicit_chunks}");
    }
}

#[test]
fn open_sky_costs_nothing_the_view_is_a_sphere_across_the_worlds_rows() {
    // A player high in the sky (the world is 256 chunk rows tall): the view is all Air chunks, and
    // the server neither generates nor stores them.
    let mut f = Fixture::new(ServerConfig {
        world_seed: 99,
        spawn: Some([0.5, 3000.0, 0.5]),
        worldgen_budget: Duration::from_secs(1),
        ..ServerConfig::default()
    });
    f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0)); // full mode: still no payload for air
    let stored_before = f.server.world().loaded_chunks();
    let chunks = f.chunks(1, 60, None);
    let mut sphere = 0usize;
    for y in -R..=R {
        for z in -R..=R {
            for x in -R..=R {
                sphere += usize::from(x * x + y * y + z * z <= R * R + R);
            }
        }
    }
    assert_eq!(chunks.len(), sphere);
    for c in &chunks {
        assert_eq!(c.form, ChunkForm::Air, "{:?}", c.coord);
    }
    assert_eq!(
        f.server.stream_stats_of(1).unwrap().air_sent as usize,
        sphere
    );
    // Falling through the sky generates nothing either (the player keeps falling for a while).
    assert!(f.server.world().loaded_chunks() <= stored_before);
    assert_eq!(f.server.world().generated_on_access(), 0);
}

#[test]
fn chunks_per_tick_and_bytes_per_tick_stay_within_the_budget() {
    let mut f = Fixture::new(ServerConfig {
        world_seed: 3,
        chunk_bytes_per_second: 60 * 20000,
        ..ServerConfig::default()
    });
    f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0)); // full mode: large explicit chunks
    let mut largest = 0usize;
    for tick in 0..120 {
        f.server.step();
        let (mut bytes, mut count) = (0usize, 0usize);
        for (m, size) in f.take(1) {
            if !matches!(m, Message::ChunkData(_)) {
                continue;
            }
            bytes += size;
            count += 1;
            largest = largest.max(size);
        }
        assert!(
            count <= MAX_CHUNKS_PER_TICK as usize,
            "tick {tick}: {count}"
        );
        // The budget may be overdrawn by the one chunk that crossed it.
        assert!(bytes <= 20000 + largest, "tick {tick}: {bytes}");
    }
    assert!(largest > 100);
}

#[test]
fn moving_away_unloads_far_chunks_and_memory_stays_bounded() {
    let mut f = Fixture::new(flat());
    f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0));
    let mut unloads = Vec::new();
    f.chunks(1, 60, Some(&mut unloads));
    assert!(unloads.is_empty());
    let full_view = f.server.stream_stats_of(1).unwrap().streamed;

    // Walk (teleporting 1 m per tick) 20 chunks east.
    let (mut max_world, mut max_collision, mut max_streamed) = (0usize, 0usize, 0usize);
    for i in 0..20 * CHUNK_SIZE {
        f.move_to(1, 0.5 + f64::from(i), 0.0, 0.5);
        f.chunks(1, 1, Some(&mut unloads));
        max_world = max_world.max(f.server.world().loaded_chunks());
        let collision = f
            .server
            .players()
            .terrain()
            .map_or(0, |t| t.loaded_chunks());
        max_collision = max_collision.max(collision);
        max_streamed = max_streamed.max(f.server.stream_stats_of(1).unwrap().streamed);
    }
    f.chunks(1, 120, Some(&mut unloads));
    let stats = f.server.stream_stats_of(1).unwrap();
    assert!(!unloads.is_empty());
    assert!(stats.unloaded > 0);
    // What the client holds stays near one view: the view plus what is still within the unload
    // margin behind the player.
    assert!(
        stats.streamed >= full_view,
        "{} < {full_view}",
        stats.streamed
    );
    assert!(
        max_streamed < full_view * 3 / 2,
        "{max_streamed} vs {full_view}"
    );
    // The server holds the prefetch region around the player plus not-yet-evicted ones, not the
    // whole path (20 chunks × 5 × 5 × 5 would be thousands).
    assert!(max_world < 600, "{max_world}");
    assert!(max_collision < 200, "{max_collision}");
    // Every unloaded chunk is outside the client's final view.
    for u in &unloads {
        for c in &u.coords {
            assert!(c[0] < 20 - R, "{c:?}");
        }
    }
}

#[test]
fn chunks_are_generated_ahead_of_a_moving_player_never_on_the_tick() {
    // No threads, unlimited budget: the pool generates the prefetch region in step, so the player's
    // collision never has to generate synchronously (walking streams without hitches).
    let mut f = Fixture::new(ServerConfig {
        generator_version: GENERATOR_FLAT,
        spawn: Some([0.5, 0.0, 0.5]),
        worldgen_budget: Duration::from_secs(1),
        ..ServerConfig::default()
    });
    f.join(1, &Client::new(1));
    let before = f.server.world().generated_on_access();
    for i in 0..12 * CHUNK_SIZE {
        f.move_to(1, 0.5 + f64::from(i), 0.0, 0.5 + f64::from(i) * 0.4);
        f.server.step();
        f.server.take_outbox();
    }
    assert_eq!(f.server.world().generated_on_access(), before);
}

#[test]
fn the_worldgen_pool_generates_on_threads_the_same_chunks() {
    let generate = generator_for(GENERATOR_TERRAIN, 5);
    let pool = WorldgenPool::new(generate.clone(), 3);
    let wanted: Vec<ChunkCoord> = (0..24)
        .map(|i| ChunkCoord::new(i % 4, i / 8, (i / 4) % 2))
        .collect();
    pool.set_wanted(&wanted);
    pool.set_wanted(&wanted); // chunks already queued, in flight, or done are not generated twice
    let mut out = Vec::new();
    pool.drain(&mut out);
    assert_eq!(out.len(), wanted.len());
    for (coord, chunk) in &out {
        let mut expected = Chunk::default();
        generate(*coord, &mut expected);
        assert_eq!(chunk_hash(chunk), chunk_hash(&expected), "{coord:?}");
        assert_eq!(chunk.revision(), 0);
    }
}

#[test]
fn a_server_with_worldgen_threads_streams_to_a_full_mode_client() {
    let mut f = Fixture::new(ServerConfig {
        world_seed: 11,
        worldgen_threads: 2,
        view_radius_chunks: 2,
        ..ServerConfig::default()
    });
    f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0));
    let mut count = 0;
    let mut i = 0;
    while i < 600 && count < 30 {
        count += f.chunks(1, 1, None).len();
        i += 1;
    }
    assert!(count >= 30, "{count}");
}

/// World rim: walking off the edge of the disc falls toward the midplane, not into a void.
#[test]
fn world_rim_walking_off_the_edge_of_the_disc_falls_toward_the_midplane_not_into_a_void() {
    // The flat world near the rim: the last solid column is x = 8 191 999. There is no void: the
    // body falls the 2 km to the midplane and the flip band carries it across (BIFACIAL_WORLD.md §3).
    let spawn_x = f64::from(WORLD_RADIUS) - 9.5;
    let mut f = Fixture::new(ServerConfig {
        generator_version: GENERATOR_FLAT,
        spawn: Some([spawn_x, 0.0, 0.5]),
        worldgen_budget: Duration::from_secs(1),
        ..ServerConfig::default()
    });
    let welcome = f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0));
    let walk_east = Input {
        move_y: 1.0,
        look_yaw: 90.0,
        run: true,
        ..Input::default()
    }; // +X
    let mut seq = 0u32;
    let (mut last_x, mut lowest) = (0.0f64, 1e9f64);
    let mut crossed = false;
    for tick in 0..60 * 40 {
        seq += 1;
        let input = PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![quantize_input(
                &(if tick < 120 {
                    walk_east
                } else {
                    Input::default()
                }),
                seq,
            )],
        };
        f.server
            .on_datagram(1, &Message::PlayerInput(input).encode());
        f.server.step();
        f.server.take_outbox();
        if let Some(h) = f.server.player_handle_of(welcome.player_id) {
            let (_, physics, players) = f.server.sim();
            let p = players.position(physics, h);
            last_x = p.x;
            lowest = lowest.min(p.y);
            crossed = crossed || players.controller(h).face < 0;
        }
    }
    assert!(f.server.health_of(welcome.player_id) > 0); // not killed: no void
    assert!(last_x > f64::from(WORLD_RADIUS), "{last_x}"); // it went over the edge …
    assert!(crossed); // … and fell through the midplane
    assert!(lowest < f64::from(MIDPLANE_Y), "{lowest}");
}

#[test]
fn after_fast_flight_the_chunks_around_the_player_all_arrive() {
    // Fast flight (400 m/s, then down to the ground): the server still streams everything around
    // the player.
    let mut f = Fixture::new(ServerConfig {
        world_seed: 5,
        ..ServerConfig::default()
    });
    f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0));
    let mut held: BTreeSet<[i32; 3]> = BTreeSet::new();
    let mut pump = |f: &mut Fixture, ticks: i32| {
        let mut unloads = Vec::new();
        for c in f.chunks(1, ticks, Some(&mut unloads)) {
            held.insert(c.coord);
        }
        for u in &unloads {
            for c in &u.coords {
                held.remove(c);
            }
        }
    };
    pump(&mut f, 60);
    // Up to 300 m, then 400 m/s (6.7 m per tick) to (4880, 3500), then down to 40 m.
    // (The C++ test moved in f32; f64 here.)
    let (x, mut y, z) = (8.5f64, 6.0f64, 0.5f64);
    while y < 300.0 {
        f.move_to(1, x, y, z);
        pump(&mut f, 1);
        y += 6.7;
    }
    let len = (4880.0 - x).hypot(3500.0 - z);
    let mut d = 0.0;
    while d < len {
        f.move_to(
            1,
            x + (4880.0 - x) * d / len,
            300.0,
            z + (3500.0 - z) * d / len,
        );
        pump(&mut f, 1);
        d += 6.7;
    }
    // Down to just above the ground there.
    let ground = f64::from(TerrainGenerator::new(5).column_at(4880, 3500).height);
    y = 300.0;
    while y > ground + 2.0 {
        f.move_to(1, 4880.0, y, 3500.0);
        pump(&mut f, 1);
        y -= 6.7;
    }
    f.move_to(1, 4880.0, ground + 2.0, 3500.0);
    pump(&mut f, 600);
    let mut missing = Vec::new();
    let row = (ground / 32.0).floor() as i32;
    for cy in row - 1..=row + 1 {
        for cz in 108..=110 {
            for cx in 151..=153 {
                if !held.contains(&[cx, cy, cz]) {
                    missing.push([cx, cy, cz]);
                }
            }
        }
    }
    assert!(missing.is_empty(), "missing {missing:?}");
}
