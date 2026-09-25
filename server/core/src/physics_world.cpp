#include "dwell/core/physics_world.h"

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
      jobs_(jobs) {
  system_->Init(config.max_bodies, /*numBodyMutexes=*/0, config.max_body_pairs,
                config.max_contact_constraints, *layers_, *layers_, *layers_);
  system_->SetGravity(JPH::Vec3(0.0f, config.gravity_y, 0.0f));
}

PhysicsWorld::~PhysicsWorld() = default;

void PhysicsWorld::Step(float dt) {
  system_->Update(dt, /*collisionSteps=*/1, temp_.get(), &jobs_);
}

}  // namespace dwell::core
