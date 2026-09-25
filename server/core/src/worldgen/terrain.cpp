#include "dwell/worldgen/terrain.h"

#include <algorithm>
#include <cstdlib>
#include <vector>

#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {

using core::Chunk;
using core::ChunkCoord;
using core::kBedrockLayers;
using core::kChunkSize;
using core::kSeaLevel;
using core::kWorldMaxY;
using core::kWorldMinY;
using core::MaterialId;
namespace M = core::Materials;

namespace {

// Lattice spacing of the interpolated 2D fields and 3D noise (voxels).
constexpr int kLattice = 4;
constexpr float kLatticeStep = 0.25f;  // 1 / kLattice, exact
// Voxels above a chunk evaluated for surface depth (grass/dirt runs need the air above).
constexpr int kSurfacePad = 8;
// Solid components smaller than this, not touching a chunk face, are removed (stability pass).
constexpr int kMinComponent = 48;
constexpr int kSnowLine = 170;
// Trees reach at most this far above their ground, and leaves this far sideways from the trunk.
constexpr int kTreeReach = 12;
constexpr int kTreeSpread = 3;
constexpr int kBoulderMaxRadius = 2;

// Piecewise-linear spline.
struct Knot {
  float x, y;
};
template <std::size_t N>
float Spline(const Knot (&k)[N], float x) {
  if (x <= k[0].x) return k[0].y;
  for (std::size_t i = 1; i < N; ++i) {
    if (x <= k[i].x) return Lerp(k[i - 1].y, k[i].y, (x - k[i - 1].x) / (k[i].x - k[i - 1].x));
  }
  return k[N - 1].y;
}

// Base height (m) from continentalness: deep ocean, shelf, coast, lowlands, uplands.
constexpr Knot kContinentHeight[] = {{-1.0f, 22.0f},  {-0.45f, 38.0f}, {-0.2f, 54.0f},
                                     {-0.08f, 61.0f}, {0.0f, 66.0f},   {0.25f, 72.0f},
                                     {0.6f, 86.0f},   {1.0f, 104.0f}};

float Clamp(float v, float lo, float hi) { return v < lo ? lo : v > hi ? hi : v; }

// Classification of one voxel before materials are assigned.
enum Cell : std::uint8_t { kOpenAir, kWater, kCaveAir, kSolid };

std::uint32_t SeedWord(std::uint64_t world_seed, std::uint32_t stream) {
  const auto lo = static_cast<std::uint32_t>(world_seed);
  const auto hi = static_cast<std::uint32_t>(world_seed >> 32);
  return Mix32(Mix32(lo ^ Mix32(stream * 0x9E3779B9u)) ^ Mix32(hi + stream));
}

// A 3D noise sample and the column it belongs to → voxel class. Shared by the chunk and point
// paths so both give bit-identical answers.
template <class Noise>
Cell Classify(const Column& col, std::int32_t y, Noise&& noise) {
  if (y < kWorldMinY || y >= kWorldMaxY) return kOpenAir;
  if (y < kWorldMinY + kBedrockLayers) return kSolid;
  const float fy = static_cast<float>(y);
  // Above the reach of the overhang noise: open sky (or sea). No noise needed.
  if (fy > col.height + col.overhang) return y < kSeaLevel ? kWater : kOpenAir;
  const auto& n = noise();
  const float density = col.height - fy + n.overhang * col.overhang;
  if (density <= 0.0f) return y < kSeaLevel ? kWater : kOpenAir;
  // Caves fade in from 3 m to 15 m below the surface and fade out just above the bedrock.
  const float fade =
      Clamp01((col.height - fy - 3.0f) * (1.0f / 12.0f)) *
      Clamp01((fy - static_cast<float>(kWorldMinY + kBedrockLayers + 2)) * (1.0f / 12.0f));
  if (fade <= 0.0f) return kSolid;
  const float tunnel = n.spaghetti_a * n.spaghetti_a + n.spaghetti_b * n.spaghetti_b;
  if (tunnel < 0.0045f * fade) return kCaveAir;
  if (n.cheese > 0.62f + (1.0f - fade) * 0.5f) return kCaveAir;
  return kSolid;
}

// Surface material for a solid voxel `run` voxels below open air or water (run 0 = the top voxel).
MaterialId SurfaceMaterial(const Column& col, int run, bool under_water, std::int32_t y,
                           float slope) {
  if (run >= 8) return M::kStone;
  if (under_water) {
    if (run >= 3) return M::kStone;
    return col.height < static_cast<float>(kSeaLevel - 8) ? M::kGravel : M::kSand;
  }
  const bool steep = slope >= 3.0f;
  switch (col.biome) {
    case Biome::kDesert:
      if (steep) return run < 3 ? M::kSandstone : M::kStone;
      return run < 4 ? M::kSand : run < 7 ? M::kSandstone : M::kStone;
    case Biome::kOcean:
    case Biome::kBeach:
      return run < 4 ? M::kSand : run < 6 ? M::kSandstone : M::kStone;
    case Biome::kSnowy:
      if (steep) return M::kStone;
      return run == 0 ? M::kSnow : run < 4 ? M::kDirt : M::kStone;
    case Biome::kMountains:
      if (y >= kSnowLine && run == 0 && slope < 4.0f) return M::kSnow;
      if (steep || y >= kSnowLine) return M::kStone;
      return run == 0 ? M::kGrass : run < 3 ? M::kDirt : M::kStone;
    case Biome::kPlains:
    case Biome::kForest:
    default:
      if (steep) return M::kStone;
      return run == 0 ? M::kGrass : run < 4 ? M::kDirt : M::kStone;
  }
}

struct OreSpec {
  MaterialId material;
  std::int32_t min_y, max_y;
  int veins_per_cell;  // per 16³ cell
  int radius;          // vein blob radius (voxels)
  float chance;        // chance each vein exists
};
constexpr int kOreCell = 16;
constexpr OreSpec kOres[] = {
    {M::kCoalOre, -100, 200, 2, 2, 0.6f},
    {M::kIronOre, -110, 72, 3, 1, 0.8f},
    {M::kGoldOre, -120, 16, 1, 1, 0.5f},
};

}  // namespace

