#pragma once

#include <array>
#include <cstdint>
#include <optional>

#include "dwell/core/voxel.h"

// Procedural terrain, generator version 2 (ARCHITECTURE.md §6.3). A chunk is a pure function of
// (world seed, chunk coordinate): every stage reads only noise and hashes of world coordinates,
// never another chunk's data, so chunks generate in any order, on any thread, natively or in WASM,
// with bit-identical results (noise.h, ADR 0010).
//
// Pipeline per chunk:
//   1. Climate (2D): continentalness, erosion, temperature, humidity → biome weights.
//   2. Base height (2D): continentalness spline + biome-blended hills + ridged mountains.
//   3. Density (3D): (height − y) + overhang noise; solid where positive.
//   4. Caves (3D): spaghetti tunnels and cheese caverns, faded out near the surface and bedrock.
//   5. Surface and strata: grass/dirt, sand/sandstone, snow, gravel by biome and depth; water fills
//      open space below the sea level; bedrock at the bottom.
//   6. Stability: small solid components floating inside the chunk are removed.
//   7. Ores: hashed vein blobs in stone.
//   8. Features: trees and boulders at hashed positions per region cell.
// 2D fields are sampled every 4 columns and 3D noise every 4 voxels, then interpolated.
namespace dwell::worldgen {

enum class Biome : std::uint8_t { kOcean, kBeach, kPlains, kForest, kDesert, kSnowy, kMountains };
const char* BiomeName(Biome b);

// 2D fields of one column.
struct Column {
  float continentalness = 0;  // < 0 ocean, > 0 land
  float erosion = 0;          // low erosion → mountains
  float temperature = 0;
  float humidity = 0;
  float mountain = 0;  // 0..1 weight of the mountain height term
  float height = 0;    // base terrain height (m), before overhang noise
  float overhang = 0;  // amplitude (m) of the 3D overhang noise
  Biome biome = Biome::kPlains;
};

// A tree or boulder; positions are world voxel coordinates.
struct Feature {
  enum class Kind : std::uint8_t { kOak, kSpruce, kBoulder } kind = Kind::kOak;
  std::int32_t x = 0, y = 0, z = 0;  // trunk base / boulder centre (first voxel above the ground)
  int size = 0;                      // trunk height, or boulder radius
  std::uint32_t hash = 0;            // per-feature randomness (leaf trimming)
};

class TerrainGenerator {
 public:
  explicit TerrainGenerator(std::uint64_t world_seed);

  // Stages after the terrain itself (1–5), for tests and debug tools; worlds use all of them.
  enum Stages : std::uint8_t {
    kStageStability = 1,
    kStageOres = 2,
    kStageFeatures = 4,
    kAllStages = 7
  };
  void Generate(const core::ChunkCoord& coord, core::Chunk& chunk,
                std::uint8_t stages = kAllStages) const;

  // Point queries with exactly the chunk path's arithmetic (used by features, spawn, and tests).
  Column ColumnAt(std::int32_t x, std::int32_t z) const;
  // Terrain solidity after caves, before the stability pass and features.
  bool SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const;
  // Top voxel of the ground near the base height, if the surface is there (open air above it).
  std::optional<std::int32_t> GroundY(std::int32_t x, std::int32_t z) const;

  // The feature rooted in a region cell, if any (cells are kTreeCell / kBoulderCell wide).
  std::optional<Feature> TreeInCell(std::int32_t cx, std::int32_t cz) const;
  std::optional<Feature> BoulderInCell(std::int32_t cx, std::int32_t cz) const;

  // Feet position for players: near the origin, on land, on level ground with no tree nearby.
  std::array<float, 3> SpawnPoint() const;

  static constexpr int kTreeCell = 7;
  static constexpr int kBoulderCell = 24;

 private:
  struct Seeds {
    std::uint32_t continent, erosion, temperature, humidity, hills, ridges, overhang, spaghetti_a,
        spaghetti_b, cheese, trees, boulders, ores;
  } seeds_;

  struct Corner2 {
    float continentalness, erosion, temperature, humidity, hills, ridges;
  };
  struct Corner3 {
    float overhang, spaghetti_a, spaghetti_b, cheese;
  };
  Corner2 SampleCorner2(std::int32_t lx, std::int32_t lz) const;
  Corner3 SampleCorner3(std::int32_t lx, std::int32_t ly, std::int32_t lz) const;
  static Column Finish(const Corner2& c);
  static Column Interp2(const Corner2 (&c)[4], int fx, int fz);
  // 3D noise interpolation: bilinear in (x, z) within a lattice layer, then linear in y.
  static Corner3 Bilerp(const Corner3 (&c)[4], int fx, int fz);
  static Corner3 LerpY(const Corner3& a, const Corner3& b, int fy);
  static bool Solid(const Column& col, const Corner3& n, std::int32_t y);
  template <class Write>
  void PlaceFeature(const Feature& f, Write&& write) const;
  friend struct ChunkBuilder;
};

}  // namespace dwell::worldgen
