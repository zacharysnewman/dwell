// Ported PPC suites Phase1Skeleton and Phase2Movement (PLAYER_CONTROLLER.md §10) on voxel
// geometry, with Dwell's default config (recommended feel, 0.3 × 1.8 m capsule).
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Physics/Body/Body.h>
#include <Jolt/Physics/Body/BodyLock.h>
#include <Jolt/Physics/Collision/Shape/CapsuleShape.h>

#include "player_test_world.h"

using namespace dwell;
using namespace dwell::test;
using player::GroundRef;

namespace {
float Sq(float v) { return v * v; }
}  // namespace

TEST_SUITE("player: skeleton") {
  TEST_CASE("spawn configures a rotation-locked, gravity-free, frictionless dynamic capsule") {
    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3::sZero());
    w.Step(1);
    JPH::BodyLockRead lock(w.physics.system().GetBodyLockInterface(), w.players.body(e));
    REQUIRE(lock.Succeeded());
    const JPH::Body& body = lock.GetBody();
    CHECK(body.IsDynamic());
    CHECK(body.GetObjectLayer() == core::ObjectLayers::kCharacter);
    CHECK(body.GetMotionProperties()->GetGravityFactor() == 0.0f);
    CHECK(body.GetFriction() == 0.0f);
    CHECK_FALSE(body.GetAllowSleeping());
    CHECK(body.GetMotionProperties()->GetAllowedDOFs() ==
          (JPH::EAllowedDOFs::TranslationX | JPH::EAllowedDOFs::TranslationY |
           JPH::EAllowedDOFs::TranslationZ));
    CHECK(1.0f / body.GetMotionProperties()->GetInverseMass() ==
          doctest::Approx(80.0f).epsilon(0.001));
    const auto* capsule = static_cast<const JPH::CapsuleShape*>(body.GetShape());
    CHECK(capsule->GetRadius() == doctest::Approx(0.3f));
    CHECK(capsule->GetHalfHeightOfCylinder() + capsule->GetRadius() == doctest::Approx(0.9f));
  }

  TEST_CASE("stands still on the floor without drift or tipping") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    const Vec3 start = w.Pos(e);
    w.Step(120);
    CHECK((w.Pos(e) - start).Length() < 0.01f);
    CHECK(w.C(e).ground.grounded);
    CHECK(w.C(e).state == State::kIdle);
  }

  TEST_CASE("input reaches the controller and move is clamped to unit length") {
    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3::sZero());
    w.input = [](int, PlayerHandle) { return Move(1, 1, true, false, false, 90, -10); };
    w.Step(3);
    const auto& in = w.C(e).input;
    CHECK(in.run);
    CHECK(in.look_yaw == 90.0f);
    CHECK(in.look_pitch == -10.0f);
    CHECK(std::sqrt(in.move_x * in.move_x + in.move_y * in.move_y) == doctest::Approx(1.0f));
  }

  TEST_CASE("a heavy cluster hitting the player pushes it") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.DynamicBox(Vec3(-2.5f, 0.9f, 0.5f), Vec3::sReplicate(0.5f), 2000.0f, Vec3(10, 0, 0));
    w.Step(30);
    CHECK(w.Pos(e).GetX() > 1.0f);
  }
}

TEST_SUITE("player: probes") {
  TEST_CASE("standing on the floor is grounded on terrain with an up normal") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.Step(2);
    const auto& g = w.C(e).ground;
    CHECK(g.grounded);
    CHECK(g.ground.kind == GroundRef::kTerrain);
    CHECK(g.slope_angle < 1.0f);
    CHECK(g.gap == doctest::Approx(0.0f).epsilon(0.01));
  }

  TEST_CASE("floating high above the floor is not grounded") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 3, 0.5f));
    w.Step(2);
    CHECK_FALSE(w.C(e).ground.grounded);
  }

  TEST_CASE("too steep ground is not walkable") {
    PlayerTestWorld w;
    w.Box(Vec3(0, -1, 0), Vec3(5, 0.5f, 5), Euler(0, 0, 60));
    const auto e = w.Spawn(Vec3(0, 0.5f, 0));
    w.Step(2);
    const auto& g = w.C(e).ground;
    CHECK_FALSE(g.grounded);
    CHECK(g.ground.kind == GroundRef::kTier1Body);
    CHECK(g.slope_angle == doctest::Approx(60.0f).epsilon(0.05));
  }

  TEST_CASE("a low voxel ceiling is detected") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-2, 2, -2, 2, 2, 2, core::Materials::kStone);  // underside at 2.0
    const auto e = w.Spawn(Vec3(0.5f, 0.15f, 0.5f));      // head at 1.95
    w.Step(1);
    CHECK(w.C(e).ground.ceiling_blocked);
  }

  TEST_CASE("an adjacent voxel wall is detected") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(1, 0, -3, 1, 3, 3, core::Materials::kStone);  // face at x = 1
    const auto e = w.Spawn(Vec3(0.65f, 0, 0.5f));
    w.Step(2);
    const auto& g = w.C(e).ground;
    CHECK(g.touching_wall);
    CHECK(g.wall_normal.GetX() == doctest::Approx(-1.0f));
  }
}

