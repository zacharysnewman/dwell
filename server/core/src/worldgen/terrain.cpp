#include "dwell/worldgen/terrain.h"

#include <algorithm>
#include <cassert>
#include <cstdlib>
#include <limits>
#include <unordered_map>
#include <utility>
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
// Altitude bands follow the ground's temperature (which the lapse rate lowers with height): snow
// lies where it falls below kSnowTemperature, and above the tree line, where it falls below
// kTreeLineTemperature, the ground is bare rock. At a sea-level temperature of 0 these are 1,080 m
// and 620 m; in a colder region they are lower, in a hotter one higher.
constexpr float kSnowTemperature = -0.35f;
constexpr float kTreeLineTemperature = -0.2f;
// Climate: the wavelength (m) of the continental temperature field (humidity's is half), and the
// lapse rate — the temperature field's units drop this much per metre of height (one unit is about
// 20 °C, so 6.5 °C per km).
constexpr std::int32_t kClimateWavelength = 1'200'000;
constexpr float kLapsePerMetre = 0.000325f;
// A land column is the mountains biome where its ground stands this far above its valley floor.
constexpr float kMountainRelief = 200.0f;
// Level of detail: erosion (raw) and ridges when the cell is wider than all their octaves — the
// value whose mountain weight (0.28) and ridged height (0.61) are the fields' means (§6.6).
constexpr float kLodErosion = -0.078f;
constexpr float kLodRidges = 0.61f;
// Placeholder planet-scale layer (prototype, §6.1): continents and oceans a few hundred km across,
// and ranges of kilometre-scale relief on the larger landmasses.
constexpr std::int32_t kMacroWavelength = 262144;  // m
constexpr std::int32_t kReliefWavelength = 49152;  // m
constexpr float kMacroReliefHeight = 1800.0f;      // m, at the crest of a range
// Massifs: in the cores of the largest ranges crests rise this much higher (peaks ~5.5 km).
constexpr float kMassifHeight = 3600.0f;  // m
// Coast distance (continents.h) → terrain. Metres of coast per unit of the local fine octaves
// (the 1.4 km continentalness noise), the distance inland at which the interior's rise reaches half
// its height, and the distance offshore at which continentalness reaches −1.
constexpr float kCoastLocal = 900.0f;
constexpr float kInlandRise = 40'000.0f;
constexpr float kSeaScale = 200'000.0f;
// The shelf's end and the foot of the continental slope, in shelf widths offshore; the abyss's
// depth.
constexpr float kShelfEnd = 1.0f;
constexpr float kSlopeFoot = 1.25f;
constexpr float kShelfDepth = -150.0f;   // m, at the edge of the shelf
constexpr float kAbyssDepth = -1500.0f;  // m, ±300 m by the planet-scale field
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

// Base height (m, sea level 0) of land from continentalness: the coast, lowlands, uplands.
constexpr Knot kLandHeight[] = {{0.0f, 2.0f}, {0.25f, 8.0f}, {0.6f, 22.0f}, {1.0f, 40.0f}};
// The seabed from the coast to the edge of the shelf, by distance offshore in shelf widths.
constexpr Knot kShelfProfile[] = {{0.0f, 2.0f},   {0.005f, -1.0f}, {0.03f, -6.0f},
                                  {0.2f, -20.0f}, {0.6f, -60.0f},  {kShelfEnd, kShelfDepth}};

float Clamp(float v, float lo, float hi) { return v < lo ? lo : v > hi ? hi : v; }

// Amplitude (m) of the 3D overhang noise: a metre or two on most land, more on mountain faces.
float OverhangAmplitude(float land, float mountain) { return 1.0f + 1.5f * land + 9.0f * mountain; }

// Classification of one voxel before materials are assigned.
enum Cell : std::uint8_t { kOpenAir, kWater, kCaveAir, kSolid };

// A 3D noise sample and the column it belongs to → voxel class. Shared by the chunk and point
// paths so both give bit-identical answers.
template <class Noise>
Cell Classify(const Column& col, std::int32_t y, Noise&& noise) {
  if (col.outside || y < kWorldMinY || y >= kWorldMaxY) return kOpenAir;
  if (y < kWorldMinY + kBedrockLayers) return kSolid;
  const float fy = static_cast<float>(y);
  // Above the reach of the overhang noise: open sky (or water). No noise needed.
  if (fy > col.height + col.overhang) return y < col.water ? kWater : kOpenAir;
  const auto& n = noise();
  const float density = col.height - fy + n.overhang * col.overhang;
  if (density <= 0.0f) return y < col.water ? kWater : kOpenAir;
  // Caves fade in from 3 m to 15 m below the surface and fade out just above the bedrock; under a
  // river, a lake or a shallow sea they start kCaveClearance deeper, so none drains or breaches
  // the water (rivers.h).
  const bool under_water = col.wet > 0.0f || (col.height < static_cast<float>(col.water) &&
                                              col.height > -rivers::kShallowSea);
  const float fade =
      Clamp01((col.height - fy - 3.0f - (under_water ? rivers::kCaveClearance : 0.0f)) *
              (1.0f / 12.0f)) *
      Clamp01((fy - static_cast<float>(kWorldMinY + kBedrockLayers + 2)) * (1.0f / 12.0f));
  if (fade <= 0.0f) return kSolid;
  const float tunnel = n.spaghetti_a * n.spaghetti_a + n.spaghetti_b * n.spaghetti_b;
  if (tunnel < 0.0045f * fade) return kCaveAir;
  if (n.cheese > 0.62f + (1.0f - fade) * 0.5f) return kCaveAir;
  return kSolid;
}

// Surface material for a solid voxel `run` voxels below open air or water (run 0 = the top voxel).
MaterialId SurfaceMaterial(const Column& col, int run, bool under_water, std::int32_t /*y*/,
                           float slope) {
  if (run >= 8) return M::kStone;
  if (under_water) {
    if (run >= 3) return M::kStone;
    return col.height < static_cast<float>(col.water - 8) ? M::kGravel : M::kSand;
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
      if (col.temperature < kSnowTemperature && run == 0 && slope < 4.0f) return M::kSnow;
      if (steep || col.temperature < kTreeLineTemperature) return M::kStone;
      return run == 0 ? M::kGrass : run < 3 ? M::kDirt : M::kStone;
    case Biome::kPlains:
    case Biome::kForest:
    default:
      if (steep) return M::kStone;
      return run == 0 ? M::kGrass : run < 4 ? M::kDirt : M::kStone;
  }
}

