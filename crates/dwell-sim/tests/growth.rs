//! Trees growing over time on the server (ADR 0079 S3): the growers step grows a recorded tree by
//! the world's days, never over a player's edit, never once felled, and the records survive a save.

mod common;

use common::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};
use dwell_worldgen::growth::{Record, cells_at};
use dwell_worldgen::species::{SPECIES, index};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        // A world day a tick, so the test sets the days it wants.
        growth_speed: 3600.0,
        ..flat()
    }
}

/// An oak at stage 2 on face `f`, 6 m from the spawn.
fn oak(f: FaceFrame) -> Record {
    Record {
        anchor: f.cell([6, BASE, 0]),
        face: f.face,
        species: index("dwell:oak").unwrap() as u16,
        seed: 11,
        stage: 2,
        since: 0.0,
        turns: 0,
        planter: 0,
        variant: 0,
        stopped: false,
    }
}

fn per() -> f64 {
    // Its own pace (owner, 2026-11-15: each tree a different growth rate).
    let s = &SPECIES[index("dwell:oak").unwrap()];
    f64::from(s.days_per_stage)
        / f64::from(dwell_worldgen::species::rate(s, oak(FaceFrame::A).seed))
}

/// A server with the oak's stage-2 cells in its world and its record due `n` stages.
fn scene(f: FaceFrame, cfg: ServerConfig, n: f64) -> (Fixture, Record) {
    let mut fx = Fixture::new(cfg);
    let mut r = oak(f);
    for _ in 0..3 {
        fx.server.step();
    }
    for ([x, y, z], m) in cells_at(&r, 2) {
        fx.server.world().set_voxel(x, y, z, m);
    }
    r.since = fx.server.world_days() - per() * n - 0.01;
    fx.server.plant_tree(r.clone());
    (fx, r)
}

fn cells(fx: &mut Fixture, of: &[([i32; 3], u16)]) -> Vec<u16> {
    of.iter()
        .map(|([x, y, z], _)| fx.server.world().get_voxel(*x, *y, *z))
        .collect()
}

dwell_world::both_faces!(
    fn a_tree_grows_its_due_stages_by_the_world_days(f: FaceFrame) {
        let (mut fx, r) = scene(f, config(f), 3.0);
        fx.server.grow_trees_now();
        let got = fx.server.growth().trees[&r.anchor].clone();
        assert_eq!(got.stage, 5, "three stages after three stages' days");
        let five = cells_at(&r, 5);
        let want: Vec<u16> = five.iter().map(|(_, m)| *m).collect();
        assert_eq!(
            cells(&mut fx, &five),
            want,
            "the world holds stage 5's tree"
        );
        // Nothing more is due yet.
        fx.server.grow_trees_now();
        assert_eq!(fx.server.growth().trees[&r.anchor].stage, 5);
    }
);

dwell_world::both_faces!(
    fn a_player_edit_stands_and_a_felled_tree_stops(f: FaceFrame) {
        let (mut fx, r) = scene(f, config(f), 3.0);
        let stone = f.state(dwell_blocks::require("dwell:stone"));
        let two: Vec<[i32; 3]> = cells_at(&r, 2).into_iter().map(|(c, _)| c).collect();
        let built = cells_at(&r, 5)
            .into_iter()
            .map(|(c, _)| c)
            .find(|c| !two.contains(c))
            .unwrap();
        fx.server
            .world()
            .set_voxel(built[0], built[1], built[2], stone);
        fx.server.grow_trees_now();
        assert_eq!(
            fx.server.world().get_voxel(built[0], built[1], built[2]),
            stone
        );

        // A second tree, felled at its foot before it is due: it never grows.
        let (mut fx, r) = scene(f, config(f), 3.0);
        let a = r.anchor;
        fx.server
            .world()
            .set_voxel(a[0], a[1], a[2], dwell_world::M.air);
        fx.server.grow_trees_now();
        let got = fx.server.growth().trees[&a].clone();
        assert!(got.stopped);
        assert_eq!(got.stage, 2);
    }
);

#[test]
fn the_growth_records_survive_a_save() {
    // face-a-only: a byte format through the store.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    assert_eq!(db.tree_growth(), None, "an older world has none");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f)
    };
    let anchor = {
        let (mut fx, r) = scene(f, cfg(), 2.0);
        fx.server.grow_trees_now();
        fx.server.save_now();
        fx.server.flush_store();
        r.anchor
    };
    assert!(db.tree_growth().is_some(), "saved");
    let fx = Fixture::new(cfg());
    let got = &fx.server.growth().trees[&anchor];
    assert_eq!(got.stage, 4, "the stage kept");
}

