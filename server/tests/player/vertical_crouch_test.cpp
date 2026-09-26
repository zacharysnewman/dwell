// Ported PPC suites Phase3Vertical and Phase4Crouch on voxel geometry. Default config: jump
// velocity √(2 · 20 · 1.25) ≈ 7.07 m/s, apex 1.25 m.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include "player_test_world.h"

using namespace dwell;
using namespace dwell::test;
namespace Ev = player::Events;

TEST_SUITE("player: vertical") {
  TEST_CASE("falls and lands on the floor") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 3, 0.5f));
    w.Step(5);
    CHECK(w.C(e).state == State::kFalling);
    w.Step(Ticks(1.5f));
    CHECK(w.C(e).ground.grounded);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
    CHECK(w.C(e).state == State::kIdle);
    CHECK(w.Count(e, Ev::kLanded) == 1);
  }

  TEST_CASE("jump reaches the configured apex") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 5 && t < 8); };
    const float apex = w.MaxFeet(e, Ticks(1.5f));
    CHECK(apex == doctest::Approx(1.25f).epsilon(0.05));
    CHECK(w.Count(e, Ev::kJumped) == 1);
    CHECK(w.C(e).ground.grounded);
  }

  TEST_CASE("holding jump does not bunny hop") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 5); };
    w.Step(Ticks(3.0f));
    CHECK(w.Count(e, Ev::kJumped) == 1);
  }

  // Falling 1.25 m from rest lands at ~0.35 s (21 ticks); the buffer is 12 ticks.
  TEST_CASE("a press just before landing is buffered") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 1.25f, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 15 && t < 17); };
    w.Step(Ticks(1.0f));
    CHECK(w.Count(e, Ev::kJumped) == 1);
  }

  TEST_CASE("a press too early is forgotten") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 1.25f, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 3 && t < 5); };
    w.Step(Ticks(1.0f));
    CHECK(w.Count(e, Ev::kJumped) == 0);
  }

  // A ledge ending at z = 2 above a deep floor; the player walks off it at 5 m/s.
  struct Ledge {
    PlayerTestWorld w;
    PlayerHandle e;
    explicit Ledge(int jump_tick) {
      w.Fill(-4, -1, -12, 3, -1, 1, core::Materials::kStone);
      w.Floor(-20);
      e = w.Spawn(Vec3(0.5f, 0, -2));
      w.input = [jump_tick](int t, PlayerHandle) {
        return Move(0, 1, false, jump_tick >= 0 && t >= jump_tick && t < jump_tick + 2);
      };
    }
  };

  int TickWhenUngrounded() {
    Ledge l(-1);
    for (int i = 0; i < 300; ++i) {
      l.w.Step();
      if (!l.w.C(l.e).ground.grounded) return i;
    }
    return -1;
  }

  TEST_CASE("coyote time allows a jump just after walking off") {
    const int off = TickWhenUngrounded();
    REQUIRE(off > 0);
    Ledge l(off + 3);
    l.w.Step(off + 6);
    CHECK(l.w.Count(l.e, Ev::kJumped) == 1);
    CHECK(l.w.Vel(l.e).GetY() > 5.0f);
  }

  TEST_CASE("coyote time lasts 0.2 s (a press 0.18 s after walking off still jumps)") {
    const int off = TickWhenUngrounded();
    REQUIRE(off > 0);
    Ledge l(off + 11);
    l.w.Step(off + 14);
    CHECK(l.w.Count(l.e, Ev::kJumped) == 1);
    CHECK(l.w.Vel(l.e).GetY() > 3.0f);
  }

  TEST_CASE("coyote time expires") {
    const int off = TickWhenUngrounded();
    Ledge l(off + 14);
    for (int i = 0; i < off + 20; ++i) {
      l.w.Step();
    }
    CHECK(l.w.Count(l.e, Ev::kJumped) == 0);
  }

  TEST_CASE("a second press right after a jump is not a double jump") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) {
      return Move(0, 0, false, (t >= 5 && t < 7) || (t >= 12 && t < 14));
    };
    const float apex = w.MaxFeet(e, Ticks(1.5f));
    CHECK(w.Count(e, Ev::kJumped) == 1);
    CHECK(apex == doctest::Approx(1.25f).epsilon(0.05));
  }

  TEST_CASE("a voxel ceiling stops the jump") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 2, -3, 3, 2, 3, core::Materials::kStone);  // underside at 2.0; head at 1.8
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 5 && t < 8); };
    const float apex = w.MaxFeet(e, Ticks(1.0f));
    w.Step(Ticks(1.0f));
    CHECK(apex > 0.1f);
    CHECK(apex < 0.25f);
    CHECK(w.C(e).ground.grounded);
  }

  TEST_CASE("a launch-pad kick while grounded launches") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.Kick(5, e, Vec3(0, 15, 0));
    const float apex = w.MaxFeet(e, Ticks(2.0f));
    CHECK(apex == doctest::Approx(15.0f * 15.0f / 40.0f).epsilon(0.05));  // 5.6 m
  }

  TEST_CASE("a jump during a launch keeps the larger velocity") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 8 && t < 10); };
    w.Kick(5, e, Vec3(0, 15, 0));
    CHECK(w.MaxFeet(e, Ticks(2.0f)) > 5.0f);
  }

  TEST_CASE("walks up and back down a 20° ramp (rotated body) staying grounded") {
    PlayerTestWorld w;
    w.Floor();
    w.Box(Vec3(0.5f, 0, 6), Vec3(2, 0.5f, 5), Euler(-20, 0, 0));
    const auto e = w.Spawn(Vec3(0.5f, 0, -1));
    const int turn = Ticks(1.4f);
    w.input = [turn](int t, PlayerHandle) { return Move(0, t < turn ? 1.0f : -1.0f); };
    int ungrounded = 0;
    Vec3 at_start, at_end;  // on the ramp, before turning back
    for (int i = 0; i < Ticks(2.8f); ++i) {
      w.Step();
      if (!w.C(e).ground.grounded) ++ungrounded;
      if (i == Ticks(1.2f)) at_start = w.Pos(e);
      if (i == Ticks(1.35f)) at_end = w.Pos(e);
    }
    // Climbing at walk speed along the ground plan, rising at speed × tan(slope): measured by
    // displacement, since on a slope the step-up supplies part of the rise (not the body velocity).
    const Vec3 climbed = (at_end - at_start) / 0.15f;
    CHECK(climbed.GetZ() == doctest::Approx(5.0f).epsilon(0.03));
    CHECK(climbed.GetY() ==
          doctest::Approx(5.0f * std::tan(20.0f * 3.14159265f / 180.0f)).epsilon(0.15));
    CHECK(ungrounded == 0);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.03));
  }
}

