#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/island.h"

// Procedural terrain, generator version 12 (ARCHITECTURE.md §6.3): the interim baseline generator
// of Phase 15 — one island at the origin, a 2D height field, and the ground filled by depth. A
// chunk is a pure function of (world seed, chunk coordinate): every value comes from noise and
// hashes of world coordinates, never another chunk's data, so chunks generate in any order, on any
// thread, natively or in WASM, with bit-identical results (noise.h, ADR 0010).
//
// Pipeline per chunk:
//   1. Layout (2D): the signed distance to the island's coast (island.h).
//   2. Height (2D): the sea floor falling from the coast, or the land rising inland, plus hills.
//   3. Fill: stone from the face's bottom up to the surface, under a skin of grass over dirt on
//      land, sand at the shore and under water; water fills open space below sea level.
// 2D fields are sampled every 4 columns and interpolated. No 3D density, caves, rivers, biomes or
// features (the baseline, ADR 0025). The parameters are code constants: Phase 17 replaces this
// generator with an evaluator of terrain definitions (data, ADR 0026); the content is prototype
// (ARCHITECTURE.md §6.1).
namespace dwell::worldgen {

// 2D fields of one column.
struct Column {
  float coast = 0;   // signed distance to the coast (m): > 0 land, < 0 sea
  float height = 0;  // ground height (m, face-local, sea level 0): the top voxel is ceil(height) − 1
  bool outside = false;  // beyond the world's disc: nothing is generated
};

class TerrainGenerator {
 public:
  explicit TerrainGenerator(std::uint64_t world_seed);

  void Generate(const core::ChunkCoord& coord, core::Chunk& chunk) const;

  // True when Generate would leave the chunk all air: outside the world's rows or disc, or sky
  // above everything the chunk's columns hold. Exactly the generator's own shortcut, so a chunk
  // this reports as air is never generated or sent.
  bool IsAirChunk(const core::ChunkCoord& coord) const;
  // The sky test's height for a chunk column: chunks with y0 above it (and at or above sea level)
  // are air. Depends only on (cx, cz), so callers can cache it per column.
  float SkyFloorAt(std::int32_t cx, std::int32_t cz) const;
  // IsAirChunk given SkyFloorAt(coord.x, coord.z).
  static bool IsAirChunk(const core::ChunkCoord& coord, float sky_floor);

  // Point queries with exactly the chunk path's arithmetic (spawn, tools and tests).
  Column ColumnAt(std::int32_t x, std::int32_t z) const;
  bool SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const;
  // The top solid voxel of a column, if it is inside the disc.
  std::optional<std::int32_t> GroundY(std::int32_t x, std::int32_t z) const;
  // The island's coast distance at the point itself (no lattice), for tools and tests.
  float CoastAt(std::int32_t x, std::int32_t z) const { return island_.CoastAt(x, z); }

  // Feet position for players: the level dry ground nearest the origin (a spiral search).
  std::array<double, 3> SpawnPoint() const;

  // Level of detail (ARCHITECTURE.md §6.6): a section evaluated at its cells' resolution. Each
  // cell samples the height field at its centre column, with noise octaves whose lattice is finer
  // than a cell dropped, and is solid when its bottom voxel is. The apron below the face reads as
  // stone, so the face's floor is not drawn. With `surface`, also each column's exact surface
  // (core::LodSurface).
  core::LodKind GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                            core::LodSurfaces* surface = nullptr) const;
  // The same for the face-local section whose cell rows start at face-local height `origin_y` (x, z
  // and the level from `c`): a face's generator works in its own frame, so the bifacial world asks
  // face B's for the mirror image of a section (bifacial.h). The kind is this face's alone.
  // `bounds_out` receives the section's column bounds (lo, hi: this face's).
  core::LodKind GenerateLodAt(const core::LodCoord& c, std::int64_t origin_y, core::LodCells& cells,
                              core::LodSurfaces* surface = nullptr,
                              core::LodBounds* bounds_out = nullptr) const;
  // The column bounds GenerateLod classifies sections by (core::LodKindFromBounds).
  core::LodBounds LodBoundsAt(int level, std::int32_t i, std::int32_t k) const;

 private:
  struct Seeds {
    std::uint32_t hills, bumps;
  } seeds_;
  // The height noise's lattice offset (noise.h NoiseOffset).
  std::int64_t offset_x_ = 0, offset_z_ = 0;
  IslandLayout island_;

  // The noise fields of a point, before the height is derived from them.
  struct Corner {
    float coast, hills, bumps;
  };
  // At a lattice corner (exact), and at a level-of-detail cell's centre (octaves finer than the
  // cell dropped).
  Corner SampleCorner(std::int32_t lx, std::int32_t lz) const;
  Corner SampleLod(std::int64_t x, std::int64_t z, std::int64_t cell) const;
  static Column Finish(const Corner& c);
  static Column Interp(const Corner (&c)[4], int fx, int fz);
  // Columns of a chunk ((S × S), row-major from (x0, z0)).
  void ChunkColumns(std::int32_t x0, std::int32_t z0, std::vector<Column>& cols) const;
  static float SkyFloor(const std::vector<Column>& cols);
  Column ColumnLod(std::int64_t x, std::int64_t z, std::int64_t cell) const;
};

}  // namespace dwell::worldgen