TEST_SUITE("player: walking") {
  TEST_CASE("walk accelerates to walk speed forward") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, -8));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(3);
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(2.5f).epsilon(0.05));  // 50 m/s² for 0.05 s
    w.Step(Ticks(1.0f));
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(5.0f).epsilon(0.01));
    CHECK(w.Pos(e).GetZ() > -3.5f);
    CHECK(w.C(e).state == State::kWalking);
  }

  TEST_CASE("run reaches run speed") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, -12));
    w.input = [](int, PlayerHandle) { return Move(0, 1, true); };
    w.Step(Ticks(1.0f));
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(8.0f).epsilon(0.01));
    CHECK(w.C(e).state == State::kRunning);
  }

  TEST_CASE("every direction moves at the same speed, walking and running") {
    // Playtest finding: forward/back must not outpace strafing. Measures distance covered over
    // two seconds of steady input, so probes, steps and wall rays count as well as velocity.
    const Input dirs[] = {Move(0, 1), Move(0, -1), Move(1, 0), Move(-1, 0), Move(0.7071f, 0.7071f)};
    for (const bool run : {false, true}) {
      for (const float yaw : {0.0f, 37.0f, 90.0f}) {
        float distances[5];
        for (int d = 0; d < 5; ++d) {
          PlayerTestWorld w;
          w.Floor(0, 40);
          const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
          Input in = dirs[d];
          in.run = run;
          in.look_yaw = yaw;
          w.input = [in](int, PlayerHandle) { return in; };
          w.Step(Ticks(0.5f));  // reach steady speed
          const Vec3 start = w.Pos(e);
          w.Step(Ticks(2.0f));
          const Vec3 moved = w.Pos(e) - start;
          distances[d] = std::sqrt(moved.GetX() * moved.GetX() + moved.GetZ() * moved.GetZ());
        }
        const float expected = 2.0f * (run ? player::DefaultConfig().movement.run_speed
                                           : player::DefaultConfig().movement.walk_speed);
        for (int d = 0; d < 5; ++d) {
          INFO("run=" << run << " yaw=" << yaw << " direction " << d);
          CHECK(distances[d] == doctest::Approx(expected).epsilon(0.01));
        }
      }
    }
  }

  TEST_CASE("movement is relative to camera yaw") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(-8, 0, 0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1, false, false, false, 90); };
    w.Step(Ticks(1.0f));
    CHECK(w.Pos(e).GetX() > -4.0f);
    CHECK(w.Pos(e).GetZ() == doctest::Approx(0.5f).epsilon(0.1));
  }

  TEST_CASE("releasing input decelerates to a stop") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, -12));
    w.input = [](int t, PlayerHandle) { return t < Ticks(1.0f) ? Move(0, 1) : Move(0, 0); };
    w.Step(Ticks(1.0f) + Ticks(0.5f));  // 12 m/s² stops 5 m/s in ~0.42 s
    CHECK(w.HorizontalSpeed(e) < 0.05f);
    CHECK(w.C(e).state == State::kIdle);
  }

  TEST_CASE("reversing brakes faster than releasing") {
    auto speed_after_braking = [](Input brake) {
      PlayerTestWorld w;
      w.Floor();
      const auto e = w.Spawn(Vec3(0.5f, 0, -12));
      w.input = [&](int t, PlayerHandle) { return t < Ticks(1.0f) ? Move(0, 1) : brake; };
      w.Step(Ticks(1.0f) + Ticks(0.1f));
      return w.Vel(e).GetZ();
    };
    const float released = speed_after_braking(Move(0, 0));  // ~3.8 m/s left
    const float reversed = speed_after_braking(Move(0, -1));
    CHECK(reversed < released - 0.5f);
  }

  TEST_CASE("a voxel wall blocks movement") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 0, 3, 3, 3, 3, core::Materials::kStone);  // face at z = 3
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(2.0f));
    CHECK(w.Pos(e).GetZ() == doctest::Approx(2.7f).epsilon(0.02));
  }

  TEST_CASE("diagonal input is not faster") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(-8, 0, -8));
    w.input = [](int, PlayerHandle) { return Move(1, 1); };
    w.Step(Ticks(1.0f));
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(5.0f).epsilon(0.01));
  }
}

