//! Terrain collision built ahead of the player (WORK_QUEUE items 0 and 16, BUGS.md #72): a chunk
//! the player is heading into is built a few a tick before its reach gets there, so no tick builds
//! a newly reached chunk (on a phone that build was the worst frame while moving), the look-ahead's
//! per-tick budget holds, and the player moves exactly as without it. In the local frame, so the
//! mirror suite (`DWELL_FACE=b`) runs it on face B too.

mod common;

use std::sync::Arc;

use common::*;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::Input;
use dwell_physics::terrain::{CollisionAhead, CollisionStats};
use dwell_physics::{DVec3, Vec3};
use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, ChunkGenerator, chunk_of_pos, local_index};

/// Rolling ground of full cubes around local y 0 (face A's rows; `shifted` mirrors it on face B):
/// surface chunks with steps and buried ones below, so builds have cells to set.
fn ground() -> ChunkGenerator {
    let block = cube_block();
    Arc::new(move |c: ChunkCoord, chunk: &mut Chunk| {
        let v = chunk.voxels_mut();
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let (wx, wz) = (c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z);
                // 0-3 rows (½ m each) of steps over the flat top: walkable.
                let top = ORIGIN_Y + ((wx.div_euclid(6) + wz.div_euclid(9)).rem_euclid(4) / 2);
                for y in 0..CHUNK_SIZE {
                    if c.y * CHUNK_SIZE + y < top {
                        v[local_index(x, y, z)] = block;
                    }
                }
            }
        }
    })
}

/// One run's record: each tick's position, and its collision stats after the tick.
struct Run {
    feet: Vec<DVec3>,
    stats: Vec<CollisionStats>,
    /// Whether the chunk holding the player was built when its tick began (from tick 1 on).
    unbuilt_ticks: Vec<usize>,
}

fn run(ahead: Option<CollisionAhead>, input: Input, start: Vec3, ticks: usize) -> Run {
    let mut w = PlayerTestWorld::with(PlayerControllerConfig::default(), ground());
    if let Some(a) = ahead {
        w.players.set_collision_ahead(a);
    }
    let h = w.spawn(start);
    w.set_input(move |_, _| input);
    let mut out = Run {
        feet: Vec::new(),
        stats: Vec::new(),
        unbuilt_ticks: Vec::new(),
    };
    for t in 0..ticks {
        let at = w.players.position(&w.physics, h);
        let terrain = w.players.terrain().expect("terrain");
        if t > 0 && !terrain.is_loaded(chunk_of_pos(at.to_array())) {
            out.unbuilt_ticks.push(t);
        }
        w.step(1);
        out.feet.push(w.players.position(&w.physics, h));
        out.stats
            .push(w.players.terrain().expect("terrain").stats());
    }
    out
}

fn off() -> CollisionAhead {
    CollisionAhead {
        chunks_per_tick: 0,
        ..CollisionAhead::default()
    }
}

/// Checks a run with the look-ahead against one without: no tick after the first builds a reached
/// chunk or waits for one, the budget holds, and the player moves exactly the same.
fn check(ahead: CollisionAhead, input: Input, start: Vec3, ticks: usize, min_m: f64) {
    let with = run(Some(ahead), input, start, ticks);
    let without = run(Some(off()), input, start, ticks);
    let moved = (with.feet[ticks - 1] - with.feet[0]).length();
    assert!(moved > min_m, "moved {moved} m");
    // Without it, chunks are built on the tick that reaches them (what this fixes).
    let late = without.stats[ticks - 1].reach_loads - without.stats[0].reach_loads;
    assert!(
        late > 0,
        "without the look-ahead no chunk was reached unbuilt"
    );
    for t in 1..ticks {
        let (a, b) = (with.stats[t - 1], with.stats[t]);
        assert_eq!(
            b.reach_loads, a.reach_loads,
            "tick {t}: a reached chunk was built on the tick that needed it ({:?})",
            with.feet[t]
        );
        let built = b.ahead_loads - a.ahead_loads;
        assert!(
            built <= ahead.chunks_per_tick as u64,
            "tick {t}: the look-ahead built {built} chunks, over its {}",
            ahead.chunks_per_tick
        );
    }
    assert!(
        with.unbuilt_ticks.is_empty(),
        "the player was in an unbuilt chunk at ticks {:?}",
        with.unbuilt_ticks
    );
    for (t, (a, b)) in with.feet.iter().zip(&without.feet).enumerate() {
        assert_eq!(a, b, "tick {t}: the look-ahead moved the player");
    }
}

