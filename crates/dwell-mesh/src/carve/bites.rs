//! The axe's bites carved into a cut wood cell (WORK_QUEUE item 26; owner, 2026-11-12: the slice
//! "cuts a line all the way across which is much bigger than the tool head. It should align more
//! with the dirt and stone ones").
//!
//! Each hit records where it struck a side ([`dwell_protocol::chopping::Bite`]): a place across the
//! side and the band it cut. A side's notch is one V along the grain under a mask across the side,
//! each bite's footprint (1 over its width, its two ends sloping to 0 over `bite.endsM`), so where
//! the hits spread across the side (a hit on a bitten spot lands beside it) they join into the
//! notch. The V is grown by the bite rule (WORK_QUEUE item 18, [`Field::of`]): the cell's carve is
//! its share of the swings to fell × half the log's volume, the V deepening, its bites widening and
//! its mouth opening past the ¼ m band until at the fell it is the gash's. Opposite sides never meet
//! (a waist of [`super::MIN_LEFT`] is left).
//!
//! The geometry: the uncut cell's faces ([`super::carve_in`], clipped to an octagonal log's prism),
//! each bitten side's bark face (a rectangle in (along the grain, across) on the side's plane)
//! replaced by a height field on a tensor grid through the V's and masks' breakpoints, pushed in
//! along the side's inward normal. Every bite's mouth is kept inside the face and every V ends
//! inside the cell, so the face's outline never moves: the other faces and the neighbours meet it
//! as before (watertight; the grid's edge vertices are T-junctions on straight edges, as
//! `damage_mesh`'s carve). Each grid quad is two flat-lit triangles (a hewn face), uncut quads one.

use super::{Face, MIN_LEFT, Part, TINY, across, normal, size_m};
use dwell_protocol::chopping::{Cut, CutLook, SIDES};

/// One V along the grain (`c` its apex, `hw` its half-width, `t` units; `d` its depth, m) under a
/// mask across the side.
#[derive(Clone, Debug)]
struct Part2 {
    c: f64,
    hw: f64,
    d: f64,
    mask: Vec<Mask>,
}

/// A bite's footprint across a side (cell units): 1 within `half − ends` of `c`, 0 past `half`.
#[derive(Clone, Copy, Debug)]
struct Mask {
    c: f64,
    half: f64,
    ends: f64,
}

impl Mask {
    fn at(&self, u: f64) -> f64 {
        let x = self.half - (u - self.c).abs();
        if self.ends <= 1e-9 {
            if x > 0.0 { 1.0 } else { 0.0 }
        } else {
            (x / self.ends).clamp(0.0, 1.0)
        }
    }
}

impl Part2 {
    fn at(&self, t: f64, u: f64) -> f64 {
        let v = self.d * (1.0 - (t - self.c).abs() / self.hw).max(0.0);
        if v <= 0.0 {
            return 0.0;
        }
        v * self.mask.iter().map(|m| m.at(u)).fold(0.0, f64::max)
    }
}

/// The bites' depth field of each side of a cell.
pub(crate) struct Field {
    grain: usize,
    sides: [Vec<Part2>; SIDES],
    /// The log's thickness (cells) along each across axis.
    thick: [f64; 2],
    /// Each side's view of the log's cross-section (cell units on the two axes across the grain):
    /// a cut stops short of the far side's outline.
    fronts: [Front; SIDES],
}

