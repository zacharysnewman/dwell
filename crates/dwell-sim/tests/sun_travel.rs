//! The sun world on the shared travel (docs/proposals/SUN_WORLD.md S-1, S-3): its grid sits on the sky's sun
//! path (its frame's centre toward today's sun direction, at the distance that keeps its radius at
//! the sky disc's size, its face A toward home), the client's `TravelMap` holds it as the server
//! does, and a flier is captured into its space and released back home by the moon's rules, the
//! world position and velocity kept at each switch. Only a client that says `grid.sun` is captured
//! (the grid's `capability`, SUN_WORLD.md §5): an older one flies on through, the sun a far body to it.

mod common;

use std::sync::Arc;

use common::{Client, Fixture, flat};
use dwell_protocol::capabilities::Capabilities;
use dwell_protocol::capability_names::{GRID_FRAMES, GRID_SUN};
use dwell_protocol::{InputFrame, Message, PlayerInput, input_buttons};
use dwell_sim::server::SessionId;

use dwell_look::celestial::Celestial;
use dwell_look::daynight::DAY_NIGHT;
use dwell_physics::DVec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::Input;
use dwell_sim::travel::{
    GRID_CENTRE, GridSpace, Transfer, Travel, TravelMap, Traveller, grid_frame_at, grid_generator,
};
use dwell_world::{ChunkGenerator, FaceFrame, MIDPLANE_M};
use dwell_worldgen::grids::{grid, grids, travel};

const DT: f64 = 1.0 / 60.0;
const SEED: u64 = 7;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

// face-a-only: home sees the sun's face A (it turns face A toward home, as the moon does).
#[test]
fn the_sun_grid_sits_on_the_sun_s_path_at_the_sky_disc_s_size() {
    let c = Celestial::new(DAY_NIGHT, None);
    let sun = grid("dwell:sun").unwrap();
    let r = sun.radius_m.unwrap();
    let mut prev: Option<V3> = None;
    for i in 0..240 {
        let t = f64::from(i) * 5.0;
        let f = grid_frame_at(&c, sun, t);
        let centre = f.to_world(GRID_CENTRE);
        let d = len(centre);
        let toward = centre.map(|v| v / d);
        // Toward today's sun, at the sky's size (sun.radiusDeg).
        assert!(
            dot(toward, c.sun_direction(t)) > 1.0 - 1e-9,
            "at {t} s: {toward:?}"
        );
        let deg = (r / d).atan().to_degrees();
        assert!((deg - DAY_NIGHT.sun.radius_deg).abs() < 0.015, "{deg}°");
        // Its face A looks at home (as the moon's does), so the sky shows face A's bake.
        let up = f.up(FaceFrame::A);
        assert!(dot(up, toward) < -1.0 + 1e-9, "face A toward home: {up:?}");
        // It moves with the sun's path: the frame's velocity is the centre's rate.
        if let Some(p) = prev {
            let v = sub(centre, p).map(|x| x / 5.0);
            let fv = f.point_velocity(centre);
            assert!(len(sub(v, fv)) < 0.02 * len(fv) + 50.0, "{v:?} vs {fv:?}");
        }
        prev = Some(centre);
    }
}

#[test]
fn the_client_s_travel_map_holds_the_sun_as_the_server_builds_it() {
    let map = TravelMap::new(SEED, 25_000.0, Celestial::new(DAY_NIGHT, None));
    let index = grids().iter().position(|g| g.name == "dwell:sun").unwrap() as u8;
    assert!(
        map.grids
            .iter()
            .any(|g| g.0 == index && g.1.name == "dwell:sun"),
        "the sun is a travel grid"
    );
    assert!(grid_generator(grid("dwell:sun").unwrap(), SEED).is_some());
    // A client without `grid.sun` holds the moon and not the sun, as the server runs it.
    let moon_only = TravelMap::new(SEED, 25_000.0, Celestial::new(DAY_NIGHT, None))
        .open_to(|c| c == GRID_FRAMES);
    let names: Vec<_> = moon_only.grids.iter().map(|g| g.1.name.as_str()).collect();
    assert_eq!(names, ["dwell:moon"]);
    let all =
        TravelMap::new(SEED, 25_000.0, Celestial::new(DAY_NIGHT, None)).open_to(|c| c == GRID_SUN);
    assert_eq!(all.grids.len(), map.grids.len(), "grid.sun opens the sun");
}

