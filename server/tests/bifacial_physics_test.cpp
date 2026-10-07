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

  TEST_CASE(
      "a body reaching the midplane settles in the band, from either side, without swinging") {
    for (const double side : {+1.0, -1.0}) {
      CAPTURE(side);
      World w;
      const JPH::BodyID id = w.Box(kMidplaneY + side * 30.0);
      int sign_changes = 0;
      float last = 0.0f;
      for (int i = 0; i < 60 * 30; ++i) {
        w.Run(1);
        const float v = w.Vy(id);
        if (std::abs(v) > 0.02f) {
          if (last != 0.0f && (v > 0.0f) != (last > 0.0f)) ++sign_changes;
          last = v;
        }
      }
      CHECK(std::abs(w.Y(id) - kMidplaneY) < 0.1);
      CHECK(std::abs(w.Vy(id)) < 0.02f);
      // At most one change of direction: past the midplane the pull reverses it once.
      CHECK(sign_changes <= 1);
    }
  }

  TEST_CASE("a fall of 2 km (the rim's drop) is braked on the way in and settles at the midplane") {
    for (const double side : {+1.0, -1.0}) {
      CAPTURE(side);
      World w;
      const JPH::BodyID id = w.Box(kMidplaneY + side * 2048.0);
      double deepest = 0;  // furthest across the midplane
      float fastest = 0;
      for (int i = 0; i < 60 * 60; ++i) {
        w.Run(1);
        deepest = std::max(deepest, -side * (w.Y(id) - kMidplaneY));
        fastest = std::max(fastest, std::abs(w.Vy(id)));
      }
      // Free fall would arrive at 200 m/s (4 m a tick, the band's width in two ticks) and swing
      // through the midplane for minutes; braked, it crosses by at most a few metres.
      CHECK(fastest < 200.0f);
      CHECK(deepest < 3.0);
      CHECK(std::abs(w.Y(id) - kMidplaneY) < 0.1);
      CHECK(std::abs(w.Vy(id)) < 0.02f);
    }
  }

  TEST_CASE("a body's own damping returns when it leaves the band") {
    World w;
    const JPH::BodyID id = w.Box(kMidplaneY + 2.0, /*damping=*/0.3f);
    w.Run(10);
    CHECK(w.physics.bodies().GetLinearVelocity(id).Length() < 1.0f);
    // Thrown out of the band: its own damping is back.
    w.physics.bodies().SetLinearVelocity(id, JPH::Vec3(0, 200.0f, 0));
    for (int i = 0; i < 60 && w.Y(id) < kMidplaneY + 6.0; ++i) w.Run(1);
    REQUIRE(w.Y(id) >= kMidplaneY + 6.0);
    w.Run(1);
    JPH::BodyLockRead lock(w.physics.system().GetBodyLockInterface(), id);
    REQUIRE(lock.Succeeded());
    CHECK(lock.GetBody().GetMotionProperties()->GetLinearDamping() == doctest::Approx(0.3f));
  }
}
