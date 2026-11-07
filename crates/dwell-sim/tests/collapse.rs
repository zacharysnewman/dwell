//! Collapses on the server's tick (docs/FELLING.md, ARCHITECTURE.md §7.1, §7.3): the falling pieces
//! a dug pit's collapse or a blast makes must not stall the tick. The tests bound the work the
//! pieces cost (bodies alive and contact pairs, summed over the ticks: the same on every machine);
//! the measurements print wall times at seed 0's dam site and for a blast.

mod common;

use std::time::Instant;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, TICK_PHASES};

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

fn run_jobs(f: &mut Fixture, runner: &mut dwell_sim::jobs::JobRunner) {
    while let Some(job) = f.server.take_job() {
        let done = runner.run(&job).expect("a job");
        f.server.deliver_job(&done);
    }
}

/// What a collapse cost the server: its ticks until no body was left, and summed over them the
/// bodies alive and the physics' contact pairs touching (the narrow phase's and solver's work).
#[derive(Debug, Default)]
struct Cost {
    ticks: u32,
    body_ticks: usize,
    pair_ticks: usize,
    /// Each tick's wall time (ms), its phases' (`Server::tick_phases`) and the bodies after it.
    ms: Vec<f64>,
    phases: Vec<[(&'static str, f64); TICK_PHASES.len()]>,
    bodies: Vec<usize>,
}

/// Steps until no body is left (at most `limit` ticks), counting the cost; the server-jobs
/// worker's part (`runner`) run between ticks, off the clock.
fn run_out(
    f: &mut Fixture,
    mut runner: Option<&mut dwell_sim::jobs::JobRunner>,
    limit: u32,
) -> Cost {
    let mut cost = Cost::default();
    f.server.tick_phases();
    while f.server.body_count() > 0 && cost.ticks < limit {
        let t = Instant::now();
        f.server.step();
        cost.ms.push(t.elapsed().as_secs_f64() * 1000.0);
        cost.phases.push(f.server.tick_phases());
        cost.ticks += 1;
        cost.body_ticks += f.server.body_count();
        cost.bodies.push(f.server.body_count());
        cost.pair_ticks += f
            .server
            .physics()
            .rapier()
            .narrow_phase
            .contact_pairs()
            .filter(|p| p.has_any_active_contact())
            .count();
        f.take_all();
        if let Some(r) = runner.as_deref_mut() {
            run_jobs(f, r);
        }
    }
    cost
}

/// The ticks over 4 ms with their phases, then each second's tick mean and worst, its bodies and
/// its phases' sums.
fn report(cost: &Cost) {
    for (k, &ms) in cost.ms.iter().enumerate() {
        if ms > 4.0 {
            let phases: Vec<String> = cost.phases[k]
                .iter()
                .filter(|(_, v)| *v >= 0.5)
                .map(|(n, v)| format!("{n} {v:.1}"))
                .collect();
            println!("  tick {k}: {ms:.1} ms: {}", phases.join(", "));
        }
    }
    for (s, sec) in cost.ms.chunks(60).enumerate() {
        let max = sec.iter().copied().fold(0.0, f64::max);
        let mean = sec.iter().sum::<f64>() / sec.len() as f64;
        let mut sum = [0.0; TICK_PHASES.len()];
        for p in &cost.phases[s * 60..s * 60 + sec.len()] {
            for (k, (_, v)) in p.iter().enumerate() {
                sum[k] += v;
            }
        }
        let phases: Vec<String> = sum
            .iter()
            .enumerate()
            .filter(|(_, v)| **v >= 1.0)
            .map(|(k, v)| format!("{} {v:.0}", TICK_PHASES[k]))
            .collect();
        println!(
            "second {s}: tick mean {mean:.2} max {max:.2} ms, bodies {}; {}",
            cost.bodies[s * 60],
            phases.join(", ")
        );
    }
    println!(
        "{} ticks, {} body-ticks, {} contact-pair-ticks",
        cost.ticks, cost.body_ticks, cost.pair_ticks
    );
}

/// Seed 0's dam site (the bench's `?at=2364.5,142,7443.5`), as the browser's local server runs:
/// the chunks and LOD sections in the server-jobs worker, the water on its own thread.
fn dam_site() -> ServerConfig {
    ServerConfig {
        world_seed: 0,
        generator_version: dwell_world::GENERATOR_TERRAIN,
        spawn: Some([2364.5, 142.0, 7443.5]),
        worldgen_threads: 0,
        lod_threads: Some(0),
        pregen_radius_chunks: 3,
        water: dwell_sim::water::host::Placement::Thread,
        jobs_remote: true,
        ..ServerConfig::default()
    }
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test collapse measure_the_dam -- --ignored --nocapture"]
fn measure_the_dam_sites_pit_collapse() {
    // face-a-only: a measurement at seed 0's dam site, on face A.
    // As the bench's `dam-flood` (docs/WATER_SURFACE_GRID.md §2d): a stone wall 31 m across the
    // river 4 m ahead and a cut 31 × 29 m beside it, 5 m tall, its top 1.5 m under the player's
    // feet on the ground: the bank's sand over it falls, 177 columns 1.5 m tall packed against
    // the cut's walls (the browser's pit dropped its bank the same way).
    let mut f = Fixture::new(dam_site());
    let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_TERRAIN, 0);
    f.join(1, &Client::new(1));
    for _ in 0..120 {
        f.server.step();
        f.take_all();
        run_jobs(&mut f, &mut runner);
    }
    let (x, z) = (2364, 7443);
    let y = (0..40)
        .map(|d| 284 - d)
        .find(|&y| dwell_blocks::info(f.server.world().get_voxel(x, y - 1, z)).solid)
        .expect("the ground under the spawn");
    let stone = dwell_blocks::require("dwell:stone");
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x - 15, y - 4, z + 4],
            [31, 19, 1],
            stone,
        ),
    );
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x - 15, y - 13, z - 31],
            [31, 10, 29],
            0,
        ),
    );
    f.server.tick_phases();
    let t = Instant::now();
    f.server.step();
    let phases: Vec<String> = f
        .server
        .tick_phases()
        .iter()
        .filter(|(_, v)| *v >= 0.1)
        .map(|(n, v)| format!("{n} {v:.1}"))
        .collect();
    println!(
        "the feet's row {y}; the edits' tick: {:.2} ms ({}), {} bodies",
        t.elapsed().as_secs_f64() * 1000.0,
        phases.join(", "),
        f.server.body_count()
    );
    f.take_all();
    report(&run_out(&mut f, Some(&mut runner), 40 * 60));
}

