#pragma once

#include <Jolt/Jolt.h>

#include <Jolt/Physics/Body/BodyID.h>
#include <Jolt/Physics/Collision/Shape/Shape.h>

#include <cstdint>
#include <memory>
#include <type_traits>
#include <vector>

#include "dwell/player/config.h"
#include "dwell/player/voxel_query.h"

// Physics player controller (PLAYER_CONTROLLER.md): a port of the PPC Quantum systems to C++/Jolt
// on the voxel grid. Each player is a dynamic Jolt capsule driven by velocity layers; the ordered
// passes below run for every player before PhysicsSystem::Update.
namespace dwell::core {
class PhysicsWorld;
class TerrainCollision;
}  // namespace dwell::core

namespace dwell::player {

enum class State : std::uint8_t {
  kIdle,
  kWalking,
  kRunning,
  kCrouching,
  kSliding,
  kJumping,
  kFalling,
  kClimbing,
  kSwimming,
};

// One tick of input. Buttons are held state; presses are detected in-sim.
struct Input {
  float move_x = 0.0f, move_y = 0.0f;  // x = right, y = forward, camera-relative, |move| <= 1
  float look_yaw = 0.0f;               // degrees; 0 = +Z, 90 = +X
  float look_pitch = 0.0f;             // degrees; positive = up
  bool jump = false, run = false, crouch = false;
};

struct GroundInfo {
  bool grounded = false, was_grounded = false;  // THE grounded flag every pass uses
  Vec3 normal = Vec3::sAxisY();
  float slope_angle = 0.0f;  // degrees
  float gap = 0.0f;          // capsule bottom → ground
  GroundRef ground;
  bool ceiling_blocked = false, touching_wall = false;
  Vec3 wall_normal = Vec3::sZero();
};

struct HorizontalLayer {
  Vec3 current = Vec3::sZero();       // player-driven
  Vec3 external = Vec3::sZero();      // absorbed external velocity, decaying
  Vec3 contribution = Vec3::sZero();  // this tick's output (read back next tick)
};

struct VerticalLayer {
  float accumulated_y = 0.0f, platform_y = 0.0f, last_platform_y = 0.0f, target_y = 0.0f;
  // Ticks left after a step-up during which small upward deviations are not launches (Dwell:
  // Jolt's speculative contact on the step's edge nudges the capsule up as it crosses).
  std::uint8_t step_grace = 0;
};

struct JumpState {
  std::uint16_t buffer_ticks = 0, coyote_ticks = 0;
  bool jumping = false, jumped_this_tick = false;
};

struct CrouchState {
  bool crouching = false;
};

struct Cell {
  std::int32_t x = 0, y = 0, z = 0;
  bool operator==(const Cell&) const = default;
};

struct ClimbState {
  bool climbing = false;
  Cell ladder;  // a cell of the ladder being climbed
  Vec3 velocity = Vec3::sZero();
  bool has_released = false;
  Cell released;  // column let go of; not re-grabbed until the capsule leaves it
};

struct SwimState {  // Dwell addition
  bool swimming = false;
  float submerged = 0.0f;
  Vec3 velocity = Vec3::sZero();
};

struct PlatformState {
  GroundRef ground;                      // what the ground probe sees (tracked even while airborne)
  Vec3 ground_velocity = Vec3::sZero();  // velocity of that ground under the player
  Vec3 base_velocity = Vec3::sZero();    // ground_velocity while grounded (the carry), else 0
  float yaw_delta = 0.0f;                // degrees this tick, for the camera
};

// PlayerEvents raised during a tick (bits of PlayerController::events).
namespace Events {
inline constexpr std::uint32_t kJumped = 1u << 0;
inline constexpr std::uint32_t kLanded = 1u << 1;  // payload: landed_speed
inline constexpr std::uint32_t kCrouchChanged = 1u << 2;
inline constexpr std::uint32_t kClimbStarted = 1u << 3;
inline constexpr std::uint32_t kClimbEnded = 1u << 4;
inline constexpr std::uint32_t kSwimStarted = 1u << 5;
inline constexpr std::uint32_t kSwimEnded = 1u << 6;
}  // namespace Events

// Complete controller state: a plain value, copied into history buffers and snapshots.
struct PlayerController {
  Input input, previous_input;
  State state = State::kIdle;
  GroundInfo ground;
  HorizontalLayer horizontal;
  VerticalLayer vertical;
  JumpState jump;
  CrouchState crouch;
  ClimbState climb;
  SwimState swim;
  PlatformState platform;
  Vec3 target_velocity = Vec3::sZero();
  std::uint32_t events = 0;
  float landed_speed = 0.0f;  // impact speed of this tick's Landed event

