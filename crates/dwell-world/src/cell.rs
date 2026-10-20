//! Cells and metres (HALF_HEIGHT_GRID.md §2). A cell is 1 m × [`CELL_H`] × 1 m. Voxel algorithms
//! work in cell space (x and z in metres, y in rows); physics, the camera, rendering, audio and
//! terrain heights work in metres. Every conversion between the two goes through these helpers,
//! so the cell height is one constant.

use crate::voxel::{CHUNK_SIZE, ChunkCoord};
use dwell_protocol::world;

/// A cell's height (m).
pub const CELL_H: f64 = dwell_protocol::CELL_HEIGHT;
/// A cell's size (m): x, y, z.
pub const CELL: [f64; 3] = [1.0, CELL_H, 1.0];
/// A chunk's size (m): x, y, z.
pub const CHUNK_EXTENT_M: [f64; 3] = [
    CHUNK_SIZE as f64,
    CHUNK_SIZE as f64 * CELL_H,
    CHUNK_SIZE as f64,
];

/// The midplane (m), where face A starts (BIFACIAL_WORLD.md).
pub const MIDPLANE_M: f64 = world::MIDPLANE_M;
/// Face B's lowest point (m).
pub const WORLD_BOTTOM_M: f64 = world::WORLD_BOTTOM_M;
/// Generated terrain stays below this (m).
pub const WORLD_MAX_M: f64 = world::WORLD_MAX_M;
/// Terrain: water fills open space below this (m).
pub const SEA_LEVEL_M: f64 = world::SEA_LEVEL_M;
/// Face B's height y (m) is face A's `MIRROR_M − y`: the mirror about the midplane.
pub const MIRROR_M: f64 = 2.0 * MIDPLANE_M;

const _: () = assert!(CELL_H > 0.0 && CELL_H <= 1.0, "a cell is at most 1 m tall");
const _: () = assert!(
    (CHUNK_SIZE as f64 * CELL_H) == ((CHUNK_SIZE as f64 * CELL_H) as i64) as f64,
    "a chunk is a whole number of metres tall"
);

/// The row holding height `y` (m): rows are half-open, `[m_of_row(r), m_of_row(r + 1))`.
pub fn row_of_m(y: f64) -> i32 {
    (y / CELL_H).floor() as i32
}

/// The bottom of row `row` (m).
pub fn m_of_row(row: i32) -> f64 {
    f64::from(row) * CELL_H
}

/// A height (m) in rows, fractional: the inverse of [`m_of_rows_f`].
pub fn rows_f_of_m(y: f64) -> f64 {
    y / CELL_H
}

/// A fractional number of rows in metres.
pub fn m_of_rows_f(rows: f64) -> f64 {
    rows * CELL_H
}

/// The rows needed to hold a height of `m` metres (at least 0).
pub fn rows_for_m(m: f64) -> i32 {
    (m / CELL_H).ceil().max(0.0) as i32
}

/// The cell holding a position (m).
pub fn cell_of_pos(p: [f64; 3]) -> [i32; 3] {
    [p[0].floor() as i32, row_of_m(p[1]), p[2].floor() as i32]
}

/// A cell's minimum corner (m).
pub fn pos_of_cell(c: [i32; 3]) -> [f64; 3] {
    [f64::from(c[0]), m_of_row(c[1]), f64::from(c[2])]
}

/// A cell's centre (m).
pub fn centre_of_cell(c: [i32; 3]) -> [f64; 3] {
    let p = pos_of_cell(c);
    [p[0] + 0.5, p[1] + 0.5 * CELL_H, p[2] + 0.5]
}

/// A position (m) in cell space (y in rows, fractional).
pub fn cell_space(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[1] / CELL_H, p[2]]
}

/// A point in cell space back in metres.
pub fn metres(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[1] * CELL_H, p[2]]
}

/// A direction (m) in cell space, not renormalised: a ray `o + t·d` in metres is `cell_space(o) +
/// t·cell_dir(d)` in cell space, with the same `t`.
pub fn cell_dir(d: [f64; 3]) -> [f64; 3] {
    [d[0], d[1] / CELL_H, d[2]]
}