const char* BiomeName(Biome b) {
  switch (b) {
    case Biome::kOcean:
      return "ocean";
    case Biome::kBeach:
      return "beach";
    case Biome::kPlains:
      return "plains";
    case Biome::kForest:
      return "forest";
    case Biome::kDesert:
      return "desert";
    case Biome::kSnowy:
      return "snowy";
    case Biome::kMountains:
      return "mountains";
  }
  return "?";
}

TerrainGenerator::TerrainGenerator(std::uint64_t world_seed) {
  std::uint32_t stream = 0;
  for (std::uint32_t* s :
       {&seeds_.continent, &seeds_.erosion, &seeds_.temperature, &seeds_.humidity, &seeds_.hills,
        &seeds_.ridges, &seeds_.overhang, &seeds_.spaghetti_a, &seeds_.spaghetti_b, &seeds_.cheese,
        &seeds_.trees, &seeds_.boulders, &seeds_.ores}) {
    *s = SeedWord(world_seed, ++stream);
  }
}

// --- 1–2: climate and base height -----------------------------------------------------------

TerrainGenerator::Corner2 TerrainGenerator::SampleCorner2(std::int32_t lx, std::int32_t lz) const {
  const float x = static_cast<float>(lx * kLattice), z = static_cast<float>(lz * kLattice);
  Corner2 c;
  c.continentalness = Fbm2(seeds_.continent, x * (1.0f / 1400.0f), z * (1.0f / 1400.0f), 5);
  c.erosion = Fbm2(seeds_.erosion, x * (1.0f / 700.0f), z * (1.0f / 700.0f), 3);
  c.temperature = Fbm2(seeds_.temperature, x * (1.0f / 1100.0f), z * (1.0f / 1100.0f), 3);
  c.humidity = Fbm2(seeds_.humidity, x * (1.0f / 900.0f), z * (1.0f / 900.0f), 3);
  c.hills = Fbm2(seeds_.hills, x * (1.0f / 96.0f), z * (1.0f / 96.0f), 4);
  c.ridges = Ridged2(seeds_.ridges, x * (1.0f / 360.0f), z * (1.0f / 360.0f), 5);
  return c;
}

