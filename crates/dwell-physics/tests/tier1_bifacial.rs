//! Gravity by side for Tier 1 bodies (BIFACIAL_WORLD.md §3): toward the midplane, fading to zero
//! over the flip band, which a body settles in instead of swinging across. Ported from
//! `server/tests/bifacial_physics_test.cpp`; geometry is in world coordinates (no test origin).

use dwell_physics::{BodyHandle, BoxBody, DVec3, Motion, Physics};
use dwell_world::MIDPLANE_Y;

const MID: f64 = MIDPLANE_Y as f64;

struct World {
    physics: Physics,
}

impl World {
    fn new() -> Self {
        Self {
            physics: Physics::default(),
        }
    }
    /// A dynamic 1 m box at (0, y, 0) that only translates.
    fn add(&mut self, y: f64) -> BodyHandle {
        let mut b = BoxBody::new(Motion::Dynamic, DVec3::new(0.0, y, 0.0), DVec3::splat(0.5));
        b.lock_rotation = true;
        self.physics.add_box(b)
    }
    fn y(&self, h: BodyHandle) -> f64 {
        self.physics.position(h).y
    }
    fn vy(&self, h: BodyHandle) -> f64 {
        self.physics.velocity(h).y
    }
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.physics.step(1.0 / 60.0);
        }
    }
}

/// doctest's `Approx(expected).epsilon(eps)`.
fn approx(actual: f64, expected: f64, eps: f64) -> bool {
    (actual - expected).abs() < eps * (1.0 + actual.abs().max(expected.abs()))
}

#[test]
fn a_body_falls_toward_the_midplane_from_either_face_at_the_same_acceleration() {
    let mut w = World::new();
    let a = w.add(MID + 500.0); // face A: down is −y
    let b = w.add(MID - 500.0); // face B: down is +y
    w.run(60); // one second
    assert!(approx(w.vy(a), -9.81, 0.02), "vy(a) = {}", w.vy(a));
    assert!(approx(w.vy(b), 9.81, 0.02), "vy(b) = {}", w.vy(b));
    assert!(
        approx(w.y(a), MID + 500.0 - 4.905, 1e-4),
        "y(a) = {}",
        w.y(a)
    );
    assert!(
        approx(w.y(b), MID - 500.0 + 4.905, 1e-4),
        "y(b) = {}",
        w.y(b)
    );
}

#[test]
fn the_band_accelerates_a_body_moving_toward_the_midplane_and_carries_it_across() {
    for side in [1.0, -1.0] {
        let mut w = World::new();
        let id = w.add(MID + side * 10.0);
        // Free fall from 10 m reaches sqrt(2 g h) ≈ 14 m/s at the midplane; the band adds to it.
        let mut fastest: f64 = 0.0;
        let mut crossed = false;
        let mut i = 0;
        while i < 60 * 6 && !crossed {
            w.run(1);
            fastest = fastest.max(w.vy(id).abs());
            crossed = side * (w.y(id) - MID) < 0.0;
            i += 1;
        }
        assert!(crossed, "side {side}: never crossed");
        assert!(fastest > 15.5, "side {side}: fastest {fastest}");
    }
}

#[test]
fn moving_away_from_the_midplane_nothing_slows_a_body_in_the_band() {
    let mut w = World::new();
    let id = w.add(MID + 0.5);
    w.physics.set_velocity(id, DVec3::new(0.0, 20.0, 0.0));
    w.run(1);
    assert!(w.vy(id) > 19.0, "vy = {}", w.vy(id));
}
