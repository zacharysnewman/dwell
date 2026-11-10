use super::*;
use crate::{AIR, info, state};

const CELL: [f64; 3] = [1.0, 0.5, 1.0];

fn log(axis: &str) -> StateId {
    state(&format!("dwell:log[axis={axis}]")).expect("the log is in the registry")
}

/// A world of `cells` (offsets from the origin) round the cell at the origin, air elsewhere.
fn world(cells: Vec<([i32; 3], StateId)>) -> impl Fn([i32; 3]) -> StateId {
    move |o| cells.iter().find(|(c, _)| *c == o).map_or(AIR, |(_, m)| *m)
}

fn count(faces: &[Face], f: impl Fn(&Part) -> bool) -> usize {
    faces.iter().filter(|x| f(&x.part)).count()
}

/// Signed area vector of a polygon (twice its area along its normal).
fn area(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

#[test]
fn the_log_is_an_octagon_by_its_data() {
    let (c, axis) = section_of(log("y")).expect("logs have a cross-section");
    assert!(c.chamfer > 0.0 && c.chamfer < 0.5);
    assert_eq!(axis, 1);
    assert_eq!(section_of(log("x")).map(|s| s.1), Some(0));
    assert!(section_of(state("dwell:stone").unwrap()).is_none());
}

#[test]
fn a_lone_log_is_an_octagonal_prism_with_outward_faces() {
    for axis in ["y", "x", "z"] {
        let m = log(axis);
        let f = open_faces(m, &world(vec![]), &|_| false, CELL);
        assert_eq!(count(&f, |p| matches!(p, Part::Cap(_))), 2, "{axis}");
        assert_eq!(count(&f, |p| matches!(p, Part::Side(_))), 4, "{axis}");
        assert_eq!(count(&f, |p| matches!(p, Part::Chamfer(..))), 4, "{axis}");
        for cap in f.iter().filter(|x| matches!(x.part, Part::Cap(_))) {
            assert_eq!(cap.pts.len(), 8, "{axis}: an octagonal cap");
        }
        // Outward: every face's normal points away from the cell's centre.
        for face in &f {
            let n = area(&face.pts);
            let mid = face
                .pts
                .iter()
                .fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
            let k = face.pts.len() as f64;
            let out = [mid[0] / k - 0.5, mid[1] / k - 0.5, mid[2] / k - 0.5];
            assert!(
                n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0,
                "{axis} {:?}",
                face.part
            );
        }
    }
}

#[test]
fn the_chamfer_is_square_in_metres_on_half_metre_cells() {
    // An x log's cross-section is z (1 m) by y (½ m): the cut is the same metres both ways.
    let ring = outline(0.3, 0, 0, CELL);
    let (p, q) = (ring[0].0, ring[1].0); // (u = y, v = z): (0, cv) then (cu, 0)
    assert!((q[0] * CELL[1] - p[1] * CELL[2]).abs() < 1e-12);
    assert!((q[0] - 0.3).abs() < 1e-12, "0.3 × ½ m over ½ m cells");
}

#[test]
fn two_same_axis_logs_share_a_full_width_side_and_draw_no_face_between() {
    let m = log("y");
    let at = world(vec![([1, 0, 0], m)]);
    let f = open_faces(m, &at, &|_| false, CELL);
    // No face on +X (the shared side) and its two corners squared: two chamfers left.
    assert_eq!(count(&f, |p| *p == Part::Side(0)), 0);
    assert_eq!(count(&f, |p| matches!(p, Part::Chamfer(..))), 2);
    // The +Z and −Z sides reach the +X edge of the cell: full width on that end.
    for side in f
        .iter()
        .filter(|x| matches!(x.part, Part::Side(4) | Part::Side(5)))
    {
        assert!(side.pts.iter().any(|p| p[0] == 1.0), "{:?}", side.pts);
    }
    // The neighbour is the mirror image: no face on its −X.
    let other = world(vec![([-1, 0, 0], m)]);
    let g = open_faces(m, &other, &|_| false, CELL);
    assert_eq!(count(&g, |p| *p == Part::Side(1)), 0);
}

#[test]
fn logs_of_different_axes_do_not_join() {
    let at = world(vec![([1, 0, 0], log("z"))]);
    let f = open_faces(log("y"), &at, &|_| false, CELL);
    assert_eq!(count(&f, |p| *p == Part::Side(0)), 1);
    assert_eq!(count(&f, |p| matches!(p, Part::Chamfer(..))), 4);
}

#[test]
fn a_trunk_draws_no_caps_between_its_cells_and_none_on_stone() {
    let m = log("y");
    let stone = state("dwell:stone").unwrap();
    let at = world(vec![([0, 1, 0], m), ([0, -1, 0], stone)]);
    let f = open_faces(m, &at, &|n| n == stone, CELL);
    assert_eq!(count(&f, |p| matches!(p, Part::Cap(_))), 0);
}

#[test]
fn a_cap_shows_where_the_log_beyond_is_narrower() {
    // This cell is joined on +X; the one above isn't (its corners chamfered): the cap must show.
    let m = log("y");
    let at = world(vec![([1, 0, 0], m), ([0, 1, 0], m)]);
    let f = open_faces(m, &at, &|_| false, CELL);
    assert_eq!(count(&f, |p| *p == Part::Cap(true)), 1);
    // And the one above sees a wider log below: its bottom cap is hidden.
    let above = world(vec![([0, -1, 0], m), ([1, -1, 0], m)]);
    let g = open_faces(m, &above, &|_| false, CELL);
    assert_eq!(count(&g, |p| *p == Part::Cap(false)), 0);
}

#[cfg(feature = "fixture")]
#[test]
fn a_cross_section_added_by_data_alone_joins_the_logs() {
    // The test registry's `test:post` (shared/blocks/fixtures/test.json): its own chamfer, the
    // logs' join group.
    let post = crate::fixture::STATES
        .iter()
        .find(|s| s.name.starts_with("test:post[axis=y"))
        .expect("the fixture's post");
    let c = post.cross_section.expect("a cross-section from data");
    assert!((c.chamfer - 0.15).abs() < 1e-6);
    let wood = info(log("y")).cross_section.unwrap();
    assert_eq!(c.joins, wood.joins);
    let ring = outline(f64::from(c.chamfer), 1, 0, CELL);
    assert_eq!(ring.len(), 8);
    assert!((ring[1].0[0] - 0.15).abs() < 1e-6);
}

/// The cap's pieces in one plane: its heart and the bark ring's bands. Their areas sum to the
/// whole cap, each piece outward along the axis.
fn cap_split(joined: u8, axis: usize, end: bool) -> Vec<Face> {
    let (c, _) = section_of(log("y")).expect("logs have a cross-section");
    let pieces = cap_pieces(&c, axis, joined, CELL, end);
    let whole = faces(&c, axis, joined, CELL)
        .into_iter()
        .find(|f| f.part == Part::Cap(end))
        .expect("a cap");
    let sum: f64 = pieces.iter().map(|f| area(&f.pts)[axis]).sum();
    let want = area(&whole.pts)[axis];
    assert!((sum - want).abs() < 1e-9, "pieces {sum} vs cap {want}");
    let sign = if end { 1.0 } else { -1.0 };
    for f in &pieces {
        assert!(area(&f.pts)[axis] * sign > 0.0, "{:?} faces inward", f.part);
    }
    pieces
}

#[test]
fn a_lone_logs_bark_ring_follows_the_octagon() {
    // Owner, 2026-11-09: "the outside bark ring should follow the octagon, it doesn't, it gets
    // cut off on the corners." Every one of the octagon's eight edges carries a band of the
    // ring's width, the chamfers too, and the heart is the octagon moved in by that width.
    let (c, _) = section_of(log("y")).unwrap();
    assert!(
        c.ring > 0.0 && c.heart.is_some(),
        "the log's data names a ring and a heart"
    );
    for axis in 0..3 {
        for end in [true, false] {
            let p = cap_split(0, axis, end);
            assert_eq!(count(&p, |x| matches!(x, Part::Ring(_, Some(_)))), 4);
            assert_eq!(
                count(&p, |x| matches!(x, Part::Ring(_, None))),
                4,
                "chamfers banded"
            );
            let heart = p.iter().find(|f| f.part == Part::Cap(end)).unwrap();
            assert_eq!(heart.pts.len(), 8, "the heart is an octagon");
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let w_m = f64::from(c.ring) * CELL[u].min(CELL[v]);
            // In metres every heart corner is at least the ring's width in from the cell's edges.
            for q in &heart.pts {
                for (a, t) in [(u, q[u]), (v, q[v])] {
                    let d = t.min(1.0 - t) * CELL[a];
                    assert!(
                        d >= w_m - 1e-9,
                        "axis {axis}: corner {q:?} {d} m from the edge"
                    );
                }
            }
        }
    }
}

#[test]
fn joined_sides_have_no_ring() {
    // Logs joined on a side: no band along it (the joined wall's top is one surface of end
    // grain), and the heart reaches the cell's edge there, while the open sides keep theirs.
    let pu = face_index(2, true); // axis y: u = z, v = x; +u is +z.
    for end in [true, false] {
        let p = cap_split(1 << pu, 1, end);
        assert!(
            !p.iter().any(|f| f.part == Part::Ring(end, Some(pu))),
            "a band along the joined side"
        );
        assert!(
            p.iter()
                .any(|f| matches!(f.part, Part::Ring(_, Some(s)) if s != pu))
        );
        let heart = p.iter().find(|f| f.part == Part::Cap(end)).unwrap();
        assert_eq!(
            heart
                .pts
                .iter()
                .filter(|q| (q[2] - 1.0).abs() < 1e-9)
                .count(),
            2,
            "the heart runs to the joined edge"
        );
    }
    // Joined all round: no ring at all, the heart is the whole square cap.
    let all = (1 << face_index(0, true))
        | (1 << face_index(0, false))
        | (1 << face_index(2, true))
        | (1 << face_index(2, false));
    let p = cap_split(all, 1, true);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].part, Part::Cap(true));
}

