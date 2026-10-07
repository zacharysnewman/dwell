#pragma once

#include <cstdint>

#include "dwell/core/blocks.gen.h"

// The biome table (WORLD_GENERATION.md §3.5–3.7, Phase 11c): which biome a column is, and what
// each biome is made of. Selection and content are *data*, not code: a temperature × humidity grid
// of zones (a Whittaker-style diagram), a few altitude and terrain overrides, and one row per biome
// naming its surface layers, trees, accent foliage, boulders and whether a distant forest shows a
// canopy. The rows are prototype content (ARCHITECTURE.md §6.1) the real world style replaces
// without touching the pipeline.
namespace dwell::worldgen {

enum class Biome : std::uint8_t {
  kOcean,
  kDeepOcean,
  kFrozenOcean,
  kBeach,
  kSeaCliff,
  kRiverbank,
  kLakeShore,
  kMeadow,
  kBroadleaf,
  kBlossomGrove,
  kAutumnWoods,
  kConifer,
  kWetland,
  kSavanna,
  kDunes,
  kTundra,
  kAlpineMeadow,
  kBareRock,
  kSnowfield,
};
inline constexpr int kBiomeCount = 19;
const char* BiomeName(Biome b);
// The sea's biomes (a column below the sea at the coast's outer side).
constexpr bool IsSeaBiome(Biome b) {
  return b == Biome::kOcean || b == Biome::kDeepOcean || b == Biome::kFrozenOcean;
}

// Temperature is the field's units (about 20 °C each) at the ground, after the lapse rate: snow
// lies where it falls below kSnowTemperature, and above the tree line (kTreeLineTemperature), on
// ground at least kAlpineMinHeight m up, come alpine meadow, then bare rock, then snow. At a
// sea-level temperature of 0 these are 1,385 m, 860 m and 1,110 m.
inline constexpr float kSnowTemperature = -0.45f;
inline constexpr float kTreeLineTemperature = -0.28f;
inline constexpr float kBareRockTemperature = -0.36f;
inline constexpr float kAlpineMinHeight = 250.0f;
// Sea floors below this (m) are deep ocean; seas colder than this (field units) are frozen.
inline constexpr float kDeepOceanHeight = -300.0f;
inline constexpr float kFrozenOceanTemperature = -0.45f;

// The land biome the climate gives: ground temperature `t`, humidity `h` (both −1..1, field units)
// and the ground's height above sea level (m): snowfield below the snow temperature; alpine meadow
// then bare rock above the tree line on high ground; otherwise the zone table.
Biome ClimateBiome(float temperature, float humidity, float height);

// Tree shapes (§3.7): the oak (round crown), the spruce (a cone) and the blossom tree (a short
// trunk and a wide round crown). Their colour is not carried: it is the biome's foliage tint.
enum class TreeKind : std::uint8_t { kOak, kSpruce, kBlossom };

// A surface layer: the voxels with depth run < `until` below the surface (run 0 = the top voxel).
struct SurfaceLayer {
  core::MaterialId material;
  std::int8_t until;
};
struct TreeChoice {
  TreeKind kind;
  std::uint8_t weight;  // of 100 among the biome's choices
};

// A colour multiplier per channel in 1/64 (64 = ×1, up to ×3.98): what the tinted blocks' textures
// (grass, leaves) are multiplied by in a biome. Nothing is stored in the voxels: the client asks
// the generator for the tint of the columns it draws (TerrainGenerator::TintGrid).
struct TintColor {
  std::uint8_t r, g, b;
};
inline constexpr std::uint8_t kTintUnit = 64;

struct BiomeDef {
  Biome id;
  const char* name;
  // Surface (§3.6): the first layer whose `until` exceeds the depth; stone below the last. On steep
  // ground (neighbouring columns more than 3 m apart in height) `steep` replaces the layers for
  // depth < steep_until (0: no steep rule), stone below.
  SurfaceLayer layers[3];
  core::MaterialId steep;
  std::int8_t steep_until;
  // Vegetation (§3.7): the chance a 7 m cell holds a tree and which shapes.
  float tree_chance;
  TreeChoice trees[2];
  // The colours of the grass and of the foliage (leaves) here.
  TintColor grass, foliage;
  float boulder_chance;  // per 24 m cell
  bool canopy;           // a distant forest: LOD columns above the tree cells show leaves
};
const BiomeDef& BiomeOf(Biome b);

}  // namespace dwell::worldgen
