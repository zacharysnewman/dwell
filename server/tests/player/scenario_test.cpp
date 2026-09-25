// Ported PPC suites CharacterStacking, GroundedConsistency, StepSmoothness, Phase8Hardening and
// GoldenTrace on voxel geometry, plus Dwell additions (doorways, crawlspaces, auto-jump, edge
// guard, voxel edits under the player, chunk seams, collision meshes).
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>
#include <string>

#include "player_test_world.h"
#include "scenario.h"

using namespace dwell;
using namespace dwell::test;
using player::GroundRef;
namespace Ev = player::Events;

TEST_SUITE("player: stacking") {
  // Player 0 stands on the floor; player 1 on player 0's head. Player 0 strafes right (−X) from
  // t = 30.
  struct Stack {
    PlayerTestWorld w;
    PlayerHandle bottom, top;
    explicit Stack(bool carried) : w(Config(carried)) {
      w.Floor();
      bottom = w.Spawn(Vec3(0.5f, 0, 0.5f));
      top = w.Spawn(Vec3(0.5f, 1.8f, 0.5f));
      w.input = [this](int t, PlayerHandle h) {
        return h == bottom && t >= 30 ? Move(1, 0) : Move(0, 0);
      };
    }
    static player::PlayerControllerConfig Config(bool carried) {
      player::PlayerControllerConfig c;
      c.movement.carried_by_characters = carried;
      return c;
    }
  };

  TEST_CASE("standing on a head is grounded on that player") {
    Stack s(false);
    s.w.Step(25);
    CHECK(s.w.C(s.top).ground.grounded);
    CHECK(s.w.C(s.top).ground.ground.kind == GroundRef::kPlayer);
    CHECK(s.w.Feet(s.top) == doctest::Approx(1.8f).epsilon(0.02));
  }

  TEST_CASE("the player below walks out from under you") {
    Stack s(false);
    s.w.Step(30 + 30);
    CHECK(s.w.Pos(s.bottom).GetX() < 0.0f);
    CHECK(s.w.Pos(s.top).GetX() == doctest::Approx(0.5f).epsilon(0.1));
  }

  TEST_CASE("opting in carries you along") {
    Stack s(true);
    s.w.Step(30 + 30);
    CHECK(s.w.Pos(s.top).GetX() < 0.0f);
  }
}

TEST_SUITE("player: grounded consistency") {
  TEST_CASE("state is Jumping from the take-off tick") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t == 10); };
    w.Step(10);
    CHECK(w.Count(e, Ev::kJumped) == 0);
    w.Step(1);
    CHECK(w.Count(e, Ev::kJumped) == 1);
    CHECK_FALSE(w.C(e).ground.grounded);
    CHECK(w.C(e).state == State::kJumping);
  }

  TEST_CASE("jumping off a moving platform stops its carry at once") {
    PlayerTestWorld w;
    w.Box(Vec3(0, -0.5f, 0), Vec3(20, 0.5f, 20), JPH::Quat::sIdentity(), Vec3(3, 0, 0));
    const auto e = w.Spawn(Vec3::sZero());
    w.input = [](int t, PlayerHandle) { return Move(0, 0, false, t == 60); };
    w.Step(61);  // take-off tick: the jump inherits the platform's motion
    CHECK(w.C(e).platform.base_velocity.GetX() == doctest::Approx(3.0f).epsilon(0.03));
    w.Step(1);
    CHECK(w.C(e).platform.base_velocity.Length() == 0.0f);
  }
}

