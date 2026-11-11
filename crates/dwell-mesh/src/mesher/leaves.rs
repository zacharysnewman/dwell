//! Leaves as a canopy (docs/TREE_GROWTH.md §8, the Leaf Lab's option 5; `shared/look/leaves.json`):
//! each leaf cell is drawn whole, a cube cut back by `chamfer` on the edges and corners where both
//! (all three) faces meeting there are exposed (no leaf beside them), and its normals lean
//! `canopy_blend` of the way toward the crown's outward direction, so a crown shades as one
//! rounded mass rather than as cubes.
//!
//! - **Exposed**: the neighbour is not a leaf and does not cover the face (a full opaque cube).
//! - **No gaps where a chamfer ends**: where a neighbouring leaf is cut back and this cell is not
//!   (or less), the part of their shared face the neighbour's cut uncovers is drawn, by clipping
//!   this cell's face by each of the neighbour's cut planes in turn. Both cells clip the one face
//!   by the same planes, so their pieces meet exactly.
//! - **The crown's outward direction** at a point: away from the centroid of the leaf cells in a
//!   box around it (3 m each way, cut to the same reach on both sides where the chunk's padding
//!   ends, so two chunks agree near their border). It stands in for an ellipsoid fitted to the
//!   tree's crown: the tree record is not at hand in the mesher (a recorded deviation).
//!
//! Face-local throughout (+y up): face B's mesh is mirrored afterwards with the rest.

use super::{Builder, Cells, FACES, Kind, PAD, surface};
use crate::sky::{AO_OPEN, vertex_light};
use dwell_look::leaves::LEAVES;
use dwell_look::normal_tint;
use dwell_world::{CELL_H, normal_to_m};

const N: i32 = dwell_protocol::CHUNK_SIZE;
const EPS: f64 = 1e-9;
/// The crown's reach around a point, in metres, each way.
const REACH_M: f64 = 3.0;

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(i: usize) -> V {
    let (a, s) = FACES[i];
    let mut v = [0.0; 3];
    v[a] = f64::from(s);
    v
}

fn normalise(v: V) -> Option<V> {
    let l = dot(v, v).sqrt();
    (l > 1e-12).then(|| v.map(|c| c / l))
}

/// A cut plane, `n · p <= d` kept (cell-centred unit cube, cells), and the faces whose edge or
/// corner it cuts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Plane {
    pub n: V,
    pub d: f64,
    pub sides: [Option<usize>; 3],
}

/// The cut planes of a cell exposed on the faces `ex` (index as [`FACES`]), cut back by `s` cells.
pub(crate) fn planes(ex: [bool; 6], s: f64) -> Vec<Plane> {
    let mut out = Vec::new();
    if s <= 0.005 {
        return out;
    }
    for i in 0..6 {
        for j in i + 1..6 {
            if FACES[i].0 == FACES[j].0 || !ex[i] || !ex[j] {
                continue;
            }
            let n = normalise(std::array::from_fn(|a| unit(i)[a] + unit(j)[a])).unwrap_or_default();
            out.push(Plane {
                n,
                d: (1.0 - s) / std::f64::consts::SQRT_2,
                sides: [Some(i), Some(j), None],
            });
        }
    }
    for sx in [0, 1] {
        for sy in [2, 3] {
            for sz in [4, 5] {
                if !ex[sx] || !ex[sy] || !ex[sz] {
                    continue;
                }
                let n = normalise(std::array::from_fn(|a| unit(sx)[a] + unit(sy)[a] + unit(sz)[a]))
                    .unwrap_or_default();
                out.push(Plane {
                    n,
                    d: (1.5 - 2.0 * s) / 3f64.sqrt(),
                    sides: [Some(sx), Some(sy), Some(sz)],
                });
            }
        }
    }
    out
}

/// Clips a convex polygon to `n · p <= d`.
pub(crate) fn clip(poly: &[V], n: V, d: f64) -> Vec<V> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    for (k, &a) in poly.iter().enumerate() {
        let b = poly[(k + 1) % poly.len()];
        let (da, db) = (dot(n, a) - d, dot(n, b) - d);
        if da <= EPS {
            out.push(a);
        }
        if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
            let t = da / (da - db);
            out.push(std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t));
        }
    }
    if out.len() < 3 { Vec::new() } else { out }
}