fn fly(level: u8, yaw: f32, up: bool) -> Input {
    Input {
        fly: true,
        fly_speed: level,
        ..input(0.0, 1.0, true, up, false, yaw, 0.0)
    }
}

#[test]
fn sprinting_into_new_chunks_never_builds_one_on_the_tick_that_reaches_it() {
    // ~8 m/s for 12 s: across three chunk borders, diagonally (corners).
    let i = input(0.0, 1.0, true, false, false, 45.0, 0.0);
    check(
        CollisionAhead::default(),
        i,
        Vec3::new(4.0, 2.0, 4.0),
        720,
        64.0,
    );
}

#[test]
fn flying_fast_into_new_chunks_never_builds_one_on_the_tick_that_reaches_it() {
    // Level 6 (8× the fly speed), climbing and level, with the phone's budget of one a tick.
    let phone = CollisionAhead {
        chunks_per_tick: 1,
        ..CollisionAhead::default()
    };
    check(
        phone,
        fly(6, 60.0, false),
        Vec3::new(4.0, 4.0, 4.0),
        360,
        200.0,
    );
    check(
        phone,
        fly(4, 300.0, true),
        Vec3::new(4.0, 4.0, 4.0),
        300,
        60.0,
    );
}

#[test]
fn a_tick_builds_at_most_the_budget_however_many_chunks_lie_ahead() {
    // A fast flight with a large look-ahead: many chunks ahead each tick, one built a tick.
    let wide = CollisionAhead {
        seconds: 4.0,
        max_metres: 64.0,
        margin_metres: 16.0,
        chunks_per_tick: 1,
        min_speed: 1.0,
    };
    let r = run(
        Some(wide),
        fly(8, 0.0, false),
        Vec3::new(4.0, 4.0, 4.0),
        240,
    );
    let mut most = 0;
    for t in 1..r.stats.len() {
        most = most.max(r.stats[t].ahead_loads - r.stats[t - 1].ahead_loads);
    }
    assert_eq!(most, 1, "a tick built {most} chunks ahead");
    assert!(r.stats[r.stats.len() - 1].ahead_loads > 20);
}

/// Fills the streamed world with the generated chunks within 3 chunks (1 up and down) of the
/// player's, at most `per_tick` a tick, nearest first: the server's streaming, ahead of the player.
fn stream(w: &mut PlayerTestWorld, source: &ChunkGenerator, at: DVec3, per_tick: usize) {
    let home = chunk_of_pos(at.to_array());
    let mut missing = Vec::new();
    for dy in -2..=2 {
        for dz in -3..=3 {
            for dx in -3..=3 {
                let c = ChunkCoord::new(home.x + dx, home.y + dy, home.z + dz);
                if w.world.find(c).is_none() {
                    missing.push((dx * dx + dy * dy + dz * dz, c));
                }
            }
        }
    }
    missing.sort_by_key(|&(d, c)| (d, c.y, c.z, c.x));
    for (_, c) in missing.into_iter().take(per_tick) {
        let mut chunk = Chunk::default();
        source(c, &mut chunk);
        w.world.put(c, chunk);
    }
}

#[test]
fn on_a_streamed_world_chunks_are_built_ahead_once_and_never_on_the_tick_that_reaches_them() {
    // The client's prediction: no generator, chunks arrive (here a few a tick ahead of the player).
    let ahead = CollisionAhead {
        chunks_per_tick: 1,
        ..CollisionAhead::default()
    };
    let source = shifted(ground());
    let mut w = PlayerTestWorld::with(PlayerControllerConfig::default(), ground());
    w.world = dwell_world::VoxelWorld::streamed();
    w.players.set_collision_ahead(ahead);
    let start = to_world(Vec3::new(4.0, 4.0, 4.0));
    for _ in 0..200 {
        stream(&mut w, &source, start, 64);
    }
    let h = w.spawn(Vec3::new(4.0, 4.0, 4.0));
    w.set_input(move |_, _| fly(5, 75.0, false));
    w.step(1);
    let first = w.players.position(&w.physics, h);
    for t in 1..600 {
        let at = w.players.position(&w.physics, h);
        stream(&mut w, &source, at, 8);
        let (s0, l0) = {
            let terrain = w.players.terrain().expect("terrain");
            assert!(
                terrain.is_loaded(chunk_of_pos(at.to_array())),
                "tick {t}: in an unbuilt chunk"
            );
            (terrain.stats(), terrain.loads())
        };
        w.step(1);
        let terrain = w.players.terrain().expect("terrain");
        let (s1, l1) = (terrain.stats(), terrain.loads());
        assert_eq!(
            s1.reach_loads, s0.reach_loads,
            "tick {t}: built when reached"
        );
        // Every build of the tick is the look-ahead's: none rebuilt for a neighbour's arrival.
        assert!(l1 - l0 <= 1, "tick {t}: {} chunk builds", l1 - l0);
    }
    let moved = (w.players.position(&w.physics, h) - first).length();
    assert!(moved > 150.0, "flew {moved} m");
}

