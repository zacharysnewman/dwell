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
  if (!cached_chunk_ || !(coord == cached_coord_)) {
    cached_chunk_ = &world_.GetOrCreate(coord);
    cached_coord_ = coord;
  }
  return cached_chunk_->Get(x - coord.x * core::kChunkSize, y - coord.y * core::kChunkSize,
                            z - coord.z * core::kChunkSize);
}

bool VoxelQuery::BodiesNear(Vec3 min, Vec3 max, JPH::BodyID self) const {
  MovingBroadPhase broad;
  MovingLayers layers;
  JPH::AllHitCollisionCollector<JPH::CollideShapeBodyCollector> collector;
  physics_.GetBroadPhaseQuery().CollideAABox(JPH::AABox(min, max), collector, broad, layers);
  for (const JPH::BodyID& id : collector.mHits) {
    if (id != self) return true;
  }
  return false;
}

bool VoxelQuery::CastRay(Vec3 origin, Vec3 dir, float max_distance, JPH::BodyID self, ProbeHit& hit,
                         bool bodies) const {
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

bool VoxelQuery::CastVoxels(Vec3 origin, Vec3 dir, float max_distance, ProbeHit& hit) const {
  // Amanatides–Woo walk. Per cell, the ray's span [t_cell, t_next] is intersected with the cell's
  // shape box; a hit is the ray entering a shape from free space.
  float o[3] = {origin.GetX(), origin.GetY(), origin.GetZ()};
  float d[3] = {dir.GetX(), dir.GetY(), dir.GetZ()};
  std::int32_t cell[3], step[3];
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

  bool in_solid = true;  // a shape containing the origin doesn't count as a hit
  float t_cell = 0.0f;
  int entry_axis = -1;
  while (t_cell <= max_distance) {
    const float t_next = std::min({t_max[0], t_max[1], t_max[2]});
    const auto& material = core::GetMaterial(Material(cell[0], cell[1], cell[2]));
    const float height = core::ShapeHeight(material.shape);
    bool free_after = true;
    if (height > 0.0f) {
      // Ray vs. the shape box [cell, cell + (1, height, 1)].
      float e0 = -kInf, e1 = kInf;
      int axis0 = -1;
      bool miss = false;
      for (int i = 0; i < 3 && !miss; ++i) {
        const float lo = static_cast<float>(cell[i]);
        const float hi = lo + (i == 1 ? height : 1.0f);
        if (d[i] == 0.0f) {
          if (o[i] < lo || o[i] > hi) miss = true;
          continue;
        }
        float a = (lo - o[i]) / d[i], b = (hi - o[i]) / d[i];
        if (a > b) std::swap(a, b);
        if (a > e0) {
          e0 = a;
          axis0 = i;
        }
        e1 = std::min(e1, b);
      }
      e0 = std::max(e0, t_cell);
      e1 = std::min(e1, t_next);
      if (!miss && e0 <= e1) {
        constexpr float kEps = 1e-6f;
        const bool enters_from_free = e0 > t_cell + kEps || !in_solid;
        if (enters_from_free) {
          if (e0 > max_distance) return false;
          const int axis = e0 > t_cell + kEps ? axis0 : entry_axis;
          float n[3] = {0.0f, 0.0f, 0.0f};
          if (axis >= 0) n[axis] = d[axis] > 0.0f ? -1.0f : 1.0f;
          hit.distance = e0;
          hit.point = origin + dir * e0;
          hit.normal = Vec3(n[0], n[1], n[2]);
          hit.ground = {GroundRef::kTerrain, 0};
          return true;
        }
        free_after = e1 < t_next - kEps;
      }
    }
    in_solid = !free_after;

    // Next cell.
    int axis = 0;
    if (t_max[1] < t_max[axis]) axis = 1;
    if (t_max[2] < t_max[axis]) axis = 2;
    if (t_max[axis] == kInf) break;
    t_cell = t_max[axis];
    t_max[axis] += t_delta[axis];
    cell[axis] += step[axis];
    entry_axis = axis;
  }
  return false;
}

bool VoxelQuery::CastBodies(Vec3 origin, Vec3 dir, float max_distance, JPH::BodyID self,
                            ProbeHit& hit) const {
  const JPH::RRayCast ray{JPH::RVec3(origin), dir * max_distance};
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
  hit.point = Vec3(point);
  hit.normal = body.GetWorldSpaceSurfaceNormal(result.mSubShapeID2, point);
  hit.ground = RefForBody(body.GetObjectLayer(), body.GetID());
  return true;
}

float VoxelQuery::SegmentBoxDistance(const Capsule& c, Vec3 lo, Vec3 hi) {
  const float dx = std::max({lo.GetX() - c.center.GetX(), 0.0f, c.center.GetX() - hi.GetX()});
  const float dz = std::max({lo.GetZ() - c.center.GetZ(), 0.0f, c.center.GetZ() - hi.GetZ()});
  const float seg_lo = c.center.GetY() - c.half_cylinder;
  const float seg_hi = c.center.GetY() + c.half_cylinder;
  const float dy = std::max({lo.GetY() - seg_hi, 0.0f, seg_lo - hi.GetY()});
  return std::sqrt(dx * dx + dy * dy + dz * dz);
}

bool VoxelQuery::OverlapsVoxels(const Capsule& c) const {
  bool overlaps = false;
  ForEachOverlappingCell(
      c, [&](std::int32_t x, std::int32_t y, std::int32_t z, core::MaterialId m) {
        const float height = core::ShapeHeight(core::GetMaterial(m).shape);
        if (overlaps || height <= 0.0f) return;
        const Vec3 lo(static_cast<float>(x), static_cast<float>(y), static_cast<float>(z));
        if (SegmentBoxDistance(c, lo, lo + Vec3(1.0f, height, 1.0f)) < c.radius) overlaps = true;
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
      &shape, Vec3::sReplicate(1.0f), JPH::RMat44::sTranslation(JPH::RVec3(c.center)), settings,
      JPH::RVec3::sZero(), collector, broad, layers, body_filter);
  return collector.HadHit();
}

float VoxelQuery::SubmergedFraction(Vec3 center, float half_height) const {
  const float feet = center.GetY() - half_height;
  const float head = center.GetY() + half_height;
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