// How much the base height changes to the four neighbouring columns (m): steepness for materials.
template <class ColumnAt>
float ColumnSlope(ColumnAt&& column_at, int x, int z) {
  const float h = column_at(x, z).height;
  float slope = 0.0f;
  for (const auto& [dx, dz] :
       {std::pair{1, 0}, std::pair{-1, 0}, std::pair{0, 1}, std::pair{0, -1}}) {
    const float d = column_at(x + dx, z + dz).height - h;
    slope = std::max(slope, d < 0.0f ? -d : d);
  }
  return slope;
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
    {M::kCoalOre, -164, 136, 2, 2, 0.6f},
    {M::kIronOre, -174, 8, 3, 1, 0.8f},
    {M::kGoldOre, -184, -48, 1, 1, 0.5f},
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

TerrainGenerator::TerrainGenerator(std::uint64_t world_seed)
    : continents_(world_seed), river_seeds_(rivers::MakeSeeds(world_seed)) {
  std::uint32_t stream = 0;
  for (std::uint32_t* s :
       {&seeds_.continent, &seeds_.erosion, &seeds_.temperature, &seeds_.humidity, &seeds_.hills,
        &seeds_.ridges, &seeds_.overhang, &seeds_.spaghetti_a, &seeds_.spaghetti_b, &seeds_.cheese,
        &seeds_.trees, &seeds_.boulders, &seeds_.ores, &seeds_.macro, &seeds_.relief}) {
    *s = SeedWord(world_seed, ++stream);
  }
}

// --- 1–2: climate and base height -----------------------------------------------------------

// A lake's surface from its centre: the terraced valley floor there, minus the freeboard — the
// terrain's own valley floor at the centre (Finish without rivers), so a lake always lies in a
// valley. No lake near the sea (inland by kLakeInland).
struct TerrainGenerator::LakeOracle final : rivers::LevelOracle {
  explicit LakeOracle(const TerrainGenerator& g) : gen(g) {}
  float LakeLevel(std::int64_t x, std::int64_t z) const override {
    const Column site = gen.Finish(gen.SampleBase(x, z));
    if (site.coast < rivers::kLakeInland) return rivers::kNoLevel;
    return rivers::TerraceSurface(gen.river_seeds_.terrace, site.valley - rivers::kLakeFreeboard);
  }
  const TerrainGenerator& gen;
};

TerrainGenerator::Corner2 TerrainGenerator::SampleCorner2(std::int32_t lx, std::int32_t lz) const {
  // World coordinates stay integers; noise splits them per octave (noise.h, ADR 0011).
  const std::int64_t x = std::int64_t{lx} * kLattice, z = std::int64_t{lz} * kLattice;
  Corner2 c = SampleBase(x, z);
  c.water = rivers::Sample(river_seeds_, x, z, 0, LakeOracle(*this));
  return c;
}

// Climate (generator version 8): temperature and humidity vary over continents, not over a few
// hundred metres, so biomes come in regions of tens to hundreds of kilometres; a small local
// octave pair keeps their borders from being smooth curves. `kept` (level of detail): octaves kept
// of the local pair, −1 for all.
float TerrainGenerator::Temperature(std::int64_t x, std::int64_t z, int kept) const {
  return 0.95f * Fbm2(seeds_.temperature, x, z, kClimateWavelength, 2) +
         0.05f * Fbm2(seeds_.temperature ^ 0x5bd1e995u, x, z, 1100, 2, kept);
}
float TerrainGenerator::Humidity(std::int64_t x, std::int64_t z, int kept) const {
  return 0.95f * Fbm2(seeds_.humidity, x, z, kClimateWavelength / 2, 3) +
         0.05f * Fbm2(seeds_.humidity ^ 0x68e31da4u, x, z, 900, 2, kept);
}

TerrainGenerator::Corner2 TerrainGenerator::SampleBase(std::int64_t x, std::int64_t z) const {
  Corner2 c;
  c.continentalness = Fbm2(seeds_.continent, x, z, 1400, 5);
  c.erosion = Fbm2(seeds_.erosion, x, z, 700, 3);
  c.temperature = Temperature(x, z, -1);
  c.humidity = Humidity(x, z, -1);
  c.hills = Fbm2(seeds_.hills, x, z, 96, 4);
  c.ridges = Ridged2(seeds_.ridges, x, z, 360, 5);
  c.macro = Fbm2(seeds_.macro, x, z, kMacroWavelength, 4);
  c.relief = Ridged2(seeds_.relief, x, z, kReliefWavelength, 4);
  const MacroCorner m = continents_.Sample(x, z);
  c.coast = m.coast;
  c.plate_edge = m.plate_edge;
  c.convergence = m.convergence;
  c.elevation = m.elevation;
  c.shelf = m.shelf;
  c.continent = m.continent;
  return c;
}

Column TerrainGenerator::Finish(const Corner2& c) const {
  Column col;
  // Fractal Perlin sums rarely leave ±0.5; stretch the climate fields to about ±1.
  const float macro = Clamp(c.macro * 2.2f, -1.0f, 1.0f);
  // Continentalness from the signed distance to the coast (continents.h): the continent layout's
  // coarse value plus the 1.4 km octaves. At sea it falls to −1 over kSeaScale; on land it rises
  // toward 1 inland (s / (s + K), saturating), the interior varying a little with the local noise.
  const float coast = c.coast + c.continentalness * kCoastLocal;
  col.coast = coast;
  col.plate_edge = c.plate_edge;
  col.convergence = c.convergence;
  col.continent = c.continent;
  float rise = 0.0f;
  if (coast > 0.0f) {
    rise = coast / (coast + kInlandRise);
    const float local = Clamp01(0.5f + c.continentalness * 1.1f);
    col.continentalness = rise * (0.7f + 0.3f * local);
  } else {
    col.continentalness = Clamp(coast * (1.0f / kSeaScale), -1.0f, 0.0f);
  }
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

  // Land weight: zero at the coast (0.5 km offshore), full 4 km inland: hills and ranges do not
  // reach out to sea.
  const float land = SmoothStep(-500.0f, 4000.0f, coast);
  col.mountain = SmoothStep(0.05f, 0.4f, cont) * SmoothStep(-0.05f, -0.4f, col.erosion);
  // Kilometre-scale ranges on large landmasses; deep basins under large oceans.
  const float range = SmoothStep(0.15f, 0.55f, macro) * land * c.relief * c.relief;
  const float massif = SmoothStep(0.4f, 0.85f, macro);
  const float range_height = range * (kMacroReliefHeight + massif * kMassifHeight);
  // At sea the shelf out to its edge, the continental slope, and the abyss.
  if (coast <= 0.0f) {
    const float u = -coast / c.shelf;  // shelf widths offshore
    const float abyss = kAbyssDepth + 300.0f * macro;
    const float base = u <= kShelfEnd
                           ? Spline(kShelfProfile, u)
                           : Lerp(kShelfDepth, abyss, SmoothStep(kShelfEnd, kSlopeFoot, u));
    col.valley = base;
    col.height = base + c.hills * hill_amplitude * (0.35f + 0.65f * land);
    col.water = kSeaLevel;
    col.mountain = std::max(col.mountain, SmoothStep(0.05f, 0.25f, range));
    col.overhang = OverhangAmplitude(land, col.mountain);
    col.biome = col.height < static_cast<float>(kSeaLevel) - 1.0f ? Biome::kOcean : Biome::kBeach;
    return col;
  }

  // On land (WORLD_GENERATION.md §3.2): the valley floor V — the coast's plain, the lowland rising
  // to the continent's interior, a slow rise inland, uplift belts along convergent plate edges and
  // a share of the ranges' relief — and the relief that stands on it, damped near the rivers
  // (D = 0 in a channel's bed, 1 away from every channel), less the channels' carve.
  const float belt = Clamp(c.convergence, 0.0f, 1.0f) *
                     SmoothStep(rivers::kBeltReach, rivers::kBeltCore, c.plate_edge);
  // V reads the layout's own (macro lattice) coast distance, not the local 1.4 km octaves that
  // roughen continentalness: a valley floor must be smooth for a river to follow it, and the water
  // surfaces cut from it flat (WORLD_GENERATION.md §3.2).
  const float coast_v = c.coast > 0.0f ? c.coast : 0.0f;
  const float rise_v = coast_v / (coast_v + kInlandRise);
  const float inland = coast_v / (coast_v + rivers::kValleyHalf);
  const float plain = SmoothStep(rivers::kPlainStart, rivers::kPlainEnd, coast_v);
  const float lowland = Spline(kLandHeight, rise_v * 0.85f) - rivers::kCoastPlain +
                        c.elevation * rise_v + rivers::kBeltValley * belt +
                        rivers::kMountainValley * range_height;
  const float valley =
      rivers::kCoastPlain + lowland * plain + rivers::kValleyRise * inland * inland;
  col.valley = valley;
  const float hills_up = Clamp01(0.5f + c.hills * 0.9f);  // ≥ 0: the ground never dips below V
  const float relief = hills_up * hill_amplitude * (0.35f + 0.65f * land) +
                       col.mountain * (18.0f + c.ridges * 150.0f) +
                       (1.0f - rivers::kMountainValley) * range_height +
                       belt * rivers::kBeltRelief * c.ridges;

  // Rivers: the distance factor and the channels' carve, tier by tier.
  const float tier_noise[3] = {c.water.rg, c.water.r1, c.water.r2};
  const float tier_channel[3] = {c.water.cg, c.water.c1, c.water.c2};
  const rivers::Tier* const tiers[3] = {&rivers::kGreat, &rivers::kRiver, &rivers::kStream};
  float distance = 1.0f, carve = 0.0f, river_wet = 0.0f;
  // The lake's squared radius scales the rivers out near its shore.
  const bool lake = c.water.lake_level > rivers::kNoLevel * 0.5f;
  const float lake_clear =
      lake ? SmoothStep(rivers::kLakeRiverZero, rivers::kLakeRiverFull, c.water.lake_q) : 1.0f;
  for (int t = 0; t < 3; ++t) {
    const rivers::Tier& tier = *tiers[t];
    const float a = tier_noise[t] < 0.0f ? -tier_noise[t] : tier_noise[t];
    // Away from the channel, banks and valley: no relief damping and no channel. A tier dropped for
    // a level-of-detail cell reads 1 (rivers.h): the mean of its ramp.
    if (a >= 1.0f) {
      distance *= rivers::kDroppedFactor;
      continue;
    }
    if (a < tier.full) distance *= SmoothStep(tier.core, tier.full, a);
    // The channel, unless the level of detail's cell is too wide for it (the valley stays).
    if (a >= tier.bank || tier_channel[t] <= 0.0f) continue;
    const float edge = 1.0f - SmoothStep(tier.core, tier.bank, a);
    const float edge2 = edge * edge;
    static_assert(rivers::kProfileSharpness == 4, "the profile below is (1 − s)⁴");
    const float channel = edge2 * edge2;
    const float spring = SmoothStep(tier.spring_lo, tier.spring_hi, c.water.spring);
    const float w = channel * SmoothStep(tier.fade_hi, tier.fade_lo, relief) * lake_clear * spring;
    carve = std::max(carve, w * tier.depth);
    river_wet = std::max(river_wet, w);
  }
  col.height = valley + distance * relief - carve;
  col.wet = river_wet;
  col.water = kSeaLevel;
  if (river_wet > 0.0f) {
    col.water = std::max(col.water, static_cast<std::int32_t>(rivers::TerraceSurface(
                                        river_seeds_.terrace, valley - rivers::kFreeboard)));
  }
  // Lakes: a bowl below the lake's surface and, beyond the shore, a low berm that keeps the water
  // from spilling over a lower shore. The shore line is the squared radius 1.
  if (lake && c.water.lake_q < rivers::kBermTo) {
    const float q = c.water.lake_q, level = c.water.lake_level;
    if (q < 1.0f) {
      const float bed = level - c.water.lake_depth * (1.0f - SmoothStep(0.0f, 1.0f, q));
      col.height = std::min(col.height, bed);
      col.water = static_cast<std::int32_t>(level);  // inside the shore the lake's own surface
      col.lake = true;
    } else {
      const float berm = level + rivers::kBermHeight *
                                     SmoothStep(rivers::kBermFrom, rivers::kBermPeak, q) *
                                     SmoothStep(rivers::kBermTo, rivers::kBermPeak, q);
      col.height = std::max(col.height, berm);
    }
    col.wet = 1.0f;
  }
  col.mountain = std::max(col.mountain, SmoothStep(0.05f, 0.25f, range));
  col.mountain = std::max(col.mountain, SmoothStep(0.1f, 0.4f, belt * c.ridges));
  col.overhang = OverhangAmplitude(land, col.mountain) * (1.0f - col.wet);

  const float h = col.height;
  // The temperature at the ground: the lapse rate takes it down with height, so snow lies on high
  // ground and, in cold regions, everywhere.
  col.temperature = Clamp(col.temperature - kLapsePerMetre * std::max(h, 0.0f), -1.0f, 1.0f);
  const float snowy_here = SmoothStep(-0.3f, -0.5f, col.temperature);
  // Desert too is a matter of the ground's temperature: hot dry plains, not hot dry mountaintops.
  const float desert_here =
      SmoothStep(0.1f, 0.3f, col.temperature) * SmoothStep(0.1f, -0.1f, col.humidity);
  if (h - valley > kMountainRelief) {
    col.biome = Biome::kMountains;
  } else if (snowy_here > 0.5f) {
    col.biome = Biome::kSnowy;
  } else if (h < static_cast<float>(kSeaLevel) + 2.0f && cont < 0.02f) {
    col.biome = Biome::kBeach;
  } else if (desert_here > 0.5f) {
    col.biome = Biome::kDesert;
  } else if (forest > plains) {
    col.biome = Biome::kForest;
  } else {
    col.biome = Biome::kPlains;
  }
  return col;
}

Column TerrainGenerator::Interp2(const Corner2 (&c)[4], int fx, int fz) const {
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
  m.macro = bi(&Corner2::macro);
  m.relief = bi(&Corner2::relief);
  m.coast = bi(&Corner2::coast);
  m.plate_edge = bi(&Corner2::plate_edge);
  m.convergence = bi(&Corner2::convergence);
  m.elevation = bi(&Corner2::elevation);
  m.shelf = bi(&Corner2::shelf);
  m.continent = c[(fx >= kLattice / 2 ? 1 : 0) + (fz >= kLattice / 2 ? 2 : 0)].continent;
  // Rivers: the noise is smooth, so it interpolates; a lake's surface is one value for the whole
  // lake, taken from whichever corner has one (a corner with none carries kNoLevel).
  const auto wi = [&](float rivers::Corner::*f) {
    return Lerp(Lerp(c[0].water.*f, c[1].water.*f, tx), Lerp(c[2].water.*f, c[3].water.*f, tx), tz);
  };
  m.water.rg = wi(&rivers::Corner::rg);
  m.water.r1 = wi(&rivers::Corner::r1);
  m.water.r2 = wi(&rivers::Corner::r2);
  m.water.cg = wi(&rivers::Corner::cg);
  m.water.c1 = wi(&rivers::Corner::c1);
  m.water.c2 = wi(&rivers::Corner::c2);
  m.water.spring = wi(&rivers::Corner::spring);
  m.water.lake_q = wi(&rivers::Corner::lake_q);
  m.water.lake_depth = wi(&rivers::Corner::lake_depth);
  m.water.lake_level = std::max(std::max(c[0].water.lake_level, c[1].water.lake_level),
                                std::max(c[2].water.lake_level, c[3].water.lake_level));
  return Finish(m);
}

Column TerrainGenerator::ColumnAt(std::int32_t x, std::int32_t z) const {
  const std::int32_t lx = FloorDiv(x, kLattice), lz = FloorDiv(z, kLattice);
  const Corner2 c[4] = {SampleCorner2(lx, lz), SampleCorner2(lx + 1, lz), SampleCorner2(lx, lz + 1),
                        SampleCorner2(lx + 1, lz + 1)};
  Column col = Interp2(c, FloorMod(x, kLattice), FloorMod(z, kLattice));
  col.outside = !core::InsideWorldDisc(x, z);
  return col;
}

rivers::Corner TerrainGenerator::RiversAt(std::int32_t x, std::int32_t z) const {
  return rivers::Sample(river_seeds_, x, z, 0, LakeOracle(*this));
}

TerrainGenerator::LandSample TerrainGenerator::LandAt(std::int32_t x, std::int32_t z) const {
  const MacroCorner m = continents_.At(x, z);
  const float local = Fbm2(seeds_.continent, x, z, 1400, 5);
  return {m.coast + local * kCoastLocal, m.continent, m.plate_edge, m.convergence};
}

// --- 3–4: density and caves -----------------------------------------------------------------

TerrainGenerator::Corner3 TerrainGenerator::SampleCorner3(std::int32_t lx, std::int32_t ly,
                                                          std::int32_t lz) const {
  const std::int64_t x = std::int64_t{lx} * kLattice, y = std::int64_t{ly} * kLattice,
                     z = std::int64_t{lz} * kLattice;
  Corner3 c;
  c.overhang = Fbm3(seeds_.overhang, x, y, z, 28, 20, 28, 2);
  c.spaghetti_a = Perlin3(seeds_.spaghetti_a, Lattice(x, 56), Lattice(y, 36), Lattice(z, 56));
  c.spaghetti_b = Perlin3(seeds_.spaghetti_b, Lattice(x, 56), Lattice(y, 36), Lattice(z, 56));
  c.cheese = Fbm3(seeds_.cheese, x, y, z, 90, 48, 90, 2);
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

// --- 5b: slopes -------------------------------------------------------------------------------

// Highest zero of the column's density, found layer by layer from the sky: density is linear in y
// between lattice layers, so the crossing inside a layer is closed-form. See terrain.h.
template <class Layer>
TerrainGenerator::SurfaceColumn TerrainGenerator::SurfaceOf(const Column& col, Layer&& layer) {
  if (col.outside) return {};
  // Above height + overhang the density is ≤ 0 (Classify's own shortcut); below height − overhang
  // it is > 0 wherever the 3D noise stays within ±1.
  const std::int32_t hi = FloorToInt(col.height + col.overhang) + 1;
  const std::int32_t lo = FloorToInt(col.height - col.overhang) - 2;
  const std::int32_t jt = FloorDiv(hi, kLattice), jb = FloorDiv(lo, kLattice);
  const auto density = [&](std::int32_t j) {
    return col.height - static_cast<float>(j * kLattice) + layer(j).overhang * col.overhang;
  };
  float upper = density(jt + 1);
  if (upper > 0.0f) return {};
  for (std::int32_t j = jt; j >= jb; --j) {
    const float lower = density(j);
    if (lower <= 0.0f) {
      upper = lower;
      continue;
    }
    const float s = static_cast<float>(j * kLattice) + 4.0f * (lower / (lower - upper));
    // The cube terrain's top voxel, and a clean surface: solid there and just below, with the
    // voxel above open (no cave or overhang pocket directly under or over the surface).
    const std::int32_t fl = FloorToInt(s);
    const std::int32_t top = static_cast<float>(fl) == s ? fl - 1 : fl;
    const auto cell = [&](std::int32_t y) {
      return Classify(col, y, [&] {
        const std::int32_t k = FloorDiv(y, kLattice);
        return LerpY(layer(k), layer(k + 1), FloorMod(y, kLattice));
      });
    };
    if (cell(top) != kSolid || cell(top - 1) != kSolid || cell(top + 1) == kSolid) return {};
    return {true, s};
  }
  return {};
}

TerrainGenerator::SurfaceColumn TerrainGenerator::SurfaceAt(std::int32_t x, std::int32_t z) const {
  const Column col = ColumnAt(x, z);
  if (col.outside) return {};
  const std::int32_t lx = FloorDiv(x, kLattice), lz = FloorDiv(z, kLattice);
  const int fx = FloorMod(x, kLattice), fz = FloorMod(z, kLattice);
  // The layers a column visits are few; keep the ones computed.
  std::vector<std::pair<std::int32_t, Corner3>> cache;
  const auto layer = [&](std::int32_t j) -> const Corner3& {
    for (const auto& [k, c] : cache) {
      if (k == j) return c;
    }
    const Corner3 c[4] = {SampleCorner3(lx, j, lz), SampleCorner3(lx + 1, j, lz),
                          SampleCorner3(lx, j, lz + 1), SampleCorner3(lx + 1, j, lz + 1)};
    cache.emplace_back(j, Bilerp(c, fx, fz));
    return cache.back().second;
  };
  return SurfaceOf(col, layer);
}

void TerrainGenerator::ChunkSurfaces(const std::vector<Column>& cols, std::int32_t x0,
                                     std::int32_t y0, std::int32_t z0,
                                     std::vector<SurfaceColumn>& out) const {
  constexpr int S = kChunkSize, kCols = S + 2;
  out.assign(static_cast<std::size_t>(kCols * kCols), SurfaceColumn{});
  // 3D noise at lattice corners, shared by neighbouring columns: (i, k, layer) → Corner3.
  const std::int32_t lx0 = FloorDiv(x0 - 1, kLattice), lz0 = FloorDiv(z0 - 1, kLattice);
  std::unordered_map<std::uint64_t, Corner3> corners;
  const auto corner = [&](std::int32_t i, std::int32_t k, std::int32_t j) -> const Corner3& {
    const std::uint64_t key = (static_cast<std::uint64_t>(i) << 40) ^
                              (static_cast<std::uint64_t>(k) << 20) ^
                              static_cast<std::uint64_t>(static_cast<std::uint32_t>(j) & 0xFFFFF);
    auto it = corners.find(key);
    if (it == corners.end()) it = corners.emplace(key, SampleCorner3(lx0 + i, j, lz0 + k)).first;
    return it->second;
  };
  constexpr int kMaxLayers = 40;
  // Only columns whose surface can matter to this chunk's cells: near it themselves, or a
  // neighbour of one that is (a cell's shape reads the 3 × 3 columns around it).
  std::vector<std::uint8_t> near(static_cast<std::size_t>(kCols * kCols));
  for (int i = 0; i < kCols * kCols; ++i) {
    const Column& col = cols[static_cast<std::size_t>(i)];
    near[static_cast<std::size_t>(i)] =
        !col.outside && col.height + col.overhang + 4.0f >= static_cast<float>(y0 - 3) &&
        col.height - col.overhang - 4.0f <= static_cast<float>(y0 + S + 3);
  }
  const auto needed = [&](int x, int z) {
    for (int dz = -1; dz <= 1; ++dz)
      for (int dx = -1; dx <= 1; ++dx) {
        const int nx = x + dx, nz = z + dz;
        if (nx < -1 || nx > S || nz < -1 || nz > S) continue;
        if (near[static_cast<std::size_t>((nz + 1) * kCols + nx + 1)]) return true;
      }
    return false;
  };
  for (int z = -1; z <= S; ++z)
    for (int x = -1; x <= S; ++x) {
      const Column& col = cols[(z + 1) * kCols + x + 1];
      if (col.outside || !needed(x, z)) continue;
      const std::int32_t wx = x0 + x, wz = z0 + z;
      const std::int32_t i = FloorDiv(wx, kLattice) - lx0, k = FloorDiv(wz, kLattice) - lz0;
      const int fx = FloorMod(wx, kLattice), fz = FloorMod(wz, kLattice);
      // The layers SurfaceOf reads: from one below its lowest to one above its highest.
      const std::int32_t jbase = FloorDiv(FloorToInt(col.height - col.overhang) - 2, kLattice) - 2;
      Corner3 layers[kMaxLayers];
      bool have[kMaxLayers] = {};
      const auto layer = [&](std::int32_t j) -> const Corner3& {
        const std::int32_t n = j - jbase;
        // The window holds every layer SurfaceOf can ask for (at most 2 × overhang + 7 m).
        assert(n >= 0 && n < kMaxLayers);
        if (!have[n]) {
          const Corner3 c[4] = {corner(i, k, j), corner(i + 1, k, j), corner(i, k + 1, j),
                                corner(i + 1, k + 1, j)};
          layers[n] = Bilerp(c, fx, fz);
          have[n] = true;
        }
        return layers[n];
      };
      out[static_cast<std::size_t>((z + 1) * kCols + x + 1)] = SurfaceOf(col, layer);
    }
}

namespace {

// Corner heights (halves of a cell, absolute) of the cell at the centre of a 3×3 block of column
// surfaces: each corner is the mean of the four columns around it, rounded to the nearest half.
// False if any of the nine has no clean surface.
template <class SurfaceCol>
bool CellCorners(const SurfaceCol (&s)[3][3], std::int32_t (&h)[4]) {
  for (const auto& row : s)
    for (const auto& c : row)
      if (!c.valid) return false;
  const auto corner = [&](int r, int c) {
    const float sum =
        ((s[r][c].height + s[r][c + 1].height) + s[r + 1][c].height) + s[r + 1][c + 1].height;
    return FloorToInt(sum * 0.5f + 0.5f);
  };
  h[0] = corner(0, 0);  // NW
  h[1] = corner(0, 1);  // NE
  h[2] = corner(1, 1);  // SE
  h[3] = corner(1, 0);  // SW
  return true;
}

// What the slope rule makes of a column: the cells below `floor` are solid, the cell at `floor`
// is `piece` (a full cube, a shape, or air), and the cells above it are open. One shaped cell per
// column, always on solid ground: a piece never rests on another piece's slope.
struct ColumnShape {
  std::int32_t floor = 0;
  slopes::Piece piece;
};

// The column's shape from its corner heights (halves, absolute) and its own surface (m). Corners
// that fit one cell give that cell, exactly (shared corners agree with every neighbour). Where they
// span more — ground steeper than a block per cell, or a block from a half-height to the next — the
// cell holding the column's own surface is chosen (within the corners' span) and the corners are
// clamped to it: a slope on top of a step instead of cubes, or of two pieces stacked with a gap.
// The cell stays within a block of the column's own surface either way.
ColumnShape ShapeColumn(const std::int32_t (&h)[4], float surface) {
  const std::int32_t lo = std::min(std::min(h[0], h[1]), std::min(h[2], h[3]));
  const std::int32_t hi = std::max(std::max(h[0], h[1]), std::max(h[2], h[3]));
  const std::int32_t own = FloorToInt(surface);
  // The lowest cell whose top reaches every corner, and the highest whose floor is under them all.
  const std::int32_t reach = FloorDiv(hi + 1, 2) - 1, under = FloorDiv(lo, 2);
  // A column above all its corners (a peak, a cliff's top) may keep a cube at its own height, and
  // one below them all (a pit) a cube up to them, rather than a piece a block off its surface.
  std::int32_t y = reach <= under ? reach : std::clamp(own, under - 1, reach + 1);
  y = std::clamp(y, own - 1, own + 1);
  int q[4];
  for (int i = 0; i < 4; ++i) q[i] = static_cast<int>(std::clamp(h[i] - 2 * y, 0, 2));
  return {y, slopes::PieceFor(q[0], q[1], q[2], q[3])};
}

// The piece of cell y of a shaped column.
slopes::Piece PieceAtHeight(const ColumnShape& c, std::int32_t y) {
  if (y < c.floor) return {slopes::Kind::kFull, 0};
  if (y > c.floor) return {slopes::Kind::kAir, 0};
  return c.piece;
}

template <class SurfaceCol>
std::optional<ColumnShape> ShapeColumnOf(const SurfaceCol (&s)[3][3]) {
  std::int32_t h[4];
  if (!CellCorners(s, h)) return std::nullopt;
  return ShapeColumn(h, s[1][1].height);
}

}  // namespace

std::optional<slopes::Piece> TerrainGenerator::SlopePieceAt(std::int32_t x, std::int32_t y,
                                                            std::int32_t z) const {
  SurfaceColumn s[3][3];
  for (int dz = 0; dz < 3; ++dz)
    for (int dx = 0; dx < 3; ++dx) s[dz][dx] = SurfaceAt(x - 1 + dx, z - 1 + dz);
  const auto shape = ShapeColumnOf(s);
  if (!shape) return std::nullopt;
  return PieceAtHeight(*shape, y);
}

void TerrainGenerator::ShapeSurface(const std::vector<Column>& cols,
                                    const std::vector<SurfaceColumn>& surfaces, std::int32_t y0,
                                    std::array<MaterialId, core::kChunkVolume>& voxels) const {
  constexpr int S = kChunkSize, kCols = S + 2;
  const auto column = [&](int x, int z) -> const Column& { return cols[(z + 1) * kCols + x + 1]; };
  for (int z = 0; z < S; ++z)
    for (int x = 0; x < S; ++x) {
      SurfaceColumn s[3][3];
      for (int dz = 0; dz < 3; ++dz)
        for (int dx = 0; dx < 3; ++dx) {
          s[dz][dx] = surfaces[static_cast<std::size_t>((z + dz) * kCols + x + dx)];
        }
      const auto shape = ShapeColumnOf(s);
      if (!shape) continue;
      const Column& col = column(x, z);
      const float slope = ColumnSlope(column, x, z);
      // The cube terrain's top voxel; it and the one below are solid, the one above open.
      const std::int32_t fl = FloorToInt(s[1][1].height);
      const std::int32_t cube_top = static_cast<float>(fl) == s[1][1].height ? fl - 1 : fl;
      // Top-down from above both tops to the cell under the piece: cells above the piece open,
      // the piece, and solid below it (filling where the column's cubes were lower); the topmost
      // filled cell is the surface (run 0).
      const std::int32_t f = shape->floor;
      int run = 0;
      for (std::int32_t y = std::max(f, cube_top) + 1; y >= std::min(f - 1, cube_top); --y) {
        if (y < kWorldMinY + kBedrockLayers + 1) break;
        const slopes::Piece piece = PieceAtHeight(*shape, y);
        const std::int32_t ly = y - y0;
        const bool here = ly >= 0 && ly < S;
        MaterialId* v = here ? &voxels[core::LocalIndex(x, ly, z)] : nullptr;
        if (piece.kind == slopes::Kind::kAir) {
          // A cube the surface lies below: removed (sea fills the hollow).
          if (v && *v != M::kAir && *v != M::kWater) *v = y < col.water ? M::kWater : M::kAir;
          continue;
        }
        const bool flooded = y < col.water;
        if (v) {
          const MaterialId cube = SurfaceMaterial(col, run, flooded, y, slope);
          if (piece.kind == slopes::Kind::kShaped) {
            *v = slopes::StateFor(cube, piece, flooded);
          } else {
            // Solid under the piece: the cells the column rises into, a pocket right under the
            // piece, and the surface layers re-counted from the new top.
            *v = cube;
          }
        }
        ++run;
      }
    }
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
      chance = col.temperature > kTreeLineTemperature ? 0.12f : 0.0f;  // below the tree line
      kind = Feature::Kind::kSpruce;
      break;
    default:
      break;
  }
  if (Unit(Mix32(h ^ 0x5bd1e995u)) >= chance) return std::nullopt;
  const auto ground = GroundY(x, z);
  if (!ground || *ground < col.water) return std::nullopt;
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
  if (!ground || *ground < col.water) return std::nullopt;
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
  // The base log goes one voxel lower than the ground the feature was found on: where the slope
  // stage shaved that voxel into a slope (or removed it), the trunk still stands on solid ground.
  write(f.x, f.y - 1, f.z, M::kLog);
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

void TerrainGenerator::ChunkColumns(std::int32_t x0, std::int32_t z0,
                                    std::vector<Column>& cols) const {
  // Columns −1..S (one beyond each side, for slopes), from a 2D lattice of corners.
  constexpr int S = kChunkSize, kCols = S + 2;
  const std::int32_t lx0 = FloorDiv(x0 - 1, kLattice), lz0 = FloorDiv(z0 - 1, kLattice);
  const int nl = FloorDiv(x0 + S, kLattice) - lx0 + 2;
  std::vector<Corner2> corners2(static_cast<std::size_t>(nl * nl));
  for (int j = 0; j < nl; ++j)
    for (int i = 0; i < nl; ++i) corners2[j * nl + i] = SampleCorner2(lx0 + i, lz0 + j);
  cols.resize(kCols * kCols);
  for (int z = -1; z <= S; ++z)
    for (int x = -1; x <= S; ++x) {
      const std::int32_t wx = x0 + x, wz = z0 + z;
      const int i = FloorDiv(wx, kLattice) - lx0, j = FloorDiv(wz, kLattice) - lz0;
      const Corner2 c[4] = {corners2[j * nl + i], corners2[j * nl + i + 1],
                            corners2[(j + 1) * nl + i], corners2[(j + 1) * nl + i + 1]};
      Column& col = cols[(z + 1) * kCols + x + 1];
      col = Interp2(c, FloorMod(wx, kLattice), FloorMod(wz, kLattice));
      col.outside = !core::InsideWorldDisc(wx, wz);
    }
}

float TerrainGenerator::SkyFloor(const std::vector<Column>& cols) {
  // Top of the terrain (height + overhang reach) of the columns inside the disc, with room for
  // trees from neighbouring columns. Chunks starting above it (and not below sea level) are air.
  float top = -1e9f;
  for (const Column& col : cols) {
    if (!col.outside) {
      top = std::max(top, std::max(col.height + col.overhang, static_cast<float>(col.water)));
    }
  }
  return top + static_cast<float>(kTreeReach + 12);
}

float TerrainGenerator::SkyFloorAt(std::int32_t cx, std::int32_t cz) const {
  std::vector<Column> cols;
  ChunkColumns(cx * kChunkSize, cz * kChunkSize, cols);
  return SkyFloor(cols);
}

bool TerrainGenerator::IsAirChunk(const ChunkCoord& coord, float sky_floor) {
  const std::int32_t y0 = coord.y * kChunkSize;
  if (y0 + kChunkSize <= kWorldMinY || y0 >= kWorldMaxY) return true;
  if (core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) return true;
  return y0 >= kSeaLevel && static_cast<float>(y0) > sky_floor;
}

bool TerrainGenerator::IsAirChunk(const ChunkCoord& coord) const {
  const std::int32_t y0 = coord.y * kChunkSize;
  if (y0 < kSeaLevel || y0 >= kWorldMaxY ||
      core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) {
    return IsAirChunk(coord, 0.0f);  // decided without the columns
  }
  return IsAirChunk(coord, SkyFloorAt(coord.x, coord.z));
}

void TerrainGenerator::Generate(const ChunkCoord& coord, Chunk& chunk, std::uint8_t stages) const {
  constexpr int S = kChunkSize;
  constexpr int kCols = S + 2;
  const std::int32_t x0 = coord.x * S, y0 = coord.y * S, z0 = coord.z * S;
  auto& voxels = chunk.generation_voxels();  // all air
  if (y0 + S <= kWorldMinY || y0 >= kWorldMaxY) return;
  // Beyond the rim of the world's disc: nothing, not even bedrock (the void).
  if (core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) return;

  // 1–2. Climate and base height per column.
  std::vector<Column> cols;
  ChunkColumns(x0, z0, cols);
  float top = -1e9f;
  for (const Column& col : cols) {
    if (!col.outside) top = std::max(top, col.height + col.overhang);
  }
  const auto column = [&](int x, int z) -> const Column& { return cols[(z + 1) * kCols + x + 1]; };

  // Sky above all terrain and sea, with room for trees from neighbouring columns: all air
  // (IsAirChunk is this same test).
  if (static_cast<float>(y0) > SkyFloor(cols) && y0 >= kSeaLevel) return;

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

  // 5b. Slopes: the surface cells follow the continuous surface (SLOPE_BLOCKS.md §5).
  if (stages & kStageSlopes) {
    float lo_band = 1e9f;
    for (const Column& col : cols) {
      if (!col.outside) lo_band = std::min(lo_band, col.height - col.overhang);
    }
    if (static_cast<float>(y0 + S + 3) >= lo_band - 4.0f &&
        static_cast<float>(y0 - 3) <= top + 4.0f) {
      std::vector<SurfaceColumn> surfaces;
      ChunkSurfaces(cols, x0, y0, z0, surfaces);
      ShapeSurface(cols, surfaces, y0, voxels);
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
        const Column& col = column(x, z);
        const bool sea = y0 + y < col.water && col.height < static_cast<float>(col.water);
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
    if (column(x, z).outside) return;  // features stop at the rim
    MaterialId& cur = voxels[core::LocalIndex(x, y, z)];
    const bool shaped = core::GetMaterial(cur).shape == core::VoxelShape::kShaped;
    if (cur == M::kAir || (m == M::kLog && (cur == M::kLeaves || shaped))) cur = m;
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
      if (const auto f = TreeInCell(cx, cz); f && in_range(*f, 1, kTreeReach))
        PlaceFeature(*f, write);
}

// --- level of detail (§6.6) ------------------------------------------------------------------

namespace {
// Rows of cells (m) near the surface where LOD cells evaluate caves, and room above the terrain
// for features.
constexpr int kLodCaveCells = 3;
constexpr float kLodFeatureReach = 16.0f;
// Tunnels are about 4 m wide: carved only in cells narrower than that.
constexpr std::int64_t kTunnelWidth = 4;

// Section columns −2..33 (the apron plus one more on each side, for slopes).
constexpr int kLodCols = core::kLodPad + 2;
int LodCol(int x, int z) { return (z + 2) * kLodCols + x + 2; }

template <class ColumnOf>
core::LodBounds LodBoundsOf(ColumnOf&& column, std::int64_t cell) {
  core::LodBounds b;
  b.lo = std::numeric_limits<double>::infinity();
  b.hi = -std::numeric_limits<double>::infinity();
  b.any_inside = false;
  bool all_inside = true;
  for (int z = -1; z <= core::kLodSectionCells; ++z)
    for (int x = -1; x <= core::kLodSectionCells; ++x) {
      const Column& col = column(x, z);
      if (col.outside) {
        all_inside = false;
        continue;
      }
      b.any_inside = true;
      b.hi = std::max(b.hi, static_cast<double>(col.height + col.overhang + kLodFeatureReach));
      b.hi = std::max(b.hi, static_cast<double>(col.water));  // river and lake water above the sea
      b.lo = std::min(b.lo, static_cast<double>(col.height - col.overhang * 1.1f -
                                                static_cast<float>(kLodCaveCells * cell)));
    }
  if (!b.any_inside) return b;
  b.hi = std::max(b.hi, static_cast<double>(kSeaLevel - 1));
  if (!all_inside) b.lo = -std::numeric_limits<double>::infinity();
  return b;
}
}  // namespace

namespace {
// LOD layout anchors lie this many columns apart. A block of columns between four anchors is
// interpolated only where the terrain hardly depends on the coast distance: the four lie wholly
// inside one continent's interior (further than kLodInterior from its coast) or in the deep sea
// (beyond the foot of any continental slope) with no island plate near. Anywhere else — coasts,
// shelves, channels between continents, islands, whose coast detail is rougher than a block — every
// column is evaluated exactly.
constexpr int kLodLayoutStride = 4;
constexpr float kLodInterior = 80'000.0f;
constexpr float kLodDeepSea = -215'000.0f;
}  // namespace

void TerrainGenerator::LodLayout(std::int64_t origin_x, std::int64_t origin_z, std::int64_t cell,
                                 int first, int count, std::vector<MacroCorner>& out) const {
  const int kept = OctavesResolved(continents::kCoastWavelength, continents::kCoastOctaves, cell);
  const auto exact = [&](int x, int z) {
    // With the internal plate edges: the uplift belts along convergent ones raise the valley
    // floor and the relief (rivers.h kBelt*), at every level.
    return continents_.At(origin_x + x * cell + cell / 2, origin_z + z * cell + cell / 2, kept,
                          true);
  };
  const int blocks = (count - 1 + kLodLayoutStride - 1) / kLodLayoutStride;
  const int anchors = blocks + 1;
  std::vector<MacroCorner> a(static_cast<std::size_t>(anchors * anchors));
  for (int j = 0; j < anchors; ++j)
    for (int i = 0; i < anchors; ++i) {
      a[static_cast<std::size_t>(j * anchors + i)] =
          exact(first + i * kLodLayoutStride, first + j * kLodLayoutStride);
    }
  out.assign(static_cast<std::size_t>(count * count), MacroCorner{});
  for (int bj = 0; bj < blocks; ++bj)
    for (int bi = 0; bi < blocks; ++bi) {
      const MacroCorner* c[4] = {&a[static_cast<std::size_t>(bj * anchors + bi)],
                                 &a[static_cast<std::size_t>(bj * anchors + bi + 1)],
                                 &a[static_cast<std::size_t>((bj + 1) * anchors + bi)],
                                 &a[static_cast<std::size_t>((bj + 1) * anchors + bi + 1)]};
      bool interior = true, deep = true;
      for (const MacroCorner* m : c) {
        interior = interior && m->coast > kLodInterior && m->continent >= 0 &&
                   m->continent == c[0]->continent;
        deep = deep && m->coast < kLodDeepSea && m->continent != continents::kIslandId &&
               !m->island_plates_near;
      }
      const bool plain = interior || deep;
      for (int dj = 0; dj < kLodLayoutStride; ++dj)
        for (int di = 0; di < kLodLayoutStride; ++di) {
          const int i = bi * kLodLayoutStride + di, j = bj * kLodLayoutStride + dj;
          if (i >= count || j >= count) continue;
          MacroCorner& m = out[static_cast<std::size_t>(j * count + i)];
          if (di == 0 && dj == 0) {
            m = *c[0];
          } else if (!plain) {
            m = exact(first + i, first + j);
          } else {
            const float tx = static_cast<float>(di) * (1.0f / kLodLayoutStride);
            const float tz = static_cast<float>(dj) * (1.0f / kLodLayoutStride);
            const auto lerp = [&](float MacroCorner::*f) {
              return Lerp(Lerp(c[0]->*f, c[1]->*f, tx), Lerp(c[2]->*f, c[3]->*f, tx), tz);
            };
            m.coast = lerp(&MacroCorner::coast);
            m.plate_edge = lerp(&MacroCorner::plate_edge);
            m.convergence = lerp(&MacroCorner::convergence);
            m.elevation = lerp(&MacroCorner::elevation);
            m.shelf = lerp(&MacroCorner::shelf);
            m.continent =
                c[(di >= kLodLayoutStride / 2 ? 1 : 0) + (dj >= kLodLayoutStride / 2 ? 2 : 0)]
                    ->continent;
          }
        }
    }
}

Column TerrainGenerator::ColumnLod(std::int64_t x, std::int64_t z, std::int64_t cell,
                                   const MacroCorner* layout) const {
  const auto kept = [&](std::int32_t wavelength, int octaves) {
    return OctavesResolved(wavelength, octaves, cell);
  };
  Corner2 c;
  c.continentalness = Fbm2(seeds_.continent, x, z, 1400, 5, kept(1400, 5));
  c.erosion = Fbm2(seeds_.erosion, x, z, 700, 3, kept(700, 3));
  c.temperature = Temperature(x, z, kept(1100, 2));
  c.humidity = Humidity(x, z, kept(900, 2));
  c.hills = Fbm2(seeds_.hills, x, z, 96, 4, kept(96, 4));
  c.ridges = Ridged2(seeds_.ridges, x, z, 360, 5, kept(360, 5));
  c.macro = Fbm2(seeds_.macro, x, z, kMacroWavelength, 4, kept(kMacroWavelength, 4));
  c.relief = Ridged2(seeds_.relief, x, z, kReliefWavelength, 4, kept(kReliefWavelength, 4));
  // Mountains switch on where erosion is low and rise by the ridged field: both are thresholded,
  // so a cell too wide for any of their octaves must not read their mean (zero erosion: no
  // mountain; zero ridges: none of their height), which would flatten every interior from afar.
  // It takes the means of what it cannot resolve, as measured over the world.
  if (kept(700, 3) == 0) c.erosion = kLodErosion;
  if (kept(360, 5) == 0) c.ridges = kLodRidges;
  // The continent layout: from the lattice the chunks use, or at cells of its own spacing and
  // wider evaluated at the cell's centre with the coast octaves it can resolve.
  const MacroCorner m = layout ? *layout
                        : cell >= continents::kMacroStep
                            ? continents_.At(x, z,
                                             OctavesResolved(continents::kCoastWavelength,
                                                             continents::kCoastOctaves, cell),
                                             true)
                            : continents_.Sample(x, z);
  c.coast = m.coast;
  c.plate_edge = m.plate_edge;
  c.convergence = m.convergence;
  c.elevation = m.elevation;
  c.shelf = m.shelf;
  c.continent = m.continent;
  c.water = rivers::Sample(river_seeds_, x, z, cell, LakeOracle(*this));
  Column col = Finish(c);
  col.outside = !core::InsideWorldDisc64(x, z);
  return col;
}

TerrainGenerator::Corner3 TerrainGenerator::NoiseLod(std::int64_t x, std::int64_t y, std::int64_t z,
                                                     std::int64_t cell) const {
  Corner3 n{0.0f, 1.0f, 1.0f, 0.0f};  // dropped: no overhang, no tunnel, no cavern
  if (const int k = OctavesResolved(20, 2, cell)) {
    n.overhang = Fbm3(seeds_.overhang, x, y, z, 28, 20, 28, 2, k);
  }
  if (cell < kTunnelWidth) {
    n.spaghetti_a = Perlin3(seeds_.spaghetti_a, Lattice(x, 56), Lattice(y, 36), Lattice(z, 56));
    n.spaghetti_b = Perlin3(seeds_.spaghetti_b, Lattice(x, 56), Lattice(y, 36), Lattice(z, 56));
  }
  if (const int k = OctavesResolved(48, 2, cell)) {
    n.cheese = Fbm3(seeds_.cheese, x, y, z, 90, 48, 90, 2, k);
  }
  return n;
}

core::LodBounds TerrainGenerator::LodBoundsAt(int level, std::int32_t i, std::int32_t k) const {
  const core::LodOrigin o = core::LodSectionOrigin({level, i, 0, k});
  const std::int64_t cell = core::LodCellSize(level);
  std::vector<Column> cols(static_cast<std::size_t>(core::kLodPad * core::kLodPad));
  std::vector<MacroCorner> layout;
  const bool have_layout = cell >= continents::kMacroStep;
  if (have_layout) LodLayout(o.x, o.z, cell, -1, core::kLodPad, layout);
  for (int z = -1; z <= core::kLodSectionCells; ++z)
    for (int x = -1; x <= core::kLodSectionCells; ++x) {
      const MacroCorner* m =
          have_layout ? &layout[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)]
                      : nullptr;
      cols[(z + 1) * core::kLodPad + x + 1] =
          ColumnLod(o.x + x * cell + cell / 2, o.z + z * cell + cell / 2, cell, m);
    }
  return LodBoundsOf(
      [&](int x, int z) -> const Column& { return cols[(z + 1) * core::kLodPad + x + 1]; }, cell);
}

core::LodKind TerrainGenerator::GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                                            core::LodSurfaces* surface) const {
  using core::kLodSectionCells;
  using core::LodCell;
  cells.assign(core::kLodVolume, M::kAir);
  if (surface) surface->assign(static_cast<std::size_t>(core::kLodPad * core::kLodPad), {});
  if (!core::LodInWorld(c)) return core::LodKind::kEmpty;
  const core::LodOrigin o = core::LodSectionOrigin(c);
  const std::int64_t cell = core::LodCellSize(c.level);
  const std::int64_t half = cell / 2;

  // 1–2. Climate and base height at each column's centre.
  std::vector<Column> cols(static_cast<std::size_t>(kLodCols * kLodCols));
  std::vector<MacroCorner> layout;
  const bool have_layout = cell >= continents::kMacroStep;
  if (have_layout) LodLayout(o.x, o.z, cell, -2, kLodCols, layout);
  for (int z = -2; z <= kLodSectionCells + 1; ++z)
    for (int x = -2; x <= kLodSectionCells + 1; ++x) {
      const MacroCorner* m =
          have_layout ? &layout[static_cast<std::size_t>((z + 2) * kLodCols + x + 2)] : nullptr;
      cols[LodCol(x, z)] = ColumnLod(o.x + x * cell + half, o.z + z * cell + half, cell, m);
    }
  const auto column = [&](int x, int z) -> const Column& { return cols[LodCol(x, z)]; };
  const core::LodKind kind = core::LodKindFromBounds(c, LodBoundsOf(column, cell));
  if (kind == core::LodKind::kEmpty) return kind;
  if (kind == core::LodKind::kBuried) {
    for (int y = -1; y <= kLodSectionCells; ++y) {
      const std::int64_t a = o.y + y * cell;
      std::fill_n(cells.begin() + LodCell(-1, y, -1), core::kLodPad * core::kLodPad,
                  a < kWorldMinY + kBedrockLayers ? M::kBedrock : M::kStone);
    }
    return kind;
  }

  // 3–5. Cells top-down per column (from above the section, for surface depth): classified at
  // their bottom voxel, then surface materials by depth in metres.
  const int pad_rows = static_cast<int>(std::max<std::int64_t>(1, (kSurfacePad + cell - 1) / cell));
  for (int z = -1; z <= kLodSectionCells; ++z)
    for (int x = -1; x <= kLodSectionCells; ++x) {
      const Column& col = column(x, z);
      const std::int64_t wx = o.x + x * cell + half, wz = o.z + z * cell + half;
      if (col.outside) {
        continue;  // beyond the rim: the void
      }
      float slope = 0.0f;
      for (const auto& [dx, dz] :
           {std::pair{1, 0}, std::pair{-1, 0}, std::pair{0, 1}, std::pair{0, -1}}) {
        const float d = column(x + dx, z + dz).height - col.height;
        slope = std::max(slope, d < 0.0f ? -d : d);
      }
      slope /= static_cast<float>(cell);
      const float deep =
          col.height - col.overhang * 1.1f - static_cast<float>(kLodCaveCells * cell);
      int run = 1000;  // solid above the padded top: treat as deep
      bool under_water = false;
      core::LodSurface* surf =
          surface ? &(*surface)[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)]
                  : nullptr;
      bool surfaced = false;  // the column's topmost filled cell has been seen
      for (int y = kLodSectionCells + pad_rows; y >= -1; --y) {
        const std::int64_t a = o.y + y * cell;
        const std::size_t idx =
            y <= kLodSectionCells ? static_cast<std::size_t>(LodCell(x, y, z)) : core::kLodVolume;
        if (a < kWorldMinY) {
          if (idx < core::kLodVolume) cells[idx] = M::kBedrock;  // the apron under the world
          continue;
        }
        const auto ay = static_cast<std::int32_t>(a);
        const Cell k = Classify(col, ay, [&] {
          return static_cast<float>(a) < deep ? Corner3{0.0f, 1.0f, 1.0f, 0.0f}
                                              : NoiseLod(wx, a, wz, cell);
        });
        if (k != kSolid) {
          run = k == kCaveAir ? 1000 : 0;
          under_water = k == kWater;
          if (idx < core::kLodVolume) cells[idx] = k == kWater ? M::kWater : M::kAir;
          if (k == kWater && !surfaced && idx < core::kLodVolume) {
            surfaced = true;  // the sea's cells top the column; its floor lies below them
          }
          continue;
        }
        if (idx < core::kLodVolume) {
          const int depth =
              run >= 1000 ? 1000 : static_cast<int>(std::min<std::int64_t>(run * cell, 1000));
          if (run == 0) {
            // The top of the column: the material of its surface as seen from above, taken where
            // the surface lies within the cell (cells taller than the relief sample the world's
            // floor, but should look like the ground on top of them). A sea whose water no cell
            // sampled (the cell is deeper than the sea) shows its water, as Downsample keeps it.
            const auto top = static_cast<std::int32_t>(
                std::clamp<std::int64_t>(FloorToInt(col.height), a, a + cell - 1));
            const bool sea = col.height < static_cast<float>(col.water);
            cells[idx] = a + cell <= kWorldMinY + kBedrockLayers ? M::kBedrock
                         : !under_water && sea                   ? M::kWater
                                               : SurfaceMaterial(col, 0, under_water, top, slope);
            // The column's surface, exactly: the ground's height (a sea's floor, whose cell may
            // have been drawn as water), if it lies in this cell — or above it, where 3D noise
            // cut the ground lower (then the cell's top). Below it (noise raised the ground),
            // the whole cell stands.
            const float h = col.height;
            if (surf && (!surfaced || under_water) && h >= static_cast<float>(a) &&
                a + cell > kWorldMinY + kBedrockLayers) {
              surf->valid = true;
              surf->wet = sea;
              surf->height = std::min(h, static_cast<float>(a + cell));
              surf->material = SurfaceMaterial(col, 0, sea, top, slope);
              surf->water = sea ? static_cast<float>(col.water) : 0.0f;
            }
            surfaced = true;
          } else {
            cells[idx] = ay < kWorldMinY + kBedrockLayers
                             ? M::kBedrock
                             : SurfaceMaterial(col, depth, under_water, ay, slope);
          }
        }
        if (run < 1000) ++run;
      }
    }

  // 8. Features at least a cell wide: counted per cell, a cell taking a feature's material when
  // the feature fills at least half of it (as Downsample would keep it).
  if (cell > 4) return kind;
  const std::int64_t volume = cell * cell * cell;
  std::vector<std::uint16_t> stone(core::kLodVolume), wood(core::kLodVolume),
      leaves(core::kLodVolume);
  const std::int64_t span = (kLodSectionCells + 1) * cell;
  const auto write = [&](std::int32_t vx, std::int32_t vy, std::int32_t vz, MaterialId m) {
    const auto local = [&](std::int64_t v, std::int64_t origin) {
      const std::int64_t d = v - origin;
      return d < -cell || d >= span ? std::int64_t{-2} : (d >= 0 ? d / cell : -1);
    };
    const std::int64_t cx = local(vx, o.x), cy = local(vy, o.y), cz = local(vz, o.z);
    if (cx < -1 || cy < -1 || cz < -1) return;
    if (!core::InsideWorldDisc(vx, vz)) return;
    const auto idx = static_cast<std::size_t>(
        LodCell(static_cast<int>(cx), static_cast<int>(cy), static_cast<int>(cz)));
    auto& counts = m == M::kLog ? wood : m == M::kLeaves ? leaves : stone;
    ++counts[idx];
  };
  const std::int64_t x_lo = o.x - cell, x_hi = o.x + span, z_lo = o.z - cell, z_hi = o.z + span;
  const std::int64_t y_lo = o.y - cell, y_hi = o.y + span;
  const auto in_range = [&](const Feature& f, int down, int up) {
    return f.y + up >= y_lo && f.y - down < y_hi;
  };
  const auto cells_of = [](std::int64_t lo, std::int64_t hi, int reach, int size) {
    return std::pair{FloorDiv(static_cast<std::int32_t>(lo - reach), size),
                     FloorDiv(static_cast<std::int32_t>(hi + reach), size)};
  };
  if (cell <= 2) {
    const auto [bx0, bx1] = cells_of(x_lo, x_hi, kBoulderMaxRadius, kBoulderCell);
    const auto [bz0, bz1] = cells_of(z_lo, z_hi, kBoulderMaxRadius, kBoulderCell);
    for (std::int32_t cz = bz0; cz <= bz1; ++cz)
      for (std::int32_t cx = bx0; cx <= bx1; ++cx)
        if (const auto f = BoulderInCell(cx, cz);
            f && in_range(*f, kBoulderMaxRadius, kBoulderMaxRadius)) {
          PlaceFeature(*f, write);
        }
  }
  const auto [tx0, tx1] = cells_of(x_lo, x_hi, kTreeSpread, kTreeCell);
  const auto [tz0, tz1] = cells_of(z_lo, z_hi, kTreeSpread, kTreeCell);
  for (std::int32_t cz = tz0; cz <= tz1; ++cz)
    for (std::int32_t cx = tx0; cx <= tx1; ++cx)
      if (const auto f = TreeInCell(cx, cz); f && in_range(*f, 1, kTreeReach)) {
        PlaceFeature(*f, write);
      }
  for (std::size_t i = 0; i < cells.size(); ++i) {
    if (cells[i] != M::kAir) continue;
    if (2 * std::int64_t{stone[i]} >= volume) {
      cells[i] = M::kStone;
    } else if (2 * (std::int64_t{wood[i]} + leaves[i]) >= volume) {
      cells[i] = wood[i] >= leaves[i] ? M::kLog : M::kLeaves;
    }
  }
  return kind;
}

// --- spawn ----------------------------------------------------------------------------------

bool TerrainGenerator::LevelSpawnY(std::int32_t x, std::int32_t z, std::int32_t& feet) const {
  // Surfaces of the 7 × 7 columns around (x, z) give the corner heights of its 5 × 5 cells.
  SurfaceColumn s[7][7];
  for (int dz = 0; dz < 7; ++dz)
    for (int dx = 0; dx < 7; ++dx) {
      s[dz][dx] = SurfaceAt(x - 3 + dx, z - 3 + dz);
      if (!s[dz][dx].valid) return false;
    }
  ColumnShape shapes[5][5];
  std::int32_t lo = std::numeric_limits<std::int32_t>::max(),
               hi = std::numeric_limits<std::int32_t>::min();
  for (int cz = 0; cz < 5; ++cz)
    for (int cx = 0; cx < 5; ++cx) {
      SurfaceColumn block[3][3];
      for (int dz = 0; dz < 3; ++dz)
        for (int dx = 0; dx < 3; ++dx) block[dz][dx] = s[cz + dz][cx + dx];
      std::int32_t h[4];
      if (!CellCorners(block, h)) return false;
      shapes[cz][cx] = ShapeColumn(h, block[1][1].height);
      for (const std::int32_t c : h) {
        lo = std::min(lo, c);
        hi = std::max(hi, c);
      }
    }
  if (hi - lo > 1) return false;  // within half a block
  for (std::int32_t y = FloorDiv(lo, 2); y <= FloorDiv(hi, 2) + 2; ++y) {
    bool fits = true;
    for (int cz = 0; cz < 5 && fits; ++cz)
      for (int cx = 0; cx < 5 && fits; ++cx) {
        fits = PieceAtHeight(shapes[cz][cx], y - 1).kind != slopes::Kind::kAir &&
               PieceAtHeight(shapes[cz][cx], y).kind == slopes::Kind::kAir &&
               PieceAtHeight(shapes[cz][cx], y + 1).kind == slopes::Kind::kAir;
      }
    if (fits) {
      feet = y;
      return true;
    }
  }
  return false;
}

std::array<double, 3> TerrainGenerator::SpawnPoint() const {
  std::optional<std::array<double, 3>> fallback;
  // A square spiral outwards from (cx, cz) in 8 m steps, for level open ground.
  const auto search = [&](std::int32_t cx, std::int32_t cz,
                          int steps) -> std::optional<std::array<double, 3>> {
    std::int32_t x = cx, z = cz, dx = 1, dz = 0, leg = 1, walked = 0, turns = 0;
    for (int n = 0; n < steps; ++n) {
      const Column col = ColumnAt(x, z);
      const bool land = col.height >= static_cast<float>(col.water + 2) && col.mountain < 0.05f &&
                        col.wet == 0.0f && col.biome != Biome::kBeach && col.biome != Biome::kOcean;
      if (land) {
        if (const auto g = GroundY(x, z)) {
          const std::array<double, 3> cube_here{x + 0.5, *g + 1.0, z + 0.5};
          if (!fallback) fallback = cube_here;
          // Level ground, slopes included: a 5 × 5 patch whose surface corners lie within half a
          // block of each other, standing on solid cells with two open cells above. No water in it
          // (a bank can slope into a river at the patch's edge).
          std::int32_t feet = 0;
          bool clear = LevelSpawnY(x, z, feet);
          for (int pz = -3; clear && pz <= 3; ++pz)
            for (int px = -3; clear && px <= 3; ++px) {
              clear = ColumnAt(x + px, z + pz).wet == 0.0f;
            }
          for (std::int32_t tz = FloorDiv(z - 6, kTreeCell);
               clear && tz <= FloorDiv(z + 6, kTreeCell); ++tz)
            for (std::int32_t tx = FloorDiv(x - 6, kTreeCell);
                 clear && tx <= FloorDiv(x + 6, kTreeCell); ++tx)
              if (const auto t = TreeInCell(tx, tz)) {
                clear = std::max(std::abs(t->x - x), std::abs(t->z - z)) > 5;
              }
          for (std::int32_t bz = FloorDiv(z - 6, kBoulderCell);
               clear && bz <= FloorDiv(z + 6, kBoulderCell); ++bz)
            for (std::int32_t bx = FloorDiv(x - 6, kBoulderCell);
                 clear && bx <= FloorDiv(x + 6, kBoulderCell); ++bx)
              if (const auto b = BoulderInCell(bx, bz)) {
                clear = std::max(std::abs(b->x - x), std::abs(b->z - z)) > 6;
              }
          if (clear) return std::array<double, 3>{x + 0.5, static_cast<double>(feet), z + 0.5};
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
    return std::nullopt;
  };

  // Near water if there is some close to the origin (a river or lake within kWaterSearch m, found
  // on rings of 32 m steps): search around the nearest of it; else, and if that finds nothing,
  // around the origin.
  constexpr std::int32_t kWaterSearch = 288, kWaterRing = 32;
  const auto nearest_water = [&]() -> std::optional<std::pair<std::int32_t, std::int32_t>> {
    for (std::int32_t r = kWaterRing; r <= kWaterSearch; r += kWaterRing)
      for (std::int32_t a = -r; a <= r; a += kWaterRing)
        for (const auto& [px, pz] :
             {std::pair{a, -r}, std::pair{a, r}, std::pair{-r, a}, std::pair{r, a}}) {
          const Column c = ColumnAt(px, pz);
          if (!c.outside && c.height < static_cast<float>(c.water)) return std::pair{px, pz};
        }
    return std::nullopt;
  };
  if (const auto water = nearest_water()) {
    if (const auto s = search(water->first, water->second, 1000)) return *s;
  }
  if (const auto s = search(0, 0, 40000)) return *s;
  return fallback.value_or(std::array<double, 3>{0.5, kSeaLevel + 1.0, 0.5});
}

}  // namespace dwell::worldgen
