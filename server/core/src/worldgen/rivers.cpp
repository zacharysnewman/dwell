#include "dwell/worldgen/rivers.h"

namespace dwell::worldgen::rivers {
namespace {

// Hashes of a grid cell mixed coordinate by coordinate (Hash2 repeats for small coordinates; see
// continents.cpp).
constexpr std::uint32_t CellHash(std::uint32_t seed, std::int32_t i, std::int32_t j) {
  const std::uint32_t h = Mix32(seed + static_cast<std::uint32_t>(i) * 0x9E3779B1u);
  return Mix32(h ^ (static_cast<std::uint32_t>(j) * 0x85EBCA77u + 0x27D4EB2Fu));
}

constexpr std::int64_t FloorDiv64(std::int64_t a, std::int64_t b) {
  return a / b - ((a % b != 0) && ((a < 0) != (b < 0)) ? 1 : 0);
}

// The boundary (m) of terrace j ≥ 1: 4j displaced by a hashed −1, 0 or +1.
float Boundary(std::uint32_t seed, std::int32_t j) {
  return static_cast<float>(j) * kTerraceStep +
         static_cast<float>(static_cast<std::int32_t>(CellHash(seed, j, 0x7E44) % 3u) - 1);
}

}  // namespace

float TerraceSurface(std::uint32_t seed, float v) {
  if (v < Boundary(seed, 1)) return 0.0f;
  const std::int32_t k = FloorToInt(v * (1.0f / kTerraceStep));
  // Boundaries stay within a metre of 4j, so the terrace holding v starts at j = k − 1, k or k + 1.
  for (std::int32_t j = k + 1; j >= k - 1; --j) {
    if (j >= 1 && v >= Boundary(seed, j)) return Boundary(seed, j);
  }
  return 0.0f;
}

namespace {

// Per-thread cache of lake surfaces: the oracle evaluates the whole terrain pipeline at the lake's
// centre, which every corner near a lake would repeat. A pure function of (world seed, lake cell),
// so the cache changes speed, never values.
struct LakeLevelEntry {
  std::uint32_t seed = 0;
  std::int32_t i = 0, j = 0;
  float level = 0.0f;
  bool valid = false;
};
constexpr std::size_t kLakeCacheSize = 64;  // power of two

float CachedLakeLevel(const Seeds& s, std::int32_t i, std::int32_t j, std::int64_t sx,
                      std::int64_t sz, const LevelOracle& oracle) {
  thread_local LakeLevelEntry cache[kLakeCacheSize];
  LakeLevelEntry& e = cache[CellHash(s.lake, i, j) & (kLakeCacheSize - 1)];
  if (!e.valid || e.seed != s.lake || e.i != i || e.j != j) {
    e = {s.lake, i, j, oracle.LakeLevel(sx, sz), true};
  }
  return e.level;
}

}  // namespace

namespace {

// The smooth fields — the great river's windings (30 km) and the spring noise (12 km) — vary far
// less than a metre over 256 m, so they are evaluated at the corners of a 256 m lattice and
// interpolated bilinearly (the error is well under a metre of displacement and 0.002 of noise).
// Corners are cached per thread, a pure function of (seed, lattice point): chunks, point queries and
// the level of detail all read the same interpolated values.
constexpr std::int64_t kSmoothStep = 256;
struct SmoothCorner {
  float gx, gz, spring;
};
struct SmoothEntry {
  std::uint32_t seed = 0;
  std::int32_t mx = 0, mz = 0;
  SmoothCorner c[4];
  bool valid = false;
};
constexpr std::size_t kSmoothCacheSize = 16;  // power of two

SmoothCorner SmoothCornerAt(const Seeds& s, std::int64_t x, std::int64_t z) {
  SmoothCorner c;
  c.gx = Perlin2(s.great_meander_x, Lattice(x, kGreatMeanderWavelength),
                 Lattice(z, kGreatMeanderWavelength));
  c.gz = Perlin2(s.great_meander_z, Lattice(x, kGreatMeanderWavelength),
                 Lattice(z, kGreatMeanderWavelength));
  c.spring = Perlin2(s.spring, Lattice(x + s.offset_x[1], kSpringWavelength),
                     Lattice(z + s.offset_z[1], kSpringWavelength));
  return c;
}

// The smooth fields at (x, z): the windings' offsets are returned as unit noise (before the
// amplitude), the spring noise as is.
SmoothCorner SmoothFields(const Seeds& s, std::int64_t x, std::int64_t z) {
  thread_local SmoothEntry cache[kSmoothCacheSize];
  const auto mx = static_cast<std::int32_t>(FloorDiv64(x, kSmoothStep));
  const auto mz = static_cast<std::int32_t>(FloorDiv64(z, kSmoothStep));
  SmoothEntry& e = cache[CellHash(s.spring, mx, mz) & (kSmoothCacheSize - 1)];
  if (!e.valid || e.seed != s.spring || e.mx != mx || e.mz != mz) {
    e.seed = s.spring;
    e.mx = mx;
    e.mz = mz;
    for (int k = 0; k < 4; ++k) {
      e.c[k] = SmoothCornerAt(s, (std::int64_t{mx} + (k & 1)) * kSmoothStep,
                              (std::int64_t{mz} + (k >> 1)) * kSmoothStep);
    }
    e.valid = true;
  }
  // 256 is a power of two: the offsets within the cell are exact.
  const float tx = static_cast<float>(x - std::int64_t{mx} * kSmoothStep) * (1.0f / 256.0f);
  const float tz = static_cast<float>(z - std::int64_t{mz} * kSmoothStep) * (1.0f / 256.0f);
  const auto lerp2 = [&](float SmoothCorner::*f) {
    return Lerp(Lerp(e.c[0].*f, e.c[1].*f, tx), Lerp(e.c[2].*f, e.c[3].*f, tx), tz);
  };
  return {lerp2(&SmoothCorner::gx), lerp2(&SmoothCorner::gz), lerp2(&SmoothCorner::spring)};
}

}  // namespace

Seeds MakeSeeds(std::uint64_t world_seed) {
  Seeds s;
  s.great = SeedWord(world_seed, 201);
  s.river = SeedWord(world_seed, 202);
  s.stream = SeedWord(world_seed, 203);
  s.meander_x = SeedWord(world_seed, 204);
  s.meander_z = SeedWord(world_seed, 205);
  s.terrace = SeedWord(world_seed, 206);
  s.lake = SeedWord(world_seed, 207);
  s.shore = SeedWord(world_seed, 208);
  s.spring = SeedWord(world_seed, 209);
  s.great_meander_x = SeedWord(world_seed, 220);
  s.great_meander_z = SeedWord(world_seed, 221);
  const std::int32_t wavelengths[3] = {kGreat.wavelength, kRiver.wavelength, kStream.wavelength};
  for (int t = 0; t < 3; ++t) {
    s.offset_x[t] = Mix32(SeedWord(world_seed, 210 + 2 * t)) % static_cast<std::uint32_t>(wavelengths[t]);
    s.offset_z[t] = Mix32(SeedWord(world_seed, 211 + 2 * t)) % static_cast<std::uint32_t>(wavelengths[t]);
  }
  return s;
}

namespace {

// Factor a tier's noise is divided by so its channel is at least one cell wide (≥ 1).
template <const Tier& T>
float Widen(std::int64_t cell) {
  if (cell <= 0) return 1.0f;
  const float one_cell = static_cast<float>(cell) / static_cast<float>(T.wavelength);
  return one_cell > T.core ? one_cell / T.core : 1.0f;
}

// Perlin noise (zero on the lattice's points, within ±~0.7) of one tier at (x, z), widened for a
// cell; 1 (far from any channel) for a tier the cell is too wide to show. The tier is a template
// argument so that the lattice's divisions are by constants.
template <const Tier& T>
float TierNoise(std::uint32_t seed, std::int64_t x, std::int64_t z, std::int64_t cell) {
  if (T.drop_cell > 0 && cell >= T.drop_cell) return 1.0f;
  const float v = Perlin2(seed, Lattice(x, T.wavelength), Lattice(z, T.wavelength));
  return cell <= 0 ? v : v / Widen<T>(cell);
}

struct Meander {
  std::int64_t x = 0, z = 0;
};

}  // namespace

Corner Sample(const Seeds& s, std::int64_t x, std::int64_t z, std::int64_t cell,
              const LevelOracle& oracle) {
  Corner c;
  if (cell >= kLakeSkipCell) {
    // Lakes (≤ 4 km across) and the great river's windings (2.5 km) are sub-cell here.
    c.rg = TierNoise<kGreat>(s.great, x + s.offset_x[0], z + s.offset_z[0], cell);
    return c;
  }
  // From 256 m cells on, neighbouring samples lie in different lattice cells: evaluate directly.
  const SmoothCorner smooth =
      cell >= kSmoothStep ? SmoothCornerAt(s, x, z) : SmoothFields(s, x, z);
  {
    // The great river winds gently across its 200 km wavelength.
    const std::int64_t gx = FloorToInt(smooth.gx * kGreatMeanderAmplitude);
    const std::int64_t gz = FloorToInt(smooth.gz * kGreatMeanderAmplitude);
    c.rg = TierNoise<kGreat>(s.great, x + gx + s.offset_x[0], z + gz + s.offset_z[0], cell);
  }
  // The two small tiers meander too: sampled at a position displaced by a gentle vector noise.
  const bool small_tiers = cell < kRiver.drop_cell;
  if (small_tiers) {
    Meander m;
    m.x = FloorToInt(Perlin2(s.meander_x, Lattice(x, kMeanderWavelength),
                             Lattice(z, kMeanderWavelength)) *
                     kMeanderAmplitude);
    m.z = FloorToInt(Perlin2(s.meander_z, Lattice(x, kMeanderWavelength),
                             Lattice(z, kMeanderWavelength)) *
                     kMeanderAmplitude);
    c.r1 = TierNoise<kRiver>(s.river, x + m.x + s.offset_x[1], z + m.z + s.offset_z[1], cell);
    c.r2 = TierNoise<kStream>(s.stream, x + m.x + s.offset_x[2], z + m.z + s.offset_z[2], cell);
    c.spring = smooth.spring;
  }

  // The lake of the point's own cell, if any. Sites lie 0.25–0.75 of a cell in (≥ 3,072 m from the
  // cell's borders) and a lake's reach, with its berm and the shore's wobble, is under 2,800 m, so
  // a lake never reaches a neighbouring cell.
  {
    const auto i = static_cast<std::int32_t>(FloorDiv64(x, kLakeCell));
    const auto j = static_cast<std::int32_t>(FloorDiv64(z, kLakeCell));
    const std::uint32_t h = CellHash(s.lake, i, j);
    if (Unit(h) < kLakeChance) {
      const std::uint32_t h1 = Mix32(h ^ 0x9E3779B9u), h2 = Mix32(h1 + 0x85EBCA6Bu),
                          h3 = Mix32(h2 + 0xC2B2AE35u);
      const std::int64_t sx = std::int64_t{i} * kLakeCell + kLakeCell / 4 +
                              static_cast<std::int64_t>(h1 % (kLakeCell / 2)),
                         sz = std::int64_t{j} * kLakeCell + kLakeCell / 4 +
                              static_cast<std::int64_t>(h2 % (kLakeCell / 2));
      const float radius = kLakeMinRadius + Unit(h3) * kLakeRadiusRange;
      const auto dx = static_cast<float>(x - sx), dz = static_cast<float>(z - sz);
      const float q_geo = (dx * dx + dz * dz) / (radius * radius);
      if (q_geo <= kBermTo + 0.5f) {  // within the berm, even with the shore noise
        float q = q_geo + kShoreNoise * Perlin2(s.shore, Lattice(x, kShoreWavelength),
                                                 Lattice(z, kShoreWavelength));
        if (q < 0.0f) q = 0.0f;
        const float level = CachedLakeLevel(s, i, j, sx, sz, oracle);
        if (level > kNoLevel * 0.5f) {
          c.lake_q = q;
          c.lake_level = level;
          c.lake_depth = kLakeMinDepth + Unit(Mix32(h3 ^ 0x68E31DA4u)) * kLakeDepthRange;
        }
      }
    }
  }
  return c;
}

}  // namespace dwell::worldgen::rivers
