//! Block damage as a 3D bit mask (owner, 2026-11-15: "We can update damage to be more of a 3D bit
//! mask, which should work on any object shape."; docs/BLOCK_DAMAGE.md §15). A damaged cell's
//! removed material is a mask over a sub-grid of the cell (the look's `grid` per metre: 8 × 4 × 8
//! for a soil, 12 × 6 × 12 for stone), and the cell is drawn as its block's own shape (any convex
//! shape: the cube, the octagonal log, a ramp, a ½ m piece) intersected with what the mask keeps:
//! the shape's faces where they cross kept sub-cells, and the walls between kept and removed
//! sub-cells clipped to the shape.
//!
//! The mask is a function of the cell's damage view alone (its fraction and its hit marks: each
//! the face struck and the point), which the server already keeps and sends, so it needs no new
//! saved or wire state. Every sub-cell is ranked by its cost from the marks (inward from the struck
//! point, shaped by the look's `profile`: a soil's round bowl, stone's flat flakes, a facet look's
//! angular chunks), and the cheapest are removed until the bite rule's volume is gone (the
//! fraction × half the shape's own volume, BLOCK_DAMAGE.md §10): so the mask only grows as the
//! damage does, and the same view always gives the same mask.
//!
//! Units: cell units (each axis 0..1 over the cell; y's 1 is the ½ m `CELL_H`), as the shapes'
//! polygons; metres only for the costs and volumes.

use dwell_protocol::damage::{BROKEN_AT, CarveProfile, DamageLook, DamageView, HitMark};
use dwell_world::CELL_H;

/// A cell's size along an axis (m).
fn ext(a: usize) -> f64 {
    if a == 1 { CELL_H } else { 1.0 }
}

