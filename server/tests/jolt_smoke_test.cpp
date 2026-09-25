// Proves Jolt builds, links, and simulates: a sphere dropped onto a static floor comes to rest.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <Jolt/Core/TempAllocator.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/BoxShape.h>
#include <Jolt/Physics/Collision/Shape/SphereShape.h>
#include <Jolt/Physics/PhysicsSystem.h>

#include "dwell/core/jolt_runtime.h"

namespace {

namespace Layers {
constexpr JPH::ObjectLayer kStatic = 0;
constexpr JPH::ObjectLayer kMoving = 1;
constexpr JPH::uint kCount = 2;
}  // namespace Layers

class BroadPhaseLayers final : public JPH::BroadPhaseLayerInterface {
 public:
  JPH::uint GetNumBroadPhaseLayers() const override { return Layers::kCount; }
  JPH::BroadPhaseLayer GetBroadPhaseLayer(JPH::ObjectLayer layer) const override {
    return JPH::BroadPhaseLayer(static_cast<JPH::uint8>(layer));
  }
#if defined(JPH_EXTERNAL_PROFILE) || defined(JPH_PROFILE_ENABLED)
  const char* GetBroadPhaseLayerName(JPH::BroadPhaseLayer) const override { return "layer"; }
#endif
};

class ObjectVsBroadPhase final : public JPH::ObjectVsBroadPhaseLayerFilter {
 public:
  bool ShouldCollide(JPH::ObjectLayer, JPH::BroadPhaseLayer) const override { return true; }
};

class ObjectPairs final : public JPH::ObjectLayerPairFilter {
 public:
  bool ShouldCollide(JPH::ObjectLayer a, JPH::ObjectLayer b) const override {
    return a == Layers::kMoving || b == Layers::kMoving;
  }
};

}  // namespace

TEST_CASE("Jolt drops a sphere onto a floor") {
  dwell::core::JoltRuntime runtime;

  BroadPhaseLayers broad_phase_layers;
  ObjectVsBroadPhase object_vs_broad_phase;
  ObjectPairs object_pairs;

  JPH::PhysicsSystem physics;
  physics.Init(/*maxBodies=*/64, /*numBodyMutexes=*/0, /*maxBodyPairs=*/64,
               /*maxContactConstraints=*/64, broad_phase_layers, object_vs_broad_phase,
               object_pairs);

  JPH::BodyInterface& bodies = physics.GetBodyInterface();
  bodies.CreateAndAddBody(
      JPH::BodyCreationSettings(new JPH::BoxShape(JPH::Vec3(10.0f, 0.5f, 10.0f)),
                                JPH::RVec3(0.0, -0.5, 0.0), JPH::Quat::sIdentity(),
                                JPH::EMotionType::Static, Layers::kStatic),
      JPH::EActivation::DontActivate);
  const JPH::BodyID sphere = bodies.CreateAndAddBody(
      JPH::BodyCreationSettings(new JPH::SphereShape(0.5f), JPH::RVec3(0.0, 5.0, 0.0),
                                JPH::Quat::sIdentity(), JPH::EMotionType::Dynamic, Layers::kMoving),
      JPH::EActivation::Activate);

  JPH::TempAllocatorImpl temp(1024 * 1024);
  JPH::JobSystemSingleThreaded jobs(JPH::cMaxPhysicsJobs);
  constexpr float kDt = 1.0f / 60.0f;  // SIM_HZ
  for (int step = 0; step < 180; ++step) {
    physics.Update(kDt, 1, &temp, &jobs);
  }

  const JPH::RVec3 position = bodies.GetCenterOfMassPosition(sphere);
  CHECK(position.GetY() == doctest::Approx(0.5).epsilon(0.02));  // resting on the floor
  CHECK(bodies.GetLinearVelocity(sphere).Length() < 0.05f);
}
