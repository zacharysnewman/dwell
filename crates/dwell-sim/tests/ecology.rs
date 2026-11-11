//! The far ecology on the server (docs/ECOLOGY.md §6): both faces' herds and packs stepped by the
//! world's days, saved with the world (an optional table), caught up after a load in bounded steps,
//! and none in a world that opts out.

mod common;

use common::*;
use dwell_ecology::{DATA, Ecosystem, Ground, Island};
use dwell_protocol::SIM_HZ;
use dwell_sim::server::{MAX_STEPS_PER_TICK, ServerConfig, default_settings, next_step};
use dwell_storage::{SaveBatch, WorldDb, WorldMeta, WorldStore};
use dwell_world::{Face, FaceFrame};
use std::sync::Arc;

/// A round island 3.6 km across (the ecology tests' own), on both faces.
fn islands() -> Arc<[Island; 2]> {
    let n = 24;
    let x0 = -(n as f64 - 1.0) * DATA.island.cell_m / 2.0;
    let isl = Island::from_ground(&DATA, n, x0, |x, z| {
        if x.hypot(z) > 1800.0 {
            Ground::Water
        } else {
            Ground::Land {
                height_m: 300.0 * (1.0 - x.hypot(z) / 1800.0),
                slope_deg: (300.0f64 / 1800.0).atan().to_degrees(),
            }
        }
    });
    Arc::new([isl.clone(), isl])
}

/// The world clock's seconds a day.
fn day_s() -> f64 {
    default_settings(1.0).clock.day_length_s()
}

/// The ecology speed that makes a tick a world day.
fn a_day_a_tick() -> f64 {
    day_s() * f64::from(SIM_HZ)
}

fn config() -> ServerConfig {
    ServerConfig {
        world_seed: 77,
        ecology_islands: Some(islands()),
        ecology_speed: a_day_a_tick(),
        ..flat()
    }
}

fn ticks(fx: &mut Fixture, n: u32) {
    for _ in 0..n {
        fx.server.step();
    }
}

fn eco(fx: &Fixture, f: FaceFrame) -> &Ecosystem {
    fx.server
        .ecosystem(f.face)
        .expect("the face's ecology runs")
}

#[test]
fn the_schedule_is_whole_days_big_steps_to_catch_up_and_waits_when_idle() {
    let big = DATA.island.step_days;
    assert_eq!(next_step(0.9, true, big), None, "less than a day behind");
    assert_eq!(
        next_step(1.2, true, big),
        Some(1.0),
        "online: a day at a time"
    );
    assert_eq!(next_step(5.0, false, big), None, "idle: waits");
    assert_eq!(
        next_step(12.0, false, big),
        Some(big.floor()),
        "idle: big steps"
    );
    assert_eq!(
        next_step(1000.0, true, big),
        Some(big.floor()),
        "catching up"
    );
    assert_eq!(
        next_step(11.0, false, 30.0),
        Some(11.0),
        "never past the clock"
    );
}

dwell_world::both_faces!(
    fn the_server_steps_each_faces_ecology_by_the_world_days(f: FaceFrame) {
        // Online (a player joined): a world day a tick, the ecology a day at a time behind it.
        let mut fx = Fixture::new(config());
        fx.join(1, &Client::new(1));
        ticks(&mut fx, 3);
        let start = eco(&fx, f).clone();
        assert_eq!(
            start.seed,
            dwell_worldgen::bifacial::face_seed(77, f.face),
            "each face's own seed"
        );
        ticks(&mut fx, 40);
        let e = eco(&fx, f);
        let clock = fx.server.ecology_clock_days();
        assert!(
            e.day <= clock && e.day > clock - 2.0 && e.day.fract() == 0.0,
            "day {} by clock {clock}",
            e.day
        );
        assert_ne!(e.digest(), start.digest(), "the herds moved and ate");
        let snap = fx.server.ecology_snapshot();
        assert!(snap.enabled && snap.running);
        let face = f.face;
        let mine: Vec<_> = snap.groups.iter().filter(|g| g.face == face).collect();
        assert_eq!(mine.len(), e.groups.len());
        assert!(mine.iter().any(|g| g.species == "dwell:deer"));
        assert!(mine.iter().any(|g| g.species == "dwell:wolf"));
        for g in &mine {
            assert!(g.size > 0.0 && g.x.is_finite() && !g.activity.is_empty());
        }

        // Nobody online: it waits, then catches up in big steps, never past the clock.
        let mut idle = Fixture::new(config());
        ticks(&mut idle, 9);
        assert_eq!(eco(&idle, f).day, 0.0, "idle under ten days: waiting");
        ticks(&mut idle, 40);
        let (e, clock) = (eco(&idle, f), idle.server.ecology_clock_days());
        assert!(
            e.day <= clock && e.day >= clock - 10.0,
            "{} by {clock}",
            e.day
        );
    }
);

/// The ecology's state on both faces.
fn digests(fx: &Fixture) -> [u64; 2] {
    [Face::A, Face::B].map(|f| fx.server.ecosystem(f).expect("runs").digest())
}

#[test]
fn save_load_and_continue_matches_an_uninterrupted_run() {
    // both-faces: it compares both faces' ecosystems at once.
    let mut straight = Fixture::new(config());
    ticks(&mut straight, 300);
    let want = digests(&straight);

    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config()
    };
    let saved = {
        let mut fx = Fixture::new(cfg());
        ticks(&mut fx, 150);
        fx.server.save_now();
        fx.server.flush_store();
        [Face::A, Face::B].map(|f| fx.server.ecosystem(f).cloned().expect("runs"))
    };
    assert!(db.ecology().is_some(), "saved in its table");
    let mut fx = Fixture::new(cfg());
    ticks(&mut fx, 1);
    for (k, f) in [Face::A, Face::B].into_iter().enumerate() {
        assert_eq!(
            fx.server.ecosystem(f),
            Some(&saved[k]),
            "face {f:?} loaded as saved"
        );
    }
    ticks(&mut fx, 149);
    assert_eq!(digests(&fx), want, "the same days as the run never saved");
}

