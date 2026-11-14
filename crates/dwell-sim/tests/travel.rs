//! Flying to the moon (MOON_WORLD.md §7, M-7), natively: a scripted flight from home to the moon,
//! a landing on each of the moon's faces, a jump under its ⅙ g, and back home. At capture and
//! release the world position doesn't jump (≤ 1 cm), nor the world velocity.

use std::sync::Arc;

use dwell_look::celestial::Celestial;
use dwell_look::daynight::DAY_NIGHT;
use dwell_physics::DVec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::Input;
use dwell_sim::travel::{GridSpace, Transfer, Travel, Traveller, grid_generator};
use dwell_world::{ChunkGenerator, FaceFrame, MIDPLANE_M};
use dwell_worldgen::grids::{CrateredTerrain, grid, travel};

const DT: f64 = 1.0 / 60.0;
const SEED: u64 = 7;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn travel_world() -> Travel {
    let home = grid("dwell:home").unwrap();
    let moon = grid("dwell:moon").unwrap();
    // Home's terrain doesn't matter here: the flight starts in the air over it.
    let air: ChunkGenerator = Arc::new(|_, _| {});
    let (moon_gen, moon_air) = grid_generator(moon, SEED).unwrap();
    Travel {
        spaces: vec![
            GridSpace::new(home, 25_000.0, air, Some(Arc::new(|_| true))),
            GridSpace::new(moon, moon.radius_m.unwrap(), moon_gen, Some(moon_air)),
        ],
        celestial: Celestial::new(DAY_NIGHT, None),
        rules: travel(),
    }
}

/// A clock time with the moon high over face A (so the flight goes up and out).
fn moon_high(c: &Celestial) -> f64 {
    (0..2000)
        .map(|i| f64::from(i) * 7.0)
        .find(|&t| c.moon_direction(t)[1] > 0.8)
        .expect("the moon rises")
}

/// Fly input steering toward a target in the current space's local frame.
fn steer(face: FaceFrame, from: V3, to: V3) -> Input {
    let d = face.vector(sub(to, from));
    let h = d[0].hypot(d[2]);
    let yaw = d[0].atan2(d[2]).to_degrees() as f32;
    // The slider's level slows the flight near the target (about its distance a second).
    let dist = len(d);
    let level = (2.0 * (dist / 20.0).max(1.0).log2()).clamp(0.0, 11.0) as u8;
    Input {
        move_y: if h > 0.4 * d[1].abs() && h > 3.0 {
            1.0
        } else {
            0.0
        },
        look_yaw: yaw,
        // Climb or sink whenever off the target's height (a level flight low over the ground
        // would run into a crater's rim).
        jump: d[1] > 1.5,
        crouch: d[1] < -1.5,
        run: true,
        fly: true,
        fly_speed: level,
        ..Input::default()
    }
}

struct Flight {
    tr: Travel,
    p: Traveller,
    t: f64,
    transfers: Vec<Transfer>,
    /// The world position each tick.
    path: Vec<V3>,
}

impl Flight {
    fn new() -> Self {
        let mut tr = travel_world();
        let t = moon_high(&tr.celestial);
        let s = &mut tr.spaces[0];
        let h = s.players.spawn(
            &mut s.physics,
            PlayerControllerConfig::default(),
            DVec3::new(0.0, 300.0, 0.0),
            0.0,
        );
        Self {
            tr,
            p: Traveller { grid: 0, handle: h },
            t,
            transfers: Vec::new(),
            path: Vec::new(),
        }
    }

    fn face(&self) -> FaceFrame {
        self.tr.spaces[self.p.grid]
            .players
            .controller(self.p.handle)
            .face
    }
    fn local(&self) -> V3 {
        self.tr.local(self.p).0
    }

    fn tick(&mut self, input: Input) {
        if let Some(x) = self.tr.update(&mut self.p, self.t) {
            eprintln!(
                "transfer {}→{} at {:.1} s: now {:?}",
                x.from,
                x.to,
                self.t,
                self.local()
            );
            self.transfers.push(x);
        }
        let s = &mut self.tr.spaces[self.p.grid];
        s.players.set_input(self.p.handle, input);
        s.step(DT);
        self.t += DT;
        self.path.push(self.tr.world(self.p, self.t).0);
    }

    /// Flies toward a grid-local target of `grid` (the current space's) until within `m`.
    fn fly_to(&mut self, grid: usize, target: impl Fn(&Travel, f64) -> V3, m: f64, max_s: f64) {
        let start = self.t;
        loop {
            let goal = target(&self.tr, self.t);
            let here = if self.p.grid == grid {
                self.local()
            } else {
                // Not in that space yet: steer at the target's world point from this space.
                let w = self.tr.frame(grid, self.t).to_world(goal);
                let f = self.tr.frame(self.p.grid, self.t);
                let goal_here = f.to_grid(w);
                let here = self.local();
                self.tick(steer(self.face(), here, goal_here));
                assert!(self.t - start < max_s, "no arrival in {max_s} s");
                continue;
            };
            if len(sub(goal, here)) < m {
                return;
            }
            self.tick(steer(self.face(), here, goal));
            assert!(
                self.t - start < max_s,
                "no arrival in {max_s} s, {} m left",
                len(sub(goal, here))
            );
        }
    }

