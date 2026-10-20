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

/// The octree's corner (−2²³, −2²³, −2²³) in metres.
pub const ORIGIN: Vec3 = [ORIGIN_X as f64, ORIGIN_Y as f64, ORIGIN_Z as f64];

/// The deepest level whose sections never straddle the midplane (2,048 m sections).
pub const MAX_ALIGNED_LEVEL: i32 = 6;

/// A section as one number (5 + 19 + 9 + 19 bits, the row counted from the world's first), for
/// maps on hot paths. Coordinates outside the octree or the world's rows map to −1.
pub type LodId = i64;

pub fn lod_id(level: i32, i: i32, j: i32, k: i32) -> LodId {
    if !(0..=MAX_LEVEL).contains(&level) {
        return -1;
    }
    let across = sections_across(level);
    let row = j - first_row(level);
    let (i64i, i64k) = (i64::from(i), i64::from(k));
    if i < 0 || k < 0 || i64i >= across || i64k >= across || !(0..512).contains(&row) {
        return -1;
    }
    if j > last_row(level) {
        return -1;
    }
    ((i64::from(level) * 524_288 + i64i) * 512 + i64::from(row)) * 524_288 + i64k
}

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

/// Whether the section lies wholly on face B (levels up to 6 never straddle the midplane): it is
/// generated, and meshed, as the mirror image of a face-local section.
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
    use dwell_world::{ChunkCoord, Face, WORLD_BOTTOM_Y, WORLD_MAX_Y};

    const fn lc(level: i32, i: i32, j: i32, k: i32) -> LodCoord {
        LodCoord::new(level, i, j, k)
    }

    #[test]
    fn level_0_is_the_chunk_grid_the_root_holds_the_whole_disc() {
        for c in [
            ChunkCoord::new(0, 0, 0),
            ChunkCoord::new(-1, -64, 5),
            ChunkCoord::new(255_999, 191, -256_000),
        ] {
            let l = lod_of_chunk(c);
            assert_eq!(chunk_of_lod(l), c);
            assert_eq!(
                origin_m(l),
                [
                    f64::from(c.x * 32),
                    f64::from(c.y * 32),
                    f64::from(c.z * 32)
                ]
            );
        }
        assert!(in_world(lc(MAX_LEVEL, 0, 0, 0)));
        assert!(!in_world(lc(MAX_LEVEL, 1, 0, 0)));
        assert_eq!(section_size(MAX_LEVEL), 1 << 24);
        assert_eq!(
            ancestor(
                lod_of_chunk(ChunkCoord::new(-256_000, -64, 255_999)),
                MAX_LEVEL
            ),
            lc(MAX_LEVEL, 0, 0, 0)
        );
    }

    #[test]
    fn counts_y_from_minus_2_23_too_rows_cover_both_faces_one_row_from_level_9() {
        assert_eq!(ORIGIN, [-8_388_608.0; 3]);
        let rows = |level: i32| last_row(level) - first_row(level) + 1;
        assert_eq!([rows(0), rows(6), rows(8), rows(19)], [512, 8, 3, 1]);
        assert_eq!(first_row(19), 0);
        let bottom = ChunkCoord::new(0, WORLD_BOTTOM_Y / 32, 0);
        assert_eq!(lod_of_chunk(bottom).j, first_row(0));
        assert!(in_world(lod_of_chunk(bottom)));
        assert!(!in_world(lod_of_chunk(ChunkCoord::new(
            0,
            WORLD_BOTTOM_Y / 32 - 1,
            0
        ))));
        assert!(!in_world(lod_of_chunk(ChunkCoord::new(
            0,
            WORLD_MAX_Y / 32,
            0
        ))));
        let j8 = first_row(8);
        assert!(in_world(lc(8, 1024, j8, 1024)));
        assert!(in_world(lc(8, 1024, j8 + 2, 1024)));
        assert!(!in_world(lc(8, 1024, j8 - 1, 1024)));
        assert!(!in_world(lc(8, 1024, j8 + 3, 1024)));
        // Up to level 6 a section lies wholly on one face; a coarser one holds the midplane.
        let mid = f64::from(MIDPLANE_Y);
        for level in 0..=6 {
            let s = section_at(level, [0.0, mid, 0.0]);
            assert_eq!(origin_m(s)[1], mid);
            assert_eq!(row_face(s, -1), Face::B); // only its apron is below
            assert_eq!(row_face(s, 0), Face::A);
        }
        let straddle = section_at(8, [0.0, mid, 0.0]);
        assert_eq!(row_face(straddle, 0), Face::B);
        assert_eq!(row_face(straddle, 31), Face::A);
    }

    #[test]
    fn numbers_sections_uniquely_ids_differ_across_rows_and_fall_outside_the_world() {
        let mut ids = std::collections::HashSet::new();
        for j in first_row(0)..=last_row(0) {
            ids.insert(lod_id(0, 262_144, j, 262_144));
        }
        assert_eq!(ids.len(), 512);
        assert!(!ids.contains(&-1));
        assert_eq!(lod_id(0, 262_144, first_row(0) - 1, 262_144), -1);
        assert_eq!(lod_id(0, 262_144, last_row(0) + 1, 262_144), -1);
        // Exact in a double (the TS keeps ids as numbers).
        let top = lod_id(0, (1 << 19) - 1, last_row(0), (1 << 19) - 1);
        assert!(top < 1 << 53);
    }

    #[test]
    fn has_children_nest_in_their_parent() {
        let p = lc(5, 1000, 3, 1001);
        for o in 0..8 {
            let c = child(p, o);
            assert_eq!(parent(c), p);
            let (x, pp) = (origin_m(c), origin_m(p));
            let h = section_size(4) as f64;
            assert_eq!(
                [x[0] - pp[0], x[1] - pp[1], x[2] - pp[2]],
                [
                    f64::from(o & 1) * h,
                    f64::from((o >> 1) & 1) * h,
                    f64::from(o >> 2) * h
                ]
            );
        }
    }

    #[test]
    fn knows_the_rim_cells_and_bounds_as_the_cpp_does() {
        assert!(in_world(section_at(10, [8_000_000.0, 0.0, 0.0])));
        assert!(!in_world(section_at(10, [8_230_000.0, 0.0, 0.0])));
        assert!(!in_world(section_at(10, [5_840_000.0, 0.0, 5_840_000.0])));
        assert_eq!(cell_index(-1, -1, -1), 0);
        assert_eq!(cell_index(0, 0, 0), 1 + 34 + 34 * 34);
        let c = section_at(2, [0.0; 3]);
        let b = LodBounds {
            lo: -40.0,
            hi: 60.0,
            lo_b: -40.0,
            hi_b: 60.0,
            any_inside: true,
            bifacial: true,
        };
        let at = |y: f64| section_at(2, [0.0, y, 0.0]);
        assert_eq!(kind_from_bounds(c, &b), LodKind::Content);
        assert_eq!(kind_from_bounds(at(300.0), &b), LodKind::Empty);
        assert_eq!(kind_from_bounds(at(-500.0), &b), LodKind::Buried);
        let outside = LodBounds {
            any_inside: false,
            ..b
        };
        assert_eq!(kind_from_bounds(c, &outside), LodKind::Empty);
        // Face B: its terrain's face-local bounds, mirrored — rock toward the midplane, sky below.
        assert_eq!(kind_from_bounds(at(-3000.0), &b), LodKind::Buried);
        assert_eq!(kind_from_bounds(at(-9000.0), &b), LodKind::Empty);
        assert_eq!(kind_from_bounds(at(-4100.0), &b), LodKind::Content);
        assert_eq!(MIDPLANE_Y, -2048);
    }

    #[test]
    fn face_b_sections_and_their_mirror_origin() {
        let a = section_at(2, [0.0, 0.0, 0.0]);
        let b = section_at(2, [0.0, -3000.0, 0.0]);
        assert!(!is_face_b_section(a));
        assert!(is_face_b_section(b));
        assert!(!is_face_b_section(section_at(8, [0.0, -3000.0, 0.0]))); // straddles
        // The mirror of the section's top is the mirrored section's bottom.
        let top = section_origin(b).y + section_size(2);
        assert_eq!(mirror_section_origin(b), i64::from(MIRROR_SUM) + 1 - top);
    }
}