const NEW: SessionId = 1;
const OLD: SessionId = 2;

/// SUN_WORLD.md §5 (orchestrator, 2026-11-13: "have the server never capture a client that doesn't
/// declare it into the sun's grid (it flies on as before, the sun stays a far body for it)"): two
/// fliers held inside the sun's capture sphere, moving with it. This build's client (`grid.sun`) is
/// captured into the sun's space; a client of the moon's line (`grid.frames` only) stays home,
/// gets no sun chunk and no sun grid in its snapshots, and its far flight doesn't count the sun.
// face-a-only: the fliers are held over the sun's face A; capture is by distance from its centre.
#[test]
fn only_a_client_saying_grid_sun_is_captured_by_the_sun() {
    let mut fx = Fixture::new(flat());
    fx.join(NEW, &Client::new(1));
    fx.join(OLD, &Client::new(2));
    fx.send(NEW, Message::Capabilities(Capabilities::this_build()));
    fx.send(OLD, Message::Capabilities(Capabilities::of([GRID_FRAMES])));
    for id in [NEW, OLD] {
        fx.send(id, Message::WorldgenCheck(0));
    }
    let sun = grid("dwell:sun").unwrap();
    let sun_index = grids().iter().position(|g| g.name == "dwell:sun").unwrap() as u8;
    let slot = fx
        .server
        .grids()
        .slots
        .iter()
        .position(|s| s.index == sun_index)
        .expect("the server holds the sun's space");
    let mut got = Vec::new();
    for seq in 1..=240u32 {
        let t = f64::from(fx.server.tick()) / 60.0;
        let f = fx.server.grids().frame(slot, t);
        // Held 1 km inside the capture sphere over face A, carried with the sun.
        let local = [0.0, MIDPLANE_M + sun.capture_m.unwrap() - 1_000.0, 0.0];
        for pid in [1u16, 2] {
            if fx.server.grid_state_of(pid).is_some_and(|g| g.0 == 0) {
                let (wp, wv) = f.release(local, [0.0; 3]);
                assert!(fx.server.place_on_grid(pid, wp, wv));
            }
        }
        for id in [NEW, OLD] {
            let m = Message::PlayerInput(PlayerInput {
                last_snapshot_tick: 0,
                inputs: vec![InputFrame {
                    seq,
                    move_x: 0,
                    move_y: 0,
                    buttons: input_buttons::FLY,
                    yaw: 0,
                    pitch: 0,
                }],
            });
            fx.server.on_datagram(id, &m.encode());
        }
        fx.server.step();
        for o in fx.server.take_outbox() {
            if let Some(m) = Message::decode(&o.bytes) {
                got.push((o.session, m));
            }
        }
    }
    let grid_of = |pid| fx.server.grid_state_of(pid).unwrap().0;
    assert_eq!(grid_of(1), sun_index, "this build's client is on the sun");
    assert_eq!(grid_of(2), 0, "a client without grid.sun flies on at home");
    let transfers = fx.server.take_transfers();
    assert!(transfers.iter().all(|(pid, _)| *pid == 1), "{transfers:?}");
    assert!(
        transfers
            .iter()
            .any(|(pid, x)| *pid == 1 && x.to == usize::from(sun_index)),
        "{transfers:?}"
    );
    let sun_chunks = |id| {
        got.iter()
            .filter(|(s, m)| *s == id && matches!(m, Message::ChunkData(c) if c.grid == sun_index))
            .count()
    };
    assert!(
        sun_chunks(NEW) > 0,
        "the sun's ground streams to this build's client"
    );
    assert_eq!(sun_chunks(OLD), 0, "and never to the older one");
    let old_grid = got.iter().rev().find_map(|(s, m)| match m {
        Message::PhysicsSnapshot(p) if *s == OLD => Some(p.grid),
        _ => None,
    });
    assert!(
        old_grid.expect("a snapshot").is_none_or(|g| g.grid == 0),
        "the older client is never told it is on the sun"
    );
}

