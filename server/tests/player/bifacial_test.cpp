// The controller on the bifacial world (BIFACIAL_WORLD.md §3, §4, §6; PLAYER_CONTROLLER.md §11):
// down is toward the midplane, so a face-B player stands upside down; the flip band at the
// midplane; crossing between the faces. Geometry is given in world coordinates (these tests are
// about the faces, so they ignore --dwell-face; the controller suites run mirrored under it).
#include <doctest/doctest.h>

#include <cmath>
#include <vector>

#include "player_test_world.h"

using namespace dwell;
using namespace dwell::test;
using core::kMidplaneY;
using core::Materials::kStone;
using player::PlayerController;

namespace {

// A player on face B hangs under a slab whose lower face is at y = −4,098.
constexpr int kSlabBottom = -4098;

void BuildSlab(PlayerTestWorld& w) {
  for (int z = -8; z < 8; ++z)
    for (int x = -8; x < 8; ++x)
      for (int y = kSlabBottom; y < kSlabBottom + 2; ++y) w.world.SetVoxel(x, y, z, kStone);
}

// A 9 × 9 block of rock across the midplane (y −2,064 … −2,033) with a 1 × 1 shaft through it.
void BuildShaft(PlayerTestWorld& w) {
  for (int z = -4; z <= 4; ++z)
    for (int x = -4; x <= 4; ++x) {
      if (x == 0 && z == 0) continue;
      for (int y = -2064; y <= -2033; ++y) w.world.SetVoxel(x, y, z, kStone);
    }
}

// These tests place everything in world coordinates; the mirrored local frame of --dwell-face=b
// (test_origin.h) would only get in the way.
struct WorldFrame {
  WorldFrame() : saved(FaceB()) { FaceB() = false; }
  ~WorldFrame() { FaceB() = saved; }
  bool saved;
};

}  // namespace

