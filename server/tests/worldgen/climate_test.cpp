// Climate and biomes at continental scale (generator version 8, ADR 0019): biomes come in regions
// of tens to hundreds of kilometres, not patches a few hundred metres across; snow lies where it is
// cold, by latitude-like regions and by height; every biome has a share of the land. Runs natively
// (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <array>
#include <cstdint>

#include "dwell/worldgen/terrain.h"

using namespace dwell;
using worldgen::Biome;
using worldgen::Column;
using worldgen::TerrainGenerator;

TEST_SUITE("worldgen: climate") {
  TEST_CASE(
      "biomes come in regions: neighbouring samples 2 km apart rarely differ in climate biome") {
    // Regression: temperature and humidity were noise of 0.7–1.1 km, so plains, forest, desert and
    // snow alternated every few hundred metres (snow "scattered everywhere").
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull}) {
      const TerrainGenerator gen(seed);
      const auto climate = [](Biome b) {
        return b == Biome::kPlains || b == Biome::kForest || b == Biome::kDesert ||
               b == Biome::kSnowy;
      };
      long pairs = 0, differ = 0;
      for (std::int32_t z = -2'000'000; z <= 2'000'000; z += 20'000)
        for (std::int32_t x = -2'000'000; x <= 2'000'000; x += 20'000)
          for (int k = 0; k < 8; ++k) {
            const std::int32_t px = x + k * 2000;
            const Column a = gen.ColumnAt(px, z), b = gen.ColumnAt(px + 2000, z);
            if (a.outside || b.outside || a.coast <= 0 || b.coast <= 0) continue;
            if (!climate(a.biome) || !climate(b.biome)) continue;
            ++pairs;
            differ += a.biome != b.biome;
          }
      CAPTURE(seed);
      REQUIRE(pairs > 1000);
      MESSAGE("seed " << seed << ": " << differ << " of " << pairs << " pairs differ");
      CHECK(differ * 12 < pairs);  // under ~8 % (a few hundred metres of patches gave over 60 %)
    }
  }

  TEST_CASE("snow lies where it is cold: the temperature at the ground, with height, says so") {
    const TerrainGenerator gen(0);
    long snowy = 0, warm_snow = 0, cold_high_not_snow = 0, cold_high = 0;
    for (std::int32_t z = -6'000'000; z <= 6'000'000; z += 60'000)
      for (std::int32_t x = -6'000'000; x <= 6'000'000; x += 60'000) {
        const Column c = gen.ColumnAt(x, z);
        if (c.outside || c.coast <= 0) continue;
        if (c.biome == Biome::kSnowy) {
          ++snowy;
          warm_snow += c.temperature > -0.3f;  // snow needs a cold ground
        }
        // Cold ground that is not mountains must be snow.
        if (c.temperature < -0.5f && c.biome != Biome::kMountains) {
          ++cold_high;
          cold_high_not_snow += c.biome != Biome::kSnowy;
        }
      }
    MESSAGE(snowy << " snowy samples");
    CHECK(snowy > 50);
    CHECK(warm_snow == 0);
    CHECK(cold_high_not_snow == 0);
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

  TEST_CASE("biome shares over the land, 8 seeds: none missing, none dominant") {
    for (std::uint64_t seed = 0; seed < 8; ++seed) {
      const TerrainGenerator gen(seed);
      std::array<long, 7> n{};
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
      for (const Biome b :
           {Biome::kPlains, Biome::kForest, Biome::kDesert, Biome::kSnowy, Biome::kMountains}) {
        const double share = static_cast<double>(n[static_cast<std::size_t>(b)]) / land;
        CAPTURE(worldgen::BiomeName(b));
        CHECK(share > 0.02);
        CHECK(share < 0.45);
      }
    }
  }
}
