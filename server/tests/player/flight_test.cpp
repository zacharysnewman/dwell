// Creative flight (PLAYER_CONTROLLER.md §6.7): an exclusive controller mode while the fly input
// is held. The server's flight policy is in netcode_test.cpp.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include "player_test_world.h"

using namespace dwell;
using namespace dwell::test;
namespace Ev = player::Events;

namespace {

Input Fly(float x = 0, float y = 0, bool run = false, bool up = false, bool down = false) {
  Input i = Move(x, y, run, up, down);
  i.fly = true;
  return i;
}

}  // namespace

TEST_SUITE("player: flight") {
  TEST_CASE("flying holds the player in the air; letting go falls") {
    PlayerTestWorld w;
    w.Floor(0);
    const auto e = w.Spawn(Vec3(0, 10, 0));
    w.input = [](int t, PlayerHandle) { return t < 60 ? Fly() : Move(0, 0); };
    w.Step(60);
    CHECK(w.C(e).fly.flying);
    CHECK(w.C(e).state == State::kFlying);
    CHECK(w.Count(e, Ev::kFlyStarted) == 1);
    CHECK(w.Feet(e) == doctest::Approx(10.0f).epsilon(0.01));
    w.Step(Ticks(2.0f));
    CHECK_FALSE(w.C(e).fly.flying);
    CHECK(w.Count(e, Ev::kFlyEnded) == 1);
    CHECK(w.C(e).ground.grounded);
    CHECK(std::abs(w.Feet(e)) < 0.05f);
  }

  TEST_CASE("jump rises, crouch sinks, move flies along the camera's yaw") {
    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3(0, 8, 0));
    w.input = [](int, PlayerHandle) { return Fly(0, 0, false, true); };
    w.Step(Ticks(1.0f));
    CHECK(w.Feet(e) > 15.0f);
    CHECK(w.C(e).crouch.crouching == false);  // flight keeps the standing shape
    w.input = [](int, PlayerHandle) { return Fly(0, 0, false, false, true); };
    const float top = w.Feet(e);
    w.Step(Ticks(0.5f));
    CHECK(w.Feet(e) < top - 3.0f);
    w.input = [](int, PlayerHandle) { return Fly(0, 1); };  // yaw 0: +Z
    w.Step(Ticks(1.0f));
    CHECK(w.Vel(e).GetZ() > 10.0f);
    CHECK(std::abs(w.Vel(e).GetY()) < 0.5f);
  }

  TEST_CASE("speed grows with height; the terrain band has a cap; the sky has a ceiling") {
    const auto cfg = player::DefaultConfig();
    auto speed_at = [](float feet) {
      PlayerTestWorld w;
      const auto e = w.Spawn(Vec3(0, feet, 0));
      w.input = [](int, PlayerHandle) { return Fly(0, 1, true); };
      w.Step(Ticks(1.5f));
      return w.HorizontalSpeed(e);
    };
    const float low = speed_at(4);
    CHECK(low ==
          doctest::Approx(cfg.fly.speed * cfg.fly.run_factor * (1 + 4 / cfg.fly.boost_height))
              .epsilon(0.05));
    CHECK(speed_at(3000) == doctest::Approx(cfg.fly.terrain_speed).epsilon(0.02));
    CHECK(speed_at(20000) > 10'000.0f);  // above the terrain: exponential climb

    PlayerTestWorld w;
    const auto e = w.Spawn(Vec3(0, cfg.fly.ceiling - 1000.0f, 0));
    w.input = [](int, PlayerHandle) { return Fly(0, 0, true, true); };
    w.Step(Ticks(2.0f));
    CHECK(w.Feet(e) <= cfg.fly.ceiling + 4.0f);  // float feet: 2 m steps up here
    CHECK(w.Feet(e) >= cfg.fly.ceiling - 4.0f);
  }

  TEST_CASE("diving at the band's top speed stops on the ground, not through it") {
    PlayerTestWorld w;
    w.Floor(0);
    const auto e = w.Spawn(Vec3(0, 600, 0));
    w.input = [](int, PlayerHandle) { return Fly(0, 0, true, false, true); };
    w.Step(Ticks(4.0f));
    CHECK(std::abs(w.Feet(e)) < 0.05f);
    CHECK(w.C(e).fly.flying);
  }
}
