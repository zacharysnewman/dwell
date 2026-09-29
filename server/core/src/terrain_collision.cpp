#include "dwell/core/terrain_collision.h"

#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/EmptyShape.h>
#include <Jolt/Physics/Collision/Shape/MeshShape.h>

#include <algorithm>
#include <cmath>

namespace dwell::core {
namespace {

// Adds a quad on the plane `axis = plane`, spanning [u0, u1] × [v0, v1] on the other two axes
// (u = axis + 1, v = axis + 2, cyclic), facing +axis when `sign` > 0 and −axis otherwise.
void AddQuad(ChunkMesh& mesh, int axis, int sign, float plane, float u0, float u1, float v0,
             float v1) {
  const int u = (axis + 1) % 3;
  const int v = (axis + 2) % 3;
  auto corner = [&](float cu, float cv) {
    float p[3];
    p[axis] = plane;
    p[u] = cu;
    p[v] = cv;
    return JPH::Float3(p[0], p[1], p[2]);
  };
  const auto base = static_cast<JPH::uint32>(mesh.vertices.size());
  mesh.vertices.push_back(corner(u0, v0));
  mesh.vertices.push_back(corner(u1, v0));
  mesh.vertices.push_back(corner(u1, v1));
  mesh.vertices.push_back(corner(u0, v1));
  // e_u × e_v = e_axis, so (0, 1, 2) winds counter-clockwise seen from +axis.
  if (sign > 0) {
    mesh.triangles.emplace_back(base, base + 1, base + 2);
    mesh.triangles.emplace_back(base, base + 2, base + 3);
  } else {
    mesh.triangles.emplace_back(base, base + 2, base + 1);
    mesh.triangles.emplace_back(base, base + 3, base + 2);
  }
}

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

  // Exposed faces of `shape` cells (full cubes: height 1, slabs: 0.5). A face is hidden by a full
  // neighbour; slab side faces also by a slab neighbour; a slab's top face is always exposed.
  auto shape_faces = [&](VoxelShape shape) {
    const float height = ShapeHeight(shape);
    auto covers = [&](MaterialId neighbour, int axis) {
      const VoxelShape n = GetMaterial(neighbour).shape;
      if (n == VoxelShape::kFull) return true;
      return shape == VoxelShape::kSlabBottom && n == VoxelShape::kSlabBottom && axis != 1;
    };
    for (int lz = 0; lz < kChunkSize; ++lz) {
      for (int ly = 0; ly < kChunkSize; ++ly) {
        for (int lx = 0; lx < kChunkSize; ++lx) {
          if (GetMaterial(chunk.Get(lx, ly, lz)).shape != shape) continue;
          const float cell[3] = {static_cast<float>(lx), static_cast<float>(ly),
                                 static_cast<float>(lz)};
          for (int axis = 0; axis < 3; ++axis) {
            const int u = (axis + 1) % 3;
            const int v = (axis + 2) % 3;
            for (int sign = -1; sign <= 1; sign += 2) {
              const bool slab_top = shape == VoxelShape::kSlabBottom && axis == 1 && sign > 0;
              if (!slab_top) {
                int n[3] = {lx, ly, lz};
                n[axis] += sign;
                if (covers(at(n[0], n[1], n[2]), axis)) continue;
              }
              const float plane = cell[axis] + (sign > 0 ? (axis == 1 ? height : 1.0f) : 0.0f);
              const float u1 = cell[u] + (u == 1 ? height : 1.0f);
              const float v1 = cell[v] + (v == 1 ? height : 1.0f);
              AddQuad(mesh, axis, sign, plane, cell[u], u1, cell[v], v1);
            }
          }
        }
      }
    }
  };
  shape_faces(VoxelShape::kFull);
  shape_faces(VoxelShape::kSlabBottom);
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
