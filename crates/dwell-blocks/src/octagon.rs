//! Octagonal cross-sections (ADR 0072): a block with `crossSection` (logs) is a prism along its axis
//! whose cross-section is an octagon inscribed in the cell, its corners cut by `chamfer` (a fraction
//! of the cross-section's shorter side, at 45° in metres). A side across the axis on which a
//! neighbour of the same join group and axis lies is *joined*: both its corners square off to the
//! cell's edge, so neighbouring logs meet flat, full width. The joins are read from the neighbours
//! where the geometry is built (mesher, collision), never stored in the state.
//!
//! Geometry is in cell coordinates ([0, 1]³, y in cells); `size_m` gives the cell's size in metres
//! so the chamfer is square in metres on ½ m cells. The shape is symmetric under y → −y, so face B
//! (its chunks meshed and collided mirrored) needs no case of its own.

use crate::{Axis, CrossSection, StateId};

/// The axis index (0 x, 1 y, 2 z) of a block's `axis`.
pub fn axis_index(a: Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// The face index (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z) of an axis and sign.
pub fn face_index(axis: usize, positive: bool) -> usize {
    2 * axis + usize::from(!positive)
}

/// The unit offset of face `face`.
pub fn face_offset(face: usize) -> [i32; 3] {
    let mut d = [0; 3];
    d[face / 2] = if face.is_multiple_of(2) { 1 } else { -1 };
    d
}

/// Its octagonal cross-section and axis, if state `m` has one. A shaped state (a wood ramp, Phase
/// 55 T4) has one only when its shape is convex: the prism cut by the shape's planes is then the
/// two's intersection; a non-convex one (a ramp's inner corner) stays its square shape.
pub fn section_of(m: StateId) -> Option<(CrossSection, usize)> {
    crate::try_info(m)
        .and_then(|s| s.cross_section.map(|c| (c, axis_index(s.axis))))
        .filter(|_| crate::shape::shape_of(m).convex)
}

/// Whether state `m`'s shape is the full cube (a log), not a cut one (a wood ramp).
pub fn full(m: StateId) -> bool {
    let s = crate::shape::shape_of(m);
    s.corners == [2, 2, 2, 2] && s.min_y == 0.0 && s.max_y == 1.0
}

/// Whether two states join (a full-width side between them): both octagonal, of one join group,
/// along one axis, and of one shape (a log joins logs; a wood ramp only the same ramp, whose side
/// it matches).
pub fn joins(a: StateId, b: StateId) -> bool {
    match (section_of(a), section_of(b)) {
        (Some((ca, aa)), Some((cb, ab))) => {
            aa == ab
                && ca.joins == cb.joins
                && ca.width >= 1.0
                && cb.width >= 1.0
                && crate::info(a).shape_index == crate::info(b).shape_index
        }
        _ => false,
    }
}

/// Whether `b` continues `a` along their axis (one join group and axis, any shape): the caps
/// between them may meet.
fn continues(a: StateId, b: StateId) -> bool {
    match (section_of(a), section_of(b)) {
        (Some((ca, aa)), Some((cb, ab))) => aa == ab && ca.joins == cb.joins,
        _ => false,
    }
}

/// The sides of the octagonal cell `m` that are joined, as bits by face index (only faces across
/// its axis are ever set), reading the states round it with `at` (offsets from the cell).
pub fn joined_mask(m: StateId, at: &dyn Fn([i32; 3]) -> StateId) -> u8 {
    let Some((_, axis)) = section_of(m) else {
        return 0;
    };
    // A lying log (axis x or z) is two ½ m cells, its lower and upper half, each knowing which by
    // the log cells it joins above and below. Beside it (across a level side), a cell joins only
    // the same half: a log lying one row higher or lower is a separate octagon, not its upper half
    // squared off against the other's lower one (BUGS.md #63).
    let half = |o: [i32; 3]| {
        let here = at(o);
        let up = joins(here, at([o[0], o[1] + 1, o[2]]));
        let down = joins(here, at([o[0], o[1] - 1, o[2]]));
        (up, down)
    };
    let mut mask = 0;
    for face in 0..6 {
        let d = face_offset(face);
        if face / 2 != axis
            && joins(m, at(d))
            && (axis == 1 || face / 2 == 1 || half([0; 3]) == half(d))
        {
            mask |= 1 << face;
        }
    }
    mask
}

/// Which part of the prism a face is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// An end cap (end grain), at the positive (`true`) or negative end of the axis; on face
    /// `face_index(axis, end)`.
    Cap(bool),
    /// A flat side on the cell face `face` (bark): full width where joined.
    Side(usize),
    /// A 45° chamfer between the cell faces `a` and `b` (bark).
    Chamfer(usize, usize),
    /// A face of a shaped state's own (a wood ramp's slant, Phase 55 T4) clipped to the prism: the
    /// cut, end grain like a cap.
    Cut,
    /// A piece of an end cap's bark ring (`CrossSection::ring`, from [`cap_pieces`]), at the
    /// positive or negative end of the axis, along the outline's edge on cell face `Some(face)` or
    /// along a chamfer (`None`).
    Ring(bool, Option<usize>),
}

