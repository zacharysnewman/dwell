#include "dwell/worldgen/biomes.h"

#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {
namespace {

namespace M = core::Materials;
using core::MaterialId;

// The accent foliage of the temperate biomes (§1.2 rule 4): yellow-green, orange, red, pink,
// violet.
constexpr MaterialId kAllAccents[5] = {M::kLeavesBright, M::kLeavesAutumn, M::kLeavesRed,
                                       M::kLeavesBlossom, M::kLeavesViolet};

// One row per biome, in Biome order.
constexpr BiomeDef kBiomes[kBiomeCount] = {
    {Biome::kOcean,
     "ocean",
     {{M::kSand, 4}, {M::kSandstone, 6}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    {Biome::kDeepOcean,
     "deep_ocean",
     {{M::kSand, 4}, {M::kSandstone, 6}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    {Biome::kFrozenOcean,
     "frozen_ocean",
     {{M::kGravel, 4}, {M::kStone, 8}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    {Biome::kBeach,
     "beach",
     {{M::kSand, 4}, {M::kSandstone, 6}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    {Biome::kSeaCliff,
     "sea_cliff",
     {{M::kStone, 8}, {M::kStone, 8}, {M::kStone, 8}},
     M::kStone,
     8,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.3f,
     false},
    {Biome::kRiverbank,
     "riverbank",
     {{M::kGravel, 1}, {M::kSand, 4}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    {Biome::kLakeShore,
     "lake_shore",
     {{M::kSand, 3}, {M::kSandstone, 5}, {M::kStone, 8}},
     M::kStone,
     0,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.0f,
     false},
    // Temperate land.
    {Biome::kMeadow,
     "meadow",
     {{M::kGrassMeadow, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.05f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.5f,
     5,
     {M::kLeavesBright, M::kLeavesAutumn, M::kLeavesRed, M::kLeavesBlossom, M::kLeavesViolet},
     0.3f,
     false},
    {Biome::kBroadleaf,
     "broadleaf_forest",
     {{M::kGrass, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.7f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.5f,
     5,
     {M::kLeavesBright, M::kLeavesAutumn, M::kLeavesRed, M::kLeavesBlossom, M::kLeavesViolet},
     0.2f,
     true},
    {Biome::kBlossomGrove,
     "blossom_grove",
     {{M::kGrassMeadow, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.55f,
     {{TreeKind::kBlossom, 75}, {TreeKind::kOak, 25}},
     0.6f,
     2,
     {M::kLeavesViolet, M::kLeavesBlossom},
     0.2f,
     true},
    {Biome::kAutumnWoods,
     "autumn_woods",
     {{M::kGrassGolden, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.6f,
     {{TreeKind::kAutumn, 65}, {TreeKind::kOak, 35}},
     0.5f,
     3,
     {M::kLeavesRed, M::kLeavesBright, M::kLeavesAutumn},
     0.2f,
     true},
    {Biome::kConifer,
     "conifer_forest",
     {{M::kGrass, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.6f,
     {{TreeKind::kSpruce, 100}, {TreeKind::kSpruce, 0}},
     0.0f,
     0,
     {},
     0.2f,
     true},
    {Biome::kWetland,
     "wetland",
     {{M::kGrass, 1}, {M::kDirt, 5}, {M::kStone, 8}},
     M::kStone,
     8,
     0.12f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.3f,
     2,
     {M::kLeavesBright, M::kLeavesViolet},
     0.0f,
     false},
    // Warm and dry.
    {Biome::kSavanna,
     "savanna",
     {{M::kGrassGolden, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.03f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.2f,
     2,
     {M::kLeavesBright, M::kLeavesAutumn},
     0.1f,
     false},
    {Biome::kDunes,
     "dunes",
     {{M::kSand, 4}, {M::kSandstone, 7}, {M::kStone, 8}},
     M::kSandstone,
     3,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.1f,
     false},
    // Cold and high.
    {Biome::kTundra,
     "tundra",
     {{M::kGrassGolden, 1}, {M::kDirt, 3}, {M::kStone, 8}},
     M::kStone,
     8,
     0.02f,
     {{TreeKind::kSpruce, 100}, {TreeKind::kSpruce, 0}},
     0.0f,
     0,
     {},
     0.3f,
     false},
    {Biome::kAlpineMeadow,
     "alpine_meadow",
     {{M::kGrassMeadow, 1}, {M::kDirt, 3}, {M::kStone, 8}},
     M::kStone,
     8,
     0.02f,
     {{TreeKind::kSpruce, 100}, {TreeKind::kSpruce, 0}},
     0.0f,
     0,
     {},
     0.3f,
     false},
    {Biome::kBareRock,
     "bare_rock",
     {{M::kGravel, 1}, {M::kStone, 8}, {M::kStone, 8}},
     M::kStone,
     8,
     0.0f,
     {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}},
     0.0f,
     0,
     {},
     0.6f,
     false},
    {Biome::kSnowfield,
     "snowfield",
     {{M::kSnow, 1}, {M::kDirt, 4}, {M::kStone, 8}},
     M::kStone,
     8,
     0.02f,
     {{TreeKind::kSpruce, 100}, {TreeKind::kSpruce, 0}},
     0.0f,
     0,
     {},
     0.3f,
     false},
};

// The climate zones (a temperature × humidity diagram), first match wins; every point of the
// diagram falls in the last row at the latest. Temperature is the ground's (field units, ≈ 20 °C
// each), humidity −1..1.
struct Zone {
  float t0, t1, h0, h1;  // t0 ≤ temperature < t1, h0 ≤ humidity < h1
  Biome biome;
};
constexpr Zone kZones[] = {
    {-0.20f, 0.05f, 0.00f, 0.50f, Biome::kAutumnWoods},  // cool, moderately humid
    {-1.01f, 0.45f, 0.55f, 1.01f, Biome::kWetland},      // very humid
    {-1.01f, -0.10f, -1.01f, -0.10f, Biome::kTundra},    // cold, dry
    {-1.01f, -0.10f, -0.10f, 0.55f, Biome::kConifer},    // cold
    {0.45f, 1.01f, -1.01f, 0.00f, Biome::kDunes},        // hot, very dry
    {0.15f, 1.01f, -1.01f, -0.20f, Biome::kSavanna},     // warm, dry
    {0.45f, 1.01f, 0.00f, 0.30f, Biome::kSavanna},       // hot, mid
    {0.10f, 0.45f, 0.15f, 0.55f, Biome::kBlossomGrove},  // warm, humid
    {-0.10f, 0.45f, 0.15f, 0.55f, Biome::kBroadleaf},    // temperate, humid
    {0.45f, 1.01f, 0.30f, 1.01f, Biome::kBroadleaf},     // hot, humid
    {-1.01f, 1.01f, -1.01f, 1.01f, Biome::kMeadow},      // the rest: temperate, mid and dry
};

}  // namespace

const char* BiomeName(Biome b) { return kBiomes[static_cast<int>(b)].name; }

const BiomeDef& BiomeOf(Biome b) { return kBiomes[static_cast<int>(b)]; }

Biome ClimateBiome(float temperature, float humidity, float height) {
  if (temperature < kSnowTemperature) return Biome::kSnowfield;
  if (height >= kAlpineMinHeight && temperature < kTreeLineTemperature) {
    return temperature < kBareRockTemperature ? Biome::kBareRock : Biome::kAlpineMeadow;
  }
  for (const Zone& z : kZones) {
    if (temperature >= z.t0 && temperature < z.t1 && humidity >= z.h0 && humidity < z.h1) {
      return z.biome;
    }
  }
  return Biome::kMeadow;
}

core::MaterialId DefaultLeaves(TreeKind kind) {
  switch (kind) {
    case TreeKind::kBlossom:
      return M::kLeavesBlossom;
    case TreeKind::kAutumn:
      return M::kLeavesAutumn;
    case TreeKind::kOak:
    case TreeKind::kSpruce:
      break;
  }
  return M::kLeaves;
}

float GroveCore(const GroveSeeds& seeds, std::int64_t x, std::int64_t z) {
  // ±0.25 or so: a grove is where the noise stands above 0.08, its core above 0.28.
  return SmoothStep(0.08f, 0.28f, Fbm2(seeds.grove, x, z, 320, 2));
}

core::MaterialId LeafMaterialAt(const GroveSeeds& seeds, const BiomeDef& biome, TreeKind kind,
                                std::int64_t x, std::int64_t z, std::uint32_t hash) {
  if (biome.accent_count == 0 || kind == TreeKind::kSpruce) return DefaultLeaves(kind);
  const float chance = biome.accent_share * GroveCore(seeds, x, z);
  if (chance <= 0.0f || Unit(Mix32(hash ^ 0x9e3779b9u)) >= chance) return DefaultLeaves(kind);
  // The patch's colour changes slowly (a ~600 m noise), so a grove is one colour.
  const float pick = Fbm2(seeds.patch, x, z, 640, 2) * 1.8f + 0.5f;
  const int n = biome.accent_count;
  int i = static_cast<int>(pick * static_cast<float>(n));
  i = i < 0 ? 0 : i >= n ? n - 1 : i;
  return biome.accents[i];
}

}  // namespace dwell::worldgen
