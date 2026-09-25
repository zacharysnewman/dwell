#pragma once

// PlayerTestWorld (PLAYER_CONTROLLER.md §10): a headless world for controller tests, the voxel
// counterpart of the PPC's PPCTestWorld + HeadlessSession. Voxel primitives (floors, block and slab
// steps, walls, doorways, crawlspaces, ladder columns, water) plus Tier 1 boxes as moving
// platforms, scheduled kicks and explosions, and per-player event counters.
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/BoxShape.h>

#include <array>
#include <cmath>
#include <functional>
#include <map>
#include <numbers>
#include <vector>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/terrain_collision.h"
#include "dwell/core/voxel.h"
#include "dwell/player/controller.h"

namespace dwell::test {

using JPH::Vec3;
using player::Input;
using player::PlayerHandle;
using player::State;

inline int Ticks(float seconds) { return static_cast<int>(std::lround(seconds * 60.0f)); }

inline Input Move(float x, float y, bool run = false, bool jump = false, bool crouch = false,
                  float yaw = 0.0f, float pitch = 0.0f) {
  Input i;
  i.move_x = x;
  i.move_y = y;
  i.run = run;
  i.jump = jump;
  i.crouch = crouch;
  i.look_yaw = yaw;
  i.look_pitch = pitch;
  return i;
}

inline JPH::Quat Euler(float pitch_deg, float yaw_deg, float roll_deg) {
  constexpr float k = std::numbers::pi_v<float> / 180.0f;
  return JPH::Quat::sRotation(Vec3::sAxisY(), yaw_deg * k) *
         JPH::Quat::sRotation(Vec3::sAxisX(), pitch_deg * k) *
         JPH::Quat::sRotation(Vec3::sAxisZ(), roll_deg * k);
}

class PlayerTestWorld {
 public:
  using InputFn = std::function<Input(int tick, PlayerHandle player)>;

  explicit PlayerTestWorld(player::PlayerControllerConfig config = player::DefaultConfig(),
                           core::ChunkGenerator generator = core::GenerateEmptyChunk)
      : config(config),
        world(std::move(generator)),
        physics(jobs),
        terrain(world, physics),
        players(world, physics, &terrain) {}

  // --- voxel primitives (inclusive cell ranges) ---
  void Fill(int x0, int y0, int z0, int x1, int y1, int z1, core::MaterialId m) {
    for (int z = z0; z <= z1; ++z)
      for (int y = y0; y <= y1; ++y)
        for (int x = x0; x <= x1; ++x) world.SetVoxel(x, y, z, m);
  }
  // A one-cell-thick floor whose top face is at `top_y`.
  void Floor(int top_y = 0, int half = 16) {
    Fill(-half, top_y - 1, -half, half - 1, top_y - 1, half - 1, core::Materials::kStone);
  }
  void Remove(int x, int y, int z) { world.SetVoxel(x, y, z, core::Materials::kAir); }

  // --- bodies (Tier 1 layer) ---
  JPH::BodyID Box(Vec3 center, Vec3 half, JPH::Quat rotation = JPH::Quat::sIdentity(),
                  Vec3 velocity = Vec3::sZero(), float yaw_rate_deg = 0.0f) {
    JPH::BodyCreationSettings s(new JPH::BoxShape(half, 0.0f), JPH::RVec3(center), rotation,
                                JPH::EMotionType::Kinematic, core::ObjectLayers::kTier1);
    s.mLinearVelocity = velocity;
    s.mAngularVelocity = Vec3(0, yaw_rate_deg * std::numbers::pi_v<float> / 180.0f, 0);
    return physics.bodies().CreateAndAddBody(s, JPH::EActivation::Activate);
  }
  JPH::BodyID DynamicBox(Vec3 center, Vec3 half, float mass, Vec3 velocity,
                         bool lock_rotation = true) {
    JPH::BodyCreationSettings s(new JPH::BoxShape(half, 0.0f), JPH::RVec3(center),
                                JPH::Quat::sIdentity(), JPH::EMotionType::Dynamic,
                                core::ObjectLayers::kTier1);
    s.mOverrideMassProperties = JPH::EOverrideMassProperties::CalculateInertia;
    s.mMassPropertiesOverride.mMass = mass;
    s.mGravityFactor = 0.0f;
    s.mLinearVelocity = velocity;
    s.mLinearDamping = 0.0f;
    s.mAllowSleeping = false;
    if (lock_rotation) {
      s.mAllowedDOFs = JPH::EAllowedDOFs::TranslationX | JPH::EAllowedDOFs::TranslationY |
                       JPH::EAllowedDOFs::TranslationZ;
    }
    return physics.bodies().CreateAndAddBody(s, JPH::EActivation::Activate);
  }
  Vec3 BodyPosition(JPH::BodyID id) { return Vec3(physics.bodies().GetCenterOfMassPosition(id)); }

