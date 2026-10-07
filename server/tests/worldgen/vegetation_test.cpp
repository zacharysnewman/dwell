// Wetland ponds and colourful vegetation (WORLD_GENERATION.md §3.5, §3.7, Phase 11c): ponds hold
// still water that never floats or spills; accent trees come in clumps; a distant forest keeps its
// canopy colour; the new leaf and grass blocks are placeable. Runs natively (dwell_tests) and under
// Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <cmath>
#include <cstdint>
#include <set>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

#include "dwell/core/block_edit.h"
#include "dwell/core/block_registry.h"
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

bool IsLeaf(MaterialId m) { return m >= M::kLeaves && m <= M::kLeavesViolet; }
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

  TEST_CASE("accent trees come in clumps: an accent's nearest tree is far more often an accent") {
    long trees = 0, accents = 0, accents_next_to_accents = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      const TerrainGenerator gen(seed);
      const auto site = testing::FindBiome(gen, Biome::kBroadleaf);
      REQUIRE(site);
      constexpr int kHalf = 70;  // cells of 7 m: ±490 m around the site
      const int c0x = worldgen::FloorDiv(site->first, TerrainGenerator::kTreeCell),
                c0z = worldgen::FloorDiv(site->second, TerrainGenerator::kTreeCell);
      std::vector<Feature> found;
      for (int cz = c0z - kHalf; cz <= c0z + kHalf; ++cz)
        for (int cx = c0x - kHalf; cx <= c0x + kHalf; ++cx) {
          const auto t = gen.TreeInCell(cx, cz);
          // Broadleaf forest trees: oaks with a green or an accent crown.
          if (t && t->kind == Feature::Kind::kOak) found.push_back(*t);
        }
      // Nearest neighbour by a grid of 16 m cells.
      std::unordered_map<std::int64_t, std::vector<std::size_t>> grid;
      const auto key = [](std::int32_t x, std::int32_t z) {
        return (static_cast<std::int64_t>(worldgen::FloorDiv(x, 16)) << 32) ^
               static_cast<std::uint32_t>(worldgen::FloorDiv(z, 16));
      };
      for (std::size_t i = 0; i < found.size(); ++i) grid[key(found[i].x, found[i].z)].push_back(i);
      for (std::size_t i = 0; i < found.size(); ++i) {
        ++trees;
        const bool accent = found[i].leaves != M::kLeaves;
        accents += accent;
        if (!accent) continue;
        // The nearest other tree within 3 grid rings (48 m).
        double best = 1e18;
        std::size_t nearest = i;
        for (int dz = -3; dz <= 3; ++dz)
          for (int dx = -3; dx <= 3; ++dx) {
            const auto it = grid.find(key(found[i].x + dx * 16, found[i].z + dz * 16));
            if (it == grid.end()) continue;
            for (const std::size_t j : it->second) {
              if (j == i) continue;
              const double ex = found[j].x - found[i].x, ez = found[j].z - found[i].z;
              if (ex * ex + ez * ez < best) {
                best = ex * ex + ez * ez;
                nearest = j;
              }
            }
          }
        accents_next_to_accents += nearest != i && found[nearest].leaves != M::kLeaves;
      }
    }
    REQUIRE(trees > 3000);
    REQUIRE(accents > 100);
    const double overall = static_cast<double>(accents) / trees;
    const double clumped = static_cast<double>(accents_next_to_accents) / accents;
    MESSAGE(trees << " trees, " << accents << " accent (" << overall * 100
                  << " %); an accent's nearest tree is an accent " << clumped * 100
                  << " % of the time");
    CHECK(overall > 0.03);  // sparingly: a few percent of the trees
    CHECK(overall < 0.2);
    CHECK(clumped > 3.0 * overall);
  }

  TEST_CASE("every accent colour appears, in the biomes that allow it, and only there") {
    const TerrainGenerator gen(0);
    std::set<MaterialId> seen;
    for (const Biome b : {Biome::kBroadleaf, Biome::kBlossomGrove, Biome::kAutumnWoods}) {
      const auto site = testing::FindBiome(gen, b);
      REQUIRE(site);
      const auto& def = worldgen::BiomeOf(b);
      const int c0x = worldgen::FloorDiv(site->first, TerrainGenerator::kTreeCell),
                c0z = worldgen::FloorDiv(site->second, TerrainGenerator::kTreeCell);
      for (int cz = c0z - 60; cz <= c0z + 60; ++cz)
        for (int cx = c0x - 60; cx <= c0x + 60; ++cx) {
          const auto t = gen.TreeInCell(cx, cz);
          if (!t || !IsLeaf(t->leaves)) continue;
          seen.insert(t->leaves);
          // The tree's own biome's rule: its default or one of the biome's accents.
          const Column col = gen.ColumnAt(t->x, t->z);
          const auto& tree_def = worldgen::BiomeOf(col.biome);
          bool allowed = t->leaves == M::kLeaves || t->leaves == M::kLeavesBlossom ||
                         t->leaves == M::kLeavesAutumn;
          for (int i = 0; i < tree_def.accent_count; ++i)
            allowed |= tree_def.accents[i] == t->leaves;
          CHECK(allowed);
        }
      (void)def;
    }
    for (const MaterialId m : {M::kLeaves, M::kLeavesBlossom, M::kLeavesAutumn}) {
      CAPTURE(m);
      CHECK(seen.count(m) == 1);
    }
    CHECK(seen.size() >= 4);
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

  TEST_CASE("the leaf and grass blocks are in the registry, placeable and shaped like their kin") {
    for (const char* id :
         {"dwell:leaves_bright", "dwell:leaves_autumn", "dwell:leaves_red", "dwell:leaves_blossom",
          "dwell:leaves_violet", "dwell:grass_meadow", "dwell:grass_golden"}) {
      CAPTURE(id);
      const auto m = core::ParseState(id);
      REQUIRE(m);
      CHECK(core::BlockOf(*m).id == std::string_view(id));
      CHECK(core::GetMaterial(*m).solid);
      CHECK(core::Placeable(*m));
    }
    CHECK(core::FindBlock("dwell:grass_meadow_slope"));
    CHECK(core::FindBlock("dwell:grass_golden_slab"));
    CHECK_FALSE(core::FindBlock("dwell:leaves_red_slope"));  // leaves are not shaped
  }
}