/// A stone well on the flat world, its inside `n` × `n` cells, a floor `up` rows over the ground
/// holding sand `deep` rows deep: the floor goes (a region cleared), and the sand falls down the
/// well as `n²` columns, packed edge to edge and against the walls, as the dam site's pit dropped
/// its bank.
fn sand_down_a_well(n: i32, up: i32, deep: i32, limit: u32) -> (Cost, Fixture) {
    let g = dwell_world::FLAT_TOP_Y;
    let stone = dwell_blocks::require("dwell:stone");
    let sand = dwell_blocks::require("dwell:sand");
    let mut f = Fixture::new(flat());
    f.join(1, &Client::new(1));
    for _ in 0..30 {
        f.server.step();
    }
    let (x0, z0) = (3, 3);
    {
        let w = f.server.world();
        for y in g..g + up + deep + 2 {
            for z in z0 - 1..=z0 + n {
                for x in x0 - 1..=x0 + n {
                    let wall = x < x0 || z < z0 || x >= x0 + n || z >= z0 + n;
                    let m = if wall || y == g + up {
                        stone
                    } else if y > g + up && y <= g + up + deep {
                        sand
                    } else {
                        dwell_world::M.air
                    };
                    w.set_voxel(x, y, z, m);
                }
            }
        }
    }
    for _ in 0..10 {
        f.server.step();
    }
    f.take_all();
    f.send(
        1,
        region(
            RegionEditOp::Fill,
            [x0, g + up, z0],
            [n as u8, 1, n as u8],
            0,
        ),
    );
    f.server.step();
    f.take_all();
    assert_eq!(
        f.server.body_count(),
        (n * n) as usize,
        "every column falls"
    );
    let cost = run_out(&mut f, None, limit);
    (cost, f)
}