fn area(poly: &[V]) -> f64 {
    let mut s = [0.0; 3];
    for k in 1..poly.len().saturating_sub(1) {
        let (a, b, c) = (poly[0], poly[k], poly[k + 1]);
        let u: V = std::array::from_fn(|i| b[i] - a[i]);
        let v: V = std::array::from_fn(|i| c[i] - a[i]);
        s[0] += u[1] * v[2] - u[2] * v[1];
        s[1] += u[2] * v[0] - u[0] * v[2];
        s[2] += u[0] * v[1] - u[1] * v[0];
    }
    dot(s, s).sqrt() / 2.0
}

/// The unit cube's face `i` (cell-centred), counter-clockwise seen from outside.
fn square(i: usize) -> Vec<V> {
    let (a, s) = FACES[i];
    let (u, v) = ((a + 1) % 3, (a + 2) % 3);
    let corners = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
    let mut pts: Vec<V> = corners
        .iter()
        .map(|&(cu, cv)| {
            let mut p = [0.0; 3];
            p[a] = 0.5 * f64::from(s);
            p[u] = cu;
            p[v] = cv;
            p
        })
        .collect();
    if s < 0 {
        pts.reverse();
    }
    pts
}

/// The cap of `planes[k]`: its plane inside the cube and the other planes, counter-clockwise
/// seen from outside.
fn cap(planes: &[Plane], k: usize) -> Vec<V> {
    let p = planes[k];
    let c = p.n.map(|x| x * p.d);
    let t1 = normalise(if p.n[1].abs() < 0.9 {
        [p.n[2], 0.0, -p.n[0]]
    } else {
        [0.0, -p.n[2], p.n[1]]
    })
    .unwrap_or([1.0, 0.0, 0.0]);
    let t2 = [
        p.n[1] * t1[2] - p.n[2] * t1[1],
        p.n[2] * t1[0] - p.n[0] * t1[2],
        p.n[0] * t1[1] - p.n[1] * t1[0],
    ];
    // t1 × t2 = n: counter-clockwise seen from outside.
    let mut poly: Vec<V> = [(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]
        .iter()
        .map(|&(a, b)| std::array::from_fn(|i| c[i] + t1[i] * a + t2[i] * b))
        .collect();
    for i in 0..6 {
        poly = clip(&poly, unit(i), 0.5);
    }
    for (j, q) in planes.iter().enumerate() {
        if j != k {
            poly = clip(&poly, q.n, q.d);
        }
    }
    poly
}

/// One polygon a leaf cell draws: its points (cell-centred), its geometric normal (cells) and the
/// faces whose light it takes.
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub pts: Vec<V>,
    pub normal: V,
    pub sides: Vec<usize>,
}

/// The polygons of a leaf cell exposed on `ex`, with `leafy[i]` whether the neighbour on face `i`
/// is a leaf and `nb_ex[i]` that neighbour's exposure, cut back by `s` (see the module docs).
pub(crate) fn pieces(ex: [bool; 6], leafy: [bool; 6], nb_ex: [[bool; 6]; 6], s: f64) -> Vec<Piece> {
    let mine = planes(ex, s);
    let mut out = Vec::new();
    for i in 0..6 {
        let mut face = square(i);
        for p in &mine {
            face = clip(&face, p.n, p.d);
        }
        if ex[i] {
            if area(&face) > EPS {
                out.push(Piece {
                    pts: face,
                    normal: unit(i),
                    sides: vec![i],
                });
            }
            continue;
        }
        if !leafy[i] {
            continue;
        }
        // Where the neighbour's cuts uncover this cell's face, draw it, one piece per cut.
        let e = unit(i);
        let theirs = planes(nb_ex[i], s);
        let mut rest = face;
        for p in &theirs {
            if rest.is_empty() {
                break;
            }
            let d = p.d + dot(p.n, e);
            let piece = clip(&rest, p.n.map(|c| -c), -d);
            if area(&piece) > EPS {
                let sides = p.sides.iter().flatten().copied().filter(|&f| f != i).collect();
                out.push(Piece {
                    pts: piece,
                    normal: e,
                    sides,
                });
            }
            rest = clip(&rest, p.n, d);
        }
    }
    for k in 0..mine.len() {
        let pts = cap(&mine, k);
        if area(&pts) > EPS {
            out.push(Piece {
                pts,
                normal: mine[k].n,
                sides: mine[k].sides.iter().flatten().copied().collect(),
            });
        }
    }
    out
}

/// Leaf cells' prefix sums over the padded chunk: the count and the centres' sums, for the crown's
/// outward direction at any point.
pub(crate) struct Crowns {
    /// (PAD + 1)³ entries of [count, Σx, Σy, Σz] (cells, chunk-local centres).
    sums: Vec<[f64; 4]>,
}