const EPS: f64 = 1e-9;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A polygon's (unnormalised) normal by Newell's method: counter-clockwise seen from outside.
pub(crate) fn newell(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for (i, a) in pts.iter().enumerate() {
        let b = pts[(i + 1) % pts.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// A polygon's area (cell units).
pub(crate) fn area(pts: &[[f64; 3]]) -> f64 {
    let n = newell(pts);
    0.5 * dot(n, n).sqrt()
}

/// The part of convex polygon `pts` where `n · p ≤ d` (Sutherland–Hodgman).
fn clip_plane(pts: &[[f64; 3]], n: [f64; 3], d: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::with_capacity(pts.len() + 2);
    for (i, a) in pts.iter().enumerate() {
        let b = pts[(i + 1) % pts.len()];
        let (da, db) = (dot(n, *a) - d, dot(n, b) - d);
        if da <= EPS {
            out.push(*a);
        }
        if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
            let t = da / (da - db);
            out.push(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t));
        }
    }
    out
}

/// A convex solid in cell units: the half-spaces `n · p ≤ d` (a block's shape).
#[derive(Clone, Debug, Default)]
pub(crate) struct Hull {
    planes: Vec<([f64; 3], f64)>,
}

impl Hull {
    /// The full cell.
    pub(crate) fn cube() -> Hull {
        let mut planes = Vec::new();
        for a in 0..3 {
            let mut n = [0.0; 3];
            n[a] = 1.0;
            planes.push((n, 1.0));
            n[a] = -1.0;
            planes.push((n, 0.0));
        }
        Hull { planes }
    }

    /// The convex solid bounded by `polygons` (a shape's faces, any winding): each polygon's
    /// plane, turned to face away from their points' centre. With the cube's planes.
    pub(crate) fn of_polygons(polygons: &[Vec<[f64; 3]>]) -> Hull {
        let all: Vec<[f64; 3]> = polygons.iter().flatten().copied().collect();
        let mut hull = Hull::cube();
        if all.is_empty() {
            return hull;
        }
        let k = all.len() as f64;
        let c = all.iter().fold([0.0; 3], |s, p| {
            [s[0] + p[0] / k, s[1] + p[1] / k, s[2] + p[2] / k]
        });
        for poly in polygons.iter().filter(|p| p.len() >= 3) {
            let n = newell(poly);
            let l = dot(n, n).sqrt();
            if l < 1e-12 {
                continue;
            }
            let n = n.map(|x| x / l);
            let d = dot(n, poly[0]);
            let (n, d) = if dot(n, c) > d {
                (n.map(|x| -x), -d)
            } else {
                (n, d)
            };
            hull.planes.push((n, d));
        }
        hull
    }

    /// The prism along `axis` of the convex cross-section `outline` (points on the axes
    /// `(axis + 1) % 3`, `(axis + 2) % 3`, cell units), inside the cell.
    pub(crate) fn prism(outline: &[[f64; 2]], axis: usize) -> Hull {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let lift = |p: [f64; 2], t: f64| {
            let mut q = [0.0; 3];
            q[axis] = t;
            q[u] = p[0];
            q[v] = p[1];
            q
        };
        let sides: Vec<Vec<[f64; 3]>> = (0..outline.len())
            .map(|i| {
                let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
                vec![lift(a, 0.0), lift(b, 0.0), lift(b, 1.0), lift(a, 1.0)]
            })
            .collect();
        Hull::of_polygons(&sides)
    }

    /// This hull cut by `other`'s planes too (their intersection).
    pub(crate) fn and(mut self, other: &Hull) -> Hull {
        self.planes.extend(other.planes.iter().copied());
        self
    }

    fn inside(&self, p: [f64; 3]) -> bool {
        self.planes.iter().all(|(n, d)| dot(*n, p) <= d + 1e-9)
    }

    /// The part of convex polygon `pts` inside the hull.
    pub(crate) fn clip(&self, pts: &[[f64; 3]]) -> Vec<[f64; 3]> {
        let mut out = pts.to_vec();
        for (n, d) in &self.planes {
            if out.len() < 3 {
                break;
            }
            out = clip_plane(&out, *n, *d);
        }
        out
    }
}

/// A wall of the removed material: a kept sub-cell's face toward a removed one, clipped to the
/// shape (cell units, counter-clockwise seen from the removed side), looking along `axis` by `sign`.
#[derive(Clone, Debug)]
pub(crate) struct InnerFace {
    pub pts: Vec<[f64; 3]>,
    pub axis: usize,
    pub sign: i32,
}

/// A damaged cell's removed material (the module's doc).
#[derive(Clone, Debug)]
pub(crate) struct DamageMask {
    n: [usize; 3],
    removed: Vec<bool>,
    /// The removed count over each box of sub-cells from the origin (a summed-volume table).
    sums: Vec<u32>,
    /// The marks, face-local: (the struck point (m), the inward normal) for the walls' shade.
    marks: Vec<([f64; 3], [f64; 3])>,
    /// The deepest a removed sub-cell lies below its nearest mark (m).
    deepest: f64,
}

/// A stable hash in [0, 1).
fn hash01(a: u32, b: u32, c: u32) -> f64 {
    let mut h =
        a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    f64::from(h >> 8) / f64::from(1u32 << 24)
}

/// The sub-grid of a look: its `grid` cells per metre on each axis (2–16), at least one.
pub(crate) fn grid_of(look: &DamageLook) -> [usize; 3] {
    let g = look.grid.clamp(2, 16) as usize;
    [g, ((g as f64 * CELL_H).round() as usize).max(1), g]
}

/// The share of sub-cell box `lo..hi` (cell units) inside `hull`: 1 or 0 when its corners say
/// so, else sampled (4³).
fn share_in(hull: &Hull, lo: [f64; 3], hi: [f64; 3]) -> f64 {
    let corner = |i: usize| -> [f64; 3] {
        std::array::from_fn(|a| if i >> a & 1 == 1 { hi[a] } else { lo[a] })
    };
    if (0..8).all(|i| hull.inside(corner(i))) {
        return 1.0;
    }
    // Wholly outside one plane: none.
    if hull
        .planes
        .iter()
        .any(|(n, d)| (0..8).all(|i| dot(*n, corner(i)) >= d - 1e-9))
    {
        return 0.0;
    }
    const S: usize = 4;
    let mut inside = 0;
    for i in 0..S {
        for j in 0..S {
            for k in 0..S {
                let f = [i, j, k].map(|t| (t as f64 + 0.5) / S as f64);
                let p = std::array::from_fn(|a| lo[a] + (hi[a] - lo[a]) * f[a]);
                inside += usize::from(hull.inside(p));
            }
        }
    }
    inside as f64 / (S * S * S) as f64
}

/// A mark face-locally: its point (m) and the inward normal of the face it struck.
fn mark_frame(m: &HitMark) -> ([f64; 3], [f64; 3]) {
    let face = usize::from(m.face.min(5));
    let axis = face / 2;
    let p = std::array::from_fn(|a| f64::from(m.point[a]) / 255.0 * ext(a));
    let mut n = [0.0; 3];
    // Even faces look +axis: inward is −axis.
    n[axis] = if face.is_multiple_of(2) { -1.0 } else { 1.0 };
    (p, n)
}

impl DamageMask {
    /// The mask of a cell of shape `hull` damaged as `view` says (face-local), shaped by `look`;
    /// none when it removes nothing (no marks, no damage yet).
    pub(crate) fn of(look: &DamageLook, view: &DamageView, hull: &Hull) -> Option<DamageMask> {
        let fraction = f64::from(view.fraction) / 255.0;
        if view.marks.is_empty() || fraction <= 0.0 {
            return None;
        }
        let n = grid_of(look);
        let count = n[0] * n[1] * n[2];
        let idx = |i: usize, j: usize, k: usize| i + n[0] * (j + n[1] * k);
        let lo = |i: usize, j: usize, k: usize| {
            [
                i as f64 / n[0] as f64,
                j as f64 / n[1] as f64,
                k as f64 / n[2] as f64,
            ]
        };
        let vm = CELL_H / count as f64;
        let mut share = vec![0.0; count];
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    share[idx(i, j, k)] = share_in(hull, lo(i, j, k), lo(i + 1, j + 1, k + 1));
                }
            }
        }
        let total: f64 = share.iter().sum::<f64>() * vm;
        let target = fraction.min(1.0) * BROKEN_AT * total;
        // Each mark weighed by its piece (its bite against the others', `HitMark::chunk`). Every
        // mark the view holds (a client keeps all it has seen, BUGS.md #188).
        let marks: Vec<&HitMark> = view.marks.iter().collect();
        let mean = marks.iter().map(|m| m.scale()).sum::<f64>() / marks.len() as f64;
        let frames: Vec<_> = marks.iter().map(|m| mark_frame(m)).collect();
        // The look's shape: how much wider than deep a bite runs (its `size` across over its
        // `depth`), kept to chunks (no hairline peel, no drill).
        let aniso = (0.5 * look.size / look.depth.max(0.02)).clamp(0.7, 3.0);
        let profile = look.profile;
        // The cost of sub-cell (i, j, k) from the first `upto` marks (each its own bite's cost).
        let cost_upto = |i: usize, j: usize, k: usize, upto: usize| -> f64 {
            let c = lo(i, j, k);
            let h = lo(i + 1, j + 1, k + 1);
            let centre: [f64; 3] = std::array::from_fn(|a| 0.5 * (c[a] + h[a]) * ext(a));
            let mut best = f64::INFINITY;
            for (mi, (m, (p, nin))) in marks.iter().zip(&frames).enumerate().take(upto) {
                let d: [f64; 3] = std::array::from_fn(|a| centre[a] - p[a]);
                let along = dot(d, *nin).abs();
                let lat: [f64; 3] = std::array::from_fn(|a| d[a] - dot(d, *nin) * nin[a]);
                let axis = nin.iter().position(|x| x.abs() > 0.5).unwrap_or(1);
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let seed = u32::from(m.point[0])
                    | u32::from(m.point[1]) << 8
                    | u32::from(m.point[2]) << 16;
                let (mut lu, mut lv) = (lat[u], lat[v]);
                let base = match profile {
                    CarveProfile::Bowl => {
                        ((lu * lu + lv * lv) / (aniso * aniso) + along * along).sqrt()
                    }
                    CarveProfile::Facets => {
                        let l2 = ((lu * lu + lv * lv) / (aniso * aniso) + along * along).sqrt();
                        let linf = (lu.abs() / aniso).max(lv.abs() / aniso).max(along);
                        0.5 * (l2 + linf)
                    }
                    CarveProfile::Spall => {
                        // A flat flake, long along a turn of its own.
                        let th = std::f64::consts::PI * hash01(seed, mi as u32, 7);
                        let (s, co) = th.sin_cos();
                        (lu, lv) = (co * lu + s * lv, -s * lu + co * lv);
                        ((lu / 1.5).powi(2) / (aniso * aniso)
                            + (lv * 1.5).powi(2) / (aniso * aniso)
                            + along * along)
                            .sqrt()
                    }
                };
                // Chunky edges: a jitter per pair of sub-cells (none for a bowl's round scoop).
                let rough = match profile {
                    CarveProfile::Bowl => 0.08,
                    CarveProfile::Facets => 0.3,
                    CarveProfile::Spall => 0.35,
                };
                let j01 = hash01(
                    (i / 2 + 31 * (j / 2) + 977 * (k / 2)) as u32,
                    seed,
                    mi as u32,
                );
                let rel = m.scale() / mean.max(1e-6);
                let c = base * (1.0 + rough * (j01 - 0.5)) / rel.max(1e-3);
                best = best.min(c);
            }
            best
        };
        // Bite by bite (BUGS.md #188: chopping round a log, the far side's bites filled back in as
        // newer marks took the volume): after mark `b` of `m` the volume removed is `b / m` of the
        // target (each hit took its share, as the fraction grew), the cheapest sub-cells by the
        // marks so far, and what an earlier bite removed stays removed.
        let m = marks.len();
        let mut removed = vec![false; count];
        let mut gone = 0.0;
        let mut any = false;
        for bite in 1..=m {
            let goal = target * bite as f64 / m as f64;
            let mut order: Vec<(f64, usize)> = Vec::with_capacity(count);
            for k in 0..n[2] {
                for j in 0..n[1] {
                    for i in 0..n[0] {
                        let at = idx(i, j, k);
                        if share[at] > 0.0 && !removed[at] {
                            order.push((cost_upto(i, j, k, bite), at));
                        }
                    }
                }
            }
            order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for &(_, at) in &order {
                let v = share[at] * vm;
                if gone + 0.5 * v > goal {
                    break;
                }
                gone += v;
                removed[at] = true;
                any = true;
            }
        }
        if !any {
            return None;
        }
        // The summed-volume table.
        let (a, b, c) = (n[0] + 1, n[1] + 1, n[2] + 1);
        let mut sums = vec![0u32; a * b * c];
        let s = |i: usize, j: usize, k: usize| i + a * (j + b * k);
        for k in 1..c {
            for j in 1..b {
                for i in 1..a {
                    let here = u32::from(removed[idx(i - 1, j - 1, k - 1)]);
                    sums[s(i, j, k)] =
                        here + sums[s(i - 1, j, k)] + sums[s(i, j - 1, k)] + sums[s(i, j, k - 1)]
                            - sums[s(i - 1, j - 1, k)]
                            - sums[s(i - 1, j, k - 1)]
                            - sums[s(i, j - 1, k - 1)]
                            + sums[s(i - 1, j - 1, k - 1)];
                }
            }
        }
        let mut mask = DamageMask {
            n,
            removed,
            sums,
            marks: frames,
            deepest: 0.0,
        };
        let mut deepest: f64 = 0.0;
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    if mask.removed[idx(i, j, k)] {
                        let c = lo(i, j, k);
                        let h = lo(i + 1, j + 1, k + 1);
                        let p = std::array::from_fn(|a| 0.5 * (c[a] + h[a]));
                        deepest = deepest.max(mask.depth_m(p));
                    }
                }
            }
        }
        mask.deepest = deepest.max(1e-6);
        Some(mask)
    }

    /// The sub-grid's size per axis.
    #[cfg(test)]
    pub(crate) fn dims(&self) -> [usize; 3] {
        self.n
    }

    fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }

    /// Whether sub-cell (i, j, k) is removed.
    pub(crate) fn is_removed(&self, i: usize, j: usize, k: usize) -> bool {
        self.removed[self.idx(i, j, k)]
    }

    /// The removed sub-cells.
    #[cfg(test)]
    pub(crate) fn removed_count(&self) -> usize {
        self.removed.iter().filter(|r| **r).count()
    }

    /// The removed count over sub-cells `lo..hi` (indices, hi exclusive).
    fn removed_in(&self, lo: [usize; 3], hi: [usize; 3]) -> u32 {
        let (a, b) = (self.n[0] + 1, self.n[1] + 1);
        let s = |i: usize, j: usize, k: usize| self.sums[i + a * (j + b * k)];
        let [x0, y0, z0] = lo;
        let [x1, y1, z1] = hi;
        (s(x1, y1, z1) + s(x0, y0, z1) + s(x0, y1, z0) + s(x1, y0, z0))
            .wrapping_sub(s(x0, y1, z1) + s(x1, y0, z1) + s(x1, y1, z0) + s(x0, y0, z0))
    }

    /// How far below the surface struck a point (cell units) lies (m): along its nearest mark's
    /// inward normal.
    fn depth_m(&self, p: [f64; 3]) -> f64 {
        let pm: [f64; 3] = std::array::from_fn(|a| p[a] * ext(a));
        self.marks
            .iter()
            .map(|(m, nin)| {
                let d: [f64; 3] = std::array::from_fn(|a| pm[a] - m[a]);
                (dot(d, *nin).max(0.0), dot(d, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0.0, |(along, _)| along)
    }

    /// How deep a point (cell units) lies in the removed material, 0 at the surface struck to 1 at
    /// the deepest removed (a wall's shade).
    pub(crate) fn depth_share(&self, p: [f64; 3]) -> f64 {
        (self.depth_m(p) / self.deepest).clamp(0.0, 1.0)
    }

    /// The pieces of convex polygon `pts` (one of the shape's faces, cell units, inside the cell)
    /// where the mask keeps the material: the polygon whole where nothing near it is removed, else
    /// split along the sub-grid where it must be, the removed sub-cells' parts dropped.
    pub(crate) fn clip(&self, pts: &[[f64; 3]]) -> Vec<Vec<[f64; 3]>> {
        let normal = newell(pts);
        let mut out = Vec::new();
        self.split(pts.to_vec(), normal, [0; 3], self.n, &mut out);
        out.retain(|p| p.len() >= 3 && area(p) > 1e-10);
        out
    }

    fn split(
        &self,
        pts: Vec<[f64; 3]>,
        normal: [f64; 3],
        lo: [usize; 3],
        hi: [usize; 3],
        out: &mut Vec<Vec<[f64; 3]>>,
    ) {
        if pts.len() < 3 {
            return;
        }
        let gone = self.removed_in(lo, hi);
        if gone == 0 {
            out.push(pts);
            return;
        }
        let cells = (0..3).map(|a| hi[a] - lo[a]).product::<usize>() as u32;
        if gone == cells {
            return;
        }
        // Split the box along its longest side (in sub-cells).
        let axis = (0..3).max_by_key(|&a| hi[a] - lo[a]).unwrap_or(0);
        let mid = (lo[axis] + hi[axis]) / 2;
        let plane = mid as f64 / self.n[axis] as f64;
        // A polygon lying on the split plane belongs to the side its material is on (behind its
        // normal).
        if pts.iter().all(|p| (p[axis] - plane).abs() < 1e-9) {
            let (mut l, mut h) = (lo, hi);
            if normal[axis] > 0.0 {
                h[axis] = mid;
            } else {
                l[axis] = mid;
            }
            self.split(pts, normal, l, h, out);
            return;
        }
        let mut e = [0.0; 3];
        e[axis] = 1.0;
        let below = clip_plane(&pts, e, plane);
        let above = clip_plane(&pts, e.map(|x| -x), -plane);
        let mut h = hi;
        h[axis] = mid;
        let mut l = lo;
        l[axis] = mid;
        self.split(below, normal, lo, h, out);
        self.split(above, normal, l, hi, out);
    }

    /// The walls between kept and removed sub-cells, clipped to `hull` (the shape), merged into
    /// rectangles where they run on.
    pub(crate) fn inner_faces(&self, hull: &Hull) -> Vec<InnerFace> {
        let n = self.n;
        let mut out = Vec::new();
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for sign in [1i32, -1] {
                // Layer `l`: the plane between sub-cells l − 1 and l along the axis.
                for l in 1..n[axis] {
                    let mut grid = vec![false; n[u] * n[v]];
                    for b in 0..n[v] {
                        for a in 0..n[u] {
                            let at = |t: usize| {
                                let mut c = [0; 3];
                                c[axis] = t;
                                c[u] = a;
                                c[v] = b;
                                self.is_removed(c[0], c[1], c[2])
                            };
                            // The kept sub-cell behind the wall, the removed one it looks into.
                            let (kept, gone) = if sign > 0 { (l - 1, l) } else { (l, l - 1) };
                            grid[a + n[u] * b] = !at(kept) && at(gone);
                        }
                    }
                    // Greedy rectangles (the walls of a cell's kept sub-cells with no material are
                    // clipped away by the hull).
                    let plane = l as f64 / n[axis] as f64;
                    for b in 0..n[v] {
                        let mut a = 0;
                        while a < n[u] {
                            if !grid[a + n[u] * b] {
                                a += 1;
                                continue;
                            }
                            let mut w = 1;
                            while a + w < n[u] && grid[a + w + n[u] * b] {
                                w += 1;
                            }
                            let mut h = 1;
                            'grow: while b + h < n[v] {
                                for t in a..a + w {
                                    if !grid[t + n[u] * (b + h)] {
                                        break 'grow;
                                    }
                                }
                                h += 1;
                            }
                            for bb in b..b + h {
                                for t in a..a + w {
                                    grid[t + n[u] * bb] = false;
                                }
                            }
                            let p = |ua: usize, vb: usize| {
                                let mut q = [0.0; 3];
                                q[axis] = plane;
                                q[u] = ua as f64 / n[u] as f64;
                                q[v] = vb as f64 / n[v] as f64;
                                q
                            };
                            // e_u × e_v = e_axis: counter-clockwise seen from +axis.
                            let mut pts = vec![p(a, b), p(a + w, b), p(a + w, b + h), p(a, b + h)];
                            if sign < 0 {
                                pts.reverse();
                            }
                            let pts = hull.clip(&pts);
                            if pts.len() >= 3 && area(&pts) > 1e-10 {
                                out.push(InnerFace { pts, axis, sign });
                            }
                            a += w;
                        }
                    }
                }
            }
        }
        out
    }
}

/// The closed volume (m³) of a triangle mesh in cell units (y × `CELL_H`), by the divergence
/// theorem: for tests over a meshed cell.
#[cfg(test)]
pub(crate) fn mesh_volume_m3(pts: &[[f64; 3]], tris: &[[u32; 3]]) -> f64 {
    let m = |p: [f64; 3]| [p[0], p[1] * CELL_H, p[2]];
    tris.iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m(pts[i as usize]));
            let cr = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            dot(a, cr) / 6.0
        })
        .sum()
}

#[cfg(test)]
#[path = "damage_mask_tests.rs"]
mod tests;
