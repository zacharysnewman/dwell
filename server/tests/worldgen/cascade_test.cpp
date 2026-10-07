// The derivative-damped ridged cascade of mountain detail (WORLD_GENERATION.md §3.4, Phase 11b):
// the analytic gradient noise it is built on, the cascade's range, its damping of steep ground, and
// the octaves a level-of-detail cell drops. Runs natively (dwell_tests) and under Node
// (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <initializer_list>

#include "dwell/worldgen/noise.h"

using namespace dwell;
using worldgen::DampedRidges2;
using worldgen::Lattice;
using worldgen::Noise2d;
using worldgen::Perlin2d;

TEST_SUITE("worldgen: cascade") {
  TEST_CASE(
      "analytic gradient noise: the value is Perlin2's and the gradient matches differences") {
    float worst = 0.0f;
    for (std::uint32_t seed : {1u, 2u, 3u}) {
      for (int i = 0; i < 2000; ++i) {
        const float x =
            -40.0f + static_cast<float>(i) * 0.0431f + 0.013f * static_cast<float>(seed);
        const float z = 17.0f - static_cast<float>(i) * 0.0287f;
        const auto split = [](float v) {
          const std::int32_t cell = worldgen::FloorToInt(v);
          return worldgen::LatticeCoord{cell, v - static_cast<float>(cell)};
        };
        const Noise2d n = Perlin2d(seed, split(x), split(z));
        CHECK(n.value == doctest::Approx(worldgen::Perlin2(seed, x, z)).epsilon(1e-5));
        constexpr float h = 1.0f / 256.0f;  // exact in float
        const float dx =
            (worldgen::Perlin2(seed, x + h, z) - worldgen::Perlin2(seed, x - h, z)) / (2 * h);
        const float dz =
            (worldgen::Perlin2(seed, x, z + h) - worldgen::Perlin2(seed, x, z - h)) / (2 * h);
        worst = std::max(worst, std::max(std::abs(n.dx - dx), std::abs(n.dz - dz)));
      }
    }
    MESSAGE("worst gradient error " << worst);
    CHECK(worst < 0.02f);  // central differences of a cubic-ish field: well within
  }

  TEST_CASE("the cascade is in [0, 1], deterministic, and has relief") {
    double sum = 0, sum_sq = 0;
    float lo = 1, hi = 0;
    int n = 0;
    for (int j = 0; j < 200; ++j)
      for (int i = 0; i < 200; ++i) {
        const std::int64_t x = -3'000'000 + i * 811LL, z = 2'000'000 + j * 977LL;
        const float v = DampedRidges2(9, x, z, 4096, 9, 600.0f, 0.6f);
        CHECK(v == DampedRidges2(9, x, z, 4096, 9, 600.0f, 0.6f));
        lo = std::min(lo, v);
        hi = std::max(hi, v);
        sum += v;
        sum_sq += static_cast<double>(v) * v;
        ++n;
      }
    const double mean = sum / n, sd = std::sqrt(sum_sq / n - mean * mean);
    MESSAGE("cascade: min " << lo << " max " << hi << " mean " << mean << " sd " << sd);
    CHECK(lo >= 0.0f);
    CHECK(hi <= 1.0f);
    CHECK(hi > 0.3f);  // crests exist
    CHECK(sd > 0.03);  // and the field varies
  }

  TEST_CASE("damping smooths steep ground: a stronger damping lowers the cascade's roughness") {
    // Roughness: mean absolute difference of neighbouring samples 16 m apart.
    const auto roughness = [](float damping) {
      double total = 0;
      int n = 0;
      for (int j = 0; j < 120; ++j)
        for (int i = 0; i < 120; ++i) {
          const std::int64_t x = 1'000'000 + i * 613LL, z = 500'000 + j * 701LL;
          total += std::abs(DampedRidges2(5, x + 16, z, 4096, 9, 600.0f, damping) -
                            DampedRidges2(5, x, z, 4096, 9, 600.0f, damping));
          ++n;
        }
      return total / n;
    };
    const double none = roughness(0.0f), some = roughness(0.6f), lots = roughness(6.0f);
    MESSAGE("roughness: " << none << " / " << some << " / " << lots);
    CHECK(some < none);
    CHECK(lots < some);
  }

  TEST_CASE(
      "detail multiplies with height: finer octaves add more where the coarse ones are high") {
    // The part the finer octaves add (full cascade less its first four octaves, which have the same
    // normalisation up to a constant) is larger over high coarse ground than over low.
    double low_sum = 0, high_sum = 0;
    int low_n = 0, high_n = 0;
    for (int i = 0; i < 20000; ++i) {
      const std::int64_t x = 200'000 + i * 331LL, z = -700'000 + i * 457LL;
      const float coarse = DampedRidges2(7, x, z, 4096, 9, 600.0f, 0.6f, 3);
      const float full = DampedRidges2(7, x, z, 4096, 9, 600.0f, 0.6f);
      const float fine = std::abs(full - coarse);
      if (coarse < 0.35f) {
        low_sum += fine, ++low_n;
      } else if (coarse > 0.65f) {
        high_sum += fine, ++high_n;
      }
    }
    REQUIRE(low_n > 100);
    REQUIRE(high_n > 100);
    MESSAGE("fine detail: " << low_sum / low_n << " over low ground, " << high_sum / high_n
                            << " over high");
    CHECK(high_sum / high_n > 1.5 * (low_sum / low_n));
  }

  TEST_CASE("level of detail: dropped octaves leave the coarse shape; none kept reads zero") {
    CHECK(DampedRidges2(3, 1000, 2000, 4096, 9, 600.0f, 0.6f, 0) == 0.0f);
    // The first octaves alone agree with the full cascade on the coarse shape (correlation).
    double sxy = 0, sx = 0, sy = 0, sxx = 0, syy = 0;
    int n = 0;
    for (int i = 0; i < 6000; ++i) {
      const std::int64_t x = -900'000 + i * 523LL, z = 100'000 + i * 389LL;
      const double a = DampedRidges2(3, x, z, 4096, 9, 600.0f, 0.6f, 4);
      const double b = DampedRidges2(3, x, z, 4096, 9, 600.0f, 0.6f);
      sx += a, sy += b, sxy += a * b, sxx += a * a, syy += b * b, ++n;
    }
    const double cov = sxy / n - sx / n * sy / n;
    const double corr = cov / std::sqrt((sxx / n - sx / n * sx / n) * (syy / n - sy / n * sy / n));
    MESSAGE("correlation of 4 kept octaves with the full cascade: " << corr);
    CHECK(corr > 0.8);
  }
}