Column TerrainGenerator::Finish(const Corner2& c) {
  Column col;
  // Fractal Perlin sums rarely leave ±0.5; stretch the climate fields to about ±1.
  col.continentalness = Clamp(c.continentalness * 2.2f + 0.15f, -1.0f, 1.0f);  // ~⅓ ocean
  col.erosion = Clamp(c.erosion * 2.2f, -1.0f, 1.0f);
  col.temperature = Clamp(c.temperature * 2.2f, -1.0f, 1.0f);
  col.humidity = Clamp(c.humidity * 2.2f, -1.0f, 1.0f);
  const float cont = col.continentalness;

  // Biome weights, blended smoothly across borders.
  const float desert =
      SmoothStep(0.1f, 0.3f, col.temperature) * SmoothStep(0.1f, -0.1f, col.humidity);
  const float snowy = SmoothStep(-0.3f, -0.5f, col.temperature);
  const float rest = (1.0f - desert) * (1.0f - snowy);
  const float forest = rest * SmoothStep(-0.05f, 0.2f, col.humidity);
  const float plains = rest - forest;
  const float hill_amplitude = 6.0f * plains + 11.0f * forest + 4.0f * desert + 12.0f * snowy;

  const float land = SmoothStep(-0.12f, 0.05f, cont);
  col.mountain = SmoothStep(0.05f, 0.4f, cont) * SmoothStep(-0.05f, -0.4f, col.erosion);
  col.height = Spline(kContinentHeight, cont) + c.hills * hill_amplitude * (0.35f + 0.65f * land) +
               col.mountain * (18.0f + c.ridges * 150.0f);
  col.overhang = 2.5f * land + 1.0f + 14.0f * col.mountain;

  const float h = col.height;
  if (h < static_cast<float>(kSeaLevel) - 1.0f) {
    col.biome = Biome::kOcean;
  } else if (col.mountain > 0.45f) {
    col.biome = Biome::kMountains;
  } else if (snowy > 0.5f) {
    col.biome = Biome::kSnowy;
  } else if (h < static_cast<float>(kSeaLevel) + 2.0f && cont < 0.02f) {
    col.biome = Biome::kBeach;
  } else if (desert > 0.5f) {
    col.biome = Biome::kDesert;
  } else if (forest > plains) {
    col.biome = Biome::kForest;
  } else {
    col.biome = Biome::kPlains;
  }
  return col;
}

Column TerrainGenerator::Interp2(const Corner2 (&c)[4], int fx, int fz) {
  const float tx = static_cast<float>(fx) * kLatticeStep,
              tz = static_cast<float>(fz) * kLatticeStep;
  const auto bi = [&](float Corner2::*f) {
    return Lerp(Lerp(c[0].*f, c[1].*f, tx), Lerp(c[2].*f, c[3].*f, tx), tz);
  };
  Corner2 m;
  m.continentalness = bi(&Corner2::continentalness);
  m.erosion = bi(&Corner2::erosion);
  m.temperature = bi(&Corner2::temperature);
  m.humidity = bi(&Corner2::humidity);
  m.hills = bi(&Corner2::hills);
  m.ridges = bi(&Corner2::ridges);
  return Finish(m);
}

Column TerrainGenerator::ColumnAt(std::int32_t x, std::int32_t z) const {
  const std::int32_t lx = FloorDiv(x, kLattice), lz = FloorDiv(z, kLattice);
  const Corner2 c[4] = {SampleCorner2(lx, lz), SampleCorner2(lx + 1, lz), SampleCorner2(lx, lz + 1),
                        SampleCorner2(lx + 1, lz + 1)};
  return Interp2(c, FloorMod(x, kLattice), FloorMod(z, kLattice));
}

// --- 3–4: density and caves -----------------------------------------------------------------

TerrainGenerator::Corner3 TerrainGenerator::SampleCorner3(std::int32_t lx, std::int32_t ly,
                                                          std::int32_t lz) const {
  const float x = static_cast<float>(lx * kLattice), y = static_cast<float>(ly * kLattice),
              z = static_cast<float>(lz * kLattice);
  Corner3 c;
  c.overhang = Fbm3(seeds_.overhang, x * (1.0f / 28.0f), y * (1.0f / 20.0f), z * (1.0f / 28.0f), 2);
  c.spaghetti_a =
      Perlin3(seeds_.spaghetti_a, x * (1.0f / 56.0f), y * (1.0f / 36.0f), z * (1.0f / 56.0f));
  c.spaghetti_b =
      Perlin3(seeds_.spaghetti_b, x * (1.0f / 56.0f), y * (1.0f / 36.0f), z * (1.0f / 56.0f));
  c.cheese = Fbm3(seeds_.cheese, x * (1.0f / 90.0f), y * (1.0f / 48.0f), z * (1.0f / 90.0f), 2);
  return c;
}

TerrainGenerator::Corner3 TerrainGenerator::Bilerp(const Corner3 (&c)[4], int fx, int fz) {
  // Corner order: index = i + 2k for offsets (i, k) along (x, z).
  const float tx = static_cast<float>(fx) * kLatticeStep,
              tz = static_cast<float>(fz) * kLatticeStep;
  const auto bi = [&](float Corner3::*f) {
    return Lerp(Lerp(c[0].*f, c[1].*f, tx), Lerp(c[2].*f, c[3].*f, tx), tz);
  };
  return {bi(&Corner3::overhang), bi(&Corner3::spaghetti_a), bi(&Corner3::spaghetti_b),
          bi(&Corner3::cheese)};
}

TerrainGenerator::Corner3 TerrainGenerator::LerpY(const Corner3& a, const Corner3& b, int fy) {
  const float t = static_cast<float>(fy) * kLatticeStep;
  return {Lerp(a.overhang, b.overhang, t), Lerp(a.spaghetti_a, b.spaghetti_a, t),
          Lerp(a.spaghetti_b, b.spaghetti_b, t), Lerp(a.cheese, b.cheese, t)};
}

