#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Geometry/IndexedTriangle.h>
#include <Jolt/Math/Float3.h>
#include <Jolt/Physics/Body/BodyID.h>
#include <Jolt/Physics/Collision/CollisionGroup.h>
#include <Jolt/Physics/Collision/GroupFilter.h>
#include <Jolt/Physics/Collision/Shape/MutableCompoundShape.h>

#include <array>
#include <cstdint>
#include <functional>
#include <optional>
#include <unordered_map>
#include <vector>

#include "dwell/core/physics_world.h"
#include "dwell/core/voxel.h"

// Static terrain collision (PLAYER_CONTROLLER.md §5), built from the voxel grid on demand around
// players and rebuilt in the same tick as an edit. A MeshShape per chunk sits in a
// MutableCompoundShape of one static body, so Jolt's enhanced internal edge removal sees both sides
// of every chunk seam and capsules slide across seams without ghost contacts.
//
// The world is 8,192 km across (ADR 0011), so a body's sub-shape offsets must stay small for float
// to represent them exactly: bodies are anchored at *regions* (kRegionChunks³ chunks, 2 048 m),
// each at its region's centre. Dividing terrain between bodies by position would put a seam (and
// ghost contacts) at region borders, so instead a region's body holds every chunk around the
// players *anchored* to it, wherever those chunks lie; a chunk's mesh is shared by the bodies that
// hold it. Each player collides only with its anchor's body (a Jolt group filter), and changes
// anchor, with hysteresis, only well inside another region, where both bodies hold the ground under
// it. Other bodies (Tier 1, later) collide with every terrain body.
namespace dwell::core {

struct ChunkMesh {
  JPH::VertexList vertices;  // chunk-local (0..32)
  JPH::IndexedTriangleList triangles;
};

// Collision triangles for one chunk: every exposed face of full cubes and slabs as its own quad on
// the voxel grid, pointing out of the solid. Faces are deliberately not merged (greedy meshing):
// merged faces create T-junctions, whose unshared edges produce ghost contacts.
ChunkMesh BuildChunkMesh(VoxelWorld& world, const ChunkCoord& coord);

// Visible voxel faces of one chunk for rendering (the client builds its meshes from these):
// faces of non-air cells not hidden by a full-cube neighbour (water also hides against water,
// slab sides against slabs). Ladders emit one face, their facing side (the plate is drawn there).
struct RenderFace {
  std::uint8_t x, y, z;  // chunk-local cell
  std::uint8_t face;     // 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z
  MaterialId material;
  std::uint16_t reserved = 0;
};
static_assert(sizeof(RenderFace) == 8);
std::vector<RenderFace> BuildRenderFaces(VoxelWorld& world, const ChunkCoord& coord);

class TerrainCollision {
 public:
  TerrainCollision(VoxelWorld& world, PhysicsWorld& physics);
  ~TerrainCollision();

  TerrainCollision(const TerrainCollision&) = delete;
  TerrainCollision& operator=(const TerrainCollision&) = delete;

  static constexpr int kRegionChunks = 64;
  // A player keeps its anchor while its chunk is within this many chunks outside the anchor's
  // region, so walking along a region border does not flip anchors.
  static constexpr int kAnchorHysteresisChunks = 8;
  using Anchor = ChunkCoord;  // a region
  // Region of a chunk (per axis), and the world position of a region's centre (its body).
  static Anchor RegionOf(const ChunkCoord& chunk);
  static JPH::RVec3 RegionCenter(const Anchor& region);
  // The anchor for a body at `position` that is currently anchored at `current` (if any).
  static Anchor AnchorFor(JPH::RVec3 position, const std::optional<Anchor>& current);

  // Makes sure every chunk overlapping the box has collision in `anchor`'s body.
  void EnsureBox(JPH::RVec3 min, JPH::RVec3 max, const Anchor& anchor);
  // The collision group a character anchored at `anchor` carries: it collides with that anchor's
  // terrain body only (and with every non-terrain body).
  JPH::CollisionGroup CharacterGroup(const Anchor& anchor);

  // Rebuilds chunks whose voxels (or whose neighbours' voxels) changed since they were built. Call
  // after voxel edits, before the next query or physics step.
  void Sync();

  // Unloads, per anchor, collision of chunks that `keep(anchor, chunk)` rejects (players moved
  // away); compound slots are reused by later chunks and unused bodies are destroyed, so memory
  // stays bounded while moving.
  void Retain(const std::function<bool(const Anchor&, const ChunkCoord&)>& keep);

  std::size_t built_chunks() const { return chunks_.size(); }
  std::size_t regions() const { return regions_.size(); }

 private:
  static constexpr JPH::uint kNoShape = ~0u;
  static constexpr std::uint32_t kMissing = ~0u;  // revision of a not-yet-streamed chunk
  struct Built {
    JPH::RefConst<JPH::Shape> shape;         // null: no faces
    std::array<std::uint32_t, 7> revisions;  // own + 6 neighbours
  };
  struct Region {
    JPH::uint group = 0;  // CollisionGroup id
    JPH::Ref<JPH::MutableCompoundShape> compound;
    JPH::BodyID body;
    std::unordered_map<ChunkCoord, JPH::uint, ChunkCoordHash> members;  // chunk → sub-shape
    std::vector<JPH::uint> free_sub_shapes;  // EmptyShape slots of unloaded chunks
  };
  class Groups;

  std::array<std::uint32_t, 7> Revisions(const ChunkCoord& coord);
  void Build(const ChunkCoord& coord, Built& built);
  Region& RegionAt(const Anchor& anchor);
  // Puts the chunk's current shape into the region (or an EmptyShape when it has none).
  void Place(Region& region, const Anchor& anchor, const ChunkCoord& coord, const Built& built);
  void DestroyBody(Region& region);

  VoxelWorld& world_;
  PhysicsWorld& physics_;
  JPH::Ref<JPH::GroupFilter> groups_;
  std::unordered_map<ChunkCoord, Built, ChunkCoordHash> chunks_;
  std::unordered_map<Anchor, Region, ChunkCoordHash> regions_;
  JPH::uint next_group_ = 1;
};

}  // namespace dwell::core
