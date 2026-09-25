#include "dwell/core/terrain_collision.h"

#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/EmptyShape.h>
#include <Jolt/Physics/Collision/Shape/MeshShape.h>

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
  Chunk& chunk = world.GetOrCreate(coord);
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

TerrainCollision::~TerrainCollision() {
  if (!body_.IsInvalid()) {
    physics_.bodies().RemoveBody(body_);
    physics_.bodies().DestroyBody(body_);
  }
}

void TerrainCollision::EnsureBox(JPH::Vec3 min, JPH::Vec3 max) {
  const auto lo = ChunkOf(static_cast<std::int32_t>(std::floor(min.GetX())),
                          static_cast<std::int32_t>(std::floor(min.GetY())),
                          static_cast<std::int32_t>(std::floor(min.GetZ())));
  const auto hi = ChunkOf(static_cast<std::int32_t>(std::floor(max.GetX())),
                          static_cast<std::int32_t>(std::floor(max.GetY())),
                          static_cast<std::int32_t>(std::floor(max.GetZ())));
  for (int z = lo.z; z <= hi.z; ++z) {
    for (int y = lo.y; y <= hi.y; ++y) {
      for (int x = lo.x; x <= hi.x; ++x) {
        const ChunkCoord coord{x, y, z};
        auto [it, inserted] = chunks_.try_emplace(coord);
        if (inserted) Build(coord, it->second);
      }
    }
  }
}

void TerrainCollision::Sync() {
  for (auto& [coord, built] : chunks_) {
    if (Revisions(coord) != built.revisions) Build(coord, built);
  }
}

std::array<std::uint32_t, 7> TerrainCollision::Revisions(const ChunkCoord& c) {
  static constexpr int kOffsets[7][3] = {{0, 0, 0},  {1, 0, 0}, {-1, 0, 0}, {0, 1, 0},
                                         {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
  std::array<std::uint32_t, 7> r{};
  for (int i = 0; i < 7; ++i) {
    const Chunk* chunk =
        world_.Find({c.x + kOffsets[i][0], c.y + kOffsets[i][1], c.z + kOffsets[i][2]});
    r[i] = chunk ? chunk->revision() : 0;
  }
  return r;
}

void TerrainCollision::Build(const ChunkCoord& coord, Built& built) {
  ChunkMesh mesh = BuildChunkMesh(world_, coord);
  built.revisions = Revisions(coord);
  JPH::RefConst<JPH::Shape> shape;
  if (!mesh.triangles.empty()) {
    JPH::MeshShapeSettings settings(std::move(mesh.vertices), std::move(mesh.triangles));
    auto result = settings.Create();
    if (!result.HasError()) shape = result.Get();
  }
  if (!shape) {
    if (built.sub_shape == kNoShape) return;
    shape = new JPH::EmptyShape;  // keeps the other sub-shapes' indices
  }
  const JPH::Vec3 origin(static_cast<float>(coord.x * kChunkSize),
                         static_cast<float>(coord.y * kChunkSize),
                         static_cast<float>(coord.z * kChunkSize));
  auto& bodies = physics_.bodies();
  if (body_.IsInvalid()) {
    JPH::MutableCompoundShapeSettings settings;
    settings.AddShape(origin, JPH::Quat::sIdentity(), shape);
    auto result = settings.Create();
    if (result.HasError()) return;
    compound_ =
        static_cast<JPH::MutableCompoundShape*>(const_cast<JPH::Shape*>(result.Get().GetPtr()));
    built.sub_shape = 0;
    JPH::BodyCreationSettings body(compound_, JPH::RVec3::sZero(), JPH::Quat::sIdentity(),
                                   JPH::EMotionType::Static, ObjectLayers::kTerrain);
    body.mFriction = 0.5f;
    body_ = bodies.CreateAndAddBody(body, JPH::EActivation::DontActivate);
    return;
  }
  const JPH::Vec3 previous_com = compound_->GetCenterOfMass();
  if (built.sub_shape == kNoShape) {
    built.sub_shape = compound_->AddShape(origin - previous_com, JPH::Quat::sIdentity(), shape);
  } else {
    compound_->ModifyShape(built.sub_shape, origin - previous_com, JPH::Quat::sIdentity(), shape);
  }
  bodies.NotifyShapeChanged(body_, previous_com, /*updateMassProperties=*/false,
                            JPH::EActivation::DontActivate);
}

}  // namespace dwell::core
