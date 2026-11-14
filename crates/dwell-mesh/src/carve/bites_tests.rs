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

/// The cut's carve at its own progress (`Cut::progress`, the log's fell or buck fraction).
fn carve(grain: usize, c: &Cut, l: &CutLook, outline: Option<&[[f64; 2]]>) -> Vec<Face> {
    let wood = chop::wood("dwell:log").unwrap();
    carve_bites(grain, c, l, outline, c.progress(wood, grain))
}

/// Side `s`'s depth (cells) at the cut's own progress.
fn depth(grain: usize, c: &Cut, l: &CutLook, s: usize, t: f64, u: f64) -> f64 {
    let wood = chop::wood("dwell:log").unwrap();
    depth_at(grain, c, l, c.progress(wood, grain), s, t, u)
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
fn one_hit_bites_a_v_centred_where_it_struck_sized_by_the_bite_rule() {
    // Red before item 26: the kerf was a slit across the whole side (z 0 to 1). Since item 18's
    // notch rule the first of the stone axe's 11 swings carves 1/11 of half the cell.
    let l = look();
    let mut c = Cut::default();
    let at = hit(&mut c, 1, 0, 0, 0.3);
    assert!((at - 0.3).abs() < 1e-9, "the bite is where it struck: {at}");
    let f = carve(1, &c, &l, None);
    let v = assert_closed(&f);
    assert!(
        (1.0 - v - 0.5 / 11.0).abs() < 1e-3,
        "the rule's bite out: {v}"
    );
    // Its apex at the row's middle where it struck, its mouth the notch's band (¼ m of the ½ m
    // row), as wide across as it is deep (`widthPerDepth` 1), and the bark untouched elsewhere.
    let d = depth(1, &c, &l, 0, 0.5, 0.3);
    assert!(d > 0.2 && d < 0.5, "{d}");
    let (lo, hi) = cut_span(&f, 0);
    assert!(
        ((lo + hi) * 0.5 - 0.3).abs() < 0.02 && (hi - lo - (d - 2.0 * l.bite.ends_m)).abs() < 0.02,
        "centred on the hit, as wide as deep ({d}): {lo}..{hi}"
    );
    assert!(depth(1, &c, &l, 0, 0.5, 0.7).abs() < 1e-12);
    assert!(depth(1, &c, &l, 0, 0.75, 0.3).abs() < 1e-12);
    assert!(
        depth(1, &c, &l, 0, 0.24, 0.3).abs() < 1e-12,
        "inside the notch's band"
    );
    assert!(
        (depth(1, &c, &l, 0, 0.375, 0.3) - 0.5 * d).abs() < 1e-6,
        "a V"
    );
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
        assert!(depth(1, &b, &l, 0, 0.375, u) > 0.0, "a gap at {u}");
    }
    let f = carve(1, &b, &l, None);
    assert_closed(&f);
    let (flo, fhi) = cut_span(&f, 0);
    let d = depth(1, &b, &l, 0, 0.5, 0.45);
    assert!(fhi - flo > w && fhi - flo <= d.max(w) + w * l.bite.step + 1e-6);
    // A hit in the other band at the same spot (the notch's second cut) lands beside it too:
    // across the side, not down it (round 2).
    let at = hit(&mut c, 1, 0, 0, 0.5);
    let at2 = hit(&mut c, 1, 0, 0, 0.5);
    assert_eq!(
        c.bites[0].all()[1].band,
        1,
        "the head came in beside the kerf"
    );
    let step = l.bite.step * l.bite.width_m;
    assert!(
        (at - 0.5).abs() < 1e-9 && (at2 - (0.5 + step)).abs() < 1e-6,
        "{at} {at2}"
    );
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
    let f = carve(1, &c, &l, None);
    let v = assert_closed(&f);
    assert!(v < 0.9 && v > 0.5, "a notch out: {v}");
    let (lo, hi) = cut_span(&f, 0);
    // Round 2 (the in-game check: "a slot the cell's full height, still ~0.14 m wide"): across
    // the log, most of its width.
    assert!(hi - lo > 0.85, "across the side: {lo}..{hi}");
    assert!(lo >= 0.0 && hi <= 1.0, "inside the side: {lo}..{hi}");
    // Item 18's notch rule: grown to the gash's V, its mouth the row's whole height and its apex
    // at the row's middle near the far side (the waist left).
    let (tlo, thi) = f
        .iter()
        .filter(|f| f.part == Part::Cut(0))
        .flat_map(|f| f.pts.iter())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[1]), hi.max(p[1]))
        });
    assert!(
        tlo < 0.05 && thi > 0.95,
        "the mouth spans the row: {tlo}..{thi}"
    );
    let apex = depth(1, &c, &l, 0, 0.5, 0.5);
    assert!(apex > 0.9 && apex < 1.0, "{apex}");
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
            let f = carve(grain, &cut, &l, None);
            let v = assert_closed(&f);
            assert!(v > 0.2 && v < 1.0, "grain {grain} {sides:?}: {v}");
            if grain == 1 {
                let f = carve(1, &cut, &l, Some(&oct));
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

/// WORK_QUEUE item 26 round 2: eleven hits at one spot of an octagonal trunk (a full log, and a
/// thinner one half its cell wide, as the server spreads them: `bite_in` over the log's span) cut a
/// notch across the log's whole width, over its chamfers (red before: only the flat face was
/// carved, 0.29..0.71 of a full log, and a thinner trunk's sides not at all), closed, inside the
/// octagon.
#[test]
fn a_notch_spans_an_octagonal_log_over_its_chamfers() {
    let l = look();
    let c = 0.2929;
    for k in [1.0, 0.5] {
        let oct: Vec<[f64; 2]> = [
            [c, 0.0],
            [1.0 - c, 0.0],
            [1.0, c],
            [1.0, 1.0 - c],
            [1.0 - c, 1.0],
            [c, 1.0],
            [0.0, 1.0 - c],
            [0.0, c],
        ]
        .iter()
        .map(|p| [0.5 + (p[0] - 0.5) * k, 0.5 + (p[1] - 0.5) * k])
        .collect();
        let span = [0.5 - 0.5 * k, 0.5 + 0.5 * k];
        let mut cut = Cut::default();
        for _ in 0..11 {
            let band = cut.open_band(0, 0);
            cut.swing(axe(), 1, 0, band, 1.0);
            cut.bite_in(1, 0, band, 0.5, &l.bite, span);
        }
        let f = carve(1, &cut, &l, Some(&oct));
        let (sum, v) = closure(&f);
        assert!(sum.iter().all(|x| x.abs() < 1e-6), "{k}: open: {sum:?}");
        assert!(v > 0.2 * k * k && v < k * k, "{k}: {v}");
        let inside = |p: &[f64; 3]| {
            let (a, b) = ((p[0] - 0.5) / k + 0.5, (p[2] - 0.5) / k + 0.5);
            [a, b].iter().all(|x| (-1e-6..=1.0 + 1e-6).contains(x))
                && a + b >= c - 1e-6
                && (1.0 - a) + b >= c - 1e-6
                && a + (1.0 - b) >= c - 1e-6
                && (1.0 - a) + (1.0 - b) >= c - 1e-6
        };
        let bad: Vec<_> = f
            .iter()
            .filter(|f| !f.pts.iter().all(inside))
            .map(|f| (f.part, f.pts.clone()))
            .take(3)
            .collect();
        assert!(bad.is_empty(), "{k}: outside the octagon: {bad:?}");
        let (lo, hi) = f
            .iter()
            .filter(|f| f.part == Part::Cut(0))
            .flat_map(|f| f.pts.iter())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[2]), hi.max(p[2]))
            });
        assert!(
            (hi - lo) / k > 0.85,
            "{k}: the notch spans the log ({lo}..{hi} of {span:?})"
        );
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
    let f = carve(1, &cut, &l, None);
    let tris: usize = f.iter().map(|f| f.pts.len() - 2).sum();
    assert!(tris <= 2048, "{tris} triangles");
    // One bite is a few dozen.
    let mut one = Cut::default();
    hit(&mut one, 1, 0, 0, 0.5);
    let tris: usize = carve(1, &one, &l, None)
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
    assert!(carve_in(0, &m, &look(), None).len() > 6);
}

