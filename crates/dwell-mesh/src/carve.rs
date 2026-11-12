//! A cut wood cell's carved geometry (ADR 0067 §11): the cell with each side's notch (a V across
//! the side, its depth and opening from the cut) and each band's kerf past it (a slit) cut in.
//!
//! The notch on a side spans the side's whole width, so at any point `t` along the grain what is
//! left of the cell is a rectangle: the cell's cross-section trimmed by each side's depth there.
//! Between the breakpoints of the four sides' depth profiles (a V's ends and apex, a slit's edges)
//! each depth runs linearly, so each side's face over a span is one planar quad; where a depth jumps
//! (a slit's walls) the two rectangles' difference is a wall square to the grain. The ends are the
//! cell's end-grain faces. Pure geometry in cell units (the cell is the unit cube; depths in metres
//! become cells by the axis's size), face-local: the caller hands in a cut with face-local sides.

use dwell_protocol::chopping::{BANDS, Cut, CutLook, SIDES, side_axis};

mod bites;
pub(crate) use bites::carve_bites;
use dwell_world::CELL_H;

/// What a carved face is, for its look: the uncut bark of side `s` (its outer plane, trimmed),
/// fresh wood cut into side `s`, or the end grain (`+` along the grain, or `−`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Bark(usize),
    /// Bark inside the cell, off its faces (an octagon's chamfer, a narrow log's side; round 2 of
    /// the chopping prototype): drawn as side `s`'s bark, never hidden by a neighbour.
    Inner(usize),
    Cut(usize),
    End(bool),
}

/// One convex face of a carved cell: its points (cell units, counter-clockwise seen from outside)
/// and what it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Face {
    pub pts: Vec<[f64; 3]>,
    pub part: Part,
}

/// The narrowest a cross-section is left (cells): a cut never makes it vanish.
const MIN_LEFT: f64 = 0.02;
/// Depths below this (cells) are uncut.
const TINY: f64 = 1e-4;

/// A cell's size along an axis (m).
fn size_m(axis: usize) -> f64 {
    if axis == 1 { CELL_H } else { 1.0 }
}

