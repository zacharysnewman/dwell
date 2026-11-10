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

use crate::{Axis, CrossSection, StateId, info};

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
    d[face / 2] = if face % 2 == 0 { 1 } else { -1 };
    d
}

/// Its octagonal cross-section and axis, if state `m` has one.
pub fn section_of(m: StateId) -> Option<(CrossSection, usize)> {
    crate::try_info(m).and_then(|s| s.cross_section.map(|c| (c, axis_index(s.axis))))
}

/// Whether two states join (a full-width side between them): both octagonal, of one join group,
/// along one axis.
pub fn joins(a: StateId, b: StateId) -> bool {
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
    let mut mask = 0;
    for face in 0..6 {
        if face / 2 != axis && joins(m, at(face_offset(face))) {
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
}

/// One convex, outward-wound polygon of the prism, in cell coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub part: Part,
    pub pts: Vec<[f64; 3]>,
}

/// The cross-section's outline: counter-clockwise points (u, v) round the axis (u = axis+1,
/// v = axis+2, so u × v points along the axis), each with the cell face its edge to the next lies
/// on (or `None`: a chamfer).
pub fn outline(chamfer: f64, axis: usize, joined: u8, size_m: [f64; 3]) -> Vec<([f64; 2], Option<usize>)> {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let c_m = chamfer * size_m[u].min(size_m[v]);
    let (cu, cv) = (c_m / size_m[u], c_m / size_m[v]);
    let j = |face: usize| joined & (1 << face) != 0;
    let (pu, nu, pv, nv) = (
        face_index(u, true),
        face_index(u, false),
        face_index(v, true),
        face_index(v, false),
    );
    // Corners CCW: (0,0) between −u and −v, (1,0) −v and +u, (1,1) +u and +v, (0,1) +v and −u.
    // Each: (corner, the side arriving at it, the side leaving it, the chamfer's two points).
    let corners: [([f64; 2], usize, usize, [[f64; 2]; 2]); 4] = [
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
    let ring = outline(f64::from(section.chamfer), axis, joined, size_m);
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
    let mut out = faces(&section, axis, joined, size_m);
    out.retain(|f| match f.part {
        Part::Side(face) => {
            let n = at(face_offset(face));
            joined & (1 << face) == 0 && !(covers(n) && section_of(n).is_none())
        }
        Part::Cap(end) => {
            let d = face_offset(face_index(axis, end));
            let n = at(d);
            if section_of(n).is_none() {
                return !covers(n);
            }
            if !joins(m, n) {
                return true;
            }
            // The neighbour's squared corners must include this one's: its joined sides hold ours.
            let theirs = joined_mask(n, &|o| at([o[0] + d[0], o[1] + d[1], o[2] + d[2]]));
            let same = section_of(n).map(|(c, _)| c) == Some(section);
            !(same && theirs & joined == joined)
        }
        Part::Chamfer(..) => true,
    });
    out
}

/// `info(m).cross_section.is_some()`: whether `m` is drawn and collided as an octagonal prism.
pub fn is_octagon(m: StateId) -> bool {
    info(m).cross_section.is_some()
}

#[cfg(test)]
#[path = "octagon_tests.rs"]
mod tests;