TEST_SUITE("player: step smoothness") {
  struct Result {
    bool arrived = false;
    float seconds = 0, max_rise = 0, max_drop = 0, final_feet = 0;
    int airborne = 0;
  };

  // `steps` slab-height (0.5 m) steps of `tread` cells along +Z from z = 2, then a landing. Up:
  // from the floor walking +Z; down: from the landing walking −Z.
  Result RunStairs(int steps, int tread, bool up, bool run, bool slabs = true) {
    PlayerTestWorld w;
    w.Floor(0, 24);
    const float rise = slabs ? 0.5f : 1.0f;
    for (int i = 0; i < steps; ++i) {
      for (int z = 2 + tread * i; z < 2 + tread * steps + 4; ++z) {
        // Step i is i + 1 half-cells high: full cubes below, a slab on top when odd.
        const int halves = slabs ? i + 1 : 2 * (i + 1);
        for (int h = 0; h < halves / 2; ++h) w.Fill(-3, h, z, 3, h, z, core::Materials::kStone);
        if (halves % 2) w.Fill(-3, halves / 2, z, 3, halves / 2, z, core::Materials::kStoneSlab);
      }
    }
    const float top = rise * static_cast<float>(steps);
    const float end = 2.0f + static_cast<float>(tread * steps);
    const float start_z = up ? 0.0f : end + 1.5f;
    const float target_z = up ? end + 1.0f : 0.5f;
    bool arrived = false;
    const auto e = w.Spawn(Vec3(0.5f, up ? 0.0f : top, start_z));
    w.input = [&](int, PlayerHandle) {
      return arrived ? Move(0, 0) : Move(0, up ? 1.0f : -1.0f, run);
    };
    w.Step(3);
    Result r;
    float previous = w.Feet(e);
    for (int tick = 1; tick <= 240 && !arrived; ++tick) {
      w.Step();
      const float feet = w.Feet(e);
      r.max_rise = std::max(r.max_rise, feet - previous);
      r.max_drop = std::max(r.max_drop, previous - feet);
      previous = feet;
      if (!w.C(e).ground.grounded) ++r.airborne;
      const float z = w.Pos(e).GetZ();
      if (up ? z >= target_z : z <= target_z) {
        arrived = r.arrived = true;
        r.seconds = static_cast<float>(tick) / 60.0f;
      }
    }
    w.Step(60);
    r.final_feet = w.Feet(e);
    return r;
  }

  TEST_CASE("slab stairs up and down, walking and running, stay grounded at full speed") {
    for (bool up : {true, false}) {
      for (bool run : {false, true}) {
        CAPTURE(up);
        CAPTURE(run);
        const Result r = RunStairs(4, 1, up, run);
        const std::string name = std::string(up ? "up" : "down") + (run ? ", run" : ", walk");
        MESSAGE("slab stairs " << name << ": " << r.seconds << " s, largest one-tick rise "
                               << r.max_rise << " / drop " << r.max_drop << " m, airborne "
                               << r.airborne);
        CHECK(r.arrived);
        CHECK(r.airborne == 0);
        CHECK(r.final_feet == doctest::Approx(up ? 2.0f : 0.0f).epsilon(0.01));
        const float distance = up ? 2.0f + 4.0f + 1.0f : 4.0f + 1.5f + 1.5f;
        CHECK(r.seconds <= distance / (run ? 8.0f : 5.0f) + 0.2f);
      }
    }
  }

  TEST_CASE("stepping down a stair's corner gains no momentum") {
    // Diagonally down the playground's slab stairs: the ground snap drives the capsule into the
    // corner of the step being left, and the contact's sideways push-out is not a kick.
    for (float x : {-9.5f, -8.5f, -7.2f}) {
      for (float yaw : {160.0f, 180.0f, 200.0f, 220.0f}) {
        for (bool run : {false, true}) {
          CAPTURE(x);
          CAPTURE(yaw);
          CAPTURE(run);
          PlayerTestWorld w(player::DefaultConfig(), core::GeneratePlaygroundChunk);
          const auto e = w.Spawn(Vec3(x, 2.0f, 10.5f), yaw);
          w.input = [&](int, PlayerHandle) { return Move(0, 1, run, false, false, yaw); };
          float external = 0.0f, driven = 0.0f;
          for (int i = 0; i < 90; ++i) {
            w.Step();
            external = std::max(external, w.C(e).horizontal.external.Length());
            driven = std::max(driven, w.C(e).horizontal.contribution.Length());
          }
          CHECK(external < 0.05f);
          CHECK(driven <= (run ? 8.0f : 5.0f) + 0.01f);
        }
      }
    }
  }

  TEST_CASE("a 1 m ledge is a fall, not a step") {
    const Result r = RunStairs(1, 3, /*up=*/false, /*run=*/false, /*slabs=*/false);
    CHECK(r.airborne > 5);
    CHECK(r.final_feet == doctest::Approx(0.0f).epsilon(0.01));
  }
}

