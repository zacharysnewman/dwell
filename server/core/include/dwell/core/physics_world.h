#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystem.h>
#include <Jolt/Core/TempAllocator.h>
#include <Jolt/Physics/PhysicsSystem.h>

#include <cstdint>
#include <memory>
#include <unordered_map>

namespace dwell::core {

// Object layers (ARCHITECTURE.md §6.2, §7). Terrain is static per-chunk collision; Tier 1 clusters
// and player characters are dynamic.
namespace ObjectLayers {
inline constexpr JPH::ObjectLayer kTerrain = 0;
inline constexpr JPH::ObjectLayer kTier1 = 1;
inline constexpr JPH::ObjectLayer kCharacter = 2;
inline constexpr JPH::ObjectLayer kCount = 3;
}  // namespace ObjectLayers

namespace BroadPhaseLayers {
inline constexpr JPH::BroadPhaseLayer kStatic{0};
inline constexpr JPH::BroadPhaseLayer kMoving{1};
inline constexpr JPH::uint kCount = 2;
}  // namespace BroadPhaseLayers

// Whether two object layers collide (terrain never collides with terrain).
bool LayersCollide(JPH::ObjectLayer a, JPH::ObjectLayer b);

struct PhysicsConfig {
  JPH::uint max_bodies = 65536;
  JPH::uint max_body_pairs = 65536;
  JPH::uint max_contact_constraints = 16384;
  std::size_t temp_allocator_bytes = 16 * 1024 * 1024;
  float gravity_y = -9.81f;  // world gravity for Tier 1 bodies (players use their own; §9)
  // The flip band (BIFACIAL_WORLD.md §3): within this distance of the midplane gravity fades to
  // zero and a body's linear damping rises to `band_damping`, overdamped (c² ≥ 4 · g / band).
  float flip_band = 4.0f;
  float band_damping = 8.0f;     // 1/s
  float brake_gravities = 2.0f;  // approach cushion braking (flip_band.h), in gravities
};

// The server's Jolt world. The host supplies the job system (thread pool natively, single-threaded
// in WASM; ADR 0007), so the core creates no threads. Requires a live JoltRuntime.
class PhysicsWorld {
 public:
  explicit PhysicsWorld(JPH::JobSystem& jobs, const PhysicsConfig& config = {});
  ~PhysicsWorld();

  PhysicsWorld(const PhysicsWorld&) = delete;
  PhysicsWorld& operator=(const PhysicsWorld&) = delete;

  // Steps the world. First sets each active Tier 1 body's gravity by its side of the midplane (the
  // world keeps one gravity vector, pointing −y: a body on face B gets a factor of −1), faded to
  // zero over the flip band with a damping that settles it there instead of swinging across it.
  void Step(float dt);

  JPH::PhysicsSystem& system() { return *system_; }
  JPH::BodyInterface& bodies() { return system_->GetBodyInterface(); }

 private:
  class Layers;
  std::unique_ptr<Layers> layers_;
  std::unique_ptr<JPH::PhysicsSystem> system_;
  std::unique_ptr<JPH::TempAllocatorImpl> temp_;
  JPH::JobSystem& jobs_;
  PhysicsConfig config_;
  // Linear damping of the bodies currently in the flip band, before the band's was set.
  std::unordered_map<std::uint32_t, float> band_damping_;
};

}  // namespace dwell::core
