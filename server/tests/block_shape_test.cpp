// Voxel shapes (docs/SLOPE_BLOCKS.md §1–§2): the shape table verified from geometry, the slope
// families in the registry, and the exhaustive adjacency check (no holes between any two shapes).
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
struct Row {
  const char* shape;
  std::array<double, 4> corners;
  double volume;
  bool convex;
};
const Row kTable[] = {
    {"wedge", {1, 0, 0, 1}, 1.0 / 2, true},
    {"outer", {1, 0, 0, 0}, 1.0 / 3, true},
    {"inner", {1, 1, 0, 1}, 2.0 / 3, false},
    {"gentle_low", {0.5, 0, 0, 0.5}, 1.0 / 4, true},
    {"gentle_high", {1, 0.5, 0.5, 1}, 3.0 / 4, true},
    {"gentle_outer_low", {0.5, 0, 0, 0}, 1.0 / 6, true},
    {"gentle_outer_high", {1, 0.5, 0.5, 0.5}, 2.0 / 3, true},
    {"gentle_inner_low", {0.5, 0.5, 0, 0.5}, 1.0 / 3, false},
    {"gentle_inner_high", {1, 1, 0.5, 1}, 5.0 / 6, false},
};

MaterialId Slope(const std::string& shape, const std::string& facing, const std::string& half,
                 bool flooded = false) {
  const auto id =
      ParseState("dwell:stone_slope[facing=" + facing + ",flooded=" + (flooded ? "true" : "false") +
                 ",half=" + half + ",shape=" + shape + "]");
  REQUIRE(id.has_value());
  return *id;
}

// The surface height at the four corners (NW, NE, SE, SW) of the cell's top surface.
std::array<double, 4> Corners(MaterialId state) {
  const ShapeInfo& s = ShapeOf(state);
  auto h = [&](float x, float z) {
    const SolidSpan span = SolidSpanAt(s, x, z);
    return static_cast<double>(s.inverted ? 1.0f - span.lo : span.hi);
  };
  return {h(0, 0), h(1, 0), h(1, 1), h(0, 1)};
}

}  // namespace

