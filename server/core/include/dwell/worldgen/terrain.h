#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/continents.h"
#include "dwell/worldgen/rivers.h"
#include "dwell/worldgen/slopes.h"

// Procedural terrain, generator version 8 (ARCHITECTURE.md §6.3). A chunk is a pure function of
// (world seed, chunk coordinate): every stage reads only noise and hashes of world coordinates,
// never another chunk's data, so chunks generate in any order, on any thread, natively or in WASM,
// with bit-identical results (noise.h, ADR 0010).
//
// Pipeline per chunk:
//   1. Climate (2D): the signed distance to the coast (continents.h), erosion, temperature,
//      humidity → biome weights.
//   2. Base height (2D): shelf / slope / abyss at sea and the inland rise on land, plus
//   biome-blended
//      hills and ridged mountains.
//   3. Density (3D): (height − y) + overhang noise; solid where positive.
//   4. Caves (3D): spaghetti tunnels and cheese caverns, faded out near the surface and bedrock.
//   5. Surface and strata: grass/dirt, sand/sandstone, snow, gravel by biome and depth; water fills
//      open space below the sea level; bedrock at the bottom.
//   5b. Slopes (versions 5–6, SLOPE_BLOCKS.md §5): each column's surface cell becomes a slope or
//      slab piece from the continuous surface's heights at its corners, shared by neighbouring
//      cells, on solid cells (one piece per column, steep ground clamped to a cell; fixed in
//      0.3.1 / 0.4.1).
//   6. Stability: small solid components floating inside the chunk are removed.
//   7. Ores: hashed vein blobs in stone.
//   8. Features: trees and boulders at hashed positions per region cell.
// 2D fields are sampled every 4 columns and 3D noise every 4 voxels, then interpolated.
//
// Version 3 (ADR 0011): the 8,192 km disc. Noise splits world coordinates into integer lattice
// cells and float offsets (noise.h), so terrain is equally detailed everywhere; a placeholder
// planet-scale layer varies land, ocean and kilometre-scale relief across the disc; sea level is
// 0; nothing is generated outside the disc. The terrain's content (biomes, materials, features) is
// prototype (ARCHITECTURE.md §6.1).
// Version 8 (ADR 0019): climate at continental scale — temperature and humidity noise of
// ~1,200 km and ~600 km, a lapse rate (snow lies on high ground), and the mountains biome from relief
// above the valley floor, so biomes come in regions rather than patches a few hundred metres across.
// Version 7 (ADR 0018, WORLD_GENERATION.md §3, Phase 11a): drainage-consistent terrain — three
// tiers of rivers as noise contours, lakes, and static water above sea level at a terraced surface
// (waterfall steps); the land is a valley floor V with the relief standing away from the channels.
// Version 6 (ADR 0017, WORLD_GENERATION.md §2): land and sea come from a plate layout of 11–14
// continents separated by open ocean, not from noise; continentalness is a signed distance to the
// coast, from which the shelf, slope, abyss and inland rise follow.
namespace dwell::worldgen {

enum class Biome : std::uint8_t { kOcean, kBeach, kPlains, kForest, kDesert, kSnowy, kMountains };
const char* BiomeName(Biome b);

// 2D fields of one column.
struct Column {
  float continentalness = 0;  // < 0 ocean, > 0 land: −1..1 from the coast distance
  float coast = 0;            // signed distance to the coast (m): > 0 land, < 0 sea
  float plate_edge = 0;       // land: distance (m) to the nearest internal plate edge
  float convergence = 0;  // −1..1 at that edge (Phase 11: mountain belts along convergent ones)
  std::int32_t continent = continents::kNoContinent;  // id (ContinentLayout::IdOf), or kIslandId
  float erosion = 0;                                  // low erosion → mountains
  float temperature = 0;
  float humidity = 0;
  float mountain = 0;  // 0..1 weight of the mountain height term
  float height = 0;    // base terrain height (m), before overhang noise
  float overhang = 0;  // amplitude (m) of the 3D overhang noise
  float valley = 0;    // the valley floor V (m): the land's lowest ground, the rivers' reference
  // Water: open voxels below `water` (y < water) are water. Sea level, or the surface of the river
  // or lake in this column (a terrace of the valley floor).
  std::int32_t water = 0;
  float wet = 0;      // 0..1: how much a river channel (banks included) or a lake claims the column
  bool lake = false;  // in a lake's bowl (inside its shore)
  Biome biome = Biome::kPlains;
  bool outside = false;  // beyond the world's disc: nothing is generated
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
    kStageSlopes = 8,
    kAllStages = 15
  };
  void Generate(const core::ChunkCoord& coord, core::Chunk& chunk,
                std::uint8_t stages = kAllStages) const;

