// face-a-only: the geometry here is face-local (cells, +y up) and has no face; the chunk test in
// mesher_tests.rs meshes it on both faces.
use super::*;

const S: f64 = 0.28;

fn total(ps: &[Piece]) -> f64 {
    ps.iter().map(|p| area(&p.pts)).sum()
}

#[test]
fn a_cell_inside_the_crown_is_a_full_cube_and_draws_nothing() {
    let ps = pieces([false; 6], [true; 6], [[false; 6]; 6], S);
    assert!(ps.is_empty(), "{ps:?}");
}

#[test]
fn a_lone_leaf_is_cut_on_every_edge_and_corner() {
    let ps = pieces([true; 6], [false; 6], [[false; 6]; 6], S);
    // 6 faces, 12 edge cuts, 8 corner cuts.
    assert_eq!(ps.len(), 26);
    // Each face keeps a square 1 − 2s across (the edges' cuts reach past the corners').
    for p in ps.iter().filter(|p| p.sides.len() == 1) {
        assert_eq!(p.pts.len(), 4, "{p:?}");
        assert!((area(&p.pts) - (1.0 - 2.0 * S).powi(2)).abs() < 1e-9);
    }
    for p in &ps {
        for q in &p.pts {
            assert!(q.iter().all(|c| c.abs() <= 0.5 + 1e-9), "{q:?}");
        }
    }
}

#[test]
fn only_edges_between_two_exposed_faces_are_cut() {
    // Exposed on top and +x only: one edge cut, both faces lose a strip S wide.
    let mut ex = [false; 6];
    ex[0] = true;
    ex[2] = true;
    let ps = pieces(ex, [false; 6], [[false; 6]; 6], S);
    let top = ps.iter().find(|p| p.sides == vec![2]).expect("top");
    assert!((area(&top.pts) - (1.0 - S)).abs() < 1e-9);
    let cut = ps
        .iter()
        .find(|p| p.sides.len() == 2)
        .expect("the edge's cut");
    assert!((area(&cut.pts) - S * std::f64::consts::SQRT_2).abs() < 1e-9);
    assert_eq!(ps.len(), 3);
}

/// The Leaf Lab's dark gaps: two leaves side by side along x, both open on top; the one at +x is
/// open toward +z too (the one at −x has a leaf there), so its top/+z edge is cut, and the cut's
/// end at their shared face showed into the cells. The −x cell fills exactly that triangle.
#[test]
fn where_a_neighbours_cut_ends_the_shared_face_is_filled_exactly() {
    let mut ex_a = [false; 6];
    ex_a[2] = true;
    let mut ex_b = [false; 6];
    ex_b[2] = true;
    ex_b[4] = true;
    let mut leafy = [false; 6];
    leafy[0] = true;
    let mut nb = [[false; 6]; 6];
    nb[0] = ex_b;
    let ps = pieces(ex_a, leafy, nb, S);
    let fill: Vec<&Piece> = ps.iter().filter(|p| p.normal == [1.0, 0.0, 0.0]).collect();
    assert_eq!(fill.len(), 1, "{ps:?}");
    assert!((area(&fill[0].pts) - S * S / 2.0).abs() < 1e-9);
    // Lit from the faces the cut looks out of, not from the leaf it faces.
    assert_eq!(fill[0].sides, vec![2, 4]);
    // Cut alike on both sides: nothing to fill.
    let ps = pieces(ex_b, leafy, nb, S);
    assert!(ps.iter().all(|p| p.normal != [1.0, 0.0, 0.0]), "{ps:?}");
}

#[test]
fn no_chamfer_draws_plain_faces() {
    let ps = pieces([true; 6], [false; 6], [[false; 6]; 6], 0.0);
    assert_eq!(ps.len(), 6);
    assert!((total(&ps) - 6.0).abs() < 1e-9);
}

/// A fixture crown: 7 × 8 × 7 leaf cells (7 m × 4 m × 7 m), x from `x0`.
fn crown(c: [i32; 3], x0: i32) -> bool {
    (x0..x0 + 7).contains(&c[0]) && (12..20).contains(&c[1]) && (13..20).contains(&c[2])
}

#[test]
fn the_canopy_leans_out_of_the_crown() {
    let crowns = Crowns::from_fn(|x, y, z| crown([x, y, z], 13)).expect("leaves");
    let top = crowns.outward([16.5, 20.0, 16.5]).expect("decided");
    assert!(top[1] > 0.99, "{top:?}");
    let corner = crowns.outward([20.0, 20.0, 20.0]).expect("decided");
    assert!(
        corner[0] > 0.3 && corner[1] > 0.3 && corner[2] > 0.3,
        "{corner:?}"
    );
    let side = crowns.outward([13.0, 16.0, 16.5]).expect("decided");
    assert!(side[0] < -0.99, "{side:?}");
    // Blended 0.75: a side face leans three parts toward the crown's direction to one its own.
    let n = canopy_normal([1.0, 0.0, 0.0], Some([0.0, 1.0, 0.0]), 0.75);
    assert!((n[1] / n[0] - 3.0).abs() < 1e-9);
    assert!(crowns.outward([2.0, 2.0, 2.0]).is_none());
}

