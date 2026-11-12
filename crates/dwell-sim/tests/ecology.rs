//! The far ecology on the server (docs/ECOLOGY.md §6): both faces' herds and packs stepped by the
//! world's days, saved with the world (an optional table), caught up after a load in bounded steps,
//! and none in a world that opts out.

mod common;

use common::*;
use dwell_ecology::{DATA, Ecosystem, Ground, Island};
use dwell_protocol::SIM_HZ;
use dwell_sim::server::{
    ECOLOGY_SEND_EVERY, MAX_STEPS_PER_TICK, ServerConfig, default_settings, next_step,
};
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

// --- slice 3: individuals near players (docs/ECOLOGY.md §8) ---

const BASE: i32 = dwell_world::FLAT_TOP_Y + 40;

/// A stone floor around face-local (8, 8), a player standing on it, the ecology running at the
/// world's own pace and face `f`'s records cut to one herd of `size` at (8, 12) (the other face's
/// none).
fn near_scene(f: FaceFrame, ecology: bool, size: f64) -> Fixture {
    near_scene_in(f, ecology, size, None)
}

fn near_scene_in(f: FaceFrame, ecology: bool, size: f64, store: Option<WorldStore>) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        store,
        generator_override: Some(dwell_world::empty_generator()),
        spawn: Some(f.point([0.5, dwell_world::m_of_row(BASE), 0.5])),
        ecology,
        ecology_speed: 1.0,
        ..config()
    });
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    for x in -30..50 {
        for z in -30..50 {
            for r in -2..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    fx.join(1, &Client::new(1));
    move_player(&mut fx, f, 8.5, 8.5);
    ticks(&mut fx, 2);
    for face in [Face::A, Face::B] {
        if let Some(e) = fx.server.ecosystem_mut(face) {
            let deer = DATA.find("dwell:deer").unwrap();
            e.groups.retain(|g| g.species == deer);
            e.groups.truncate(usize::from(face == f.face));
            for g in &mut e.groups {
                (g.x, g.z, g.size, g.hunger) = (8.0, 12.0, size, 0.6);
            }
        }
    }
    fx.take_all();
    fx
}

fn move_player(fx: &mut Fixture, f: FaceFrame, x: f64, z: f64) {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = players.half_height(h) as f64;
    let c = f.point([x, dwell_world::m_of_row(BASE) + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(c),
        dwell_physics::Vec3::ZERO,
    );
}

dwell_world::both_faces!(
    fn a_herd_near_a_player_becomes_deer_and_folds_back_when_they_leave(f: FaceFrame) {
        let mut fx = near_scene(f, true, 4.0);
        ticks(&mut fx, 40);
        let herds = fx.server.herds();
        assert_eq!(herds.groups.len(), 1, "the herd came near");
        assert_eq!(fx.server.ecosystem(f.face).unwrap().groups.len(), 0);
        let deer: Vec<_> = fx.server.creatures().iter().filter(|c| c.driven).collect();
        assert_eq!(deer.len(), 4, "its four deer, through the creatures");
        for c in &deer {
            let l = f.point(c.position);
            assert!(
                (l[1] - dwell_world::m_of_row(BASE)).abs() < 1e-9,
                "on the floor: {l:?}"
            );
        }
        // No rule-spawned deer or wolves beside them.
        assert!(fx.server.creatures().iter().all(|c| c.driven));
        ticks(&mut fx, 600);
        let g = fx.server.herds().groups[0].clone();
        let head = 4 + g.births - g.deaths;
        move_player(&mut fx, f, 8.5, 400.0);
        ticks(&mut fx, 40);
        assert!(fx.server.herds().groups.is_empty(), "folded back");
        assert!(fx.server.creatures().is_empty(), "the deer are gone");
        let e = fx.server.ecosystem(f.face).unwrap();
        assert_eq!(e.groups.len(), 1);
        assert!(
            e.groups[0].size >= f64::from(head),
            "the head kept: {}",
            e.groups[0].size
        );
    }
);

dwell_world::both_faces!(
    fn an_opted_out_world_spawns_no_individuals(f: FaceFrame) {
        let mut fx = near_scene(f, false, 4.0);
        ticks(&mut fx, 120);
        assert!(fx.server.herds().groups.is_empty());
        assert!(fx.server.creatures().iter().all(|c| !c.driven));
    }
);

dwell_world::both_faces!(
    fn a_save_while_the_herd_is_near_keeps_it_as_it_would_fold_back(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let mut fx = near_scene_in(f, true, 3.0, Some(WorldStore::new(db.clone(), false)));
        ticks(&mut fx, 40);
        assert_eq!(fx.server.herds().groups.len(), 1);
        let head = fx.server.herds().groups[0].members.len();
        fx.server.save_now();
        fx.server.flush_store();
        let mut back = Fixture::new(ServerConfig {
            store: Some(WorldStore::new(db.clone(), false)),
            ecology_speed: 1.0,
            ..config()
        });
        ticks(&mut back, 1);
        let e = back.server.ecosystem(f.face).expect("loaded");
        assert_eq!(e.groups.len(), 1, "the near herd was saved");
        assert_eq!(e.groups[0].size, head as f64);
    }
);

#[test]
fn night_is_each_faces_own() {
    // both-faces: it compares the two faces' nights.
    let clock = default_settings(1.0).clock;
    let len = clock.day_length_s();
    // World time of face A's noon and midnight.
    let at = |t: f64| ((t - f64::from(clock.start_hour) / 24.0).rem_euclid(1.0)) * len;
    use dwell_sim::server::night_on;
    assert!(!night_on(clock, at(0.5), Face::A) && night_on(clock, at(0.5), Face::B));
    assert!(night_on(clock, at(0.0), Face::A) && !night_on(clock, at(0.0), Face::B));
}

/// The `Ecology` messages each client got.
fn ecology_told(
    fx: &mut Fixture,
) -> std::collections::BTreeMap<u32, Vec<dwell_protocol::EcologyMessage>> {
    fx.take_all()
        .into_iter()
        .map(|(id, msgs)| {
            let e = msgs
                .into_iter()
                .filter_map(|m| match m {
                    dwell_protocol::Message::Ecology(e) => Some(e),
                    _ => None,
                })
                .collect();
            (id, e)
        })
        .collect()
}

dwell_world::both_faces!(
    fn a_client_that_asks_gets_the_far_groups_at_a_low_rate_and_others_never(f: FaceFrame) {
        let mut fx = Fixture::new(config());
        fx.join(1, &Client::new(1));
        fx.join(2, &Client::new(2));
        // The server says it takes the hello (its Inventory's flag).
        fx.send(
            1,
            dwell_protocol::Message::InventoryAction(dwell_protocol::InventoryAction::Hello),
        );
        ticks(&mut fx, 3);
        let flags = fx.take_all()[&1]
            .iter()
            .find_map(|m| match m {
                dwell_protocol::Message::Inventory(i) => Some(i.flags),
                _ => None,
            })
            .expect("an Inventory");
        assert_ne!(flags & dwell_protocol::inventory_flags::ECOLOGY, 0);
        // Client 1 asks (a build that reads the ecology); client 2 is an older one that doesn't.
        fx.send(1, dwell_protocol::Message::EcologyHello);
        let now = ecology_told(&mut fx);
        assert_eq!(
            now.get(&1).map(Vec::len),
            Some(1),
            "the far groups at the hello"
        );
        assert!(now.get(&2).is_none_or(Vec::is_empty));
        // Then once every ECOLOGY_SEND_EVERY ticks, to it only; each as the server's far groups
        // were at the tick it was sent.
        let mut mine = Vec::new();
        let mut at_send = Vec::new();
        for _ in 0..2 * ECOLOGY_SEND_EVERY {
            ticks(&mut fx, 1);
            let told = ecology_told(&mut fx);
            assert!(
                told.get(&2).is_none_or(Vec::is_empty),
                "never to a client that didn't ask"
            );
            if let Some(m) = told.get(&1).filter(|m| !m.is_empty()) {
                mine.extend(m.iter().cloned());
                let feet = fx
                    .server
                    .player_of_session(1)
                    .and_then(|p| fx.server.feet_of(p));
                at_send.push((
                    fx.server.ecology_message_from(feet).unwrap(),
                    fx.server.ecology_snapshot(),
                    feet.expect("client 1 plays"),
                ));
            }
        }
        assert_eq!(mine.len(), 2, "every {ECOLOGY_SEND_EVERY} ticks");
        let m = mine.last().unwrap();
        let (now, snap, feet) = at_send.last().unwrap();
        assert_eq!(m, now);
        // Its own nearest first (WORK_QUEUE item 46): its face's groups by distance, then the other's.
        let mine_face = u8::from(FaceFrame::at_height(feet[1]).is_b());
        let local = FaceFrame::at_height(feet[1]).point(*feet);
        let key = |g: &dwell_protocol::EcologyGroupNet| {
            (
                g.face != mine_face,
                (f64::from(g.x) - local[0]).hypot(f64::from(g.z) - local[2]),
            )
        };
        assert!(
            m.groups.windows(2).all(|w| key(&w[0]).0 < key(&w[1]).0
                || (key(&w[0]).0 == key(&w[1]).0 && key(&w[0]).1 <= key(&w[1]).1)),
            "nearest first"
        );
        // What it holds: the server's far groups, each on its face, face-local.
        assert_eq!(m.groups.len(), snap.groups.len());
        assert_eq!(m.day, snap.day as f32);
        let face = u8::from(f.is_b());
        let on: Vec<_> = m.groups.iter().filter(|g| g.face == face).collect();
        let want: Vec<_> = snap.groups.iter().filter(|g| g.face == f.face).collect();
        assert_eq!(on.len(), want.len());
        assert!(!on.is_empty(), "this face has herds and packs");
        for g in &on {
            let w = want.iter().find(|w| w.id == g.id).expect("a server group");
            assert_eq!((g.id, g.species.as_str()), (w.id, w.species));
            assert_eq!((g.x, g.z, g.size), (w.x as f32, w.z as f32, w.size as f32));
        }
    }
);

#[test]
fn a_world_without_an_ecology_sends_none() {
    let mut fx = Fixture::new(ServerConfig {
        ecology: false,
        ..config()
    });
    fx.join(1, &Client::new(1));
    fx.send(1, dwell_protocol::Message::EcologyHello);
    ticks(&mut fx, ECOLOGY_SEND_EVERY + 1);
    assert!(ecology_told(&mut fx).values().all(Vec::is_empty));
}
