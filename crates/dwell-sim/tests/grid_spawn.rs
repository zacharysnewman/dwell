//! Spawns on any grid (docs/proposals/MOON_WORLD.md §8, M-8; owner 2026-11-13: "Im not seeing a moon
//! spawn or sun spawn in my new world settings"): a world set to spawn on the moon or the sun starts a
//! player that can be there (this build's capabilities) standing on that grid's face at its stand point
//! (the sun's on dry crust away from lava), and a respawn after a death there goes back to it. A client
//! that can't be on the grid (an older build) starts at home's spawn.

mod common;

use common::{Client, Fixture, flat};
use dwell_protocol::capabilities::Capabilities;
use dwell_protocol::capability_names::GRID_FRAMES;
use dwell_protocol::{InputFrame, Message, PlayerInput};
use dwell_sim::server::ServerConfig;
use dwell_world::{Face, FaceFrame};
use dwell_worldgen::grids::{grid, grids, spawn_key, stand_point};

const SECOND: u32 = 60;

fn index_of(name: &str) -> u8 {
    grids().iter().position(|g| g.name == name).unwrap() as u8
}

fn world(name: &str, f: FaceFrame, mode: &str) -> Fixture {
    Fixture::new(ServerConfig {
        spawn_grid: Some(spawn_key(grid(name).unwrap(), f.face)),
        mode: mode.into(),
        ..flat()
    })
}

fn step(fx: &mut Fixture, seq: &mut u32, n: u32) {
    for _ in 0..n {
        let m = Message::PlayerInput(PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![InputFrame {
                seq: *seq,
                move_x: 0,
                move_y: 0,
                buttons: 0,
                yaw: 0,
                pitch: 0,
            }],
        });
        fx.server.on_datagram(1, &m.encode());
        *seq += 1;
        fx.server.step();
        fx.server.take_outbox();
    }
}

/// The player's grid and its feet's face-local height over `name`'s stand point on `f`, with the
/// horizontal distance from it.
fn where_on(fx: &Fixture, name: &str, f: FaceFrame) -> (u8, f64, f64) {
    let (g, p, _) = fx.server.grid_state_of(1).expect("a live player");
    let seed = fx.server.config().world_seed;
    let s = stand_point(grid(name).unwrap(), seed, f.face).unwrap();
    let half =
        f64::from(dwell_physics::config::PlayerControllerConfig::default().half_height(false));
    let l = f.point(p);
    (g, l[1] - half - s[1], (l[0] - s[0]).hypot(l[2] - s[2]))
}

fn join_this_build(fx: &mut Fixture) {
    fx.join(1, &Client::new(1));
    fx.send(1, Message::Capabilities(Capabilities::this_build()));
    fx.send(1, Message::WorldgenCheck(0));
}

/// Stands on grid `name`'s face `f` at its stand point, at rest.
fn assert_standing(fx: &Fixture, name: &str, f: FaceFrame) {
    let (g, up, off) = where_on(fx, name, f);
    assert_eq!(g, index_of(name), "on {name}");
    assert_eq!(
        FaceFrame::at_height(fx.server.grid_state_of(1).unwrap().1[1]),
        f,
        "on face {f:?}"
    );
    assert!(off < 2.0, "by the stand point: {off} m off");
    assert!(up.abs() < 0.6, "standing on the ground: {up} m over it");
    let v = fx.server.grid_state_of(1).unwrap().2;
    assert!(
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() < 0.2,
        "at rest: {v:?}"
    );
}

dwell_world::both_faces!(
    /// A world spawning on the moon's face `f` starts this build's player standing there.
    fn a_moon_world_starts_its_player_standing_on_the_moon(f: FaceFrame) {
        let mut fx = world("dwell:moon", f, "creative");
        join_this_build(&mut fx);
        let mut seq = 1;
        step(&mut fx, &mut seq, 4 * SECOND);
        assert_standing(&fx, "dwell:moon", f);
    }
);

dwell_world::both_faces!(
    /// A world spawning on the sun's face `f` starts this build's player standing on dry crust,
    /// away from the lava; dying there (adventure), it stands up on the sun's spawn again.
    fn a_sun_world_starts_and_respawns_its_player_on_the_sun(f: FaceFrame) {
        let mut fx = world("dwell:sun", f, "adventure");
        join_this_build(&mut fx);
        let mut seq = 1;
        step(&mut fx, &mut seq, 4 * SECOND);
        assert_standing(&fx, "dwell:sun", f);
        let seed = fx.server.config().world_seed;
        let t = dwell_worldgen::volcanic::VolcanicTerrain::for_grid("dwell:sun", seed).unwrap();
        let s = stand_point(grid("dwell:sun").unwrap(), seed, f.face).unwrap();
        let c = t.column(f.face, s[0], s[2]);
        assert!(c.ground > c.lava + 2.0, "dry crust: {c:?}");
        assert!(!fx.server.burning(1));
        // Defeated there: up again on the sun's spawn, not at home.
        fx.server.kill_player(1);
        step(&mut fx, &mut seq, 1);
        assert!(fx.server.grid_state_of(1).is_none(), "dead");
        step(&mut fx, &mut seq, 8 * SECOND);
        assert_standing(&fx, "dwell:sun", f);
    }
);

dwell_world::both_faces!(
    /// An older client: one of the moon's line (`grid.frames` only) in a sun world, and one that
    /// says no capabilities in a moon world, start at home's spawn and stay there.
    fn a_client_that_can_t_be_on_the_grid_starts_at_home(f: FaceFrame) {
        for (name, caps) in [
            ("dwell:sun", Some(Capabilities::of([GRID_FRAMES]))),
            ("dwell:moon", None),
        ] {
            let mut fx = world(name, f, "creative");
            fx.join(1, &Client::new(1));
            if let Some(c) = caps {
                fx.send(1, Message::Capabilities(c));
            }
            fx.send(1, Message::WorldgenCheck(0));
            let mut seq = 1;
            step(&mut fx, &mut seq, 12 * SECOND);
            let (g, p, _) = fx.server.grid_state_of(1).unwrap();
            assert_eq!(g, 0, "{name}: an older client stays home");
            let home = fx.server.config().spawn.unwrap();
            assert!(
                (p[0] - home[0]).hypot(p[2] - home[2]) < 4.0,
                "{name}: at home's spawn"
            );
        }
    }
);

// face-a-only: the world file's meta holds the key as text; the faces are its content.
#[test]
fn a_saved_world_keeps_its_spawn_grid() {
    let dir = std::env::temp_dir().join(format!("dwell-grid-spawn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("w.dwellworld");
    let key = spawn_key(grid("dwell:moon").unwrap(), Face::B);
    {
        let store = dwell_storage::WorldStore::open_file(&path).unwrap();
        let mut fx = Fixture::new(ServerConfig {
            spawn_grid: Some(key.clone()),
            store: Some(store),
            ..flat()
        });
        fx.server.save_now();
    }
    let store = dwell_storage::WorldStore::open_file(&path).unwrap();
    assert_eq!(
        store.db().load_meta().unwrap().spawn_grid,
        Some(key.clone())
    );
    // Opened again without a choice: the world's own.
    let fx = Fixture::new(ServerConfig {
        store: Some(store),
        ..flat()
    });
    assert_eq!(fx.server.config().spawn_grid, Some(key));
    let _ = std::fs::remove_dir_all(&dir);
}
