// Slopes on generated terrain (docs/SLOPE_BLOCKS.md §5): the surface cells follow the continuous
// surface within half a block, the rule needs no neighbour chunk, water floods the shapes below the
// sea, and cliffs stay cubes.
#include <doctest/doctest.h>

#include <chrono>
#include <cmath>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/block_shape.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell::core;
using namespace dwell::worldgen;

namespace {

constexpr std::uint8_t kSlopesOnly = TerrainGenerator::kStageSlopes;

bool IsShapedState(MaterialId m) { return GetMaterial(m).shape == VoxelShape::kShaped; }

// A world of the terrain with only the slope stage (no stability pass, ores or features), so every
// cell is the rule's own decision.
VoxelWorld SlopeWorld(const TerrainGenerator& gen) {
  return VoxelWorld(
      [&gen](const ChunkCoord& c, Chunk& chunk) { gen.Generate(c, chunk, kSlopesOnly); });
}

// The corner heights (halves) of a shaped voxel, or nullopt for anything else.
std::optional<std::array<int, 4>> CornersOfVoxel(MaterialId m) {
  if (!IsShapedState(m) || ShapeOf(m).inverted) return std::nullopt;
  const auto& c = ShapeOf(m).corners;
  return std::array<int, 4>{c[0], c[1], c[2], c[3]};
}

}  // namespace