impl Field {
    /// The field of `cut` (face-local sides, its bites recorded) at `progress` of its swings to
    /// fell ([`Cut::progress`]): side `s`'s bites kept inside `range[s]` across it (cell units:
    /// the stretch of the log its grid covers), the log `poly` its cross-section (cell units; its
    /// waist is left inside it).
    ///
    /// The bite rule (WORK_QUEUE item 18; owner, 2026-11-12: "Yes the notch should grow to follow
    /// the 50% rule"; red before: a V within the notch's ¼ m band, about a fifth of the rule at the
    /// fell): the cell's carved wood is `progress` × ½ × the log's volume, shared by the bitten
    /// sides as their cuts are (`Cut::removed_m`). Each bitten side's notch is one V along the grain
    /// centred on the cell's middle, `x` m deep, its mouth `look.bite.band_m` at least and
    /// `x ÷ depth_per_mouth` past it (the cell's length at most), under the mask of the side's
    /// bites, each widened to `x × width_per_depth` across the side (the head's width at least):
    /// `x` is found so the side's carved wood (summed over the outline) is its share. One hit is a
    /// small V where it struck; more grow it deeper, taller and across, the bites joining into it,
    /// until at the fell it is the gash's V, its mouth the cell's height, its apex near the far side.
    fn of(
        grain: usize,
        cut: &Cut,
        look: &CutLook,
        range: [[f64; 2]; SIDES],
        poly: &[[f64; 2]],
        progress: f64,
    ) -> Field {
        let thick: [f64; 2] = std::array::from_fn(|j| {
            let lo = poly.iter().map(|p| p[j]).fold(f64::INFINITY, f64::min);
            let hi = poly.iter().map(|p| p[j]).fold(f64::NEG_INFINITY, f64::max);
            hi - lo
        });
        let mut field = Field {
            grain,
            sides: std::array::from_fn(|_| Vec::new()),
            thick,
            fronts: std::array::from_fn(|s| Front::of(poly, s)),
        };
        let bitten: Vec<usize> = (0..SIDES)
            .filter(|&s| cut.bites[s].n > 0 && cut.removed_m(s) > 0.0)
            .collect();
        let total: f64 = bitten.iter().map(|&s| cut.removed_m(s)).sum();
        if bitten.is_empty() || total <= 0.0 {
            return field;
        }
        let want = progress.clamp(0.0, 1.0) * 0.5 * area2(poly);
        for &s in &bitten {
            let share = want * cut.removed_m(s) / total;
            let (mut lo, mut hi) = (0.0, 4.0);
            field.sides[s] = Self::grown(grain, cut, look, range[s], s, hi);
            if field.carved(s, range[s]) > share {
                for _ in 0..28 {
                    let x = 0.5 * (lo + hi);
                    field.sides[s] = Self::grown(grain, cut, look, range[s], s, x);
                    if field.carved(s, range[s]) < share {
                        lo = x;
                    } else {
                        hi = x;
                    }
                }
                field.sides[s] = Self::grown(grain, cut, look, range[s], s, 0.5 * (lo + hi));
            }
        }
        field
    }

    /// Side `s`'s notch grown to `x` m deep (its V and its bites' widened masks, [`Field::of`]).
    fn grown(
        grain: usize,
        cut: &Cut,
        look: &CutLook,
        range: [f64; 2],
        s: usize,
        x: f64,
    ) -> Vec<Part2> {
        let ax = across(grain);
        let len = size_m(grain);
        let size = size_m(ax[1 - s / 2]);
        let b = &look.bite;
        let [lo, hi] = range;
        let wide = (x * b.width_per_depth).max(b.width_m) / size;
        let half = (0.5 * wide).min(0.5 * (hi - lo)).max(1e-3);
        let ends = (b.ends_m / size).min(half);
        let mask = cut.bites[s]
            .all()
            .iter()
            .map(|bt| Mask {
                c: f64::from(bt.u).clamp(lo + half, (hi - half).max(lo + half)),
                half,
                ends,
            })
            .collect();
        let mouth = (x / b.depth_per_mouth.max(1e-6)).max(b.band_m).min(len);
        vec![Part2 {
            c: 0.5,
            hw: (0.5 * mouth / len).clamp(0.02, 0.5),
            d: x,
            mask,
        }]
    }

