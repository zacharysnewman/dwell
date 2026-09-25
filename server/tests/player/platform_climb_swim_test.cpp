// Ported PPC suites Phase5Platform and Phase6Climb (ladders as voxels), plus Dwell's swim layer.
// Moving platforms are Tier 1 bodies (kinematic boxes moved by their velocity, or dynamic boxes).
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include "player_test_world.h"

using namespace dwell;
using namespace dwell::test;
using player::GroundRef;
namespace Ev = player::Events;

namespace {
const Vec3 kPlatformHalf(3, 0.5f, 3);
}

TEST_SUITE("player: platforms") {
  TEST_CASE("rides a moving kinematic platform") {
    PlayerTestWorld w;
    const auto platform =
        w.Box(Vec3(0, -0.5f, 0), kPlatformHalf, JPH::Quat::sIdentity(), Vec3(3, 0, 0));
    const auto e = w.Spawn(Vec3::sZero());
    w.Step(Ticks(0.5f));
    const Vec3 settled = w.Pos(e) - w.BodyPosition(platform);
    w.Step(Ticks(2.0f));
    CHECK(w.BodyPosition(platform).GetX() > 5.0f);
    CHECK(((w.Pos(e) - w.BodyPosition(platform)) - settled).Length() < 0.03f);
    CHECK(w.C(e).ground.grounded);
    CHECK(w.C(e).ground.ground.kind == GroundRef::kTier1Body);
  }

  TEST_CASE("rides a dynamic platform using its body velocity") {
    PlayerTestWorld w;
    const auto platform = w.DynamicBox(Vec3(0, -0.5f, 0), kPlatformHalf, 1000.0f, Vec3(2, 0, 0));
    const auto e = w.Spawn(Vec3::sZero());
    w.Step(Ticks(0.5f));
    const Vec3 settled = w.Pos(e) - w.BodyPosition(platform);
    w.Step(Ticks(2.0f));
    CHECK(w.BodyPosition(platform).GetX() > 4.0f);
    CHECK(((w.Pos(e) - w.BodyPosition(platform)) - settled).Length() < 0.05f);
    CHECK(w.C(e).platform.base_velocity.GetX() == doctest::Approx(2.0f).epsilon(0.05));
  }

  TEST_CASE("walking on a moving platform is relative to it") {
    PlayerTestWorld w;
    w.Box(Vec3(0, -0.5f, 0), Vec3(10, 0.5f, 10), JPH::Quat::sIdentity(), Vec3(0, 0, 3));
    const auto e = w.Spawn(Vec3::sZero());
    w.input = [](int, PlayerHandle) { return Move(-1, 0); };  // strafe left: +X at yaw 0
    w.Step(Ticks(0.8f));
    CHECK(w.Vel(e).GetX() == doctest::Approx(5.0f).epsilon(0.02));
    CHECK(w.Vel(e).GetZ() == doctest::Approx(3.0f).epsilon(0.03));
  }

  TEST_CASE("carried around a rotating platform") {
    PlayerTestWorld w;
    w.Box(Vec3(0, -0.5f, 0), Vec3(5, 0.5f, 5), JPH::Quat::sIdentity(), Vec3::sZero(), 90.0f);
    const auto e = w.Spawn(Vec3(0, 0, 2));
    w.Step(3);
    const float yaw_delta = w.C(e).platform.yaw_delta;
    w.Step(Ticks(1.0f) - 3);
    const Vec3 p = w.Pos(e);
    CHECK(std::sqrt(p.GetX() * p.GetX() + p.GetZ() * p.GetZ()) ==
          doctest::Approx(2.0f).epsilon(0.08));
    CHECK(p.GetX() > 1.0f);  // 90° about +Y takes (0, 0, 2) towards (2, 0, 0)
    CHECK(yaw_delta == doctest::Approx(1.5f).epsilon(0.05));
  }

  TEST_CASE("rides an elevator up and down staying grounded") {
    for (float speed : {2.0f, -2.0f}) {
      PlayerTestWorld w;
      w.Floor(-30);
      const auto lift =
          w.Box(Vec3(0, -0.5f, 0), kPlatformHalf, JPH::Quat::sIdentity(), Vec3(0, speed, 0));
      const auto e = w.Spawn(Vec3::sZero());
      int ungrounded = 0;
      for (int i = 0; i < Ticks(2.0f); ++i) {
        w.Step();
        if (!w.C(e).ground.grounded) ++ungrounded;
      }
      const float top = w.BodyPosition(lift).GetY() + 0.5f;
      CHECK(w.Feet(e) - top == doctest::Approx(0.0f).epsilon(0.04));
      CHECK(ungrounded <= 2);  // spawned onto an already-moving lift
    }
  }

  TEST_CASE("jumping off a moving platform keeps its momentum") {
    PlayerTestWorld w;
    w.Box(Vec3(0, -0.5f, 0), kPlatformHalf, JPH::Quat::sIdentity(), Vec3(4, 0, 0));
    const auto e = w.Spawn(Vec3::sZero());
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 40 && t < 42); };
    w.Step(44);
    CHECK_FALSE(w.C(e).ground.grounded);
    CHECK(w.Vel(e).GetX() > 3.8f);
    CHECK(w.Vel(e).GetX() < 4.05f);
  }

  TEST_CASE("an explosion pushes nearby players only") {
    PlayerTestWorld w;
    w.Floor();
    const auto near_player = w.Spawn(Vec3(2, 0, 0.5f));
    const auto far_player = w.Spawn(Vec3(12, 0, 0.5f));
    w.Step(2);
    w.Explosion(5, Vec3(0, 1, 0.5f), 6.0f, 12.0f, 0.5f);
    w.Step(12);
    CHECK(w.Pos(near_player).GetX() > 2.5f);
    CHECK(w.Pos(far_player).GetX() == doctest::Approx(12.0f).epsilon(0.002));
  }

  TEST_CASE("standing on a Tier 1 body that is removed starts a fall (walk-off rules)") {
    PlayerTestWorld w;
    w.Floor(-10);
    const auto box = w.Box(Vec3(0, -0.5f, 0), kPlatformHalf);
    const auto e = w.Spawn(Vec3::sZero());
    w.Step(10);
    CHECK(w.C(e).ground.grounded);
    w.physics.bodies().RemoveBody(box);
    w.Step(10);
    CHECK_FALSE(w.C(e).ground.grounded);
    CHECK(w.C(e).state == State::kFalling);
  }
}

