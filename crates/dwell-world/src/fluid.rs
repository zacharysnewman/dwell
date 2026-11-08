//! Whether the camera's eye is under a fluid's drawn surface (docs/WATER_RENDERING.md §1, §3
//! "Underwater"): what the renderer's underwater effects key on. Physics' `submerged_fraction`
//! counts whole liquid cells as wet, but the chunk mesher draws a fluid's top `surfaceDrop` below
//! its cell's top when the cell above is not the same fluid; going by the cell alone would put the
//! view underwater that much early. On face B "up" is −y (BIFACIAL_WORLD.md §3), so the surface
//! there lies `surfaceDrop` above the cell's bottom, as the mirrored chunk mesh draws it.

use dwell_blocks::{FLUIDS, FluidId, StateId, info};

use crate::{FaceFrame, cell_of_pos, m_of_row, m_of_rows_f};

/// The eye under a fluid's surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeFluid {
    pub fluid: FluidId,
    /// World y (m) of the drawn surface above the eye (on face B, below it in world y), when it is
    /// within [`SURFACE_SCAN`] cells; deeper, the top of the last cell scanned. With the eye above
    /// the surface (within the reach asked for), the surface below it.
    pub surface_y: f64,
    /// Whether the eye itself is under the surface (else it is above it, within the reach: the
    /// renderer splits the view along the waterline, WATER_RENDERING.md §10).
    pub under: bool,
}

/// How many cells above the eye's are searched for the surface.
pub const SURFACE_SCAN: i32 = 64;

/// The fluid a state holds: its own (the `fluid` look), or for a flooded shaped state the fluid
/// of `dwell:water`, whose look the chunk mesher fills flooded cells with.
pub fn fluid_of(id: StateId) -> Option<FluidId> {
    let s = info(id);
    if let Some(f) = s.fluid {
        return Some(f);
    }
    if !s.flooded {
        return None;
    }
    dwell_blocks::state("dwell:water").and_then(|w| info(w).fluid)
}