    /// The wood side `s`'s cut takes out of the log (cell units) as its grid draws it
    /// ([`side_grid`]'s triangles: the bites' surface where it is deeper than the outline).
    fn carved(&self, s: usize, range: [f64; 2]) -> f64 {
        let mut v = 0.0;
        for_grid_tris(self, &self.fronts[s], s, range, &mut |tri, _| {
            let inside = clip_gp(tri, &|p| p.d - p.c);
            for k in 1..inside.len().saturating_sub(1) {
                let (a, b, c) = (inside[0], inside[k], inside[k + 1]);
                let area = 0.5 * ((b.t - a.t) * (c.u - a.u) - (c.t - a.t) * (b.u - a.u)).abs();
                v += area * ((a.d - a.c) + (b.d - b.c) + (c.d - c.c)) / 3.0;
            }
        });
        v
    }

    /// Side `s`'s depth (m) at `t` along the grain and `u` across, before the waist.
    fn raw(&self, s: usize, t: f64, u: f64) -> f64 {
        self.sides[s].iter().map(|p| p.at(t, u)).fold(0.0, f64::max)
    }

    /// Side `s`'s depth in cells along its axis: both opposite sides scaled down together where
    /// they would leave less than the waist.
    pub(crate) fn depth(&self, s: usize, t: f64, u: f64) -> f64 {
        let axis = across(self.grain)[s / 2];
        let size = size_m(axis);
        let (a, b) = (self.raw(s, t, u), self.raw(s ^ 1, t, u));
        let room = (self.thick[s / 2] - MIN_LEFT).max(MIN_LEFT) * size;
        let k = if a + b > room { room / (a + b) } else { 1.0 };
        // Short of the far side's outline there (a thinner trunk's chamfers come in close).
        let back = (self.thick[s / 2] - self.fronts[s ^ 1].depth(u) - MIN_LEFT) * size;
        (a * k).min(back.max(0.0)) / size
    }

    fn breaks(&self, s: usize, t: &mut Vec<f64>, u: &mut Vec<f64>) {
        for p in &self.sides[s] {
            t.extend([p.c - p.hw, p.c, p.c + p.hw]);
            for m in &p.mask {
                u.extend([
                    m.c - m.half,
                    m.c - m.half + m.ends,
                    m.c + m.half - m.ends,
                    m.c + m.half,
                ]);
            }
        }
        // The opposite side's breakpoints too: the waist's scaling follows them.
        for p in &self.sides[s ^ 1] {
            t.extend([p.c - p.hw, p.c, p.c + p.hw]);
            for m in &p.mask {
                u.extend([m.c - m.half, m.c + m.half]);
            }
        }
    }
}

