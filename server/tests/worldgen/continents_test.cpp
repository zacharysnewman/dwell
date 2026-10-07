// Continents from Voronoi plates (WORLD_GENERATION.md §2, ADR 0017): the layout's guarantees —
// open ocean between continents, the shape statistics, a fractal coast — and the terrain built on
// the coast distance. Runs natively (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <map>
#include <set>
#include <vector>

#include "dwell/core/voxel.h"
#include "dwell/worldgen/continents.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;
using worldgen::ContinentLayout;
using worldgen::MacroCorner;
using worldgen::TerrainGenerator;
namespace C = worldgen::continents;

namespace {

// A small deterministic generator for test sampling (xorshift64*).
struct Rng {
  std::uint64_t s;
  std::uint64_t Next() {
    s ^= s >> 12;
    s ^= s << 25;
    s ^= s >> 27;
    return s * 0x2545F4914F6CDD1Dull;
  }
  double Unit() { return static_cast<double>(Next() >> 11) / 9007199254740992.0; }
};

const std::uint64_t kSeeds[8] = {0, 1, 2, 3, 20260925, 42, 7, 123456789};

// A point of the disc, uniformly by area.
std::pair<std::int32_t, std::int32_t> DiscPoint(Rng& rng) {
  for (;;) {
    const auto x = static_cast<std::int32_t>((rng.Unit() * 2 - 1) * core::kWorldRadius);
    const auto z = static_cast<std::int32_t>((rng.Unit() * 2 - 1) * core::kWorldRadius);
    if (core::InsideWorldDisc(x, z)) return {x, z};
  }
}

// Land points of continents (not islands), `want` of them, from rejection sampling.
std::vector<std::pair<std::int32_t, std::int32_t>> ContinentPoints(const TerrainGenerator& gen,
                                                                   Rng& rng, int want) {
  std::vector<std::pair<std::int32_t, std::int32_t>> out;
  while (static_cast<int>(out.size()) < want) {
    const auto p = DiscPoint(rng);
    const auto l = gen.LandAt(p.first, p.second);
    if (l.coast > 0.0f && l.continent >= 0) out.push_back(p);
  }
  return out;
}

// Directions: 64 evenly spaced angles, by integer-free table (cos/sin of test code only).
struct Dir {
  double x, z;
};
std::vector<Dir> Directions(int n) {
  std::vector<Dir> d;
  for (int i = 0; i < n; ++i) {
    const double a = 2.0 * 3.14159265358979323846 * i / n;
    d.push_back({std::cos(a), std::sin(a)});
  }
  return d;
}

int PointsPerSeed() {
  // CI runs 500 land points per seed (the full 10,000 takes minutes in a Debug build and under
  // WASM); DWELL_SEPARATION_POINTS=10000 runs the exit criterion's count (30 s natively, Release).
  if (const char* v = std::getenv("DWELL_SEPARATION_POINTS")) return std::max(1, std::atoi(v));
  return 500;
}

// Marching-squares length (m) of the zero contour of `value` in a square window, on a grid of
// spacing `h`.
template <class Value>
double ContourLength(Value&& value, std::int64_t x0, std::int64_t z0, std::int64_t size,
                     std::int64_t h) {
  const std::int64_t n = size / h;
  std::vector<float> v(static_cast<std::size_t>((n + 1) * (n + 1)));
  for (std::int64_t j = 0; j <= n; ++j)
    for (std::int64_t i = 0; i <= n; ++i) {
      v[static_cast<std::size_t>(j * (n + 1) + i)] = value(x0 + i * h, z0 + j * h);
    }
  double total = 0;
  for (std::int64_t j = 0; j < n; ++j)
    for (std::int64_t i = 0; i < n; ++i) {
      const float c[4] = {v[static_cast<std::size_t>(j * (n + 1) + i)],
                          v[static_cast<std::size_t>(j * (n + 1) + i + 1)],
                          v[static_cast<std::size_t>((j + 1) * (n + 1) + i + 1)],
                          v[static_cast<std::size_t>((j + 1) * (n + 1) + i)]};
      // Crossing points on the four edges, in cell units.
      double px[4], pz[4];
      int k = 0;
      for (int e = 0; e < 4; ++e) {
        const float a = c[e], b = c[(e + 1) % 4];
        if ((a > 0.0f) == (b > 0.0f)) continue;
        const double t = static_cast<double>(a) / (static_cast<double>(a) - b);
        static const double ex[4] = {0, 1, 1, 0}, ez[4] = {0, 0, 1, 1};
        const double dx = ex[(e + 1) % 4] - ex[e], dz = ez[(e + 1) % 4] - ez[e];
        px[k] = ex[e] + t * dx;
        pz[k] = ez[e] + t * dz;
        ++k;
      }
      for (int s = 0; s + 1 < k; s += 2) {
        total += std::hypot(px[s + 1] - px[s], pz[s + 1] - pz[s]) * static_cast<double>(h);
      }
    }
  return total;
}

}  // namespace