bool TerrainGenerator::SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const {
  const Column col = ColumnAt(x, z);
  return Classify(col, y, [&] {
           const std::int32_t lx = FloorDiv(x, kLattice), ly = FloorDiv(y, kLattice),
                              lz = FloorDiv(z, kLattice);
           const int fx = FloorMod(x, kLattice), fz = FloorMod(z, kLattice);
           Corner3 layer[2];
           for (int j = 0; j < 2; ++j) {
             const Corner3 c[4] = {SampleCorner3(lx, ly + j, lz), SampleCorner3(lx + 1, ly + j, lz),
                                   SampleCorner3(lx, ly + j, lz + 1),
                                   SampleCorner3(lx + 1, ly + j, lz + 1)};
             layer[j] = Bilerp(c, fx, fz);
           }
           return LerpY(layer[0], layer[1], FloorMod(y, kLattice));
         }) == kSolid;
}

std::optional<std::int32_t> TerrainGenerator::GroundY(std::int32_t x, std::int32_t z) const {
  // The ground near the base height: the top of a solid run at least 4 deep with 2 voxels of
  // open space above, searched within ±4 m of the base height. One column, so the 2D fields are
  // computed once and each lattice layer of 3D corners at most once.
  const Column col = ColumnAt(x, z);
  const std::int32_t lx = FloorDiv(x, kLattice), lz = FloorDiv(z, kLattice);
  const int fx = FloorMod(x, kLattice), fz = FloorMod(z, kLattice);
  const std::int32_t base = FloorToInt(col.height);
  // Layers ly_lo..ly_lo+kLayers−1 cover y from base − 7 to base + 6.
  constexpr int kLayers = 6;
  const std::int32_t ly_lo = FloorDiv(base - 7, kLattice);
  Corner3 layers[kLayers];
  bool sampled[kLayers] = {};
  const auto layer = [&](int j) -> const Corner3& {
    if (!sampled[j]) {
      const std::int32_t ly = ly_lo + j;
      const Corner3 c[4] = {SampleCorner3(lx, ly, lz), SampleCorner3(lx + 1, ly, lz),
                            SampleCorner3(lx, ly, lz + 1), SampleCorner3(lx + 1, ly, lz + 1)};
      layers[j] = Bilerp(c, fx, fz);
      sampled[j] = true;
    }
    return layers[j];
  };
  const auto solid = [&](std::int32_t y) {
    return Classify(col, y, [&] {
             const int j = FloorDiv(y, kLattice) - ly_lo;
             return LerpY(layer(j), layer(j + 1), FloorMod(y, kLattice));
           }) == kSolid;
  };
  bool above[2] = {!solid(base + 6), !solid(base + 5)};
  for (std::int32_t y = base + 4; y >= base - 4; --y) {
    const bool here = solid(y);
    if (here && above[0] && above[1] && solid(y - 1) && solid(y - 3)) return y;
    above[0] = above[1];
    above[1] = !here;
  }
  return std::nullopt;
}

// --- 8: features ----------------------------------------------------------------------------

std::optional<Feature> TerrainGenerator::TreeInCell(std::int32_t cx, std::int32_t cz) const {
  const std::uint32_t h = Hash2(seeds_.trees, cx, cz);
  const std::int32_t x = cx * kTreeCell + static_cast<std::int32_t>(h % kTreeCell);
  const std::int32_t z = cz * kTreeCell + static_cast<std::int32_t>((h >> 8) % kTreeCell);
  const Column col = ColumnAt(x, z);
  float chance = 0.0f;
  Feature::Kind kind = Feature::Kind::kOak;
  switch (col.biome) {
    case Biome::kForest:
      chance = 0.7f;
      break;
    case Biome::kPlains:
      chance = 0.05f;
      break;
    case Biome::kSnowy:
      chance = 0.3f;
      kind = Feature::Kind::kSpruce;
      break;
    case Biome::kMountains:
      chance = col.height < 140.0f ? 0.12f : 0.0f;
      kind = Feature::Kind::kSpruce;
      break;
    default:
      break;
  }
  if (Unit(Mix32(h ^ 0x5bd1e995u)) >= chance) return std::nullopt;
  const auto ground = GroundY(x, z);
  if (!ground || *ground < kSeaLevel) return std::nullopt;
  const std::uint32_t r = Mix32(h + 1u);
  const int size =
      kind == Feature::Kind::kOak ? 4 + static_cast<int>(r % 3u) : 6 + static_cast<int>(r % 4u);
  return Feature{kind, x, *ground + 1, z, size, r};
}

