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

use crate::{CHUNK_SIZE, ChunkCoord, Face, MIDPLANE_M, row_of_m};

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
    /// m: how far over the opening a body rising in the shaft tops out (its feet), whatever the
    /// shaft's length (owner, 2026-11-23, 0.76's 1.2 km shafts: the portal "should provide just
    /// enough to clear the other side by 3m under your feet"); 0: no lift, the swing alone.
    pub crest: f32,
}

impl PortalExit {
    /// No push (a world without portals).
    pub const NONE: Self = Self {
        push: 0.0,
        depth: 0.0,
        min_rise: 0.0,
        crest: 0.0,
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
    /// No block is placed with its column's centre within this far (m) of an axis, at any height
    /// on either face (owner, 2026-11-23: "With a no build zone"); 0: none.
    pub no_build: f64,
    /// A client within this far (m, across) of an axis draws the shaft's walls from this data
    /// where its chunks aren't loaded ([`Self::stand_ins`]); 0: none.
    pub stand_in: f64,
    /// How far over an opening (m) the far end's stand-ins reach: the monument over it (its arch).
    pub stand_in_over: f64,
    /// How far across from an axis (m) the stand-ins reach: the shaft, or the monument if wider.
    pub stand_in_across: f64,
}

impl PortalShafts {
    pub fn new(shafts: Vec<Shaft>, exit: PortalExit) -> Self {
        Self {
            shafts: Arc::new(shafts),
            exit,
            no_build: 0.0,
            stand_in: 0.0,
            stand_in_over: 0.0,
            stand_in_across: 0.0,
        }
    }

    /// Drawn as generated within `reach` m of an axis where the chunks aren't loaded, out to
    /// `over` m above the far opening and `across` m from the axis (the monument's height and
    /// half-width; BUGS.md #236).
    pub fn with_stand_ins(mut self, reach: f64, over: f64, across: f64) -> Self {
        self.stand_in = reach;
        self.stand_in_over = over;
        self.stand_in_across = across;
        self
    }

    /// The chunks a client with its eye at world point `eye` draws as generated where it doesn't
    /// hold them (BUGS.md #236, the owner's pick: "The tube mesh"; then "don't special case it"):
    /// of each shaft within [`Self::stand_in`] m across, the chunks from a chunk's height under the
    /// eye's face's opening down the shaft and out of the far face's opening, up to
    /// [`Self::stand_in_over`] m over it (its arch), within [`Self::stand_in_across`] m of the axis.
    /// The full-detail view reaches 48 m of a ~1.2 km shaft, and the far view draws neither a hole
    /// a few metres wide nor the far face; the eye's own face's ground is the far view's, so its
    /// stand-ins stop inside the rock.
    pub fn stand_ins(&self, eye: [f64; 3]) -> Vec<ChunkCoord> {
        let mut out = Vec::new();
        if self.stand_in <= 0.0 {
            return out;
        }
        let n = CHUNK_SIZE;
        let near_b = crate::FaceFrame::at_height(eye[1]).is_b();
        for s in self.shafts.iter() {
            let (dx, dz) = (eye[0] - s.axis[0], eye[2] - s.axis[1]);
            if dx * dx + dz * dz > self.stand_in * self.stand_in {
                continue;
            }
            let (top_a, top_b) = (MIDPLANE_M + s.top(Face::A), MIDPLANE_M - s.top(Face::B));
            let over = self.stand_in_over;
            let (lo, hi) = if near_b {
                (row_of_m(top_b) + n, row_of_m(top_a + over))
            } else {
                (row_of_m(top_b - over), row_of_m(top_a) - n)
            };
            let (cy0, cy1) = if near_b {
                ((lo + n - 1).div_euclid(n), hi.div_euclid(n))
            } else {
                (lo.div_euclid(n), (hi + 1).div_euclid(n) - 1)
            };
            let reach = s.radius.max(self.stand_in_across).ceil() as i32;
            let [cx, cz] = s.axis.map(|v| v.floor() as i32);
            for x in (cx - reach).div_euclid(n)..=(cx + reach).div_euclid(n) {
                for z in (cz - reach).div_euclid(n)..=(cz + reach).div_euclid(n) {
                    for y in cy0..=cy1 {
                        let c = ChunkCoord::new(x, y, z);
                        if !out.contains(&c) {
                            out.push(c);
                        }
                    }
                }
            }
        }
        // Nearest the eye first: the shaft below it is drawn before its far end.
        let at = [eye[0], eye[1] / crate::CELL_H, eye[2]];
        out.sort_by_key(|c| {
            let d = [
                f64::from(c.x * n + n / 2) - at[0],
                f64::from(c.y * n + n / 2) - at[1],
                f64::from(c.z * n + n / 2) - at[2],
            ];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as i64
        });
        out
    }

    /// Whether world column (`x`, `z`) is one of a shaft's open columns (as the generator opens
    /// them: its centre within the radius of the axis column's centre).
    pub fn open_column(&self, x: i32, z: i32) -> bool {
        self.shafts.iter().any(|s| {
            let [cx, cz] = s.axis.map(|v| v.floor() as i32);
            let (dx, dz) = (f64::from(x - cx), f64::from(z - cz));
            dx * dx + dz * dz <= s.radius * s.radius
        })
    }

