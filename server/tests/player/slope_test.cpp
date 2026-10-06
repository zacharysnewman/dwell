// The controller on slope blocks (docs/SLOPE_BLOCKS.md §4): walking up and down 45° and gentle
// slopes, across hips and valleys, from slope to flat and back, at rest, into walls and under
// sloped ceilings. Every scenario must hold at the origin and ~8,000 km out (main.cpp).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <string>

#include "dwell/core/block_registry.h"
#include "player_test_world.h"

namespace dwell::test {
namespace {

using namespace dwell::core;

MaterialId SlopeState(const std::string& shape, const std::string& facing,
                      const std::string& half = "bottom") {
  const auto id =
      ParseState("dwell:stone_slope[facing=" + facing + ",half=" + half + ",shape=" + shape + "]");
  REQUIRE(id.has_value());
  return *id;
}

// The four ways a ramp can run: the direction a player walks up it (dx, dz), the wedge facing that
// descends the other way, and the yaw that walks along it (input yaw 0 walks +Z).
struct Heading {
  const char* name;
  int dx, dz;
  const char* down_facing;
  float yaw;
};
constexpr Heading kHeadings[] = {{"+z", 0, 1, "north", 0.0f},
                                 {"+x", 1, 0, "west", 90.0f},
                                 {"-z", 0, -1, "south", 180.0f},
                                 {"-x", -1, 0, "east", -90.0f}};

// A staircase of `kind` pieces rising along `h` from cell (ox, oz): step i at distance
// `start + i·stride`, rising `rise` per step on a stone core, then a stone platform six cells long.
// `width` cells either side of the centre line. Returns the platform's top y.
struct Ramp {
  int top;       // platform top y
  int end_dist;  // distance from the origin cell where the platform starts
};

Ramp BuildRamp(PlayerTestWorld& w, const Heading& h, const std::string& shape_low,
               const std::string& shape_high, int steps, int start = 3, int width = 2) {
  // Cells along the ramp: a "standard" ramp is one wedge per block of rise; a gentle one is a low
  // wedge then a high wedge (two cells per block).
  const bool gentle = !shape_high.empty();
  const int stride = gentle ? 2 : 1;
  const MaterialId low = SlopeState(shape_low, h.down_facing);
  const MaterialId high = gentle ? SlopeState(shape_high, h.down_facing) : low;
  auto cell = [&](int along, int across, int y, MaterialId m, bool core_below) {
    const int x = h.dx * along + h.dz * across;
    const int z = h.dz * along + h.dx * across;
    if (core_below && y > 0) w.Fill(x, 0, z, x, y - 1, z, Materials::kStone);
    w.Set(x, y, z, m);
  };
  for (int i = 0; i < steps; ++i) {
    for (int a = -width; a <= width; ++a) {
      if (gentle) {
        cell(start + i * 2, a, i, low, true);
        cell(start + i * 2 + 1, a, i, high, true);
      } else {
        cell(start + i, a, i, low, true);
      }
    }
  }
  const int end = start + steps * stride;
  for (int along = end; along < end + 6; ++along) {
    for (int a = -width; a <= width; ++a) {
      const int x = h.dx * along + h.dz * a;
      const int z = h.dz * along + h.dx * a;
      w.Fill(x, 0, z, x, steps - 1, z, Materials::kStone);
    }
  }
  return {steps, end};
}

// A point on the ramp's centre line: in the cell `k` cells from the origin along the heading, a
// fraction `frac` of the way through it (cell k spans [k, k + 1] going +, [−k, −k + 1] going −).
Vec3 Spot(const Heading& h, int k, float frac, float y) {
  const auto along = [&](int d) { return d > 0 ? k + frac : -k + 1.0f - frac; };
  return Vec3(h.dx != 0 ? along(h.dx) : 0.5f, y, h.dz != 0 ? along(h.dz) : 0.5f);
}

struct Walk {
  int airborne = 0;
  float max_speed = 0.0f;  // fastest horizontal speed while walking
  float max_tick_rise = 0.0f;
};

Walk Run(PlayerTestWorld& w, PlayerHandle e, int ticks) {
  Walk out;
  float prev_feet = w.Feet(e);
  for (int i = 0; i < ticks; ++i) {
    w.Step();
    if (!w.C(e).ground.grounded) ++out.airborne;
    out.max_speed = std::max(out.max_speed, w.HorizontalSpeed(e));
    out.max_tick_rise = std::max(out.max_tick_rise, w.Feet(e) - prev_feet);
    prev_feet = w.Feet(e);
  }
  return out;
}

}  // namespace

TEST_SUITE("player: slopes") {
  TEST_CASE("walks up a 45° slope in every direction without jumping") {
    for (const Heading& h : kHeadings) {
      CAPTURE(h.name);
      PlayerTestWorld w;
      w.Floor();
      const Ramp ramp = BuildRamp(w, h, "wedge", "", 4);
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      const float yaw = h.yaw;
      w.input = [yaw](int, PlayerHandle) { return Move(0, 1, false, false, false, yaw); };
      const Walk walk = Run(w, e, Ticks(2.2f));
      CHECK(w.Feet(e) == doctest::Approx(static_cast<float>(ramp.top)).epsilon(0.02));
      CHECK(walk.airborne == 0);
      // No gain: never faster than walking, and rising no faster than the slope allows.
      CHECK(walk.max_speed <= player::DefaultConfig().movement.walk_speed * 1.05f);
      CHECK(walk.max_tick_rise <= player::DefaultConfig().movement.walk_speed / 60.0f * 1.5f);
    }
  }

  TEST_CASE("walks up a gentle (1:2) slope in every direction without jumping") {
    for (const Heading& h : kHeadings) {
      CAPTURE(h.name);
      PlayerTestWorld w;
      w.Floor();
      const Ramp ramp = BuildRamp(w, h, "gentle_low", "gentle_high", 3);
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      const float yaw = h.yaw;
      w.input = [yaw](int, PlayerHandle) { return Move(0, 1, false, false, false, yaw); };
      const Walk walk = Run(w, e, Ticks(2.4f));
      CHECK(w.Feet(e) == doctest::Approx(static_cast<float>(ramp.top)).epsilon(0.02));
      CHECK(walk.airborne == 0);
    }
  }

  TEST_CASE("walks down a slope staying on the ground, in every direction") {
    for (const bool gentle : {false, true}) {
      for (const Heading& h : kHeadings) {
        CAPTURE(h.name);
        CAPTURE(gentle);
        PlayerTestWorld w;
        w.Floor();
        // Build the ramp rising away from the origin, then start on its platform and walk back
        // down: the player turns around (yaw + 180°).
        const Ramp ramp = gentle ? BuildRamp(w, h, "gentle_low", "gentle_high", 3)
                                 : BuildRamp(w, h, "wedge", "", 4);
        const float yaw = h.yaw + 180.0f;
        const float along = static_cast<float>(ramp.end_dist) + 2.5f;
        const auto e = w.Spawn(
            Vec3(h.dx * along + 0.5f, static_cast<float>(ramp.top), h.dz * along + 0.5f), yaw);
        w.input = [yaw](int, PlayerHandle) { return Move(0, 1, false, false, false, yaw); };
        const Walk walk = Run(w, e, Ticks(4.0f));
        CHECK(w.Feet(e) < 0.05f);
        CHECK(walk.airborne == 0);  // the ground snap holds the player to the descent
      }
    }
  }

  TEST_CASE("running down a slope stays grounded and never exceeds run speed") {
    const float limit = player::DefaultConfig().movement.run_speed * 1.05f;
    for (const Heading& h : kHeadings) {
      CAPTURE(h.name);
      PlayerTestWorld w;
      w.Floor();
      const Ramp ramp = BuildRamp(w, h, "wedge", "", 6, 3, 2);
      const float yaw = h.yaw + 180.0f;
      const float along = static_cast<float>(ramp.end_dist) + 2.5f;
      const auto e = w.Spawn(
          Vec3(h.dx * along + 0.5f, static_cast<float>(ramp.top), h.dz * along + 0.5f), yaw);
      w.input = [yaw](int, PlayerHandle) { return Move(0, 1, true, false, false, yaw); };
      const Walk walk = Run(w, e, Ticks(3.0f));
      CHECK(walk.airborne == 0);
      CHECK(walk.max_speed <= limit);
    }
  }

  TEST_CASE("standing still on a slope does not slide") {
    for (const bool gentle : {false, true}) {
      for (const Heading& h : kHeadings) {
        CAPTURE(h.name);
        CAPTURE(gentle);
        PlayerTestWorld w;
        w.Floor();
        const Ramp ramp = gentle ? BuildRamp(w, h, "gentle_low", "gentle_high", 3)
                                 : BuildRamp(w, h, "wedge", "", 4);
        (void)ramp;
        // Midway up the ramp, on its surface: the 3rd standard wedge (rise 2, +½ at mid-cell), or
        // the 2nd step's low gentle wedge (rise 1, +¼).
        const Vec3 at = gentle ? Spot(h, 3 + 2, 0.5f, 1.25f) : Spot(h, 3 + 2, 0.5f, 2.5f);
        const auto e = w.Spawn(at);
        w.input = [](int, PlayerHandle) { return Move(0, 0); };
        w.Step(Ticks(0.5f));  // settle
        const Vec3 start = w.Pos(e);
        w.Step(Ticks(2.0f));
        const Vec3 end = w.Pos(e);
        CHECK(std::abs(end.GetX() - start.GetX()) < 0.01f);
        CHECK(std::abs(end.GetZ() - start.GetZ()) < 0.01f);
        CHECK(std::abs(end.GetY() - start.GetY()) < 0.01f);
        CHECK(w.C(e).ground.grounded);
      }
    }
  }

  TEST_CASE("crosses a hip (outer corner) and a valley (inner corner) staying on the ground") {
    // A plateau (top y = 1) with its edges sloped: wedges along the two exposed sides and one
    // corner piece between them, walked diagonally from the low ground to the top.
    for (const bool valley : {false, true}) {
      CAPTURE(valley);
      PlayerTestWorld w;
      w.Floor();
      const MaterialId toward_south = SlopeState("wedge", "north");  // rises toward +Z
      const MaterialId toward_east = SlopeState("wedge", "west");    // rises toward +X
      if (!valley) {
        // Plateau in the quadrant x ≥ 1, z ≥ 1; slopes rise toward it; an outer corner at (0, 0).
        w.Fill(1, 0, 1, 8, 0, 8, Materials::kStone);
        for (int i = 1; i <= 8; ++i) {
          w.Set(i, 0, 0, toward_south);
          w.Set(0, 0, i, toward_east);
        }
        w.Set(0, 0, 0, SlopeState("outer", "west"));  // high corner at the south-east
      } else {
        // Plateau everywhere but the quadrant x ≤ 0, z ≤ 0: the low corner is a valley at (0, 0).
        w.Fill(-8, 0, 1, 8, 0, 8, Materials::kStone);
        w.Fill(1, 0, -8, 8, 0, 0, Materials::kStone);
        for (int i = 1; i <= 8; ++i) w.Set(-i, 0, 1, Materials::kAir);
        w.Fill(-8, 0, 1, -1, 0, 1, Materials::kAir);
        w.Fill(1, 0, -8, 1, 0, 0, Materials::kAir);
        // Cells on the low side next to the plateau: wedges rising toward it, a valley between.
        for (int i = 1; i <= 8; ++i) w.Set(-i, 0, 1, toward_south);  // low cells x ≤ −1 at z = 1
        for (int i = 1; i <= 8; ++i) w.Set(1, 0, -i, toward_east);  // low cells z ≤ −1 at x = 1
        w.Set(0, 0, 0, SlopeState("inner", "west"));                // low corner at the north-west
        w.Fill(0, 0, 1, 0, 0, 1, Materials::kStone);
        w.Fill(1, 0, 0, 1, 0, 0, Materials::kStone);
      }
      const auto e = w.Spawn(Vec3(-2.5f, 0, -2.5f), 45.0f);
      w.input = [](int, PlayerHandle) { return Move(0, 1, false, false, false, 45.0f); };
      const Walk walk = Run(w, e, Ticks(3.0f));
      CHECK(w.Pos(e).GetX() > 1.5f);
      CHECK(w.Pos(e).GetZ() > 1.5f);
      CHECK(w.Feet(e) == doctest::Approx(1.0f).epsilon(0.03));
      CHECK(walk.airborne == 0);
    }
  }

  TEST_CASE("a slope into a wall stops the player without hopping or sliding back") {
    PlayerTestWorld w;
    w.Floor();
    const Heading& h = kHeadings[0];
    const Ramp ramp = BuildRamp(w, h, "wedge", "", 3, 3, 3);
    // A wall rising from the top of the ramp, two blocks past it.
    w.Fill(-3, 0, ramp.end_dist + 2, 3, ramp.top + 3, ramp.end_dist + 2, Materials::kStone);
    const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
    w.input = [](int, PlayerHandle) { return Move(0, 1); };
    const Walk walk = Run(w, e, Ticks(4.0f));
    CHECK(w.Pos(e).GetZ() < static_cast<float>(ramp.end_dist) + 2.0f);
    CHECK(w.Feet(e) == doctest::Approx(static_cast<float>(ramp.top)).epsilon(0.02));
    CHECK(walk.airborne == 0);
    const Vec3 rest = w.Pos(e);
    w.Step(Ticks(1.0f));
    CHECK((w.Pos(e) - rest).Length() < 0.01f);
  }

  TEST_CASE("a sloped ceiling: standing is blocked where it comes down, crouching passes") {
    // A tunnel along +Z: walls at x = ±2, floor top y = 0, a ceiling of inverted wedges coming down
    // from 3 m to 1 m over two blocks, then a flat 1 m crawlspace.
    auto build = [](PlayerTestWorld& w) {
      w.Floor(0, 24);
      w.Fill(-2, 0, 0, -2, 4, 14, Materials::kStone);
      w.Fill(2, 0, 0, 2, 4, 14, Materials::kStone);
      const MaterialId down = SlopeState("wedge", "north", "top");  // thicker toward +Z
      for (int x = -1; x <= 1; ++x) {
        w.Set(x, 2, 3, down);                          // underside 3 → 2
        w.Set(x, 1, 4, down);                          // underside 2 → 1
        w.Fill(x, 1, 5, x, 1, 12, Materials::kStone);  // flat roof, underside 1
        w.Fill(x, 3, 3, x, 3, 4, Materials::kAir);
      }
    };
    {
      PlayerTestWorld w;
      build(w);
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [](int, PlayerHandle) { return Move(0, 1); };
      w.Step(Ticks(3.0f));
      // Standing (1.8 m) fits where the underside is ≥ 1.8: z + (3 − 1.8) ≈ 4.2 − radius.
      CHECK(w.Pos(e).GetZ() > 3.5f);
      CHECK(w.Pos(e).GetZ() < 4.5f);
    }
    {
      PlayerTestWorld w;
      build(w);
      const auto e = w.Spawn(Vec3(0.5f, 0, 0.5f));
      w.input = [](int, PlayerHandle) { return Move(0, 1, false, false, true); };
      w.Step(Ticks(8.0f));
      CHECK(w.Pos(e).GetZ() > 12.0f);  // through the crawlspace
      CHECK(w.Feet(e) == doctest::Approx(0.0f).epsilon(0.05));
    }
  }

  TEST_CASE("the playground's slope features can be walked up from the flat") {
    // The in-game slope playground (voxel.cpp): the 45° ramp, the gentle ramp and the hill's side.
    const struct {
      const char* name;
      float x;
      float top;
      float seconds;  // to be on the top, short of walking off its far side
    } features[] = {{"45° ramp", 19.5f, 4.0f, 1.8f},
                    {"gentle ramp", 23.5f, 3.0f, 2.2f},
                    {"hill", 28.5f, 1.0f, 1.2f}};
    for (const auto& f : features) {
      CAPTURE(f.name);
      PlayerTestWorld w(player::DefaultConfig(), core::GeneratePlaygroundChunk);
      const auto e = w.Spawn(Vec3(f.x, 0, 3.0f));
      w.input = [](int, PlayerHandle) { return Move(0, 1); };
      // Walk until on the top, short of its far edge (the platforms are ~3 blocks long).
      const Walk walk = Run(w, e, Ticks(f.seconds));
      CHECK(w.Feet(e) == doctest::Approx(f.top).epsilon(0.02));
      CHECK(walk.airborne == 0);
    }
  }

  TEST_CASE("walking across generated sloped terrain: no hops, no launches, no getting stuck") {
    // A stretch of the real terrain (generator version 5: its surface is made of slopes and slabs).
    for (const bool run : {false, true}) {
      CAPTURE(run);
      PlayerTestWorld w(player::DefaultConfig(), core::GeneratorFor(core::kGeneratorTerrain, 0));
      const auto spawn = core::SpawnPointFor(core::kGeneratorTerrain, 0);
      const auto e = w.Spawn(Vec3(static_cast<float>(spawn[0]), static_cast<float>(spawn[1]),
                                  static_cast<float>(spawn[2])));
      w.input = [run](int, PlayerHandle) { return Move(0, 1, run, false, false, 90.0f); };  // +X
      const float x0 = w.Pos(e).GetX();
      int airborne = 0, longest_air = 0, streak = 0;
      float max_rise = 0.0f, max_drop = 0.0f, prev = w.Feet(e);
      for (int i = 0; i < Ticks(12.0f); ++i) {
        w.Step();
        streak = w.C(e).ground.grounded ? 0 : streak + 1;
        airborne += !w.C(e).ground.grounded;
        longest_air = std::max(longest_air, streak);
        max_rise = std::max(max_rise, w.Feet(e) - prev);
        max_drop = std::max(max_drop, prev - w.Feet(e));
        prev = w.Feet(e);
      }
      const float travelled = w.Pos(e).GetX() - x0;
      const float speed = run ? player::DefaultConfig().movement.run_speed
                              : player::DefaultConfig().movement.walk_speed;
      MESSAGE("travelled " << travelled << " m, airborne " << airborne << " ticks (longest "
                           << longest_air << "), largest rise " << max_rise << " drop "
                           << max_drop);
      CHECK(travelled > speed * 12.0f * 0.5f);  // not stuck on a ledge (the terrain has cliffs)
      // Steps up (0.5 m slabs) happen in one tick; nothing rises faster: never launched.
      CHECK(max_rise <= player::DefaultConfig().movement.max_step_height + speed / 60.0f);
    }
  }

  TEST_CASE("the walkable slope limit leaves room for the 45° pitch of a standard slope") {
    // A standard slope's face is exactly 45°: the limit must not hinge on float rounding.
    CHECK(player::DefaultConfig().probes.max_slope_angle >= 49.9f);
  }
}

}  // namespace dwell::test
