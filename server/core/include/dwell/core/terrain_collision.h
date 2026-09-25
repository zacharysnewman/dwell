#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Geometry/IndexedTriangle.h>
#include <Jolt/Math/Float3.h>
#include <Jolt/Physics/Body/BodyID.h>
#include <Jolt/Physics/Collision/Shape/MutableCompoundShape.h>

#include <array>
#include <cstdint>
#include <unordered_map>
#include <vector>

#include "dwell/core/physics_world.h"
#include "dwell/core/voxel.h"

// Static terrain collision (PLAYER_CONTROLLER.md §5): one static Jolt body whose
// MutableCompoundShape holds a MeshShape per chunk, built from the voxel grid on demand around
// players and rebuilt in the same tick as an edit. One body (rather than one per chunk) lets Jolt's
// enhanced internal edge removal see both sides of a chunk seam, so capsules slide across seams
// without ghost contacts.
namespace dwell::core {

struct ChunkMesh {
  JPH::VertexList vertices;  // chunk-local (0..32)
  JPH::IndexedTriangleList triangles;
};

// Collision triangles for one chunk: every exposed face of full cubes and slabs as its own quad on
// the voxel grid, pointing out of the solid. Faces are deliberately not merged (greedy meshing):
// merged faces create T-junctions, whose unshared edges produce ghost contacts.
ChunkMesh BuildChunkMesh(VoxelWorld& world, const ChunkCoord& coord);

class TerrainCollision {
 public:
  TerrainCollision(VoxelWorld& world, PhysicsWorld& physics) : world_(world), physics_(physics) {}
  ~TerrainCollision();

  TerrainCollision(const TerrainCollision&) = delete;
  TerrainCollision& operator=(const TerrainCollision&) = delete;

  // Makes sure every chunk overlapping the box has collision.
  void EnsureBox(JPH::Vec3 min, JPH::Vec3 max);

  // Rebuilds chunks whose voxels (or whose neighbours' voxels) changed since they were built. Call
  // after voxel edits, before the next query or physics step.
  void Sync();

  std::size_t built_chunks() const { return chunks_.size(); }
  JPH::BodyID body() const { return body_; }

 private:
  static constexpr JPH::uint kNoShape = ~0u;
  struct Built {
    JPH::uint sub_shape = kNoShape;          // index in the compound; kNoShape until it has faces
    std::array<std::uint32_t, 7> revisions;  // own + 6 neighbours
  };

  std::array<std::uint32_t, 7> Revisions(const ChunkCoord& coord);
  void Build(const ChunkCoord& coord, Built& built);

  VoxelWorld& world_;
  PhysicsWorld& physics_;
  std::unordered_map<ChunkCoord, Built, ChunkCoordHash> chunks_;
  JPH::Ref<JPH::MutableCompoundShape> compound_;
  JPH::BodyID body_;
};

}  // namespace dwell::core