#[test]
fn an_older_world_starts_a_fresh_ecology_and_catches_up_a_thousand_days_in_bounded_ticks() {
    // both-faces: it checks both faces' ecosystems at once.
    // An older build's save: its meta (a clock 1000 world days on) and no ecology table.
    let db = WorldDb::in_memory().expect("db");
    let days = 1000.0;
    db.save(&SaveBatch {
        meta: Some(WorldMeta {
            world_seed: 77,
            generator_version: dwell_world::GENERATOR_FLAT,
            spawn: Some([0.5, dwell_world::FLAT_TOP_M, 0.5]),
            world_tick: (days * day_s() * f64::from(SIM_HZ)).ceil() as u32 + 1,
            created_at: 0,
            saved_at: 0,
            app_version_created: "0.50.0".into(),
            app_version_last: "0.50.0".into(),
            settings: None,
        }),
        ..SaveBatch::default()
    })
    .expect("older save");
    assert_eq!(db.ecology(), None, "an older world has none");
    let mut fx = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ecology_speed: 1.0,
        ..config()
    });
    let clock = fx.server.ecology_clock_days();
    assert!(clock > days, "{clock}");
    let isl = islands();
    let caps = [DATA.find("dwell:deer"), DATA.find("dwell:wolf")]
        .map(|s| s.expect("in the data"))
        .map(|s| (s, isl[0].carrying_capacity(&DATA, s)));
    let mut ticked = 0;
    let mut worst_ms = 0.0f64;
    loop {
        fx.server.step();
        ticked += 1;
        let (steps, ms, _) = fx.server.ecology_stats();
        assert!(steps <= MAX_STEPS_PER_TICK);
        worst_ms = worst_ms.max(ms);
        for f in [Face::A, Face::B] {
            let e = fx
                .server
                .ecosystem(f)
                .expect("a fresh ecology from the seed");
            for &(s, cap) in &caps {
                assert!(
                    e.population(s) <= cap,
                    "{f:?} day {}: over its capacity",
                    e.day
                );
            }
            for g in &e.groups {
                let r = DATA.species[g.species].group;
                assert!(g.size >= r.die_below && g.size <= r.cap + 1e-9);
            }
        }
        if fx.server.ecosystem(Face::A).expect("runs").day >= days - 10.0 {
            break;
        }
        assert!(ticked < 200, "still catching up after {ticked} ticks");
    }
    // A thousand days in big steps: MAX_STEPS_PER_TICK a tick, so tens of ticks, each cheap.
    let least = (days - 10.0) / (DATA.island.step_days * f64::from(MAX_STEPS_PER_TICK));
    assert!(f64::from(ticked) >= least.floor(), "{ticked} ticks");
    // The tick's budget is 16 ms; the ecology's share stays a small part of it (a generous bound
    // for an unoptimised test build).
    assert!(
        worst_ms < 8.0,
        "the worst tick's ecology took {worst_ms} ms"
    );
    let e = fx.server.ecosystem(Face::A).expect("runs");
    assert_eq!(e.seed, 77, "face A's seed is the world's");
    assert!(e.population(caps[0].0) > 0.0);
}

#[test]
fn a_world_that_opts_out_has_no_ecology_and_keeps_its_choice() {
    // face-a-only: no ecology on either face; the switch is the world's.
    let db = WorldDb::in_memory().expect("db");
    {
        let mut fx = Fixture::new(ServerConfig {
            store: Some(WorldStore::new(db.clone(), false)),
            ecology: false,
            ..config()
        });
        ticks(&mut fx, 60);
        let snap = fx.server.ecology_snapshot();
        assert!(!snap.enabled && !snap.running && snap.groups.is_empty());
        assert!(fx.server.ecosystem(Face::A).is_none());
        fx.server.save_now();
        fx.server.flush_store();
    }
    // Opened by a build whose new worlds have an ecology: the world keeps its own choice.
    let mut fx = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config()
    });
    ticks(&mut fx, 60);
    assert!(!fx.server.ecology_snapshot().enabled);
    assert!(fx.server.ecosystem(Face::B).is_none());
}

#[test]
fn a_world_without_a_map_has_no_ecology_to_run() {
    // face-a-only: the flat world has no world map on either face.
    let mut fx = Fixture::new(flat());
    ticks(&mut fx, 30);
    let snap = fx.server.ecology_snapshot();
    assert!(snap.enabled && !snap.running);
}

#[test]
fn an_unreadable_record_is_kept_and_not_stepped() {
    // face-a-only: a byte record through the store.
    let db = WorldDb::in_memory().expect("db");
    {
        let fx = Fixture::new(ServerConfig {
            store: Some(WorldStore::new(db.clone(), false)),
            ..config()
        });
        drop(fx);
    }
    db.save(&SaveBatch {
        ecology: Some(vec![99, 1, 2, 3]),
        ..SaveBatch::default()
    })
    .expect("a newer build's record");
    let mut fx = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config()
    });
    ticks(&mut fx, 30);
    assert!(fx.server.ecosystem(Face::A).is_none(), "not stepped");
    fx.server.save_now();
    fx.server.flush_store();
    assert_eq!(db.ecology(), Some(vec![99, 1, 2, 3]), "kept as it was");
}
