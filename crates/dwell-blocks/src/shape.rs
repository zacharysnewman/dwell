//! Voxel shapes: the solid geometry of a state inside its 1 m cell (docs/SLOPE_BLOCKS.md). Every
//! function uses only `+ − × /`, `min`, `max` and `sqrt` on floats (ADR 0010), so server and WASM
//! client agree bit for bit.

use crate::{SHAPE_FACES, SHAPES, ShapeFace, ShapeInfo, StateId, info};

/// The `tag` of a sloped (or flat, partial-height) top polygon; 0..5 are cell faces.
pub const SURFACE_TAG: u8 = 6;

pub fn shape_of(state: StateId) -> &'static ShapeInfo {
    &SHAPES[usize::from(info(state).shape_index)]
}

pub fn faces_of(shape: &ShapeInfo) -> &'static [ShapeFace] {
    let first = usize::from(shape.first_face);
    &SHAPE_FACES[first..first + usize::from(shape.face_count)]
}

/// The same shape turned upside down (mirrored in y): hanging ↔ upright. A shape whose mirror image
/// is not in the table (none today) is returned as it is.
pub fn turned_over(shape: &'static ShapeInfo) -> &'static ShapeInfo {
    static MIRROR: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let table = MIRROR.get_or_init(|| {
        (0..SHAPES.len())
            .map(|i| {
                let a = &SHAPES[i];
                SHAPES
                    .iter()
                    .position(|b| {
                        b.face_count > 0
                            && b.inverted != a.inverted
                            && b.corners == a.corners
                            && b.points == a.points
                            && b.fan == a.fan
                            && b.diagonal == a.diagonal
                    })
                    .unwrap_or(i) as u16
            })
            .collect()
    });
    // Its row in SHAPES (a shape from elsewhere is returned as it is).
    match SHAPES.iter().position(|s| std::ptr::eq(s, shape)) {
        Some(i) => &SHAPES[usize::from(table[i])],
        None => shape,
    }
}

/// The solid's extent in y at a horizontal point within the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidSpan {
    pub lo: f32,
    pub hi: f32,
}

type V = [f32; 3];
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn clamp01(v: f32) -> f32 {
    0.0f32.max(1.0f32.min(v))
}
fn dist_sq(a: V, b: V) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}

/// Slack, in cross-product units, for a hit on a polygon's edge (~4 µm).
const EDGE_TOLERANCE: f32 = 1e-5;

