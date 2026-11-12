// both-faces: the carve is face-local geometry (a face-B chunk's cut is mirrored first,
// `mirror_cut`, tested below); the faces through the mesher on both faces are in `mesher_tests.rs`
// `a_bitten_log_is_carved_where_it_was_struck_…`.
use super::super::{Face, Part, area_vector, carve_in, mirror_cut};
use super::*;
use dwell_protocol::chopping::{self as chop, ToolDef};

fn look() -> CutLook {
    chop::wood("dwell:log").expect("the log is a wood").cut
}

fn axe() -> &'static ToolDef {
    chop::tool("dwell:stone_axe").expect("the stone axe is data")
}

/// A swing as the server lands it (`server/chopping.rs` `swing`): the band the head comes in at,
/// the cut, then the bite where it struck (`u` across the side).
fn hit(c: &mut Cut, grain: usize, side: usize, aimed_band: usize, u: f64) -> f64 {
    let band = c.open_band(side, aimed_band);
    c.swing(axe(), grain, side, band, 1.0);
    c.bite(grain, side, band, u, &look().bite)
}

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

fn assert_closed(faces: &[Face]) -> f64 {
    let (s, v) = closure(faces);
    assert!(s.iter().all(|c| c.abs() < 1e-6), "open surface: {s:?}");
    v
}

/// The span across side 0 (−x) of a standing log (along z) its cut faces cover.
fn cut_span(faces: &[Face], side: usize) -> (f64, f64) {
    faces
        .iter()
        .filter(|f| f.part == Part::Cut(side))
        .flat_map(|f| f.pts.iter())
        .filter(|p| p[0] > 1e-6)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[2]), hi.max(p[2]))
        })
}

#[test]
fn one_hit_bites_no_wider_than_the_axe_head_centred_where_it_struck() {
    // Red before item 26: the kerf was a slit across the whole side (z 0 to 1).
    let l = look();
    let mut c = Cut::default();
    let at = hit(&mut c, 1, 0, 0, 0.3);
    assert!((at - 0.3).abs() < 1e-9, "the bite is where it struck: {at}");
    let f = carve_bites(1, &c, &l, None);
    let v = assert_closed(&f);
    assert!(v < 1.0 - 1e-4 && v > 0.95, "a small bite out: {v}");
    let (lo, hi) = cut_span(&f, 0);
    let half = 0.5 * l.bite.width_m;
    assert!(
        lo >= 0.3 - half - 1e-9 && hi <= 0.3 + half + 1e-9,
        "no wider than the head ({}) centred on the hit: {lo}..{hi}",
        l.bite.width_m
    );
    assert!(
        hi - lo > 0.5 * l.bite.width_m,
        "a head-sized bite: {lo}..{hi}"
    );
    // As deep as the cut at its middle, a V along the grain in the band hit (the lower quarter),
    // and the bark untouched elsewhere.
    let k = f64::from(c.kerf[0][0]);
    assert!(k > 0.05);
    assert!((depth_at(1, &c, &l, 0, 0.25, 0.3) - k).abs() < 1e-9);
    assert!(depth_at(1, &c, &l, 0, 0.25, 0.7).abs() < 1e-12);
    assert!(depth_at(1, &c, &l, 0, 0.75, 0.3).abs() < 1e-12);
    assert!(depth_at(1, &c, &l, 0, 0.32, 0.3) < k);
    // Its faces are fresh wood sloping in; every point inside the cell.
    assert!(f.iter().all(|f| {
        f.pts
            .iter()
            .all(|p| p.iter().all(|x| (-1e-9..=1.0 + 1e-9).contains(x)))
    }));
}

#[test]
fn bites_at_nearby_spots_merge_into_one() {
    let l = look();
    let mut c = Cut::default();
    // Two hits in the same band, 5 cm apart: the second's head comes in beside the first.
    let mut b = Cut::default();
    b.swing(axe(), 1, 0, 0, 1.0);
    let a = b.bite(1, 0, 0, 0.4, &l.bite);
    b.swing(axe(), 1, 0, 0, 1.0);
    let a2 = b.bite(1, 0, 0, 0.45, &l.bite);
    assert!((a - 0.4).abs() < 1e-9);
    assert!(
        (a2 - (0.4 + l.bite.step * l.bite.width_m)).abs() < 1e-6,
        "beside the first: {a2}"
    );
    // Cut continuously from one mouth to the other: one wider bite, not two.
    let w = l.bite.width_m;
    let (lo, hi) = (0.4 - 0.5 * w + l.bite.ends_m, a2 + 0.5 * w - l.bite.ends_m);
    for i in 0..=40 {
        let u = lo + (hi - lo) * f64::from(i) / 40.0;
        assert!(depth_at(1, &b, &l, 0, 0.25, u) > 0.0, "a gap at {u}");
    }
    let f = carve_bites(1, &b, &l, None);
    assert_closed(&f);
    let (flo, fhi) = cut_span(&f, 0);
    assert!(fhi - flo > w && fhi - flo <= w * (1.0 + l.bite.step) + 1e-9);
    // Hits in the other band at the same spot are the classic notch's second cut: same place.
    let at = hit(&mut c, 1, 0, 0, 0.5);
    let at2 = hit(&mut c, 1, 0, 0, 0.5);
    assert_eq!(
        c.bites[0].all()[1].band,
        1,
        "the head came in beside the kerf"
    );
    assert!((at - 0.5).abs() < 1e-9 && (at2 - 0.5).abs() < 1e-9);
}