TEST_SUITE("player: steps and air") {
  TEST_CASE("walks up a slab step without leaving the ground") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 0, 2, 3, 0, 12, core::Materials::kStoneSlab);  // 0.5 m step from z = 2
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    int airborne = 0;
    for (int i = 0; i < Ticks(1.5f); ++i) {
      w.Step();
      if (!w.C(e).ground.grounded) ++airborne;
    }
    CHECK(w.Pos(e).GetZ() > 4.0f);
    CHECK(w.Feet(e) == doctest::Approx(0.5f).epsilon(0.03));
    CHECK(airborne == 0);
  }

  TEST_CASE("a full block needs a jump") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 0, 2, 3, 0, 12, core::Materials::kStone);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(1.5f));
    CHECK(w.Pos(e).GetZ() < 1.75f);
    CHECK(w.Feet(e) < 0.05f);
  }

  TEST_CASE("stepping up never moves the player faster than its speed") {
    // Playtest bug: jump towards a block while holding forward and the player shot forward as it
    // came down on the block's edge. The step-up there (and on every slab) moved the capsule
    // forward by its nudge on top of the tick's own movement. Measures the distance moved per
    // tick for every jump timing, from a running start to pressed against the block.
    const auto& m = player::DefaultConfig().movement;
    for (const bool run : {false, true}) {
      CAPTURE(run);
      const float limit = (run ? m.run_speed : m.walk_speed) / 60.0f * 1.02f;
      float worst = 0.0f;
      int worst_jump = -1, onto = 0;
      for (int jump_tick = 0; jump_tick <= 60; jump_tick += 2) {
        PlayerTestWorld w;
        w.Floor(0, 40);
        w.Fill(-3, 0, 3, 3, 0, 20, core::Materials::kStone);
        const auto e = w.Spawn(Vec3(0.5f, 0, 0));
        w.input = [&](int t, PlayerHandle) {
          return Move(0, 1, run, t >= jump_tick && t < jump_tick + 3);
        };
        Vec3 prev = w.Pos(e);
        for (int t = 0; t < jump_tick + Ticks(1.5f); ++t) {
          w.Step(1);
          const Vec3 p = w.Pos(e);
          const float moved = std::sqrt(Sq(p.GetX() - prev.GetX()) + Sq(p.GetZ() - prev.GetZ()));
          if (moved > worst) {
            worst = moved;
            worst_jump = jump_tick;
          }
          prev = p;
        }
        onto += w.Feet(e) > 0.95f;  // made it onto the block (early jumps land short)
      }
      INFO("worst step " << worst * 60.0f << " m/s, jumping at tick " << worst_jump);
      CHECK(onto > 10);
      CHECK(worst <= limit);
    }
    // Walking up a slab step.
    PlayerTestWorld w;
    w.Floor(0, 40);
    w.Fill(-3, 0, 3, 3, 0, 20, core::Materials::kStoneSlab);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    Vec3 prev = w.Pos(e);
    float worst = 0.0f;
    for (int t = 0; t < Ticks(1.5f); ++t) {
      w.Step(1);
      worst = std::max(worst, w.Pos(e).GetZ() - prev.GetZ());
      prev = w.Pos(e);
    }
    CHECK(w.Feet(e) > 0.45f);
    CHECK(worst * 60.0f <= m.walk_speed * 1.02f);
  }

  TEST_CASE("air control scales airborne acceleration") {
    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3(0, 20, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(0.25f));
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(2.5f).epsilon(0.05));  // 0.2 × 50 m/s² × 0.25 s
  }
}

TEST_SUITE("player: external forces") {
  TEST_CASE("a ground kick is absorbed, then decays by friction") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(-8, 0, 0.5f));
    w.Kick(5, e, Vec3(6, 0, 0));
    w.Step(8);
    const float after_kick = w.Vel(e).GetX();
    w.Step(Ticks(0.5f));
    CHECK(after_kick > 4.0f);
    CHECK(after_kick < 6.1f);
    CHECK(w.Vel(e).GetX() < 0.1f);  // 15 m/s² stops it within ~0.4 s
    CHECK(w.Pos(e).GetX() > -7.5f);
  }

  TEST_CASE("an air kick keeps its momentum (no air drag by default)") {
    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3(0, 30, 0));
    w.Kick(5, e, Vec3(6, 0, 0));
    w.Step(8 + 60);
    CHECK(w.Vel(e).GetX() == doctest::Approx(6.0f).epsilon(0.01));
  }

  TEST_CASE("air drag decays an air kick (Unity parity preset)") {
    PlayerTestWorld w(player::UnityParityConfig());
    const auto e = w.Spawn(Vec3(0, 30, 0));
    w.Kick(5, e, Vec3(6, 0, 0));
    w.Step(8 + 60);
    CHECK(w.Vel(e).GetX() == doctest::Approx(6.0f * std::exp(-0.5f)).epsilon(0.08));
  }
}
