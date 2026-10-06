// Rivers, lakes and water above sea level (Phase 11a, WORLD_GENERATION.md §3.2–3.3, §3.9): the
// terraces, rivers lying in valleys and reaching the sea at sea level, water that never floats, caves
// that never breach it, and the spawn beside water. Runs natively (dwell_tests) and under Node
// (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <set>
#include <vector>

#include "dwell/core/voxel.h"
#include "dwell/worldgen/rivers.h"
#include "dwell/worldgen/terrain.h"

#include "biome_search.h"
#include "water_search.h"

using namespace dwell;
using core::MaterialId;
using testing::FindBiome;
using testing::Point;
using testing::Tier;
using worldgen::Biome;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

bool IsSolid(MaterialId m) { return m != M::kAir && m != M::kWater; }

// The shore of the origin's continent: one point per bearing, where the coast distance is `at` m,
// found by walking out from the origin and bisecting.
std::vector<Point> ShorePoints(const TerrainGenerator& gen, float at, int bearings) {
  std::vector<Point> out;
  for (int a = 0; a < bearings; ++a) {
    const double angle = 6.283185307179586 * a / bearings;
    const double dx = std::cos(angle), dz = std::sin(angle);
    const auto point = [&](double r) {
      return Point{static_cast<std::int32_t>(std::lround(dx * r)),
                   static_cast<std::int32_t>(std::lround(dz * r))};
    };
    double r = 20000.0;
    while (r < 3.0e6 && gen.LandAt(point(r).x, point(r).z).coast > at) r += 20000.0;
    if (r >= 3.0e6 || !core::InsideWorldDisc(point(r).x, point(r).z)) continue;
    double lo = r - 20000.0, hi = r;  // coast > at at lo, ≤ at at hi
    for (int i = 0; i < 24; ++i) {
      const double mid = (lo + hi) / 2;
      (gen.LandAt(point(mid).x, point(mid).z).coast > at ? lo : hi) = mid;
    }
    out.push_back(point(lo));
  }
  return out;
}

// Points where the great river's centreline lies within a band of the coast (coast distance in
// (0, 5,000) m), near `around`: a grid of `step` m over ±`half` m, a sign change of the tier's noise
// between east-west neighbours that are both in the band, bisected to the contour.
std::vector<Point> GreatRiverMouths(const TerrainGenerator& gen, Point around, int half, int step) {
  std::vector<Point> out;
  for (std::int32_t z = around.z - half; z <= around.z + half; z += step) {
    float prev = 0.0f, prev_coast = -1.0f;
    for (std::int32_t x = around.x - half; x <= around.x + half; x += step) {
      const float coast = gen.LandAt(x, z).coast;
      const float v = testing::TierValue(gen, Tier::kGreat, x, z);
      if (coast > 100.0f && coast < 4900.0f && prev_coast > 100.0f && prev_coast < 4900.0f &&
          (v < 0.0f) != (prev < 0.0f) && std::abs(v - prev) < 0.5f) {
        out.push_back(testing::BisectContour(gen, Tier::kGreat, {x - step, z}, {x, z}));
      }
      prev = v;
      prev_coast = coast;
    }
  }
  return out;
}

}  // namespace