#[test]
fn chopping_one_spot_widens_the_bites_into_the_notch_and_still_fells_at_the_rule() {
    // BUGS.md #75's one aim, 11 swings (ADR 0067 §2): the cut fells at the 11th as before, and
    // its bites have spread across the side into the notch.
    let l = look();
    let wood = chop::wood("dwell:log").unwrap();
    let mut c = Cut::default();
    let mut fell_at = None;
    for n in 1..=20 {
        hit(&mut c, 1, 0, 0, 0.5);
        if c.cross(1) >= wood.fell_fraction {
            fell_at = Some(n);
            break;
        }
    }
    assert_eq!(fell_at, Some(11), "fells at the rule's 11 swings");
    let f = carve_bites(1, &c, &l, None);
    let v = assert_closed(&f);
    assert!(v < 0.9 && v > 0.5, "a notch out: {v}");
    let (lo, hi) = cut_span(&f, 0);
    assert!(
        hi - lo > 3.0 * l.bite.width_m,
        "widened across the side: {lo}..{hi}"
    );
    assert!(lo > 0.0 && hi < 1.0, "inside the side: {lo}..{hi}");
    // The notch's V at its middle is the cut's notch (cells: ×1 across x).
    let n = f64::from(c.notch[0]);
    assert!(n > 0.3);
    assert!((depth_at(1, &c, &l, 0, 0.5, 0.5) - n).abs() < 1e-9);
}

#[test]
fn bitten_cells_stay_closed_on_every_grain_and_in_the_octagon() {
    let l = look();
    let c = 0.2929;
    let oct = vec![
        [c, 0.0],
        [1.0 - c, 0.0],
        [1.0, c],
        [1.0, 1.0 - c],
        [1.0 - c, 1.0],
        [c, 1.0],
        [0.0, 1.0 - c],
        [0.0, c],
    ];
    let inside = |p: &[f64; 3]| {
        let (a, b) = (p[0], p[2]);
        a + b >= c - 1e-6
            && (1.0 - a) + b >= c - 1e-6
            && a + (1.0 - b) >= c - 1e-6
            && (1.0 - a) + (1.0 - b) >= c - 1e-6
    };
    for grain in 0..3 {
        for sides in [vec![0], vec![1, 3], vec![0, 1], vec![2, 3, 0]] {
            let mut cut = Cut::default();
            for (i, &s) in sides.iter().enumerate() {
                for k in 0..(4 + 3 * i) {
                    hit(&mut cut, grain, s, k % 2, 0.1 + 0.2 * i as f64);
                }
            }
            let f = carve_bites(grain, &cut, &l, None);
            let v = assert_closed(&f);
            assert!(v > 0.2 && v < 1.0, "grain {grain} {sides:?}: {v}");
            if grain == 1 {
                let f = carve_bites(1, &cut, &l, Some(&oct));
                let (s, v) = closure(&f);
                assert!(s.iter().all(|x| x.abs() < 1e-6), "octagon open: {s:?}");
                assert!(v > 0.2 && v < 1.0 - 2.0 * c * c);
                assert!(
                    f.iter().all(|f| f.pts.iter().all(inside)),
                    "outside the octagon"
                );
            }
        }
    }
}

#[test]
fn a_bitten_cells_triangles_keep_to_a_budget() {
    // Every side hacked to its most bites in both bands, and a fresh hit: the worst a cell draws.
    let l = look();
    let mut cut = Cut::default();
    for s in 0..SIDES {
        for k in 0..40 {
            hit(&mut cut, 1, s, k % 2, (k % 7) as f64 / 6.0);
        }
    }
    let f = carve_bites(1, &cut, &l, None);
    let tris: usize = f.iter().map(|f| f.pts.len() - 2).sum();
    assert!(tris <= 2048, "{tris} triangles");
    // One bite is a few dozen.
    let mut one = Cut::default();
    hit(&mut one, 1, 0, 0, 0.5);
    let tris: usize = carve_bites(1, &one, &l, None)
        .iter()
        .map(|f| f.pts.len() - 2)
        .sum();
    assert!(tris <= 64, "{tris} triangles for one bite");
}

#[test]
fn a_cut_without_bites_is_carved_as_before_and_mirrored_bites_follow_their_side() {
    // An older save's or server's cut has no bites: the mesher keeps today's look for it.
    let mut c = Cut::default();
    c.notch[0] = 0.2;
    c.kerf[0][1] = 0.05;
    assert!(!c.has_bites());
    // Grain x: sides 0 and 1 run along y and swap on face B, their bites with them.
    let mut b = Cut::default();
    hit(&mut b, 0, 0, 0, 0.3);
    let m = mirror_cut(0, &b);
    assert_eq!(m.bites[1], b.bites[0]);
    assert_eq!(m.bites[0].n, 0);
    assert_eq!(carve_in(0, &m, &look(), None).len() > 6, true);
}
