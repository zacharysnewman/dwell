#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Physics/Body/BodyID.h>

#include <cmath>
#include <cstdint>

#include "dwell/core/block_registry.h"
#include "dwell/core/voxel.h"
#include "dwell/player/voxel_query.h"

// The controller's face-local frame (BIFACIAL_WORLD.md §6, PLAYER_CONTROLLER.md §11). Down is
// always toward the midplane, so a player on face B stands upside down in the world. The controller
// is written for an upright player; for face B it runs in the *mirror image* of the world — heights
// y → −4,096 − y (voxel rows y → −4,097 − y, the frame the face's terrain is generated in),
// vertical velocities and normals negated, slabs and slopes turned over — where gravity points to
// −y as ever. Everything the controller reads from or writes to the physics world and the voxel
// grid crosses this boundary; on face A the maps are the identity (no arithmetic), so face A plays
// exactly as before. Both maps are involutions: the same function converts either way.
namespace dwell::player {

struct Frame {
  std::int8_t face = 1;  // +1 face A, −1 face B

  bool b() const { return face < 0; }
  // The continuous mirror: heights sum to this (voxel rows sum to one less).
  static constexpr double kMirror = core::kMirrorSum + 1;  // −4,096

  double Y(double y) const { return b() ? kMirror - y : y; }
  RVec3 P(RVec3 p) const { return b() ? RVec3(p.GetX(), kMirror - p.GetY(), p.GetZ()) : p; }
  Vec3 V(Vec3 v) const { return b() ? Vec3(v.GetX(), -v.GetY(), v.GetZ()) : v; }
  std::int32_t Row(std::int32_t y) const { return b() ? core::MirrorY(y) : y; }
  core::MaterialId M(core::MaterialId m) const { return b() ? core::MirrorMaterial(m) : m; }
  // The unit vector the player's head points along, in world coordinates.
  Vec3 WorldUp() const { return Vec3(0.0f, static_cast<float>(face), 0.0f); }
};

// The voxel queries of VoxelQuery in a player's face-local frame: arguments and results are
// local; the grid and the moving bodies are read in the world.
class FaceQuery {
 public:
  FaceQuery(const VoxelQuery& query, Frame frame) : q_(query), f_(frame) {}

  bool CastRay(RVec3 origin, Vec3 dir, float max_distance, JPH::BodyID self, ProbeHit& hit,
               bool bodies = true) const {
    if (!f_.b()) return q_.CastRay(origin, dir, max_distance, self, hit, bodies);
    if (!q_.CastRay(f_.P(origin), f_.V(dir), max_distance, self, hit, bodies)) return false;
    hit.point = f_.P(hit.point);
    hit.normal = f_.V(hit.normal);
    return true;
  }
  bool BodiesNear(RVec3 lo, RVec3 hi, JPH::BodyID self) const {
    if (!f_.b()) return q_.BodiesNear(lo, hi, self);
    const RVec3 a = f_.P(lo), c = f_.P(hi);
    return q_.BodiesNear(RVec3::sMin(a, c), RVec3::sMax(a, c), self);
  }
  bool OverlapsSolid(const Capsule& capsule, JPH::BodyID self) const {
    return q_.OverlapsSolid({f_.P(capsule.center), capsule.radius, capsule.half_cylinder}, self);
  }
  // fn(x, y, z, material) with the cell's row and state in the local frame.
  template <typename Fn>
  void ForEachOverlappingCell(const Capsule& capsule, Fn&& fn) const {
    q_.ForEachOverlappingCell({f_.P(capsule.center), capsule.radius, capsule.half_cylinder},
                              [&](std::int32_t x, std::int32_t y, std::int32_t z,
                                  core::MaterialId m) { fn(x, f_.Row(y), z, f_.M(m)); });
  }
  float SubmergedFraction(RVec3 center, float half_height) const {
    return q_.SubmergedFraction(f_.P(center), half_height);
  }
  core::MaterialId Material(std::int32_t x, std::int32_t y, std::int32_t z) const {
    return f_.M(q_.Material(x, f_.Row(y), z));
  }

 private:
  const VoxelQuery& q_;
  Frame f_;
};

}  // namespace dwell::player
