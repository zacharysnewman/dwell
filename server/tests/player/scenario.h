#pragma once

// The four-player scenario (PPC Phase8Hardening / GoldenTrace) on voxel geometry: a floor, ramp,
// slab step, crawlspace, ladder to a ledge, pool, a moving and turning platform, and an explosion.
// Used by the golden-trace test, the performance gate, and the native↔WASM divergence tool
// (server/tools/scenario_trace.cpp).
#include <cstdio>
#include <string>
#include <vector>

#include "player_test_world.h"

namespace dwell::test {

inline constexpr int kScenarioTicks = 600;
inline constexpr int kScenarioPlayers = 4;

// Floor, ramp, slab step, crawlspace roof, ladder to a ledge, pool, a moving and turning platform,
// and an explosion at tick 300.
inline void ScenarioWorld(PlayerTestWorld& w) {
  w.Floor(0, 24);
  w.Box(Vec3(-6, 0, 6), Vec3(2, 0.5f, 5), Euler(-20, 0, 0));  // ramp
  w.Fill(4, 0, 6, 7, 0, 10, core::Materials::kStoneSlab);     // slab step
  w.Fill(-1, 1, 7, 1, 1, 9, core::Materials::kStone);         // crawlspace roof
  w.Fill(10, 0, 2, 14, 2, 6, core::Materials::kStone);        // ledge
  w.Fill(12, 0, 1, 12, 2, 1, core::Materials::kLadderN);      // ladder
  w.Fill(-16, -1, -12, -10, -1, -6, core::Materials::kAir);   // pool pit
  w.Fill(-16, -3, -12, -10, -2, -6, core::Materials::kStone);
  w.Fill(-15, -2, -11, -11, -1, -7, core::Materials::kWater);
  w.Box(Vec3(-12, 0.25f, 0), Vec3(2, 0.25f, 2), JPH::Quat::sIdentity(), Vec3(0, 0, 1), 30.0f);
  w.Explosion(300, Vec3(0, 1, 6), 8.0f, 10.0f, 0.5f);
  w.Spawn(Vec3(-6, 0, 0));      // ramp
  w.Spawn(Vec3(0.5f, 0, 4));    // crawlspace
  w.Spawn(Vec3(12.5f, 0, -1));  // ladder
  w.Spawn(Vec3(-12, 0.5f, 0));  // platform
}

// Different deterministic input scripts per player, exercising every action.
inline Input ScenarioInput(int tick, PlayerHandle player) {
  const int p = static_cast<int>(player);
  const int phase = (tick / 40 + p) % 6;
  const float yaw = static_cast<float>((tick * (p + 1)) % 360);
  switch (phase) {
    case 0:
      return Move(0, 1, false, false, false, p == 2 ? 0.0f : yaw);
    case 1:
      return Move(1, 1, true, false, false, yaw);
    case 2:
      return Move(0, 1, false, tick % 40 < 3);
    case 3:
      return Move(-1, 0, false, false, true);
    case 4:
      return Move(0, 1, false, false, false, 0, -45);
    default:
      return Move(0, 0, false, tick % 7 == 0);
  }
}

inline std::vector<std::string> RecordTrace() {
  PlayerTestWorld w;
  ScenarioWorld(w);
  w.input = ScenarioInput;
  std::vector<std::string> lines;
  char buf[160];
  for (int tick = 0; tick < kScenarioTicks; ++tick) {
    w.Step();
    for (PlayerHandle h : w.players.handles()) {
      const Vec3 p = w.Pos(h);
      std::snprintf(buf, sizeof buf, "%d %u %.6f %.6f %.6f %d", tick, h, p.GetX(), p.GetY(),
                    p.GetZ(), static_cast<int>(w.C(h).state));
      lines.emplace_back(buf);
    }
  }
  return lines;
}

// Full-precision trace for the divergence tool: per tick and player, position and velocity.
struct TraceRow {
  int tick;
  PlayerHandle player;
  float position[3];
  float velocity[3];
  int state;
};

inline std::vector<TraceRow> RecordFullTrace() {
  PlayerTestWorld w;
  ScenarioWorld(w);
  w.input = ScenarioInput;
  std::vector<TraceRow> rows;
  for (int tick = 0; tick < kScenarioTicks; ++tick) {
    w.Step();
    for (PlayerHandle h : w.players.handles()) {
      const Vec3 p = w.Pos(h), v = w.Vel(h);
      rows.push_back({tick,
                      h,
                      {p.GetX(), p.GetY(), p.GetZ()},
                      {v.GetX(), v.GetY(), v.GetZ()},
                      static_cast<int>(w.C(h).state)});
    }
  }
  return rows;
}

}  // namespace dwell::test