/// The octagon of a full log (cell units across a standing log's x and z).
fn octagon() -> Vec<[f64; 2]> {
    let c = 0.2929;
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

/// The area of a convex outline (cell units).
fn poly_area(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    0.5 * (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
}

/// WORK_QUEUE item 18 (owner, 2026-11-12: "Yes the notch should grow to follow the 50% rule"):
/// chopping one spot of a standing log, as the server lands the swings, carves k × ½ × the log's
/// volume ÷ its swings to fell after k swings (within 8 %; the log's volume: the cell, or its
/// octagon's), half of it at the fell, for the stone axe (11 swings) and the metal one (5), on a
/// square cell and in the octagon, the bites joining into one V across the side (red on main: the
/// notch kept its own sizes, a V in a ¼ m band: 0.0013 of the rule's 0.0455 at the first swing).
#[test]
fn the_notch_carves_half_the_log_over_its_swings_k_bites_after_k() {
    let l = look();
    let wood = chop::wood("dwell:log").unwrap();
    for name in ["dwell:stone_axe", "dwell:metal_axe"] {
        let tool = chop::tool(name).unwrap();
        let n = chop::swings_to_fell(tool, wood);
        for outline in [None, Some(octagon())] {
            let wood_area = outline.as_deref().map_or(1.0, poly_area);
            let mut c = Cut::default();
            for k in 1..=n {
                let band = c.open_band(0, 0);
                c.swing(tool, 1, 0, band, tool.across);
                c.bite(1, 0, band, 0.5, &l.bite);
                let f = carve(1, &c, &l, outline.as_deref());
                let v = assert_closed(&f);
                let carved = wood_area - v;
                let want = f64::from(k) / f64::from(n) * 0.5 * wood_area;
                // In the octagon the fell's half is more than a V inside it holds (its chamfers
                // take the mouth's corners): the fell's carve is the largest V, never drawn (the
                // cell splits into the gash, half the cell).
                let fell_in_octagon = k == n && outline.is_some();
                if fell_in_octagon {
                    assert!(
                        carved > 0.8 * want,
                        "{name} the octagon's fell: {carved:.4}"
                    );
                    continue;
                }
                assert!(
                    (carved / want - 1.0).abs() < 0.08,
                    "{name} {:?} swing {k}/{n}: carved {carved:.4}, the rule {want:.4}",
                    outline.is_some()
                );
            }
            assert!(c.cross(1) >= wood.fell_fraction, "{name} fells at {n}");
        }
    }
}