#[test]
fn no_ring_returns_the_cap_whole() {
    let c = CrossSection {
        chamfer: 0.2,
        joins: "x",
        ring: 0.0,
        heart: None,
    };
    let p = cap_pieces(&c, 1, 0, CELL, true);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].pts.len(), 8);
}

#[test]
fn a_wood_ramp_is_the_log_octagon_cut_by_its_slant() {
    // Phase 55 T4 (owner, 2026-11-09: "everything log/tree should be octagonal"): the wood ramp
    // takes the log's cross-section by data (`crossSection: "of"`), and its faces are the prism
    // clipped to the ramp: every point inside the octagon, the slant a `Cut`, the uncut end whole.
    let m = state("dwell:log_ramp[facing=west,flooded=false,half=bottom,shape=ramp]").unwrap();
    assert!(is_octagon(m), "the wood ramp has the log's octagon");
    let fs = state_faces(m, 0, CELL);
    assert!(fs.iter().any(|f| f.part == Part::Cut), "the slant is a cut");
    let c = 0.2929 * 0.5 + 1e-6;
    for f in &fs {
        for p in &f.pts {
            let (du, dv) = (p[0].min(1.0 - p[0]), p[2].min(1.0 - p[2]));
            assert!(
                du + dv >= c - 2e-6,
                "{:?} point {p:?} outside the octagon",
                f.part
            );
        }
    }
    assert!(
        cap_whole(m, 0, CELL, false),
        "its bottom is the whole octagon"
    );
    assert!(!cap_whole(m, 0, CELL, true), "its top is cut away");
    // A log under it: its top is covered by the ramp.s whole bottom.
    let log = log("y");
    let up = world(vec![([0, 1, 0], m)]);
    assert!(
        !open_faces(log, &up, &|_| false, CELL)
            .iter()
            .any(|f| f.part == Part::Cap(true)),
        "the log's top under the ramp's whole end is hidden"
    );
}