    /// The cells of chunk `c` that see the sky down a shaft (chunk-local `x | y << 5 | z << 10`,
    /// [`crate::light::ShaftSky`]): its open columns' cells from face B's opening to face A's
    /// (owner, 2026-11-24: "the structure arches will mostly block the sky/light for the tube").
    pub fn sky_cells(&self, c: ChunkCoord) -> Vec<u16> {
        let n = CHUNK_SIZE;
        let mut out = Vec::new();
        for s in self.shafts.iter() {
            let lo = row_of_m(MIDPLANE_M - s.top(Face::B)) + 1;
            let hi = row_of_m(MIDPLANE_M + s.top(Face::A)) - 1;
            let (y0, y1) = (lo.max(c.y * n), hi.min(c.y * n + n - 1));
            if y0 > y1 {
                continue;
            }
            let reach = s.radius.ceil() as i32;
            let [cx, cz] = s.axis.map(|v| v.floor() as i32);
            for z in (cz - reach).max(c.z * n)..=(cz + reach).min(c.z * n + n - 1) {
                for x in (cx - reach).max(c.x * n)..=(cx + reach).min(c.x * n + n - 1) {
                    let (dx, dz) = (f64::from(x - cx), f64::from(z - cz));
                    if dx * dx + dz * dz > s.radius * s.radius {
                        continue;
                    }
                    for y in y0..=y1 {
                        let (lx, ly, lz) = (x - c.x * n, y - c.y * n, z - c.z * n);
                        out.push((lx | (ly << 5) | (lz << 10)) as u16);
                    }
                }
            }
        }
        out
    }

    /// With a no-build zone of `radius` m round each axis.
    pub fn with_no_build(mut self, radius: f64) -> Self {
        self.no_build = radius;
        self
    }

    /// Whether placing a block in world cell `c` is refused: its column's centre within the
    /// no-build zone of a portal's axis (any row: the shaft runs through both faces).
    pub fn forbids(&self, c: [i32; 3]) -> bool {
        let r = self.no_build;
        r > 0.0
            && self.shafts.iter().any(|s| {
                let (dx, dz) = (
                    f64::from(c[0]) + 0.5 - s.axis[0],
                    f64::from(c[2]) + 0.5 - s.axis[1],
                );
                dx * dx + dz * dz <= r * r
            })
    }

    /// The shaft whose footprint holds world column point (`x`, `z`).
    pub fn at(&self, x: f64, z: f64) -> Option<&Shaft> {
        self.shafts.iter().find(|s| {
            let (dx, dz) = (x - s.axis[0], z - s.axis[1]);
            dx * dx + dz * dz <= s.radius * s.radius
        })
    }

    /// The rising speed (m/s) the portal holds an airborne body to on `face` at world column
    /// (`x`, `z`) with its feet at face-local `feet`, rising at `rise` m/s under `gravity` m/s²:
    /// in a shaft's footprint, under its opening (in the shaft, never over it: a jump from the rim
    /// gets nothing), the speed whose apex puts the feet `crest` m over the opening,
    /// `√(2 g (top + crest − feet))`; `None` elsewhere, falling, or with no crest. A faster rise is
    /// slowed to it and a slower one raised, so a body tops out there however long the shaft.
    pub fn lift(&self, face: Face, [x, feet, z]: [f64; 3], rise: f32, gravity: f32) -> Option<f32> {
        let crest = f64::from(self.exit.crest);
        if crest <= 0.0 || rise <= 0.0 {
            return None;
        }
        let s = self.at(x, z)?;
        let top = s.top(face);
        if feet >= top {
            return None;
        }
        Some((2.0 * f64::from(gravity) * (top + crest - feet)).sqrt() as f32)
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
        crest: 3.0,
    };

    #[test]
    fn no_block_is_placed_in_a_portals_no_build_zone() {
        // both-faces: the zone is a column's distance from an axis, at every row of both faces.
        let p = shafts().with_no_build(7.5);
        // The axis at (10.5, -4.5): its own column, 7 m out, and 8 m out.
        for y in [-3000, -5, 0, 5, 1500] {
            assert!(p.forbids([10, y, -5]), "the axis's column at row {y}");
            assert!(p.forbids([17, y, -5]), "7 m out at row {y}");
            assert!(!p.forbids([18, y, -5]), "8 m out at row {y}");
        }
        assert!(!shafts().forbids([10, 0, -5]), "no zone without the rule");
    }

    #[test]
    fn the_lift_tops_a_rising_body_out_three_metres_over_the_opening() {
        // both-faces: each face's opening by `Shaft::top`, in turn.
        let p = shafts();
        let g = 20.0f32;
        for face in [Face::A, Face::B] {
            let top = p.shafts[0].top(face);
            // Deep in the shaft, rising slowly or fast: held to the speed whose apex is top + 3.
            for (feet, rise) in [(top - 500.0, 3.0), (top - 500.0, 300.0), (top - 1.0, 40.0)] {
                let v = f64::from(p.lift(face, [10.5, feet, -4.5], rise, g).expect("lifted"));
                let apex = feet + v * v / (2.0 * f64::from(g));
                assert!(
                    (apex - (top + 3.0)).abs() < 1e-3,
                    "{face:?} from {feet}: apex {apex}"
                );
            }
            // Never falling, over the opening (a jump from the rim), or outside the footprint.
            assert_eq!(p.lift(face, [10.5, top - 10.0, -4.5], -5.0, g), None);
            assert_eq!(p.lift(face, [10.5, top + 0.5, -4.5], 5.0, g), None);
            assert_eq!(p.lift(face, [20.5, top - 10.0, -4.5], 5.0, g), None);
        }
        let none = PortalShafts::new(p.shafts.to_vec(), PortalExit { crest: 0.0, ..EXIT });
        assert_eq!(none.lift(Face::A, [10.5, 0.0, -4.5], 5.0, g), None);
    }

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
