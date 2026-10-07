#include "dwell/core/physics_world.h"

#include <Jolt/Physics/Body/BodyLock.h>

#include <cmath>

#include "dwell/core/flip_band.h"
#include "dwell/core/voxel.h"

namespace dwell::core {

bool LayersCollide(JPH::ObjectLayer a, JPH::ObjectLayer b) {
  return !(a == ObjectLayers::kTerrain && b == ObjectLayers::kTerrain);
}

class PhysicsWorld::Layers final : public JPH::BroadPhaseLayerInterface,
                                   public JPH::ObjectVsBroadPhaseLayerFilter,
                                   public JPH::ObjectLayerPairFilter {
 public:
  // BroadPhaseLayerInterface
  JPH::uint GetNumBroadPhaseLayers() const override { return BroadPhaseLayers::kCount; }
  JPH::BroadPhaseLayer GetBroadPhaseLayer(JPH::ObjectLayer layer) const override {
    return layer == ObjectLayers::kTerrain ? BroadPhaseLayers::kStatic : BroadPhaseLayers::kMoving;
  }
#if defined(JPH_EXTERNAL_PROFILE) || defined(JPH_PROFILE_ENABLED)
  const char* GetBroadPhaseLayerName(JPH::BroadPhaseLayer layer) const override {
    return layer == BroadPhaseLayers::kStatic ? "static" : "moving";
  }
#endif

  // ObjectVsBroadPhaseLayerFilter
  bool ShouldCollide(JPH::ObjectLayer layer, JPH::BroadPhaseLayer broad) const override {
    return !(layer == ObjectLayers::kTerrain && broad == BroadPhaseLayers::kStatic);
  }

  // ObjectLayerPairFilter
  bool ShouldCollide(JPH::ObjectLayer a, JPH::ObjectLayer b) const override {
    return LayersCollide(a, b);
  }
};

PhysicsWorld::PhysicsWorld(JPH::JobSystem& jobs, const PhysicsConfig& config)
    : layers_(std::make_unique<Layers>()),
      system_(std::make_unique<JPH::PhysicsSystem>()),
      temp_(std::make_unique<JPH::TempAllocatorImpl>(config.temp_allocator_bytes)),
      jobs_(jobs),
      config_(config) {
  system_->Init(config.max_bodies, /*numBodyMutexes=*/0, config.max_body_pairs,
                config.max_contact_constraints, *layers_, *layers_, *layers_);
  system_->SetGravity(JPH::Vec3(0.0f, config.gravity_y, 0.0f));
}

PhysicsWorld::~PhysicsWorld() = default;

void PhysicsWorld::Step(float dt) {
  // Gravity by side (BIFACIAL_WORLD.md §3): toward the midplane, fading to zero across the band.
  JPH::BodyIDVector active;
  system_->GetActiveBodies(JPH::EBodyType::RigidBody, active);
  JPH::BodyInterface& bi = system_->GetBodyInterface();
  for (const JPH::BodyID id : active) {
    if (bi.GetObjectLayer(id) != ObjectLayers::kTier1) continue;
    JPH::BodyLockWrite lock(system_->GetBodyLockInterface(), id);
    if (!lock.Succeeded() || !lock.GetBody().IsDynamic()) continue;
    JPH::Body& body = lock.GetBody();
    const double over = body.GetCenterOfMassPosition().GetY() - kMidplaneY;
    const float side = over >= 0.0 ? 1.0f : -1.0f;
    const float depth = static_cast<float>(std::abs(over));
    const bool in_band = depth < config_.flip_band;
    JPH::MotionProperties& mp = *body.GetMotionProperties();
    mp.SetGravityFactor(side * (in_band ? depth / config_.flip_band : 1.0f));
    // The approach cushion (flip_band.h): not toward the midplane faster than the band can stop.
    const float limit = ApproachSpeedLimit(depth, config_.flip_band, config_.band_damping,
                                           config_.brake_gravities * std::abs(config_.gravity_y));
    const float toward = -side * mp.GetLinearVelocity().GetY();  // speed toward the midplane
    if (toward > limit) {
      JPH::Vec3 v = mp.GetLinearVelocity();
      v.SetY(-side * limit);
      mp.SetLinearVelocity(v);
    }
    const std::uint32_t key = id.GetIndexAndSequenceNumber();
    if (in_band) {
      band_damping_.try_emplace(key, mp.GetLinearDamping());  // the first tick in the band
      mp.SetLinearDamping(config_.band_damping);
    } else if (const auto it = band_damping_.find(key); it != band_damping_.end()) {
      mp.SetLinearDamping(it->second);  // out again: the body's own
      band_damping_.erase(it);
    }
  }
  system_->Update(dt, /*collisionSteps=*/1, temp_.get(), &jobs_);
}

}  // namespace dwell::core