TEST_SUITE("player: crouch") {
  TEST_CASE("crouching on the ground keeps the feet planted") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, false, t >= 5); };
    w.Step(20);
    CHECK(w.C(e).crouch.crouching);
    CHECK(w.players.HalfHeight(e) == doctest::Approx(0.45f));
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
    CHECK(w.C(e).state == State::kCrouching);
    CHECK(w.Count(e, Ev::kCrouchChanged) == 1);
  }

  TEST_CASE("releasing crouch stands back up") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, false, t >= 5 && t < 20); };
    w.Step(40);
    CHECK_FALSE(w.C(e).crouch.crouching);
    CHECK(w.players.HalfHeight(e) == doctest::Approx(0.9f));
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
  }

  TEST_CASE("crouch walking is slower") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, -8));
    w.input = [](int, PlayerHandle) { return Move(0, 1, false, false, true); };
    w.Step(Ticks(1.0f));
    CHECK(w.HorizontalSpeed(e) == doctest::Approx(1.6f).epsilon(0.02));
  }

  // A 1-tall crawlspace: roof blocks at y = 1 (underside 1.0 m) over z = 2..4.
  void Crawlspace(PlayerTestWorld & w) {
    w.Floor();
    w.Fill(-3, 1, 2, 3, 1, 4, core::Materials::kStone);
  }

  TEST_CASE("standing, the player cannot pass under a 1 m roof") {
    PlayerTestWorld w;
    Crawlspace(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(2.0f));
    CHECK(w.Pos(e).GetZ() < 1.75f);
  }

  TEST_CASE("crouched, the player crawls through, stays crouched under it, and stands once clear") {
    PlayerTestWorld w;
    Crawlspace(w);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    const int release = Ticks(2.4f);  // released while under the roof
    w.input = [release](int t, PlayerHandle) { return Move(0, 1, false, false, t < release); };
    bool stayed_crouched = true;
    for (int i = 0; i < Ticks(4.5f); ++i) {
      w.Step();
      const float z = w.Pos(e).GetZ();
      if (z > 2.35f && z < 4.65f && !w.C(e).crouch.crouching) stayed_crouched = false;
    }
    CHECK(w.Pos(e).GetZ() > 6.0f);
    CHECK(stayed_crouched);
    CHECK_FALSE(w.C(e).crouch.crouching);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
  }

  TEST_CASE("mid-air crouch tucks the legs up without moving the head") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 2 && t < 4, t >= 12); };
    w.Step(12);
    const float head_before = w.Head(e);
    w.Step(1);
    CHECK(w.C(e).crouch.crouching);
    const float head_step = w.Head(e) - head_before;
    CHECK(head_step > -0.01f);
    CHECK(head_step < 0.15f);
  }

  TEST_CASE("a crouch jump lifts the feet higher") {
    auto max_feet = [](bool crouch) {
      PlayerTestWorld w;
      w.Floor();
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [crouch](int t, PlayerHandle) {
        return Move(0, 0, false, t >= 2 && t < 4, crouch && t >= 10);
      };
      return w.MaxFeet(e, Ticks(1.0f));
    };
    CHECK(max_feet(true) > max_feet(false) + 0.8f);
  }

  TEST_CASE("mid-air boost adds height") {
    auto apex = [](float boost) {
      player::PlayerControllerConfig config;
      config.crouch.mid_air_boost = boost;
      PlayerTestWorld w(config);
      w.Floor();
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t >= 2 && t < 4, t >= 10); };
      return w.MaxFeet(e, Ticks(1.5f));
    };
    CHECK(apex(3.0f) > apex(0.0f) + 0.5f);
  }

  TEST_CASE("landing while crouched keeps a centred capsule, then stands from the feet") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    const int release = Ticks(2.0f);
    w.input = [release](int t, PlayerHandle) {
      return Move(0, 0, false, t >= 2 && t < 4, t >= 10 && t < release);
    };
    w.Step(release - 5);
    CHECK(w.C(e).crouch.crouching);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
    w.Step(20);
    CHECK_FALSE(w.C(e).crouch.crouching);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.02));
  }
}