fn sun_travel() -> Travel {
    let home = grid("dwell:home").unwrap();
    let sun = grid("dwell:sun").unwrap();
    let air: ChunkGenerator = Arc::new(|_, _| {});
    let (sun_gen, sun_air) = grid_generator(sun, SEED).unwrap();
    Travel {
        spaces: vec![
            GridSpace::new(home, 25_000.0, air, Some(Arc::new(|_| true))),
            GridSpace::new(sun, sun.radius_m.unwrap(), sun_gen, Some(sun_air)),
        ],
        celestial: Celestial::new(DAY_NIGHT, None),
        rules: travel(),
    }
}

fn fly_toward(face: FaceFrame, d: V3) -> Input {
    let d = face.vector(d);
    let h = d[0].hypot(d[2]);
    Input {
        move_y: if h > 0.4 * d[1].abs() { 1.0 } else { 0.0 },
        look_yaw: d[0].atan2(d[2]).to_degrees() as f32,
        jump: d[1] > 1.5,
        crouch: d[1] < -1.5,
        run: true,
        fly: true,
        fly_speed: 11,
        ..Input::default()
    }
}

fn check(x: &Transfer) {
    let d = len(sub(x.world_after, x.world_before));
    assert!(d <= 0.01, "transfer {}→{} jumped {d} m", x.from, x.to);
    let dv = len(sub(x.velocity_after, x.velocity_before));
    assert!(
        dv <= 0.01,
        "transfer {}→{}: velocity jumped {dv} m/s",
        x.from,
        x.to
    );
}

/// From just outside the sun's capture sphere a flier steers at it, is captured (nothing jumps),
/// flies on its ground's side away from it and is released home past `captureM × releaseFactor`.
// face-a-only: the capture and release are of the sun's grid frame, flown in and out over its face A
// (the moon's travel test covers the crossing round a grid's rim to face B).
#[test]
fn a_flier_is_captured_by_the_sun_and_released_home() {
    let mut tr = sun_travel();
    let sun = grid("dwell:sun").unwrap();
    let capture = sun.capture_m.unwrap();
    let mut t = 300.0;
    // Start 1 km outside the capture sphere, on the home side of the sun.
    let f = tr.frame(1, t);
    let start = f.to_world([0.0, MIDPLANE_M + capture + 1_000.0, 0.0]);
    let s = &mut tr.spaces[0];
    let h = s.players.spawn(
        &mut s.physics,
        PlayerControllerConfig::default(),
        DVec3::from_array(start),
        0.0,
    );
    let mut p = Traveller { grid: 0, handle: h };
    let mut transfers = Vec::new();
    let mut tick = |tr: &mut Travel, p: &mut Traveller, t: &mut f64, to_world: V3| {
        if let Some(x) = tr.update(p, *t) {
            transfers.push(x);
        }
        let here_frame = tr.frame(p.grid, *t);
        let goal = here_frame.to_grid(to_world);
        let (here, _) = tr.local(*p);
        let s = &mut tr.spaces[p.grid];
        let face = s.players.controller(p.handle).face;
        s.players
            .set_input(p.handle, fly_toward(face, sub(goal, here)));
        s.step(DT);
        *t += DT;
    };
    // In: toward a point 5 km over the sun's face A (its world point this tick).
    let mut n = 0;
    while p.grid == 0 {
        let goal = tr.frame(1, t).to_world([0.0, MIDPLANE_M + 5_000.0, 0.0]);
        tick(&mut tr, &mut p, &mut t, goal);
        n += 1;
        assert!(n < 60 * 60, "never captured");
    }
    assert_eq!(p.grid, 1, "in the sun's space");
    // Out: away from the sun along its face A's up, until released.
    n = 0;
    while p.grid == 1 {
        let goal = tr
            .frame(1, t)
            .to_world([0.0, MIDPLANE_M + 2.0 * capture, 0.0]);
        tick(&mut tr, &mut p, &mut t, goal);
        n += 1;
        assert!(n < 60 * 120, "never released");
    }
    assert_eq!(p.grid, 0, "home again");
    let (local, _) = tr.local(p);
    let in_sun = tr.frame(1, t).to_grid(local);
    assert!(len(sub(in_sun, GRID_CENTRE)) > capture * tr.rules.release_factor - 1.0);
    let moves: Vec<_> = transfers.iter().map(|x| (x.from, x.to)).collect();
    assert_eq!(moves, [(0, 1), (1, 0)], "one capture, one release");
    transfers.iter().for_each(check);
}
