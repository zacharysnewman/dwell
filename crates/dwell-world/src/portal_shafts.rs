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
/// `min_rise` m/s with its feet within `depth` m under the opening is pushed along its facing at
/// `push` m/s², like a steady crosswind across the shaft's top. A faster rise crosses the depth
/// sooner (less push) and flies longer, so where it lands doesn't depend on how fast it rose:
/// about `2 · push · depth / g` m beyond where it left the shaft. A jump (a few m/s) and a ladder
/// climb are never pushed; a body swung up from the far face (tens of m/s) is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalExit {
    /// m/s² along the facing.
    pub push: f32,
    /// m: how far under the opening (its rim's top) the push begins.
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
    /// rising at `rise` m/s: zero outside every shaft, below the push's depth, above the opening,
    /// or rising slower than `min_rise`.
    pub fn exit_push(&self, face: Face, [x, feet, z]: [f64; 3], rise: f32, dt: f32) -> f32 {
        let e = self.exit;
        if e.push <= 0.0 || rise < e.min_rise {
            return 0.0;
        }
        let Some(s) = self.at(x, z) else {
            return 0.0;
        };
        let top = s.top(face);
        if feet < top - f64::from(e.depth) || feet > top {
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
        depth: 6.0,
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
        let dt = 1.0 / 60.0;
        let full = 30.0 * dt;
        assert_eq!(s.exit_push(Face::A, [10.5, 47.0, -4.5], 20.0, dt), full);
        assert_eq!(s.exit_push(Face::B, [10.5, 37.0, -4.5], 20.0, dt), full);
        // Face B's opening is lower: 47 m there is above it.
        assert_eq!(s.exit_push(Face::B, [10.5, 47.0, -4.5], 20.0, dt), 0.0);
        // Below the depth, too slow (a jump), outside the footprint: none.
        assert_eq!(s.exit_push(Face::A, [10.5, 40.0, -4.5], 20.0, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [10.5, 47.0, -4.5], 7.0, dt), 0.0);
        assert_eq!(s.exit_push(Face::A, [13.5, 47.0, -4.5], 20.0, dt), 0.0);
        // A world without portals pushes nothing.
        let none = PortalShafts::default();
        assert_eq!(none.exit_push(Face::A, [10.5, 47.0, -4.5], 20.0, dt), 0.0);
    }
}