std::optional<Feature> TerrainGenerator::BoulderInCell(std::int32_t cx, std::int32_t cz) const {
  const std::uint32_t h = Hash2(seeds_.boulders, cx, cz);
  const std::int32_t x = cx * kBoulderCell + static_cast<std::int32_t>(h % kBoulderCell);
  const std::int32_t z = cz * kBoulderCell + static_cast<std::int32_t>((h >> 8) % kBoulderCell);
  const Column col = ColumnAt(x, z);
  float chance = 0.0f;
  switch (col.biome) {
    case Biome::kMountains:
      chance = 0.6f;
      break;
    case Biome::kPlains:
    case Biome::kSnowy:
      chance = 0.3f;
      break;
    case Biome::kForest:
      chance = 0.2f;
      break;
    case Biome::kDesert:
      chance = 0.1f;
      break;
    default:
      break;
  }
  if (Unit(Mix32(h ^ 0x68e31da4u)) >= chance) return std::nullopt;
  const auto ground = GroundY(x, z);
  if (!ground || *ground < kSeaLevel) return std::nullopt;
  const std::uint32_t r = Mix32(h + 7u);
  return Feature{Feature::Kind::kBoulder, x, *ground, z, 1 + static_cast<int>(r % 2u), r};
}

template <class Write>
void TerrainGenerator::PlaceFeature(const Feature& f, Write&& write) const {
  if (f.kind == Feature::Kind::kBoulder) {
    const int r = f.size;
    for (int dy = -r; dy <= r; ++dy)
      for (int dz = -r; dz <= r; ++dz)
        for (int dx = -r; dx <= r; ++dx) {
          const int d2 = dx * dx + dy * dy + dz * dz;
          if (d2 > r * r + 1) continue;
          if (d2 > r * r - 1 && (Hash3(f.hash, dx, dy, dz) & 1u)) continue;  // rough edges
          write(f.x + dx, f.y + dy, f.z + dz, M::kStone);
        }
    return;
  }
  const std::int32_t top = f.y + f.size - 1;
  for (std::int32_t y = f.y; y <= top; ++y) write(f.x, y, f.z, M::kLog);
  if (f.kind == Feature::Kind::kOak) {
    for (std::int32_t y = top - 2; y <= top + 1; ++y) {
      const int r = y <= top - 1 ? 2 : 1;
      for (int dz = -r; dz <= r; ++dz)
        for (int dx = -r; dx <= r; ++dx) {
          const bool corner = (dx == r || dx == -r) && (dz == r || dz == -r);
          if (corner && (y == top + 1 || (Hash3(f.hash, dx, y - top, dz) & 1u))) continue;
          write(f.x + dx, y, f.z + dz, M::kLeaves);
        }
    }
  } else {
    // Spruce: a cone of alternating wide and narrow layers from the third log up.
    for (std::int32_t y = f.y + 2; y <= top + 1; ++y) {
      const int from_top = top + 1 - y;
      const int r =
          from_top == 0 ? 0 : std::min(kTreeSpread - 1, 1 + from_top / 3) - (from_top & 1);
      for (int dz = -r; dz <= r; ++dz)
        for (int dx = -r; dx <= r; ++dx) {
          if (r > 1 && (dx == r || dx == -r) && (dz == r || dz == -r)) continue;
          write(f.x + dx, y, f.z + dz, M::kLeaves);
        }
    }
  }
}

// --- chunk generation -----------------------------------------------------------------------