/// Closest point of triangle abc to p (Ericson, Real-Time Collision Detection §5.1.5).
fn closest_on_triangle(p: V, a: V, b: V, c: V) -> V {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, mul(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, mul(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let bc = sub(c, b);
        return add(b, mul(bc, (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(mul(ab, vb * denom), mul(ac, vc * denom)))
}

/// Squared distance between segments p1q1 and p2q2 (Ericson §5.1.9).
fn segment_segment_sq(p1: V, q1: V, p2: V, q2: V) -> f32 {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    const EPS: f32 = 1e-12;
    if a <= EPS && e <= EPS {
        return dot(r, r);
    }
    let (s, t);
    if a <= EPS {
        s = 0.0;
        t = clamp01(f / e);
    } else {
        let c = dot(d1, r);
        if e <= EPS {
            t = 0.0;
            s = clamp01(-c / a);
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let s0 = if denom > EPS {
                clamp01((b * f - c * e) / denom)
            } else {
                0.0
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = clamp01(-c / a);
            } else if t0 > 1.0 {
                t = 1.0;
                s = clamp01((b - c) / a);
            } else {
                s = s0;
                t = t0;
            }
        }
    }
    dist_sq(add(p1, mul(d1, s)), add(p2, mul(d2, t)))
}

/// Whether the segment pq crosses the triangle abc (a touch counts).
fn segment_hits_triangle(p: V, q: V, a: V, b: V, c: V) -> bool {
    let n = cross(sub(b, a), sub(c, a));
    let dp = dot(n, sub(p, a));
    let dq = dot(n, sub(q, a));
    if (dp > 0.0 && dq > 0.0) || (dp < 0.0 && dq < 0.0) || dp == dq {
        return false;
    }
    let hit = add(p, mul(sub(q, p), dp / (dp - dq)));
    dist_sq(closest_on_triangle(hit, a, b, c), hit) <= 1e-12
}

/// Height of the surface (cell units) at (x, z): the planar triangle the point lies in.
fn surface_height(s: &ShapeInfo, x: f32, z: f32) -> f32 {
    if s.fan {
        return fan_height(s, x, z);
    }
    let h = |i: usize| f32::from(s.corners[i]) * 0.5;
    if s.diagonal == 0 {
        // NW–SE
        if x >= z {
            h(0) + (h(1) - h(0)) * x + (h(2) - h(1)) * z
        } else {
            h(0) + (h(2) - h(3)) * x + (h(3) - h(0)) * z
        }
    } else if x + z <= 1.0 {
        // NE–SW
        h(0) + (h(1) - h(0)) * x + (h(3) - h(0)) * z
    } else {
        h(2) + (h(3) - h(2)) * (1.0 - x) + (h(1) - h(2)) * (1.0 - z)
    }
}

/// A ridge shape's surface: in the quadrant holding (x, z), the triangle of its corner K, the
/// centre C and the edge midpoint on the point's side of the K–C diagonal.
fn fan_height(s: &ShapeInfo, x: f32, z: f32) -> f32 {
    let p = |r: usize, c: usize| f32::from(s.points[r][c]) * 0.25;
    let (kc, kx) = if x >= 0.5 { (2, 1.0) } else { (0, 0.0) };
    let (kr, kz) = if z >= 0.5 { (2, 1.0) } else { (0, 0.0) };
    // Distances from K toward the centre along x and z, in [0, 1].
    let u = (x - kx).abs() * 2.0;
    let w = (z - kz).abs() * 2.0;
    let (hk, hc) = (p(kr, kc), p(1, 1));
    if u >= w {
        // K, the midpoint along x from K (same row), C.
        let hm = p(kr, 1);
        hk + (hm - hk) * u + (hc - hm) * w
    } else {
        let hm = p(1, kc);
        hk + (hm - hk) * w + (hc - hm) * u
    }
}

/// The solid's extent in y at (fx, fz) ∈ [0, 1]²: upright pieces fill [0, h], inverted ones
/// [1 − h, 1].
pub fn solid_span_at(shape: &ShapeInfo, fx: f32, fz: f32) -> SolidSpan {
    let h = surface_height(shape, clamp01(fx), clamp01(fz));
    if shape.inverted {
        SolidSpan {
            lo: 1.0 - h,
            hi: 1.0,
        }
    } else {
        SolidSpan { lo: 0.0, hi: h }
    }
}

/// Height of the top of the solid at (fx, fz) (0 for an empty cell).
pub fn surface_height_at(state: StateId, fx: f32, fz: f32) -> f32 {
    let shape = shape_of(state);
    if shape.face_count == 0 {
        0.0
    } else {
        solid_span_at(shape, fx, fz).hi
    }
}

/// Whether the point (cell coordinates) is inside the solid or on its boundary, within `eps`.
pub fn point_in_solid(shape: &ShapeInfo, x: f32, y: f32, z: f32, eps: f32) -> bool {
    if shape.face_count == 0 {
        return false;
    }
    if x < -eps || x > 1.0 + eps || y < -eps || y > 1.0 + eps || z < -eps || z > 1.0 + eps {
        return false;
    }
    let span = solid_span_at(shape, x, z);
    span.hi > span.lo && y >= span.lo - eps && y <= span.hi + eps
}

/// Cell faces as the mesher numbers them: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
pub const fn opposite_face(face: usize) -> usize {
    face ^ 1
}

/// Whether the part of `shape` on its cell face `face` is completely covered by the opposite face
/// of `neighbour`. Empty faces are covered by anything.
pub fn face_covered(shape: &ShapeInfo, face: usize, neighbour: &ShapeInfo) -> bool {
    match face {
        2 => !shape.full_top || neighbour.full_bottom,
        3 => !shape.full_bottom || neighbour.full_top,
        _ => {
            let side = |f: usize| if f < 2 { f } else { f - 2 };
            let mine = shape.sides[side(face)];
            let theirs = neighbour.sides[side(opposite_face(face))];
            if mine == [0; 3] {
                return true;
            }
            if shape.inverted == neighbour.inverted {
                (0..3).all(|i| theirs[i] >= mine[i])
            } else {
                theirs == [4; 3]
            }
        }
    }
}

/// Where a ray enters a solid: the distance and the outward normal of the face entered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeHit {
    pub t: f32,
    pub normal: [f32; 3],
}

/// The nearest entry of the ray `origin + t·dir`, t ∈ [min_t, max_t], into the solid of the cell
/// whose minimum corner is `cell` (origin and cell share a frame). A ray starting inside does not
/// hit its own surface. With `neighbours` (the shape across face +X −X +Y −Y +Z −Z, asked only for
/// faces the ray could enter through), a point of a cell face lying against the neighbour's solid
/// is internal and not an entry.
pub fn ray_enter_shape(
    shape: &ShapeInfo,
    cell: [f32; 3],
    origin: [f32; 3],
    dir: [f32; 3],
    max_t: f32,
    mut neighbours: Option<&mut dyn FnMut(usize) -> &'static ShapeInfo>,
    min_t: f32,
) -> Option<ShapeHit> {
    let mut best = f32::INFINITY;
    let mut best_normal = [0.0; 3];
    for face in faces_of(shape) {
        let count = usize::from(face.count);
        let pts: Vec<V> = (0..count).map(|i| add(cell, face.v[i])).collect();
        let a = pts[0];
        let n = cross(sub(pts[1], a), sub(pts[2], a));
        let denom = dot(n, dir);
        if denom >= 0.0 {
            continue; // entering means moving against the outward normal
        }
        // An axis-aligned plane takes the box cast's exact arithmetic: (plane − origin) / direction.
        let mut flat_axis = None;
        for k in 0..3 {
            if n[k] != 0.0 && n[(k + 1) % 3] == 0.0 && n[(k + 2) % 3] == 0.0 {
                flat_axis = Some(k);
            }
        }
        let t = match flat_axis {
            Some(k) => (a[k] - origin[k]) / dir[k],
            None => dot(n, sub(a, origin)) / denom,
        };
        if t < min_t || t > max_t || t >= best {
            continue;
        }
        let p = add(origin, mul(dir, t));
        let inside = (0..count).all(|i| {
            let e = sub(pts[(i + 1) % count], pts[i]);
            dot(cross(e, sub(p, pts[i])), n) >= -EDGE_TOLERANCE
        });
        if !inside {
            continue;
        }
        if let Some(nb) = neighbours.as_mut()
            && face.tag < SURFACE_TAG
        {
            let mut q = sub(p, cell);
            q[usize::from(face.tag >> 1)] -= if face.tag & 1 != 0 { -1.0 } else { 1.0 };
            if point_in_solid(nb(usize::from(face.tag)), q[0], q[1], q[2], 1e-4) {
                continue;
            }
        }
        best = t;
        let len2 = dot(n, n);
        let inv = if len2 > 0.0 { 1.0 / len2.sqrt() } else { 0.0 };
        best_normal = mul(n, inv);
    }
    (best != f32::INFINITY).then_some(ShapeHit {
        t: best,
        normal: best_normal,
    })
}

/// Squared shortest distance from the vertical segment (x, y0..y1, z) — a capsule axis, cell
/// coordinates — to the shape's surface; 0 if it touches or lies inside the solid.
pub fn vertical_segment_distance_sq(shape: &ShapeInfo, x: f32, z: f32, y0: f32, y1: f32) -> f32 {
    if shape.face_count == 0 {
        return f32::INFINITY;
    }
    if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&z) {
        let span = solid_span_at(shape, x, z);
        if y1 >= span.lo && y0 <= span.hi && span.hi > span.lo {
            return 0.0;
        }
    }
    let p = [x, y0, z];
    let q = [x, y1, z];
    let mut best = f32::INFINITY;
    for face in faces_of(shape) {
        for k in 1..usize::from(face.count) - 1 {
            let (a, b, c) = (face.v[0], face.v[k], face.v[k + 1]);
            if segment_hits_triangle(p, q, a, b, c) {
                return 0.0;
            }
            best = best.min(dist_sq(closest_on_triangle(p, a, b, c), p));
            best = best.min(dist_sq(closest_on_triangle(q, a, b, c), q));
            best = best.min(segment_segment_sq(p, q, a, b));
            best = best.min(segment_segment_sq(p, q, b, c));
            best = best.min(segment_segment_sq(p, q, c, a));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::require;

    #[test]
    fn cube_geometry() {
        let cube = shape_of(require("dwell:stone"));
        assert!(point_in_solid(cube, 0.5, 0.5, 0.5, 0.0));
        assert!(!point_in_solid(cube, 0.5, 1.5, 0.5, 0.0));
        assert_eq!(surface_height_at(require("dwell:stone"), 0.3, 0.7), 1.0);
        assert_eq!(surface_height_at(require("dwell:air"), 0.3, 0.7), 0.0);
        assert!(face_covered(cube, 0, cube));
        assert!(!face_covered(cube, 2, shape_of(require("dwell:air"))));
        let hit = ray_enter_shape(
            cube,
            [0.0; 3],
            [-1.0, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            5.0,
            None,
            0.0,
        )
        .expect("hits");
        assert_eq!(hit.t, 1.0);
        assert_eq!(hit.normal, [-1.0, 0.0, 0.0]);
        assert_eq!(vertical_segment_distance_sq(cube, 0.5, 0.5, 1.5, 2.0), 0.25);
        assert_eq!(vertical_segment_distance_sq(cube, 0.5, 0.5, 0.5, 2.0), 0.0);
    }
}