#include "dwell/worldgen/terrain.h"

namespace {
using worldgen::Column;
using worldgen::TerrainGenerator;

// Mean absolute height change over 16 m (a measure of roughness) of the land columns a predicate
// picks, over a coarse scan of the world.
template <class Pick>
double Roughness(const TerrainGenerator& gen, Pick&& pick, int& samples) {
  double total = 0;
  samples = 0;
  for (std::int32_t z = -4'000'000; z <= 4'000'000; z += 25'000)
    for (std::int32_t x = -4'000'000; x <= 4'000'000; x += 25'000) {
      const Column c = gen.ColumnAt(x, z);
      if (c.outside || c.coast <= 5000.0f || c.wet > 0.0f || !pick(c)) continue;
      const Column d = gen.ColumnAt(x + 16, z);
      total += std::abs(d.height - c.height);
      ++samples;
    }
  return samples ? total / samples : 0.0;
}
}  // namespace

TEST_SUITE("worldgen: mountain detail") {
  TEST_CASE("the cascade is on where there is uplift: mountains are rougher than lowland") {
    const TerrainGenerator gen(0);
    int n_mountain = 0, n_low = 0;
    const double mountain =
        Roughness(gen, [](const Column& c) { return c.mountain > 0.7f; }, n_mountain);
    const double lowland = Roughness(
        gen, [](const Column& c) { return c.mountain < 0.02f && c.cascade == 0.0f; }, n_low);
    MESSAGE("roughness over 16 m: mountains " << mountain << " (" << n_mountain
                                              << " samples), lowland " << lowland << " (" << n_low
                                              << ")");
    REQUIRE(n_mountain > 30);
    REQUIRE(n_low > 100);
    CHECK(mountain > 3.0 * lowland);
  }

  TEST_CASE("the cascade is zero at sea and in [0, 1] on land; its mean is the level of detail's") {
    double sum = 0;
    long n = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull}) {
      const TerrainGenerator gen(seed);
      for (std::int32_t z = -5'000'000; z <= 5'000'000; z += 40'000)
        for (std::int32_t x = -5'000'000; x <= 5'000'000; x += 40'000) {
          const Column c = gen.ColumnAt(x, z);
          if (c.outside) continue;
          CHECK(c.cascade >= 0.0f);
          CHECK(c.cascade <= 1.0f);
          if (c.coast < -2000.0f) CHECK(c.cascade == 0.0f);
          if (c.cascade > 0.0f) sum += c.cascade, ++n;
        }
    }
    REQUIRE(n > 500);
    // terrain.cpp's kLodCascade (0.55): what a cell too wide for any octave reads.
    MESSAGE("mean cascade where it is evaluated: " << sum / n);
    CHECK(sum / n == doctest::Approx(0.55).epsilon(0.1));
  }

  TEST_CASE("mountains rise away from rivers: the relief is damped in and beside a channel") {
    // The cascade's amplitude is scaled by the distance factor (rivers.h): in a channel the ground
    // is at the valley floor, however mountainous the place.
    double wet_relief = 0, dry_relief = 0;
    long wet_n = 0, dry_n = 0;
    for (const std::uint64_t seed : {0ull, 1ull, 2ull, 3ull}) {
      const TerrainGenerator gen(seed);
      for (std::int32_t z = -3'000'000; z <= 3'000'000; z += 20'000)
        for (std::int32_t x = -3'000'000; x <= 3'000'000; x += 20'000) {
          const Column c = gen.ColumnAt(x, z);
          if (c.outside || c.coast <= 5000.0f || c.mountain < 0.5f || c.cascade < 0.3f) continue;
          if (c.lake) continue;
          const float relief = c.height - c.valley;
          if (c.wet > 0.3f) {
            wet_relief += relief, ++wet_n;
          } else if (c.wet == 0.0f) {
            dry_relief += relief, ++dry_n;
          }
        }
    }
    REQUIRE(wet_n > 100);
    REQUIRE(dry_n > 500);
    MESSAGE("mean relief above the valley floor: " << wet_relief / wet_n << " m in channels ("
                                                   << wet_n << "), " << dry_relief / dry_n
                                                   << " m elsewhere (" << dry_n << ")");
    CHECK(wet_relief / wet_n < 0.25 * (dry_relief / dry_n));
  }
}
