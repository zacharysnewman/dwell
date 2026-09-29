#pragma once

// Where tests put their local frame in the world (ADR 0011: the controller must behave the same at
// the origin and ~8,000 km from it). Tests describe geometry and positions in local coordinates;
// these helpers translate. Set with `--dwell-origin-x=<metres>` (tests/main.cpp); a multiple of
// the terrain collision region (2 048 m), so local geometry lands on the same region layout.
#include <Jolt/Jolt.h>

#include <cstdint>

#include "dwell/core/terrain_collision.h"
#include "dwell/core/voxel.h"

namespace dwell::test {

inline std::int32_t& OriginX() {
  static std::int32_t x = 0;
  return x;
}
inline constexpr std::int32_t kOriginAlign =
    core::TerrainCollision::kRegionChunks * core::kChunkSize;
// A far origin inside the world disc: 3 906 regions east of the origin (7 999 488 m).
inline constexpr std::int32_t kFarOriginX = 3906 * kOriginAlign;

inline JPH::RVec3 ToWorld(JPH::Vec3 local) {
  return JPH::RVec3(local) + JPH::RVec3(OriginX(), 0, 0);
}
inline JPH::RVec3 ToWorld(double x, double y, double z) { return JPH::RVec3(x + OriginX(), y, z); }
inline JPH::Vec3 ToLocal(JPH::RVec3 world) {
  return JPH::Vec3(world - JPH::RVec3(OriginX(), 0, 0));
}
inline std::int32_t WorldCellX(std::int32_t local_x) { return local_x + OriginX(); }

// A generator whose output is moved from the origin to the test origin.
inline core::ChunkGenerator Shifted(core::ChunkGenerator generator) {
  if (OriginX() == 0) return generator;
  const std::int32_t cx = OriginX() / core::kChunkSize;
  return [generator = std::move(generator), cx](const core::ChunkCoord& c, core::Chunk& chunk) {
    generator({c.x - cx, c.y, c.z}, chunk);
  };
}

}  // namespace dwell::test