TEST_SUITE("worldgen: continents") {
  TEST_CASE("the layout is a pure function of the seed, whatever the caches hold") {
    const TerrainGenerator a(5), b(5), c(6);
    std::vector<MacroCorner> first;
    Rng rng{99};
    std::vector<std::pair<std::int32_t, std::int32_t>> points;
    for (int i = 0; i < 200; ++i) points.push_back(DiscPoint(rng));
    for (const auto& [x, z] : points) first.push_back(a.Continents().Sample(x, z));
    // Thrash the per-thread caches with another world and many other points, then ask again.
    for (int i = 0; i < 3000; ++i) {
      const auto p = DiscPoint(rng);
      (void)c.Continents().Sample(p.first, p.second);
    }
    for (std::size_t i = 0; i < points.size(); ++i) {
      const MacroCorner m = b.Continents().Sample(points[i].first, points[i].second);
      CHECK(m.coast == first[i].coast);
      CHECK(m.continent == first[i].continent);
      CHECK(m.plate_edge == first[i].plate_edge);
    }
    int differ = 0;
    for (const auto& [x, z] : points) {
      differ += a.Continents().At(x, z).coast != c.Continents().At(x, z).coast;
    }
    CHECK(differ > 150);
  }

  TEST_CASE("lattice corners are the exact field, and the lattice interpolates between them") {
    const TerrainGenerator gen(11);
    const ContinentLayout& layout = gen.Continents();
    Rng rng{5};
    for (int i = 0; i < 200; ++i) {
      const auto [x, z] = DiscPoint(rng);
      const std::int32_t mx = worldgen::FloorDiv(x, C::kMacroStep) * C::kMacroStep;
      const std::int32_t mz = worldgen::FloorDiv(z, C::kMacroStep) * C::kMacroStep;
      const MacroCorner exact = layout.At(mx, mz);
      const MacroCorner sampled = layout.Sample(mx, mz);
      CHECK(sampled.coast == exact.coast);
      CHECK(sampled.continent == exact.continent);
      // Between corners the value is between the corners' (bilinear): it never overshoots.
      const MacroCorner m = layout.Sample(x, z);
      float lo = 1e30f, hi = -1e30f;
      for (int dz = 0; dz <= 1; ++dz)
        for (int dx = 0; dx <= 1; ++dx) {
          const float c = layout.At(mx + dx * C::kMacroStep, mz + dz * C::kMacroStep).coast;
          lo = std::min(lo, c);
          hi = std::max(hi, c);
        }
      CHECK(m.coast >= lo - 1.0f);
      CHECK(m.coast <= hi + 1.0f);
    }
  }

  TEST_CASE(
      "the domain warp stretches distances by at most the Lipschitz bound the clamp assumes") {
    // The separation guarantee widens the clamp by kWarpLipschitz: |w(p) − w(q)| ≤ L |p − q|
    // (plus a metre of rounding), at every scale.
    for (const std::uint64_t seed : {0ull, 17ull}) {
      const TerrainGenerator gen(seed);
      Rng rng{seed + 1};
      double worst = 0;
      for (const double d : {50.0, 400.0, 3000.0, 25'000.0, 200'000.0}) {
        for (int i = 0; i < 4000; ++i) {
          const auto [x, z] = DiscPoint(rng);
          const double a = rng.Unit() * 6.283185307179586;
          const auto x2 = x + static_cast<std::int64_t>(std::cos(a) * d);
          const auto z2 = z + static_cast<std::int64_t>(std::sin(a) * d);
          const auto w1 = gen.Continents().Warp(x, z), w2 = gen.Continents().Warp(x2, z2);
          const double dist = std::hypot(static_cast<double>(x2 - x), static_cast<double>(z2 - z));
          const double dw =
              std::hypot(static_cast<double>(w2.x - w1.x), static_cast<double>(w2.z - w1.z));
          worst = std::max(worst, std::max(0.0, dw - 2.0) / dist);
          CHECK(std::max(0.0, dw - 2.0) <= C::kWarpLipschitz * dist);
        }
      }
      MESSAGE("seed " << seed << ": largest warp stretch seen " << worst << " (bound "
                      << C::kWarpLipschitz << ")");
    }
  }

  TEST_CASE("the origin is on land, deep inside its continent") {
    for (std::uint64_t seed = 0; seed < 32; ++seed) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      const auto l = gen.LandAt(0, 0);
      CHECK(l.coast > 100'000.0f);
      CHECK(l.continent == ContinentLayout::IdOf(0, 0));
      CHECK(gen.Continents().CellIsLand(0, 0));
      CHECK(gen.ColumnAt(0, 0).height > 0.0f);
    }
  }

  TEST_CASE("each world has 6–14 continents, a quarter to a third of the disc is land") {
    for (const std::uint64_t seed : kSeeds) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      std::map<std::int32_t, long> area;
      long land = 0, inside = 0, island = 0;
      constexpr int kStep = 64'000;  // m
      for (int z = -core::kWorldRadius + kStep / 2; z < core::kWorldRadius; z += kStep)
        for (int x = -core::kWorldRadius + kStep / 2; x < core::kWorldRadius; x += kStep) {
          if (!core::InsideWorldDisc(x, z)) continue;
          ++inside;
          const auto l = gen.LandAt(x, z);
          if (l.coast <= 0.0f) continue;
          ++land;
          island += l.continent == C::kIslandId;
          CHECK(l.continent != C::kNoContinent);  // all land belongs to a continent or an island
          ++area[l.continent];
        }
      int continents = 0;
      for (const auto& [id, n] : area) continents += id >= 0 && n > 0;
      const double share = static_cast<double>(land) / static_cast<double>(inside);
      MESSAGE(continents << " continents, " << 100 * share << " % land, " << 100.0 * island / inside
                         << " % of it islands");
      CHECK(continents >= 6);
      CHECK(continents <= 14);
      CHECK(continents == gen.Continents().ContinentCount());
      CHECK(share >= 0.25);
      CHECK(share <= 0.35);
      CHECK(island > 0);  // island chains exist
    }
  }

  TEST_CASE("no land within the rim ocean of the disc's edge") {
    for (const std::uint64_t seed : kSeeds) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      const auto dirs = Directions(720);
      int land = 0;
      for (const Dir& d : dirs) {
        for (double r = core::kWorldRadius - C::kRimOcean + 1000.0; r < core::kWorldRadius;
             r += 8000.0) {
          const auto x = static_cast<std::int32_t>(d.x * r), z = static_cast<std::int32_t>(d.z * r);
          if (!core::InsideWorldDisc(x, z)) continue;
          land += gen.LandAt(x, z).coast > 0.0f;
        }
      }
      CHECK(land == 0);
    }
  }

  TEST_CASE("continents keep their distance: sea or the same continent within the ocean gap") {
    // The key guarantee (§2.5): from land points, 64 directions × 4 radii out to 0.99 × the gap.
    const auto dirs = Directions(64);
    const double radii[4] = {0.25, 0.5, 0.75, 0.99};
    const int points = PointsPerSeed();
    for (const std::uint64_t seed : kSeeds) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      Rng rng{seed * 7919 + 1};
      long violations = 0, checked = 0;
      for (const auto& [px, pz] : ContinentPoints(gen, rng, points)) {
        const std::int32_t own = gen.LandAt(px, pz).continent;
        for (const Dir& d : dirs)
          for (const double r : radii) {
            const double dist = r * C::kOceanGap;
            const auto x = static_cast<std::int64_t>(px + d.x * dist);
            const auto z = static_cast<std::int64_t>(pz + d.z * dist);
            if (!core::InsideWorldDisc(static_cast<std::int32_t>(x),
                                       static_cast<std::int32_t>(z))) {
              continue;
            }
            ++checked;
            const auto l = gen.LandAt(static_cast<std::int32_t>(x), static_cast<std::int32_t>(z));
            if (l.coast > 0.0f && l.continent != own) {
              if (++violations <= 3) {
                FAIL_CHECK("land of " << l.continent << " at " << x << "," << z << ", "
                                      << dist / 1000 << " km from " << own << " at " << px << ","
                                      << pz);
              }
            }
          }
      }
      MESSAGE(points << " points, " << checked << " samples, " << violations << " violations");
      CHECK(violations == 0);
    }
  }

  TEST_CASE("islands keep their distance from continents too") {
    const auto dirs = Directions(64);
    const double radii[4] = {0.25, 0.5, 0.75, 0.99};
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      int islands = 0;
      constexpr std::int32_t kStep = 32'000;
      for (std::int32_t pz = -core::kWorldRadius + kStep / 2;
           pz < core::kWorldRadius && islands < 100; pz += kStep)
        for (std::int32_t px = -core::kWorldRadius + kStep / 2; px < core::kWorldRadius;
             px += kStep) {
          if (!core::InsideWorldDisc(px, pz)) continue;
          const auto here = gen.LandAt(px, pz);
          if (here.coast <= 0.0f || here.continent != C::kIslandId) continue;
          ++islands;
          for (const Dir& d : dirs)
            for (const double r : radii) {
              const auto x = static_cast<std::int32_t>(px + d.x * r * C::kOceanGap);
              const auto z = static_cast<std::int32_t>(pz + d.z * r * C::kOceanGap);
              if (!core::InsideWorldDisc(x, z)) continue;
              const auto l = gen.LandAt(x, z);
              CHECK_FALSE((l.coast > 0.0f && l.continent >= 0));
            }
        }
      CHECK(islands >= 30);
    }
  }

  TEST_CASE("the coastline is fractal: longer measured with a finer ruler") {
    // Marching-squares length of the coast in windows on continent coasts: a 1 km grid sees at
    // least 1.5 times the length a 16 km grid does.
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      Rng rng{seed + 77};
      int windows = 0;
      double coarse = 0, fine = 0;
      for (int tries = 0; tries < 200000 && windows < 3; ++tries) {
        const auto [px, pz] = DiscPoint(rng);
        const auto here = gen.LandAt(px, pz);
        if (here.coast <= 0.0f || here.coast > 3000.0f || here.continent < 0) continue;
        const std::int64_t x0 = px - 128'000, z0 = pz - 128'000;
        const auto value = [&](std::int64_t x, std::int64_t z) {
          return gen.LandAt(static_cast<std::int32_t>(x), static_cast<std::int32_t>(z)).coast;
        };
        coarse += ContourLength(value, x0, z0, 256'000, 16'000);
        fine += ContourLength(value, x0, z0, 256'000, 1'000);
        ++windows;
      }
      REQUIRE(windows == 3);
      MESSAGE("coast length: " << coarse / 1000 << " km at 16 km, " << fine / 1000
                               << " km at 1 km (" << fine / coarse << "x)");
      CHECK(fine >= 1.5 * coarse);
    }
  }

  TEST_CASE("each continent has its own character, hashed from its id") {
    const TerrainGenerator gen(3);
    std::set<float> elevations, shelves;
    for (int j = -3; j <= 3; ++j)
      for (int i = -3; i <= 3; ++i) {
        const auto r = gen.Continents().Record(ContinentLayout::IdOf(i, j));
        CHECK(r.elevation >= -8.0f);
        CHECK(r.elevation <= 30.0f);
        CHECK(r.shelf >= 80'000.0f);
        CHECK(r.shelf <= 160'000.0f);
        CHECK(r.wind >= 0);
        CHECK(r.wind < 8);
        CHECK(r.mountainousness >= 0.0f);
        CHECK(r.mountainousness <= 1.0f);
        CHECK(r.temperature_bias >= -10.0f);
        CHECK(r.temperature_bias <= 10.0f);
        elevations.insert(r.elevation);
        shelves.insert(r.shelf);
      }
    CHECK(elevations.size() == 49);
    CHECK(shelves.size() == 49);
    const TerrainGenerator other(4);
    CHECK(gen.Continents().Record(ContinentLayout::IdOf(1, 1)).shelf !=
          other.Continents().Record(ContinentLayout::IdOf(1, 1)).shelf);
  }

  TEST_CASE("internal plate edges are exported for the mountain stage") {
    const TerrainGenerator gen(0);
    Rng rng{3};
    int land = 0, near_edge = 0, positive = 0, negative = 0;
    for (const auto& [x, z] : ContinentPoints(gen, rng, 3000)) {
      const auto l = gen.LandAt(x, z);
      ++land;
      CHECK(l.plate_edge >= 0.0f);
      CHECK(l.plate_edge <= C::kCoastCap);
      CHECK(l.convergence >= -1.0f);
      CHECK(l.convergence <= 1.0f);
      if (l.plate_edge < 10'000.0f) {
        ++near_edge;
        positive += l.convergence > 0.0f;
        negative += l.convergence < 0.0f;
      }
    }
    MESSAGE(near_edge << " of " << land << " land points within 10 km of a plate edge");
    CHECK(near_edge > 20);
    CHECK(positive > 5);  // both convergent and divergent edges
    CHECK(negative > 5);
    // The columns carry the same fields.
    const auto col = gen.ColumnAt(0, 0);
    CHECK(col.plate_edge > 0.0f);
    CHECK(col.continent == ContinentLayout::IdOf(0, 0));
  }

  TEST_CASE("the terrain follows the coast distance: lowland, shelf, slope, abyss") {
    // Sea height by distance offshore: just under the sea at the shore, a shallow shelf (to
    // ~−150 m at its edge, 80–160 km out), a continental slope, and the abyss (−1,200 to −1,800 m);
    // inland, continentalness rises with the distance from the coast.
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      Rng rng{seed + 5};
      int shore = 0, shelf = 0, abyss = 0, inland_near = 0, inland_far = 0;
      double near_sum = 0, far_sum = 0;
      float deepest = 0;
      for (int i = 0; i < 20000; ++i) {
        const auto [x, z] = DiscPoint(rng);
        const auto col = gen.ColumnAt(x, z);
        const float s = col.coast;
        if (s < -600.0f && s > -4'000.0f) {
          ++shore;
          CHECK(col.height > -25.0f);  // lowland or a shallow sea at the shore
          CHECK(col.height < 12.0f);
        } else if (s < -30'000.0f && s > -50'000.0f) {
          ++shelf;
          CHECK(col.height < -8.0f);  // out on the shelf, not yet at its edge
          CHECK(col.height > -80.0f);
        } else if (s < -200'000.0f) {
          ++abyss;
          CHECK(col.height < -1190.0f);  // the abyss: −1,500 ± 300 m (± the hills' few metres)
          CHECK(col.height > -1810.0f);
          deepest = std::min(deepest, col.height);
          CHECK(worldgen::IsSeaBiome(col.biome));
        } else if (s > 500.0f && s < 3'000.0f && col.continent >= 0) {
          ++inland_near;
          near_sum += col.continentalness;
        } else if (s > 100'000.0f) {
          ++inland_far;
          far_sum += col.continentalness;
        }
      }
      MESSAGE(shore << " shore, " << shelf << " shelf, " << abyss << " abyss samples; deepest "
                    << deepest << " m");
      CHECK(shore > 20);
      CHECK(shelf > 20);
      CHECK(abyss > 200);
      CHECK(inland_near > 5);
      CHECK(inland_far > 200);
      CHECK(far_sum / inland_far > near_sum / inland_near + 0.3);  // the interior is more inland
      CHECK(deepest > static_cast<float>(core::kWorldMinY + 100));
    }
  }

  TEST_CASE("the generated terrain agrees with the layout: land is land") {
    // ColumnAt reads the coast from the macro lattice; LandAt evaluates it at the point. They agree
    // on land and sea except within metres of the coast.
    const TerrainGenerator gen(1);
    Rng rng{8};
    int disagree = 0, total = 0;
    for (int i = 0; i < 4000; ++i) {
      const auto [x, z] = DiscPoint(rng);
      const auto col = gen.ColumnAt(x, z);
      const auto l = gen.LandAt(x, z);
      ++total;
      if ((col.coast > 0.0f) != (l.coast > 0.0f) && std::abs(l.coast) > 200.0f) ++disagree;
      CHECK(std::abs(col.coast - l.coast) < 2000.0f);
    }
    CHECK(disagree == 0);
    CHECK(total == 4000);
  }
}
