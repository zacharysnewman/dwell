#include "dwell/player/voxel_query.h"

#include <Jolt/Geometry/AABox.h>
#include <Jolt/Physics/Body/BodyFilter.h>
#include <Jolt/Physics/Body/BodyLock.h>
#include <Jolt/Physics/Collision/BroadPhase/BroadPhaseLayer.h>
#include <Jolt/Physics/Collision/CastResult.h>
#include <Jolt/Physics/Collision/CollideShape.h>
#include <Jolt/Physics/Collision/CollisionCollectorImpl.h>
#include <Jolt/Physics/Collision/ObjectLayer.h>
#include <Jolt/Physics/Collision/RayCast.h>
#include <Jolt/Physics/Collision/Shape/CapsuleShape.h>

#include <algorithm>
#include <limits>

#include "dwell/core/block_shape.h"
#include "dwell/core/physics_world.h"

namespace dwell::player {
namespace {

using core::ObjectLayers::kCharacter;
using core::ObjectLayers::kTier1;

class MovingBroadPhase final : public JPH::BroadPhaseLayerFilter {
 public:
  bool ShouldCollide(JPH::BroadPhaseLayer layer) const override {
    return layer == core::BroadPhaseLayers::kMoving;
  }
};

class MovingLayers final : public JPH::ObjectLayerFilter {
 public:
  bool ShouldCollide(JPH::ObjectLayer layer) const override {
    return layer == kTier1 || layer == kCharacter;
  }
};

GroundRef RefForBody(JPH::ObjectLayer layer, JPH::BodyID id) {
  return {layer == kCharacter ? GroundRef::kPlayer : GroundRef::kTier1Body,
          id.GetIndexAndSequenceNumber()};
}

constexpr float kInf = std::numeric_limits<float>::infinity();

}  // namespace

core::MaterialId VoxelQuery::Material(std::int32_t x, std::int32_t y, std::int32_t z) const {
  const core::ChunkCoord coord = core::ChunkOf(x, y, z);
  if (!cached_chunk_ || !(coord == cached_coord_) || cached_epoch_ != world_.epoch()) {
    cached_chunk_ = &world_.Read(coord);
    cached_coord_ = coord;
    cached_epoch_ = world_.epoch();
  }
  return cached_chunk_->Get(x - coord.x * core::kChunkSize, y - coord.y * core::kChunkSize,
                            z - coord.z * core::kChunkSize);
}

bool VoxelQuery::BodiesNear(RVec3 min, RVec3 max, JPH::BodyID self) const {
  MovingBroadPhase broad;
  MovingLayers layers;
  JPH::AllHitCollisionCollector<JPH::CollideShapeBodyCollector> collector;
  physics_.GetBroadPhaseQuery().CollideAABox(JPH::AABox(min.ToVec3RoundDown(), max.ToVec3RoundUp()),
                                             collector, broad, layers);
  for (const JPH::BodyID& id : collector.mHits) {
    if (id != self) return true;
  }
  return false;
}

bool VoxelQuery::CastRay(RVec3 origin, Vec3 dir, float max_distance, JPH::BodyID self,
                         ProbeHit& hit, bool bodies) const {
  ProbeHit a, b;
  const bool hit_a = CastVoxels(origin, dir, max_distance, a);
  const bool hit_b = bodies && CastBodies(origin, dir, hit_a ? a.distance : max_distance, self, b);
  if (hit_b && (!hit_a || b.distance < a.distance)) {
    hit = b;
    return true;
  }
  if (hit_a) hit = a;
  return hit_a;
}

bool VoxelQuery::CastVoxels(RVec3 origin, Vec3 dir, float max_distance, ProbeHit& hit) const {
  // Amanatides–Woo walk. Per cell, the ray's span [t_cell, t_next] is intersected with the cell's
  // shape box; a hit is the ray entering a shape from free space. The walk runs in float relative
  // to the origin's cell (`base`), so it is equally precise anywhere in the world (ADR 0011).
  std::int32_t base[3];
  float o[3];
  for (int i = 0; i < 3; ++i) {
    base[i] = static_cast<std::int32_t>(std::floor(origin[i]));
    o[i] = static_cast<float>(origin[i] - static_cast<double>(base[i]));
  }
  float d[3] = {dir.GetX(), dir.GetY(), dir.GetZ()};
  std::int32_t cell[3], step[3];  // cell: relative to base
  float t_max[3], t_delta[3];
  for (int i = 0; i < 3; ++i) {
    cell[i] = static_cast<std::int32_t>(std::floor(o[i]));
    if (d[i] > 0.0f) {
      step[i] = 1;
      t_delta[i] = 1.0f / d[i];
      t_max[i] = (static_cast<float>(cell[i]) + 1.0f - o[i]) / d[i];
    } else if (d[i] < 0.0f) {
      step[i] = -1;
      t_delta[i] = -1.0f / d[i];
      t_max[i] = (static_cast<float>(cell[i]) - o[i]) / d[i];
    } else {
      step[i] = 0;
      t_delta[i] = kInf;
      t_max[i] = kInf;
    }
  }

  float t_cell = 0.0f;
  bool first_cell = true;  // a ray starting on a solid's surface does not enter it
  while (t_cell <= max_distance) {
    const float t_next = std::min({t_max[0], t_max[1], t_max[2]});
    const std::int32_t cx = base[0] + cell[0], cy = base[1] + cell[1], cz = base[2] + cell[2];
    const core::ShapeInfo& shape = core::ShapeOf(Material(cx, cy, cz));
    if (shape.face_count > 0) {
      // Faces against a solid neighbour are internal: a ray crossing them is already inside.
      static constexpr int kStep[6][3] = {{1, 0, 0},  {-1, 0, 0}, {0, 1, 0},
                                          {0, -1, 0}, {0, 0, 1},  {0, 0, -1}};
      const core::ShapeInfo* neighbours[6];
      for (int face = 0; face < 6; ++face) {
        neighbours[face] =
            &core::ShapeOf(Material(cx + kStep[face][0], cy + kStep[face][1], cz + kStep[face][2]));
      }
      const float corner[3] = {static_cast<float>(cell[0]), static_cast<float>(cell[1]),
                               static_cast<float>(cell[2])};
      if (const auto entry = core::RayEnterShape(shape, corner, o, d, t_next, neighbours,
                                                 /*min_t=*/first_cell ? 1e-6f : 0.0f)) {
        if (entry->t > max_distance) return false;
        hit.distance = entry->t;
        hit.point = origin + dir * entry->t;
        hit.normal = Vec3(entry->normal[0], entry->normal[1], entry->normal[2]);
        hit.ground = {GroundRef::kTerrain, 0};
        return true;
      }
    }

    first_cell = false;

    // Next cell.
    int axis = 0;
    if (t_max[1] < t_max[axis]) axis = 1;
    if (t_max[2] < t_max[axis]) axis = 2;
    if (t_max[axis] == kInf) break;
    t_cell = t_max[axis];
    t_max[axis] += t_delta[axis];
    cell[axis] += step[axis];
  }
  return false;
}

bool VoxelQuery::CastBodies(RVec3 origin, Vec3 dir, float max_distance, JPH::BodyID self,
                            ProbeHit& hit) const {
  const JPH::RRayCast ray{origin, dir * max_distance};
  JPH::RayCastResult result;
  MovingBroadPhase broad;
  MovingLayers layers;
  JPH::IgnoreSingleBodyFilter body_filter(self);
  if (!physics_.GetNarrowPhaseQuery().CastRay(ray, result, broad, layers, body_filter)) {
    return false;
  }
  JPH::BodyLockRead lock(physics_.GetBodyLockInterface(), result.mBodyID);
  if (!lock.Succeeded()) return false;
  const JPH::Body& body = lock.GetBody();
  const JPH::RVec3 point = ray.GetPointOnRay(result.mFraction);
  hit.distance = result.mFraction * max_distance;
  hit.point = point;
  hit.normal = body.GetWorldSpaceSurfaceNormal(result.mSubShapeID2, point);
  hit.ground = RefForBody(body.GetObjectLayer(), body.GetID());
  return true;
}

float VoxelQuery::SegmentBoxDistance(const Capsule& c, RVec3 world_lo, RVec3 world_hi) {
  // Relative to the capsule centre, so float is exact enough anywhere in the world.
  const Vec3 lo(world_lo - c.center), hi(world_hi - c.center);
  const float dx = std::max({lo.GetX(), 0.0f, -hi.GetX()});
  const float dz = std::max({lo.GetZ(), 0.0f, -hi.GetZ()});
  const float dy = std::max({lo.GetY() - c.half_cylinder, 0.0f, -c.half_cylinder - hi.GetY()});
  return std::sqrt(dx * dx + dy * dy + dz * dz);
}

bool VoxelQuery::OverlapsVoxels(const Capsule& c) const {
  bool overlaps = false;
  ForEachOverlappingCell(c, [&](std::int32_t x, std::int32_t y, std::int32_t z,
                                core::MaterialId m) {
    const core::ShapeInfo& shape = core::ShapeOf(m);
    if (overlaps || shape.face_count == 0) return;
    // The capsule axis against the shape's surface in the cell's own coordinates, relative
    // to the capsule centre (float is exact enough anywhere in the world).
    const Vec3 rel(c.center - RVec3(x, y, z));
    if (core::VerticalSegmentDistanceSq(shape, rel.GetX(), rel.GetZ(), rel.GetY() - c.half_cylinder,
                                        rel.GetY() + c.half_cylinder) < c.radius * c.radius) {
      overlaps = true;
    }
  });
  return overlaps;
}

bool VoxelQuery::OverlapsSolid(const Capsule& c, JPH::BodyID self) const {
  if (OverlapsVoxels(c)) return true;
  JPH::CapsuleShape shape(c.half_cylinder, c.radius);
  shape.SetEmbedded();
  JPH::CollideShapeSettings settings;
  JPH::AnyHitCollisionCollector<JPH::CollideShapeCollector> collector;
  MovingBroadPhase broad;
  MovingLayers layers;
  JPH::IgnoreSingleBodyFilter body_filter(self);
  physics_.GetNarrowPhaseQuery().CollideShape(
      &shape, Vec3::sReplicate(1.0f), JPH::RMat44::sTranslation(c.center), settings,
      JPH::RVec3::sZero(), collector, broad, layers, body_filter);
  return collector.HadHit();
}

float VoxelQuery::SubmergedFraction(RVec3 center, float half_height) const {
  // Heights are bounded (WORLD_MIN_Y..WORLD_MAX_Y), so float y is exact enough.
  const float feet = static_cast<float>(center.GetY()) - half_height;
  const float head = static_cast<float>(center.GetY()) + half_height;
  const auto x = static_cast<std::int32_t>(std::floor(center.GetX()));
  const auto z = static_cast<std::int32_t>(std::floor(center.GetZ()));
  float wet = 0.0f;
  for (auto y = static_cast<std::int32_t>(std::floor(feet));
       y <= static_cast<std::int32_t>(std::floor(head)); ++y) {
    if (!core::GetMaterial(Material(x, y, z)).liquid) continue;
    const float lo = std::max(feet, static_cast<float>(y));
    const float hi = std::min(head, static_cast<float>(y) + 1.0f);
    wet += std::max(0.0f, hi - lo);
  }
  return std::clamp(wet / (head - feet), 0.0f, 1.0f);
}

}  // namespace dwell::player