const P1: usize = PAD + 1;

impl Crowns {
    /// None when the padded chunk holds no leaves.
    pub(crate) fn new(cells: &Cells<'_>) -> Option<Self> {
        Self::from_fn(|x, y, z| cells.kind(x, y, z).style.leafy)
    }

    pub(crate) fn from_fn(leafy: impl Fn(i32, i32, i32) -> bool) -> Option<Self> {
        let mut sums = vec![[0.0; 4]; P1 * P1 * P1];
        let at = |x: usize, y: usize, z: usize| x + P1 * (z + P1 * y);
        let mut any = false;
        for y in 0..PAD {
            for z in 0..PAD {
                for x in 0..PAD {
                    let (cx, cy, cz) = (x as i32 - 1, y as i32 - 1, z as i32 - 1);
                    let v = if leafy(cx, cy, cz) {
                        any = true;
                        [
                            1.0,
                            f64::from(cx) + 0.5,
                            f64::from(cy) + 0.5,
                            f64::from(cz) + 0.5,
                        ]
                    } else {
                        [0.0; 4]
                    };
                    let mut s = [0.0; 4];
                    for c in 0..4 {
                        s[c] = v[c] + sums[at(x, y + 1, z + 1)][c] + sums[at(x + 1, y, z + 1)][c]
                            + sums[at(x + 1, y + 1, z)][c]
                            - sums[at(x, y, z + 1)][c]
                            - sums[at(x, y + 1, z)][c]
                            - sums[at(x + 1, y, z)][c]
                            + sums[at(x, y, z)][c];
                    }
                    sums[at(x + 1, y + 1, z + 1)] = s;
                }
            }
        }
        any.then_some(Self { sums })
    }

    /// Sums over the cells `lo..=hi` (chunk-local, each in −1..=N).
    fn box_sum(&self, lo: [i32; 3], hi: [i32; 3]) -> [f64; 4] {
        let at = |x: i32, y: i32, z: i32| (x + P1 as i32 * (z + P1 as i32 * y)) as usize;
        let (a, b) = (lo.map(|c| c + 1), hi.map(|c| c + 2));
        let s = |x: i32, y: i32, z: i32| self.sums[at(x, y, z)];
        let mut out = [0.0; 4];
        for (c, o) in out.iter_mut().enumerate() {
            *o = s(b[0], b[1], b[2])[c] - s(a[0], b[1], b[2])[c] - s(b[0], a[1], b[2])[c]
                - s(b[0], b[1], a[2])[c]
                + s(a[0], a[1], b[2])[c]
                + s(a[0], b[1], a[2])[c]
                + s(b[0], a[1], a[2])[c]
                - s(a[0], a[1], a[2])[c];
        }
        out
    }

    /// The crown's outward direction (a unit vector in metres) at chunk-local point `p` (cells),
    /// or none where it is undecided (no leaves around, or `p` at their centre).
    pub(crate) fn outward(&self, p: V) -> Option<V> {
        let reach = [REACH_M, REACH_M / CELL_H, REACH_M];
        let mut lo = [0; 3];
        let mut hi = [0; 3];
        for a in 0..3 {
            // The same reach on both sides, so two chunks agree near their shared border.
            let r = reach[a].min(p[a] + 1.0).min(f64::from(N + 1) - p[a]).max(0.0);
            lo[a] = ((p[a] - r - 0.5).ceil() as i32).clamp(-1, N);
            hi[a] = ((p[a] + r - 0.5).floor() as i32).clamp(-1, N);
            if hi[a] < lo[a] {
                return None;
            }
        }
        let s = self.box_sum(lo, hi);
        if s[0] < 0.5 {
            return None;
        }
        let d = [
            p[0] - s[1] / s[0],
            (p[1] - s[2] / s[0]) * CELL_H,
            p[2] - s[3] / s[0],
        ];
        normalise(d)
    }
}

/// A leaf face's normal (metres): its own, leaned `blend` of the way toward `outward`.
pub(crate) fn canopy_normal(face: V, outward: Option<V>, blend: f64) -> V {
    let Some(g) = outward else {
        return face;
    };
    normalise(std::array::from_fn(|i| face[i] * (1.0 - blend) + g[i] * blend)).unwrap_or(face)
}

