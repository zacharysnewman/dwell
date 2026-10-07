// Finding a column of a given biome, for tests (worldgen and LOD). Land biomes are looked for on a
// coarse grid around the origin; the sea and its beaches only occur at a coast, which on the
// plate layout (WORLD_GENERATION.md §2) is hundreds of kilometres from the origin, so those are
// searched around the nearest coasts.
#pragma once

#include <cstdint>
#include <optional>
#include <utility>

#include "dwell/worldgen/terrain.h"

namespace dwell::testing {

// A point (x, z) where `is(x, z)` holds: on a coarse grid around the origin, then rings out to the
// rim (climate regions are hundreds of kilometres across), or — if `coastal`, for what only occurs
// at a coast — around the coast met walking out from the origin in each of eight directions.
template <class Is>
std::optional<std::pair<std::int32_t, std::int32_t>> FindPoint(
    const worldgen::TerrainGenerator& gen, Is&& is, bool coastal = false) {
  if (!coastal) {
    for (int r = 0; r < 4000; r += 48)
      for (int x = -r; x <= r; x += 48)
        for (const int z : {-r, r}) {
          if (is(x, z)) return std::pair{x, z};
          if (is(z, x)) return std::pair{z, x};
        }
    for (std::int32_t r = 192'000; r <= 7'000'000; r += 24'000)
      for (std::int32_t a = -r; a <= r; a += 24'000)
        for (const std::int32_t b : {-r, r}) {
          if (core::InsideWorldDisc(a, b) && is(a, b)) return std::pair{a, b};
          if (core::InsideWorldDisc(b, a) && is(b, a)) return std::pair{b, a};
        }
    return std::nullopt;
  }
  constexpr int kDirs[8][2] = {{1, 0}, {0, 1},  {-1, 0}, {0, -1},
                               {1, 1}, {-1, 1}, {1, -1}, {-1, -1}};
  for (const auto& d : kDirs) {
    std::int32_t x = 0, z = 0;
    for (int n = 0; n < 1000 && gen.LandAt(x, z).coast > 0.0f; ++n) {
      x += d[0] * 4000;
      z += d[1] * 4000;
    }
    if (!core::InsideWorldDisc(x, z)) continue;
    for (int r = 0; r <= 6000; r += 48)
      for (int a = -r; a <= r; a += 48)
        for (const int b : {-r, r}) {
          if (is(x + a, z + b)) return std::pair{x + a, z + b};
          if (is(x + b, z + a)) return std::pair{x + b, z + a};
        }
  }
  return std::nullopt;
}

// A dry column satisfying `pred` (FindPoint's search).
template <class Pred>
std::optional<std::pair<std::int32_t, std::int32_t>> FindColumn(
    const worldgen::TerrainGenerator& gen, Pred&& pred, bool coastal = false) {
  return FindPoint(
      gen,
      [&](std::int32_t x, std::int32_t z) {
        const worldgen::Column c = gen.ColumnAt(x, z);
        return !c.outside && c.wet == 0.0f && pred(c);
      },
      coastal);
}

// A point well inside a region of the biome: the 5 × 5 columns 400 m apart around it (1.6 km
// across) are all the biome's and dry.
inline std::optional<std::pair<std::int32_t, std::int32_t>> FindBiomeInterior(
    const worldgen::TerrainGenerator& gen, worldgen::Biome biome) {
  return FindPoint(gen, [&](std::int32_t x, std::int32_t z) {
    for (int dz = -2; dz <= 2; ++dz)
      for (int dx = -2; dx <= 2; ++dx) {
        const worldgen::Column c = gen.ColumnAt(x + dx * 400, z + dz * 400);
        if (c.outside || c.wet != 0.0f || c.biome != biome) return false;
      }
    return true;
  });
}

// A dry column of the biome. The sea's biomes and the land's climate biomes are found on the grid
// of FindColumn; beaches and sea cliffs, which only occur at a coast, around the coasts.
inline std::optional<std::pair<std::int32_t, std::int32_t>> FindBiome(
    const worldgen::TerrainGenerator& gen, worldgen::Biome biome) {
  using worldgen::Biome;
  const auto is = [&](const worldgen::Column& c) { return c.biome == biome; };
  const bool coastal = biome == Biome::kBeach || biome == Biome::kSeaCliff;
  return FindColumn(gen, is, coastal);
}

// A dry column where the ground stands over 200 m above its valley floor: mountains.
inline std::optional<std::pair<std::int32_t, std::int32_t>> FindMountain(
    const worldgen::TerrainGenerator& gen) {
  return FindColumn(gen, [](const worldgen::Column& c) {
    return c.coast > 5000.0f && c.height - c.valley > 200.0f;
  });
}

// Landmarks of the plate layout (WORLD_GENERATION.md §2), found by scanning the layout alone: where
// the golden tests and the LOD tests look at the coast, an ocean gap between two continents, an
// island, a continent's interior and the abyss. All pure functions of the seed.
struct Landmarks {
  std::pair<std::int32_t, std::int32_t> coast{}, gap{}, island{}, interior{}, abyss{};
  bool found = false;
};

inline Landmarks FindLandmarks(const worldgen::TerrainGenerator& gen) {
  using worldgen::continents::kIslandId;
  Landmarks m;
  // The coast: east from the origin to where the land ends, refined to the metre.
  std::int32_t lo = 0, hi = 0;
  while (gen.LandAt(hi, 0).coast > 0.0f) hi += 1000;
  lo = hi - 1000;
  while (hi - lo > 1) {
    const std::int32_t mid = lo + (hi - lo) / 2;
    (gen.LandAt(mid, 0).coast > 0.0f ? lo : hi) = mid;
  }
  m.coast = {lo, 0};
  bool gap = false, island = false, interior = false, abyss = false;
  constexpr std::int32_t kStep = 64'000;
  for (std::int32_t z = -core::kWorldRadius + kStep / 2; z < core::kWorldRadius; z += kStep)
    for (std::int32_t x = -core::kWorldRadius + kStep / 2; x < core::kWorldRadius; x += kStep) {
      if (!core::InsideWorldDisc(x, z)) continue;
      const auto l = gen.LandAt(x, z);
      if (!island && l.coast > 0.0f && l.continent == kIslandId) {
        island = true;
        m.island = {x, z};
      }
      if (!interior && l.coast > 200'000.0f && l.continent >= 0 &&
          l.continent != worldgen::ContinentLayout::IdOf(0, 0)) {
        interior = true;
        m.interior = {x, z};
      }
      if (!abyss && l.coast < -200'000.0f && gen.ColumnAt(x, z).height < -1100.0f) {
        abyss = true;
        m.abyss = {x, z};
      }
      if (!gap && l.coast < -150'000.0f) {
        // Between two continents: land of different continents on both sides, either way.
        for (const auto& d : {std::pair{250'000, 0}, std::pair{0, 250'000}}) {
          const auto a = gen.LandAt(x + d.first, z + d.second);
          const auto b = gen.LandAt(x - d.first, z - d.second);
          if (a.coast > 0.0f && b.coast > 0.0f && a.continent >= 0 && b.continent >= 0 &&
              a.continent != b.continent) {
            gap = true;
            m.gap = {x, z};
          }
        }
      }
    }
  m.found = gap && island && interior && abyss;
  return m;
}

}  // namespace dwell::testing
