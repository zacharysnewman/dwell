//! The portals' shafts as the player controller sees them (docs/BIFACIAL_WORLD.md §12.3): each
//! shaft's axis, radius and opening on both faces, and the exit push (`midplane.json`
//! `portals.exit`) that carries a body rising fast out of a shaft sideways, so it arcs out of the
//! far opening and lands on the ground beside it instead of falling back in and swinging between
//! the faces.
//!
//! The world's holder fills it from the generator (`dwell_worldgen::portal_shafts_for_sized`) on
//! the server and in the client's prediction alike, so both run the same push. Face-local: an
//! opening's height is metres above the midplane on its face, as `FaceFrame::y` gives it.

use std::sync::Arc;

use crate::Face;

/// The exit push (`midplane.json` `portals.exit`): in a shaft's footprint, a body rising at least
/// `min_rise` m/s with its feet within `depth` m over the opening is pushed along its facing at
/// `push` m/s², like a steady crosswind across the shaft's mouth. Over the opening, not in the
/// shaft: a body that rose against the shaft's wall (it walked in, BUGS.md #107) would be pushed
/// into the wall there and get nothing; over the rim it is clear of every wall. A faster rise crosses the depth
/// sooner (less push) and flies longer, so where it lands doesn't depend on how fast it rose:
/// about `2 · push · depth / g` m beyond where it left the shaft. A jump (a few m/s) and a ladder
/// climb are never pushed; a body swung up from the far face (tens of m/s) is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalExit {
    /// m/s² along the facing.
    pub push: f32,
    /// m: how far over the opening (its rim's top) the push acts.
    pub depth: f32,
    /// m/s: the least rising speed that is pushed.
    pub min_rise: f32,
}

impl PortalExit {
    /// No push (a world without portals).
    pub const NONE: Self = Self {
        push: 0.0,
        depth: 0.0,
        min_rise: 0.0,
    };
}

impl Default for PortalExit {
    fn default() -> Self {
        Self::NONE
    }
}

/// One portal's shaft: its axis (world x, z), the radius its open columns lie within, and its
/// opening on each face (`top[face]`: the rim's top, face-local metres above the midplane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaft {
    pub axis: [f64; 2],
    pub radius: f64,
    pub top: [f64; 2],
}

impl Shaft {
    /// The opening's height on `face` (face-local m).
    pub fn top(&self, face: Face) -> f64 {
        self.top[usize::from(face == Face::B)]
    }
}

/// The world's portal shafts and their exit push ([`crate::VoxelWorld::portals`]); empty by
/// default (no portals, no push).
#[derive(Clone, Debug, Default)]
pub struct PortalShafts {
    pub shafts: Arc<Vec<Shaft>>,
    pub exit: PortalExit,
}

impl PortalShafts {
    pub fn new(shafts: Vec<Shaft>, exit: PortalExit) -> Self {
        Self {
            shafts: Arc::new(shafts),
            exit,
        }
    }

    /// The shaft whose footprint holds world column point (`x`, `z`).
    pub fn at(&self, x: f64, z: f64) -> Option<&Shaft> {
        self.shafts.iter().find(|s| {
            let (dx, dz) = (x - s.axis[0], z - s.axis[1]);
            dx * dx + dz * dz <= s.radius * s.radius
        })
    }

    /// The exit push's speed gain this step (m/s along the facing, ≥ 0) for an airborne body on
    /// `face` at world column (`x`, `z`) with its feet at face-local height `feet` (`FaceFrame::y`),
    /// rising at `rise` m/s under `gravity` m/s²: zero outside every shaft, under the opening (in
    /// the shaft), more than the push's depth over it, not rising, or slower than `min_rise` when
    /// it passed the opening (`rise² + 2 g (feet − top)`, the speed it had there, with no state:
    /// a jump from the rim would need its apex `min_rise² / 2g` over the opening, ~2.5 m).
    pub fn exit_push(
        &self,
        face: Face,
        [x, feet, z]: [f64; 3],
        rise: f32,
        gravity: f32,
        dt: f32,
    ) -> f32 {
        let e = self.exit;
        if e.push <= 0.0 || rise <= 0.0 {
            return 0.0;
        }
        let Some(s) = self.at(x, z) else {
            return 0.0;
        };
        let over = feet - s.top(face);
        if over < 0.0 || over > f64::from(e.depth) {
            return 0.0;
        }
        let at_opening = f64::from(rise).powi(2) + 2.0 * f64::from(gravity) * over;
        if at_opening < f64::from(e.min_rise).powi(2) {
            return 0.0;
        }
        e.push * dt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXIT: PortalExit = PortalExit {
        push: 30.0,
        depth: 2.0,
        min_rise: 10.0,
    };

    fn shafts() -> PortalShafts {
        PortalShafts::new(
            vec![Shaft {
                axis: [10.5, -4.5],
                radius: 2.5,
                top: [50.0, 40.0],
            }],
            EXIT,
        )
    }

    #[test]
    fn the_exit_push_acts_in_a_shafts_top_on_a_fast_rise_only() {
        // both-faces: the opening's height is each face's own.
        let s = shafts();
        let (g, dt) = (20.0, 1.0 / 60.0);
        let full = 30.0 * dt;
        assert_eq!(s.exit_push(Face::A, [10.5, 51.0, -4.5], 20.0, g, dt), full);
        assert_eq!(s.exit_push(Face::B, [10.5, 41.0, -4.5], 20.0, g, dt), full);
        // Face B's opening is lower: 51 m there is over its depth.
        assert_eq!(s.exit_push(Face::B, [10.5, 51.0, -4.5], 20.0, g, dt), 0.0);
        // In the shaft, under the opening (BUGS.md #107: a wall there takes the push), over the
        // depth, outside the footprint: none.
        assert_eq!(s.exit_push(Face::A, [10.5, 49.0, -4.5], 20.0, g, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [10.5, 52.5, -4.5], 20.0, g, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [13.5, 51.0, -4.5], 20.0, g, dt), 0.0);
        // Slowed over the opening, it passed it fast: 8 m/s 1.5 m over it was ~11 m/s there.
        assert_eq!(s.exit_push(Face::A, [10.5, 51.5, -4.5], 8.0, g, dt), full);
        // A jump (5 m/s at the rim's top, ~0.6 m up) never was; nor is a fall.
        assert_eq!(s.exit_push(Face::A, [10.5, 50.0, -4.5], 5.0, g, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [10.5, 50.6, -4.5], 0.5, g, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [10.5, 51.0, -4.5], -20.0, g, dt), 0.0);
        // A world without portals pushes nothing.
        let none = PortalShafts::default();
        assert_eq!(
            none.exit_push(Face::A, [10.5, 51.0, -4.5], 20.0, g, dt),
            0.0
        );
    }
}