TEST_SUITE("player: climb") {
  // A 3 m ledge (x −2..2, z 2..12) with a ladder column (x = 0, z = 1, y 0..2) facing north (−Z),
  // mounted on the ledge's face. The player starts at z = −0.5 facing +Z.
  PlayerTestWorld& LadderWorld(PlayerTestWorld & w) {
    w.Floor();
    w.Fill(-2, 0, 2, 2, 2, 12, core::Materials::kStone);
    w.Fill(0, 0, 1, 0, 2, 1, core::Materials::kLadderN);
    return w;
  }

  TEST_CASE("walking into a ladder grabs it and holds height") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) { return t < 20 ? Move(0, 1) : Move(0, 0); };
    w.Step(30);
    const float y = w.Pos(e).GetY();
    w.Step(60);
    CHECK(w.Count(e, Ev::kClimbStarted) == 1);
    CHECK(w.C(e).climb.climbing);
    CHECK(w.C(e).state == State::kClimbing);
    CHECK(w.Pos(e).GetY() == doctest::Approx(y).epsilon(0.01));
  }

  TEST_CASE("forward climbs up at climb speed and snaps to the ladder face") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(45);
    CHECK(w.C(e).climb.climbing);
    CHECK(w.Vel(e).GetY() == doctest::Approx(3.0f).epsilon(0.03));
    CHECK(w.Pos(e).GetZ() ==
          doctest::Approx(1.70f).epsilon(0.02));  // plate 1.95 − (0.05 + 0.3 − 0.1)
  }

  TEST_CASE("looking down past the threshold climbs down") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) {
      return t < 40 ? Move(0, 1) : Move(0, 1, false, false, false, 0, -45);
    };
    w.Step(48);
    CHECK(w.C(e).climb.climbing);
    CHECK(w.Vel(e).GetY() == doctest::Approx(-3.0f).epsilon(0.03));
  }

  TEST_CASE("glancing down slightly still climbs up") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) {
      return Move(0, 1, false, false, false, 0, t < 20 ? 0.0f : -10.0f);
    };
    w.Step(40);
    CHECK(w.Vel(e).GetY() == doctest::Approx(3.0f).epsilon(0.03));
  }

  TEST_CASE("strafing moves along the ladder") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) { return t < 30 ? Move(0, 1) : Move(1, 0); };
    w.Step(33);
    CHECK(w.C(e).climb.climbing);
    CHECK(w.Vel(e).GetX() == doctest::Approx(-3.0f).epsilon(0.03));  // right of +Z is −X
  }

  TEST_CASE("jumping off launches away and does not re-grab") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) {
      return Move(0, t < 40 ? 1.0f : 0.0f, false, t >= 40 && t < 42);
    };
    w.Step(41);
    CHECK_FALSE(w.C(e).climb.climbing);
    CHECK(w.Count(e, Ev::kClimbEnded) == 1);
    CHECK(w.Vel(e).GetZ() < -2.0f);  // away from the ladder (−Z)
    CHECK(w.Vel(e).GetY() > 3.0f);
    w.Step(60);
    CHECK(w.Count(e, Ev::kClimbStarted) == 1);
  }

  TEST_CASE("climbing down to the floor lets go") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int t, PlayerHandle) { return t < 50 ? Move(0, 1) : Move(0, -1); };
    w.Step(50 + Ticks(1.5f));
    CHECK(w.Count(e, Ev::kClimbEnded) == 1);
    CHECK_FALSE(w.C(e).climb.climbing);
    CHECK(w.C(e).ground.grounded);
  }

  TEST_CASE("climbing over the top lands on the ledge") {
    PlayerTestWorld w;
    LadderWorld(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    for (int i = 0; i < Ticks(3.0f); ++i) {
      w.Step();
    }
    CHECK_FALSE(w.C(e).climb.climbing);
    CHECK(w.C(e).ground.grounded);
    CHECK(w.Feet(e) == doctest::Approx(3.0f).epsilon(0.01));
    CHECK(w.Pos(e).GetZ() > 2.3f);
  }
}

