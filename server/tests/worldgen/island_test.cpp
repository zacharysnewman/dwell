// The interim land layout (island.h, Phase 15): one island at the origin.
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>

#include "dwell/worldgen/island.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;
using worldgen::IslandLayout;
using worldgen::TerrainGenerator;

TEST_SUITE("worldgen: island") {
  TEST_CASE("the layout is a pure function of the seed") {
    const IslandLayout a(9), b(9), c(10);
    for (const auto& [x, z] : {std::pair{0, 0}, {21'000, -4'000}, {-17'500, 19'250}}) {
      CHECK(a.CoastAt(x, z) == b.CoastAt(x, z));
      CHECK(a.CoastAt(x, z) != c.CoastAt(x, z));
    }
  }

  TEST_CASE("one island at the origin, 20–30 km in radius, sea all round") {
    for (const std::uint64_t seed : {0ull, 1ull, 42ull, 20260925ull}) {
      CAPTURE(seed);
      const IslandLayout island(seed);
      CHECK(island.CoastAt(0, 0) > 5000.0f);
      double nearest_sea = 1e9, farthest_land = 0;
      for (int a = 0; a < 64; ++a) {
        const double angle = a * (2.0 * 3.14159265358979323846 / 64);
        for (int r = 0; r <= 45'000; r += 250) {
          const auto x = static_cast<std::int32_t>(std::lround(r * std::cos(angle)));
          const auto z = static_cast<std::int32_t>(std::lround(r * std::sin(angle)));
          if (island.CoastAt(x, z) > 0.0f) {
            farthest_land = std::max(farthest_land, static_cast<double>(r));
          } else {
            nearest_sea = std::min(nearest_sea, static_cast<double>(r));
          }
        }
      }
      MESSAGE("sea from " << nearest_sea << " m, land to " << farthest_land << " m");
      CHECK(nearest_sea > 15'000.0);
      CHECK(farthest_land < 36'000.0);
      // Far out: sea.
      CHECK(island.CoastAt(100'000, -70'000) < -50'000.0f);
    }
  }

  TEST_CASE("the level of detail keeps the coast's coarse octaves") {
    const IslandLayout island(3);
    for (const auto& [x, z] : {std::pair{0, 0}, {24'000, 0}, {-12'000, 20'000}}) {
      // With no detail octaves the coast moves by at most the detail's amplitude.
      CHECK(std::abs(island.CoastAt(x, z, 0) - island.CoastAt(x, z)) <=
            2.0f * worldgen::island::kCoastAmplitude);
      CHECK(island.CoastAt(x, z, worldgen::island::kCoastOctaves) == island.CoastAt(x, z));
    }
  }

  TEST_CASE("the terrain follows the layout: land well inside the coast, sea well outside") {
    const TerrainGenerator gen(0);
    int land = 0, sea = 0;
    for (std::int32_t z = -40'000; z <= 40'000; z += 1000)
      for (std::int32_t x = -40'000; x <= 40'000; x += 1000) {
        const worldgen::Column c = gen.ColumnAt(x, z);
        if (c.coast > 500.0f) {
          CHECK(c.height > 0.0f);
          ++land;
        } else if (c.coast < -500.0f) {
          CHECK(c.height < 0.0f);
          ++sea;
        }
      }
    CHECK(land > 1000);
    CHECK(sea > 1000);
  }
}
