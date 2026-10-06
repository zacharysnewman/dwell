#include "dwell/core/block_shape.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace dwell::core {
namespace {

using V = std::array<float, 3>;

V Sub(const V& a, const V& b) { return {a[0] - b[0], a[1] - b[1], a[2] - b[2]}; }
V Add(const V& a, const V& b) { return {a[0] + b[0], a[1] + b[1], a[2] + b[2]}; }
V Mul(const V& a, float s) { return {a[0] * s, a[1] * s, a[2] * s}; }
float Dot(const V& a, const V& b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
V Cross(const V& a, const V& b) {
  return {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]};
}
// Slack, in cross-product units, for a hit on a polygon's edge (~4 µm).
constexpr float kEdgeTolerance = 1e-5f;

float Clamp01(float v) { return std::max(0.0f, std::min(1.0f, v)); }

V Vertex(const ShapeFace& f, int i) { return {f.v[i][0], f.v[i][1], f.v[i][2]}; }

// Closest point of triangle abc to p (Ericson, Real-Time Collision Detection §5.1.5).
V ClosestOnTriangle(const V& p, const V& a, const V& b, const V& c) {
  const V ab = Sub(b, a), ac = Sub(c, a), ap = Sub(p, a);
  const float d1 = Dot(ab, ap), d2 = Dot(ac, ap);
  if (d1 <= 0.0f && d2 <= 0.0f) return a;
  const V bp = Sub(p, b);
  const float d3 = Dot(ab, bp), d4 = Dot(ac, bp);
  if (d3 >= 0.0f && d4 <= d3) return b;
  const float vc = d1 * d4 - d3 * d2;
  if (vc <= 0.0f && d1 >= 0.0f && d3 <= 0.0f) return Add(a, Mul(ab, d1 / (d1 - d3)));
  const V cp = Sub(p, c);
  const float d5 = Dot(ab, cp), d6 = Dot(ac, cp);
  if (d6 >= 0.0f && d5 <= d6) return c;
  const float vb = d5 * d2 - d1 * d6;
  if (vb <= 0.0f && d2 >= 0.0f && d6 <= 0.0f) return Add(a, Mul(ac, d2 / (d2 - d6)));
  const float va = d3 * d6 - d5 * d4;
  if (va <= 0.0f && (d4 - d3) >= 0.0f && (d5 - d6) >= 0.0f) {
    const V bc = Sub(c, b);
    return Add(b, Mul(bc, (d4 - d3) / ((d4 - d3) + (d5 - d6))));
  }
  const float denom = 1.0f / (va + vb + vc);
  return Add(a, Add(Mul(ab, vb * denom), Mul(ac, vc * denom)));
}

float DistSq(const V& a, const V& b) {
  const V d = Sub(a, b);
  return Dot(d, d);
}

// Squared distance between segments p1q1 and p2q2 (Ericson §5.1.9).
float SegmentSegmentSq(const V& p1, const V& q1, const V& p2, const V& q2) {
  const V d1 = Sub(q1, p1), d2 = Sub(q2, p2), r = Sub(p1, p2);
  const float a = Dot(d1, d1), e = Dot(d2, d2), f = Dot(d2, r);
  constexpr float kEps = 1e-12f;
  float s, t;
  if (a <= kEps && e <= kEps) return Dot(r, r);
  if (a <= kEps) {
    s = 0.0f;
    t = Clamp01(f / e);
  } else {
    const float c = Dot(d1, r);
    if (e <= kEps) {
      t = 0.0f;
      s = Clamp01(-c / a);
    } else {
      const float b = Dot(d1, d2);
      const float denom = a * e - b * b;
      s = denom > kEps ? Clamp01((b * f - c * e) / denom) : 0.0f;
      t = (b * s + f) / e;
      if (t < 0.0f) {
        t = 0.0f;
        s = Clamp01(-c / a);
      } else if (t > 1.0f) {
        t = 1.0f;
        s = Clamp01((b - c) / a);
      }
    }
  }
  return DistSq(Add(p1, Mul(d1, s)), Add(p2, Mul(d2, t)));
}

// Whether the segment pq crosses the triangle abc (a touch counts).
bool SegmentHitsTriangle(const V& p, const V& q, const V& a, const V& b, const V& c) {
  const V n = Cross(Sub(b, a), Sub(c, a));
  const float dp = Dot(n, Sub(p, a)), dq = Dot(n, Sub(q, a));
  if ((dp > 0.0f && dq > 0.0f) || (dp < 0.0f && dq < 0.0f) || dp == dq) return false;
  const V hit = Add(p, Mul(Sub(q, p), dp / (dp - dq)));
  return DistSq(ClosestOnTriangle(hit, a, b, c), hit) <= 1e-12f;
}

// Height of the surface (in cell units) at (x, z): the planar triangle the point lies in.
float SurfaceHeight(const ShapeInfo& s, float x, float z) {
  const auto h = [&](int i) { return static_cast<float>(s.corners[i]) * 0.5f; };
  float v;
  if (s.diagonal == 0) {  // NW–SE
    v = x >= z ? h(0) + (h(1) - h(0)) * x + (h(2) - h(1)) * z
               : h(0) + (h(2) - h(3)) * x + (h(3) - h(0)) * z;
  } else {  // NE–SW
    v = x + z <= 1.0f ? h(0) + (h(1) - h(0)) * x + (h(3) - h(0)) * z
                      : h(2) + (h(3) - h(2)) * (1.0f - x) + (h(1) - h(2)) * (1.0f - z);
  }
  return v;
}

}  // namespace

SolidSpan SolidSpanAt(const ShapeInfo& shape, float fx, float fz) {
  const float h =
      SurfaceHeight(shape, std::max(0.0f, std::min(1.0f, fx)), std::max(0.0f, std::min(1.0f, fz)));
  return shape.inverted ? SolidSpan{1.0f - h, 1.0f} : SolidSpan{0.0f, h};
}

