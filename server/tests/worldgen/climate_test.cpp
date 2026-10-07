// Climate and biomes (ADR 0019, Phase 11c, WORLD_GENERATION.md §3.5–3.7): biomes come in regions of
// tens to hundreds of kilometres, not patches a few hundred metres across; snow lies where it is
// cold, by region and by height; the lee sides of ranges are drier; every biome has a share of the
// land; the biome table is well formed; wetland ponds hold still water; the biomes' tints blend
// across borders; a distant forest keeps its canopy. Runs natively (dwell_tests) and under Node
// (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <set>
#include <unordered_map>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/biomes.h"
#include "dwell/worldgen/terrain.h"

#include "biome_search.h"

using namespace dwell;
using worldgen::Biome;
using worldgen::Column;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

// The land's climate biomes: what the temperature × humidity table and the altitude bands give.
bool IsClimateBiome(Biome b) {
  return b >= Biome::kMeadow;  // meadow … snowfield (biomes.h order)
}

}  // namespace

TEST_SUITE("worldgen: climate") {
  TEST_CASE(
      "biomes come in regions: neighbouring samples 2 km apart rarely differ in climate biome") {
    // Regression: temperature and humidity were noise of 0.7–1.1 km, so plains, forest, desert and
    // snow alternated every few hundred metres (snow "scattered everywhere").
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull}) {
      const TerrainGenerator gen(seed);
      long pairs = 0, differ = 0;
      for (std::int32_t z = -2'000'000; z <= 2'000'000; z += 20'000)
        for (std::int32_t x = -2'000'000; x <= 2'000'000; x += 20'000)
          for (int k = 0; k < 8; ++k) {
            const std::int32_t px = x + k * 2000;
            const Column a = gen.ColumnAt(px, z), b = gen.ColumnAt(px + 2000, z);
            if (a.outside || b.outside || a.coast <= 0 || b.coast <= 0) continue;
            if (!IsClimateBiome(a.biome) || !IsClimateBiome(b.biome)) continue;
            // Lowland: on mountainsides the biome follows the height (the altitude bands).
            if (a.height > 150.0f || b.height > 150.0f) continue;
            ++pairs;
            differ += a.biome != b.biome;
          }
      CAPTURE(seed);
      REQUIRE(pairs > 1000);
      MESSAGE("seed " << seed << ": " << differ << " of " << pairs << " pairs differ");
      // Under ~14 % on lowland (a few hundred metres of patches gave over 60 % of four classes'
      // pairs; the twelve climate biomes have three times the borders of those four).
      CHECK(differ * 7 < pairs);
    }
  }

  TEST_CASE("snow lies where it is cold: the temperature at the ground, with height, says so") {
    const TerrainGenerator gen(0);
    long snowfield = 0, warm_snow = 0, cold_not_snow = 0, cold = 0;
    // The border noise moves a biome's edge by a few hundredths of the temperature.
    constexpr float kBorder = 0.07f;
    for (std::int32_t z = -6'000'000; z <= 6'000'000; z += 60'000)
      for (std::int32_t x = -6'000'000; x <= 6'000'000; x += 60'000) {
        const Column c = gen.ColumnAt(x, z);
        if (c.outside || c.coast <= 0) continue;
        if (c.biome == Biome::kSnowfield) {
          ++snowfield;
          warm_snow += c.temperature > worldgen::kSnowTemperature + kBorder;  // snow needs cold
        }
        // Cold ground is snow, unless the coast or a river's bank overrides it.
        if (c.temperature < worldgen::kSnowTemperature - kBorder && c.wet == 0.0f &&
            c.biome != Biome::kBeach && c.biome != Biome::kSeaCliff) {
          ++cold;
          cold_not_snow += c.biome != Biome::kSnowfield;
        }
      }
    MESSAGE(snowfield << " snowfield samples, " << cold << " cold ones");
    CHECK(snowfield > 50);
    CHECK(warm_snow == 0);
    CHECK(cold_not_snow == 0);
  }

  TEST_CASE("the altitude bands: alpine meadow, bare rock and snow stand above the tree line") {
    const TerrainGenerator gen(0);
    long alpine = 0, rock = 0, wrong = 0;
    constexpr float kBorder = 0.07f;
    for (std::int32_t z = -6'000'000; z <= 6'000'000; z += 30'000)
      for (std::int32_t x = -6'000'000; x <= 6'000'000; x += 30'000) {
        const Column c = gen.ColumnAt(x, z);
        if (c.outside || c.coast <= 0 || c.wet > 0.0f) continue;
        if (c.biome == Biome::kAlpineMeadow || c.biome == Biome::kBareRock) {
          (c.biome == Biome::kAlpineMeadow ? alpine : rock) += 1;
          // On high ground, between the tree line and the snow line (give or take the borders).
          wrong += c.height < worldgen::kAlpineMinHeight - 1.0f ||
                   c.temperature > worldgen::kTreeLineTemperature + kBorder ||
                   c.temperature < worldgen::kSnowTemperature - kBorder;
          // Bare rock is the colder band.
          if (c.biome == Biome::kBareRock) {
            wrong += c.temperature > worldgen::kBareRockTemperature + kBorder;
          }
        }
      }
    MESSAGE(alpine << " alpine meadow, " << rock << " bare rock samples");
    CHECK(alpine > 30);
    CHECK(rock > 30);
    CHECK(wrong == 0);
  }

  TEST_CASE("height cools: the same place is colder on a mountain than in the valley") {
    // The lapse rate: over land, columns' ground temperature falls with their height above sea
    // level.
    const TerrainGenerator gen(1);
    double low_t = 0, high_t = 0;
    int low_n = 0, high_n = 0;
    for (std::int32_t z = -3'000'000; z <= 3'000'000; z += 30'000)
      for (std::int32_t x = -3'000'000; x <= 3'000'000; x += 30'000) {
        const Column c = gen.ColumnAt(x, z);
        if (c.outside || c.coast <= 0) continue;
        if (c.height < 150.0f) {
          low_t += c.temperature;
          ++low_n;
        } else if (c.height > 1200.0f) {
          high_t += c.temperature;
          ++high_n;
        }
      }
    REQUIRE(low_n > 100);
    REQUIRE(high_n > 20);
    CHECK(high_t / high_n < low_t / low_n - 0.25);
  }

  TEST_CASE("lee sides of ranges are drier than windward sides (rain shadow)") {
    // Behind a high range (downwind of it, along its continent's prevailing wind) the humidity is
    // lower than in front of it (upwind), sampled across ranges and seeds.
    double windward = 0, lee = 0;
    int n = 0;
    constexpr std::int32_t kStep = 40'000, kReach = 70'000;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull, 4ull, 5ull}) {
      const TerrainGenerator gen(seed);
      for (std::int32_t z = -6'000'000; z <= 6'000'000; z += kStep)
        for (std::int32_t x = -6'000'000; x <= 6'000'000; x += kStep) {
          const Column crest = gen.ColumnAt(x, z);
          // A crest: well above its valley floor, on a big range, away from the coast.
          if (crest.outside || crest.coast < 150'000.0f || crest.height < 2500.0f ||
              crest.continent < 0) {
            continue;
          }
          const int wind = gen.Continents().Record(crest.continent).wind;
          constexpr int kWindX[8] = {1, 1, 0, -1, -1, -1, 0, 1};
          constexpr int kWindZ[8] = {0, 1, 1, 1, 0, -1, -1, -1};
          const std::int32_t dx = kWindX[wind] *
                                  (kWindX[wind] != 0 && kWindZ[wind] != 0 ? 49'000 : kReach),
                             dz = kWindZ[wind] *
                                  (kWindX[wind] != 0 && kWindZ[wind] != 0 ? 49'000 : kReach);
          const Column up = gen.ColumnAt(x - dx, z - dz), down = gen.ColumnAt(x + dx, z + dz);
          if (up.outside || down.outside || up.coast < 100'000.0f || down.coast < 100'000.0f ||
              up.continent != crest.continent || down.continent != crest.continent) {
            continue;
          }
          windward += up.humidity;
          lee += down.humidity;
          ++n;
        }
    }
    REQUIRE(n > 30);
    MESSAGE(n << " ranges: windward humidity " << windward / n << ", lee " << lee / n);
    CHECK(lee / n < windward / n - 0.12);
  }

  TEST_CASE("continents differ in climate: a hot, a cold, a wet and a dry one") {
    // The continents' records bias temperature and humidity: the mean ground temperature at low
    // height over a continent spreads with its record.
    const TerrainGenerator gen(2);
    std::unordered_map<std::int32_t, std::pair<double, int>> temperature;
    for (std::int32_t z = -6'000'000; z <= 6'000'000; z += 60'000)
      for (std::int32_t x = -6'000'000; x <= 6'000'000; x += 60'000) {
        const Column c = gen.ColumnAt(x, z);
        if (c.outside || c.coast < 20'000.0f || c.continent < 0 || c.height > 300.0f) continue;
        auto& [sum, count] = temperature[c.continent];
        sum += c.temperature;
        ++count;
      }
    double lo = 1e9, hi = -1e9;
    int continents = 0;
    for (const auto& [id, st] : temperature) {
      if (st.second < 30) continue;
      ++continents;
      const double mean = st.first / st.second;
      lo = std::min(lo, mean);
      hi = std::max(hi, mean);
    }
    REQUIRE(continents >= 6);
    MESSAGE(continents << " continents: mean temperature " << lo << " .. " << hi);
    CHECK(hi - lo > 0.2);
  }

  TEST_CASE("biome shares over the land, 8 seeds: none missing, none dominant") {
    for (std::uint64_t seed = 0; seed < 8; ++seed) {
      const TerrainGenerator gen(seed);
      std::array<long, worldgen::kBiomeCount> n{};
      long land = 0;
      for (std::int32_t z = -8'000'000; z <= 8'000'000; z += 64'000)
        for (std::int32_t x = -8'000'000; x <= 8'000'000; x += 64'000) {
          const Column c = gen.ColumnAt(x, z);
          if (c.outside || c.coast <= 0) continue;
          ++n[static_cast<std::size_t>(c.biome)];
          ++land;
        }
      CAPTURE(seed);
      REQUIRE(land > 5000);
      for (int b = static_cast<int>(Biome::kMeadow); b < worldgen::kBiomeCount; ++b) {
        const double share = static_cast<double>(n[static_cast<std::size_t>(b)]) / land;
        CAPTURE(worldgen::BiomeName(static_cast<Biome>(b)));
        CHECK(share > 0.005);
        CHECK(share < 0.35);
      }
    }
  }

  TEST_CASE("the biome table is well formed") {
    std::set<std::string> names;
    for (int b = 0; b < worldgen::kBiomeCount; ++b) {
      const auto& def = worldgen::BiomeOf(static_cast<Biome>(b));
      CAPTURE(b);
      CHECK(static_cast<int>(def.id) == b);  // one row per biome, in order
      REQUIRE(def.name != nullptr);
      CHECK(names.insert(def.name).second);  // unique names
      CHECK(std::string(worldgen::BiomeName(def.id)) == def.name);
      // Layers: increasing depths, the last reaching 8 (stone below).
      CHECK(def.layers[0].until >= 1);
      CHECK(def.layers[0].until <= def.layers[1].until);
      CHECK(def.layers[1].until <= def.layers[2].until);
      CHECK(def.layers[2].until == 8);
      // Trees: weights sum to 100 when there are any.
      if (def.tree_chance > 0.0f) CHECK(def.trees[0].weight + def.trees[1].weight == 100);
      CHECK(def.tree_chance >= 0.0f);
      CHECK(def.tree_chance <= 1.0f);
      // Tints are within what a multiplier of the tiles can be (up to x3.98) and not black.
      for (const auto& t : {def.grass, def.foliage}) {
        CHECK(std::min({t.r, t.g, t.b}) >= 16);
        CHECK(t.r > 0);
      }
    }
    // Every point of the temperature × humidity diagram has a climate biome, whatever the height.
    for (int ti = -100; ti <= 100; ++ti)
      for (int hi = -100; hi <= 100; ++hi) {
        const Biome low = worldgen::ClimateBiome(static_cast<float>(ti) * 0.01f,
                                                 static_cast<float>(hi) * 0.01f, 10.0f);
        const Biome high = worldgen::ClimateBiome(static_cast<float>(ti) * 0.01f,
                                                  static_cast<float>(hi) * 0.01f, 1500.0f);
        CHECK(IsClimateBiome(low));
        CHECK(IsClimateBiome(high));
      }
  }

  TEST_CASE("frozen seas exist where the sea is cold") {
    bool found = false;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull}) {
      const TerrainGenerator gen(seed);
      const auto at = testing::FindBiome(gen, Biome::kFrozenOcean);
      if (at) {
        found = true;
        const Column c = gen.ColumnAt(at->first, at->second);
        CHECK(c.temperature < worldgen::kFrozenOceanTemperature);
        // Its surface is a metre of ice over water: the top water voxel is ice, the ones below are
        // water, the sea floor gravel.
        core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
        int ice = 0, columns = 0;
        for (int dz = 0; dz < 16; dz += 3)
          for (int dx = 0; dx < 16; dx += 3) {
            const auto col = gen.ColumnAt(at->first + dx, at->second + dz);
            if (col.biome != Biome::kFrozenOcean || col.height > static_cast<float>(col.water) - 4)
              continue;
            ++columns;
            const int x = at->first + dx, z = at->second + dz;
            ice += world.GetVoxel(x, col.water - 1, z) == M::kIce &&
                   world.GetVoxel(x, col.water - 2, z) == M::kWater &&
                   world.GetVoxel(x, col.water, z) == M::kAir;
          }
        MESSAGE(ice << " of " << columns << " frozen columns have a sheet of ice");
        CHECK(columns > 5);
        CHECK(ice == columns);
        break;
      }
    }
    CHECK(found);
  }
}