  // True when Generate would leave the chunk all air: outside the world's rows or disc, or sky
  // above everything the chunk's columns (and features reaching into it) can hold. Exactly the
  // generator's own shortcut, so a chunk this reports as air is never generated or sent.
  bool IsAirChunk(const core::ChunkCoord& coord) const;
  // The sky test's height for a chunk column: chunks with y0 above it (and at or above sea level)
  // are air. Depends only on (cx, cz), so callers can cache it per column.
  float SkyFloorAt(std::int32_t cx, std::int32_t cz) const;
  // IsAirChunk given SkyFloorAt(coord.x, coord.z).
  static bool IsAirChunk(const core::ChunkCoord& coord, float sky_floor);

  // Point queries with exactly the chunk path's arithmetic (used by features, spawn, and tests).
  Column ColumnAt(std::int32_t x, std::int32_t z) const;
  // The landmass layout alone (no terrain): the coast distance of ColumnAt, evaluated at the point
  // itself rather than on the 4 m lattice, with the lattice-free fine octaves. For statistics, the
  // inspect tool and the separation tests.
  struct LandSample {
    float coast = 0;  // signed distance to the coast (m): > 0 land
    std::int32_t continent = continents::kNoContinent;
    float plate_edge = 0, convergence = 0;
  };
  LandSample LandAt(std::int32_t x, std::int32_t z) const;
  const ContinentLayout& Continents() const { return continents_; }
  // The three river tiers' noise (rivers.h) at the point itself, for the inspect tool and tests:
  // a river runs where a tier's value is zero.
  rivers::Corner RiversAt(std::int32_t x, std::int32_t z) const;
  // Terrain solidity after caves, before the stability pass and features.
  bool SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const;
  // Top voxel of the ground near the base height, if the surface is there (open air above it).
  // The cube terrain's ground: slopes (stage 5b) lower or raise it by less than a cell.
  std::optional<std::int32_t> GroundY(std::int32_t x, std::int32_t z) const;

  // The continuous surface of a column (SLOPE_BLOCKS.md §5): the height (m) where the terrain's
  // density crosses zero going down from the sky, if the column has a clean surface there (solid
  // below it, no cave or overhang pocket directly under it). The cube terrain's top voxel is
  // ceil(height) − 1.
  struct SurfaceColumn {
    bool valid = false;
    float height = 0.0f;
  };
  SurfaceColumn SurfaceAt(std::int32_t x, std::int32_t z) const;
  // What the slope rule makes of cell (x, y, z), from point queries alone — exactly the chunk
  // path's decision: the piece (air above the column's surface cell, a slope, slab, cube or air in
  // it, a full cube below it), or nothing when the column stays as the cube terrain has it (no
  // clean surface around it). The cell just under the surface cell is always solid; deeper cells
  // keep the cube terrain's caves.
  std::optional<slopes::Piece> SlopePieceAt(std::int32_t x, std::int32_t y, std::int32_t z) const;

  // The feature rooted in a region cell, if any (cells are kTreeCell / kBoulderCell wide).
  std::optional<Feature> TreeInCell(std::int32_t cx, std::int32_t cz) const;
  std::optional<Feature> BoulderInCell(std::int32_t cx, std::int32_t cz) const;

  // Feet position for players: near the origin, on land, on level ground with no tree nearby.
  std::array<double, 3> SpawnPoint() const;