void TerrainGenerator::Generate(const ChunkCoord& coord, Chunk& chunk, std::uint8_t stages) const {
  constexpr int S = kChunkSize;
  const std::int32_t x0 = coord.x * S, y0 = coord.y * S, z0 = coord.z * S;
  auto& voxels = chunk.generation_voxels();  // all air
  if (y0 + S <= kWorldMinY || y0 >= kWorldMaxY) return;

  // 1–2. Columns −1..S (one beyond each side, for slopes), from a 2D lattice of corners.
  constexpr int kCols = S + 2;
  const std::int32_t lx0 = FloorDiv(x0 - 1, kLattice), lz0 = FloorDiv(z0 - 1, kLattice);
  const int nl = FloorDiv(x0 + S, kLattice) - lx0 + 2;
  std::vector<Corner2> corners2(static_cast<std::size_t>(nl * nl));
  for (int j = 0; j < nl; ++j)
    for (int i = 0; i < nl; ++i) corners2[j * nl + i] = SampleCorner2(lx0 + i, lz0 + j);
  std::vector<Column> cols(kCols * kCols);
  float top = -1e9f;
  for (int z = -1; z <= S; ++z)
    for (int x = -1; x <= S; ++x) {
      const std::int32_t wx = x0 + x, wz = z0 + z;
      const int i = FloorDiv(wx, kLattice) - lx0, j = FloorDiv(wz, kLattice) - lz0;
      const Corner2 c[4] = {corners2[j * nl + i], corners2[j * nl + i + 1],
                            corners2[(j + 1) * nl + i], corners2[(j + 1) * nl + i + 1]};
      Column& col = cols[(z + 1) * kCols + x + 1];
      col = Interp2(c, FloorMod(wx, kLattice), FloorMod(wz, kLattice));
      top = std::max(top, col.height + col.overhang);
    }
  const auto column = [&](int x, int z) -> const Column& { return cols[(z + 1) * kCols + x + 1]; };

  // Sky above all terrain and sea, with room for trees from neighbouring columns: all air.
  if (static_cast<float>(y0) > top + static_cast<float>(kTreeReach + 12) && y0 >= kSeaLevel) return;

  // 3–4. Classify voxels (plus kSurfacePad above the chunk) from a 3D lattice of noise corners.
  constexpr int kH = S + kSurfacePad;
  const int nxz = S / kLattice + 1, ny = kH / kLattice + 1;
  const std::int32_t lx = x0 / kLattice, ly = FloorDiv(y0, kLattice), lz = z0 / kLattice;
  // Corners above everything the density can reach are never read; skip sampling them.
  const int ny_used = std::min(ny, std::max(0, FloorDiv(FloorToInt(top), kLattice) - ly + 2));
  std::vector<Corner3> corners3(static_cast<std::size_t>(nxz * ny * nxz));
  for (int k = 0; k < nxz; ++k)
    for (int j = 0; j < ny_used; ++j)
      for (int i = 0; i < nxz; ++i)
        corners3[(k * ny + j) * nxz + i] = SampleCorner3(lx + i, ly + j, lz + k);
  std::vector<std::uint8_t> cells(static_cast<std::size_t>(S * S * kH));
  const auto cell = [&](int x, int y, int z) -> std::uint8_t& {
    return cells[(static_cast<std::size_t>(z) * S + x) * kH + y];
  };
  std::vector<Corner3> layers(static_cast<std::size_t>(ny));
  for (int z = 0; z < S; ++z)
    for (int x = 0; x < S; ++x) {
      const Column& col = column(x, z);
      const int i = x / kLattice, k = z / kLattice, fx = x % kLattice, fz = z % kLattice;
      // The column's noise at each lattice layer (bilinear in x, z), then linear in y per voxel:
      // the same composition as the point path (SolidAt).
      for (int j = 0; j < ny_used; ++j) {
        const auto at = [&](int di, int dk) {
          return corners3[((k + dk) * ny + j) * nxz + i + di];
        };
        const Corner3 c[4] = {at(0, 0), at(1, 0), at(0, 1), at(1, 1)};
        layers[j] = Bilerp(c, fx, fz);
      }
      for (int y = 0; y < kH; ++y) {
        cell(x, y, z) = Classify(col, y0 + y, [&] {
          const int j = y / kLattice;
          return LerpY(layers[j], layers[j + 1], y % kLattice);
        });
      }
    }

  // 5. Surface and strata, top-down per column; bedrock at the bottom.
  for (int z = 0; z < S; ++z)
    for (int x = 0; x < S; ++x) {
      const Column& col = column(x, z);
      const float h = col.height;
      float slope = 0.0f;
      for (const auto& [dx, dz] :
           {std::pair{1, 0}, std::pair{-1, 0}, std::pair{0, 1}, std::pair{0, -1}}) {
        const float d = column(x + dx, z + dz).height - h;
        slope = std::max(slope, d < 0.0f ? -d : d);
      }
      int run = 1000;  // solid below the padded top: treat as deep
      bool under_water = false;
      for (int y = kH - 1; y >= 0; --y) {
        const std::uint8_t c = cell(x, y, z);
        if (c != kSolid) {
          // Only open sky or sea starts a surface; cave air leaves the rock below bare.
          run = c == kCaveAir ? 1000 : 0;
          under_water = c == kWater;
          if (y < S) {
            voxels[core::LocalIndex(x, y, z)] = c == kWater ? M::kWater : M::kAir;
          }
          continue;
        }
        if (y < S) {
          const std::int32_t wy = y0 + y;
          voxels[core::LocalIndex(x, y, z)] =
              wy < kWorldMinY + kBedrockLayers ? M::kBedrock
                                               : SurfaceMaterial(col, run, under_water, wy, slope);
        }
        if (run < 1000) ++run;
      }
    }

  // 6. Stability: remove small solid components that float inside the chunk. Components that
  // touch a chunk face may continue into a neighbour and are kept.
  if (stages & kStageStability) {
    // open[i]: solid and not yet reached. The stack never holds a voxel twice.
    std::vector<std::uint8_t> open(core::kChunkVolume);
    for (int i = 0; i < core::kChunkVolume; ++i) {
      open[i] = voxels[i] != M::kAir && voxels[i] != M::kWater;
    }
    std::vector<int> stack(core::kChunkVolume);
    std::vector<int> component;
    for (int start = 0; start < core::kChunkVolume; ++start) {
      if (!open[start]) continue;
      component.clear();
      int top_of_stack = 0;
      stack[top_of_stack++] = start;
      open[start] = 0;
      // Once a component is anchored or large, only the flood (marking it reached) continues.
      bool keep = false;
      while (top_of_stack > 0) {
        const int idx = stack[--top_of_stack];
        const int x = idx & 31, y = (idx >> 5) & 31, z = idx >> 10;
        if (!keep) {
          component.push_back(idx);
          keep = x == 0 || y == 0 || z == 0 || x == S - 1 || y == S - 1 || z == S - 1 ||
                 static_cast<int>(component.size()) >= kMinComponent;
        }
        const int neighbours[6] = {x > 0 ? idx - 1 : -1,     x < S - 1 ? idx + 1 : -1,
                                   y > 0 ? idx - S : -1,     y < S - 1 ? idx + S : -1,
                                   z > 0 ? idx - S * S : -1, z < S - 1 ? idx + S * S : -1};
        for (const int ni : neighbours) {
          if (ni >= 0 && open[ni]) {
            open[ni] = 0;
            stack[top_of_stack++] = ni;
          }
        }
      }
      if (keep) continue;
      // Floating debris in open sea becomes water; anywhere else (sky, caves) air.
      for (const int idx : component) {
        const int x = idx & 31, y = (idx >> 5) & 31, z = idx >> 10;
        const bool sea = y0 + y < kSeaLevel && column(x, z).height < static_cast<float>(kSeaLevel);
        voxels[idx] = sea ? M::kWater : M::kAir;
      }
    }
  }

  // 7. Ores: hashed blobs per 16³ cell, replacing stone only.
  for (std::size_t o = 0; (stages & kStageOres) && o < std::size(kOres); ++o) {
    const OreSpec& ore = kOres[o];
    if (y0 > ore.max_y + ore.radius || y0 + S < ore.min_y - ore.radius) continue;
    const std::uint32_t seed = Mix32(seeds_.ores + static_cast<std::uint32_t>(o));
    const std::int32_t c0x = FloorDiv(x0 - ore.radius, kOreCell),
                       c1x = FloorDiv(x0 + S + ore.radius, kOreCell);
    const std::int32_t c0y = FloorDiv(y0 - ore.radius, kOreCell),
                       c1y = FloorDiv(y0 + S + ore.radius, kOreCell);
    const std::int32_t c0z = FloorDiv(z0 - ore.radius, kOreCell),
                       c1z = FloorDiv(z0 + S + ore.radius, kOreCell);
    for (std::int32_t cz = c0z; cz <= c1z; ++cz)
      for (std::int32_t cy = c0y; cy <= c1y; ++cy)
        for (std::int32_t cx = c0x; cx <= c1x; ++cx)
          for (int v = 0; v < ore.veins_per_cell; ++v) {
            const std::uint32_t h = Hash3(seed + static_cast<std::uint32_t>(v), cx, cy, cz);
            if (Unit(h) >= ore.chance) continue;
            const std::uint32_t p = Mix32(h);
            const std::int32_t vx = cx * kOreCell + static_cast<std::int32_t>(p % kOreCell);
            const std::int32_t vy = cy * kOreCell + static_cast<std::int32_t>((p >> 8) % kOreCell);
            const std::int32_t vz = cz * kOreCell + static_cast<std::int32_t>((p >> 16) % kOreCell);
            if (vy < ore.min_y || vy > ore.max_y) continue;
            const int r = ore.radius;
            for (int dz = -r; dz <= r; ++dz)
              for (int dy = -r; dy <= r; ++dy)
                for (int dx = -r; dx <= r; ++dx) {
                  const std::int32_t x = vx + dx - x0, y = vy + dy - y0, z = vz + dz - z0;
                  if (x < 0 || y < 0 || z < 0 || x >= S || y >= S || z >= S) continue;
                  const int d2 = dx * dx + dy * dy + dz * dz;
                  if (d2 > r * r + 1) continue;
                  if (d2 > 0 && (Hash3(p, dx, dy, dz) & 3u) == 0) continue;  // irregular blobs
                  MaterialId& m = voxels[core::LocalIndex(x, y, z)];
                  if (m == M::kStone) m = ore.material;
                }
          }
  }

  // 8. Features: boulders, then trees, each in increasing cell order, so overlapping features
  // resolve the same way in every chunk. Logs replace air and leaves; leaves and boulders only air.
  if (!(stages & kStageFeatures)) return;
  const std::int32_t ymin = y0 - kTreeReach, ymax = y0 + S + kBoulderMaxRadius;
  // Skip chunks far from any column's surface.
  float lo = 1e9f;
  for (const Column& col : cols) lo = std::min(lo, col.height - col.overhang);
  if (static_cast<float>(ymax) < lo - 8.0f || static_cast<float>(ymin) > top + 8.0f) return;
  const auto write = [&](std::int32_t wx, std::int32_t wy, std::int32_t wz, MaterialId m) {
    const std::int32_t x = wx - x0, y = wy - y0, z = wz - z0;
    if (x < 0 || y < 0 || z < 0 || x >= S || y >= S || z >= S) return;
    MaterialId& cur = voxels[core::LocalIndex(x, y, z)];
    if (cur == M::kAir || (m == M::kLog && cur == M::kLeaves)) cur = m;
  };
  const auto in_range = [&](const Feature& f, int reach_down, int reach_up) {
    return f.y + reach_up >= y0 && f.y - reach_down < y0 + S;
  };
  for (std::int32_t cz = FloorDiv(z0 - kBoulderMaxRadius, kBoulderCell);
       cz <= FloorDiv(z0 + S + kBoulderMaxRadius, kBoulderCell); ++cz)
    for (std::int32_t cx = FloorDiv(x0 - kBoulderMaxRadius, kBoulderCell);
         cx <= FloorDiv(x0 + S + kBoulderMaxRadius, kBoulderCell); ++cx)
      if (const auto f = BoulderInCell(cx, cz);
          f && in_range(*f, kBoulderMaxRadius, kBoulderMaxRadius)) {
        PlaceFeature(*f, write);
      }
  for (std::int32_t cz = FloorDiv(z0 - kTreeSpread, kTreeCell);
       cz <= FloorDiv(z0 + S + kTreeSpread, kTreeCell); ++cz)
    for (std::int32_t cx = FloorDiv(x0 - kTreeSpread, kTreeCell);
         cx <= FloorDiv(x0 + S + kTreeSpread, kTreeCell); ++cx)
      if (const auto f = TreeInCell(cx, cz); f && in_range(*f, 0, kTreeReach))
        PlaceFeature(*f, write);
}