/// The area of a convex outline (cell units).
fn area2(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    0.5 * (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
}

/// Sorted, in `[lo, hi]`, without near-duplicates.
fn lines(mut v: Vec<f64>, lo: f64, hi: f64) -> Vec<f64> {
    v.push(lo);
    v.push(hi);
    v.retain(|x| (lo - 1e-9..=hi + 1e-9).contains(x));
    for x in &mut v {
        *x = x.clamp(lo, hi);
    }
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    v
}

/// Drops the interior lines across which `f` is linear along the lines' axis for every `other`
/// line (they add triangles and no shape).
fn simplify(ls: &mut Vec<f64>, others: &[f64], f: &dyn Fn(f64, f64) -> f64) {
    let mut i = 1;
    while i + 1 < ls.len() {
        let (a, b, c) = (ls[i - 1], ls[i], ls[i + 1]);
        let k = (b - a) / (c - a);
        let flat = others.iter().all(|&o| {
            let (fa, fb, fc) = (f(a, o), f(b, o), f(c, o));
            (fa + (fc - fa) * k - fb).abs() < 1e-7
        });
        if flat {
            ls.remove(i);
        } else {
            i += 1;
        }
    }
}

/// A side's view of the log's cross-section (cell units): its outer plane (the outline's extreme
/// toward the side), which way is in, and the outline's depth in from that plane at each `u`
/// across the side (0 on the side's flat face, rising over the chamfers).
struct Front {
    j: usize,
    plane: f64,
    inward: f64,
    poly: Vec<[f64; 2]>,
}

impl Front {
    fn of(poly: &[[f64; 2]], s: usize) -> Front {
        let j = s / 2;
        let hi = s % 2 == 1;
        let plane = poly.iter().map(|p| p[j]).fold(
            if hi { f64::NEG_INFINITY } else { f64::INFINITY },
            |a, b| if hi { a.max(b) } else { a.min(b) },
        );
        Front {
            j,
            plane,
            inward: if hi { -1.0 } else { 1.0 },
            poly: poly.to_vec(),
        }
    }

    /// The outline's stretch across the side (`u`, the other axis).
    fn extent(&self) -> [f64; 2] {
        let k = 1 - self.j;
        let lo = self.poly.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
        let hi = self
            .poly
            .iter()
            .map(|p| p[k])
            .fold(f64::NEG_INFINITY, f64::max);
        [lo, hi]
    }

    /// The side's flat face across it: where the outline lies on the plane.
    fn flat(&self) -> [f64; 2] {
        let k = 1 - self.j;
        let on: Vec<f64> = self
            .poly
            .iter()
            .filter(|p| (p[self.j] - self.plane).abs() < 1e-9)
            .map(|p| p[k])
            .collect();
        let lo = on.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = on.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        [lo, hi]
    }

    /// The outline's depth in from the plane at `u` (its nearest boundary there; convex).
    fn depth(&self, u: f64) -> f64 {
        let k = 1 - self.j;
        let n = self.poly.len();
        let mut best = f64::INFINITY;
        for i in 0..n {
            let (a, b) = (self.poly[i], self.poly[(i + 1) % n]);
            let (ua, ub) = (a[k], b[k]);
            if (u < ua.min(ub) - 1e-9) || (u > ua.max(ub) + 1e-9) {
                continue;
            }
            let into = |x: f64| (x - self.plane) * self.inward;
            let here = if (ub - ua).abs() < 1e-12 {
                into(a[self.j]).min(into(b[self.j]))
            } else {
                let k = ((u - ua) / (ub - ua)).clamp(0.0, 1.0);
                into(a[self.j] + (b[self.j] - a[self.j]) * k)
            };
            best = best.min(here);
        }
        if best.is_finite() { best.max(0.0) } else { 0.0 }
    }

    /// The outline's corners across the side (where its depth kinks).
    fn kinks(&self) -> Vec<f64> {
        self.poly.iter().map(|p| p[1 - self.j]).collect()
    }
}

/// The sides a face of the uncut prism looks out of (by its normal's parts across the grain): one
/// for a side's flat face, two for a chamfer, none for an end.
fn facing(grain: usize, f: &Face) -> Vec<usize> {
    let ax = across(grain);
    let n = normal(&f.pts);
    (0..2)
        .filter(|&k| n[ax[k]].abs() > 1e-6)
        .map(|k| 2 * k + usize::from(n[ax[k]] > 0.0))
        .collect()
}

/// The carved faces of a wood cell with grain `grain` whose cut `cut` records its bites (face-local
/// sides), drawn as `look` says, inside the convex cross-section `outline` when given (an
/// octagonal log's; [`super::carve_in`]).
///
/// Each bitten side's front (its flat face and the chamfers it owns: both of its own unless the
/// side round the corner is bitten too and comes first) is redrawn as one grid across the log's
/// whole width (WORK_QUEUE item 26 round 2: the bites span the log, chamfers and a thinner trunk's
/// octagon included; red before: only the flat bark face was carved, ~0.2 m on a thinner trunk):
/// each grid triangle is the bites' surface where it lies inside the wood and the outline's
/// surface (bark) where it doesn't, split along the same line, so the two meet without a gap.
pub(crate) fn carve_bites(
    grain: usize,
    cut: &Cut,
    look: &CutLook,
    outline: Option<&[[f64; 2]]>,
    progress: f64,
) -> Vec<Face> {
    let base = super::carve_in(grain, &Cut::default(), look, outline);
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let poly: Vec<[f64; 2]> = outline
        .filter(|p| p.len() >= 3)
        .map_or(square.to_vec(), <[_]>::to_vec);
    let fronts: [Front; SIDES] = std::array::from_fn(|s| Front::of(&poly, s));
    let bitten = |s: usize| cut.bites[s].n > 0 && (cut.notch[s] > 0.0 || cut.kerf[s] != [0.0; 2]);
    let gridded: [bool; SIDES] = std::array::from_fn(|s| bitten(s) || bitten(s ^ 1));
    // The chamfer between sides `a` and `b` (on different axes) is drawn by the first gridded one.
    let owner = |a: usize, b: usize| -> Option<usize> {
        match (gridded[a], gridded[b]) {
            (true, true) => Some(a.min(b)),
            (true, false) => Some(a),
            (false, true) => Some(b),
            _ => None,
        }
    };
    // Each side's grid range across it: its flat face, out to the outline's ends over the chamfers
    // it owns.
    let range: [[f64; 2]; SIDES] = std::array::from_fn(|s| {
        let fr = &fronts[s];
        let [flo, fhi] = fr.flat();
        let [elo, ehi] = fr.extent();
        let k = 1 - s / 2;
        let lo = if owner(s, 2 * k) == Some(s) { elo } else { flo };
        let hi = if owner(s, 2 * k + 1) == Some(s) {
            ehi
        } else {
            fhi
        };
        [lo, hi]
    });
    let field = Field::of(grain, cut, look, range, &poly, progress);
    let mut out = Vec::new();
    for f in base {
        let drop = match f.part {
            Part::Bark(_) | Part::Inner(_) => match facing(grain, &f)[..] {
                [s] => gridded[s],
                [a, b] => owner(a, b).is_some(),
                _ => false,
            },
            _ => false,
        };
        if !drop {
            out.push(f);
        }
    }
    for s in 0..SIDES {
        if gridded[s] {
            side_grid(&field, &fronts[s], s, range[s], &mut out);
        }
    }
    out.retain(|f| super::area(&f.pts) > 1e-10);
    out
}

/// A grid point: along the grain, across the side, the bites' depth there and the outline's.
#[derive(Clone, Copy)]
struct Gp {
    t: f64,
    u: f64,
    d: f64,
    c: f64,
}

impl Gp {
    fn lerp(a: Gp, b: Gp, k: f64) -> Gp {
        let l = |x: f64, y: f64| x + (y - x) * k;
        Gp {
            t: l(a.t, b.t),
            u: l(a.u, b.u),
            d: l(a.d, b.d),
            c: l(a.c, b.c),
        }
    }
}

/// The part of the convex polygon `pts` where `keep` (linear over it) is ≥ 0.
fn clip_gp(pts: &[Gp], keep: &dyn Fn(&Gp) -> f64) -> Vec<Gp> {
    let mut out = Vec::with_capacity(pts.len() + 2);
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (ka, kb) = (keep(&a), keep(&b));
        if ka >= -1e-12 {
            out.push(a);
        }
        if (ka < -1e-12 && kb > 1e-12) || (ka > 1e-12 && kb < -1e-12) {
            out.push(Gp::lerp(a, b, ka / (ka - kb)));
        }
    }
    out
}