TEST_SUITE("block shapes") {
  TEST_CASE("the shape table matches the design: corner heights, volumes and convexity") {
    for (const Row& row : kTable) {
      CAPTURE(row.shape);
      const MaterialId state = Slope(row.shape, "east", "bottom");
      const ShapeInfo& shape = ShapeOf(state);
      const auto corners = Corners(state);
      for (int i = 0; i < 4; ++i) CHECK(corners[i] == doctest::Approx(row.corners[i]));
      // Independent of the baked numbers: volume and convexity from the faces themselves.
      CHECK(VolumeOf(shape) == doctest::Approx(row.volume));
      CHECK(shape.volume == doctest::Approx(row.volume));
      CHECK(IsConvexByGeometry(shape) == row.convex);
      CHECK(shape.convex == row.convex);
      CHECK(IsClosed(shape));
    }
  }

  TEST_CASE("cubes and slabs are shapes too") {
    CHECK(VolumeOf(ShapeOf(Materials::kStone)) == doctest::Approx(1.0));
    CHECK(VolumeOf(ShapeOf(Materials::kStoneSlab)) == doctest::Approx(0.5));
    const MaterialId top = *ParseState("dwell:stone_slab[half=top]");
    CHECK(VolumeOf(ShapeOf(top)) == doctest::Approx(0.5));
    CHECK(ShapeOf(top).inverted);
    CHECK(SurfaceHeightAt(Materials::kStoneSlab, 0.3f, 0.7f) == 0.5f);
    CHECK(SurfaceHeightAt(Materials::kStone, 0.3f, 0.7f) == 1.0f);
    CHECK(SurfaceHeightAt(Materials::kAir, 0.3f, 0.7f) == 0.0f);
    CHECK(ShapeOf(Materials::kAir).face_count == 0);
  }

  TEST_CASE("every variant is a closed outward-wound solid of the right volume") {
    for (const Row& row : kTable) {
      for (const char* facing : {"north", "east", "south", "west"}) {
        for (const char* half : {"bottom", "top"}) {
          CAPTURE(row.shape);
          CAPTURE(facing);
          CAPTURE(half);
          const ShapeInfo& shape = ShapeOf(Slope(row.shape, facing, half));
          CHECK(IsClosed(shape));
          CHECK(VolumeOf(shape) == doctest::Approx(row.volume));  // positive: wound outward
          CHECK(IsConvexByGeometry(shape) == row.convex);
          CHECK(shape.inverted == (std::string(half) == "top"));
        }
      }
    }
  }

  TEST_CASE("a slope descends toward its facing") {
    // North is −Z, east +X, south +Z, west −X; the centre of the side it faces is lower than the
    // centre of the opposite side.
    const struct {
      const char* facing;
      float fx, fz;  // the side it descends toward
      float ox, oz;  // the opposite side
    } cases[] = {{"east", 1, 0.5f, 0, 0.5f},
                 {"west", 0, 0.5f, 1, 0.5f},
                 {"south", 0.5f, 1, 0.5f, 0},
                 {"north", 0.5f, 0, 0.5f, 1}};
    for (const auto& c : cases) {
      CAPTURE(c.facing);
      const MaterialId s = Slope("wedge", c.facing, "bottom");
      CHECK(SurfaceHeightAt(s, c.fx, c.fz) == 0.0f);
      CHECK(SurfaceHeightAt(s, c.ox, c.oz) == 1.0f);
      CHECK(SurfaceHeightAt(s, 0.5f, 0.5f) == doctest::Approx(0.5f));
    }
    // Gentle wedges: 1 in 2.
    CHECK(SurfaceHeightAt(Slope("gentle_low", "east", "bottom"), 0.0f, 0.5f) == 0.5f);
    CHECK(SurfaceHeightAt(Slope("gentle_low", "east", "bottom"), 1.0f, 0.5f) == 0.0f);
    CHECK(SurfaceHeightAt(Slope("gentle_high", "east", "bottom"), 0.0f, 0.5f) == 1.0f);
    CHECK(SurfaceHeightAt(Slope("gentle_high", "east", "bottom"), 1.0f, 0.5f) == 0.5f);
  }

  TEST_CASE("corner pieces keep the pitch of their faces; the hip line is shallower") {
    // Outer corner, east-facing: the faces rise 1 per 1 along x and z; the hip runs from the high
    // corner (0,0) to the low one (1,1): 1 over √2 = 35.26°.
    const MaterialId outer = Slope("outer", "east", "bottom");
    CHECK(SurfaceHeightAt(outer, 0.5f, 0.0f) == doctest::Approx(0.5f));  // face: 45°
    CHECK(SurfaceHeightAt(outer, 0.0f, 0.5f) == doctest::Approx(0.5f));
    CHECK(SurfaceHeightAt(outer, 0.5f, 0.5f) == doctest::Approx(0.5f));  // on the hip
    CHECK(SurfaceHeightAt(outer, 1.0f, 1.0f) == doctest::Approx(0.0f));
    const double hip_angle = std::atan(1.0 / std::sqrt(2.0)) * 180.0 / 3.14159265358979;
    CHECK(hip_angle == doctest::Approx(35.264).epsilon(1e-4));
  }

  TEST_CASE("inverted variants hang from the ceiling") {
    const MaterialId up = Slope("wedge", "east", "bottom");
    const MaterialId down = Slope("wedge", "east", "top");
    for (float x : {0.0f, 0.25f, 0.5f, 1.0f}) {
      const SolidSpan a = SolidSpanAt(ShapeOf(up), x, 0.5f);
      const SolidSpan b = SolidSpanAt(ShapeOf(down), x, 0.5f);
      CHECK(a.lo == 0.0f);
      CHECK(b.hi == 1.0f);
      CHECK(a.hi - a.lo == doctest::Approx(b.hi - b.lo));
    }
  }

  TEST_CASE("slope families are in the registry with canonical strings that round-trip") {
    std::size_t slopes = 0, slabs = 0;
    for (MaterialId m = 0; m < Materials::kCount; ++m) {
      const std::string name(StateString(m));
      CAPTURE(name);
      CHECK(ParseState(name) == m);
      if (name.find("_slope[") != std::string::npos) ++slopes;
      if (name.find("_slab[") != std::string::npos) ++slabs;
    }
    CHECK(slopes == 8 * 144);  // 9 shapes × 4 facings × 2 halves × 2 flooded, per material
    CHECK(slabs == 8 * 4);
    CHECK(*ParseState("dwell:stone_slope[shape=outer,facing=south,half=top,flooded=true]") ==
          *ParseState("dwell:stone_slope[facing=south,flooded=true,half=top,shape=outer]"));
    CHECK(StateString(*ParseState("dwell:stone_slope")) ==
          "dwell:stone_slope[facing=north,flooded=false,half=bottom,shape=wedge]");
    // Shapes link to their material: density and look come from the base block.
    CHECK(GetMaterial(*ParseState("dwell:dirt_slope")).density_kg_m3 ==
          GetMaterial(Materials::kDirt).density_kg_m3);
  }

  TEST_CASE("flooded states are flagged, others are not") {
    CHECK(GetMaterial(Slope("wedge", "east", "bottom", true)).flooded);
    CHECK_FALSE(GetMaterial(Slope("wedge", "east", "bottom", false)).flooded);
    CHECK_FALSE(GetMaterial(Materials::kStone).flooded);
    CHECK(ShapeOf(Slope("wedge", "east", "bottom", true)).face_count ==
          ShapeOf(Slope("wedge", "east", "bottom", false)).face_count);
  }

  TEST_CASE("rays enter slopes through their sloped face") {
    const MaterialId wedge = Slope("wedge", "east", "bottom");  // z = 1 − x, 45°
    const float cell[3] = {0, 0, 0};
    const float down[3] = {0, -1, 0};
    const float from_above[3] = {0.25f, 2.0f, 0.5f};
    const auto hit = RayEnterShape(ShapeOf(wedge), cell, from_above, down, 10.0f);
    REQUIRE(hit);
    CHECK(hit->t == doctest::Approx(2.0f - 0.75f));
    CHECK(hit->normal[0] == doctest::Approx(std::sqrt(0.5f)));
    CHECK(hit->normal[1] == doctest::Approx(std::sqrt(0.5f)));
    CHECK(hit->normal[2] == doctest::Approx(0.0f));
    // Above the low edge there is nothing to hit within the cell's height.
    const float low_edge[3] = {0.999f, 2.0f, 0.5f};
    const auto near_zero = RayEnterShape(ShapeOf(wedge), cell, low_edge, down, 10.0f);
    REQUIRE(near_zero);
    CHECK(near_zero->t == doctest::Approx(2.0f - 0.001f).epsilon(1e-3));
    // A ray from the low side, level, hits the sloped face from the side it faces.
    const float level_origin[3] = {2.0f, 0.5f, 0.5f};
    const float west[3] = {-1, 0, 0};
    const auto level = RayEnterShape(ShapeOf(wedge), cell, level_origin, west, 10.0f);
    REQUIRE(level);
    CHECK(level->t == doctest::Approx(1.5f));  // surface at x = 0.5
    // A ray inside the solid does not hit its own surface from within.
    const float inside[3] = {0.1f, 0.5f, 0.5f};
    CHECK_FALSE(RayEnterShape(ShapeOf(wedge), cell, inside, west, 10.0f));
  }

  TEST_CASE("a capsule axis against a slope: clearance above the surface, contact below it") {
    const MaterialId wedge = Slope("wedge", "east", "bottom");
    const ShapeInfo& s = ShapeOf(wedge);
    // Over the high side (surface height 1 − x) at x = 0.25: surface y = 0.75.
    CHECK(VerticalSegmentDistanceSq(s, 0.25f, 0.5f, 1.0f, 2.0f) > 0.0f);
    CHECK(VerticalSegmentDistanceSq(s, 0.25f, 0.5f, 1.0f, 2.0f) ==
          doctest::Approx(0.25f * 0.25f * 0.5f).epsilon(0.05));  // ⟂ distance ≈ 0.177 → ²= 0.03125
    CHECK(VerticalSegmentDistanceSq(s, 0.25f, 0.5f, 0.5f, 2.0f) == 0.0f);  // crosses the surface
    CHECK(VerticalSegmentDistanceSq(s, 0.9f, 0.5f, 0.2f, 0.3f) > 0.0f);    // over the low edge: air
    CHECK(VerticalSegmentDistanceSq(s, 0.9f, 0.5f, -0.5f, 0.05f) == 0.0f);  // touches y ≤ 0.1
  }
}

// ---- Exhaustive adjacency ---------------------------------------------------------------------

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
    REQUIRE(distinct.size() >= 70);
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
    CHECK(checked > 1000000);
  }
}

}  // namespace dwell::core