TEST_SUITE("worldgen: rivers") {
  TEST_CASE("terraces are 2 to 6 m apart, rise with the valley floor, and start at sea level") {
    for (const std::uint64_t world : {0ull, 7ull, 123456789ull}) {
      const auto seed = worldgen::rivers::MakeSeeds(world).terrace;
      float prev = 0.0f, prev_level = 0.0f;
      std::set<float> levels;
      for (int i = -500; i <= 40000; ++i) {
        const float v = static_cast<float>(i) * 0.01f;
        const float level = worldgen::rivers::TerraceSurface(seed, v);
        CHECK(level <= std::max(v, 0.0f));
        CHECK(level >= prev_level);  // never falls as the floor rises
        if (v < 3.0f) CHECK(level == 0.0f);
        levels.insert(level);
        prev = v;
        prev_level = level;
      }
      (void)prev;
      REQUIRE(levels.size() > 50);
      float last = *levels.begin();
      for (const float level : levels) {
        if (level == last) continue;
        if (last > 0.0f) {
          CHECK(level - last >= 2.0f);
          CHECK(level - last <= 6.0f);
        } else {
          CHECK(level >= 3.0f);
          CHECK(level <= 5.0f);
        }
        last = level;
      }
    }
  }

  TEST_CASE("no world has a river crossing its origin: every tier's lattice is shifted") {
    // Regression: Perlin noise is exactly zero at lattice points, so unshifted tiers all crossed
    // at the origin (the spawn) in every world.
    for (std::uint64_t seed = 0; seed < 16; ++seed) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      const auto c = gen.RiversAt(0, 0);
      CHECK(c.rg != 0.0f);
      CHECK(c.r1 != 0.0f);
      CHECK(c.r2 != 0.0f);
    }
  }

  TEST_CASE("rivers lie in valleys: beds are no higher than the ground 50-500 m to either side") {
    for (const std::uint64_t seed : {0ull, 1ull}) {
      const TerrainGenerator gen(seed);
      for (const Tier tier : {Tier::kGreat, Tier::kRiver, Tier::kStream}) {
        CAPTURE(seed);
        CAPTURE(static_cast<int>(tier));
        const auto lines =
            tier == Tier::kGreat
                ? testing::FindCentrelines(gen, tier, 0, 0, 240000, 8000, 12)
                : testing::FindCentrelines(gen, tier, 0, 0, 40000,
                                           tier == Tier::kRiver ? 400 : 200, 80);
        REQUIRE(lines.size() >= 5);
        int checked = 0, violations = 0;
        for (const Point& p : lines) {
          const auto bed = gen.ColumnAt(p.x, p.z);
          // A channel with its water; a spring's last stretch fades to a gully with hardly any bed.
          if (bed.wet < 0.8f) continue;
          const auto n = testing::TierNormal(gen, tier, p.x, p.z);
          // The great river's banks reach ~800 m: its valley is measured from there out.
          const std::vector<int> reaches = tier == Tier::kGreat
                                               ? std::vector<int>{800, 1000, 1500, 2500}
                                               : std::vector<int>{50, 100, 200, 350, 500};
          for (const int d : reaches)
            for (const double sign : {1.0, -1.0}) {
              const auto side = gen.ColumnAt(p.x + static_cast<std::int32_t>(std::lround(n.first * d * sign)),
                                             p.z + static_cast<std::int32_t>(std::lround(n.second * d * sign)));
              if (side.wet > 0.0f || side.outside) continue;  // another channel, or a lake's rim
              ++checked;
              if (side.height < bed.height - 0.01f) {
                ++violations;
                CAPTURE(p.x);
                CAPTURE(p.z);
                CAPTURE(d);
                CHECK(side.height >= bed.height - 0.01f);
              }
            }
        }
        CHECK(checked > 20);
        CHECK(violations == 0);
      }
    }
  }

  TEST_CASE("great rivers' water is at sea level within 5 km of the coast") {
    int found = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull}) {
      const TerrainGenerator gen(seed);
      const auto shore = ShorePoints(gen, 2500.0f, 720);
      REQUIRE(shore.size() > 100);
      int crossings = 0;
      for (std::size_t i = 0; i + 1 < shore.size() && crossings < 3; ++i) {
        const Point a = shore[i], b = shore[i + 1];
        if (std::abs(a.x - b.x) + std::abs(a.z - b.z) > 60000) continue;  // a jump in the shore
        const float va = testing::TierValue(gen, Tier::kGreat, a.x, a.z);
        const float vb = testing::TierValue(gen, Tier::kGreat, b.x, b.z);
        if ((va < 0.0f) == (vb < 0.0f) || std::abs(va - vb) > 0.5f) continue;
        // The river crosses the shore somewhere near this stretch of it.
        const Point mid{(a.x + b.x) / 2, (a.z + b.z) / 2};
        for (const Point& p : GreatRiverMouths(gen, mid, 20000, 500)) {
          const auto col = gen.ColumnAt(p.x, p.z);
          if (col.outside || col.coast <= 0.0f || col.coast > 5000.0f) continue;
          ++found;
          ++crossings;
          CAPTURE(seed);
          CAPTURE(p.x);
          CAPTURE(p.z);
          CHECK(col.wet > 0.0f);
          CHECK(col.water == core::kSeaLevel);
          CHECK(col.height < static_cast<float>(core::kSeaLevel));  // the bed lies under the sea
          // And the stretch of river around it that is within 5 km of the coast.
          for (int dz = -400; dz <= 400; dz += 100)
            for (int dx = -400; dx <= 400; dx += 100) {
              const auto near = gen.ColumnAt(p.x + dx, p.z + dz);
              if (near.coast > 0.0f && near.coast <= 5000.0f) CHECK(near.water == core::kSeaLevel);
            }
          break;
        }
      }
    }
    MESSAGE(found << " great-river mouths checked");
    CHECK(found >= 3);
  }

  TEST_CASE("rivers have terraced surfaces with waterfall steps, lakes have flat ones") {
    const TerrainGenerator gen(0);
    // The great river through the origin's valley, and the first lake of the seed.
    const auto lines = testing::FindCentrelines(gen, Tier::kGreat, 0, 0, 240000, 8000, 1);
    REQUIRE_FALSE(lines.empty());
    std::set<std::int32_t> levels;
    for (int dx = -2000; dx <= 2000; dx += 50) {
      const auto c = gen.ColumnAt(lines[0].x + dx, lines[0].z);
      if (c.height < static_cast<float>(c.water)) levels.insert(c.water);
    }
    CHECK(levels.size() >= 1);

    const auto lake = testing::FindLake(gen);
    REQUIRE(lake);
    const auto centre = gen.ColumnAt(lake->x, lake->z);
    CHECK(centre.lake);
    // One surface across each lake: connected lake columns (4-connected, on a 25 m grid) all have
    // the same level.
    constexpr int kHalf = 100, kN = 2 * kHalf + 1;  // ±2.5 km
    std::vector<std::int32_t> level(kN * kN, -1);
    int lake_columns = 0;
    for (int j = 0; j < kN; ++j)
      for (int i = 0; i < kN; ++i) {
        const auto c = gen.ColumnAt(lake->x + (i - kHalf) * 25, lake->z + (j - kHalf) * 25);
        if (c.lake) {
          level[static_cast<std::size_t>(j * kN + i)] = c.water;
          ++lake_columns;
        }
      }
    CHECK(lake_columns > 50);
    int mismatches = 0;
    for (int j = 0; j < kN; ++j)
      for (int i = 0; i < kN; ++i) {
        const auto here = level[static_cast<std::size_t>(j * kN + i)];
        if (here < 0) continue;
        if (i + 1 < kN) {
          const auto next = level[static_cast<std::size_t>(j * kN + i + 1)];
          mismatches += next >= 0 && next != here;
        }
        if (j + 1 < kN) {
          const auto next = level[static_cast<std::size_t>((j + 1) * kN + i)];
          mismatches += next >= 0 && next != here;
        }
      }
    CHECK(mismatches == 0);
  }

  TEST_CASE("water never floats, and spills only at terrace steps") {
    // Around a lake, a stream and a waterfall: every water voxel has water or a solid below it, and
    // every horizontal contact between water and air is a step down to a lower pool.
    const TerrainGenerator gen(0);
    core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, 0));
    std::vector<Point> sites;
    if (const auto lake = testing::FindLake(gen)) sites.push_back(*lake);
    for (const Tier tier : {Tier::kRiver, Tier::kStream}) {
      const auto lines = testing::FindCentrelines(gen, tier, 0, 0, 12000, 200, 2);
      sites.insert(sites.end(), lines.begin(), lines.end());
    }
    const auto great = testing::FindCentrelines(gen, Tier::kGreat, 0, 0, 240000, 8000, 1);
    if (!great.empty()) {
      if (const auto fall = testing::FindWaterfall(gen, Tier::kGreat, great[0], 40000, 16)) {
        sites.push_back(fall->first);
      }
    }
    REQUIRE(sites.size() >= 3);
    long water = 0, below_air = 0, contacts = 0, spills = 0;
    for (const Point& site : sites) {
      for (int dz = -40; dz <= 40; ++dz)
        for (int dx = -40; dx <= 40; ++dx) {
          const std::int32_t x = site.x + dx, z = site.z + dz;
          const auto col = gen.ColumnAt(x, z);
          if (col.outside || col.height >= static_cast<float>(col.water)) continue;
          const int bed = static_cast<int>(std::floor(col.height));
          for (int y = bed - 2; y < col.water + 2; ++y) {
            if (world.GetVoxel(x, y, z) != M::kWater) continue;
            ++water;
            below_air += world.GetVoxel(x, y - 1, z) == M::kAir;
            for (const auto& [ox, oz] : {std::pair{1, 0}, std::pair{-1, 0}, std::pair{0, 1},
                                         std::pair{0, -1}}) {
              if (world.GetVoxel(x + ox, y, z + oz) != M::kAir) continue;
              ++contacts;
              const auto n = gen.ColumnAt(x + ox, z + oz);
              // A terrace step: the neighbour is a channel or lake column whose pool is lower.
              const bool step = n.wet > 0.0f && n.water < col.water;
              if (!step) {
                ++spills;
                CAPTURE(x);
                CAPTURE(y);
                CAPTURE(z);
                CHECK(step);
              }
            }
          }
        }
    }
    MESSAGE(water << " water voxels, " << contacts << " contacts with air at steps, " << spills
                  << " elsewhere");
    CHECK(water > 1000);
    CHECK(below_air == 0);
    CHECK(spills == 0);
  }

  TEST_CASE("no cave air within the suppression depth below any water") {
    for (const std::uint64_t seed : {0ull, 1ull}) {
      const TerrainGenerator gen(seed);
      std::vector<Point> sites;
      if (const auto lake = testing::FindLake(gen)) sites.push_back(*lake);
      for (const Tier tier : {Tier::kRiver, Tier::kStream}) {
        const auto lines = testing::FindCentrelines(gen, tier, 0, 0, 12000, 200, 2);
        sites.insert(sites.end(), lines.begin(), lines.end());
      }
      if (const auto sea = FindBiome(gen, Biome::kOcean)) sites.push_back({sea->first, sea->second});
      REQUIRE(sites.size() >= 3);
      long columns = 0, caves = 0;
      for (const Point& site : sites)
        for (int dz = -60; dz <= 60; dz += 3)
          for (int dx = -60; dx <= 60; dx += 3) {
            const std::int32_t x = site.x + dx, z = site.z + dz;
            const auto col = gen.ColumnAt(x, z);
            const bool water = col.height < static_cast<float>(col.water) &&
                               col.height > -worldgen::rivers::kShallowSea;
            if (col.outside || !water) continue;
            ++columns;
            const int bed = static_cast<int>(std::floor(col.height));
            // From two voxels under the bed (the surface voxel itself may be open where the
            // overhang noise lowers the ground) to 13 m under it.
            for (int y = bed - 2; y >= bed - 13; --y) {
              if (!gen.SolidAt(x, y, z)) {
                ++caves;
                CAPTURE(x);
                CAPTURE(y);
                CAPTURE(z);
                CHECK(gen.SolidAt(x, y, z));
              }
            }
          }
      MESSAGE("seed " << seed << ": " << columns << " water columns, " << caves << " cave voxels");
      CHECK(columns > 500);
      CHECK(caves == 0);
    }
  }

  TEST_CASE("chunks holding river and lake water are never taken for air") {
    const TerrainGenerator gen(0);
    std::vector<Point> sites;
    if (const auto lake = testing::FindLake(gen)) sites.push_back(*lake);
    const auto lines = testing::FindCentrelines(gen, Tier::kGreat, 0, 0, 240000, 8000, 2);
    sites.insert(sites.end(), lines.begin(), lines.end());
    REQUIRE(sites.size() >= 2);
    int chunks = 0;
    for (const Point& site : sites)
      for (int dz = -64; dz <= 64; dz += 8)
        for (int dx = -64; dx <= 64; dx += 8) {
          const auto col = gen.ColumnAt(site.x + dx, site.z + dz);
          if (col.outside || col.height >= static_cast<float>(col.water)) continue;
          // The chunk holding the surface voxel of the water.
          const auto c = core::ChunkOf(site.x + dx, col.water - 1, site.z + dz);
          CHECK_FALSE(gen.IsAirChunk(c));
          const float sky = gen.SkyFloorAt(c.x, c.z);
          CHECK(sky >= static_cast<float>(col.water));
          ++chunks;
        }
    CHECK(chunks > 20);
  }

  TEST_CASE("players spawn beside water when there is some near the origin") {
    int with_water = 0, beside = 0;
    for (std::uint64_t seed = 0; seed < 24; ++seed) {
      const TerrainGenerator gen(seed);
      const auto s = gen.SpawnPoint();
      const auto x = static_cast<std::int32_t>(std::floor(s[0]));
      const auto z = static_cast<std::int32_t>(std::floor(s[2]));
      CAPTURE(seed);
      CHECK(std::abs(x) < 512);
      CHECK(std::abs(z) < 512);
      CHECK(gen.ColumnAt(x, z).wet == 0.0f);  // dry ground, not a river bed
      // Is there water within 288 m of the origin (what the spawn search looks for)?
      const auto water_near = [&](std::int32_t cx, std::int32_t cz, int reach) {
        for (int dz = -reach; dz <= reach; dz += 32)
          for (int dx = -reach; dx <= reach; dx += 32) {
            const auto c = gen.ColumnAt(cx + dx, cz + dz);
            if (!c.outside && c.height < static_cast<float>(c.water)) return true;
          }
        return false;
      };
      if (!water_near(0, 0, 288)) continue;
      ++with_water;
      // The spawn lies beside it — unless there is no level ground beside the water, in which case
      // the search falls back to level ground anywhere near the origin.
      beside += water_near(x, z, 288);
    }
    MESSAGE(beside << " of " << with_water << " seeds with water near the origin spawn beside it");
    CHECK(with_water >= 3);
    CHECK(beside * 2 >= with_water);
  }
}