bool PointInSolid(const ShapeInfo& shape, float x, float y, float z, float eps) {
  if (shape.face_count == 0) return false;
  if (x < -eps || x > 1.0f + eps || y < -eps || y > 1.0f + eps || z < -eps || z > 1.0f + eps) {
    return false;
  }
  const SolidSpan span = SolidSpanAt(shape, x, z);
  return span.hi > span.lo && y >= span.lo - eps && y <= span.hi + eps;
}

bool FaceCovered(const ShapeInfo& shape, int face, const ShapeInfo& neighbour) {
  switch (face) {
    case 2:  // +Y: the neighbour above's floor
      return !shape.full_top || neighbour.full_bottom;
    case 3:  // −Y: the neighbour below's ceiling
      return !shape.full_bottom || neighbour.full_top;
    default: {
      // Side faces: the region under (upright) or over (inverted) a linear height profile along
      // the face. The neighbour's opposite face runs along the same coordinate.
      const auto& mine = shape.sides[face < 2 ? face : face - 2];
      const auto& theirs =
          neighbour.sides[OppositeFace(face) < 2 ? OppositeFace(face) : OppositeFace(face) - 2];
      if (mine[0] == 0 && mine[1] == 0) return true;
      if (shape.inverted == neighbour.inverted) {
        return theirs[0] >= mine[0] && theirs[1] >= mine[1];
      }
      return theirs[0] == 2 && theirs[1] == 2;
    }
  }
}

std::optional<ShapeHit> RayEnterShape(const ShapeInfo& shape, const float (&cell)[3],
                                      const float (&origin)[3], const float (&dir)[3], float max_t,
                                      const ShapeInfo* const* neighbours, float min_t) {
  const V c{cell[0], cell[1], cell[2]};
  const V o{origin[0], origin[1], origin[2]}, d{dir[0], dir[1], dir[2]};
  float best = std::numeric_limits<float>::infinity();
  V best_normal{0, 0, 0};
  for (const ShapeFace& face : FacesOf(shape)) {
    // The polygon in the ray's frame.
    V pts[4];
    for (int i = 0; i < face.count; ++i) pts[i] = Add(c, Vertex(face, i));
    const V a = pts[0];
    const V n = Cross(Sub(pts[1], a), Sub(pts[2], a));
    const float denom = Dot(n, d);
    if (denom >= 0.0f) continue;  // entering the solid means moving against the outward normal
    // An axis-aligned plane takes the box cast's exact arithmetic: (plane − origin) / direction.
    int flat_axis = -1;
    for (int k = 0; k < 3; ++k) {
      if (n[k] != 0.0f && n[(k + 1) % 3] == 0.0f && n[(k + 2) % 3] == 0.0f) flat_axis = k;
    }
    const float t =
        flat_axis >= 0 ? (a[flat_axis] - o[flat_axis]) / d[flat_axis] : Dot(n, Sub(a, o)) / denom;
    if (t < min_t || t > max_t || t >= best) continue;
    const V p = Add(o, Mul(d, t));
    // Inside the convex polygon: on the inner side of every edge.
    bool inside = true;
    for (int i = 0; i < face.count && inside; ++i) {
      const V e = Sub(pts[(i + 1) % face.count], pts[i]);
      // On or inside every edge (a ray along an edge counts as touching).
      if (Dot(Cross(e, Sub(p, pts[i])), n) < -kEdgeTolerance) inside = false;
    }
    if (!inside) continue;
    if (neighbours && face.tag < kSurfaceTag) {
      // Where the neighbour's solid lies against this face, the ray crosses from solid to solid.
      V q = Sub(p, c);
      q[face.tag >> 1] -= face.tag & 1 ? -1.0f : 1.0f;
      if (PointInSolid(*neighbours[face.tag], q[0], q[1], q[2], 1e-4f)) continue;
    }
    best = t;
    const float len2 = Dot(n, n);
    const float inv = len2 > 0.0f ? 1.0f / std::sqrt(len2) : 0.0f;
    best_normal = Mul(n, inv);
  }
  if (best == std::numeric_limits<float>::infinity()) return std::nullopt;
  return ShapeHit{best, {best_normal[0], best_normal[1], best_normal[2]}};
}

float VerticalSegmentDistanceSq(const ShapeInfo& shape, float x, float z, float y0, float y1) {
  if (shape.face_count == 0) return std::numeric_limits<float>::infinity();
  // An endpoint inside the solid (within the cell column) means overlap.
  if (x >= 0.0f && x <= 1.0f && z >= 0.0f && z <= 1.0f) {
    const SolidSpan span = SolidSpanAt(shape, x, z);
    if (y1 >= span.lo && y0 <= span.hi && span.hi > span.lo) return 0.0f;
  }
  const V p{x, y0, z}, q{x, y1, z};
  float best = std::numeric_limits<float>::infinity();
  for (const ShapeFace& face : FacesOf(shape)) {
    for (int k = 1; k + 1 < face.count; ++k) {
      const V a = Vertex(face, 0), b = Vertex(face, k), c = Vertex(face, k + 1);
      if (SegmentHitsTriangle(p, q, a, b, c)) return 0.0f;
      best = std::min(best, DistSq(ClosestOnTriangle(p, a, b, c), p));
      best = std::min(best, DistSq(ClosestOnTriangle(q, a, b, c), q));
      best = std::min(best, SegmentSegmentSq(p, q, a, b));
      best = std::min(best, SegmentSegmentSq(p, q, b, c));
      best = std::min(best, SegmentSegmentSq(p, q, c, a));
    }
  }
  return best;
}

}  // namespace dwell::core
