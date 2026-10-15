//! Geometry of every baked shape (`shared/blocks/shapes.mjs`), the adaptive ones included: each is
//! a closed, outward-wound solid whose volume, side coverage and top surface agree with the fields
//! the meshers, the culling and the probes read. And the adaptive table matches its shared vector.

use dwell_blocks::shape::{SURFACE_TAG, faces_of, ray_enter_shape, solid_span_at};
use dwell_blocks::{ADAPTIVE_DOWN, ADAPTIVE_UP, SHAPES, ShapeInfo};

type V = [f64; 3];

fn pts(f: &dwell_blocks::ShapeFace) -> Vec<V> {
    f.v[..usize::from(f.count)]
        .iter()
        .map(|p| p.map(f64::from))
        .collect()
}

/// Twice the area vector of a polygon (Newell's method).
fn area2(p: &[V]) -> V {
    let mut n = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn shapes() -> impl Iterator<Item = (usize, &'static ShapeInfo)> {
    SHAPES.iter().enumerate().filter(|(_, s)| s.face_count > 0)
}

#[test]
fn every_shape_is_closed_and_wound_outward_with_its_volume() {
    for (i, s) in shapes() {
        let mut sum = [0.0; 3];
        let mut volume = 0.0;
        for f in faces_of(s) {
            let p = pts(f);
            assert!(p.len() >= 3, "shape {i}: a face of {} points", p.len());
            let n = area2(&p);
            for k in 0..3 {
                sum[k] += n[k];
            }
            // Divergence theorem: V = Σ (p₀ · n) · area / 3 over planar faces (n·area = n2 / 2).
            volume += (p[0][0] * n[0] + p[0][1] * n[1] + p[0][2] * n[2]) / 6.0;
            // Wound outward: a cell face along its axis; the top surface up (down when hanging).
            if f.tag < SURFACE_TAG {
                let axis = usize::from(f.tag / 2);
                let sign = if f.tag % 2 == 0 { 1.0 } else { -1.0 };
                assert!(n[axis] * sign > 0.0, "shape {i}: face tag {} wound inward", f.tag);
                assert_eq!(n[(axis + 1) % 3], 0.0, "shape {i}: face tag {} not in its plane", f.tag);
                assert_eq!(n[(axis + 2) % 3], 0.0, "shape {i}: face tag {} not in its plane", f.tag);
            } else {
                assert!(
                    (n[1] > 0.0) != s.inverted,
                    "shape {i}: the surface is wound the wrong way"
                );
            }
            // Convex polygons (the meshers fan-triangulate them).
            for k in 0..p.len() {
                let (a, b, c) = (p[k], p[(k + 1) % p.len()], p[(k + 2) % p.len()]);
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
                let t = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                assert!(t[0] * n[0] + t[1] * n[1] + t[2] * n[2] >= -1e-9, "shape {i}: a concave face");
            }
        }
        for (k, v) in sum.iter().enumerate() {
            assert!(v.abs() < 1e-6, "shape {i}: not closed (area vector {k} = {v})");
        }
        assert!(
            (volume - f64::from(s.volume)).abs() < 1e-5,
            "shape {i}: volume {volume} from its faces, {} in the table",
            s.volume
        );
    }
}

#[test]
fn side_faces_cover_exactly_their_profiles() {
    // Profile k is faces +X −X +Z −Z (tags 0 1 4 5); its area is the integral of the piecewise
    // linear profile (quarters of a cell). Hanging shapes hang their profiles from the top.
    for (i, s) in shapes() {
        for (k, tag) in [0u8, 1, 4, 5].into_iter().enumerate() {
            let area: f64 = faces_of(s)
                .iter()
                .filter(|f| f.tag == tag)
                .map(|f| {
                    let n = area2(&pts(f));
                    (n[0].abs() + n[2].abs()) / 2.0
                })
                .sum();
            let h = s.sides[k].map(|q| f64::from(q) / 4.0);
            let want = (h[0] + h[1]) / 4.0 + (h[1] + h[2]) / 4.0;
            assert!(
                (area - want).abs() < 1e-6,
                "shape {i}: side {k} has area {area}, its profile {want}"
            );
        }
    }
}

#[test]
fn the_top_surface_covers_the_cell_once_at_the_height_the_probes_read() {
    for (i, s) in shapes() {
        let projected: f64 = faces_of(s)
            .iter()
            .filter(|f| f.tag == SURFACE_TAG || f.tag == 2 && !s.inverted || f.tag == 3 && s.inverted)
            .map(|f| area2(&pts(f))[1].abs() / 2.0)
            .sum();
        assert!((projected - 1.0).abs() < 1e-6, "shape {i}: top covers {projected}");
        // A ray straight down (or up, hanging) enters the solid where `solid_span_at` says.
        for (x, z) in [(0.1, 0.2), (0.5, 0.5), (0.8, 0.3), (0.25, 0.75), (0.9, 0.9), (0.6, 0.05)] {
            let span = solid_span_at(s, x, z);
            let (origin, dir, want) = if s.inverted {
                ([x, -1.0, z], [0.0, 1.0, 0.0], span.lo + 1.0)
            } else {
                ([x, 2.0, z], [0.0, -1.0, 0.0], 2.0 - span.hi)
            };
            let hit = ray_enter_shape(s, [0.0; 3], origin, dir, 5.0, None, 0.0).expect("hits");
            assert!(
                (hit.t - want).abs() < 1e-5,
                "shape {i} at ({x}, {z}): the ray enters at {}, the span says {want}",
                hit.t
            );
        }
    }
}

#[test]
fn the_adaptive_table_matches_its_shared_vector() {
    let text = include_str!("../../../shared/blocks/adaptive_vectors.txt");
    let mut count = 0;
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        let i: usize = f[0].parse().expect("index");
        let (up, down): (u16, u16) = (
            f[f.len() - 2].parse().expect("up"),
            f[f.len() - 1].parse().expect("down"),
        );
        assert_eq!(ADAPTIVE_UP[i], up, "{line}");
        assert_eq!(ADAPTIVE_DOWN[i], down, "{line}");
        let (u, d) = (&SHAPES[usize::from(up)], &SHAPES[usize::from(down)]);
        assert!(!u.inverted && d.inverted && u.points == d.points, "{line}");
        // The neighbourhood's code at index order k is its around string's k-th symbol.
        let symbols: Vec<char> = f[1].chars().filter(|c| !matches!(c, '/' | '.')).collect();
        let mut rest = i;
        for c in symbols {
            assert_eq!(c, ['=', '-', '+'][rest % 3], "{line}");
            rest /= 3;
        }
        count += 1;
    }
    assert_eq!(count, 6561);
}

#[test]
fn ridge_shapes_are_convex_only_without_a_turning_crest() {
    // Half pyramid, ridge and ridge end are convex; the corner, T and cross are not (Phase 20
    // splits them).
    let mut seen = std::collections::BTreeSet::new();
    for (_, s) in shapes().filter(|(_, s)| s.fan && !s.inverted) {
        let raised = s.points.iter().flatten().filter(|&&q| q > 0).count();
        // Centre plus 0..4 edge midpoints raised: 1 pyramid, 2 end, 3 ridge or corner, 4 T, 5 cross.
        let straight = raised == 3 && (s.points[1][0] == s.points[1][2]);
        let convex_expected = raised <= 2 || straight;
        assert_eq!(s.convex, convex_expected, "{:?}", s.points);
        seen.insert(raised);
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
}