TEST_SUITE("player: bifacial") {
#define WORLD_FRAME const WorldFrame frame_guard
  TEST_CASE("a face-B player stands upside down under the ground, grounded, feet above its head") {
    WORLD_FRAME;
    PlayerTestWorld w;
    BuildSlab(w);
    const auto h = w.SpawnAt(JPH::RVec3(0.5, kSlabBottom, 0.5));
    CHECK(w.C(h).face == -1);
    w.Step(60);
    CHECK(w.C(h).ground.grounded);
    CHECK(w.C(h).state == player::State::kIdle);
    // The capsule hangs from the slab: feet at its lower face, the head 1.8 m below.
    CHECK(w.players.Feet(h) == doctest::Approx(kSlabBottom).epsilon(1e-5));
    CHECK(w.players.Head(h) == doctest::Approx(kSlabBottom - 1.8f).epsilon(1e-5));
    CHECK(w.players.Position(h).GetY() == doctest::Approx(kSlabBottom - 0.9));
  }

  TEST_CASE("a face-B player falls toward the midplane (up) and lands on a floor above") {
    WORLD_FRAME;
    PlayerTestWorld w;
    // A floor of rock at y −3,000: a player under it, 20 m below, falls up and lands on it (head
    // pointing away from it).
    for (int z = -8; z < 8; ++z)
      for (int x = -8; x < 8; ++x) w.world.SetVoxel(x, -3000, z, kStone);
    const double y0 = -3000.0 - 20.0;  // feet 20 m below the floor's lower face (at y = −3,000)
    const auto h = w.SpawnAt(JPH::RVec3(0.5, y0, 0.5));
    CHECK(w.C(h).face == -1);
    double highest = y0;
    for (int i = 0; i < 240 && !w.C(h).ground.grounded; ++i) {
      w.Step();
      highest = std::max(highest, w.players.Position(h).GetY());
      CHECK(w.players.Velocity(h).GetY() >= -1e-4f);  // never away from the midplane
    }
    CHECK(w.C(h).ground.grounded);
    CHECK(w.Count(h, player::Events::kLanded) == 1);
    // Free fall of 20 m at 20 m/s²: ~28 m/s on landing, as on face A.
    CHECK(w.C(h).landed_speed == doctest::Approx(std::sqrt(2.0f * 20.0f * 20.0f)).epsilon(0.05));
    CHECK(w.players.Feet(h) == doctest::Approx(-3000.0).epsilon(1e-4));
  }

  TEST_CASE("jumping on face B rises toward −y by the same height, then falls back") {
    WORLD_FRAME;
    PlayerTestWorld w;
    BuildSlab(w);
    const auto h = w.SpawnAt(JPH::RVec3(0.5, kSlabBottom, 0.5));
    w.Step(30);
    REQUIRE(w.C(h).ground.grounded);
    const float start = w.players.Feet(h);
    const int t0 = w.tick;
    w.input = [t0](int tick, player::PlayerHandle) { return Move(0, 0, false, tick - t0 < 3); };
    float lowest = start;
    for (int i = 0; i < 90; ++i) {
      w.Step();
      lowest = std::min(lowest, w.players.Feet(h));
    }
    CHECK(start - lowest == doctest::Approx(w.config.jump.height).epsilon(0.05));
    CHECK(w.C(h).ground.grounded);
  }

  TEST_CASE("strafing right is toward the player's right on either face") {
    WORLD_FRAME;
    // Facing +Z (yaw 0) an upright player's right is −X; an upside-down player's, +X.
    for (const bool face_b : {false, true}) {
      CAPTURE(face_b);
      PlayerTestWorld w;
      double dx = 0;
      if (face_b) {
        BuildSlab(w);
      } else {
        for (int z = -8; z < 8; ++z)
          for (int x = -8; x < 8; ++x) w.world.SetVoxel(x, -1, z, kStone);
      }
      const auto h = w.SpawnAt(JPH::RVec3(0.5, face_b ? kSlabBottom : 0.0, 0.5));
      w.Step(30);
      const double x0 = w.players.Position(h).GetX();
      w.input = [](int, player::PlayerHandle) { return Move(1, 0, false, false, false, 0.0f); };
      w.Step(30);
      dx = w.players.Position(h).GetX() - x0;
      CHECK(std::abs(dx) > 1.0);
      const bool toward_minus_x = dx < 0.0;
      CHECK(toward_minus_x == !face_b);
    }
  }

  TEST_CASE("the flip band: a body falling down a shaft is accelerated across the midplane") {
    WORLD_FRAME;
    for (const double side : {+1.0, -1.0}) {
      CAPTURE(side);
      PlayerTestWorld w;
      BuildShaft(w);
      const auto h = w.SpawnAt(JPH::RVec3(0.5, kMidplaneY + side * 20.0, 0.5));
      CHECK(w.C(h).face == (side > 0 ? 1 : -1));
      int flips = 0;
      std::int8_t face = w.C(h).face;
      float fastest_in_band = 0.0f;
      for (int i = 0; i < 60 * 4; ++i) {
        w.Step();
        if (std::abs(w.players.Position(h).GetY() - kMidplaneY) < 4.0) {
          fastest_in_band = std::max(fastest_in_band, std::abs(w.players.Velocity(h).GetY()));
        }
        if (w.C(h).face != face) {
          ++flips;
          face = w.C(h).face;
        }
      }
      // Free fall over 20 m reaches 28 m/s; the band's acceleration adds to it.
      CHECK(flips >= 1);
      CHECK(fastest_in_band > 30.0f);
    }
  }

  TEST_CASE("crossing the midplane by swimming: the face switches, and up is away from it") {
    WORLD_FRAME;
    PlayerTestWorld w;
    BuildShaft(w);
    const auto h = w.SpawnAt(JPH::RVec3(0.5, kMidplaneY + 6.0, 0.5));
    // Press down (crouch: toward the midplane) until across it; on the far side "down" points
    // back, so the player presses up (jump) to go on — away from the midplane.
    REQUIRE(w.C(h).face == 1);
    w.input = [&](int, player::PlayerHandle p) {
      const bool across = w.C(p).face < 0;
      return Move(0, 0, false, across, !across);
    };
    int flips = 0;
    std::int8_t face = w.C(h).face;
    double lowest = kMidplaneY;
    for (int i = 0; i < 60 * 3; ++i) {
      w.Step();
      if (w.C(h).face != face) {
        ++flips;
        face = w.C(h).face;
      }
      lowest = std::min(lowest, w.players.Position(h).GetY());
    }
    // Out of the band the pull is back toward the midplane, so the player may come back across.
    CHECK(flips >= 1);
    // On, into the far side of the band (3 m/s for the best part of three seconds).
    CHECK(lowest < kMidplaneY - 3.0);
  }

  TEST_CASE("crossing keeps the motion: the layers' state is mirrored with the frame") {
    WORLD_FRAME;
    PlayerTestWorld w;
    BuildShaft(w);
    const auto h = w.SpawnAt(JPH::RVec3(0.5, kMidplaneY + 0.5, 0.5));
    w.Step(1);
    w.players.AddVelocity(h, JPH::Vec3(0, -20.0f, 0));
    float before = 0, after = 0;
    std::int8_t face = w.C(h).face;
    for (int i = 0; i < 60 * 2 && after == 0.0f; ++i) {
      before = w.players.Velocity(h).GetY();
      w.Step();
      if (w.C(h).face != face) after = w.players.Velocity(h).GetY();
    }
    REQUIRE(after != 0.0f);
    // Still moving the same way, at about the same speed: not reversed, not lost, by the switch.
    CHECK(before < 0.0f);
    CHECK(after < 0.0f);
    CHECK(std::abs(after - before) < 1.5f);
  }
}