  PlayerHandle Spawn(Vec3 feet, float yaw = 0.0f) {
    const PlayerHandle h = players.Spawn(config, feet, yaw);
    if (h >= counts.size()) counts.resize(h + 1);
    return h;
  }

  void Kick(int at_tick, PlayerHandle h, Vec3 dv) { kicks.push_back({at_tick, h, dv}); }
  void Explosion(int at_tick, Vec3 center, float radius, float speed, float bias) {
    explosions.push_back({at_tick, center, radius, speed, bias});
  }

  void Step(int n = 1) {
    for (int i = 0; i < n; ++i) {
      for (PlayerHandle h : players.handles()) {
        if (input) players.SetInput(h, input(tick, h));
      }
      players.Tick();
      for (PlayerHandle h : players.handles()) {
        const auto events = players.controller(h).events;
        for (int bit = 0; bit < 32; ++bit) {
          if (events & (1u << bit)) ++counts[h][bit];
        }
      }
      for (const auto& k : kicks) {
        if (k.tick == tick) players.AddVelocity(k.player, k.dv);
      }
      for (const auto& e : explosions) {
        if (e.tick == tick) players.AddExplosion(e.center, e.radius, e.speed, e.bias);
      }
      physics.Step(1.0f / 60.0f);
      ++tick;
    }
  }

  // Number of times `event` (a player::Events bit) was raised for `h` so far.
  int Count(PlayerHandle h, std::uint32_t event) const {
    int bit = 0;
    while ((1u << bit) != event) ++bit;
    return counts[h][bit];
  }

  const player::PlayerController& C(PlayerHandle h) const { return players.controller(h); }
  Vec3 Pos(PlayerHandle h) const { return players.Position(h); }
  Vec3 Vel(PlayerHandle h) const { return players.Velocity(h); }
  float Feet(PlayerHandle h) const { return players.Feet(h); }
  float Head(PlayerHandle h) const { return players.Head(h); }
  float HorizontalSpeed(PlayerHandle h) const {
    const Vec3 v = Vel(h);
    return std::sqrt(v.GetX() * v.GetX() + v.GetZ() * v.GetZ());
  }
  // Highest feet height reached over `n` ticks.
  float MaxFeet(PlayerHandle h, int n) {
    float best = -1e9f;
    for (int i = 0; i < n; ++i) {
      Step();
      best = std::max(best, Feet(h));
    }
    return best;
  }

  core::JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  player::PlayerControllerConfig config;
  core::VoxelWorld world;
  core::PhysicsWorld physics;
  core::TerrainCollision terrain;
  player::Players players;
  InputFn input;
  int tick = 0;

 private:
  struct KickAt {
    int tick;
    PlayerHandle player;
    Vec3 dv;
  };
  struct ExplosionAt {
    int tick;
    Vec3 center;
    float radius, speed, bias;
  };
  std::vector<KickAt> kicks;
  std::vector<ExplosionAt> explosions;
  std::vector<std::array<int, 32>> counts;
};

}  // namespace dwell::test