/// One convex, outward-wound polygon of the prism, in cell coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub part: Part,
    pub pts: Vec<[f64; 3]>,
}

/// A corner of the outline: its point, the side arriving at it, the side leaving it, and the
/// chamfer's two points that replace it when cut.
type Corner = ([f64; 2], usize, usize, [[f64; 2]; 2]);

/// The cross-section's outline: counter-clockwise points (u, v) round the axis (u = axis+1,
/// v = axis+2, so u × v points along the axis), each with the cell face its edge to the next lies
/// on (or `None`: a chamfer).
pub fn outline(
    chamfer: f64,
    axis: usize,
    joined: u8,
    size_m: [f64; 3],
) -> Vec<([f64; 2], Option<usize>)> {
    outline_width(chamfer, 1.0, axis, joined, size_m)
}

/// [`outline`] of a cross-section `width` of the cell across (ADR 0079): along y, a narrower
/// octagon centred in the cell (its sides then touch no cell face); lying logs stay full width.
pub fn outline_width(
    chamfer: f64,
    width: f64,
    axis: usize,
    joined: u8,
    size_m: [f64; 3],
) -> Vec<([f64; 2], Option<usize>)> {
    let ring = outline_full(chamfer, axis, joined, size_m);
    if axis != 1 || width >= 1.0 {
        return ring;
    }
    let k = width.clamp(0.05, 1.0);
    ring.into_iter()
        .map(|(p, side)| ([0.5 + (p[0] - 0.5) * k, 0.5 + (p[1] - 0.5) * k], side))
        .collect()
}

/// The log's width across its axis in metres, which `chamfer` and `ring` are fractions of: the
/// cell's shorter side across, but for half of a lying log (axis x or z, joined above or below to
/// its other ½ m half) the whole 1 m log, so a felled log's octagon is the standing trunk's
/// (BUGS.md #168: it was cut by a fraction of the ½ m cell, half as deep).
pub fn across_m(axis: usize, joined: u8, size_m: [f64; 3]) -> f64 {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (short, long) = if size_m[u] <= size_m[v] {
        (u, v)
    } else {
        (v, u)
    };
    let halved = joined & ((1 << face_index(short, true)) | (1 << face_index(short, false))) != 0;
    if halved {
        size_m[long].min(2.0 * size_m[short])
    } else {
        size_m[short]
    }
}

fn outline_full(
    chamfer: f64,
    axis: usize,
    joined: u8,
    size_m: [f64; 3],
) -> Vec<([f64; 2], Option<usize>)> {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let c_m = chamfer * across_m(axis, joined, size_m);
    let (cu, cv) = ((c_m / size_m[u]).min(1.0), (c_m / size_m[v]).min(1.0));
    let j = |face: usize| joined & (1 << face) != 0;
    let (pu, nu, pv, nv) = (
        face_index(u, true),
        face_index(u, false),
        face_index(v, true),
        face_index(v, false),
    );
    // Corners CCW: (0,0) between −u and −v, (1,0) −v and +u, (1,1) +u and +v, (0,1) +v and −u.
    // Each: (corner, the side arriving at it, the side leaving it, the chamfer's two points).
    let corners: [Corner; 4] = [
        ([0.0, 0.0], nu, nv, [[0.0, cv], [cu, 0.0]]),
        ([1.0, 0.0], nv, pu, [[1.0 - cu, 0.0], [1.0, cv]]),
        ([1.0, 1.0], pu, pv, [[1.0, 1.0 - cv], [1.0 - cu, 1.0]]),
        ([0.0, 1.0], pv, nu, [[cu, 1.0], [0.0, 1.0 - cv]]),
    ];
    let mut out = Vec::with_capacity(8);
    for (corner, arrive, leave, cut) in corners {
        if j(arrive) || j(leave) {
            out.push((corner, Some(leave)));
        } else {
            out.push((cut[0], None));
            out.push((cut[1], Some(leave)));
        }
    }
    out
}