TEST_SUITE("player: voxel geometry") {
  TEST_CASE("fits through a 1×2 doorway") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 0, 4, 3, 3, 4, core::Materials::kStone);
    w.Fill(0, 0, 4, 0, 1, 4, core::Materials::kAir);  // doorway at x = 0
    const auto e = w.Spawn(Vec3(0.5f, 0, 0));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(2.0f));
    CHECK(w.Pos(e).GetZ() > 7.0f);
    CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.01));
  }

  TEST_CASE("a 1-wide, 2-tall doorway still blocks a player aimed at its frame") {
    PlayerTestWorld w;
    w.Floor();
    w.Fill(-3, 0, 4, 3, 3, 4, core::Materials::kStone);
    w.Fill(0, 0, 4, 0, 1, 4, core::Materials::kAir);
    const auto e = w.Spawn(Vec3(1.2f, 0, 0));  // lined up with the wall beside the doorway
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(1.5f));
    CHECK(w.Pos(e).GetZ() < 3.75f);
  }

  TEST_CASE("walking across chunk seams in every direction never loses the ground") {
    PlayerTestWorld w;
    w.Floor(0, 40);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    // Diagonal and axis runs that cross x = ±32 and z = ±32 seams.
    w.input = [](int t, PlayerHandle) {
      const float yaw = static_cast<float>((t / 120) * 45);
      return Move(0, 1, true, false, false, yaw);
    };
    int airborne = 0;
    float max_lift = 0;
    for (int i = 0; i < 120 * 8; ++i) {
      w.Step();
      if (!w.C(e).ground.grounded) ++airborne;
      max_lift = std::max(max_lift, std::abs(w.Feet(e)));
    }
    CHECK(airborne == 0);
    CHECK(max_lift < 0.003f);  // sub-millimetre ripples at most
  }

  TEST_CASE("a block removed under the feet starts a fall") {
    PlayerTestWorld w;
    w.Floor(-10);
    w.Fill(0, -1, 0, 0, -1, 0, core::Materials::kStone);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.Step(10);
    CHECK(w.C(e).ground.grounded);
    w.Remove(0, -1, 0);
    w.Step(10);
    CHECK_FALSE(w.C(e).ground.grounded);
    CHECK(w.Vel(e).GetY() < -1.0f);
    w.Step(Ticks(2.0f));
    CHECK(w.Feet(e) == doctest::Approx(-10.0f).epsilon(0.002));
  }

  TEST_CASE("a block placed in front of the player is collided with in the same tick") {
    PlayerTestWorld w;
    w.Floor();
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(5);
    w.Fill(-2, 0, 3, 2, 2, 3, core::Materials::kStone);
    w.Step(Ticks(1.5f));
    CHECK(w.Pos(e).GetZ() == doctest::Approx(2.7f).epsilon(0.01));
  }

  TEST_CASE("auto-jump climbs a one-block step when enabled (touch preset)") {
    for (bool auto_jump : {false, true}) {
      PlayerTestWorld w(auto_jump ? player::TouchConfig() : player::DefaultConfig());
      w.Floor();
      w.Fill(-3, 0, 3, 3, 0, 10, core::Materials::kStone);
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [](int, PlayerHandle) { return Move(0, 1); };
      w.Step(Ticks(2.0f));
      if (auto_jump) {
        CHECK(w.Feet(e) == doctest::Approx(1.0f).epsilon(0.01));
        CHECK(w.Pos(e).GetZ() > 5.0f);
      } else {
        CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.01));
      }
    }
  }

  TEST_CASE("auto-jump does not jump at a two-block wall") {
    PlayerTestWorld w(player::TouchConfig());
    w.Floor();
    w.Fill(-3, 0, 3, 3, 1, 3, core::Materials::kStone);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    w.Step(Ticks(1.5f));
    CHECK(w.Count(e, Ev::kJumped) == 0);
  }

  TEST_CASE("edge guard keeps a crouched player on a ledge") {
    for (bool guard : {false, true}) {
      player::PlayerControllerConfig config;
      config.movement.edge_guard = guard;
      PlayerTestWorld w(config);
      w.Floor(-5);
      w.Fill(-4, -1, -4, 3, -1, 2, core::Materials::kStone);  // ledge ends at z = 3
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [](int, PlayerHandle) { return Move(0, 1, false, false, true); };
      w.Step(Ticks(3.0f));
      if (guard) {
        CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.01));
        CHECK(w.Pos(e).GetZ() > 2.9f);  // leans out as far as the probe ring allows
        CHECK(w.Pos(e).GetZ() < 3.3f);
      } else {
        CHECK(w.Feet(e) < -4.0f);
      }
    }
  }
}