  bool JumpPressed() const { return input.jump && !previous_input.jump; }
  bool CrouchPressed() const { return input.crouch && !previous_input.crouch; }
  bool Exclusive() const { return climb.climbing || swim.swimming; }
};
static_assert(std::is_trivially_copyable_v<PlayerController>);

// Move input as a horizontal, camera-relative world direction (length <= 1).
Vec3 MoveDirection(const Input& input);
Vec3 CameraRight(float yaw_degrees);

using PlayerHandle = std::uint32_t;

// Runs the controller for a set of players in one physics world.
class Players {
 public:
  Players(core::VoxelWorld& world, core::PhysicsWorld& physics, core::TerrainCollision* terrain);
  ~Players();

  Players(const Players&) = delete;
  Players& operator=(const Players&) = delete;

  // Spawns a player standing with its feet at `feet`. The config must outlive the player.
  PlayerHandle Spawn(const PlayerControllerConfig& config, Vec3 feet, float yaw_degrees = 0.0f);
  void Despawn(PlayerHandle handle);
  bool Exists(PlayerHandle handle) const;

  void SetInput(PlayerHandle handle, const Input& input);

  // Runs the pipeline (PLAYER_CONTROLLER.md §4) for every player. Call before the physics step.
  void Tick();

  // Instant velocity change (knockback, launch pads, explosions): absorbed next tick.
  void AddVelocity(PlayerHandle handle, Vec3 delta_v);
  // Radial push with linear falloff and upward bias (PPC AddExplosion), for players in range.
  void AddExplosion(Vec3 center, float radius, float speed, float upward_bias);

  // --- state access (tests, snapshots, reconciliation) ---
  const PlayerController& controller(PlayerHandle handle) const;
  PlayerController& mutable_controller(PlayerHandle handle);
  const PlayerControllerConfig& config(PlayerHandle handle) const;
  JPH::BodyID body(PlayerHandle handle) const;
  Vec3 Position(PlayerHandle handle) const;  // capsule centre
  Vec3 Velocity(PlayerHandle handle) const;
  float Feet(PlayerHandle handle) const;
  float Head(PlayerHandle handle) const;
  float HalfHeight(PlayerHandle handle) const;
  // Restores a player's full state (controller + body), e.g. from a snapshot before replaying.
  void Restore(PlayerHandle handle, const PlayerController& state, Vec3 position, Vec3 velocity);
  // Moves the body (and swaps its capsule to match `crouching`).
  void Teleport(PlayerHandle handle, Vec3 position, Vec3 velocity);

  std::vector<PlayerHandle> handles() const;
  const VoxelQuery& query() const { return query_; }

 private:
  struct Player;
  Player& Get(PlayerHandle handle);
  const Player& Get(PlayerHandle handle) const;
  void SetCrouchShape(Player& p, bool crouching);

  void StepInput(Player& p);
  void StepProbe(Player& p);
  void StepPlatform(Player& p);
  void StepCrouch(Player& p);
  void StepJump(Player& p);
  void StepClimb(Player& p);
  void StepSwim(Player& p);
  void StepHorizontal(Player& p);
  void StepVertical(Player& p);
  void StepAggregate(Player& p);
  void StepState(Player& p);

  void PerformJump(Player& p);
  void TryStep(Player& p, Vec3 move_direction);
  void ApplyEdgeGuard(Player& p);
  bool AutoJumpObstacle(Player& p);
  bool RingCast(const Player& p, Vec3 center, Vec3 dir, float distance, float radius,
                ProbeHit* closest) const;
  bool FitsStanding(const Player& p, Vec3 center) const;
  bool FindLadder(const Player& p, Cell& ladder, bool& in_released_column) const;

  // Contact normals of each player's body from the last physics step (a Jolt ContactListener).
  class Contacts;

  core::PhysicsWorld& physics_;
  core::TerrainCollision* terrain_;
  VoxelQuery query_;
  std::vector<Player*> players_;  // indexed by handle; null when free
  std::unique_ptr<Contacts> contacts_;
};

}  // namespace dwell::player