TEST_SUITE("player: swim") {
  // A 2 m deep pool (x −3..3, z 0..8) in a floor whose top is at y = 0.
  void Pool(PlayerTestWorld & w) {
    w.Fill(-8, -3, -8, 8, -1, 12, core::Materials::kStone);
    w.Fill(-3, -2, 0, 3, -1, 8, core::Materials::kWater);
  }

  TEST_CASE("walking into deep water starts swimming; buoyancy floats the player") {
    PlayerTestWorld w;
    Pool(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, -1));
    w.input = [](int t, PlayerHandle) { return t < 30 ? Move(0, 1) : Move(0, 0); };
    w.Step(Ticks(4.0f));
    CHECK(w.C(e).swim.swimming);
    CHECK(w.C(e).state == State::kSwimming);
    CHECK(w.Count(e, Ev::kSwimStarted) == 1);
    // Settles near float_fraction (0.7) submerged: feet ≈ 0 − 0.7 × 1.8.
    CHECK(w.C(e).swim.submerged == doctest::Approx(0.7f).epsilon(0.08));
  }

  TEST_CASE("crouch dives, jump surfaces, and look-direction swimming moves") {
    PlayerTestWorld w;
    Pool(w);
    const auto e = w.Spawn(Vec3(0.5f, -2, 4));
    w.input = [](int t, PlayerHandle) {
      if (t < 60) return Move(0, 0, false, false, true);  // hold down
      return Move(0, 1, false, false, false, 0, 0);
    };
    w.Step(60);
    CHECK(w.C(e).swim.swimming);
    CHECK(w.Feet(e) < -1.9f);  // at the bottom
    const float z = w.Pos(e).GetZ();
    w.Step(30);
    CHECK(w.Pos(e).GetZ() > z + 0.5f);
  }

  TEST_CASE("jumping at the edge climbs out of the pool") {
    PlayerTestWorld w;
    Pool(w);
    const auto e = w.Spawn(Vec3(0.5f, -1.2f, 7));
    w.input = [](int t, PlayerHandle) { return t < 60 ? Move(0, 1, false, true) : Move(0, 0); };
    w.Step(Ticks(3.0f));
    CHECK_FALSE(w.C(e).swim.swimming);
    CHECK(w.Count(e, Ev::kSwimEnded) >= 1);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
    CHECK(w.Pos(e).GetZ() > 9.0f);
  }

  TEST_CASE("shallow water (1 m) is walked through") {
    PlayerTestWorld w;
    w.Fill(-8, -2, -8, 8, -2, 12, core::Materials::kStone);
    w.Fill(-8, -1, -8, 8, -1, 12, core::Materials::kWater);
    const auto e = w.Spawn(Vec3(0.5f, -1, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(1.0f));
    CHECK_FALSE(w.C(e).swim.swimming);
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(5.0f).epsilon(0.02));
  }
}
