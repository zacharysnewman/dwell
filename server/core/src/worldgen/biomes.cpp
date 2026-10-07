#include "dwell/worldgen/biomes.h"

namespace dwell::worldgen {
namespace {

namespace M = core::Materials;
using core::MaterialId;

// One row per biome, in Biome order. Tints (1/64) turn the grass tile (a vivid yellow-green) and
// the leaves tile (a mid green) into each biome's colours (WORLD_GENERATION.md §1.3: warm yellow-
// green meadows, deep teal conifers, pink blossom, orange autumn, golden dry grass).
constexpr TintColor kNeutral{64, 64, 64};
constexpr std::int8_t kStone8 = 8;
constexpr SurfaceLayer kStoneLayer{M::kStone, kStone8};
constexpr TreeChoice kOak[2] = {{TreeKind::kOak, 100}, {TreeKind::kOak, 0}};
constexpr TreeChoice kSpruce[2] = {{TreeKind::kSpruce, 100}, {TreeKind::kSpruce, 0}};

constexpr BiomeDef kBiomes[kBiomeCount] = {
    {Biome::kOcean,
     "ocean",
     {{M::kSand, 4}, {M::kSandstone, 6}, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    {Biome::kDeepOcean,
     "deep_ocean",
     {{M::kSand, 4}, {M::kSandstone, 6}, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    {Biome::kFrozenOcean,
     "frozen_ocean",
     {{M::kGravel, 4}, kStoneLayer, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    {Biome::kBeach,
     "beach",
     {{M::kSand, 4}, {M::kSandstone, 6}, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    {Biome::kSeaCliff,
     "sea_cliff",
     {kStoneLayer, kStoneLayer, kStoneLayer},
     M::kStone,
     8,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.3f,
     false},
    {Biome::kRiverbank,
     "riverbank",
     {{M::kGravel, 1}, {M::kSand, 4}, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    {Biome::kLakeShore,
     "lake_shore",
     {{M::kSand, 3}, {M::kSandstone, 5}, kStoneLayer},
     M::kStone,
     0,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.0f,
     false},
    // Temperate land.
    {Biome::kMeadow,
     "meadow",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.05f,
     {kOak[0], kOak[1]},
     {68, 64, 62},
     {66, 68, 56},
     0.3f,
     false},
    {Biome::kBroadleaf,
     "broadleaf_forest",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.7f,
     {kOak[0], kOak[1]},
     {56, 62, 58},
     {62, 70, 58},
     0.2f,
     true},
    {Biome::kBlossomGrove,
     "blossom_grove",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.55f,
     {{TreeKind::kBlossom, 75}, {TreeKind::kOak, 25}},
     {68, 64, 70},
     {134, 40, 154},
     0.2f,
     true},
    {Biome::kAutumnWoods,
     "autumn_woods",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.6f,
     {kOak[0], kOak[1]},
     {88, 60, 74},
     {125, 49, 49},
     0.2f,
     true},
    {Biome::kConifer,
     "conifer_forest",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.6f,
     {kSpruce[0], kSpruce[1]},
     {46, 58, 70},
     {35, 42, 89},
     0.2f,
     true},
    {Biome::kWetland,
     "wetland",
     {{M::kGrass, 1}, {M::kDirt, 5}, kStoneLayer},
     M::kStone,
     8,
     0.12f,
     {kOak[0], kOak[1]},
     {48, 60, 64},
     {40, 53, 66},
     0.0f,
     false},
    // Warm and dry.
    {Biome::kSavanna,
     "savanna",
     {{M::kGrass, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.03f,
     {kOak[0], kOak[1]},
     {89, 59, 75},
     {109, 64, 77},
     0.1f,
     false},
    {Biome::kDunes,
     "dunes",
     {{M::kSand, 4}, {M::kSandstone, 7}, kStoneLayer},
     M::kSandstone,
     3,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.1f,
     false},
    // Cold and high.
    {Biome::kTundra,
     "tundra",
     {{M::kGrass, 1}, {M::kDirt, 3}, kStoneLayer},
     M::kStone,
     8,
     0.02f,
     {kSpruce[0], kSpruce[1]},
     {70, 54, 77},
     {64, 54, 96},
     0.3f,
     false},
    {Biome::kAlpineMeadow,
     "alpine_meadow",
     {{M::kGrass, 1}, {M::kDirt, 3}, kStoneLayer},
     M::kStone,
     8,
     0.02f,
     {kSpruce[0], kSpruce[1]},
     {61, 67, 64},
     {58, 70, 58},
     0.3f,
     false},
    {Biome::kBareRock,
     "bare_rock",
     {{M::kGravel, 1}, kStoneLayer, kStoneLayer},
     M::kStone,
     8,
     0.0f,
     {kOak[0], kOak[1]},
     kNeutral,
     kNeutral,
     0.6f,
     false},
    {Biome::kSnowfield,
     "snowfield",
     {{M::kSnow, 1}, {M::kDirt, 4}, kStoneLayer},
     M::kStone,
     8,
     0.02f,
     {kSpruce[0], kSpruce[1]},
     {56, 60, 66},
     {40, 56, 80},
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

}  // namespace dwell::worldgen
