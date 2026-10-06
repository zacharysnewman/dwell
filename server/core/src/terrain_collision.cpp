#include "dwell/core/terrain_collision.h"

#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/EmptyShape.h>
#include <Jolt/Physics/Collision/Shape/MeshShape.h>

#include <algorithm>
#include <cmath>

#include "dwell/core/block_shape.h"

namespace dwell::core {
namespace {

// Adds a convex polygon (cell coordinates, outward-wound) at `offset`, as a triangle fan.
void AddPolygon(ChunkMesh& mesh, const ShapeFace& face, const float (&offset)[3]) {
  const auto base = static_cast<JPH::uint32>(mesh.vertices.size());
  for (int i = 0; i < face.count; ++i) {
    mesh.vertices.push_back(
        JPH::Float3(face.v[i][0] + offset[0], face.v[i][1] + offset[1], face.v[i][2] + offset[2]));
  }
  // A negative cell face's quad is listed (0, 3, 2, 1): its triangles go (0, 2, 1) then (0, 3, 2).
  if (face.count == 4 && face.tag < kSurfaceTag && (face.tag & 1)) {
    mesh.triangles.emplace_back(base, base + 2, base + 3);
    mesh.triangles.emplace_back(base, base + 1, base + 2);
    return;
  }
  for (int i = 1; i + 1 < face.count; ++i) {
    mesh.triangles.emplace_back(base, base + i, base + i + 1);
  }
}

// Offsets of the neighbour across each cell face (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z).
constexpr int kFaceStep[6][3] = {{1, 0, 0},  {-1, 0, 0}, {0, 1, 0},
                                 {0, -1, 0}, {0, 0, 1},  {0, 0, -1}};

}  // namespace

ChunkMesh BuildChunkMesh(VoxelWorld& world, const ChunkCoord& coord) {
  ChunkMesh mesh;
  const Chunk& chunk = world.Read(coord);
  bool any = false;
  for (MaterialId m : chunk.voxels()) {
    if (m != Materials::kAir) {
      any = true;
      break;
    }
  }
  if (!any) return mesh;

  const int ox = coord.x * kChunkSize, oy = coord.y * kChunkSize, oz = coord.z * kChunkSize;
  // Material at chunk-local coordinates that may lie one cell outside the chunk.
  auto at = [&](int lx, int ly, int lz) -> MaterialId {
    if (lx >= 0 && lx < kChunkSize && ly >= 0 && ly < kChunkSize && lz >= 0 && lz < kChunkSize) {
      return chunk.Get(lx, ly, lz);
    }
    return world.GetVoxel(ox + lx, oy + ly, oz + lz);
  };

  // The exposed surface of every solid cell: each polygon of its shape (block_shape.h), except the
  // parts of its cell faces that the neighbour's opposite face completely covers. Sloped and
  // partial-height surfaces are never culled.
  for (int lz = 0; lz < kChunkSize; ++lz) {
    for (int ly = 0; ly < kChunkSize; ++ly) {
      for (int lx = 0; lx < kChunkSize; ++lx) {
        const ShapeInfo& shape = ShapeOf(chunk.Get(lx, ly, lz));
        if (shape.face_count == 0) continue;
        const float cell[3] = {static_cast<float>(lx), static_cast<float>(ly),
                               static_cast<float>(lz)};
        for (const ShapeFace& face : FacesOf(shape)) {
          if (face.tag != kSurfaceTag) {
            const int* step = kFaceStep[face.tag];
            if (FaceCovered(shape, face.tag,
                            ShapeOf(at(lx + step[0], ly + step[1], lz + step[2])))) {
              continue;
            }
          }
          AddPolygon(mesh, face, cell);
        }
      }
    }
  }
  return mesh;
}

// Terrain bodies are sub-group kTerrain of their region's group; characters sub-group kCharacter of
// their anchor's. A terrain body and a character collide only within the same group; every other
// pair (Tier 1 bodies carry no group) collides as usual.
class TerrainCollision::Groups final : public JPH::GroupFilter {
 public:
  static constexpr JPH::CollisionGroup::SubGroupID kTerrain = 0;
  static constexpr JPH::CollisionGroup::SubGroupID kCharacter = 1;

