//! The axe's bites carved into a cut wood cell (WORK_QUEUE item 26; owner, 2026-11-12: the slice
//! "cuts a line all the way across which is much bigger than the tool head. It should align more
//! with the dirt and stone ones").
//!
//! Each hit records where it struck a side ([`dwell_protocol::chopping::Bite`]): a place across the
//! side and the band it cut. A bite is a V along the grain (the notch's or its band's kerf's, as the
//! cut's depths say: opened by the look's `open`) under a mask across the side: 1 over the axe
//! head's width (`bite.widthM`), its two ends sloping to 0 over `bite.endsM`. A side's depth at a
//! point is the deepest of the notch's V under the mask of all its bites and each band's kerf V
//! under the mask of that band's bites, so the notch's cross-section is the union of the bites, and
//! where the hits spread across the side (a hit on a bitten spot lands beside it) they join into
//! the notch. Opposite sides never meet (a waist of [`super::MIN_LEFT`] is left).
//!
//! The geometry: the uncut cell's faces ([`super::carve_in`], clipped to an octagonal log's prism),
//! each bitten side's bark face (a rectangle in (along the grain, across) on the side's plane)
//! replaced by a height field on a tensor grid through the V's and masks' breakpoints, pushed in
//! along the side's inward normal. Every bite's mouth is kept inside the face and every V ends
//! inside the cell, so the face's outline never moves: the other faces and the neighbours meet it
//! as before (watertight; the grid's edge vertices are T-junctions on straight edges, as
//! `damage_mesh`'s carve). Each grid quad is two flat-lit triangles (a hewn face), uncut quads one.

use super::{Face, MIN_LEFT, Part, TINY, across, normal, size_m};
use dwell_protocol::chopping::{BANDS, Cut, CutLook, SIDES};

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
    /// Its cross-section (cell units on the two axes across the grain): a cut stops short of the
    /// far side's outline.
    poly: Vec<[f64; 2]>,
}

/// The notch's band: every V within this (cell units along the grain) of the cell's middle, the
/// band's height (½ the cell: one of its [`BANDS`]).
const NOTCH_HALF: f64 = 0.5 / BANDS as f64;
/// A band's kerf V's apex off the cell's middle: the middle of its half of the notch's band.
const KERF_OFF: f64 = 0.5 * NOTCH_HALF;

impl Field {
    /// The field of `cut` (face-local sides, its bites recorded): side `s`'s bites kept inside
    /// `range[s]` across it (cell units: the stretch of the log its grid covers), the log
    /// `poly` its cross-section (cell units; its waist is left inside it).
    ///
    /// Every V lies in the notch's band, the middle [`NOTCH_HALF`] either side of the cell's
    /// middle along the grain (WORK_QUEUE item 26 round 2: a bite is the band's height, not the
    /// cell's; red before: the notch's V opened `open` × its depth and reached the cell's ends by
    /// the second or third hit, a tall narrow slot); each band's kerf V in its half of it.
    fn of(
        grain: usize,
        cut: &Cut,
        look: &CutLook,
        range: [[f64; 2]; SIDES],
        poly: &[[f64; 2]],
    ) -> Field {
        let thick: [f64; 2] = std::array::from_fn(|j| {
            let lo = poly.iter().map(|p| p[j]).fold(f64::INFINITY, f64::min);
            let hi = poly.iter().map(|p| p[j]).fold(f64::NEG_INFINITY, f64::max);
            hi - lo
        });
        let ax = across(grain);
        let len = size_m(grain);
        let sides = std::array::from_fn(|s| {
            let j = s / 2;
            // The axis across this side (its bites' `u`) and the stretch the bites may cover.
            let other = ax[1 - j];
            let [lo, hi] = range[s];
            let size = size_m(other);
            let w = look.bite.width_m / size;
            let half = (0.5 * w).min(0.5 * (hi - lo)).max(1e-3);
            let ends = (look.bite.ends_m / size).min(half);
            let mask_of = |band: Option<usize>| -> Vec<Mask> {
                cut.bites[s]
                    .all()
                    .iter()
                    .filter(|b| band.is_none_or(|k| usize::from(b.band) == k))
                    .map(|b| Mask {
                        c: f64::from(b.u).clamp(lo + half, (hi - half).max(lo + half)),
                        half,
                        ends,
                    })
                    .collect()
            };
            let notch = f64::from(cut.notch[s]);
            let hw_n = (0.5 * look.open * notch / len).clamp(0.02, NOTCH_HALF);
            let mut parts = Vec::new();
            if notch > TINY * len {
                parts.push(Part2 {
                    c: 0.5,
                    hw: hw_n,
                    d: notch,
                    mask: mask_of(None),
                });
            }
            for b in 0..BANDS {
                let k = f64::from(cut.kerf[s][b]);
                if k <= TINY * len {
                    continue;
                }
                // In the band's half of the notch's band.
                let c = if b == 0 {
                    0.5 - KERF_OFF
                } else {
                    0.5 + KERF_OFF
                };
                let d = notch + k;
                let hw = (0.5 * look.open * d / len).clamp(0.02, NOTCH_HALF - KERF_OFF);
                parts.push(Part2 {
                    c,
                    hw,
                    d,
                    mask: mask_of(Some(b)),
                });
            }
            parts.retain(|p| !p.mask.is_empty());
            parts
        });
        Field {
            grain,
            sides,
            thick,
            poly: poly.to_vec(),
        }
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
        let back = (self.thick[s / 2] - Front::of(&self.poly, s ^ 1).depth(u) - MIN_LEFT) * size;
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
    let field = Field::of(grain, cut, look, range, &poly);
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

/// Side `s`'s front over `range` across it (cell units) as its grid: the bites' surface inside
/// the wood (fresh wood where cut, bark where not), the outline's surface (bark) where the bites
/// lie outside it.
fn side_grid(field: &Field, front: &Front, s: usize, range: [f64; 2], out: &mut Vec<Face>) {
    let grain = field.grain;
    let ax = across(grain);
    let a_side = ax[s / 2];
    let a_u = ax[1 - s / 2];
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
                push(q.iter().map(|p| at(p.t, p.u, 0.0)).collect(), Part::Bark(s));
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
                // The bites' surface where it is inside the wood (deeper than the outline).
                let inside = clip_gp(&tri, &|p| p.d - p.c);
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
                    let bark = clip_gp(&tri, &|p| p.c - p.d);
                    if bark.len() >= 3 && bark.iter().any(|p| p.c > 1e-9) {
                        push(
                            bark.iter().map(|p| at(p.t, p.u, p.c)).collect(),
                            Part::Inner(s),
                        );
                    }
                }
            }
        }
    }
}

/// Side `s`'s depth (cells) at `t` along the grain and `u` across, for a cut with bites (tests).
#[cfg(test)]
pub(crate) fn depth_at(grain: usize, cut: &Cut, look: &CutLook, s: usize, t: f64, u: f64) -> f64 {
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    Field::of(grain, cut, look, [[0.0, 1.0]; SIDES], &square).depth(s, t, u)
}

#[cfg(test)]
#[path = "bites_tests.rs"]
mod tests;