/// Side `s`'s grid over `range` across it (cell units): each quad uncut and on the flat face
/// (`f(quad, true)`, four points), else each of its two triangles (`f(tri, false)`).
fn for_grid_tris(
    field: &Field,
    front: &Front,
    s: usize,
    range: [f64; 2],
    f: &mut dyn FnMut(&[Gp], bool),
) {
    let [u0, u1] = range;
    let mut ts = Vec::new();
    let mut us = Vec::new();
    field.breaks(s, &mut ts, &mut us);
    let mut ts = lines(ts, 0.0, 1.0);
    let mut us = lines(us, u0, u1);
    let d = |t: f64, u: f64| field.depth(s, t, u);
    simplify(&mut us, &ts.clone(), &|u, t| d(t, u));
    simplify(&mut ts, &us.clone(), &d);
    // The outline's corners: its depth is linear between them.
    us.extend(front.kinks());
    let us = lines(us, u0, u1);
    let gp = |t: f64, u: f64| Gp {
        t,
        u,
        d: d(t, u),
        c: front.depth(u),
    };
    for wt in ts.windows(2) {
        for wu in us.windows(2) {
            let q = [
                gp(wt[0], wu[0]),
                gp(wt[1], wu[0]),
                gp(wt[1], wu[1]),
                gp(wt[0], wu[1]),
            ];
            let cut = q.iter().any(|p| p.d > TINY);
            let flat = q.iter().all(|p| p.c <= 1e-9);
            if !cut && flat {
                f(&q, true);
                continue;
            }
            // Split along the diagonal whose ends differ least in depth (the crease follows the V).
            let dd = |a: usize, b: usize| (q[a].d - q[b].d).abs();
            let tris = if dd(0, 2) <= dd(1, 3) {
                [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
            } else {
                [[q[0], q[1], q[3]], [q[1], q[2], q[3]]]
            };
            for tri in tris {
                f(&tri, false);
            }
        }
    }
}

/// Side `s`'s front over `range` across it (cell units) as its grid: the bites' surface inside
/// the wood (fresh wood where cut, bark where not), the outline's surface (bark) where the bites
/// lie outside it.
fn side_grid(field: &Field, front: &Front, s: usize, range: [f64; 2], out: &mut Vec<Face>) {
    let grain = field.grain;
    let ax = across(grain);
    let a_side = ax[s / 2];
    let a_u = ax[1 - s / 2];
    let at = |t: f64, u: f64, depth: f64| {
        let mut p = [0.0; 3];
        p[grain] = t;
        p[a_u] = u;
        p[a_side] = front.plane + front.inward * depth;
        p
    };
    let mut outward = [0.0; 3];
    outward[a_side] = -front.inward;
    let mut push = |mut pts: Vec<[f64; 3]>, part: Part| {
        let n = normal(&pts);
        if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0 {
            pts.reverse();
        }
        out.push(Face { pts, part });
    };
    for_grid_tris(field, front, s, range, &mut |tri, quad| {
        if quad {
            push(
                tri.iter().map(|p| at(p.t, p.u, 0.0)).collect(),
                Part::Bark(s),
            );
            return;
        }
        let flat = tri.iter().all(|p| p.c <= 1e-9);
        // The bites' surface where it is inside the wood (deeper than the outline).
        let inside = clip_gp(tri, &|p| p.d - p.c);
        if inside.len() >= 3 {
            let part = if inside.iter().any(|p| p.d > TINY) {
                Part::Cut(s)
            } else if flat {
                Part::Bark(s)
            } else {
                Part::Inner(s)
            };
            push(inside.iter().map(|p| at(p.t, p.u, p.d)).collect(), part);
        }
        // The outline's surface where the bites don't reach it (off the flat face).
        if !flat {
            let bark = clip_gp(tri, &|p| p.c - p.d);
            if bark.len() >= 3 && bark.iter().any(|p| p.c > 1e-9) {
                push(
                    bark.iter().map(|p| at(p.t, p.u, p.c)).collect(),
                    Part::Inner(s),
                );
            }
        }
    });
}

/// Side `s`'s depth (cells) at `t` along the grain and `u` across, for a cut with bites at
/// `progress` of its swings to fell (tests).
#[cfg(test)]
pub(crate) fn depth_at(
    grain: usize,
    cut: &Cut,
    look: &CutLook,
    progress: f64,
    s: usize,
    t: f64,
    u: f64,
) -> f64 {
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    Field::of(grain, cut, look, [[0.0, 1.0]; SIDES], &square, progress).depth(s, t, u)
}

#[cfg(test)]
#[path = "bites_tests.rs"]
mod tests;