/// The fluid whose drawn surface the eye is under, or is above by at most `reach` metres (the
/// waterline's band: 0 for under only), if any. `voxel` reads the state at a cell.
pub fn eye_fluid(
    eye: [f64; 3],
    reach: f64,
    mut voxel: impl FnMut(i32, i32, i32) -> StateId,
) -> Option<EyeFluid> {
    let frame = FaceFrame::at_row(cell_of_pos(eye)[1]);
    let up = frame.sign();
    // The eye's cell, else (the eye above the water) the cell `reach` below it toward the ground.
    let low = [eye[0], eye[1] - f64::from(up) * reach, eye[2]];
    let (fluid, [x, y, z]) = [eye, low].into_iter().find_map(|p| {
        let c = cell_of_pos(p);
        fluid_of(voxel(c[0], c[1], c[2])).map(|f| (f, c))
    })?;
    // The topmost cell of this fluid in the eye's column (toward the face's up).
    let mut top = y;
    while (top - y).abs() < SURFACE_SCAN && fluid_of(voxel(x, top + up, z)) == Some(fluid) {
        top += up;
    }
    // The drop is a fraction of a cell, as the chunk mesher draws it.
    let drop = m_of_rows_f(FLUIDS[usize::from(fluid)].surface_drop);
    let covered = fluid_of(voxel(x, top + up, z)) == Some(fluid);
    let surface_y = match (up, covered) {
        (1, false) => m_of_row(top + 1) - drop,
        (1, true) => m_of_row(top + 1),
        (_, false) => m_of_row(top) + drop,
        (_, true) => m_of_row(top),
    };
    // Face-local heights: the eye's over the surface's.
    let above = frame.y(eye[1]) - frame.y(surface_y);
    (above <= reach).then_some(EyeFluid {
        fluid,
        surface_y,
        under: above < 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MIDPLANE_Y;
    use dwell_blocks::{AIR, require};
    use std::collections::HashMap;

    struct Column(HashMap<(i32, i32, i32), StateId>);

    impl Column {
        fn new(cells: &[(i32, &str)]) -> Self {
            Self(
                cells
                    .iter()
                    .map(|&(y, s)| ((0, y, 0), require(s)))
                    .collect(),
            )
        }
        fn at(&self, eye: [f64; 3]) -> Option<EyeFluid> {
            eye_fluid(eye, 0.0, |x, y, z| *self.0.get(&(x, y, z)).unwrap_or(&AIR))
        }
        fn near(&self, eye: [f64; 3], reach: f64) -> Option<EyeFluid> {
            eye_fluid(eye, reach, |x, y, z| {
                *self.0.get(&(x, y, z)).unwrap_or(&AIR)
            })
        }
    }

    fn water() -> FluidId {
        dwell_blocks::fluid("dwell:water").expect("dwell:water")
    }

    /// A height `frac` of the way up row `row` (m).
    fn up(row: i32, frac: f64) -> f64 {
        crate::m_of_row(row) + frac * crate::CELL_H
    }

    #[test]
    fn an_eye_above_the_drawn_surface_of_a_water_cell_under_air_is_not_submerged() {
        // Regression (WATER_RENDERING.md §1): physics counts the whole cell wet, but the surface is drawn
        // 1/8 of a cell below its top; 0.9 of the way up the cell the eye is above the water.
        let col = Column::new(&[(9, "dwell:stone"), (10, "dwell:water")]);
        assert_eq!(col.at([0.5, up(10, 0.9), 0.5]), None);
        assert_eq!(
            col.at([0.5, up(10, 0.8), 0.5]),
            Some(EyeFluid {
                fluid: water(),
                surface_y: up(10, 0.875),
                under: true,
            })
        );
    }

    #[test]
    fn water_above_keeps_the_whole_cell_wet_and_the_surface_is_found_above() {
        let col = Column::new(&[
            (10, "dwell:water"),
            (11, "dwell:water"),
            (12, "dwell:water"),
        ]);
        let at = col.at([0.5, up(10, 0.99), 0.5]).expect("under");
        assert_eq!(at.surface_y, up(12, 0.875));
        assert_eq!(col.at([0.5, up(12, 0.9), 0.5]), None);
    }

    #[test]
    fn air_and_solids_are_dry_and_flooded_cells_are_water() {
        let col = Column::new(&[
            (10, "dwell:stone"),
            (12, "dwell:ladder[facing=north,flooded=true]"),
        ]);
        assert_eq!(col.at([0.5, up(10, 0.5), 0.5]), None);
        assert_eq!(col.at([0.5, up(11, 0.5), 0.5]), None);
        assert_eq!(
            col.at([0.5, up(12, 0.5), 0.5]).map(|e| e.fluid),
            Some(water())
        );
    }

    #[test]
    fn face_b_is_the_mirror_image_its_surface_above_the_cells_bottom() {
        // Face B's up is −y: the cell "above" is y − 1 and the surface is 1/8 above the bottom.
        let y = MIDPLANE_Y - 20;
        let col = Column::new(&[(y + 1, "dwell:stone"), (y, "dwell:water")]);
        assert_eq!(col.at([0.5, up(y, 0.1), 0.5]), None);
        assert_eq!(
            col.at([0.5, up(y, 0.2), 0.5]),
            Some(EyeFluid {
                fluid: water(),
                surface_y: up(y, 0.125),
                under: true,
            })
        );
        let deep = Column::new(&[(y, "dwell:water"), (y - 1, "dwell:water")]);
        assert_eq!(
            deep.at([0.5, up(y, 0.01), 0.5]).map(|e| e.surface_y),
            Some(up(y - 1, 0.125))
        );
    }

    #[test]
    fn deeper_than_the_scan_reports_the_last_cell_scanned() {
        let cells: Vec<(i32, &str)> = (0..200).map(|y| (y, "dwell:water")).collect();
        let col = Column::new(&cells);
        let at = col.at([0.5, up(0, 0.5), 0.5]).expect("under");
        assert_eq!(at.surface_y, crate::m_of_row(SURFACE_SCAN + 1));
    }

    crate::both_faces!(
        fn an_eye_just_above_the_surface_finds_it_within_the_reach_for_the_waterline(f: FaceFrame) {
            // Face-local: stone at row 9, water at row 10 under air; the surface at 0.875 of row 10.
            let col = Column(
                [(9, "dwell:stone"), (10, "dwell:water")]
                    .iter()
                    .map(|&(y, s)| ((0, f.row(y), 0), require(s)))
                    .collect(),
            );
            let surface = up(10, 0.875);
            let eye = |h: f64| f.point([0.5, surface + h, 0.5]);
            // 0.2 m above: under-only finds nothing; a 0.3 m reach finds the surface below.
            assert_eq!(col.at(eye(0.2)), None);
            let near = col.near(eye(0.2), 0.3).expect("within the reach");
            assert!(!near.under);
            assert!((f.y(near.surface_y) - surface).abs() < 1e-9);
            // Further than the reach (the eye in the air cell above): none.
            assert_eq!(col.near(eye(0.4), 0.3), None);
            // Just under: under, with the same surface.
            let under = col.near(eye(-0.05), 0.3).expect("under");
            assert!(under.under);
            assert!((f.y(under.surface_y) - surface).abs() < 1e-9);
        }
    );
}
