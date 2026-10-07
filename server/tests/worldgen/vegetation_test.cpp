// Wetland ponds and colourful vegetation (WORLD_GENERATION.md §3.5, §3.7, Phase 11c): ponds hold
// still water that never floats or spills; the biomes' grass and foliage tints blend across borders
// and agree between neighbouring chunks and the level of detail; a distant forest keeps its canopy.
// Runs natively (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <unordered_map>
#include <utility>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/biomes.h"
#include "dwell/worldgen/rivers.h"
#include "dwell/worldgen/terrain.h"

#include "biome_search.h"

using namespace dwell;
using worldgen::Biome;
using worldgen::Column;
using worldgen::Feature;
using worldgen::TerrainGenerator;
namespace M = core::Materials;
using core::MaterialId;

namespace {

bool IsLeaf(MaterialId m) { return m == M::kLeaves; }
bool Open(MaterialId m) { return m == M::kAir; }

}  // namespace

TEST_SUITE("worldgen: vegetation") {
  TEST_CASE("wetland ponds hold still water: none floats, none spills") {
    long pond_columns = 0, water_voxels = 0, floating = 0, spilling = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      const TerrainGenerator gen(seed);
      const auto site = testing::FindBiome(gen, Biome::kWetland);
      REQUIRE(site);
      core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
      // Ponds in a 700 m square of the wetland (they are 5–16 m in radius, one in two cells of 128
      // m).
      std::vector<std::pair<std::int32_t, std::int32_t>> ponds;
      for (std::int32_t z = site->second - 350; z < site->second + 350; z += 2)
        for (std::int32_t x = site->first - 350; x < site->first + 350; x += 2) {
          if (gen.ColumnAt(x, z).pond) ponds.push_back({x, z});
        }
      pond_columns += static_cast<long>(ponds.size());
      // Check the water voxels of a sample of pond columns and their neighbours.
      for (std::size_t i = 0; i < ponds.size(); i += 7) {
        const auto [px, pz] = ponds[i];
        for (int dz = -1; dz <= 1; ++dz)
          for (int dx = -1; dx <= 1; ++dx) {
            const std::int32_t x = px + dx, z = pz + dz;
            const Column col = gen.ColumnAt(x, z);
            for (std::int32_t y = col.water - 4; y <= col.water + 1; ++y) {
              if (world.GetVoxel(x, y, z) != M::kWater) continue;
              ++water_voxels;
              // Below: solid or water. Beside: never open air (a pond has no steps).
              const MaterialId below = world.GetVoxel(x, y - 1, z);
              floating += Open(below);
              for (const auto& [ax, az] :
                   {std::pair{1, 0}, std::pair{-1, 0}, std::pair{0, 1}, std::pair{0, -1}}) {
                spilling += Open(world.GetVoxel(x + ax, y, z + az));
              }
            }
          }
      }
    }
    MESSAGE(pond_columns << " pond columns, " << water_voxels << " water voxels sampled; "
                         << floating << " floating, " << spilling << " spilling");
    CHECK(pond_columns > 200);
    CHECK(water_voxels > 200);
    CHECK(floating == 0);
    CHECK(spilling == 0);
  }

  TEST_CASE("ponds stand in wetland, flat and wet, not in the water of rivers or lakes") {
    const TerrainGenerator gen(1);
    const auto site = testing::FindBiome(gen, Biome::kWetland);
    REQUIRE(site);
    long ponds = 0, not_wet = 0, steep = 0;
    for (std::int32_t z = site->second - 600; z < site->second + 600; z += 3)
      for (std::int32_t x = site->first - 600; x < site->first + 600; x += 3) {
        const Column c = gen.ColumnAt(x, z);
        if (!c.pond) continue;
        ++ponds;
        not_wet += c.humidity < 0.4f;  // the oracle reads the pond's centre: a little slack
        steep += c.height - c.valley > worldgen::rivers::kPondRelief + 2.0f;
        CHECK_FALSE(c.lake);
      }
    MESSAGE(ponds << " pond columns");
    CHECK(ponds > 50);
    CHECK(not_wet == 0);
    CHECK(steep == 0);
  }

  TEST_CASE("biome tints: a chunk's grid agrees with its neighbours' on the shared edge") {
    const TerrainGenerator gen(1);
    int checked = 0;
    for (std::int32_t cz = -6; cz <= 6; cz += 3)
      for (std::int32_t cx = -200; cx <= 200; cx += 41) {
        const auto a = gen.TintGrid(cx, cz), east = gen.TintGrid(cx + 1, cz),
                   south = gen.TintGrid(cx, cz + 1);
        constexpr int N = TerrainGenerator::kTintOutPoints, S = TerrainGenerator::kTintStride;
        for (int j = 0; j < N; ++j)
          for (int c = 0; c < S; ++c) {
            // The chunk's x = 32 column of points is the east neighbour's x = 0.
            CHECK(a[static_cast<std::size_t>((j * N + (N - 1)) * S + c)] ==
                  east[static_cast<std::size_t>((j * N) * S + c)]);
            // And its z = 32 row the south neighbour's z = 0.
            CHECK(a[static_cast<std::size_t>(((N - 1) * N + j) * S + c)] ==
                  south[static_cast<std::size_t>(j * S + c)]);
            ++checked;
          }
      }
    CHECK(checked > 100);
  }

  TEST_CASE(
      "biome tints blend: neighbouring points differ by a third of the biomes' range at most") {
    long steps = 0, big = 0;
    int worst = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      const TerrainGenerator gen(seed);
      for (std::int32_t cz = -400; cz <= 400; cz += 37)
        for (std::int32_t cx = -9000; cx <= 9000; cx += 997) {
          const auto g = gen.TintGrid(cx, cz);
          constexpr int N = TerrainGenerator::kTintOutPoints, S = TerrainGenerator::kTintStride;
          for (int j = 0; j < N; ++j)
            for (int i = 0; i + 1 < N; ++i)
              for (int c = 0; c < S; ++c) {
                const int d = std::abs(int{g[static_cast<std::size_t>((j * N + i) * S + c)]} -
                                       int{g[static_cast<std::size_t>((j * N + i + 1) * S + c)]});
                worst = std::max(worst, d);
                ++steps;
                big += d > 50;  // a third of the widest range, 154 − 35
              }
        }
    }
    MESSAGE(steps << " steps, widest " << worst << " (1/64 units); " << big << " over 50");
    CHECK(steps > 1000);
    CHECK(big == 0);
  }

  TEST_CASE("inside a biome the tint is the biome's: the table's colours reach the grid") {
    const TerrainGenerator gen(0);
    for (const Biome b : {Biome::kMeadow, Biome::kAutumnWoods, Biome::kBlossomGrove,
                          Biome::kConifer, Biome::kSavanna}) {
      const auto site = testing::FindBiomeInterior(gen, b);
      REQUIRE(site);
      const auto& def = worldgen::BiomeOf(b);
      // A chunk well inside the region: the grid is all within a few units of the biome's tint.
      int near = 0, total = 0;
      for (int dz = -4; dz <= 4; ++dz)
        for (int dx = -4; dx <= 4; ++dx) {
          const auto g = gen.TintGrid(worldgen::FloorDiv(site->first, 32) + dx * 8,
                                      worldgen::FloorDiv(site->second, 32) + dz * 8);
          ++total;
          near +=
              std::abs(int{g[0]} - def.grass.r) <= 3 && std::abs(int{g[3]} - def.foliage.r) <= 3;
        }
      CAPTURE(worldgen::BiomeName(b));
      CHECK(near * 10 > total * 8);  // inside a region (its borders and neighbours are further out)
    }
    // The colours say what they should: autumn leaves orange, blossom pink, conifers dark, grass in
    // dry biomes golden — against the plain green tile (64 = unchanged).
    const auto& autumn = worldgen::BiomeOf(Biome::kAutumnWoods);
    CHECK(autumn.foliage.r > autumn.foliage.g * 2);
    const auto& blossom = worldgen::BiomeOf(Biome::kBlossomGrove);
    CHECK(blossom.foliage.r > 100);
    CHECK(blossom.foliage.b > blossom.foliage.g * 3);
    const auto& conifer = worldgen::BiomeOf(Biome::kConifer);
    CHECK(conifer.foliage.r < worldgen::kTintUnit);
    const auto& savanna = worldgen::BiomeOf(Biome::kSavanna);
    CHECK(savanna.grass.r > worldgen::kTintUnit);
    CHECK(savanna.grass.g < savanna.grass.r);
  }

  TEST_CASE("the level of detail carries the tint of its columns, agreeing with the chunks'") {
    const TerrainGenerator gen(0);
    const auto site = testing::FindBiomeInterior(gen, Biome::kAutumnWoods);
    REQUIRE(site);
    const core::LodCoord c{
        3, static_cast<std::int32_t>((site->first - core::kLodOriginX) / core::LodSectionSize(3)),
        static_cast<std::int32_t>(
            (std::max<std::int64_t>(
                 static_cast<std::int64_t>(gen.ColumnAt(site->first, site->second).height), 0) -
             core::kLodOriginY) /
            core::LodSectionSize(3)),
        static_cast<std::int32_t>((site->second - core::kLodOriginZ) / core::LodSectionSize(3))};
    core::LodCells cells;
    core::LodSurfaces surface;
    REQUIRE(gen.GenerateLod(c, cells, &surface) == core::LodKind::kContent);
    // The column at the site: its tint is near the chunk grid's there (both blur the biomes').
    const core::LodOrigin o = core::LodSectionOrigin(c);
    const int x = static_cast<int>((site->first - o.x) / 8),
              z = static_cast<int>((site->second - o.z) / 8);
    const auto& s = surface[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)];
    const auto g =
        gen.TintGrid(worldgen::FloorDiv(site->first, 32), worldgen::FloorDiv(site->second, 32));
    const int lod_r = static_cast<int>(s.tint_foliage >> 16), chunk_r = g[3];
    MESSAGE("foliage red: level of detail " << lod_r << ", chunk grid " << chunk_r);
    CHECK(s.tint_grass != 0);
    CHECK(std::abs(lod_r - chunk_r) <= 6);
  }

  TEST_CASE("a distant forest keeps its colour: a forested site's level-4 surface is leaves") {
    const TerrainGenerator gen(0);
    for (const Biome b : {Biome::kBroadleaf, Biome::kConifer}) {
      const auto site = testing::FindBiome(gen, b);
      REQUIRE(site);
      // Level 4: 16 m cells, above the cells real trees are drawn in (4 m).
      const core::LodCoord c{
          4, static_cast<std::int32_t>((site->first - core::kLodOriginX) / core::LodSectionSize(4)),
          static_cast<std::int32_t>(
              (std::max<std::int64_t>(
                   static_cast<std::int64_t>(gen.ColumnAt(site->first, site->second).height), 0) -
               core::kLodOriginY) /
              core::LodSectionSize(4)),
          static_cast<std::int32_t>((site->second - core::kLodOriginZ) / core::LodSectionSize(4))};
      core::LodCells cells;
      core::LodSurfaces surface;
      REQUIRE(gen.GenerateLod(c, cells, &surface) == core::LodKind::kContent);
      const core::LodOrigin o = core::LodSectionOrigin(c);
      long forest = 0, leaf = 0;
      for (int z = 0; z < core::kLodSectionCells; ++z)
        for (int x = 0; x < core::kLodSectionCells; ++x) {
          const auto& s = surface[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)];
          if (!s.valid || s.wet) continue;
          const Column col = gen.ColumnAt(static_cast<std::int32_t>(o.x + x * 16 + 8),
                                          static_cast<std::int32_t>(o.z + z * 16 + 8));
          if (col.biome != b || col.wet > 0.0f) continue;
          ++forest;
          leaf += IsLeaf(s.material);
        }
      CAPTURE(worldgen::BiomeName(b));
      MESSAGE(worldgen::BiomeName(b)
              << ": " << leaf << " of " << forest << " forested columns show leaves");
      REQUIRE(forest > 50);
      CHECK(leaf * 10 > forest * 6);  // mostly (steep ground stays rock)
    }
  }
}
