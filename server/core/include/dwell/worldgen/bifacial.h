#pragma once

#include <array>
#include <cstdint>

#include "dwell/core/voxel.h"
#include "dwell/worldgen/terrain.h"

// The bifacial world's terrain (docs/BIFACIAL_WORLD.md §5, ADR 0023): two face-local generators —
// face A is the terrain of TerrainGenerator(seed) as it always was, face B a second terrain from
// its own seed streams — and chunks of face B are face-local chunks flipped about the midplane.
// Still a pure function of (seed, chunk): any chunk, in any order, on any thread, natively or in
// WASM.
namespace dwell::worldgen {

// The seed of a face's generator. Face A keeps the world seed; face B's is derived from it, so its
// continents, rivers, climate and features differ from face A's.
std::uint64_t FaceSeed(std::uint64_t world_seed, core::Face face);

class BifacialTerrain {
 public:
  explicit BifacialTerrain(std::uint64_t world_seed);

  // A face's generator, in face-local coordinates (sea level 0, ground band from the midplane up).
  const TerrainGenerator& Local(core::Face face) const { return face == core::Face::kA ? a_ : b_; }

  // Generates a chunk of either face: face A's directly, face B's as the vertical flip of the
  // face-local chunk (core::FaceLocalChunk) with slabs and slopes turned over (MirrorMaterial).
  void Generate(const core::ChunkCoord& coord, core::Chunk& chunk,
                std::uint8_t stages = TerrainGenerator::kAllStages) const;
  // Exactly the generator's own all-air shortcut, per face.
  bool IsAirChunk(const core::ChunkCoord& coord) const;
  // The sky test's height for a chunk column of a face (face-local, TerrainGenerator::SkyFloorAt).
  float SkyFloorAt(std::int32_t cx, std::int32_t cz, core::Face face) const {
    return Local(face).SkyFloorAt(cx, cz);
  }
  // TerrainGenerator::IsAirChunk(coord, sky_floor) for a chunk in the world: maps face B's chunks
  // to their face-local ones, `sky_floor` being the face's SkyFloorAt of the column.
  static bool IsAirChunk(const core::ChunkCoord& coord, float sky_floor) {
    return TerrainGenerator::IsAirChunk(core::FaceLocalChunk(coord), sky_floor);
  }

  // Level of detail (ARCHITECTURE.md §6.6): a section of the world, its cell rows in world order.
  // Rows belong to the face of their centre: face A's come from its generator directly, face B's
  // from the mirror image of the section (its rows reversed) in face B's. `surface` receives face
  // A's column surfaces, `surface_b` face B's — in face-local terms (heights as on face A, row 0
  // at the mirror of the section's top) — each only where the section has rows of that face.
  core::LodKind GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                            core::LodSurfaces* surface = nullptr,
                            core::LodSurfaces* surface_b = nullptr) const;
  // The column bounds of both faces (core::LodBounds); sections are classified from them.
  core::LodBounds LodBoundsAt(int level, std::int32_t i, std::int32_t k) const;

  // Feet position players spawn at: always on face A.
  std::array<double, 3> SpawnPoint() const { return a_.SpawnPoint(); }

  // Point queries in world coordinates: face B's voxel (x, y, z) is the face-local voxel
  // (x, MirrorY(y), z) of face B's generator.
  bool SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const;

 private:
  TerrainGenerator a_, b_;
};

}  // namespace dwell::worldgen
