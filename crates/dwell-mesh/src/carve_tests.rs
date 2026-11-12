// both-faces: the carve is face-local geometry; its faces are tested through the mesher below
// (`mesher_tests.rs` `a_cut_log_is_carved_…`, both faces), here only the shape.
use super::*;

const LOOK: CutLook = CutLook {
    tile: Some("logCut"),
    tint: [255; 3],
    open: 2.5,
    kerf_width_m: 0.035,
    bite: dwell_protocol::chopping::BITE_LOOK,
    cracks: None,
};

/// The sum of the faces' area vectors (zero for a closed surface) and the enclosed volume.
fn closure(faces: &[Face]) -> ([f64; 3], f64) {
    let mut sum = [0.0; 3];
    let mut vol = 0.0;
    for f in faces {
        let a = area_vector(&f.pts);
        sum = [sum[0] + a[0], sum[1] + a[1], sum[2] + a[2]];
        let p = f.pts[0];
        vol += 0.5 * (p[0] * a[0] + p[1] * a[1] + p[2] * a[2]) / 3.0;
    }
    (sum, vol)
}

fn assert_closed(faces: &[Face], vol: f64) {
    let (s, v) = closure(faces);
    assert!(s.iter().all(|c| c.abs() < 1e-6), "open surface: {s:?}");
    assert!((v - vol).abs() < 1e-6, "volume {v}, expected {vol}");
}

fn notch(side: usize, m: f32) -> Cut {
    let mut c = Cut::default();
    c.notch[side] = m;
    c
}

#[test]
fn an_uncut_cell_is_the_cube() {
    for grain in 0..3 {
        let f = carve(grain, &Cut::default(), &LOOK);
        assert_eq!(f.len(), 6, "grain {grain}");
        assert_closed(&f, 1.0);
        assert!(f.iter().all(|f| !matches!(f.part, Part::Cut(_))));
    }
}

#[test]
fn a_notch_is_a_v_its_depth_and_opening_from_the_cut() {
    // A standing log (grain y, ½ m tall), notched 0.08 m into its −x side (side 0): the V opens
    // 2.5 × 0.08 = 0.2 m along the grain (0.4 of the row), its apex 0.08 m in at the row's middle.
    let f = carve(1, &notch(0, 0.08), &LOOK);
    let apex = [0.08, 0.5, 0.0];
    let has = |p: [f64; 3]| {
        f.iter().any(|f| {
            f.pts
                .iter()
                .any(|q| (0..3).all(|k| (q[k] - p[k]).abs() < 1e-6))
        })
    };
    assert!(has(apex) && has([0.08, 0.5, 1.0]), "the apex line");
    assert!(has([0.0, 0.3, 0.0]) && has([0.0, 0.7, 1.0]), "the mouth");
    // Its two cut faces, sloping in from the mouth to the apex; no bark left over the mouth.
    let cuts: Vec<_> = f.iter().filter(|f| f.part == Part::Cut(0)).collect();
    assert_eq!(cuts.len(), 2);
    for c in &cuts {
        let n = normal(&c.pts);
        assert!(
            n[0] < -0.5 && n[1].abs() > 0.1,
            "slopes out of the side: {n:?}"
        );
    }
    for b in f.iter().filter(|f| f.part == Part::Bark(0)) {
        let ys: Vec<f64> = b.pts.iter().map(|p| p[1]).collect();
        let (lo, hi) = (
            ys.iter().copied().fold(1.0, f64::min),
            ys.iter().copied().fold(0.0, f64::max),
        );
        assert!(
            hi <= 0.3 + 1e-6 || lo >= 0.7 - 1e-6,
            "bark over the notch: {ys:?}"
        );
        assert!(
            b.pts.iter().all(|p| p[0].abs() < 1e-12),
            "bark on the side's plane"
        );
    }
    // The wedge cut out: ½ × 0.4 (cells along the grain) × 0.08 (cells in) × 1 across.
    assert_closed(&f, 1.0 - 0.5 * 0.4 * 0.08);
}

#[test]
fn a_deep_notch_opens_over_the_whole_row_and_stays_closed() {
    let f = carve(1, &notch(3, 0.45), &LOOK);
    // Side 3 is +z; its apex 0.45 m in from z = 1.
    assert!(
        f.iter()
            .any(|f| f.pts.iter().any(|p| (p[2] - 0.55).abs() < 1e-6))
    );
    // Opened as wide as the row (clamped): the V's mouth is the row's ends.
    assert_closed(&f, 1.0 - 0.5 * 1.0 * 0.45);
    assert!(
        !f.iter().any(|f| f.part == Part::Bark(3)),
        "no bark left on the cut side"
    );
}

#[test]
fn kerfs_are_slits_past_the_notch() {
    let mut c = notch(0, 0.08);
    c.kerf[0][0] = 0.04;
    c.kerf[0][1] = 0.02;
    let f = carve(1, &c, &LOOK);
    // Band 0's slit reaches 0.12 m in, band 1's 0.10, each with walls square to the grain.
    let deepest = |lo: f64, hi: f64| {
        f.iter()
            .flat_map(|f| f.pts.iter())
            .filter(|p| p[1] > lo && p[1] < hi && p[0] < 0.5)
            .map(|p| p[0])
            .fold(0.0, f64::max)
    };
    assert!((deepest(0.0, 0.5) - 0.12).abs() < 1e-6);
    assert!((deepest(0.5, 1.0) - 0.10).abs() < 1e-6);
    let walls = f
        .iter()
        .filter(|f| f.part == Part::Cut(0) && normal(&f.pts)[1].abs() > 0.999)
        .count();
    assert!(walls >= 4, "two walls per slit, got {walls}");
    let (s, _) = closure(&f);
    assert!(s.iter().all(|c| c.abs() < 1e-6), "open: {s:?}");
}

