//! The client's parts of the LOD grid (`client/src/lod/grid.ts`) that `dwell_world::lod` lacks:
//! section ids for hot-path maps, the section holding a point, and the face-B helpers. The rest of
//! the grid (coordinates, children, rows, bounds, kinds) is `dwell_world::lod`.

use dwell_world::lod::{
    LodCoord, MAX_LEVEL, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, first_row, last_row, section_origin,
    section_size, sections_across,
};
use dwell_world::{CELL_H, MIDPLANE_Y, MIRROR_SUM, cell_space, metres};

/// World metres, as the client's `Vec3`.
pub type Vec3 = [f64; 3];

/// The octree's corner (the root's minimum corner, −2¹⁶ on each axis) in cell space (x and z in
/// metres, y in rows).
pub const ORIGIN: Vec3 = [ORIGIN_X as f64, ORIGIN_Y as f64, ORIGIN_Z as f64];

/// The deepest level whose sections never straddle the midplane: it lies on their boundaries
/// (level 11, 65,536 m sections; only the root holds both faces).
pub const MAX_ALIGNED_LEVEL: i32 = max_aligned_level();

const fn max_aligned_level() -> i32 {
    let mut level = MAX_LEVEL;
    while level > 0 && (MIDPLANE_Y as i64 - ORIGIN_Y) % section_size(level) != 0 {
        level -= 1;
    }
    level
}

/// Sections across the octree at level 0 (and so at most rows): ids give i, the row and k this
/// many values each.
const SPAN: i64 = 1 << MAX_LEVEL;

/// A section as one number (level, i, row, k; the row counted from the world's first), for maps
/// on hot paths. Coordinates outside the octree or the world's rows map to −1.
pub type LodId = i64;

pub fn lod_id(level: i32, i: i32, j: i32, k: i32) -> LodId {
    if !(0..=MAX_LEVEL).contains(&level) {
        return -1;
    }
    let across = sections_across(level);
    let row = j - first_row(level);
    let (i64i, i64k) = (i64::from(i), i64::from(k));
    if i < 0 || k < 0 || i64i >= across || i64k >= across || row < 0 || j > last_row(level) {
        return -1;
    }
    ((i64::from(level) * SPAN + i64i) * SPAN + i64::from(row)) * SPAN + i64k
}

const _: () = assert!(
    (last_row(0) - first_row(0) + 1) as i64 <= SPAN,
    "the rows of level 0 fit an id"
);

pub fn id_of(c: LodCoord) -> LodId {
    lod_id(c.level, c.i, c.j, c.k)
}

/// The section at `level` holding world point p (m).
pub fn section_at(level: i32, p: Vec3) -> LodCoord {
    let s = section_size(level) as f64;
    let p = cell_space(p);
    let at = |a: usize| ((p[a] - ORIGIN[a]) / s).floor() as i32;
    LodCoord::new(level, at(0), at(1), at(2))
}

/// World metres of the section's min corner.
pub fn origin_m(c: LodCoord) -> Vec3 {
    let o = section_origin(c);
    metres([o.x as f64, o.y as f64, o.z as f64])
}

/// The section's box in world metres.
pub fn section_box(c: LodCoord) -> (Vec3, Vec3) {
    let lo = origin_m(c);
    let s = section_size(c.level) as f64;
    (lo, [lo[0] + s, lo[1] + s * CELL_H, lo[2] + s])
}

/// Whether the section lies wholly on face B (levels up to `MAX_ALIGNED_LEVEL` never straddle the
/// midplane): it is generated, and meshed, as the mirror image of a face-local section.
pub fn is_face_b_section(c: LodCoord) -> bool {
    c.level <= MAX_ALIGNED_LEVEL && section_origin(c).y < i64::from(MIDPLANE_Y)
}