/// The two axes across `grain`, in the chopping model's side order (sides 0, 1 on the first).
fn across(grain: usize) -> [usize; 2] {
    match grain {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// A side's depth profile along the grain: a V (centre, half-width, depth; `t` units and metres)
/// and slits (centre, half-width, depth).
#[derive(Clone, Copy, Debug, Default)]
struct Profile {
    v: Option<(f64, f64, f64)>,
    slits: [Option<(f64, f64, f64)>; BANDS],
}

impl Profile {
    fn of(cut: &Cut, side: usize, grain: usize, look: &CutLook) -> Profile {
        let len = size_m(grain);
        let notch = f64::from(cut.notch[side]);
        let mut p = Profile::default();
        let hw = if notch > TINY * len {
            (0.5 * look.open * notch / len).clamp(0.02, 0.5)
        } else {
            0.0
        };
        if hw > 0.0 {
            p.v = Some((0.5, hw, notch));
        }
        let w = (0.5 * look.kerf_width_m / len).clamp(0.002, 0.1);
        for b in 0..BANDS {
            let k = f64::from(cut.kerf[side][b]);
            if k <= TINY {
                continue;
            }
            // In the band's half, along the V's arm (half way out), or a quarter in when there is
            // no notch yet.
            let off = (0.5 * hw).max(0.125);
            let c = if b == 0 { 0.5 - off } else { 0.5 + off };
            let c = c.clamp(w + 0.01, 1.0 - w - 0.01);
            p.slits[b] = Some((c, w, notch + k));
        }
        p
    }

    fn breaks(&self, out: &mut Vec<f64>) {
        if let Some((c, hw, _)) = self.v {
            out.extend([c - hw, c, c + hw]);
        }
        for (c, w, _) in self.slits.iter().flatten() {
            out.extend([c - w, c + w]);
        }
    }

    /// The depth (m) at `t` in the span whose middle is `mid` (a slit's depth is flat across it).
    fn depth(&self, t: f64, mid: f64) -> f64 {
        let slit = self
            .slits
            .iter()
            .flatten()
            .filter(|(c, w, _)| (mid - c).abs() < *w)
            .map(|s| s.2)
            .fold(0.0, f64::max);
        let v = self
            .v
            .map_or(0.0, |(c, hw, d)| d * (1.0 - (t - c).abs() / hw).max(0.0));
        v.max(slit)
    }
}

/// The cross-section left at `t` (in the span whose middle is `mid`): [lo, hi] on each axis across
/// the grain (cells).
type Rect = [[f64; 2]; 2];

fn rect(profiles: &[Profile; SIDES], grain: usize, t: f64, mid: f64) -> Rect {
    let ax = across(grain);
    std::array::from_fn(|j| {
        let s = size_m(ax[j]);
        let mut lo = profiles[2 * j].depth(t, mid) / s;
        let mut hi = profiles[2 * j + 1].depth(t, mid) / s;
        if lo + hi > 1.0 - MIN_LEFT {
            let k = (1.0 - MIN_LEFT) / (lo + hi);
            lo *= k;
            hi *= k;
        }
        [lo, 1.0 - hi]
    })
}

/// The pieces of `a` outside `b`, each tagged with the side of `a` it is cut from (the strip on
/// the first axis's low side is side 0, …).
fn difference(a: &Rect, b: &Rect) -> Vec<(Rect, usize)> {
    let i0 = [a[0][0].max(b[0][0]), a[0][1].min(b[0][1])];
    let i1 = [a[1][0].max(b[1][0]), a[1][1].min(b[1][1])];
    if i0[0] >= i0[1] || i1[0] >= i1[1] {
        return vec![(*a, 0)];
    }
    let mut out = Vec::new();
    let e = 1e-9;
    if i0[0] - a[0][0] > e {
        out.push(([[a[0][0], i0[0]], a[1]], 0));
    }
    if a[0][1] - i0[1] > e {
        out.push(([[i0[1], a[0][1]], a[1]], 1));
    }
    if i1[0] - a[1][0] > e {
        out.push(([i0, [a[1][0], i1[0]]], 2));
    }
    if a[1][1] - i1[1] > e {
        out.push(([i0, [i1[1], a[1][1]]], 3));
    }
    out
}

/// [`carve_in`] of the whole cell (no cross-section outline).
#[cfg(test)]
pub(crate) fn carve(grain: usize, cut: &Cut, look: &CutLook) -> Vec<Face> {
    carve_in(grain, cut, look, None)
}

/// The carved faces of a cell with grain `grain` (face-local axis) cut `cut` (face-local sides),
/// drawn as `look` says, inside the convex cross-section `outline` (points (a, b) on the two axes
/// across the grain in [`across`]'s order, cell units; an octagonal log's, ADR 0072) when given:
/// the carved cell clipped to the prism, with the prism's own sides (the chamfers) as bark where
/// wood is left (round 2 of the chopping prototype: "the notch carved octagon-aware"). Uncut: the
/// cube's six faces (clipped to the prism).
pub(crate) fn carve_in(
    grain: usize,
    cut: &Cut,
    look: &CutLook,
    outline: Option<&[[f64; 2]]>,
) -> Vec<Face> {
    let profiles: [Profile; SIDES] = std::array::from_fn(|s| Profile::of(cut, s, grain, look));
    let mut ts = vec![0.0, 1.0];
    for p in &profiles {
        p.breaks(&mut ts);
    }
    for t in &mut ts {
        *t = t.clamp(0.0, 1.0);
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    let ax = across(grain);
    let at = |t: f64, u: f64, v: f64| {
        let mut p = [0.0; 3];
        p[grain] = t;
        p[ax[0]] = u;
        p[ax[1]] = v;
        p
    };
    let mut out = Vec::new();
    let mut push = |mut pts: Vec<[f64; 3]>, outward: [f64; 3], part: Part| {
        let n = normal(&pts);
        if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0 {
            pts.reverse();
        }
        out.push(Face { pts, part });
    };
    let dir = |axis: usize, s: f64| {
        let mut d = [0.0; 3];
        d[axis] = s;
        d
    };
    let spans: Vec<(f64, f64)> = ts.windows(2).map(|w| (w[0], w[1])).collect();
    for &(t0, t1) in &spans {
        let mid = 0.5 * (t0 + t1);
        let r0 = rect(&profiles, grain, t0, mid);
        let r1 = rect(&profiles, grain, t1, mid);
        for (s, profile) in profiles.iter().enumerate() {
            let j = s / 2;
            let k = 1 - j;
            let hi = s % 2;
            let d0 = profile.depth(t0, mid);
            let d1 = profile.depth(t1, mid);
            let part = if d0 <= TINY && d1 <= TINY {
                Part::Bark(s)
            } else {
                Part::Cut(s)
            };
            // The side's plane at each end, the other axis's span along it.
            let pt = |t: f64, r: &Rect, other: f64| {
                let mut uv = [0.0; 2];
                uv[j] = r[j][hi];
                uv[k] = other;
                at(t, uv[0], uv[1])
            };
            let pts = vec![
                pt(t0, &r0, r0[k][0]),
                pt(t0, &r0, r0[k][1]),
                pt(t1, &r1, r1[k][1]),
                pt(t1, &r1, r1[k][0]),
            ];
            let outward = dir(ax[j], if hi == 1 { 1.0 } else { -1.0 });
            push(pts, outward, part);
        }
    }
    // The walls where a depth jumps, and the two ends.
    let square = |t: f64, r: &Rect| {
        vec![
            at(t, r[0][0], r[1][0]),
            at(t, r[0][1], r[1][0]),
            at(t, r[0][1], r[1][1]),
            at(t, r[0][0], r[1][1]),
        ]
    };
    for (i, &(t0, t1)) in spans.iter().enumerate() {
        let here = rect(&profiles, grain, t0, 0.5 * (t0 + t1));
        if i == 0 {
            push(square(t0, &here), dir(grain, -1.0), Part::End(false));
        } else {
            let (p0, p1) = spans[i - 1];
            let before = rect(&profiles, grain, t0, 0.5 * (p0 + p1));
            for (r, s) in difference(&before, &here) {
                push(square(t0, &r), dir(grain, 1.0), Part::Cut(s));
            }
            for (r, s) in difference(&here, &before) {
                push(square(t0, &r), dir(grain, -1.0), Part::Cut(s));
            }
        }
        if i == spans.len() - 1 {
            let end = rect(&profiles, grain, t1, 0.5 * (t0 + t1));
            push(square(t1, &end), dir(grain, 1.0), Part::End(true));
        }
    }
    if let Some(poly) = outline.filter(|p| p.len() >= 3) {
        out = clip_to_prism(out, &profiles, grain, &spans, poly);
    }
    // Faces with no area (a span's side squeezed to a line) are dropped.
    out.retain(|f| area(&f.pts) > 1e-10);
    out
}

/// The carved faces clipped to the prism of the convex cross-section `poly`, and the prism's sides
/// inside the cell (off its faces) added as bark where wood is left at each point along the grain.
fn clip_to_prism(
    faces: Vec<Face>,
    profiles: &[Profile; SIDES],
    grain: usize,
    spans: &[(f64, f64)],
    poly: &[[f64; 2]],
) -> Vec<Face> {
    let ax = across(grain);
    let n = poly.len() as f64;
    let centre = poly
        .iter()
        .fold([0.0; 2], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n]);
    let uv = |p: &[f64; 3]| [p[ax[0]], p[ax[1]]];
    let at = |t: f64, w: [f64; 2]| {
        let mut p = [0.0; 3];
        p[grain] = t;
        p[ax[0]] = w[0];
        p[ax[1]] = w[1];
        p
    };
    // Each edge: its start, direction and outward normal (away from the centre).
    let edges: Vec<([f64; 2], [f64; 2], [f64; 2])> = (0..poly.len())
        .map(|i| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            let d = [q[0] - p[0], q[1] - p[1]];
            let mut o = [d[1], -d[0]];
            if o[0] * (centre[0] - p[0]) + o[1] * (centre[1] - p[1]) > 0.0 {
                o = [-o[0], -o[1]];
            }
            (p, d, o)
        })
        .collect();
    let mut out = Vec::new();
    for f in faces {
        let mut pts = f.pts;
        for (p, _, o) in &edges {
            let side = |x: &[f64; 3]| {
                let w = uv(x);
                o[0] * (w[0] - p[0]) + o[1] * (w[1] - p[1])
            };
            pts = clip(&pts, &side);
            if pts.len() < 3 {
                break;
            }
        }
        if pts.len() >= 3 {
            out.push(Face { pts, part: f.part });
        }
    }
    // The prism's sides off the cell's faces: per span, the edge's stretch inside what is left.
    let on_cell_face = |a: [f64; 2], b: [f64; 2]| {
        (0..2).any(|k| (a[k] - b[k]).abs() < 1e-9 && (a[k] < 1e-9 || a[k] > 1.0 - 1e-9))
    };
    for (i, (p, d, o)) in edges.iter().enumerate() {
        if on_cell_face(poly[i], poly[(i + 1) % poly.len()]) {
            continue;
        }
        // Bark of the side it mostly faces.
        let k = usize::from(o[1].abs() > o[0].abs());
        let s = 2 * k + usize::from(o[k] > 0.0);
        let mut outward = [0.0; 3];
        outward[ax[0]] = o[0];
        outward[ax[1]] = o[1];
        for &(t0, t1) in spans {
            let mid = 0.5 * (t0 + t1);
            // The edge's parameter range left at t: [lo, hi], or none.
            let range = |t: f64| -> Option<(f64, f64)> {
                let r = rect(profiles, grain, t, mid);
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for a in 0..2 {
                    if d[a].abs() < 1e-12 {
                        if p[a] < r[a][0] - 1e-9 || p[a] > r[a][1] + 1e-9 {
                            return None;
                        }
                        continue;
                    }
                    let (x, y) = ((r[a][0] - p[a]) / d[a], (r[a][1] - p[a]) / d[a]);
                    lo = lo.max(x.min(y));
                    hi = hi.min(x.max(y));
                }
                (hi >= lo - 1e-9).then_some((lo, hi.max(lo)))
            };
            // Where the range's bounds change which constraint holds: sample finely enough that
            // each piece is one linear stretch (the bounds are linear in t between kinks).
            let mut ts = vec![t0, t1];
            const STEPS: usize = 16;
            for j in 1..STEPS {
                ts.push(t0 + (t1 - t0) * j as f64 / STEPS as f64);
            }
            ts.sort_by(f64::total_cmp);
            for w in ts.windows(2) {
                let (ta, tb) = (w[0], w[1]);
                let (Some(a), Some(b)) = (range(ta), range(tb)) else {
                    continue;
                };
                let pt = |t: f64, s: f64| at(t, [p[0] + d[0] * s, p[1] + d[1] * s]);
                let mut pts = vec![pt(ta, a.0), pt(ta, a.1), pt(tb, b.1), pt(tb, b.0)];
                if normal(&pts)
                    .iter()
                    .zip(outward)
                    .map(|(x, y)| x * y)
                    .sum::<f64>()
                    < 0.0
                {
                    pts.reverse();
                }
                out.push(Face {
                    pts,
                    part: Part::Inner(s),
                });
            }
        }
    }
    out
}

/// A convex polygon's part where `side` ≤ 0 (Sutherland–Hodgman), its winding kept.
fn clip(pts: &[[f64; 3]], side: &dyn Fn(&[f64; 3]) -> f64) -> Vec<[f64; 3]> {
    let mut out = Vec::with_capacity(pts.len() + 2);
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let (sa, sb) = (side(&a), side(&b));
        if sa <= 1e-12 {
            out.push(a);
        }
        if (sa < -1e-12 && sb > 1e-12) || (sa > 1e-12 && sb < -1e-12) {
            let k = sa / (sa - sb);
            out.push([
                a[0] + (b[0] - a[0]) * k,
                a[1] + (b[1] - a[1]) * k,
                a[2] + (b[2] - a[2]) * k,
            ]);
        }
    }
    out
}

/// The cut sides as a face-B chunk's mirrored mesh sees them: a side whose depth runs along y
/// swaps with its opposite (bands stay: they are face-local already).
pub(crate) fn mirror_cut(grain: usize, cut: &Cut) -> Cut {
    let mut out = *cut;
    for s in 0..SIDES {
        if side_axis(grain, s) == 1 {
            out.notch[s] = cut.notch[s ^ 1];
            out.kerf[s] = cut.kerf[s ^ 1];
            out.bites[s] = cut.bites[s ^ 1];
        }
    }
    out
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A polygon's area vector (Newell), twice its area along its normal.
fn area_vector(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 1..pts.len().saturating_sub(1) {
        let c = cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[0]));
        n = [n[0] + c[0], n[1] + c[1], n[2] + c[2]];
    }
    n
}

pub(crate) fn area(pts: &[[f64; 3]]) -> f64 {
    let n = area_vector(pts);
    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// A polygon's unit normal (cell units), from all its points (robust to a collapsed corner).
pub(crate) fn normal(pts: &[[f64; 3]]) -> [f64; 3] {
    let n = area_vector(pts);
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [n[0] / l, n[1] / l, n[2] / l]
    }
}

#[cfg(test)]
#[path = "carve_tests.rs"]
mod tests;