/// A surface normal in cell space as a unit normal in metres (the inverse transpose of the
/// metres-from-cells scale). At 1 m cells it is returned unchanged, bit for bit.
pub fn normal_to_m(n: [f64; 3]) -> [f64; 3] {
    if CELL_H == 1.0 {
        return n;
    }
    let m = [n[0], n[1] / CELL_H, n[2]];
    let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    if len == 0.0 {
        return n;
    }
    [m[0] / len, m[1] / len, m[2] / len]
}

/// The chunk holding a position (m).
pub fn chunk_of_pos(p: [f64; 3]) -> ChunkCoord {
    let [x, y, z] = cell_of_pos(p);
    crate::voxel::chunk_of(x, y, z)
}

/// A chunk's minimum corner (m).
pub fn chunk_origin_m(c: ChunkCoord) -> [f64; 3] {
    pos_of_cell([c.x * CHUNK_SIZE, c.y * CHUNK_SIZE, c.z * CHUNK_SIZE])
}

/// A chunk's centre (m).
pub fn chunk_centre_m(c: ChunkCoord) -> [f64; 3] {
    let o = chunk_origin_m(c);
    [
        o[0] + 0.5 * CHUNK_EXTENT_M[0],
        o[1] + 0.5 * CHUNK_EXTENT_M[1],
        o[2] + 0.5 * CHUNK_EXTENT_M[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_metres_round_trip() {
        for row in [-4097, -1, 0, 1, 31, 32, 6143] {
            assert_eq!(row_of_m(m_of_row(row)), row, "row {row}");
            // Just below a row's bottom is the row below; just above, the row itself.
            assert_eq!(row_of_m(m_of_row(row) - 1e-9), row - 1);
            assert_eq!(row_of_m(m_of_row(row) + 1e-9), row);
        }
        assert_eq!(rows_for_m(0.0), 0);
        assert_eq!(rows_for_m(CELL_H), 1);
        assert_eq!(rows_for_m(CELL_H * 1.01), 2);
    }

    #[test]
    fn cells_and_chunks_from_positions() {
        let c = [-3, -5, 7];
        let p = pos_of_cell(c);
        assert_eq!(cell_of_pos(p), c);
        assert_eq!(cell_of_pos(centre_of_cell(c)), c);
        let k = ChunkCoord::new(-2, 3, 5);
        assert_eq!(chunk_of_pos(chunk_origin_m(k)), k);
        assert_eq!(chunk_of_pos(chunk_centre_m(k)), k);
        assert_eq!(metres(cell_space([1.5, -2.25, 3.0])), [1.5, -2.25, 3.0]);
    }

    #[test]
    fn a_ray_keeps_its_parameter_in_cell_space() {
        let (o, d, t) = ([0.25, 10.0, -1.0], [0.6, -0.8, 0.0], 2.5);
        let at_m = [o[0] + t * d[0], o[1] + t * d[1], o[2] + t * d[2]];
        let (oc, dc) = (cell_space(o), cell_dir(d));
        let at_c = [oc[0] + t * dc[0], oc[1] + t * dc[1], oc[2] + t * dc[2]];
        for (a, b) in metres(at_c).iter().zip(at_m) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn normals_stay_unit_and_upward_faces_stay_up() {
        let n = normal_to_m([0.6, 0.8, 0.0]);
        assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-12);
        assert!(n[1] > 0.0 && n[0] > 0.0);
        assert_eq!(normal_to_m([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(normal_to_m([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_metre_twins_are_the_rows_times_the_cell_height() {
        use crate::voxel::{MIDPLANE_Y, SEA_LEVEL, WORLD_BOTTOM_Y, WORLD_MAX_Y};
        assert_eq!(MIDPLANE_M, m_of_row(MIDPLANE_Y));
        assert_eq!(WORLD_BOTTOM_M, m_of_row(WORLD_BOTTOM_Y));
        assert_eq!(WORLD_MAX_M, m_of_row(WORLD_MAX_Y));
        assert_eq!(SEA_LEVEL_M, m_of_row(SEA_LEVEL));
        // The mirror in rows (y ↦ MIRROR_SUM − y, a cell's index) and in metres agree: a cell's
        // bottom maps to its mirror's top.
        let row = MIDPLANE_Y + 5;
        let mirrored = crate::voxel::mirror_y(row);
        assert_eq!(MIRROR_M - m_of_row(row), m_of_row(mirrored + 1));
    }
}