/// The prism's faces: the two caps, a side on each cell face across the axis, and a chamfer at
/// each corner neither of whose sides is joined. Culling against neighbours is the caller's.
pub fn faces(section: &CrossSection, axis: usize, joined: u8, size_m: [f64; 3]) -> Vec<Face> {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let ring = outline_width(
        f64::from(section.chamfer),
        f64::from(section.width),
        axis,
        joined,
        size_m,
    );
    let at = |p: [f64; 2], w: f64| {
        let mut q = [0.0; 3];
        q[u] = p[0];
        q[v] = p[1];
        q[axis] = w;
        q
    };
    let mut out = Vec::with_capacity(10);
    out.push(Face {
        part: Part::Cap(true),
        pts: ring.iter().map(|(p, _)| at(*p, 1.0)).collect(),
    });
    out.push(Face {
        part: Part::Cap(false),
        pts: ring.iter().rev().map(|(p, _)| at(*p, 0.0)).collect(),
    });
    for i in 0..ring.len() {
        let (a, side) = ring[i];
        let b = ring[(i + 1) % ring.len()].0;
        let part = match side {
            Some(f) => Part::Side(f),
            None => {
                // The faces either side: the previous point's side and this edge's next.
                let prev = ring[(i + ring.len() - 1) % ring.len()].1.unwrap_or(0);
                let next = ring[(i + 1) % ring.len()].1.unwrap_or(0);
                Part::Chamfer(prev, next)
            }
        };
        out.push(Face {
            part,
            pts: vec![at(a, 0.0), at(b, 0.0), at(b, 1.0), at(a, 1.0)],
        });
    }
    out
}

/// The faces of octagonal cell `m` that nothing covers: a joined side is shared (neither cell draws
/// it), a side or cap against a neighbour that `covers` (a plain full cube) is hidden, and a cap
/// against a joined log along the axis whose outline holds this one's is hidden. `at` reads the
/// states round the cell (offsets up to one cell on two axes).
pub fn open_faces(
    m: StateId,
    at: &dyn Fn([i32; 3]) -> StateId,
    covers: &dyn Fn(StateId) -> bool,
    size_m: [f64; 3],
) -> Vec<Face> {
    let Some((section, axis)) = section_of(m) else {
        return Vec::new();
    };
    let joined = joined_mask(m, at);
    let mut out = state_faces(m, joined, size_m);
    out.retain(|f| match f.part {
        Part::Side(face) => {
            // A narrow standing log's sides are inside its cell: no neighbour covers them.
            if axis == 1 && section.width < 1.0 {
                return true;
            }
            let n = at(face_offset(face));
            joined & (1 << face) == 0 && !(covers(n) && section_of(n).is_none())
        }
        Part::Cap(end) => {
            let d = face_offset(face_index(axis, end));
            let n = at(d);
            if section_of(n).is_none() {
                return !covers(n);
            }
            if !continues(m, n) {
                return true;
            }
            // The neighbour's squared corners must include this one's: its joined sides hold ours;
            // and its cap toward this one must be whole (a wood ramp's cut end covers nothing).
            let theirs = joined_mask(n, &|o| at([o[0] + d[0], o[1] + d[1], o[2] + d[2]]));
            let same = section_of(n).map(|(c, _)| c) == Some(section);
            !(same && theirs & joined == joined && cap_whole(n, theirs, size_m, !end))
        }
        Part::Chamfer(..) | Part::Ring(..) | Part::Cut => true,
    });
    out
}

