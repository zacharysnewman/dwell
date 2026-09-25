#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Physics/Body/BodyID.h>
#include <Jolt/Physics/PhysicsSystem.h>

#include <cmath>
#include <cstdint>

#include "dwell/core/voxel.h"

// Probes for the player controller (PLAYER_CONTROLLER.md §5): static terrain is answered from the
// voxel grid (DDA / cell overlap), moving things (Tier 1 bodies, other players) from Jolt.
namespace dwell::player {

using JPH::Vec3;

struct GroundRef {
  enum Kind : std::uint8_t { kNone, kTerrain, kTier1Body, kPlayer };
  Kind kind = kNone;
  std::uint32_t id = 0;  // Jolt BodyID (index and sequence) for bodies, 0 for terrain
  bool operator==(const GroundRef&) const = default;
};

struct ProbeHit {
  float distance = 0.0f;
  Vec3 point = Vec3::sZero();
  Vec3 normal = Vec3::sAxisY();
  GroundRef ground;
};

// A vertical capsule: centre, radius, and half the cylinder's height.
struct Capsule {
  Vec3 center;
  float radius;
  float half_cylinder;
};

class VoxelQuery {
 public:
  VoxelQuery(core::VoxelWorld& world, JPH::PhysicsSystem& physics)
      : world_(world), physics_(physics) {}

  // Nearest solid along the ray (unit `dir`), ignoring the body `self`. A ray that starts inside
  // solid terrain only hits once it has left it (as against a surface mesh).
  // `bodies` = false skips the Jolt part (the caller knows no moving body is in reach).
  bool CastRay(Vec3 origin, Vec3 dir, float max_distance, JPH::BodyID self, ProbeHit& hit,
               bool bodies = true) const;
  // Is any moving body other than `self` within the box? (Lets a batch of probes skip Jolt.)
  bool BodiesNear(Vec3 min, Vec3 max, JPH::BodyID self) const;
  bool CastVoxels(Vec3 origin, Vec3 dir, float max_distance, ProbeHit& hit) const;
  bool CastBodies(Vec3 origin, Vec3 dir, float max_distance, JPH::BodyID self, ProbeHit& hit) const;

  // Does the capsule overlap solid terrain or a moving body other than `self`?
  bool OverlapsSolid(const Capsule& capsule, JPH::BodyID self) const;
  bool OverlapsVoxels(const Capsule& capsule) const;

  // Calls fn(x, y, z, material) for every non-air cell the capsule overlaps (cell as a full box).
  template <typename Fn>
  void ForEachOverlappingCell(const Capsule& capsule, Fn&& fn) const;

  // Fraction [0, 1] of the capsule's height inside liquid cells, sampled at its centre column.
  float SubmergedFraction(Vec3 center, float half_height) const;

  // Voxel lookup with a one-chunk cache (probes walk neighbouring cells).
  core::MaterialId Material(std::int32_t x, std::int32_t y, std::int32_t z) const;
  JPH::PhysicsSystem& physics() const { return physics_; }

  // Distance between a vertical capsule's axis segment and an axis-aligned box.
  static float SegmentBoxDistance(const Capsule& capsule, Vec3 box_min, Vec3 box_max);

 private:
  core::VoxelWorld& world_;
  JPH::PhysicsSystem& physics_;
  mutable core::ChunkCoord cached_coord_{};
  mutable const core::Chunk* cached_chunk_ = nullptr;
};

template <typename Fn>
void VoxelQuery::ForEachOverlappingCell(const Capsule& c, Fn&& fn) const {
  const float half_height = c.half_cylinder + c.radius;
  const auto x0 = static_cast<std::int32_t>(std::floor(c.center.GetX() - c.radius));
  const auto x1 = static_cast<std::int32_t>(std::floor(c.center.GetX() + c.radius));
  const auto y0 = static_cast<std::int32_t>(std::floor(c.center.GetY() - half_height));
  const auto y1 = static_cast<std::int32_t>(std::floor(c.center.GetY() + half_height));
  const auto z0 = static_cast<std::int32_t>(std::floor(c.center.GetZ() - c.radius));
  const auto z1 = static_cast<std::int32_t>(std::floor(c.center.GetZ() + c.radius));
  for (std::int32_t z = z0; z <= z1; ++z) {
    for (std::int32_t y = y0; y <= y1; ++y) {
      for (std::int32_t x = x0; x <= x1; ++x) {
        const core::MaterialId m = Material(x, y, z);
        if (m == core::Materials::kAir) continue;
        const Vec3 lo(static_cast<float>(x), static_cast<float>(y), static_cast<float>(z));
        if (SegmentBoxDistance(c, lo, lo + Vec3::sReplicate(1.0f)) < c.radius) fn(x, y, z, m);
      }
    }
  }
}

}  // namespace dwell::player