#[test]
fn sand_packed_in_a_well_falls_down_it_and_settles_in_seconds() {
    // face-a-only: the flat test world is face A's; the pieces' boxes are drawn in alike on every
    // axis and fall by the physics' face gravity (tested in dwell-physics on both faces).
    // Seed 0's dam site, 2026-11-06: a pit dug under a river bank dropped 177 sand columns that
    // lay packed edge to edge against the pit's walls. Their boxes touched exactly and the solver
    // held the whole raft by its walls' friction: it crept down at ~0.2 m/s, every column a body
    // stepped on the server's tick (4.7 ms of each natively, 27–45 ms in the browser) for the
    // 30 s until `maxSeconds` settled it in the air. Here 256 columns 3 m tall fall 6 m down a
    // well: all must be blocks again at its bottom within 6 s, and the bodies and contacts they
    // cost stay under bounds (a column's three cells are one box). Red before: 256 bodies left
    // at 6 s, 92,160 body-ticks, the sand ~1 m down.
    let (n, up, deep) = (16, 12, 6);
    let (cost, mut f) = sand_down_a_well(n, up, deep, 6 * 60);
    let left = f.server.body_count();
    let g = dwell_world::FLAT_TOP_Y;
    let sand = dwell_blocks::require("dwell:sand");
    let w = f.server.world();
    let mut low = 0;
    for y in g..g + deep {
        for z in 3..3 + n {
            for x in 3..3 + n {
                low += usize::from(w.get_voxel(x, y, z) == sand);
            }
        }
    }
    let mean = cost.ms.iter().sum::<f64>() / cost.ms.len().max(1) as f64;
    println!(
        "{} ticks ({mean:.2} ms mean), {} body-ticks, {} contact-pair-ticks, {left} left, {low} sand at the bottom",
        cost.ticks, cost.body_ticks, cost.pair_ticks
    );
    assert_eq!(left, 0, "every column settled within 6 s");
    assert!(
        low >= (n * n * deep) as usize * 9 / 10,
        "the sand lies at the well's bottom: {low} of {}",
        n * n * deep
    );
    assert!(
        cost.body_ticks <= 50_000,
        "the columns were bodies {} body-ticks (at most 50,000: ~3 s each)",
        cost.body_ticks
    );
    assert!(
        cost.pair_ticks <= 150_000,
        "the columns' contacts cost {} pair-ticks (at most 150,000)",
        cost.pair_ticks
    );
}

/// Dynamite's shape (docs/FUTURE.md: every block in a radius flung as a piece): every cell of a
/// ball of `radius` m 1 m over the flat world's ground, each its own natural body, flung out from
/// its centre at up to `speed` m/s; as the browser's local server runs (the chunks and LOD
/// sections in their worker).
fn blast(radius: i32, speed: f64, limit: u32) -> Cost {
    let stone = dwell_blocks::require("dwell:stone");
    let mut f = Fixture::new(ServerConfig {
        jobs_remote: true,
        lod_threads: Some(0),
        worldgen_threads: 0,
        ..flat()
    });
    let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_FLAT, 0);
    f.join(1, &Client::new(1));
    for _ in 0..30 {
        f.server.step();
        run_jobs(&mut f, &mut runner);
    }
    f.take_all();
    let centre = [0.5, dwell_world::FLAT_TOP_M + f64::from(radius) + 1.0, 40.5];
    for dy in -2 * radius..=2 * radius {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let o = dwell_physics::DVec3::new(
                    f64::from(dx),
                    f64::from(dy) * dwell_world::CELL_H,
                    f64::from(dz),
                );
                let d = o.length();
                if d > f64::from(radius) {
                    continue;
                }
                let at = [centre[0] + o.x, centre[1] + o.y, centre[2] + o.z];
                let id = f.server.add_body(0, vec![([0, 0, 0], stone)], at);
                let h = f.server.body_handle(id).expect("a body");
                let out = o / d.max(0.5);
                f.server
                    .physics()
                    .set_velocity(h, out * speed * (0.5 + 0.5 * d / f64::from(radius)));
            }
        }
    }
    println!("{} bodies", f.server.body_count());
    run_out(&mut f, Some(&mut runner), limit)
}