/// The face-local height of the bottom of the mirror image of a face-B section: row r' of the
/// mirrored section is row 31 − r' of the section, at this + r' · cell.
pub fn mirror_section_origin(c: LodCoord) -> i64 {
    i64::from(MIRROR_SUM) + 1 - section_origin(c).y - section_size(c.level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::lod::{
        LodBounds, LodKind, ancestor, cell_index, child, chunk_of_lod, in_world, kind_from_bounds,
        lod_of_chunk, parent, row_face,
    };
    use dwell_world::{ChunkCoord, Face, MAX_CHUNK_Y, MIN_CHUNK_Y};

    const fn lc(level: i32, i: i32, j: i32, k: i32) -> LodCoord {
        LodCoord::new(level, i, j, k)
    }

    #[test]
    fn level_0_is_the_chunk_grid_the_root_holds_the_whole_disc() {
        for c in [
            ChunkCoord::new(0, 0, 0),
            ChunkCoord::new(-1, -2, 5),
            ChunkCoord::new(1561, 1562, -1563),
        ] {
            let l = lod_of_chunk(c);
            assert_eq!(chunk_of_lod(l), c);
            assert_eq!(origin_m(l), dwell_world::chunk_origin_m(c));
        }
        assert!(in_world(lc(MAX_LEVEL, 0, 0, 0)));
        assert!(!in_world(lc(MAX_LEVEL, 1, 0, 0)));
        assert_eq!(MAX_LEVEL, 12);
        assert_eq!(section_size(MAX_LEVEL), 1 << 17);
        assert_eq!(
            ancestor(
                lod_of_chunk(ChunkCoord::new(-1563, MIN_CHUNK_Y, 1562)),
                MAX_LEVEL
            ),
            lc(MAX_LEVEL, 0, 0, 0)
        );
    }

    #[test]
    fn the_root_is_centred_on_the_origin_rows_span_the_build_sphere() {
        assert_eq!(ORIGIN, [-65_536.0; 3]);
        let rows = |level: i32| last_row(level) - first_row(level) + 1;
        assert_eq!([rows(0), rows(6), rows(8), rows(12)], [3126, 50, 14, 1]);
        assert_eq!(first_row(12), 0);
        let bottom = ChunkCoord::new(0, MIN_CHUNK_Y, 0);
        assert_eq!(lod_of_chunk(bottom).j, first_row(0));
        assert!(in_world(lod_of_chunk(bottom)));
        assert!(in_world(lod_of_chunk(ChunkCoord::new(0, MAX_CHUNK_Y, 0))));
        assert!(!in_world(lod_of_chunk(ChunkCoord::new(
            0,
            MIN_CHUNK_Y - 1,
            0
        ))));
        assert!(!in_world(lod_of_chunk(ChunkCoord::new(
            0,
            MAX_CHUNK_Y + 1,
            0
        ))));
        let j8 = first_row(8);
        assert!(in_world(lc(8, 8, j8, 8)));
        assert!(in_world(lc(8, 8, j8 + 13, 8)));
        assert!(!in_world(lc(8, 8, j8 - 1, 8)));
        assert!(!in_world(lc(8, 8, j8 + 14, 8)));
        // Inside the rows but wholly outside the sphere: not in the world (ADR 0025).
        // (The sphere is in cells: 45,000 rows is 22.5 km up.)
        assert!(!in_world(section_at(4, [30_000.0, 22_500.0, 0.0])));
        assert!(in_world(section_at(4, [20_000.0, 22_500.0, 0.0])));
        // Up to level 11 a section lies wholly on one face; only the root holds the midplane.
        assert_eq!(MAX_ALIGNED_LEVEL, 11);
        let mid = dwell_world::MIDPLANE_M;
        for level in 0..=MAX_ALIGNED_LEVEL {
            let s = section_at(level, [0.0, mid, 0.0]);
            assert_eq!(origin_m(s)[1], mid);
            assert_eq!(row_face(s, -1), Face::B); // only its apron is below
            assert_eq!(row_face(s, 0), Face::A);
        }
        let straddle = section_at(MAX_LEVEL, [0.0, mid, 0.0]);
        assert_eq!(row_face(straddle, 0), Face::B);
        assert_eq!(row_face(straddle, 31), Face::A);
    }

    #[test]
    fn numbers_sections_uniquely_ids_differ_across_rows_and_fall_outside_the_world() {
        let mut ids = std::collections::HashSet::new();
        for j in first_row(0)..=last_row(0) {
            ids.insert(lod_id(0, 2048, j, 2048));
        }
        assert_eq!(ids.len(), 3126);
        assert!(!ids.contains(&-1));
        assert_eq!(lod_id(0, 2048, first_row(0) - 1, 2048), -1);
        assert_eq!(lod_id(0, 2048, last_row(0) + 1, 2048), -1);
        assert_eq!(lod_id(0, 4096, first_row(0), 0), -1);
        assert_ne!(lod_id(1, 0, first_row(1), 0), lod_id(0, 0, first_row(0), 0));
        // Exact in a double (the TS kept ids as numbers).
        let top = lod_id(0, 4095, last_row(0), 4095);
        assert!(top > 0 && top < 1 << 53);
    }

    #[test]
    fn has_children_nest_in_their_parent() {
        let p = lc(5, 100, 3, 101);
        for o in 0..8 {
            let c = child(p, o);
            assert_eq!(parent(c), p);
            let (x, pp) = (origin_m(c), origin_m(p));
            let h = section_size(4) as f64;
            assert_eq!(
                [x[0] - pp[0], x[1] - pp[1], x[2] - pp[2]],
                [
                    f64::from(o & 1) * h,
                    f64::from((o >> 1) & 1) * h * dwell_world::CELL_H,
                    f64::from(o >> 2) * h
                ]
            );
        }
    }

    #[test]
    fn knows_the_rim_cells_and_bounds() {
        assert!(in_world(section_at(4, [49_000.0, 0.0, 0.0])));
        assert!(!in_world(section_at(4, [50_200.0, 0.0, 0.0])));
        assert!(in_world(section_at(4, [35_500.0, 0.0, 35_500.0])));
        assert!(!in_world(section_at(4, [35_900.0, 0.0, 35_900.0])));
        assert_eq!(cell_index(-1, -1, -1), 0);
        assert_eq!(cell_index(0, 0, 0), 1 + 34 + 34 * 34);
        let b = LodBounds {
            lo: 20.0,
            hi: 60.0,
            lo_b: 20.0,
            hi_b: 60.0,
            any_inside: true,
            bifacial: true,
        };
        let at = |y: f64| section_at(2, [0.0, y, 0.0]);
        assert_eq!(kind_from_bounds(at(0.0), &b), LodKind::Content);
        assert_eq!(kind_from_bounds(at(300.0), &b), LodKind::Empty);
        let outside = LodBounds {
            any_inside: false,
            ..b
        };
        assert_eq!(kind_from_bounds(at(0.0), &outside), LodKind::Empty);
        // Face B: its terrain's face-local bounds, mirrored — ground toward the midplane, sky below.
        assert_eq!(kind_from_bounds(at(-10.0), &b), LodKind::Content);
        assert_eq!(kind_from_bounds(at(-300.0), &b), LodKind::Empty);
        let deep = LodBounds {
            lo: 1000.0,
            hi: 1100.0,
            lo_b: 1000.0,
            hi_b: 1100.0,
            ..b
        };
        assert_eq!(kind_from_bounds(at(0.0), &deep), LodKind::Buried);
        assert_eq!(kind_from_bounds(at(-10.0), &deep), LodKind::Buried);
        assert_eq!(MIDPLANE_Y, 0);
    }

    #[test]
    fn face_b_sections_and_their_mirror_origin() {
        let a = section_at(2, [0.0, 0.0, 0.0]);
        let b = section_at(2, [0.0, -3000.0, 0.0]);
        assert!(!is_face_b_section(a));
        assert!(is_face_b_section(b));
        assert!(is_face_b_section(section_at(11, [0.0, -3000.0, 0.0])));
        assert!(!is_face_b_section(section_at(12, [0.0, -3000.0, 0.0]))); // straddles
        // The mirror of the section's top is the mirrored section's bottom.
        let top = section_origin(b).y + section_size(2);
        assert_eq!(mirror_section_origin(b), i64::from(MIRROR_SUM) + 1 - top);
    }
}
