//! Hold E on a moving body (`shared/physics/bodies.json` `hold`; owner, 2026-11-23: "Hold E
//! (think FPS controls like Halo, X on controller), Q is used for inventory").
//!
//! A client with the `bodies.hold` capability sends `BodyHold` (the body's id) a few times a
//! second while E is held aimed at a body, once `HOLD.hold_seconds` have passed. While they keep
//! coming (none missed for `GAP_TICKS`), each tick: where the player stands inside the body's hull
//! and the world's mode has the `bail` ability (creative; "Hold E inside the hull"), its water
//! bails out, a full hull in `HOLD.bail_seconds`; else a body tipped more than `HOLD.upright_deg`
//! from its build's up turns back toward it ("Both modes"), lifted off the ground it lies on.
//! Within `HOLD.reach_m` of the body's nearest cell only, and never a body held still (on its
//! build platform).

use dwell_protocol::capability_names::BODIES_HOLD;

use super::*;
use crate::felling::HOLD;

/// Ticks a hold lasts after its last `BodyHold` (the client sends one every ~100 ms).
pub const GAP_TICKS: u32 = SIM_HZ / 4;

/// What a hold did on a tick (for tests and the dev tools).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HoldDone {
    /// Bailed: the water left in the hull (m³).
    Bailed(f64),
    /// Righting: the body's tilt from its build's up (degrees).
    Righting(f64),
}

impl Server {
    /// A `BodyHold` from a session's client: kept only from one that said `bodies.hold`.
    pub(super) fn body_hold(&mut self, id: SessionId, body: u32) {
        let tick = self.tick;
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        if !s.capabilities.has(BODIES_HOLD) {
            return;
        }
        self.holds.insert(id, (body, tick));
    }

    /// Each tick before the physics step: the holds still coming, done.
    pub(super) fn hold_bodies(&mut self) {
        let tick = self.tick;
        self.holds
            .retain(|_, &mut (_, at)| tick.saturating_sub(at) <= GAP_TICKS);
        let holds: Vec<(SessionId, u32)> = self.holds.iter().map(|(&k, &(b, _))| (k, b)).collect();
        for (id, body) in holds {
            let done = self.hold_body(id, body);
            self.hold_done = done;
        }
    }

    /// One tick of session `id`'s hold on body `body`.
    fn hold_body(&mut self, id: SessionId, body: u32) -> Option<HoldDone> {
        let dt = 1.0 / f64::from(SIM_HZ);
        let h = self.sessions.get(&id).and_then(|s| s.handle)?;
        let eye = DVec3::from(self.eye_of(h));
        let p = self.felling.pieces.iter().find(|p| p.id == body)?;
        let (handle, afloat) = (p.body, p.is_afloat());
        if p.is_anchored() {
            return None;
        }
        let (at, rot) = (self.physics.position(handle), self.physics.rotation(handle));
        let near = p
            .cells
            .iter()
            .map(|&(o, _)| (at + rot * crate::felling::cell_offset_m(o) - eye).length())
            .fold(f64::INFINITY, f64::min);
        if near > HOLD.reach_m {
            return None;
        }
        // Inside the hull: the eye's cell or the one under it (where the player stands), in the
        // body's own grid.
        let grid = dwell_world::GridFrame::at_pose(at.to_array(), rot.to_array());
        let local = |q: DVec3| {
            let l = DVec3::from(grid.to_grid(q.to_array()));
            [
                l.x.round() as i32,
                (l.y / dwell_protocol::CELL_HEIGHT).round() as i32,
                l.z.round() as i32,
            ]
        };
        let f = dwell_world::FaceFrame::at_height(at.y);
        let face_up = DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let inside = [eye, eye - face_up]
            .iter()
            .any(|&q| self.felling.in_hull(body, local(q)));
        if inside && self.settings.allows(Ability::Bail) {
            let (water, room) = self.felling.hull_water(body);
            if water > 0.0 {
                let left = self.felling.bail(body, room / HOLD.bail_seconds * dt);
                self.physics
                    .set_velocity(handle, self.physics.velocity(handle));
                return Some(HoldDone::Bailed(left));
            }
        }
        // Righting: turn its build's up back to the face's.
        let up = rot * face_up;
        let tilt = up.angle_between(face_up);
        if tilt.to_degrees() <= HOLD.upright_deg {
            return None;
        }
        let axis = up.cross(face_up);
        let axis = if axis.length() > 1e-6 {
            axis.normalize()
        } else {
            // Upside down: roll about its length.
            (rot * DVec3::X).normalize()
        };
        // Easing in over the last ~15°, so it comes to rest upright rather than over.
        let speed = HOLD.right_rad_per_s.min(tilt * 4.0);
        self.physics.set_angular_velocity(handle, axis * speed);
        if !afloat {
            let v = self.physics.velocity(handle);
            let rise = v.dot(face_up);
            if rise < HOLD.right_lift_ms {
                self.physics
                    .set_velocity(handle, v + face_up * (HOLD.right_lift_ms - rise));
            }
        }
        Some(HoldDone::Righting(tilt.to_degrees()))
    }

    /// The water in body `id`'s hull and the most it holds (m³; docs/CANNONS.md §12 H2).
    pub fn hull_water(&self, id: u32) -> (f64, f64) {
        self.felling.hull_water(id)
    }

    /// What the last hold did this tick (tests).
    pub fn hold_done(&self) -> Option<HoldDone> {
        self.hold_done
    }
}