TEST_SUITE("player: collision mesh") {
  TEST_CASE("a lone cube has 6 outward faces; a buried cube has none") {
    core::VoxelWorld world(core::GenerateEmptyChunk);
    world.SetVoxel(1, 1, 1, core::Materials::kStone);
    auto mesh = core::BuildChunkMesh(world, {0, 0, 0});
    REQUIRE(mesh.triangles.size() == 12);
    for (const auto& t : mesh.triangles) {
      const Vec3 a(mesh.vertices[t.mIdx[0]]), b(mesh.vertices[t.mIdx[1]]),
          c(mesh.vertices[t.mIdx[2]]);
      const Vec3 normal = (b - a).Cross(c - a);
      const Vec3 centroid = (a + b + c) / 3.0f;
      CHECK(normal.Dot(centroid - Vec3::sReplicate(1.5f)) > 0.0f);  // points away from the cube
    }
    for (int x = 0; x <= 2; ++x)
      for (int y = 0; y <= 2; ++y)
        for (int z = 0; z <= 2; ++z) world.SetVoxel(x, y, z, core::Materials::kStone);
    mesh = core::BuildChunkMesh(world, {0, 0, 0});
    CHECK(mesh.triangles.size() == 9 * 6 * 2);  // only the outer faces of the 3³ block
  }

  TEST_CASE("slab faces: half-height sides, hidden between neighbouring slabs") {
    core::VoxelWorld world(core::GenerateEmptyChunk);
    world.SetVoxel(1, 1, 1, core::Materials::kStoneSlab);
    world.SetVoxel(2, 1, 1, core::Materials::kStoneSlab);
    const auto mesh = core::BuildChunkMesh(world, {0, 0, 0});
    CHECK(mesh.triangles.size() == 2 * 5 * 2);  // each slab: top, bottom, 3 sides
    float max_y = 0;
    for (const auto& v : mesh.vertices) max_y = std::max(max_y, v.y);
    CHECK(max_y == 1.5f);
  }

  TEST_CASE("faces across a chunk border use the neighbour chunk") {
    core::VoxelWorld world(core::GenerateEmptyChunk);
    world.SetVoxel(31, 0, 0, core::Materials::kStone);
    world.SetVoxel(32, 0, 0, core::Materials::kStone);
    CHECK(core::BuildChunkMesh(world, {0, 0, 0}).triangles.size() == 10);
    CHECK(core::BuildChunkMesh(world, {1, 0, 0}).triangles.size() == 10);
  }
}

// ---------------------------------------------------------------------------------------------
// Multi-player scenario (PPC Phase8Hardening / GoldenTrace): see scenario.h.

TEST_SUITE("player: scenario") {
  TEST_CASE("the four-player scenario is deterministic across runs") {
    const auto a = RecordTrace();
    const auto b = RecordTrace();
    REQUIRE(a.size() == static_cast<std::size_t>(kScenarioTicks * kScenarioPlayers));
    CHECK(a == b);
  }

  TEST_CASE("the four-player scenario matches the golden trace") {
    const auto actual = RecordTrace();
    const std::string path = DWELL_PLAYER_GOLDEN;
    if (const char* update = std::getenv("DWELL_UPDATE_GOLDEN");
        update && std::string(update) == "1") {
      std::ofstream out(path);
      for (const auto& line : actual) out << line << '\n';
      MESSAGE("golden trace written to " << path);
      return;
    }
    std::ifstream in(path);
    REQUIRE_MESSAGE(in.good(), "missing " << path << "; run with DWELL_UPDATE_GOLDEN=1");
    std::vector<std::string> expected;
    for (std::string line; std::getline(in, line);) expected.push_back(line);
    REQUIRE(expected.size() == actual.size());
    double worst = 0;
    std::string worst_line;
    for (std::size_t i = 0; i < expected.size(); ++i) {
      std::istringstream e(expected[i]), a(actual[i]);
      int et, at, es, as;
      unsigned ep, ap;
      double ex, ey, ez, ax, ay, az;
      e >> et >> ep >> ex >> ey >> ez >> es;
      a >> at >> ap >> ax >> ay >> az >> as;
      REQUIRE(et == at);
      REQUIRE(ep == ap);
      CHECK_MESSAGE(es == as,
                    "state differs at row " << i << ": " << expected[i] << " vs " << actual[i]);
      for (double d : {std::abs(ex - ax), std::abs(ey - ay), std::abs(ez - az)}) {
        if (d > worst) {
          worst = d;
          worst_line = expected[i] + "  vs  " + actual[i];
        }
      }
    }
    MESSAGE("max deviation from golden: " << worst << " m " << worst_line);
    CHECK(worst <= 0.001);
  }
}

TEST_SUITE("player: performance") {
  TEST_CASE("64 players' controller passes cost under 1 ms per tick (Release)") {
    constexpr int kPlayers = 64, kTicks = 300;
    PlayerTestWorld w;
    w.Floor(0, 32);
    for (int p = 0; p < kPlayers; ++p) {
      w.Spawn(Vec3(static_cast<float>(p % 8) * 3.0f - 12.0f, 0,
                   static_cast<float>(p / 8) * 3.0f - 12.0f));
    }
    w.input = ScenarioInput;
    w.Step(30);  // warm up (terrain collision built)
    double controller_ms = 0;
    for (int i = 0; i < kTicks; ++i) {
      for (PlayerHandle h : w.players.handles()) w.players.SetInput(h, ScenarioInput(w.tick, h));
      const auto start = std::chrono::steady_clock::now();
      w.players.Tick();
      controller_ms +=
          std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start)
              .count();
      w.physics.Step(1.0f / 60.0f);
      ++w.tick;
    }
    const double per_tick = controller_ms / kTicks;
    MESSAGE("64 players: controller passes " << per_tick << " ms per tick");
#ifdef NDEBUG
    CHECK(per_tick < 1.0);
#else
    CHECK(per_tick < 25.0);  // unoptimized build: catch pathological regressions only
#endif
  }
}
