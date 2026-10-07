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

enum class TreeKind : std::uint8_t { kOak, kSpruce, kBlossom, kAutumn };

// A surface layer: the voxels with depth run < `until` below the surface (run 0 = the top voxel).
struct SurfaceLayer {
  core::MaterialId material;
  std::int8_t until;
};
struct TreeChoice {
  TreeKind kind;
  std::uint8_t weight;  // of 100 among the biome's choices
};

struct BiomeDef {
  Biome id;
  const char* name;
  // Surface (§3.6): the first layer whose `until` exceeds the depth; stone below the last. On steep
  // ground (neighbouring columns more than 3 m apart in height) `steep` replaces the layers for
  // depth < steep_until (0: no steep rule), stone below.
  SurfaceLayer layers[3];
  core::MaterialId steep;
  std::int8_t steep_until;
  // Vegetation (§3.7): the chance a 7 m cell holds a tree and which kinds; groves of accent trees
  // (the share of a grove's core that is accent-coloured, and the leaf materials they may be).
  float tree_chance;
  TreeChoice trees[2];
  float accent_share;
  std::uint8_t accent_count;
  core::MaterialId accents[5];
  float boulder_chance;  // per 24 m cell
  bool canopy;           // a distant forest: LOD columns above the tree cells show leaves
};
const BiomeDef& BiomeOf(Biome b);

// The default leaf material of a tree kind.
core::MaterialId DefaultLeaves(TreeKind kind);

// Grove noise (§3.7): groves of accent trees come in clumps a few hundred metres across. The
// share of trees that are accents at (x, z) and the accent leaf material of the patch there.
struct GroveSeeds {
  std::uint32_t grove, patch;
};
// 0 outside groves, up to 1 in a grove's core.
float GroveCore(const GroveSeeds& seeds, std::int64_t x, std::int64_t z);
// The leaf material of a tree of `kind` in `biome` at (x, z) whose own hash is `hash`: the kind's
// default, or the grove's accent colour with the chance GroveCore × the biome's accent share.
core::MaterialId LeafMaterialAt(const GroveSeeds& seeds, const BiomeDef& biome, TreeKind kind,
                                std::int64_t x, std::int64_t z, std::uint32_t hash);

}  // namespace dwell::worldgen