/// Natively, the per-tick terrain cost flying over the seed-0 terrain with and without the
/// look-ahead (WORK_QUEUE item 16's measurement): `cargo test --release -p dwell-physics --test
/// collision_ahead -- --ignored --nocapture`.
// face-a-only: a timing of the generated world's face A spawn; the parity is the tests above.
#[test]
#[ignore]
fn measure_the_tick_cost_with_and_without_the_look_ahead() {
    use dwell_world::GENERATOR_TERRAIN;
    for (name, level, cells) in [
        ("sprint, desktop", None, 8192),
        ("fly 6, desktop", Some(6u8), 8192),
        ("fly 6, phone", Some(6), 1024),
        ("fly 8, phone", Some(8), 1024),
    ] {
        for chunks in [0usize, 2, 1] {
            let seed = 0;
            let mut w = PlayerTestWorld::world_frame_of(
                dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
                Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
            );
            w.players.set_collision_cells_per_tick(cells);
            let margin = std::env::var("AHEAD_MARGIN")
                .ok()
                .and_then(|v| v.parse().ok());
            w.players.set_collision_ahead(CollisionAhead {
                chunks_per_tick: chunks,
                margin_metres: margin.unwrap_or(CollisionAhead::default().margin_metres),
                ..CollisionAhead::default()
            });
            let sp = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, seed);
            let h = w.spawn_at(DVec3::new(sp[0], sp[1] + 2.0, sp[2]), 90.0);
            let i = match level {
                None => input(0.0, 1.0, true, false, false, 90.0, 0.0),
                Some(l) => fly(l, 90.0, false),
            };
            w.set_input(move |_, _| i);
            w.step(1);
            // Pre-generate the path's chunks, as the server's streaming would have: the timing is
            // the collider's, not the generator's.
            let pos = w.players.position(&w.physics, h);
            let home = chunk_of_pos(pos.to_array());
            for dx in -2..=40 {
                for dy in -3..=3 {
                    for dz in -3..=3 {
                        let c = ChunkCoord::new(home.x + dx, home.y + dy, home.z + dz);
                        let _ = w.world.read(c);
                    }
                }
            }
            let mut times = Vec::new();
            let (mut late, mut ahead_n) = (0, 0);
            let s0 = w.players.terrain().expect("terrain").stats();
            for _ in 0..600 {
                let t = std::time::Instant::now();
                w.players.tick_terrain(&mut w.world, &mut w.physics);
                times.push(t.elapsed().as_secs_f64() * 1e3);
                w.players.tick_controllers(&mut w.world, &mut w.physics);
                w.physics.step(1.0 / 60.0);
                let i2 = w.input_for(i);
                w.players.set_input(h, i2);
            }
            let s1 = w.players.terrain().expect("terrain").stats();
            late += s1.reach_loads - s0.reach_loads;
            ahead_n += s1.ahead_loads - s0.ahead_loads;
            let moved = (w.players.position(&w.physics, h) - pos).length();
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let over = |ms: f64| times.iter().filter(|&&t| t > ms).count();
            eprintln!(
                "{name:16} ahead {chunks}/tick: moved {moved:5.0} m, reached-unbuilt {late:3}, built ahead {ahead_n:3}; terrain ms/tick mean {mean:.3} p99 {:.3} max {:.3}; ticks >1 ms {} >2 ms {}",
                times[times.len() * 99 / 100],
                times[times.len() - 1],
                over(1.0),
                over(2.0),
            );
        }
    }
}