  bool CanCollide(const JPH::CollisionGroup& a, const JPH::CollisionGroup& b) const override {
    const bool terrain_character =
        (a.GetSubGroupID() == kTerrain && b.GetSubGroupID() == kCharacter) ||
        (a.GetSubGroupID() == kCharacter && b.GetSubGroupID() == kTerrain);
    return !terrain_character || a.GetGroupID() == b.GetGroupID();
  }
};

TerrainCollision::TerrainCollision(VoxelWorld& world, PhysicsWorld& physics)
    : world_(world), physics_(physics), groups_(new Groups) {}

TerrainCollision::~TerrainCollision() {
  for (auto& [key, region] : regions_) DestroyBody(region);
}

void TerrainCollision::DestroyBody(Region& region) {
  if (region.body.IsInvalid()) return;
  physics_.bodies().RemoveBody(region.body);
  physics_.bodies().DestroyBody(region.body);
  region.body = {};
}

TerrainCollision::Anchor TerrainCollision::RegionOf(const ChunkCoord& c) {
  constexpr int kHalf = kRegionChunks / 2;
  const auto axis = [](std::int32_t v) {
    const std::int32_t shifted = v + kHalf;
    return shifted / kRegionChunks - (shifted % kRegionChunks < 0 ? 1 : 0);
  };
  return {axis(c.x), axis(c.y), axis(c.z)};
}

JPH::RVec3 TerrainCollision::RegionCenter(const Anchor& r) {
  constexpr double kSpan = static_cast<double>(kRegionChunks) * kChunkSize;
  return JPH::RVec3(r.x * kSpan, r.y * kSpan, r.z * kSpan);
}

TerrainCollision::Anchor TerrainCollision::AnchorFor(JPH::RVec3 p,
                                                     const std::optional<Anchor>& current) {
  const ChunkCoord chunk = ChunkOf(static_cast<std::int32_t>(std::floor(p.GetX())),
                                   static_cast<std::int32_t>(std::floor(p.GetY())),
                                   static_cast<std::int32_t>(std::floor(p.GetZ())));
  if (current) {
    constexpr int kReach = kRegionChunks / 2 + kAnchorHysteresisChunks;
    const auto within = [&](std::int32_t c, std::int32_t r) {
      const std::int32_t d = c - r * kRegionChunks;
      return d >= -kReach && d < kReach;
    };
    if (within(chunk.x, current->x) && within(chunk.y, current->y) && within(chunk.z, current->z)) {
      return *current;
    }
  }
  return RegionOf(chunk);
}

JPH::CollisionGroup TerrainCollision::CharacterGroup(const Anchor& anchor) {
  return JPH::CollisionGroup(groups_, RegionAt(anchor).group, Groups::kCharacter);
}

TerrainCollision::Region& TerrainCollision::RegionAt(const Anchor& anchor) {
  auto [it, inserted] = regions_.try_emplace(anchor);
  if (inserted) it->second.group = next_group_++;
  return it->second;
}

void TerrainCollision::EnsureBox(JPH::RVec3 min, JPH::RVec3 max, const Anchor& anchor) {
  const auto lo = ChunkOf(static_cast<std::int32_t>(std::floor(min.GetX())),
                          static_cast<std::int32_t>(std::floor(min.GetY())),
                          static_cast<std::int32_t>(std::floor(min.GetZ())));
  const auto hi = ChunkOf(static_cast<std::int32_t>(std::floor(max.GetX())),
                          static_cast<std::int32_t>(std::floor(max.GetY())),
                          static_cast<std::int32_t>(std::floor(max.GetZ())));
  Region& region = RegionAt(anchor);
  for (int z = lo.z; z <= hi.z; ++z) {
    for (int y = lo.y; y <= hi.y; ++y) {
      for (int x = lo.x; x <= hi.x; ++x) {
        const ChunkCoord coord{x, y, z};
        auto [it, inserted] = chunks_.try_emplace(coord);
        if (inserted) Build(coord, it->second);
        if (!region.members.count(coord)) Place(region, anchor, coord, it->second);
      }
    }
  }
}

void TerrainCollision::Retain(const std::function<bool(const Anchor&, const ChunkCoord&)>& keep) {
  for (auto region_it = regions_.begin(); region_it != regions_.end();) {
    Region& region = region_it->second;
    const Anchor& anchor = region_it->first;
    bool changed = false;
    const JPH::Vec3 previous_com =
        region.compound ? region.compound->GetCenterOfMass() : JPH::Vec3::sZero();
    for (auto it = region.members.begin(); it != region.members.end();) {
      if (keep(anchor, it->first)) {
        ++it;
        continue;
      }
      if (it->second != kNoShape) {
        region.compound->ModifyShape(it->second, JPH::Vec3::sZero(), JPH::Quat::sIdentity(),
                                     new JPH::EmptyShape);
        region.free_sub_shapes.push_back(it->second);
        changed = true;
      }
      it = region.members.erase(it);
    }
    if (region.members.empty()) {
      DestroyBody(region);
      region_it = regions_.erase(region_it);
      continue;
    }
    if (changed) {
      physics_.bodies().NotifyShapeChanged(region.body, previous_com,
                                           /*updateMassProperties=*/false,
                                           JPH::EActivation::DontActivate);
    }
    ++region_it;
  }
  // Chunks no region holds any more.
  std::erase_if(chunks_, [&](const auto& kv) {
    return std::none_of(regions_.begin(), regions_.end(),
                        [&](const auto& r) { return r.second.members.count(kv.first) != 0; });
  });
}

void TerrainCollision::Sync() {
  for (auto& [coord, built] : chunks_) {
    if (Revisions(coord) == built.revisions) continue;
    Build(coord, built);
    for (auto& [anchor, region] : regions_) {
      if (region.members.count(coord)) Place(region, anchor, coord, built);
    }
  }
}

std::array<std::uint32_t, 7> TerrainCollision::Revisions(const ChunkCoord& c) {
  static constexpr int kOffsets[7][3] = {{0, 0, 0},  {1, 0, 0}, {-1, 0, 0}, {0, 1, 0},
                                         {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
  std::array<std::uint32_t, 7> r{};
  for (int i = 0; i < 7; ++i) {
    const Chunk* chunk =
        world_.Find({c.x + kOffsets[i][0], c.y + kOffsets[i][1], c.z + kOffsets[i][2]});
    // A missing chunk of a streamed world may still arrive; a generated world's missing chunk is
    // an evicted unmodified one (revision 0).
    r[i] = chunk ? chunk->revision() : world_.streamed() ? kMissing : 0;
  }
  return r;
}

void TerrainCollision::Build(const ChunkCoord& coord, Built& built) {
  ChunkMesh mesh = BuildChunkMesh(world_, coord);
  built.revisions = Revisions(coord);
  built.shape = nullptr;
  if (!mesh.triangles.empty()) {
    JPH::MeshShapeSettings settings(std::move(mesh.vertices), std::move(mesh.triangles));
    auto result = settings.Create();
    if (!result.HasError()) built.shape = result.Get();
  }
}

void TerrainCollision::Place(Region& region, const Anchor& anchor, const ChunkCoord& coord,
                             const Built& built) {
  auto [member, inserted] = region.members.try_emplace(coord, kNoShape);
  JPH::uint& sub_shape = member->second;
  JPH::RefConst<JPH::Shape> shape = built.shape;
  if (!shape) {
    if (sub_shape == kNoShape) return;
    shape = new JPH::EmptyShape;  // keeps the other sub-shapes' indices
  }
  // Chunk origin relative to the region's centre: an integer offset, exact in float (players
  // anchored here stay within a few hundred metres of the region, so offsets stay small).
  const JPH::Vec3 origin(
      static_cast<float>((std::int64_t{coord.x} - std::int64_t{anchor.x} * kRegionChunks) *
                         kChunkSize),
      static_cast<float>((std::int64_t{coord.y} - std::int64_t{anchor.y} * kRegionChunks) *
                         kChunkSize),
      static_cast<float>((std::int64_t{coord.z} - std::int64_t{anchor.z} * kRegionChunks) *
                         kChunkSize));
  auto& bodies = physics_.bodies();
  if (region.body.IsInvalid()) {
    JPH::MutableCompoundShapeSettings settings;
    settings.AddShape(origin, JPH::Quat::sIdentity(), shape);
    auto result = settings.Create();
    if (result.HasError()) return;
    region.compound =
        static_cast<JPH::MutableCompoundShape*>(const_cast<JPH::Shape*>(result.Get().GetPtr()));
    sub_shape = 0;
    region.free_sub_shapes.clear();
    JPH::BodyCreationSettings body(region.compound, RegionCenter(anchor), JPH::Quat::sIdentity(),
                                   JPH::EMotionType::Static, ObjectLayers::kTerrain);
    body.mFriction = 0.5f;
    body.mCollisionGroup = JPH::CollisionGroup(groups_, region.group, Groups::kTerrain);
    region.body = bodies.CreateAndAddBody(body, JPH::EActivation::DontActivate);
    return;
  }
  const JPH::Vec3 previous_com = region.compound->GetCenterOfMass();
  if (sub_shape == kNoShape && !region.free_sub_shapes.empty()) {
    sub_shape = region.free_sub_shapes.back();
    region.free_sub_shapes.pop_back();
  }
  if (sub_shape == kNoShape) {
    sub_shape = region.compound->AddShape(origin - previous_com, JPH::Quat::sIdentity(), shape);
  } else {
    region.compound->ModifyShape(sub_shape, origin - previous_com, JPH::Quat::sIdentity(), shape);
  }
  bodies.NotifyShapeChanged(region.body, previous_com, /*updateMassProperties=*/false,
                            JPH::EActivation::DontActivate);
}

}  // namespace dwell::core