#[test]
fn the_crown_agrees_with_itself_across_a_chunk_border() {
    // One crown across the border at x = 32 seen by the chunk on each side: at a point on the
    // border both reach the same distance each way, so both see the same direction.
    let a = Crowns::from_fn(|x, y, z| crown([x, y, z], 28)).expect("leaves");
    let b = Crowns::from_fn(|x, y, z| crown([x + N, y, z], 28)).expect("leaves");
    let pa = a.outward([f64::from(N), 20.0, 18.0]).expect("a");
    let pb = b.outward([0.0, 20.0, 18.0]).expect("b");
    for i in 0..3 {
        assert!((pa[i] - pb[i]).abs() < 1e-9, "{pa:?} {pb:?}");
    }
}

#[test]
fn cards_stand_off_exposed_faces_by_the_data_and_none_inside() {
    let look = LEAVES.cards;
    let up = |_: V| Some([0.0, 1.0, 0.0]);
    assert!(cards([false; 6], [3, 4, 5], &look, up, |_| false).is_empty());
    // Over many lone cells the mean count per cell is the data's: tops 2.6×, sides 2.2× by their
    // area (½), bottoms 1.2×.
    let n = 2000;
    let total: usize = (0..n)
        .map(|i| cards([true; 6], [i, 7, -i], &look, up, |_| false).len())
        .sum();
    let want = look.density * (2.6 + 1.2 + 4.0 * 2.2 * CELL_H);
    let mean = total as f64 / n as f64;
    assert!((mean - want).abs() < 0.1 * want, "{mean} vs {want}");
    // The same cell: the same cards, whichever chunk meshes it.
    let a = cards([true; 6], [11, 2, 3], &look, up, |_| false);
    let b = cards([true; 6], [11, 2, 3], &look, up, |_| false);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    for c in &a {
        let w = (0..3)
            .map(|i| (c.pts[1][i] - c.pts[0][i]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(w >= look.size_m * 0.74 && w <= look.size_m * 1.26, "{w}");
    }
}

/// WORK_QUEUE item 48 (owner, 2026-11-12: "the inside leave edges make a flat side against the
/// logs"): a lone leaf with a log on −x draws its −x face flat and whole but for the four edges
/// along x, whose cuts end square against it; no cut leans toward −x; its other faces are cut as
/// before.
#[test]
fn a_flush_face_is_drawn_flat_and_the_cuts_along_it_end_square() {
    let mut ex = [true; 6];
    ex[1] = false;
    let mut flush = [false; 6];
    flush[1] = true;
    let ps = flush_pieces(ex, flush, [false; 6], [[false; 6]; 6], S);
    let inner: Vec<&Piece> = ps.iter().filter(|p| p.normal == [-1.0, 0.0, 0.0]).collect();
    assert_eq!(inner.len(), 1, "{ps:?}");
    // The square less the four edges' (±y, ±z) triangular ends.
    assert!(
        (area(&inner[0].pts) - (1.0 - 2.0 * S * S)).abs() < 1e-9,
        "{inner:?}"
    );
    assert!(inner[0].pts.iter().all(|q| (q[0] + 0.5).abs() < 1e-9));
    for p in &ps {
        assert!(p.normal[0] > -1e-9 || p.normal == [-1.0, 0.0, 0.0], "{p:?}");
        assert!(!p.sides.contains(&1) || p.sides == vec![1], "{p:?}");
    }
    // The ±y/±z edges' cuts reach the flush face: each has an edge lying on x = −0.5.
    let ends = ps
        .iter()
        .filter(|p| p.sides.len() == 2)
        .filter(|p| p.pts.iter().filter(|q| (q[0] + 0.5).abs() < 1e-9).count() == 2)
        .count();
    assert_eq!(ends, 4, "{ps:?}");
    // The other faces: +x keeps its 1 − 2s square, 5 edges and 4 corners cut there less.
    let out = ps.iter().find(|p| p.normal == [1.0, 0.0, 0.0]).expect("+x");
    assert!((area(&out.pts) - (1.0 - 2.0 * S).powi(2)).abs() < 1e-9);
    // 6 faces, 12 − 4 edge cuts, 8 − 4 corner cuts.
    assert_eq!(ps.len(), 6 + 8 + 4);
}

/// Without a flush face, `flush_pieces` is `pieces`.
#[test]
fn flush_pieces_without_a_flush_face_are_the_plain_ones() {
    let mut ex = [true; 6];
    ex[3] = false;
    let a = pieces(ex, [false; 6], [[false; 6]; 6], S);
    let b = flush_pieces(ex, [false; 6], [false; 6], [[false; 6]; 6], S);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}
