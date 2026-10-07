// Gravity by side for Tier 1 bodies (BIFACIAL_WORLD.md §3): toward the midplane, fading to zero
// over the flip band, which a body settles in instead of swinging across.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/BoxShape.h>

#include <cmath>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/voxel.h"

using namespace dwell::core;

namespace {

struct World {
  JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  PhysicsWorld physics{jobs};

  JPH::BodyID Box(double y, float damping = 0.0f) {
    JPH::BodyCreationSettings s(new JPH::BoxShape(JPH::Vec3::sReplicate(0.5f)), JPH::RVec3(0, y, 0),
                                JPH::Quat::sIdentity(), JPH::EMotionType::Dynamic,
                                ObjectLayers::kTier1);
    s.mAllowedDOFs = JPH::EAllowedDOFs::TranslationX | JPH::EAllowedDOFs::TranslationY |
                     JPH::EAllowedDOFs::TranslationZ;
    s.mLinearDamping = damping;
    s.mAllowSleeping = false;
    return physics.bodies().CreateAndAddBody(s, JPH::EActivation::Activate);
  }
  double Y(JPH::BodyID id) { return physics.bodies().GetCenterOfMassPosition(id).GetY(); }
  float Vy(JPH::BodyID id) { return physics.bodies().GetLinearVelocity(id).GetY(); }
  void Run(int ticks) {
    for (int i = 0; i < ticks; ++i) physics.Step(1.0f / 60.0f);
  }
};

}  // namespace

TEST_SUITE("bifacial: body gravity") {
  TEST_CASE("a body falls toward the midplane from either face, at the same acceleration") {
    World w;
    const JPH::BodyID a = w.Box(kMidplaneY + 500.0);  // face A: down is −y
    const JPH::BodyID b = w.Box(kMidplaneY - 500.0);  // face B: down is +y
    w.Run(60);                                        // one second
    CHECK(w.Vy(a) == doctest::Approx(-9.81f).epsilon(0.02));
    CHECK(w.Vy(b) == doctest::Approx(+9.81f).epsilon(0.02));
    CHECK(w.Y(a) == doctest::Approx(kMidplaneY + 500.0 - 4.905).epsilon(1e-4));
    CHECK(w.Y(b) == doctest::Approx(kMidplaneY - 500.0 + 4.905).epsilon(1e-4));
  }

  TEST_CASE("the band accelerates a body moving toward the midplane and carries it across") {
    for (const double side : {+1.0, -1.0}) {
      CAPTURE(side);
      World w;
      const JPH::BodyID id = w.Box(kMidplaneY + side * 10.0);
      // Free fall from 10 m reaches sqrt(2 g h) ≈ 14 m/s at the midplane; the band adds to it.
      float fastest = 0;
      bool crossed = false;
      for (int i = 0; i < 60 * 6 && !crossed; ++i) {
        w.Run(1);
        fastest = std::max(fastest, std::abs(w.Vy(id)));
        crossed = side * (w.Y(id) - kMidplaneY) < 0.0;
      }
      CHECK(crossed);
      CHECK(fastest > 15.5f);
    }
  }

  TEST_CASE("moving away from the midplane, nothing slows a body in the band") {
    World w;
    const JPH::BodyID id = w.Box(kMidplaneY + 0.5);
    w.physics.bodies().SetLinearVelocity(id, JPH::Vec3(0, 20.0f, 0));
    w.Run(1);
    CHECK(w.Vy(id) > 19.0f);
  }
}
