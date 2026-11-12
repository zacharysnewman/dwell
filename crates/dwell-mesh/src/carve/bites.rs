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
}

impl Field {
    /// The field of `cut` (face-local sides, its bites recorded) for a side face spanning `span`
    /// across (cell units, the same for a side and its opposite: an octagon's flat faces).
    fn of(grain: usize, cut: &Cut, look: &CutLook, span: [[f64; 2]; 2]) -> Field {
        let ax = across(grain);
        let len = size_m(grain);
        let sides = std::array::from_fn(|s| {
            let j = s / 2;
            // The axis across this side (its bites' `u`) and the face's extent on it.
            let other = ax[1 - j];
            let [lo, hi] = span[1 - j];
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
            let hw_n = (0.5 * look.open * notch / len).clamp(0.02, 0.5);
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
                // In the band's half, along the notch's arm (a quarter in when there is none).
                let off = if notch > TINY * len {
                    (0.5 * hw_n).max(0.125)
                } else {
                    0.25
                };
                let c = if b == 0 { 0.5 - off } else { 0.5 + off };
                let d = notch + k;
                let hw = (0.5 * look.open * d / len).clamp(0.02, c.min(1.0 - c));
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
        Field { grain, sides }
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
        let room = (1.0 - MIN_LEFT) * size;
        let k = if a + b > room { room / (a + b) } else { 1.0 };
        a * k / size
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

/// The carved faces of a wood cell with grain `grain` whose cut `cut` records its bites (face-local
/// sides), drawn as `look` says, inside the convex cross-section `outline` when given (an
/// octagonal log's; [`super::carve_in`]).
pub(crate) fn carve_bites(
    grain: usize,
    cut: &Cut,
    look: &CutLook,
    outline: Option<&[[f64; 2]]>,
) -> Vec<Face> {
    let base = super::carve_in(grain, &Cut::default(), look, outline);
    let ax = across(grain);
    // Each side's bark face's extent across (the octagon's flat faces are narrower than the cell).
    let mut span = [[0.0, 1.0]; 2];
    for f in &base {
        if let Part::Bark(s) = f.part {
            let a = ax[1 - s / 2];
            let lo = f.pts.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min);
            let hi = f.pts.iter().map(|p| p[a]).fold(f64::NEG_INFINITY, f64::max);
            span[1 - s / 2] = [lo, hi];
        }
    }
    let field = Field::of(grain, cut, look, span);
    let mut out = Vec::new();
    for f in base {
        match f.part {
            Part::Bark(s) if !field.sides[s].is_empty() || !field.sides[s ^ 1].is_empty() => {
                side_grid(&field, s, &f, &mut out);
            }
            _ => out.push(f),
        }
    }
    out.retain(|f| super::area(&f.pts) > 1e-10);
    out
}

/// Side `s`'s bark face `face` (a rectangle in (t, u) on the side's plane) as its height field.
fn side_grid(field: &Field, s: usize, face: &Face, out: &mut Vec<Face>) {
    let grain = field.grain;
    let ax = across(grain);
    let (j, hi) = (s / 2, s % 2 == 1);
    let a_side = ax[j];
    let a_u = ax[1 - j];
    let plane = face.pts[0][a_side];
    let range = |a: usize| {
        let lo = face.pts.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min);
        let hi = face
            .pts
            .iter()
            .map(|p| p[a])
            .fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };
    let (t0, t1) = range(grain);
    let (u0, u1) = range(a_u);
    let mut ts = Vec::new();
    let mut us = Vec::new();
    field.breaks(s, &mut ts, &mut us);
    let mut ts = lines(ts, t0, t1);
    let mut us = lines(us, u0, u1);
    let d = |t: f64, u: f64| field.depth(s, t, u);
    simplify(&mut us, &ts.clone(), &|u, t| d(t, u));
    simplify(&mut ts, &us.clone(), &d);
    let inward = if hi { -1.0 } else { 1.0 };
    let at = |t: f64, u: f64| {
        let mut p = [0.0; 3];
        p[grain] = t;
        p[a_u] = u;
        p[a_side] = plane + inward * d(t, u);
        p
    };
    let mut outward = [0.0; 3];
    outward[a_side] = -inward;
    let mut push = |mut pts: Vec<[f64; 3]>, part: Part| {
        let n = normal(&pts);
        if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0 {
            pts.reverse();
        }
        out.push(Face { pts, part });
    };
    for wt in ts.windows(2) {
        for wu in us.windows(2) {
            let q = [
                (wt[0], wu[0]),
                (wt[1], wu[0]),
                (wt[1], wu[1]),
                (wt[0], wu[1]),
            ];
            let cut = q.iter().any(|&(t, u)| d(t, u) > TINY);
            let p: Vec<[f64; 3]> = q.iter().map(|&(t, u)| at(t, u)).collect();
            if !cut {
                push(p, Part::Bark(s));
            } else {
                // Split along the diagonal whose ends differ least in depth (the crease follows
                // the V).
                let dd = |a: usize, b: usize| (d(q[a].0, q[a].1) - d(q[b].0, q[b].1)).abs();
                if dd(0, 2) <= dd(1, 3) {
                    push(vec![p[0], p[1], p[2]], Part::Cut(s));
                    push(vec![p[0], p[2], p[3]], Part::Cut(s));
                } else {
                    push(vec![p[0], p[1], p[3]], Part::Cut(s));
                    push(vec![p[1], p[2], p[3]], Part::Cut(s));
                }
            }
        }
    }
}

/// Side `s`'s depth (cells) at `t` along the grain and `u` across, for a cut with bites (tests).
#[cfg(test)]
pub(crate) fn depth_at(grain: usize, cut: &Cut, look: &CutLook, s: usize, t: f64, u: f64) -> f64 {
    Field::of(grain, cut, look, [[0.0, 1.0]; 2]).depth(s, t, u)
}

#[cfg(test)]
#[path = "bites_tests.rs"]
mod tests;