#[test]
fn a_lying_log_is_cut_across_its_length() {
    // Along x (grain 0, 1 m long), notched 0.1 m down from its top (side 1: +y, ½ m: 0.2 of a
    // cell): the V opens 0.25 m along x.
    let f = carve(0, &notch(1, 0.1), &LOOK);
    let apex = f
        .iter()
        .flat_map(|f| f.pts.iter())
        .map(|p| p[1])
        .fold(1.0, f64::min);
    assert!((apex - 0.0).abs() < 1e-6, "the bottom stays");
    assert!(f.iter().any(|f| {
        f.pts
            .iter()
            .any(|p| (p[1] - 0.8).abs() < 1e-6 && (p[0] - 0.5).abs() < 1e-6)
    }));
    assert_closed(&f, 1.0 - 0.5 * 0.25 * 0.2);
    // Along z the same, by its side 3 (+y).
    let g = carve(2, &notch(3, 0.1), &LOOK);
    assert_closed(&g, 1.0 - 0.5 * 0.25 * 0.2);
}

#[test]
fn two_sides_cut_meet_without_a_gap() {
    let mut c = notch(0, 0.2);
    c.notch[2] = 0.15;
    c.notch[1] = 0.1;
    c.kerf[2][1] = 0.05;
    let f = carve(1, &c, &LOOK);
    let (s, v) = closure(&f);
    assert!(s.iter().all(|c| c.abs() < 1e-6), "open: {s:?}");
    assert!(v > 0.5 && v < 1.0);
}

#[test]
fn opposite_sides_never_cut_the_cell_in_two() {
    let mut c = notch(0, 0.6);
    c.notch[1] = 0.6;
    let f = carve(1, &c, &LOOK);
    let (_, v) = closure(&f);
    assert!(v > 0.0);
    let xs: Vec<f64> = f.iter().flat_map(|f| f.pts.iter()).map(|p| p[0]).collect();
    // The waist left is at least MIN_LEFT.
    assert!(
        f.iter()
            .all(|f| f.pts.iter().all(|p| p[0] >= 0.0 && p[0] <= 1.0)),
        "{xs:?}"
    );
}

#[test]
fn a_mirrored_cut_swaps_its_vertical_sides_only() {
    let mut c = notch(1, 0.1);
    c.notch[2] = 0.2;
    // Grain x: sides 0 and 1 run along y (swap), 2 and 3 along z (stay).
    let m = mirror_cut(0, &c);
    assert_eq!(m.notch, [0.1, 0.0, 0.2, 0.0]);
    // A standing log's sides are all level: unchanged.
    assert_eq!(mirror_cut(1, &c), c);
}

/// A full-width octagon's outline (ADR 0072's chamfer), on the two axes across the grain.
fn octagon(c: f64) -> Vec<[f64; 2]> {
    vec![
        [c, 0.0],
        [1.0 - c, 0.0],
        [1.0, c],
        [1.0, 1.0 - c],
        [1.0 - c, 1.0],
        [c, 1.0],
        [0.0, 1.0 - c],
        [0.0, c],
    ]
}

#[test]
fn a_notch_in_an_octagonal_log_is_carved_inside_its_octagon() {
    // Round 2 of the chopping prototype: red before, a cut log's carve was the cube (its corners
    // came back square: points outside the octagon, no chamfer bark).
    let c = 0.2929;
    let oct = octagon(c);
    let inside = |p: &[f64; 3]| {
        let (a, b) = (p[0], p[2]);
        a + b >= c - 1e-6
            && (1.0 - a) + b >= c - 1e-6
            && a + (1.0 - b) >= c - 1e-6
            && (1.0 - a) + (1.0 - b) >= c - 1e-6
    };
    // Uncut: the octagonal prism, closed, its volume the octagon's area.
    let f = carve_in(1, &Cut::default(), &LOOK, Some(&oct));
    assert_closed(&f, 1.0 - 2.0 * c * c);
    // A notch on side 0 (−x) and a back cut on side 1 (+x): closed, every point inside the
    // octagon, the chamfers bark where wood is left, the notch's faces fresh wood.
    for cut in [notch(0, 0.3), {
        let mut k = notch(0, 0.3);
        k.kerf[1][0] = 0.25;
        k
    }] {
        let f = carve_in(1, &cut, &LOOK, Some(&oct));
        let (s, v) = closure(&f);
        assert!(s.iter().all(|x| x.abs() < 1e-3), "open surface: {s:?}");
        assert!(v > 0.4 && v < 1.0 - 2.0 * c * c, "volume {v}");
        assert!(
            f.iter().all(|f| f.pts.iter().all(inside)),
            "outside the octagon"
        );
        assert!(
            f.iter().any(|f| matches!(f.part, Part::Inner(_))),
            "chamfer bark"
        );
        assert!(f.iter().any(|f| f.part == Part::Cut(0)), "the notch");
    }
}
