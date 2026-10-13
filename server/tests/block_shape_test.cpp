// Voxel shapes (docs/SLOPE_BLOCKS.md §1): the shape table verified from geometry, and the
// exhaustive adjacency check (no holes between any two shapes) over every shape in the registry.
#include <doctest/doctest.h>

#include <array>
#include <cmath>
#include <map>
#include <set>
#include <string>
#include <tuple>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/block_shape.h"
#include "dwell/core/voxel.h"

namespace dwell::core {
namespace {

struct Vec {
  double x, y, z;
};
Vec Sub(Vec a, Vec b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
Vec Cross(Vec a, Vec b) {
  return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
}
double Dot(Vec a, Vec b) { return a.x * b.x + a.y * b.y + a.z * b.z; }

Vec Vertex(const ShapeFace& f, int i) { return {f.v[i][0], f.v[i][1], f.v[i][2]}; }

// Volume by the divergence theorem over the fan-triangulated outward faces.
double VolumeOf(const ShapeInfo& shape) {
  double six = 0;
  for (const ShapeFace& f : FacesOf(shape)) {
    for (int k = 1; k + 1 < f.count; ++k) {
      six += Dot(Vertex(f, 0), Cross(Vertex(f, k), Vertex(f, k + 1)));
    }
  }
  return six / 6.0;
}

// Every edge of a closed surface is used once in each direction.
bool IsClosed(const ShapeInfo& shape) {
  std::map<std::tuple<float, float, float, float, float, float>, int> edges;
  auto key = [](Vec a, Vec b) {
    return std::make_tuple(static_cast<float>(a.x), static_cast<float>(a.y),
                           static_cast<float>(a.z), static_cast<float>(b.x),
                           static_cast<float>(b.y), static_cast<float>(b.z));
  };
  for (const ShapeFace& f : FacesOf(shape)) {
    for (int i = 0; i < f.count; ++i) {
      ++edges[key(Vertex(f, i), Vertex(f, (i + 1) % f.count))];
    }
  }
  // A polygon edge may be split in two on the other side (a T-junction); compare by the boundary's
  // total length per direction instead: every directed edge's reverse must lie on the surface.
  // Here shapes share whole edges, so the exact reverse must exist.
  for (const auto& [k, n] : edges) {
    const auto reverse = std::make_tuple(std::get<3>(k), std::get<4>(k), std::get<5>(k),
                                         std::get<0>(k), std::get<1>(k), std::get<2>(k));
    const auto it = edges.find(reverse);
    if (it == edges.end() || it->second != n) return false;
  }
  return true;
}

// A solid is convex iff every vertex lies on or inside every face's plane.
bool IsConvexByGeometry(const ShapeInfo& shape) {
  for (const ShapeFace& f : FacesOf(shape)) {
    const Vec n = Cross(Sub(Vertex(f, 1), Vertex(f, 0)), Sub(Vertex(f, 2), Vertex(f, 0)));
    for (const ShapeFace& g : FacesOf(shape)) {
      for (int i = 0; i < g.count; ++i) {
        if (Dot(n, Sub(Vertex(g, i), Vertex(f, 0))) > 1e-9) return false;
      }
    }
  }
  return true;
}

// The §1.2 table, in the canonical orientation (facing east, upright): corner heights NW NE SE SW
// in cells, the volume and whether the solid is convex.
}  // namespace

TEST_SUITE("block shapes") {
  // The shaped families were cut with the baseline (Phase 15): the registry holds cubes only, so
  // the shape table is the empty shape and the cube. Shaped blocks return as adaptive natural
  // blocks (Phase 18), with their own table tests.
  TEST_CASE("cubes are shapes too") {
    CHECK(VolumeOf(ShapeOf(Materials::kStone)) == doctest::Approx(1.0));
    CHECK(IsClosed(ShapeOf(Materials::kStone)));
    CHECK(IsConvexByGeometry(ShapeOf(Materials::kStone)));
    CHECK(SurfaceHeightAt(Materials::kStone, 0.3f, 0.7f) == 1.0f);
    CHECK(SurfaceHeightAt(Materials::kAir, 0.3f, 0.7f) == 0.0f);
    CHECK(ShapeOf(Materials::kAir).face_count == 0);
    CHECK(ShapeOf(Materials::kLadder).face_count == 0);  // empty-shaped blocks have no solid
  }
}

namespace {

// Is (a, b) on the shared plane inside the polygon (2D, dropping the plane's axis)?
bool InPolygon(const ShapeFace& f, int axis, double a, double b) {
  const int u = (axis + 1) % 3, v = (axis + 2) % 3;
  bool inside = true;
  int sign = 0;
  for (int i = 0; i < f.count && inside; ++i) {
    const double ax = f.v[i][u], ay = f.v[i][v];
    const double bx = f.v[(i + 1) % f.count][u], by = f.v[(i + 1) % f.count][v];
    const double cross = (bx - ax) * (b - ay) - (by - ay) * (a - ax);
    if (std::fabs(cross) < 1e-9) continue;
    const int s = cross > 0 ? 1 : -1;
    if (sign == 0) sign = s;
    if (s != sign) inside = false;
  }
  return inside && sign != 0;
}

}  // namespace

TEST_SUITE("block shapes: adjacency") {
  // The distinct shapes: one state per shape index.
  TEST_CASE("no holes between any two shapes on any side") {
    std::map<std::uint16_t, MaterialId> distinct;
    for (MaterialId m = 0; m < Materials::kCount; ++m) {
      const auto index = GetMaterial(m).shape_index;
      if (index != 0) distinct.try_emplace(index, m);
    }
    REQUIRE(!distinct.empty());
    constexpr int kSamples = 13;
    long checked = 0, doubled = 0;
    for (const auto& [ia, a] : distinct) {
      const ShapeInfo& sa = ShapeOf(a);
      for (const auto& [ib, b] : distinct) {
        const ShapeInfo& sb = ShapeOf(b);
        for (int face = 0; face < 6; ++face) {
          const int axis = face / 2;
          const int u = (axis + 1) % 3, v = (axis + 2) % 3;
          // Polygons of A on `face`, and of B on the opposite face, as they would be drawn.
          const auto drawn_a = [&](double pu, double pv) {
            if (FaceCovered(sa, face, sb)) return 0;
            int n = 0;
            for (const ShapeFace& f : FacesOf(sa)) n += f.tag == face && InPolygon(f, axis, pu, pv);
            return n;
          };
          const auto drawn_b = [&](double pu, double pv) {
            if (FaceCovered(sb, OppositeFace(face), sa)) return 0;
            int n = 0;
            for (const ShapeFace& f : FacesOf(sb)) {
              n += f.tag == OppositeFace(face) && InPolygon(f, axis, pu, pv);
            }
            return n;
          };
          // Is the solid on the plane at (pu, pv), seen from each shape's own cell?
          const auto solid = [&](const ShapeInfo& s, int own_face, double pu, double pv) {
            double p[3];
            p[axis] = own_face % 2 == 0 ? 1.0 : 0.0;
            p[u] = pu;
            p[v] = pv;
            // Strictly inside the solid's extent on the plane, or filling the plane's whole cell
            // face (the cell's top or bottom).
            if (axis == 1) return own_face == 2 ? s.full_top : s.full_bottom;
            const SolidSpan span =
                SolidSpanAt(s, static_cast<float>(p[0]), static_cast<float>(p[2]));
            return span.hi - span.lo > 1e-6f && p[1] > span.lo + 1e-6 && p[1] < span.hi - 1e-6;
          };
          for (int i = 0; i < kSamples; ++i) {
            for (int j = 0; j < kSamples; ++j) {
              // Off-grid sample points, away from edges and slope lines.
              const double pu = (i + 0.31) / kSamples, pv = (j + 0.57) / kSamples;
              const bool in_a = solid(sa, face, pu, pv);
              const bool in_b = solid(sb, OppositeFace(face), pu, pv);
              const int drawn = drawn_a(pu, pv) + drawn_b(pu, pv);
              ++checked;
              if (in_a != in_b) {
                // Exposed solid surface: drawn exactly once (by the exposed side), no hole.
                if (drawn != 1) {
                  FAIL_CHECK("hole or overlap: shapes " << StateString(a) << " | " << StateString(b)
                                                        << " face " << face << " at " << pu << ","
                                                        << pv << " drawn " << drawn);
                  return;
                }
              } else if (!in_a && drawn != 0) {
                FAIL_CHECK("a face drawn in empty space: " << StateString(a) << " | "
                                                           << StateString(b) << " face " << face);
                return;
              } else if (in_a && in_b) {
                // Both solid: the interface should be culled. A partial cover of different
                // inversions may leave a back-to-back pair (invisible); count them.
                if (drawn > 0) ++doubled;
              }
            }
          }
        }
      }
    }
    MESSAGE(checked << " samples; back-to-back interior faces: " << doubled);
    CHECK(checked >=
          6 * kSamples * kSamples * static_cast<long>(distinct.size() * distinct.size()));
  }
}

}  // namespace dwell::core
