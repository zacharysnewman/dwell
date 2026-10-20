//! Whether the camera's eye is under a fluid's drawn surface (docs/WATER_RENDERING.md §1, §3
//! "Underwater"): what the renderer's underwater effects key on. Physics' `submerged_fraction`
//! counts whole liquid cells as wet, but the chunk mesher draws a fluid's top `surfaceDrop` below
//! its cell's top when the cell above is not the same fluid; going by the cell alone would put the
//! view underwater that much early. On face B "up" is −y (BIFACIAL_WORLD.md §3), so the surface
//! there lies `surfaceDrop` above the cell's bottom, as the mirrored chunk mesh draws it.

use dwell_blocks::{FLUIDS, FluidId, StateId, info};

use crate::{Face, cell_of_pos, face_of_y, m_of_row, m_of_rows_f};

/// The eye under a fluid's surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeFluid {
    pub fluid: FluidId,
    /// World y (m) of the drawn surface above the eye (on face B, below it in world y), when it is
    /// within [`SURFACE_SCAN`] cells; deeper, the top of the last cell scanned.
    pub surface_y: f64,
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

/// The fluid whose drawn surface the eye is under, if any. `voxel` reads the state at a cell.
pub fn eye_fluid(
    eye: [f64; 3],
    mut voxel: impl FnMut(i32, i32, i32) -> StateId,
) -> Option<EyeFluid> {
    let [x, y, z] = cell_of_pos(eye);
    let fluid = fluid_of(voxel(x, y, z))?;
    let up = match face_of_y(i64::from(y)) {
        Face::A => 1,
        Face::B => -1,
    };
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
    let under = if up > 0 {
        eye[1] < surface_y
    } else {
        eye[1] > surface_y
    };
    under.then_some(EyeFluid { fluid, surface_y })
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
            eye_fluid(eye, |x, y, z| *self.0.get(&(x, y, z)).unwrap_or(&AIR))
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
                surface_y: up(10, 0.875)
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
                surface_y: up(y, 0.125)
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
}