/// The faces of octagonal state `m` with sides `joined`: [`faces`], and for a shaped state (a wood
/// ramp) the prism cut by its convex shape: each prism face clipped to the shape, and each of the
/// shape's faces that no prism face lies on clipped to the prism (`Part::Cut`). Empty if `m` has
/// no cross-section.
pub fn state_faces(m: StateId, joined: u8, size_m: [f64; 3]) -> Vec<Face> {
    let Some((section, axis)) = section_of(m) else {
        return Vec::new();
    };
    let prism = faces(&section, axis, joined, size_m);
    if full(m) {
        return prism;
    }
    let shape: Vec<Vec<[f64; 3]>> = crate::shape::faces_of(crate::shape::shape_of(m))
        .iter()
        .map(|f| {
            f.v[..usize::from(f.count)]
                .iter()
                .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
                .collect()
        })
        .collect();
    let prism_pts: Vec<Vec<[f64; 3]>> = prism.iter().map(|f| f.pts.clone()).collect();
    let (pp, sp) = (planes(&prism_pts), planes(&shape));
    let mut out = Vec::with_capacity(prism.len() + 2);
    for f in prism {
        let pts = clip(f.pts, &sp);
        if area(&pts) > 1e-9 {
            out.push(Face { part: f.part, pts });
        }
    }
    for (pts, plane) in shape.into_iter().zip(&sp) {
        // A shape face lying on a prism face is that face (already clipped above).
        let on = pp
            .iter()
            .any(|q| dot(q.0, plane.0) > 1.0 - 1e-6 && (q.1 - plane.1).abs() < 1e-6);
        if on {
            continue;
        }
        let pts = clip(pts, &pp);
        if area(&pts) > 1e-9 {
            // Wound outward (its normal along its plane's).
            let pts = if dot(area_vector(&pts), plane.0) < 0.0 {
                pts.into_iter().rev().collect()
            } else {
                pts
            };
            out.push(Face {
                part: Part::Cut,
                pts,
            });
        }
    }
    out
}

/// Whether state `n` (joined `joined`) has its whole cap at end `end` (a log: yes; a wood ramp: the
/// end its slant doesn't cut into).
fn cap_whole(n: StateId, joined: u8, size_m: [f64; 3], end: bool) -> bool {
    if full(n) {
        return true;
    }
    let Some((section, axis)) = section_of(n) else {
        return false;
    };
    let whole = |fs: &[Face]| {
        fs.iter()
            .filter(|f| f.part == Part::Cap(end))
            .map(|f| area(&f.pts))
            .sum::<f64>()
    };
    let want = whole(&faces(&section, axis, joined, size_m));
    (whole(&state_faces(n, joined, size_m)) - want).abs() < 1e-6
}

/// Each convex polygon's outward plane (unit normal, offset), outward from the polygons' centre.
fn planes(polys: &[Vec<[f64; 3]>]) -> Vec<([f64; 3], f64)> {
    let all: Vec<[f64; 3]> = polys.iter().flatten().copied().collect();
    let n = all.len().max(1) as f64;
    let c = all.iter().fold([0.0; 3], |a, p| {
        [a[0] + p[0] / n, a[1] + p[1] / n, a[2] + p[2] / n]
    });
    polys
        .iter()
        .map(|p| {
            let v = area_vector(p);
            let len = dot(v, v).sqrt().max(1e-12);
            let mut nrm = [v[0] / len, v[1] / len, v[2] / len];
            let mut d = dot(nrm, p[0]);
            if dot(nrm, c) > d {
                nrm = [-nrm[0], -nrm[1], -nrm[2]];
                d = -d;
            }
            (nrm, d)
        })
        .collect()
}

/// `pts` clipped to the inside of every plane (Sutherland–Hodgman).
fn clip(mut pts: Vec<[f64; 3]>, planes: &[([f64; 3], f64)]) -> Vec<[f64; 3]> {
    for &(nrm, d) in planes {
        if pts.is_empty() {
            break;
        }
        let side = |p: [f64; 3]| dot(nrm, p) - d;
        let mut next = Vec::with_capacity(pts.len() + 1);
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let (sa, sb) = (side(a), side(b));
            if sa <= 1e-9 {
                next.push(a);
            }
            if (sa > 1e-9) != (sb > 1e-9) {
                let t = sa / (sa - sb);
                next.push([
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ]);
            }
        }
        next.dedup_by(|a, b| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9));
        if next.len() > 1 && (0..3).all(|k| (next[0][k] - next[next.len() - 1][k]).abs() < 1e-9) {
            next.pop();
        }
        pts = next;
    }
    if pts.len() < 3 { Vec::new() } else { pts }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Twice a polygon's area along its normal (Newell).