// --- spawn ----------------------------------------------------------------------------------

std::array<float, 3> TerrainGenerator::SpawnPoint() const {
  std::optional<std::array<float, 3>> fallback;
  // Square spiral outwards from the origin in 8 m steps.
  std::int32_t x = 0, z = 0, dx = 1, dz = 0, leg = 1, walked = 0, turns = 0;
  for (int n = 0; n < 40000; ++n) {
    const Column col = ColumnAt(x, z);
    const bool land = col.height >= static_cast<float>(kSeaLevel + 2) && col.mountain < 0.05f &&
                      col.biome != Biome::kBeach && col.biome != Biome::kOcean;
    if (land) {
      if (const auto g = GroundY(x, z)) {
        const std::array<float, 3> here{static_cast<float>(x) + 0.5f, static_cast<float>(*g + 1),
                                        static_cast<float>(z) + 0.5f};
        if (!fallback) fallback = here;
        bool level = true;
        for (int oz = -3; oz <= 3 && level; ++oz)
          for (int ox = -3; ox <= 3 && level; ++ox) {
            if (ox == 0 && oz == 0) continue;
            const auto o = GroundY(x + ox, z + oz);
            level = o && *o == *g;
          }
        bool clear = level;
        for (std::int32_t cz = FloorDiv(z - 6, kTreeCell);
             clear && cz <= FloorDiv(z + 6, kTreeCell); ++cz)
          for (std::int32_t cx = FloorDiv(x - 6, kTreeCell);
               clear && cx <= FloorDiv(x + 6, kTreeCell); ++cx)
            if (const auto t = TreeInCell(cx, cz)) {
              clear = std::max(std::abs(t->x - x), std::abs(t->z - z)) > 5;
            }
        for (std::int32_t cz = FloorDiv(z - 6, kBoulderCell);
             clear && cz <= FloorDiv(z + 6, kBoulderCell); ++cz)
          for (std::int32_t cx = FloorDiv(x - 6, kBoulderCell);
               clear && cx <= FloorDiv(x + 6, kBoulderCell); ++cx)
            if (const auto b = BoulderInCell(cx, cz)) {
              clear = std::max(std::abs(b->x - x), std::abs(b->z - z)) > 6;
            }
        if (clear) return here;
      }
    }
    x += dx * 8;
    z += dz * 8;
    if (++walked == leg) {
      walked = 0;
      const std::int32_t t = dx;
      dx = -dz;
      dz = t;
      if (++turns % 2 == 0) ++leg;
    }
  }
  return fallback.value_or(std::array<float, 3>{0.5f, static_cast<float>(kSeaLevel) + 1.0f, 0.5f});
}

}  // namespace dwell::worldgen