/// Draws the leaf cell `cell` (chunk-local) of kind `k` into `b`.
pub(super) fn leaf_voxel(b: &mut Builder, cells: &Cells<'_>, crowns: &Crowns, k: &Kind, cell: [i32; 3]) {
    let nb = |i: usize, c: [i32; 3]| {
        let (a, s) = FACES[i];
        let mut o = c;
        o[a] += s;
        o
    };
    let inside = |c: [i32; 3]| c.iter().all(|&v| (-1..=N).contains(&v));
    let leafy_at = |c: [i32; 3]| inside(c) && cells.kind(c[0], c[1], c[2]).style.leafy;
    let exposed_at = |c: [i32; 3]| -> [bool; 6] {
        std::array::from_fn(|i| {
            let o = nb(i, c);
            if !inside(o) {
                // Past the padding: unknown, taken as leaf so no cut is guessed.
                return false;
            }
            let kn = cells.kind(o[0], o[1], o[2]);
            !kn.style.leafy
                && !(kn.occludes && kn.shape == super::Shape::Full && !kn.style.octagon)
        })
    };
    let ex = exposed_at(cell);
    let leafy: [bool; 6] = std::array::from_fn(|i| leafy_at(nb(i, cell)));
    let nb_ex: [[bool; 6]; 6] =
        std::array::from_fn(|i| if leafy[i] { exposed_at(nb(i, cell)) } else { [false; 6] });
    let list = pieces(ex, leafy, nb_ex, LEAVES.chamfer);
    for piece in list {
        emit(b, cells, crowns, k, cell, &piece);
    }
}

fn emit(b: &mut Builder, cells: &Cells<'_>, crowns: &Crowns, k: &Kind, cell: [i32; 3], piece: &Piece) {
    let [x, y, z] = cell;
    let n_m = normal_to_m(piece.normal);
    // The tile and its uv projection: the dominant axis of the face.
    let dom = (0..3)
        .max_by(|&i, &j| n_m[i].abs().total_cmp(&n_m[j].abs()))
        .unwrap_or(1);
    let sign = if n_m[dom] >= 0.0 { 1 } else { -1 };
    let (color, tile) = surface(&k.style, dom, sign);
    let base = b.vertex_count();
    let sides: Vec<usize> = if piece.sides.is_empty() {
        vec![super::face_of(dom, sign)]
    } else {
        piece.sides.clone()
    };
    for &q in &piece.pts {
        // Cell-local point in 0..1 and chunk-local point (cells).
        let f: V = q.map(|c| c + 0.5);
        let p: V = [f64::from(x) + f[0], f64::from(y) + f[1], f64::from(z) + f[2]];
        let n = canopy_normal(n_m, crowns.outward(p), LEAVES.canopy_blend);
        let tint = normal_tint(n[0], n[1], n[2]);
        let r = super::channel(color, 16) * tint[0];
        let g = super::channel(color, 8) * tint[1];
        let bl = super::channel(color, 0) * tint[2];
        b.m.positions.extend([p[0] as f32, p[1] as f32, p[2] as f32]);
        b.m.normals.extend([n[0] as f32, n[1] as f32, n[2] as f32]);
        b.m.colors.extend([r as f32, g as f32, bl as f32]);
        let (su, sv) = match dom {
            1 => (p[0], p[2]),
            0 => (p[2], p[1] * CELL_H),
            _ => (p[0], p[1] * CELL_H),
        };
        b.m.uvs.extend([su as f32, sv as f32]);
        b.push_tile(&tile);
        // The light of the cell faces it looks out of, as an octagonal log's chamfer takes it.
        let mut light = 0.0;
        let mut flood = [0.0f32; 2];
        for &side in &sides {
            let (a, s) = FACES[side];
            let mut fq = f;
            fq[a] = if s > 0 { 1.0 } else { 0.0 };
            light += vertex_light(AO_OPEN, cells.sky_rays(cell, fq, unit(side), 40 + side as i64));
            let front = super::neighbour_of(x, y, z, a, s);
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let toward = |c: f64| if c < 0.5 { -1 } else { 1 };
            let (sky, block) = cells
                .flood
                .corner(cells.occ, front, u, toward(f[u]), v, toward(f[v]));
            flood[0] = flood[0].max(f32::from(sky) / crate::flood::QUARTERS);
            flood[1] = flood[1].max(f32::from(block) / crate::flood::QUARTERS);
        }
        b.m.light.push(light / sides.len() as f32);
        b.m.flood.extend(flood);
    }
    for i in 1..piece.pts.len().saturating_sub(1) as u32 {
        b.m.indices.extend([base, base + i, base + i + 1]);
    }
}

#[cfg(test)]
#[path = "leaves_tests.rs"]
mod tests;