fn area_vector(p: &[[f64; 3]]) -> [f64; 3] {
    let mut v = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        v[0] += (a[1] - b[1]) * (a[2] + b[2]);
        v[1] += (a[2] - b[2]) * (a[0] + b[0]);
        v[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    v
}

/// A polygon's area.
fn area(p: &[[f64; 3]]) -> f64 {
    if p.len() < 3 {
        return 0.0;
    }
    let v = area_vector(p);
    dot(v, v).sqrt() / 2.0
}

/// An end cap split into its bark ring and the heart inside it (Phase 55 T3): the ring is a band
/// `ring` wide (a fraction of the shorter side, in metres so it is even on ½ m cells) along each
/// edge of the outline that is not joined, so it follows the octagon; along a joined side there is
/// none, and the heart reaches the cell's edge there, so joined logs' caps read as one surface. The
/// heart is `Part::Cap(end)`, the bands `Part::Ring(end, _)`; together they tile the cap. With no
/// ring (0) the cap is returned whole.
pub fn cap_pieces(
    section: &CrossSection,
    axis: usize,
    joined: u8,
    size_m: [f64; 3],
    end: bool,
) -> Vec<Face> {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let ring = outline_width(
        f64::from(section.chamfer),
        f64::from(section.width),
        axis,
        joined,
        size_m,
    );
    let w = if end { 1.0 } else { 0.0 };
    let at = |p: [f64; 2]| {
        let mut q = [0.0; 3];
        q[u] = p[0];
        q[v] = p[1];
        q[axis] = w;
        q
    };
    // Counter-clockwise round the axis at the positive end; reversed (outward) at the negative.
    let wind = |mut pts: Vec<[f64; 3]>| {
        if !end {
            pts.reverse();
        }
        pts
    };
    let width = f64::from(section.ring) * across_m(axis, joined, size_m);
    if width <= 0.0 {
        return vec![Face {
            part: Part::Cap(end),
            pts: wind(ring.iter().map(|(p, _)| at(*p)).collect()),
        }];
    }
    // In metres: each edge's line moved inward by its band's width (0 where joined); the heart's
    // corners are where consecutive moved lines meet.
    let (su, sv) = (size_m[u], size_m[v]);
    let n = ring.len();
    let m: Vec<[f64; 2]> = ring.iter().map(|(p, _)| [p[0] * su, p[1] * sv]).collect();
    let banded = |i: usize| match ring[i].1 {
        Some(face) => joined & (1 << face) == 0,
        None => true,
    };
    let line = |i: usize| {
        let (a, b) = (m[i], m[(i + 1) % n]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let len = (e[0] * e[0] + e[1] * e[1]).sqrt();
        let d = if banded(i) { width } else { 0.0 };
        // The inward normal of a counter-clockwise edge: its direction turned left.
        let o = [a[0] - e[1] / len * d, a[1] + e[0] / len * d];
        (o, e)
    };
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let inner: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let (a, ea) = line((i + n - 1) % n);
            let (b, eb) = line(i);
            let t = cross([b[0] - a[0], b[1] - a[1]], eb) / cross(ea, eb);
            let p = [a[0] + ea[0] * t, a[1] + ea[1] * t];
            [p[0] / su, p[1] / sv]
        })
        .collect();
    let mut out = vec![Face {
        part: Part::Cap(end),
        pts: wind(inner.iter().map(|p| at(*p)).collect()),
    }];
    for i in (0..n).filter(|&i| banded(i)) {
        let j = (i + 1) % n;
        out.push(Face {
            part: Part::Ring(end, ring[i].1),
            pts: wind(vec![
                at(ring[i].0),
                at(ring[j].0),
                at(inner[j]),
                at(inner[i]),
            ]),
        });
    }
    out
}

/// Whether `m` is drawn and collided as an octagonal prism (`section_of` is some: a log, or a
/// convex wood ramp cut from one).
pub fn is_octagon(m: StateId) -> bool {
    section_of(m).is_some()
}

#[cfg(test)]
#[path = "octagon_tests.rs"]
mod tests;