  // Level of detail (ARCHITECTURE.md §6.6): a section evaluated at its cells' resolution. Each
  // cell samples the pipeline at its centre column and bottom voxel (the voxel whose solidity
  // decides a floor under Downsample), with noise octaves whose lattice is finer than a cell
  // dropped and features only where they are at least a cell wide (trees up to 4 m cells,
  // boulders up to 2 m). Caves carve only within three cells of the surface (deeper cave air is
  // never seen from afar); ores and the stability pass are below a cell. The apron below the
  // world reads as bedrock, so the world's floor is not drawn.
  // With `surface`, also each column's exact surface (core::LodSurface).
  core::LodKind GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                            core::LodSurfaces* surface = nullptr) const;
  // The column bounds GenerateLod classifies sections by (core::LodKindFromBounds).
  core::LodBounds LodBoundsAt(int level, std::int32_t i, std::int32_t k) const;

  static constexpr int kTreeCell = 7;
  static constexpr int kBoulderCell = 24;

 private:
  struct Seeds {
    std::uint32_t continent, erosion, temperature, humidity, hills, ridges, overhang, spaghetti_a,
        spaghetti_b, cheese, trees, boulders, ores, macro, relief;
  } seeds_;
  ContinentLayout continents_;
  rivers::Seeds river_seeds_;
  // Lakes' surfaces from their centres (rivers.h); defined with the terrain.
  struct LakeOracle;

  struct Corner2 {
    float continentalness, erosion, temperature, humidity, hills, ridges, macro, relief;
    // The continent layout (continents.h) at this point.
    float coast, plate_edge, convergence, elevation, shelf;
    std::int32_t continent;
    // Rivers and lakes (rivers.h).
    rivers::Corner water;
  };

  struct Corner3 {
    float overhang, spaghetti_a, spaghetti_b, cheese;
  };
  Corner2 SampleCorner2(std::int32_t lx, std::int32_t lz) const;
  // Everything but the rivers, at a point (a lattice corner, or a lake's centre).
  Corner2 SampleBase(std::int64_t x, std::int64_t z) const;
  // Continental temperature and humidity noise (kept: local octaves kept, −1 all).
  float Temperature(std::int64_t x, std::int64_t z, int kept) const;
  float Humidity(std::int64_t x, std::int64_t z, int kept) const;
  // Columns of a chunk and one beyond each side ((S + 2)², row-major from (x0 − 1, z0 − 1)).
  void ChunkColumns(std::int32_t x0, std::int32_t z0, std::vector<Column>& cols) const;
  static float SkyFloor(const std::vector<Column>& cols);
  Corner3 SampleCorner3(std::int32_t lx, std::int32_t ly, std::int32_t lz) const;
  Column Finish(const Corner2& c) const;
  // The continuous surface of one column from its fields and its 3D noise at lattice layers
  // (layer(j) = the column's noise at y = 4j).
  template <class Layer>
  static SurfaceColumn SurfaceOf(const Column& col, Layer&& layer);
  // Surfaces of the columns −1..S around a chunk ((S + 2)², row-major from (x0 − 1, z0 − 1)).
  void ChunkSurfaces(const std::vector<Column>& cols, std::int32_t x0, std::int32_t y0,
                     std::int32_t z0, std::vector<SurfaceColumn>& out) const;
  // A level spawn: the feet height standing on the 5 × 5 patch around (x, z), if there is one.
  bool LevelSpawnY(std::int32_t x, std::int32_t z, std::int32_t& feet) const;
  // Step 5b of the chunk path: shapes the surface cells.
  void ShapeSurface(const std::vector<Column>& cols, const std::vector<SurfaceColumn>& surfaces,
                    std::int32_t y0,
                    std::array<core::MaterialId, core::kChunkVolume>& voxels) const;
  // Level of detail: a column's fields and a point's 3D noise at a cell size (octaves dropped).
  // `layout`: the continent layout at this column when the caller has it (LodLayout), else
  // computed.
  Column ColumnLod(std::int64_t x, std::int64_t z, std::int64_t cell,
                   const MacroCorner* layout = nullptr) const;
  // The continent layout at the cell centres of a LOD section's columns first..first+count−1 along
  // each axis (row-major, z major), for cells of the macro lattice's spacing and wider: evaluated
  // at anchors every kLodLayoutStride columns, with the columns between interpolated where the four
  // anchors around them lie in the interior of one continent or in the deep sea, and evaluated
  // exactly everywhere else (coasts, shelves, channels, islands).
  void LodLayout(std::int64_t origin_x, std::int64_t origin_z, std::int64_t cell, int first,
                 int count, std::vector<MacroCorner>& out) const;
  Corner3 NoiseLod(std::int64_t x, std::int64_t y, std::int64_t z, std::int64_t cell) const;
  Column Interp2(const Corner2 (&c)[4], int fx, int fz) const;
  // 3D noise interpolation: bilinear in (x, z) within a lattice layer, then linear in y.
  static Corner3 Bilerp(const Corner3 (&c)[4], int fx, int fz);
  static Corner3 LerpY(const Corner3& a, const Corner3& b, int fy);
  static bool Solid(const Column& col, const Corner3& n, std::int32_t y);
  template <class Write>
  void PlaceFeature(const Feature& f, Write&& write) const;
  friend struct ChunkBuilder;
};

}  // namespace dwell::worldgen