    /// Hovers two seconds (flight's drag stops the body), then lets go until standing.
    fn land(&mut self) {
        for _ in 0..120 {
            self.tick(Input {
                fly: true,
                ..Input::default()
            });
        }
        for _ in 0..(60 * 120) {
            self.tick(Input::default());
            if self.tr.spaces[self.p.grid]
                .players
                .controller(self.p.handle)
                .ground
                .grounded
            {
                return;
            }
        }
        panic!(
            "never landed: {:?} on {:?}, v {:?}",
            self.local(),
            self.face(),
            self.tr.local(self.p).1
        );
    }

    /// One jump's rise (m, along the face's up).
    fn jump(&mut self) -> f64 {
        for _ in 0..30 {
            self.tick(Input::default());
        }
        let face = self.face();
        let ground = face.y(self.local()[1]);
        let mut peak = ground;
        for i in 0..(60 * 15) {
            self.tick(Input {
                jump: i < 3,
                ..Input::default()
            });
            peak = peak.max(face.y(self.local()[1]));
        }
        peak - ground
    }

    /// No jump at any transfer: the world position kept (≤ 1 cm) and the world velocity (≤ 1 cm/s)
    /// at the switch's instant. From then on flight steers relative to the new grid (its wish is a
    /// speed in that grid), which turns the path over the next ticks as a change of heading does.
    fn check_transfers(&self) {
        for x in &self.transfers {
            let d = len(sub(x.world_after, x.world_before));
            assert!(d <= 0.01, "transfer {}→{} jumped {d} m", x.from, x.to);
            let dv = len(sub(x.velocity_after, x.velocity_before));
            assert!(
                dv <= 0.01,
                "transfer {}→{}: the velocity jumped {dv} m/s",
                x.from,
                x.to
            );
        }
    }
}

fn spawn_over(face: FaceFrame, up: f64) -> impl Fn(&Travel, f64) -> V3 {
    let moon = CrateredTerrain::for_grid("dwell:moon", SEED).unwrap();
    // Face-local (x, ground, z) to the grid's space.
    let s = moon.spawn_point(face.face);
    let p = face.point([s[0], s[1] + up, s[2]]);
    move |_, _| p
}

fn home_over(up: f64) -> impl Fn(&Travel, f64) -> V3 {
    move |_, _| [0.0, up, 0.0]
}

fn round_trip(face: FaceFrame) {
    let mut f = Flight::new();
    let start = f.t;
    // Out to the moon's near side, then (for face B) round its rim to the far side.
    if face == FaceFrame::B {
        f.fly_to(1, |_, _| [16_000.0, MIDPLANE_M + 400.0, 0.0], 300.0, 400.0);
        f.fly_to(1, |_, _| [16_000.0, MIDPLANE_M - 400.0, 0.0], 100.0, 120.0);
    }
    f.fly_to(1, spawn_over(face, 40.0), 4.0, 400.0);
    let trip = f.t - start;
    assert_eq!(f.p.grid, 1, "captured by the moon");
    assert_eq!(f.face(), face, "on the moon's face {face:?}");
    f.land();
    let s = &f.tr.spaces[1];
    assert!((s.players.gravity_scale(f.p.handle) - 0.1667).abs() < 1e-6);
    // ⅙ g: the jump rises 1/0.1667 × the config's height.
    let rise = f.jump();
    let want = f64::from(PlayerControllerConfig::default().jump.height) / 0.1667;
    assert!(
        (rise - want).abs() < 0.05 * want,
        "moon jump {rise} m, want {want}"
    );
    // Home again: (from face B round the rim first) up off the moon until released, then home.
    if face == FaceFrame::B {
        f.fly_to(1, |_, _| [16_000.0, MIDPLANE_M - 400.0, 0.0], 300.0, 200.0);
        f.fly_to(1, |_, _| [16_000.0, MIDPLANE_M + 400.0, 0.0], 100.0, 120.0);
    }
    f.fly_to(0, home_over(400.0), 50.0, 400.0);
    assert_eq!(f.p.grid, 0, "released to home");
    assert_eq!(
        f.tr.spaces[0].players.gravity_scale(f.p.handle),
        1.0,
        "home gravity"
    );
    assert_eq!(f.transfers.len(), 2, "one capture, one release");
    f.check_transfers();
    if face == FaceFrame::A {
        // The way out: about a minute and a half at far flight (owner: ~1–1.5 min).
        assert!((20.0..=40.0).contains(&trip), "the trip took {trip} s");
    }
    eprintln!(
        "face {face:?}: trip {trip:.0} s, moon jump {rise:.2} m, {} ticks",
        f.path.len()
    );
}

#[test]
fn a_flight_home_to_the_moons_face_a_and_back_never_jumps() {
    round_trip(FaceFrame::A);
}

#[test]
fn a_flight_home_to_the_moons_face_b_and_back_never_jumps() {
    round_trip(FaceFrame::B);
}

#[test]
fn far_flight_is_off_near_the_ground() {
    let tr = travel_world();
    let t = moon_high(&tr.celestial);
    assert_eq!(tr.far_speed([0.0, 100.0, 0.0], t), 0.0);
    assert!(tr.far_speed([0.0, 150_000.0, 0.0], t) > 10_000.0);
}
