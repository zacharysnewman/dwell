//! The sun world on the shared travel (docs/proposals/SUN_WORLD.md S-1, S-3): its grid sits on the sky's sun
//! path (its frame's centre toward today's sun direction, at the distance that keeps its radius at
//! the sky disc's size, its face A toward home), the client's `TravelMap` holds it as the server
//! does, and a flier is captured into its space and released back home by the moon's rules, the
//! world position and velocity kept at each switch.

use std::sync::Arc;

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
            let fv = f.point_velocity(GRID_CENTRE);
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
