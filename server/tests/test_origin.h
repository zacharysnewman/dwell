#pragma once

// Where tests put their local frame in the world (ADR 0011: the controller must behave the same at
// the origin and ~8,000 km from it). Tests describe geometry and positions in local coordinates;
// these helpers translate. Set with `--dwell-origin-x=<metres>` (tests/main.cpp); a multiple of
// the terrain collision region (2 048 m), so local geometry lands on the same region layout.
//
// `--dwell-face=b` puts the local frame on face B (ADR 0023): the mirror image of face A about the
// midplane (heights y → −4,096 − y, voxel rows y → −4,097 − y, slabs and slopes turned over,
// vertical velocities negated). The controller suites run unchanged on it and must give the
// mirrored traces: the mirror-equivalence suite (PLAYER_CONTROLLER.md §11).
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

// Whether the local frame is face B's mirror image (see above).
inline bool& FaceB() {
  static bool b = false;
  return b;
}
inline constexpr double kMirror = core::kMirrorSum + 1;  // heights sum to this across the mirror

inline JPH::RVec3 ToWorld(double x, double y, double z) {
  return JPH::RVec3(x + OriginX(), FaceB() ? kMirror - y : y, z);
}
inline JPH::RVec3 ToWorld(JPH::Vec3 local) {
  return ToWorld(local.GetX(), local.GetY(), local.GetZ());
}
inline JPH::Vec3 ToLocal(JPH::RVec3 world) {
  const double y = FaceB() ? kMirror - world.GetY() : world.GetY();
  return JPH::Vec3(static_cast<float>(world.GetX() - OriginX()), static_cast<float>(y),
                   static_cast<float>(world.GetZ()));
}
// Velocities and directions (world → local and back: the same map).
inline JPH::Vec3 MirrorVec(JPH::Vec3 v) {
  return FaceB() ? JPH::Vec3(v.GetX(), -v.GetY(), v.GetZ()) : v;
}
inline std::int32_t WorldCellX(std::int32_t local_x) { return local_x + OriginX(); }
inline std::int32_t WorldCellY(std::int32_t local_y) {
  return FaceB() ? core::MirrorY(local_y) : local_y;
}
inline core::MaterialId WorldMaterial(core::MaterialId local) {
  return FaceB() ? core::MirrorMaterial(local) : local;
}

// A generator whose output is moved from the origin to the test origin (and, on face B, flipped).
inline core::ChunkGenerator Shifted(core::ChunkGenerator generator) {
  if (OriginX() == 0 && !FaceB()) return generator;
  const std::int32_t cx = OriginX() / core::kChunkSize;
  return [generator = std::move(generator), cx](const core::ChunkCoord& c, core::Chunk& chunk) {
    if (!FaceB()) {
      generator({c.x - cx, c.y, c.z}, chunk);
      return;
    }
    core::Chunk local;
    generator({c.x - cx, core::MirrorChunkY(c.y), c.z}, local);
    auto& dst = chunk.generation_voxels();
    for (int z = 0; z < core::kChunkSize; ++z)
      for (int y = 0; y < core::kChunkSize; ++y)
        for (int x = 0; x < core::kChunkSize; ++x) {
          dst[static_cast<std::size_t>(core::LocalIndex(x, core::kChunkSize - 1 - y, z))] =
              core::MirrorMaterial(local.Get(x, y, z));
        }
  };
}

}  // namespace dwell::test