#[test]
fn a_blasts_pieces_land_in_a_heap_and_settle_in_seconds() {
    // face-a-only: the flat test world is face A's (the settle rule has no up).
    // Dynamite (queued, owner 2026-11-06) flings every block in a radius as a piece. In a heap
    // each piece settling nudged those resting on it, which then counted their still time again,
    // and every settle rebuilt the terrain collision of the 27 chunks round it: 513 pieces of a
    // 4 m ball took over 10 s to be blocks again. Pieces that haven't gone anywhere
    // (`restDrift`) count as still, and settle after 1 s (`sleepSeconds`), together every 0.1 s
    // (`settleEveryTicks`). Red before: 108 bodies left at 10 s, 205,388 body-ticks by then;
    // green: none left by 11.1 s, 121,389 in all.
    let cost = blast(4, 8.0, 12 * 60);
    let left = *cost.bodies.last().expect("ticks");
    println!(
        "{} ticks, {} body-ticks, {} contact-pair-ticks, {left} left",
        cost.ticks, cost.body_ticks, cost.pair_ticks
    );
    assert!(left == 0, "{left} bodies still moving after 12 s");
    assert!(
        cost.body_ticks <= 130_000,
        "the blast's pieces were bodies {} body-ticks (at most 130,000)",
        cost.body_ticks
    );
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test collapse measure_a_blast -- --ignored --nocapture"]
fn measure_a_blast() {
    // face-a-only: a measurement on the flat world (face A's).
    let radius = std::env::var("RADIUS").map_or(4, |v| v.parse().expect("a radius"));
    report(&blast(radius, 8.0, 20 * 60));
}

/// Dynamite lit at seed 0's dam site (the browser check's), 1.5 m ahead of the spawn: the world
/// after its blast's tick, and that tick's wall time (ms) and phases.
fn dam_blast() -> (Fixture, dwell_sim::jobs::JobRunner, f64, String) {
    let mut f = Fixture::new(dam_site());
    let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_TERRAIN, 0);
    f.join(1, &Client::new(1));
    for _ in 0..120 {
        f.server.step();
        f.take_all();
        run_jobs(&mut f, &mut runner);
    }
    let (x, z) = (2364, 7445);
    let y = (0..40)
        .map(|d| 290 - d)
        .find(|&y| dwell_blocks::info(f.server.world().get_voxel(x, y - 1, z)).solid)
        .expect("the ground ahead of the spawn");
    f.server
        .world()
        .set_voxel(x, y, z, dwell_blocks::require("dwell:dynamite"));
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [x, y, z],
            face: 5,
            material: dwell_blocks::require("dwell:torch"),
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    f.take_all();
    // The player steps 10 m back (as the browser check's did) and watches.
    let h = f.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = f.server.sim();
    let at = players.position(physics, h);
    players.teleport(
        physics,
        h,
        at - dwell_physics::DVec3::new(0.0, 0.0, 10.0) + dwell_physics::DVec3::Y,
        dwell_physics::Vec3::ZERO,
    );
    let fuse = f.server.fuses().first().expect("lit").1;
    for _ in 1..fuse {
        f.server.step();
        f.take_all();
        run_jobs(&mut f, &mut runner);
    }
    f.server.tick_phases();
    let t = Instant::now();
    f.server.step();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let phases: Vec<String> = f
        .server
        .tick_phases()
        .iter()
        .filter(|(_, v)| *v >= 0.1)
        .map(|(n, v)| format!("{n} {v:.1}"))
        .collect();
    f.take_all();
    (f, runner, ms, phases.join(", "))
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test collapse measure_dynamite_at_the_dam -- --ignored --nocapture"]
fn measure_dynamite_at_the_dam() {
    // face-a-only: a measurement at seed 0's dam site, on face A.
    let (mut f, mut runner, ms, phases) = dam_blast();
    println!(
        "the blast's tick: {ms:.2} ms ({phases}), {} bodies",
        f.server.body_count()
    );
    report(&run_out(&mut f, Some(&mut runner), 30 * 60));
}