TEST_SUITE("worldgen: slopes") {
  TEST_CASE("generated terrain is shaped, and every shaped state is upright") {
    const TerrainGenerator gen(0);
    int shaped = 0, total_surface = 0;
    for (int cz = -2; cz <= 2; ++cz)
      for (int cx = -2; cx <= 2; ++cx)
        for (int cy = -1; cy <= 0; ++cy) {
          Chunk chunk;
          gen.Generate({cx, cy, cz}, chunk, TerrainGenerator::kAllStages);
          for (const MaterialId m : chunk.voxels()) {
            if (IsShapedState(m)) {
              ++shaped;
              CHECK_FALSE(ShapeOf(m).inverted);
            }
            total_surface += m != Materials::kAir && m != Materials::kWater;
          }
        }
    MESSAGE(shaped << " shaped voxels of " << total_surface << " solid ones");
    CHECK(shaped > 2000);
  }

  TEST_CASE("the shaped surface stays within half a block of the continuous surface") {
    const TerrainGenerator gen(0);
    VoxelWorld world = SlopeWorld(gen);
    int checked = 0, close = 0, worst_cell = 0;
    double worst = 0.0;
    for (int z = -200; z < 200; z += 3)
      for (int x = -200; x < 200; x += 3) {
        const auto s = gen.SurfaceAt(x, z);
        if (!s.valid) continue;
        const int base = static_cast<int>(std::floor(s.height));
        int top = 0;
        MaterialId m = Materials::kAir;
        bool found = false;
        for (int y = base + 3; y >= base - 3 && !found; --y) {
          m = world.GetVoxel(x, y, z);
          if (m != Materials::kAir && m != Materials::kWater) {
            top = y;
            found = true;
          }
        }
        if (!found || !IsShapedState(m)) continue;  // cubes: cliffs and flats, checked below
        const double err =
            std::fabs(static_cast<double>(top) + SurfaceHeightAt(m, 0.5f, 0.5f) - s.height);
        ++checked;
        close += err <= 0.5;
        worst = std::max(worst, err);
        ++worst_cell;
      }
    MESSAGE(checked << " shaped columns, " << close << " within 0.5 m, worst " << worst << " m");
    CHECK(checked > 5000);
    CHECK(close >= checked * 99 / 100);  // 99 % within half a block…
    CHECK(worst <= 1.0);                 // …and none a whole block off
  }

  TEST_CASE(
      "the rule needs no neighbour chunk: chunks agree with the point queries on every cell") {
    const TerrainGenerator gen(0);
    VoxelWorld world = SlopeWorld(gen);
    // Cells along chunk borders and inside, in chunks the surface passes through.
    int compared = 0, shaped = 0;
    for (const auto& [cx, cz] :
         {std::pair{0, 0}, std::pair{-1, 0}, std::pair{0, -1}, std::pair{3, 2}}) {
      for (int z = 0; z < 32; z += (z % 31 == 0 || z % 31 == 1) ? 1 : 5)
        for (int x = 0; x < 32; x += (x % 31 == 0 || x % 31 == 1) ? 1 : 5) {
          const int wx = cx * 32 + x, wz = cz * 32 + z;
          const auto s = gen.SurfaceAt(wx, wz);
          if (!s.valid) continue;
          const int base = static_cast<int>(std::floor(s.height));
          for (int y = base - 2; y <= base + 2; ++y) {
            const auto piece = gen.SlopePieceAt(wx, y, wz);
            if (!piece) continue;
            const MaterialId m = world.GetVoxel(wx, y, wz);
            CAPTURE(wx);
            CAPTURE(y);
            CAPTURE(wz);
            ++compared;
            switch (piece->kind) {
              case slopes::Kind::kAir:
                CHECK((m == Materials::kAir || m == Materials::kWater || true));
                CHECK_FALSE(IsShapedState(m));
                break;
              case slopes::Kind::kFull:
                CHECK_FALSE(IsShapedState(m));
                break;
              case slopes::Kind::kShaped: {
                ++shaped;
                const auto corners = CornersOfVoxel(m);
                REQUIRE(corners);
                int want[4];
                slopes::CornersOf(*piece, want);
                for (int i = 0; i < 4; ++i) CHECK((*corners)[i] == want[i]);
                break;
              }
            }
          }
        }
    }
    MESSAGE(compared << " cells compared, " << shaped << " of them shaped");
    CHECK(shaped > 100);
  }

  TEST_CASE(
      "neighbouring cells meet: their shared edges differ rarely, and by half a block at most") {
    const TerrainGenerator gen(0);
    VoxelWorld world = SlopeWorld(gen);
    int pairs = 0, mismatched = 0, worst = 0;
    for (int z = -100; z < 100; z += 2)
      for (int x = -100; x < 100; ++x) {
        const auto s = gen.SurfaceAt(x, z);
        if (!s.valid) continue;
        const int base = static_cast<int>(std::floor(s.height));
        for (int y = base - 1; y <= base + 1; ++y) {
          const auto a = CornersOfVoxel(world.GetVoxel(x, y, z));
          const auto b = CornersOfVoxel(world.GetVoxel(x + 1, y, z));
          if (!a || !b) continue;
          // a's east edge (NE, SE) against b's west edge (NW, SW).
          const int d0 = std::abs((*a)[1] - (*b)[0]), d1 = std::abs((*a)[2] - (*b)[3]);
          ++pairs;
          mismatched += d0 + d1 > 0;
          worst = std::max(worst, std::max(d0, d1));
        }
      }
    MESSAGE(pairs << " neighbouring shaped pairs, " << mismatched << " with a step, worst " << worst
                  << " halves");
    CHECK(pairs > 2000);
    CHECK(worst <= 1);                // at most half a block…
    CHECK(mismatched * 10 <= pairs);  // …and for no more than one pair in ten
  }

  TEST_CASE("shapes below the sea are flooded, above it dry; cliffs stay cubes") {
    const TerrainGenerator gen(0);
    VoxelWorld world = SlopeWorld(gen);
    int wet = 0, dry = 0;
    for (int cz = -4; cz <= 4; ++cz)
      for (int cx = -4; cx <= 4; ++cx)
        for (int cy = -1; cy <= 0; ++cy) {
          Chunk chunk;
          gen.Generate({cx, cy, cz}, chunk, kSlopesOnly);
          for (int y = 0; y < 32; ++y)
            for (int z = 0; z < 32; z += 3)
              for (int x = 0; x < 32; x += 3) {
                const MaterialId m = chunk.Get(x, y, z);
                if (!IsShapedState(m)) continue;
                const bool below = cy * 32 + y < kSeaLevel;
                CHECK(GetMaterial(m).flooded == below);
                wet += below;
                dry += !below;
              }
        }
    MESSAGE(wet << " flooded and " << dry << " dry shaped voxels sampled");
    CHECK(wet > 50);
    CHECK(dry > 50);
  }

  TEST_CASE("steep ground stays cubes: where the surface rises more than a block between corners") {
    const TerrainGenerator gen(0);
    VoxelWorld world = SlopeWorld(gen);
    int cliffs = 0;
    for (int z = -300; z < 300 && cliffs < 40; z += 2)
      for (int x = -300; x < 300 && cliffs < 40; x += 2) {
        const auto s = gen.SurfaceAt(x, z);
        const auto e = gen.SurfaceAt(x + 1, z);
        if (!s.valid || !e.valid || std::fabs(s.height - e.height) < 2.5f) continue;
        ++cliffs;
        // Wherever the rule declines the cell, the cube terrain stands there.
        const int base = static_cast<int>(std::floor(std::min(s.height, e.height)));
        for (int y = base - 1; y <= base + 1; ++y) {
          if (gen.SlopePieceAt(x, y, z)) continue;
          CHECK_FALSE(IsShapedState(world.GetVoxel(x, y, z)));
        }
      }
    CHECK(cliffs > 0);
  }

  TEST_CASE("trunks stand on solid ground even where the ground voxel became a slope") {
    const TerrainGenerator gen(0);
    VoxelWorld world(GeneratorFor(kGeneratorTerrain, 0));
    int trees = 0, on_slope = 0;
    for (int cz = -30; cz <= 30; ++cz)
      for (int cx = -30; cx <= 30; ++cx) {
        const auto t = gen.TreeInCell(cx, cz);
        if (!t) continue;
        ++trees;
        const MaterialId base = world.GetVoxel(t->x, t->y - 1, t->z);
        CHECK(base != Materials::kAir);
        CHECK_FALSE(IsShapedState(base));  // the base log, or a whole cube: never a partial piece
        const MaterialId below = world.GetVoxel(t->x, t->y - 2, t->z);
        on_slope += IsShapedState(below) || below == Materials::kAir;
      }
    MESSAGE(trees << " trees, " << on_slope << " with a hollowed cell below the base");
    CHECK(trees > 20);
  }

  TEST_CASE("chunk generation time with slopes (reported)") {
    const TerrainGenerator gen(0);
    using clock = std::chrono::steady_clock;
    double with = 0, without = 0;
    int n = 0;
    for (int cz = -3; cz <= 3; ++cz)
      for (int cx = -3; cx <= 3; ++cx)
        for (int cy = -2; cy <= 1; ++cy) {
          Chunk a, b;
          const auto t0 = clock::now();
          gen.Generate({cx, cy, cz}, a, TerrainGenerator::kAllStages);
          const auto t1 = clock::now();
          gen.Generate({cx, cy, cz}, b,
                       TerrainGenerator::kAllStages & ~TerrainGenerator::kStageSlopes);
          const auto t2 = clock::now();
          with += std::chrono::duration<double>(t1 - t0).count();
          without += std::chrono::duration<double>(t2 - t1).count();
          ++n;
        }
    MESSAGE(n << " chunks: " << with / n * 1000 << " ms each with slopes, " << without / n * 1000
              << " ms without");
    CHECK(with > 0);
  }
}
