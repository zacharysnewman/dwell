//! A check for z-fighting (BUGS.md #166): two triangles of a mesh that lie in one plane and
//! overlap with area. Test-only.

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The triangles of a mesh's `positions` and `indices`, less those whose first vertex is a card's
/// (`bare` 1; empty: none are).
pub(crate) fn triangles(positions: &[f32], indices: &[u32], bare: &[f32]) -> Vec<[V; 3]> {
    let at = |i: u32| {
        let i = i as usize * 3;
        [
            f64::from(positions[i]),
            f64::from(positions[i + 1]),
            f64::from(positions[i + 2]),
        ]
    };
    indices
        .chunks_exact(3)
        .filter(|t| bare.get(t[0] as usize).is_none_or(|&b| b < 0.5))
        .map(|t| [at(t[0]), at(t[1]), at(t[2])])
        .collect()
}

/// The area two coplanar triangles share (their 2D intersection on the plane's dominant axes).
fn overlap(a: &[V; 3], b: &[V; 3], drop: usize) -> f64 {
    let (u, v) = ((drop + 1) % 3, (drop + 2) % 3);
    let flat = |t: &[V; 3]| {
        let mut p: Vec<[f64; 2]> = t.iter().map(|q| [q[u], q[v]]).collect();
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        if area < 0.0 {
            p.reverse();
        }
        p
    };
    let (mut poly, clipper) = (flat(a), flat(b));
    for k in 0..3 {
        let (c, d) = (clipper[k], clipper[(k + 1) % 3]);
        let side = |p: [f64; 2]| (d[0] - c[0]) * (p[1] - c[1]) - (d[1] - c[1]) * (p[0] - c[0]);
        let mut next = Vec::new();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                next.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                next.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        poly = next;
        if poly.len() < 3 {
            return 0.0;
        }
    }
    let mut s = 0.0;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        s += p[0] * q[1] - q[0] * p[1];
    }
    s.abs() / 2.0
}

/// The first pair of triangles lying in one plane (within `eps`) and overlapping by more than
/// `min_area`, if any.
pub(crate) fn coplanar_overlap(
    tris: &[[V; 3]],
    eps: f64,
    min_area: f64,
) -> Option<([V; 3], [V; 3], f64)> {
    let info: Vec<(V, f64, V, V)> = tris
        .iter()
        .map(|t| {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let l = dot(n, n).sqrt().max(1e-30);
            let n = n.map(|c| c / l);
            let lo = std::array::from_fn(|k| t.iter().map(|q| q[k]).fold(f64::MAX, f64::min));
            let hi = std::array::from_fn(|k| t.iter().map(|q| q[k]).fold(f64::MIN, f64::max));
            (n, dot(n, t[0]), lo, hi)
        })
        .collect();
    for i in 0..tris.len() {
        let (n, d, lo, hi) = info[i];
        for j in i + 1..tris.len() {
            let (m, _, lo2, hi2) = info[j];
            if (0..3).any(|k| lo2[k] > hi[k] + eps || hi2[k] < lo[k] - eps) {
                continue;
            }
            if dot(n, m).abs() < 1.0 - 1e-6 {
                continue;
            }
            if tris[j].iter().any(|q| (dot(n, *q) - d).abs() > eps) {
                continue;
            }
            let drop = (0..3)
                .max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs()))
                .unwrap_or(0);
            let a = overlap(&tris[i], &tris[j], drop);
            if a > min_area {
                return Some((tris[i], tris[j], a));
            }
        }
    }
    None
}
