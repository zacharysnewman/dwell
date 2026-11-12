//! Leaves as a canopy (docs/TREE_GROWTH.md §10, the Leaf Lab's option 5; `shared/look/leaves.json`):
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
//! - **Per-tree tints** (ADR 0079): a leaf cell's colour, and its cards', are tinted by the tree
//!   owning it among the chunk's species trees (`dwell_world::tree_tint`), its seed's `variety`.
//!
//! Face-local throughout (+y up): face B's mesh is mirrored afterwards with the rest.

use super::{Builder, Cells, FACES, Kind, PAD, surface};
use crate::sky::{AO_OPEN, vertex_light};
use dwell_look::leaves::LEAVES;
use dwell_look::normal_tint;
use dwell_world::tree_tint::Tint;
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
    planes_of(ex, s, false)
}

/// [`planes`], or with `upright` the vertical edges' cuts alone (an octagonal column: a log at LOD).
fn planes_of(ex: [bool; 6], s: f64, upright: bool) -> Vec<Plane> {
    let mut out = Vec::new();
    if s <= 0.005 {
        return out;
    }
    for i in 0..6 {
        for j in i + 1..6 {
            if FACES[i].0 == FACES[j].0 || !ex[i] || !ex[j] {
                continue;
            }
            if upright && (FACES[i].0 == 1 || FACES[j].0 == 1) {
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
    if upright {
        return out;
    }
    for sx in [0, 1] {
        for sy in [2, 3] {
            for sz in [4, 5] {
                if !ex[sx] || !ex[sy] || !ex[sz] {
                    continue;
                }
                let n = normalise(std::array::from_fn(|a| {
                    unit(sx)[a] + unit(sy)[a] + unit(sz)[a]
                }))
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
    pieces_of(ex, leafy, nb_ex, s, false)
}

/// The polygons of an octagonal column (a log cell at LOD) exposed on `ex`: its vertical edges cut
/// by `s` where both sides are open, its open faces drawn.
pub(crate) fn column_pieces(ex: [bool; 6], s: f64) -> Vec<Piece> {
    pieces_of(ex, [false; 6], [[false; 6]; 6], s, true)
}

fn pieces_of(
    ex: [bool; 6],
    leafy: [bool; 6],
    nb_ex: [[bool; 6]; 6],
    s: f64,
    upright: bool,
) -> Vec<Piece> {
    let mine = planes_of(ex, s, upright);
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
                let sides = p
                    .sides
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|&f| f != i)
                    .collect();
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
    pub(super) fn new(cells: &Cells<'_>) -> Option<Self> {
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
                        s[c] = v[c]
                            + sums[at(x, y + 1, z + 1)][c]
                            + sums[at(x + 1, y, z + 1)][c]
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
            *o = s(b[0], b[1], b[2])[c]
                - s(a[0], b[1], b[2])[c]
                - s(b[0], a[1], b[2])[c]
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
            let r = reach[a]
                .min(p[a] + 1.0)
                .min(f64::from(N + 1) - p[a])
                .max(0.0);
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
    normalise(std::array::from_fn(|i| {
        face[i] * (1.0 - blend) + g[i] * blend
    }))
    .unwrap_or(face)
}

/// Draws the leaf cell `cell` (chunk-local) of kind `k` into `b`.
pub(super) fn leaf_voxel(
    b: &mut Builder,
    cells: &Cells<'_>,
    crowns: &Crowns,
    k: &Kind,
    cell: [i32; 3],
    with_cards: bool,
) {
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
            let covers = kn.occludes && kn.shape == super::Shape::Full && !kn.style.octagon;
            !kn.style.leafy && !covers
        })
    };
    let ex = exposed_at(cell);
    let leafy: [bool; 6] = std::array::from_fn(|i| leafy_at(nb(i, cell)));
    let nb_ex: [[bool; 6]; 6] = std::array::from_fn(|i| {
        if leafy[i] {
            exposed_at(nb(i, cell))
        } else {
            [false; 6]
        }
    });
    let list = pieces(ex, leafy, nb_ex, LEAVES.chamfer);
    // The tree owning the cell tints it (ADR 0079); a placed leaf keeps its colour.
    let tint = cells.tint(cell);
    for piece in list {
        emit(b, cells, crowns, k, (cell, ex), &piece, tint);
    }
    if with_cards && k.style.render == dwell_blocks::RenderMode::Cutout {
        let world: [i64; 3] =
            std::array::from_fn(|a| i64::from(cells.origin[a]) + i64::from(cell[a]));
        let at = |p: V| {
            crowns.outward(std::array::from_fn(|a| {
                f64::from(cell[a]) + 0.5 + if a == 1 { p[a] / CELL_H } else { p[a] }
            }))
        };
        // A card reaching into a block beside it that is neither leaves nor empty (a trunk's bark)
        // would cut through or lie along that block's faces and z-fight with them: none there.
        let half = [0.5, 0.5 * CELL_H, 0.5];
        let blocked = |p: V| {
            let o: [i32; 3] =
                std::array::from_fn(|a| (p[a] / (2.0 * half[a]) + 0.5).floor() as i32);
            let c: [i32; 3] = std::array::from_fn(|a| cell[a] + o[a]);
            if o == [0; 3] || !inside(c) {
                return false;
            }
            let kn = cells.kind(c[0], c[1], c[2]);
            kn.shape != super::Shape::Empty && !kn.style.leafy
        };
        let open = card_faces(ex, LEAVES.cards.interior_cells, |d, step| {
            let mut c = cell;
            c[FACES[d].0] += FACES[d].1 * step;
            // Past the padding: unknown, taken as open (the cards kept).
            inside(c).then(|| cells.kind(c[0], c[1], c[2]).style.leafy)
        });
        let list = cards(open, world, &LEAVES.cards, at, blocked);
        emit_cards(b, cells, k, cell, &list, tint);
    }
}

/// The exposed faces `ex` that get cards: those whose way out is open. A face is dropped when a
/// leaf cell lies `2..=reach` cells straight out of it (`leafy_at(face, step)`: Some(leafy) for
/// the cell `step` cells out, None where unknown, taken as open): it looks into a cavity inside
/// the crown, where its cards would be drawn and hidden (BUGS.md #78).
pub(crate) fn card_faces(
    ex: [bool; 6],
    reach: u32,
    leafy_at: impl Fn(usize, i32) -> Option<bool>,
) -> [bool; 6] {
    std::array::from_fn(|d| {
        ex[d]
            && !(2..=reach as i32)
                .map_while(|step| leafy_at(d, step))
                .any(|leafy| leafy)
    })
}

/// A card's `plants.y` (the wind's speed for a plant, >= 0): the card's plane normal `n` (metres,
/// unit), octahedral-packed into 11 + 11 bits, stored as -(1 + packed), which marks the vertex a
/// card's (its wind speed 1). The cutout shaders' `plant_speed` and `card_normal` read it back
/// (BUGS.md #78: a card on the crown's far side, facing away, is dropped in the vertex shader).
pub(crate) fn card_speed(n: V) -> f32 {
    let l1 = n[0].abs() + n[1].abs() + n[2].abs();
    let (mut u, mut v) = (n[0] / l1, n[2] / l1);
    if n[1] < 0.0 {
        let (pu, pv) = (u, v);
        u = (1.0 - pv.abs()) * if pu >= 0.0 { 1.0 } else { -1.0 };
        v = (1.0 - pu.abs()) * if pv >= 0.0 { 1.0 } else { -1.0 };
    }
    let q = |x: f64| ((x.clamp(-1.0, 1.0) * 0.5 + 0.5) * 2047.0).round();
    -(1.0 + (q(u) * 2048.0 + q(v))) as f32
}

/// `card_speed`'s normal back (the WGSL `card_normal`'s twin; face B's mirror re-packs with it).
pub(crate) fn card_normal_of(speed: f32) -> V {
    let q = f64::from(-speed - 1.0);
    let uq = (q / 2048.0).floor();
    let vq = q - uq * 2048.0;
    let (u, v) = (uq / 2047.0 * 2.0 - 1.0, vq / 2047.0 * 2.0 - 1.0);
    let mut n = [u, 1.0 - u.abs() - v.abs(), v];
    if n[1] < 0.0 {
        n[0] = (1.0 - v.abs()) * if u >= 0.0 { 1.0 } else { -1.0 };
        n[2] = (1.0 - u.abs()) * if v >= 0.0 { 1.0 } else { -1.0 };
    }
    normalise(n).unwrap_or([0.0, 1.0, 0.0])
}

/// A leaf card: its corners (metres from the cell's centre, counter-clockwise seen from its
/// front), the normal at each (metres) and the cell face it stands off.
#[derive(Clone, Debug)]
pub(crate) struct Card {
    pub pts: [V; 4],
    pub normals: [V; 4],
    pub face: usize,
}

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The cards of a leaf cell exposed on `ex` at face-local world cell `world` (the Lab's
/// `leafCards`): per exposed face a number from `look.density` (tops 2.6×, sides 2.2× by their
/// area, bottoms 1.2×), each `look.size_m` ±25 % across, standing a little off the face and facing
/// out of the crown (`outward`, metres from the cell's centre → the crown's direction), jittered by
/// a hash of the cell and face, so a cell's cards are the same whichever chunk meshes it. A card
/// with a corner where `blocked` (metres from the cell's centre: inside a block it must not cross)
/// is dropped.
pub(crate) fn cards(
    ex: [bool; 6],
    world: [i64; 3],
    look: &dwell_look::leaves::LeafCards,
    outward: impl Fn(V) -> Option<V>,
    blocked: impl Fn(V) -> bool,
) -> Vec<Card> {
    let half = [0.5, 0.5 * CELL_H, 0.5];
    let mut out = Vec::new();
    for (d, &open) in ex.iter().enumerate() {
        if !open {
            continue;
        }
        let mut state = mix64(
            (world[0] as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (world[1] as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
                ^ (world[2] as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
                ^ (d as u64 + 1),
        );
        let mut r = || {
            state = mix64(state.wrapping_add(0x9e37_79b9_7f4a_7c15));
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let (axis, _) = FACES[d];
        let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
        let share = match d {
            2 => 2.6,
            3 => 1.2,
            _ => 2.2 * CELL_H,
        };
        let want = look.density * share;
        let mut n = want.floor() as usize + usize::from(r() < want.fract());
        let fnrm = unit(d);
        while n > 0 {
            n -= 1;
            let size = look.size_m * (0.75 + r() * 0.5);
            let mut c = [0.0; 3];
            c[axis] = fnrm[axis] * (half[axis] + size * (0.05 + r() * 0.2));
            c[ua] = (r() - 0.5) * 1.8 * half[ua];
            c[va] = (r() - 0.5) * 1.8 * half[va];
            let out_dir = canopy_normal(fnrm, outward(c), 0.5);
            let jitter = [r() - 0.5, r() - 0.5, r() - 0.5];
            let cn =
                normalise(std::array::from_fn(|i| out_dir[i] + jitter[i] * 0.9)).unwrap_or(out_dir);
            let up0: V = if cn[1].abs() > 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            // Turn `up` about the card's normal (Rodrigues).
            let ang = (r() - 0.5) * 1.2;
            let (s, co) = ang.sin_cos();
            let kx = cross(cn, up0);
            let kd = dot(cn, up0);
            let up: V = std::array::from_fn(|i| up0[i] * co + kx[i] * s + cn[i] * kd * (1.0 - co));
            let Some(ux) = normalise(cross(up, cn)) else {
                continue;
            };
            let uy = cross(cn, ux);
            let h = size / 2.0;
            let pts: [V; 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(a, b)| std::array::from_fn(|i| c[i] + ux[i] * a * h + uy[i] * b * h));
            if pts.iter().any(|&p| blocked(p)) {
                continue;
            }
            let blend = LEAVES.canopy_blend.max(0.6);
            let normals = pts.map(|p| canopy_normal(cn, outward(p), blend));
            out.push(Card {
                pts,
                normals,
                face: d,
            });
        }
    }
    out
}

/// Pushes `list` into the cutout mesh `b` with their `plants` stream (sway, wind, the distance
/// fade) and `bare` (no ink).
fn emit_cards(
    b: &mut Builder,
    cells: &Cells<'_>,
    k: &Kind,
    cell: [i32; 3],
    list: &[Card],
    leaf_tint: Tint,
) {
    if list.is_empty() {
        return;
    }
    let look = &LEAVES.cards;
    // The streams the leaf faces before skipped: zeros, as the mesher pads them.
    let before = b.vertex_count() as usize;
    b.m.plants.resize(before * 4, 0.0);
    b.m.bare.resize(before, 0.0);
    // The painted clump (`cards.tile`), white so it keeps its own colours; the leaf's side tile in a
    // build whose atlas lacks it.
    let (color, mut tile) = if crate::textures::has_tile(look.tile) {
        (0xffffff, crate::textures::tile_rect(look.tile))
    } else {
        surface(&k.style, 0, 1)
    };
    let color = leaf_tint.apply_rgb8(color);
    tile.turn = false;
    let [x, y, z] = cell;
    for card in list {
        let d = card.face;
        let (a, s) = FACES[d];
        // The card's plane normal, for the vertex shader's far-side cull (`card_speed`).
        let p = &card.pts;
        let edge = |i: usize| -> V { std::array::from_fn(|c| p[i][c] - p[0][c]) };
        let card_normal = normalise(cross(edge(1), edge(3))).unwrap_or(unit(d));
        let mut fq = [0.5; 3];
        fq[a] = if s > 0 { 1.0 } else { 0.0 };
        let light = vertex_light(AO_OPEN, cells.sky_rays(cell, fq, unit(d), 60 + d as i64));
        let front = super::neighbour_of(x, y, z, a, s);
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let (sky, block) = cells.flood.corner(cells.occ, front, u, 1, v, 1);
        let flood = [
            f32::from(sky) / crate::flood::QUARTERS,
            f32::from(block) / crate::flood::QUARTERS,
        ];
        let base = b.vertex_count();
        for (i, p) in card.pts.iter().enumerate() {
            let n = card.normals[i];
            let tint = normal_tint(n[0], n[1], n[2]);
            let pos = [
                f64::from(x) + 0.5 + p[0],
                f64::from(y) + 0.5 + p[1] / CELL_H,
                f64::from(z) + 0.5 + p[2],
            ];
            b.m.positions.extend(pos.map(|c| c as f32));
            b.m.normals.extend(n.map(|c| c as f32));
            b.m.colors.extend([
                (super::channel(color, 16) * tint[0]) as f32,
                (super::channel(color, 8) * tint[1]) as f32,
                (super::channel(color, 0) * tint[2]) as f32,
            ]);
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][i];
            b.m.uvs.extend(uv);
            b.push_tile(&tile);
            b.m.light.push(light);
            b.m.flood.extend(flood);
            // The far corners (the top two) sway; the near ones barely.
            let sway = if i >= 2 {
                look.sway_m
            } else {
                look.sway_m * 0.25
            };
            b.m.plants.extend([
                sway as f32,
                card_speed(card_normal),
                look.fade_start_m as f32,
                look.fade_end_m as f32,
            ]);
            b.m.bare.push(1.0);
        }
        b.m.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn emit(
    b: &mut Builder,
    cells: &Cells<'_>,
    crowns: &Crowns,
    k: &Kind,
    (cell, ex): ([i32; 3], [bool; 6]),
    piece: &Piece,
    leaf_tint: Tint,
) {
    let [x, y, z] = cell;
    let n_m = normal_to_m(piece.normal);
    // The tile and its uv projection: the dominant axis of the face.
    let dom = (0..3)
        .max_by(|&i, &j| n_m[i].abs().total_cmp(&n_m[j].abs()))
        .unwrap_or(1);
    let sign = if n_m[dom] >= 0.0 { 1 } else { -1 };
    let (color, tile) = surface(&k.style, dom, sign);
    let color = leaf_tint.apply_rgb8(color);
    let base = b.vertex_count();
    let sides: Vec<usize> = if piece.sides.is_empty() {
        vec![super::face_of(dom, sign)]
    } else {
        piece.sides.clone()
    };
    for &q in &piece.pts {
        // Cell-local point in 0..1 and chunk-local point (cells).
        let f: V = q.map(|c| c + 0.5);
        let p: V = [
            f64::from(x) + f[0],
            f64::from(y) + f[1],
            f64::from(z) + f[2],
        ];
        let n = canopy_normal(n_m, crowns.outward(p), LEAVES.canopy_blend);
        let tint = normal_tint(n[0], n[1], n[2]);
        let r = super::channel(color, 16) * tint[0];
        let g = super::channel(color, 8) * tint[1];
        let bl = super::channel(color, 0) * tint[2];
        b.m.positions
            .extend([p[0] as f32, p[1] as f32, p[2] as f32]);
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
        // The flood light also from the open face the leaned normal looks out of most (a side's
        // corner leaned up takes the sky above it).
        let nd = (0..3)
            .max_by(|&i, &j| n[i].abs().total_cmp(&n[j].abs()))
            .unwrap_or(1);
        let looks = super::face_of(nd, if n[nd] >= 0.0 { 1 } else { -1 });
        let extra = (ex[looks] && !sides.contains(&looks)).then_some(looks);
        for &side in sides.iter().chain(extra.iter()) {
            let (a, s) = FACES[side];
            let mut fq = f;
            fq[a] = if s > 0 { 1.0 } else { 0.0 };
            if Some(side) != extra {
                light += vertex_light(
                    AO_OPEN,
                    cells.sky_rays(cell, fq, unit(side), 40 + side as i64),
                );
            }
            let front = super::neighbour_of(x, y, z, a, s);
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let toward = |c: f64| if c < 0.5 { -1 } else { 1 };
            let (sky, block) =
                cells
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
