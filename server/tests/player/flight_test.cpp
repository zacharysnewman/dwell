// Creative flight (PLAYER_CONTROLLER.md §6.7): an exclusive controller mode while the fly input
// is held. The server's flight policy is in netcode_test.cpp.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <cmath>

#include "dwell/player/net.h"
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

  TEST_CASE("flying around the outside: down through the midplane, on past it to face B and back") {
    PlayerTestWorld w;
    const auto e = w.SpawnAt(JPH::RVec3(0.0, core::kMidplaneY + 30.0, 0.0));
    REQUIRE(w.C(e).face == 1);
    // Down (toward the midplane) until across, then up in the new frame (away from it) for a while.
    w.input = [&](int, PlayerHandle p) {
      const bool across = w.C(p).face < 0;
      return Fly(0, 0, false, across, !across);
    };
    int flips = 0;
    std::int8_t face = w.C(e).face;
    double lowest = core::kMidplaneY;
    for (int i = 0; i < Ticks(6.0f); ++i) {
      w.Step();
      if (w.C(e).face != face) {
        ++flips;
        face = w.C(e).face;
      }
      lowest = std::min(lowest, w.players.Position(e).GetY());
      CHECK(w.C(e).fly.flying);
    }
    CHECK(flips >= 1);
    CHECK(lowest < core::kMidplaneY - 30.0);  // well out onto face B's side
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

  TEST_CASE("the speed slider's level is a true minimum speed") {
    const auto cfg = player::DefaultConfig();
    CHECK(player::FlySpeedFactor(0) == 1.0f);
    CHECK(player::FlySpeedFactor(1) == std::sqrt(2.0f));
    CHECK(player::FlySpeedFactor(20) == 1024.0f);
    const float top = player::FlySpeedFactor(protocol::kFlySpeedMaxLevel);
    CHECK(top == std::ldexp(std::sqrt(2.0f), 19));
    CHECK(player::FlySpeedFactor(255) == top);
    // The fastest level stays within the height-based factor at the ceiling (the body's speed
    // limit is set from that).
    CHECK(top <=
          1.0f + (cfg.fly.ceiling - static_cast<float>(core::kSeaLevel)) / cfg.fly.boost_height);

    auto speed_at = [](float feet, std::uint8_t level) {
      PlayerTestWorld w;
      const auto e = w.Spawn(Vec3(0, feet, 0));
      w.input = [level](int, PlayerHandle) {
        Input i = Fly(0, 1);
        i.fly_speed = level;
        return i;
      };
      w.Step(Ticks(1.5f));
      return w.HorizontalSpeed(e);
    };
    // A level slower than the height gives changes nothing (level 0 is the old behaviour).
    CHECK(speed_at(20000, 10) == speed_at(20000, 0));
    // Above the terrain band a higher level flies faster: level 30 is 11 m/s × 2^15.
    CHECK(speed_at(20000, 30) == doctest::Approx(cfg.fly.speed * 32768.0f).epsilon(0.05));
    // Near the ground a low level helps: level 8 is 11 m/s × 16 = 176 m/s.
    CHECK(speed_at(4, 8) == doctest::Approx(cfg.fly.speed * 16.0f).epsilon(0.05));
    // The terrain band's cap limits only the height-based speed: the level's minimum holds there
    // too (level 20 is 11 m/s × 1024, well over the cap)...
    CHECK(speed_at(100, 20) == doctest::Approx(cfg.fly.speed * 1024.0f).epsilon(0.05));
    CHECK(speed_at(3000, 20) == doctest::Approx(cfg.fly.speed * 1024.0f).epsilon(0.05));
    // ...while a level under the cap leaves the capped height-based speed as it was.
    CHECK(speed_at(3000, 8) == doctest::Approx(cfg.fly.terrain_speed).epsilon(0.02));
  }

  TEST_CASE("the speed level travels in the input's buttons and is capped") {
    Input i;
    i.fly = true;
    i.fly_speed = 17;
    CHECK(player::DequantizeInput(player::QuantizeInput(i, 1)).fly_speed == 17);
    CHECK(player::DequantizeInput(player::QuantizeInput(i, 1)).fly);
    i.fly_speed = 200;
    CHECK(player::DequantizeInput(player::QuantizeInput(i, 1)).fly_speed ==
          protocol::kFlySpeedMaxLevel);
    protocol::InputFrame f;
    f.buttons = protocol::InputButtons::kFlySpeed;  // level 63 on the wire
    CHECK(player::DequantizeInput(f).fly_speed == protocol::kFlySpeedMaxLevel);
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
