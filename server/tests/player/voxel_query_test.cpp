// VoxelQuery's exact-shape ray cast against the box ray cast it replaced (slabs and cubes must
// answer exactly as before), and slope ray casts (docs/SLOPE_BLOCKS.md §4).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <random>

#include "dwell/core/block_registry.h"
#include "dwell/core/block_shape.h"
#include "dwell/player/voxel_query.h"
#include "player_test_world.h"

namespace dwell::test {
namespace {

using namespace dwell::core;
using player::ProbeHit;
using player::VoxelQuery;

constexpr float kInf = std::numeric_limits<float>::infinity();

// The box-based Amanatides–Woo cast this replaced: a cell's shape is the box [0,1]×[0,h]×[0,1],
// h = 1 for a cube, ½ for a slab.
bool ReferenceCast(const VoxelQuery& q, player::RVec3 origin, player::Vec3 dir, float max_distance,
                   ProbeHit& hit) {
  std::int32_t base[3];
  float o[3];
  for (int i = 0; i < 3; ++i) {
    base[i] = static_cast<std::int32_t>(std::floor(origin[i]));
    o[i] = static_cast<float>(origin[i] - static_cast<double>(base[i]));
  }
  float d[3] = {dir.GetX(), dir.GetY(), dir.GetZ()};
  std::int32_t cell[3], step[3];
  float t_max[3], t_delta[3];
  for (int i = 0; i < 3; ++i) {
    cell[i] = static_cast<std::int32_t>(std::floor(o[i]));
    if (d[i] > 0.0f) {
      step[i] = 1;
      t_delta[i] = 1.0f / d[i];
      t_max[i] = (static_cast<float>(cell[i]) + 1.0f - o[i]) / d[i];
    } else if (d[i] < 0.0f) {
      step[i] = -1;
      t_delta[i] = -1.0f / d[i];
      t_max[i] = (static_cast<float>(cell[i]) - o[i]) / d[i];
    } else {
      step[i] = 0;
      t_delta[i] = kInf;
      t_max[i] = kInf;
    }
  }
  bool in_solid = true;
  float t_cell = 0.0f;
  int entry_axis = -1;
  while (t_cell <= max_distance) {
    const float t_next = std::min({t_max[0], t_max[1], t_max[2]});
    const auto& material =
        GetMaterial(q.Material(base[0] + cell[0], base[1] + cell[1], base[2] + cell[2]));
    const float height = material.shape == VoxelShape::kFull     ? 1.0f
                         : material.shape == VoxelShape::kShaped ? 0.5f
                                                                 : 0.0f;
    bool free_after = true;
    if (height > 0.0f) {
      float e0 = -kInf, e1 = kInf;
      int axis0 = -1;
      bool miss = false;
      for (int i = 0; i < 3 && !miss; ++i) {
        const float lo = static_cast<float>(cell[i]);
        const float hi = lo + (i == 1 ? height : 1.0f);
        if (d[i] == 0.0f) {
          if (o[i] < lo || o[i] > hi) miss = true;
          continue;
        }
        float a = (lo - o[i]) / d[i], b = (hi - o[i]) / d[i];
        if (a > b) std::swap(a, b);
        if (a > e0) {
          e0 = a;
          axis0 = i;
        }
        e1 = std::min(e1, b);
      }
      e0 = std::max(e0, t_cell);
      e1 = std::min(e1, t_next);
      if (!miss && e0 <= e1) {
        constexpr float kEps = 1e-6f;
        const bool enters_from_free = e0 > t_cell + kEps || !in_solid;
        if (enters_from_free) {
          if (e0 > max_distance) return false;
          const int axis = e0 > t_cell + kEps ? axis0 : entry_axis;
          float n[3] = {0.0f, 0.0f, 0.0f};
          if (axis >= 0) n[axis] = d[axis] > 0.0f ? -1.0f : 1.0f;
          hit.distance = e0;
          hit.point = origin + dir * e0;
          hit.normal = player::Vec3(n[0], n[1], n[2]);
          return true;
        }
        free_after = e1 < t_next - kEps;
      }
    }
    in_solid = !free_after;
    int axis = 0;
    if (t_max[1] < t_max[axis]) axis = 1;
    if (t_max[2] < t_max[axis]) axis = 2;
    if (t_max[axis] == kInf) break;
    t_cell = t_max[axis];
    t_max[axis] += t_delta[axis];
    cell[axis] += step[axis];
    entry_axis = axis;
  }
  return false;
}

}  // namespace

TEST_SUITE("player: voxel query") {
  TEST_CASE("axis-aligned rays from grid points answer like the box cast") {
    PlayerTestWorld w;
    std::mt19937 rng(11);
    for (int z = -4; z <= 4; ++z) {
      for (int y = -2; y <= 2; ++y) {
        for (int x = -4; x <= 4; ++x) {
          const unsigned r = rng() % 6;
          if (r == 0) w.Set(x, y, z, Materials::kStone);
          if (r == 1) w.Set(x, y, z, Materials::kStoneSlab);
        }
      }
    }
    VoxelQuery q(w.world, w.physics.system());
    const player::Vec3 dirs[] = {{1, 0, 0}, {-1, 0, 0},  {0, 1, 0}, {0, -1, 0},
                                 {0, 0, 1}, {0, 0, -1},  {1, 1, 0}, {1, -1, 0},
                                 {0, 1, 1}, {-1, -1, 1}, {1, 1, 1}};
    int compared = 0;
    for (int zi = -16; zi <= 16; ++zi) {
      for (int yi = -8; yi <= 8; ++yi) {
        for (int xi = -16; xi <= 16; ++xi) {
          const player::RVec3 origin = ToWorld(player::Vec3(xi * 0.25f, yi * 0.25f, zi * 0.25f));
          for (const player::Vec3& raw : dirs) {
            const player::Vec3 d = raw.Normalized();
            ProbeHit expected, actual;
            const bool e = ReferenceCast(q, origin, d, 6.0f, expected);
            const bool a = q.CastVoxels(origin, d, 6.0f, actual);
            ++compared;
            INFO("origin " << xi * 0.25f << "," << yi * 0.25f << "," << zi * 0.25f << " dir "
                           << raw.GetX() << "," << raw.GetY() << "," << raw.GetZ());
            // A ray starting exactly on a cell's edge or corner grazes cells and has no one answer
            // about whether it is already inside one.
            const bool edge_start = (xi % 4 == 0) + (yi % 4 == 0) + (zi % 4 == 0) >= 2;
            // So does a ray running in a plane that is a cell face (or a slab's top).
            const int cells[3] = {xi, yi, zi};
            bool in_plane = false;
            for (int k = 0; k < 3; ++k) in_plane |= raw[k] == 0.0f && cells[k] % 2 == 0;
            if ((edge_start || in_plane) && a != e) continue;
            if (in_plane || edge_start) continue;
            // A hit on a cell's edge (the ray grazes it) may go either way.
            const auto on_edge = [&](const player::RVec3& at) {
              int n = 0;
              for (int k = 0; k < 3; ++k) {
                const double f = at[k] - std::floor(at[k]);
                if (f < 1e-3 || f > 1.0 - 1e-3 || std::fabs(f - 0.5) < 1e-3) ++n;
              }
              return n >= 2;
            };
            if ((e && on_edge(origin + d * expected.distance)) || (a && on_edge(actual.point))) {
              continue;
            }
            INFO("new " << a << " d=" << actual.distance << " n=" << actual.normal.GetX() << ","
                        << actual.normal.GetY() << "," << actual.normal.GetZ() << " pt "
                        << actual.point.GetX() << "," << actual.point.GetY() << ","
                        << actual.point.GetZ());
            REQUIRE(a == e);
            if (!e) continue;
            CHECK(actual.distance == doctest::Approx(expected.distance).epsilon(1e-4));
            CHECK(actual.normal == expected.normal);
          }
        }
      }
    }
    MESSAGE(compared << " rays");
  }

  TEST_CASE("the exact-shape ray cast answers like the box cast on cubes and slabs") {
    PlayerTestWorld w;
    std::mt19937 rng(7);
    for (int z = -6; z <= 6; ++z) {
      for (int y = -3; y <= 3; ++y) {
        for (int x = -6; x <= 6; ++x) {
          const unsigned r = rng() % 8;
          if (r == 0) w.Set(x, y, z, Materials::kStone);
          if (r == 1) w.Set(x, y, z, Materials::kStoneSlab);
          if (r == 2) w.Set(x, y, z, Materials::kGrass);
        }
      }
    }
    VoxelQuery q(w.world, w.physics.system());
    std::uniform_real_distribution<float> pos(-5.0f, 5.0f), dir(-1.0f, 1.0f);
    int compared = 0, hits = 0;
    for (int i = 0; i < 20000; ++i) {
      const player::RVec3 origin = ToWorld(player::Vec3(pos(rng), pos(rng) * 0.5f, pos(rng)));
      player::Vec3 d(dir(rng), dir(rng), dir(rng));
      if (d.LengthSq() < 1e-4f) continue;
      d = d.Normalized();
      ProbeHit expected, actual;
      const bool e = ReferenceCast(q, origin, d, 12.0f, expected);
      const bool a = q.CastVoxels(origin, d, 12.0f, actual);
      ++compared;
      hits += e;
      INFO("ray " << i << " origin " << origin.GetX() << "," << origin.GetY() << ","
                  << origin.GetZ() << " dir " << d.GetX() << "," << d.GetY() << "," << d.GetZ());
      INFO("expected " << expected.distance << " normal " << expected.normal.GetX() << ","
                       << expected.normal.GetY() << "," << expected.normal.GetZ());
      // A hit within a hair of a cell's edge has no one answer.
      int near_edge = 0;
      if (e) {
        const player::RVec3 at = origin + d * expected.distance;
        for (int k = 0; k < 3; ++k) {
          const double f = at[k] - std::floor(at[k]);
          if (f < 1e-3 || f > 1.0 - 1e-3 || std::fabs(f - 0.5) < 1e-3) ++near_edge;
        }
      }
      if (a != e && near_edge >= 2) continue;
      REQUIRE(a == e);
      if (!e) continue;
      CHECK(actual.distance == doctest::Approx(expected.distance).epsilon(1e-4));
      if (near_edge < 2) CHECK(actual.normal == expected.normal);
    }
    MESSAGE(compared << " rays, " << hits << " hits");
    CHECK(hits > 1000);
  }
}

}  // namespace dwell::test