dwell_world::both_faces!(
    /// BUGS.md #194 (owner, 2026-11-16: "a periodic stutter ... there's almost always a stutter
    /// going through the portal"): the growers looked at up to 256 newly loaded chunks for their
    /// trees in one tick every 5 s (~0.25 ms each: ~60 ms), a stall after every big load, crossing
    /// a portal above all. Now a tick looks at a few, every tick.
    fn the_growers_look_at_new_chunks_a_few_a_tick(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.server
            .set_growers(dwell_worldgen::growth::Growers::for_generator(
                dwell_world::GENERATOR_TERRAIN,
                7,
            ));
        let mut most = 0;
        // A new column takes four ticks (a quarter a tick): 100 columns in ~7 s.
        for _ in 0..(8 * 60) {
            for dx in -5..5 {
                for dz in -5..5 {
                    let [x, y, z] = f.cell([dx * 32 + 4, BASE, dz * 32 + 4]);
                    fx.server
                        .world()
                        .get_or_create(dwell_world::chunk_of(x, y, z));
                }
            }
            let before = fx.server.tree_chunks_scanned();
            fx.server.step();
            most = most.max(fx.server.tree_chunks_scanned() - before);
        }
        assert!(most <= 1, "at most one chunk a tick: {most}");
        assert!(
            fx.server.tree_chunks_scanned() >= 100,
            "all of them in turn"
        );
    }
);

dwell_world::both_faces!(
    /// Priority 0 (BUGS.md #194; owner, 2026-11-16: "big hitches while flying at 362 m/s"): the
    /// scan places a chunk column's trees once for all its chunks (the placements cost ~1–5 ms
    /// natively a column, the same for every chunk stacked in it), not once a chunk.
    fn the_scan_places_a_column_once_for_its_stacked_chunks(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.server
            .set_growers(dwell_worldgen::growth::Growers::for_generator(
                dwell_world::GENERATOR_TERRAIN,
                7,
            ));
        for dy in -3..3 {
            let [x, y, z] = f.cell([4, BASE + dy * 32, 4]);
            fx.server
                .world()
                .get_or_create(dwell_world::chunk_of(x, y, z));
        }
        for _ in 0..120 {
            fx.server.step();
        }
        let chunks = fx.server.tree_chunks_scanned();
        let columns = fx.server.tree_columns_scanned();
        assert!(
            chunks > columns,
            "stacked chunks scanned: {chunks} in {columns} columns"
        );
        assert_eq!(
            fx.server.tree_columns_placed(),
            columns,
            "one placement a column ({chunks} chunks)"
        );
    }
);

dwell_world::both_faces!(
    /// Priority 0 (BUGS.md #194): a tick places at most a quarter of a new column's trees (a
    /// column's placements cost ~5 ms natively, ~15–20 ms in the browser), the scan still covering
    /// every chunk in turn.
    fn the_scan_places_a_quarter_column_a_tick(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.server
            .set_growers(dwell_worldgen::growth::Growers::for_generator(
                dwell_world::GENERATOR_TERRAIN,
                7,
            ));
        for dx in -2..2 {
            for dz in -2..2 {
                let [x, y, z] = f.cell([dx * 32 + 4, BASE, dz * 32 + 4]);
                fx.server
                    .world()
                    .get_or_create(dwell_world::chunk_of(x, y, z));
            }
        }
        let mut most = 0;
        for _ in 0..400 {
            let before = fx.server.tree_quarters_placed();
            fx.server.step();
            most = most.max(fx.server.tree_quarters_placed() - before);
        }
        assert!(most <= 1, "{most} quarter columns placed in one tick");
        let (placed, columns) = (
            fx.server.tree_quarters_placed(),
            fx.server.tree_columns_scanned(),
        );
        // Every scanned column placed whole, once (and at most the next one part way).
        assert!(
            (4 * columns..4 * columns + 4).contains(&placed),
            "{placed} quarters for {columns} columns"
        );
    }
);

dwell_world::both_faces!(
    /// Priority 0 (BUGS.md #194): with the browser's server-jobs worker (`jobs_remote`), a new
    /// column's tree placements are its job ("GJ1"), never the tick's (a quarter column was still
    /// ~4–6 ms natively, ~15–20 ms in the browser); its chunks' trees are recorded once it answers.
    fn a_remote_server_places_tree_columns_in_its_jobs_worker(f: FaceFrame) {
        let mut fx = Fixture::new(ServerConfig {
            jobs_remote: true,
            ..config(f)
        });
        fx.server
            .set_growers(dwell_worldgen::growth::Growers::for_generator(
                dwell_world::GENERATOR_TERRAIN,
                7,
            ));
        let mut runner = dwell_sim::jobs::JobRunner::for_world(dwell_world::GENERATOR_TERRAIN, 7);
        for dx in -2..2 {
            for dz in -2..2 {
                let [x, y, z] = f.cell([dx * 32 + 4, BASE, dz * 32 + 4]);
                fx.server
                    .world()
                    .get_or_create(dwell_world::chunk_of(x, y, z));
            }
        }
        for _ in 0..240 {
            fx.server.step();
            // Only the tree-column jobs answered (this world's own chunks are the test's).
            while let Some(job) = fx.server.take_job() {
                if job.starts_with(b"GJ1")
                    && let Some(done) = runner.run(&job)
                {
                    assert!(fx.server.deliver_job(&done), "a column's answer read");
                }
            }
        }
        assert_eq!(
            fx.server.tree_quarters_placed(),
            0,
            "columns placed on the tick"
        );
        assert!(
            fx.server.tree_chunks_scanned() >= 16,
            "the chunks' trees recorded from the jobs' columns: {}",
            fx.server.tree_chunks_scanned()
        );
    }
);
