// Finding rivers, lakes and waterfalls, for tests (worldgen and LOD; Phase 11a). Rivers run where a
// tier's noise (rivers.h) is zero, so a centreline is found by looking for a sign change between
// neighbouring samples and bisecting; a river is followed by stepping along the contour.
#pragma once

#include <cmath>
#include <cstdint>
#include <optional>
#include <utility>
#include <vector>

#include "dwell/worldgen/rivers.h"
#include "dwell/worldgen/terrain.h"

namespace dwell::testing {

enum class Tier { kGreat, kRiver, kStream };

struct Point {
  std::int32_t x = 0, z = 0;
};

inline float TierValue(const worldgen::TerrainGenerator& gen, Tier tier, std::int32_t x,
                       std::int32_t z) {
  const auto c = gen.RiversAt(x, z);
  return tier == Tier::kGreat ? c.rg : tier == Tier::kRiver ? c.r1 : c.r2;
}

// The unit normal of a tier's contour at (x, z) (towards rising noise), from central differences.
inline std::pair<double, double> TierNormal(const worldgen::TerrainGenerator& gen, Tier tier,
                                            std::int32_t x, std::int32_t z, int h = 4) {
  const double gx = TierValue(gen, tier, x + h, z) - TierValue(gen, tier, x - h, z);
  const double gz = TierValue(gen, tier, x, z + h) - TierValue(gen, tier, x, z - h);
  const double len = std::sqrt(gx * gx + gz * gz);
  return len > 0.0 ? std::pair{gx / len, gz / len} : std::pair{1.0, 0.0};
}

// Bisects between a and b (where the tier's noise changes sign) to the contour, to a metre.
inline Point BisectContour(const worldgen::TerrainGenerator& gen, Tier tier, Point a, Point b) {
  float va = TierValue(gen, tier, a.x, a.z);
  for (int i = 0; i < 40; ++i) {
    const Point m{(a.x + b.x) / 2, (a.z + b.z) / 2};
    if (std::abs(b.x - a.x) <= 1 && std::abs(b.z - a.z) <= 1) break;
    const float vm = TierValue(gen, tier, m.x, m.z);
    if ((vm < 0.0f) == (va < 0.0f)) {
      a = m;
      va = vm;
    } else {
      b = m;
    }
  }
  return a;
}

// Up to `count` points on a tier's centrelines within `half` m of (x0, z0), found along east-west
// scan lines `step` m apart (sampled every `step` m): a sign change with both values small is a
// contour (a large jump is the noise's own sign flip across a lattice cell, which has no channel).
// Only points on land with a channel in them (wet > 0) are kept.
inline std::vector<Point> FindCentrelines(const worldgen::TerrainGenerator& gen, Tier tier,
                                          std::int32_t x0, std::int32_t z0, std::int32_t half,
                                          std::int32_t step, std::size_t count) {
  std::vector<Point> out;
  for (std::int32_t z = z0 - half; z <= z0 + half && out.size() < count; z += step) {
    float prev = TierValue(gen, tier, x0 - half, z);
    for (std::int32_t x = x0 - half + step; x <= x0 + half && out.size() < count; x += step) {
      const float v = TierValue(gen, tier, x, z);
      if ((v < 0.0f) != (prev < 0.0f) && std::abs(v - prev) < 0.5f) {
        const Point p = BisectContour(gen, tier, {x - step, z}, {x, z});
        const auto col = gen.ColumnAt(p.x, p.z);
        if (!col.outside && col.coast > 0.0f && col.wet > 0.0f && !col.lake) out.push_back(p);
      }
      prev = v;
    }
  }
  return out;
}

// A column inside a lake, found on a grid within `half` m of the origin.
inline std::optional<Point> FindLake(const worldgen::TerrainGenerator& gen,
                                     std::int32_t half = 60000, std::int32_t step = 400) {
  for (std::int32_t r = 0; r <= half; r += step)
    for (std::int32_t a = -r; a <= r; a += step)
      for (const std::int32_t b : {-r, r}) {
        if (gen.ColumnAt(a, b).lake) return Point{a, b};
        if (gen.ColumnAt(b, a).lake) return Point{b, a};
      }
  return std::nullopt;
}

// Follows a tier's contour from `from` (on it) for up to `length` m in steps of `step` m, and
// returns the first point where the water surface changes between two consecutive steps by a
// terrace (a waterfall): the point before the step, and the one after.
inline std::optional<std::pair<Point, Point>> FindWaterfall(
    const worldgen::TerrainGenerator& gen, Tier tier, Point from, std::int32_t length,
    std::int32_t step = 8) {
  Point p = from;
  auto n = TierNormal(gen, tier, p.x, p.z);
  // Walk both ways along the tangent (perpendicular to the normal).
  for (const double dir : {1.0, -1.0}) {
    p = from;
    std::optional<std::int32_t> last_water;
    Point last = p;
    for (std::int32_t walked = 0; walked < length; walked += step) {
      n = TierNormal(gen, tier, p.x, p.z);
      const double tx = -n.second * dir, tz = n.first * dir;
      Point q{static_cast<std::int32_t>(std::lround(p.x + tx * step)),
              static_cast<std::int32_t>(std::lround(p.z + tz * step))};
      // Back onto the contour: bisect along the normal across the sign change.
      const float v = TierValue(gen, tier, q.x, q.z);
      const double off = v < 0.0f ? 1.0 : -1.0;
      for (const int reach : {2, 4, 8, 16, 32}) {
        const Point r{static_cast<std::int32_t>(std::lround(q.x + n.first * off * reach)),
                      static_cast<std::int32_t>(std::lround(q.z + n.second * off * reach))};
        if ((TierValue(gen, tier, r.x, r.z) < 0.0f) != (v < 0.0f)) {
          q = BisectContour(gen, tier, q, r);
          break;
        }
      }
      p = q;
      const auto col = gen.ColumnAt(p.x, p.z);
      if (col.outside || col.wet <= 0.0f || col.height >= static_cast<float>(col.water)) {
        last_water.reset();
        continue;
      }
      if (last_water && std::abs(col.water - *last_water) >= 2) return std::pair{last, p};
      last_water = col.water;
      last = p;
    }
  }
  return std::nullopt;
}

// Water landmarks of a seed, for the golden tests: a lake, a stream, a river, a great river (each
// the first found by the searches above, near the origin) and a waterfall on one of them. Only
// sqrt and the noise are used, so native and WASM builds find the same ones.
struct WaterLandmarks {
  Point lake, stream, river, great, waterfall;
  bool found = false;
};

inline WaterLandmarks FindWaterLandmarks(const worldgen::TerrainGenerator& gen) {
  WaterLandmarks m;
  const auto lake = FindLake(gen);
  const auto streams = FindCentrelines(gen, Tier::kStream, 0, 0, 12000, 200, 3);
  const auto rivers = FindCentrelines(gen, Tier::kRiver, 0, 0, 12000, 400, 3);
  const auto greats = FindCentrelines(gen, Tier::kGreat, 0, 0, 240000, 8000, 3);
  if (!lake || streams.empty() || rivers.empty() || greats.empty()) return m;
  m.lake = *lake;
  m.stream = streams[0];
  m.river = rivers[0];
  m.great = greats[0];
  // A waterfall: along the first great river or river that has one.
  for (const Point& p : greats)
    if (const auto fall = FindWaterfall(gen, Tier::kGreat, p, 60000, 16)) {
      m.waterfall = fall->first;
      m.found = true;
      return m;
    }
  for (const Point& p : rivers)
    if (const auto fall = FindWaterfall(gen, Tier::kRiver, p, 20000, 8)) {
      m.waterfall = fall->first;
      m.found = true;
      return m;
    }
  for (const Point& p : streams)
    if (const auto fall = FindWaterfall(gen, Tier::kStream, p, 8000, 4)) {
      m.waterfall = fall->first;
      m.found = true;
      return m;
    }
  return m;
}

}  // namespace dwell::testing
