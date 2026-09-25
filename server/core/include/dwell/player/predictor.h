#pragma once

#include <array>
#include <cstdint>
#include <memory>
#include <optional>
#include <unordered_map>

#include "dwell/core/physics_world.h"
#include "dwell/core/terrain_collision.h"
#include "dwell/core/voxel.h"
#include "dwell/player/controller.h"
#include "dwell/protocol/messages.h"

// Client-side prediction and reconciliation for the local player (PLAYER_CONTROLLER.md §8). Runs
// the same controller as the server against a small prediction world: the client's terrain, the
// local player as the only dynamic body, and remote players as kinematic capsules. Built into the
// client's WASM sim core; natively it backs the latency/loss tests.
namespace dwell::player {

struct PredictorStats {
  std::uint32_t ticks = 0;
  std::uint32_t snapshots = 0;  // snapshots compared against history
  std::uint32_t replays = 0;    // snapshots that needed a replay
  std::uint32_t snaps = 0;      // corrections above RECONCILE_SNAP_DISTANCE
  std::uint32_t knockback_replays = 0;
  std::uint32_t resets = 0;      // history lost: restarted from the server state
  float last_correction = 0.0f;  // |present before − present after| of the last replay (m)
  float last_error = 0.0f;       // position error at ack of the last compared snapshot (m)
  std::uint32_t last_replay_ticks = 0;
};

class Predictor {
 public:
  // Owns its physics world; `world` holds the client's chunks (generated or streamed).
  Predictor(core::VoxelWorld& world, JPH::JobSystem& jobs, const PlayerControllerConfig& config);
  ~Predictor();

  Predictor(const Predictor&) = delete;
  Predictor& operator=(const Predictor&) = delete;

  // True once a snapshot has placed the local player (and while alive).
  bool active() const { return handle_.has_value(); }
  // Sequence number the next Tick must carry.
  std::uint32_t next_seq() const { return latest_seq_ + 1; }

  // Predicts one tick with this (quantized) input; `input.seq` must equal next_seq().
  void Tick(const protocol::InputFrame& input);

  // Handles this client's snapshot: starts prediction, or compares and reconciles.
  void OnSnapshot(const protocol::PhysicsSnapshot& snapshot);

  // A server knockback (PlayerEvent) applied after the pipeline of input `input_seq`.
  void OnKnockback(std::uint32_t input_seq, JPH::Vec3 delta_v);

  // Remote players as kinematic capsules in the prediction world. `feet` and `velocity` are from a
  // snapshot `lead_seconds` older than the local player's predicted present; the proxy is
  // extrapolated to the present and dead-reckoned until the next update (at most
  // kMaxDeadReckoning).
  void SetRemote(std::uint16_t player_id, JPH::Vec3 feet, JPH::Vec3 velocity, bool crouched,
                 float lead_seconds);
  void RemoveRemote(std::uint16_t player_id);

  // --- outputs ---
  JPH::Vec3 Position() const;  // predicted capsule centre
  JPH::Vec3 Velocity() const;
  // Visual correction still being smoothed out: render at Position() + RenderOffset().
  JPH::Vec3 RenderOffset() const { return offset_; }
  const PlayerController& controller() const;
  float HalfHeight() const;
  const PredictorStats& stats() const { return stats_; }
  const PlayerControllerConfig& config() const { return config_; }
  core::PhysicsWorld& physics() { return *physics_; }

  // Tolerances for "prediction matched the server" (PLAYER_CONTROLLER.md §8.2).
  static constexpr float kPositionTolerance = 0.01f;  // m
  static constexpr float kVelocityTolerance = 0.05f;  // m/s
  // Time constant of the visual correction smoothing.
  static constexpr float kSmoothingSeconds = 0.1f;

 private:
  static constexpr std::uint32_t kHistory = 256;  // ticks (> 4 s)
  struct Entry {
    std::uint32_t seq = 0;  // 0 = empty
    protocol::InputFrame input;
    PlayerController controller;
    JPH::Vec3 position, velocity;
  };
  struct Remote {
    JPH::BodyID body;
    JPH::Vec3 target;
    JPH::Vec3 velocity;
    int dead_reckoning_ticks = 0;
    bool crouched = false;
  };
  static constexpr float kMaxDeadReckoning = 0.5f;  // s

  void Reset(const protocol::PhysicsSnapshot& snapshot);
  void Simulate(const protocol::InputFrame& input, bool forward);
  void Record(const protocol::InputFrame& input);
  void Replay(
      std::uint32_t from_seq);  // re-runs inputs from_seq..latest_seq_ from the current state
  GroundRef GroundFromNet(protocol::GroundKind kind, std::uint16_t id) const;

  core::VoxelWorld& world_;
  PlayerControllerConfig config_;
  std::unique_ptr<core::PhysicsWorld> physics_;
  std::unique_ptr<core::TerrainCollision> terrain_;
  std::unique_ptr<Players> players_;
  std::optional<PlayerHandle> handle_;
  std::array<Entry, kHistory> history_{};
  std::uint32_t latest_seq_ = 0;
  std::uint32_t last_ack_ = 0;
  std::uint32_t last_knockback_seq_ = 0;  // latest knockback seen (event or snapshot)
  std::unordered_map<std::uint32_t, JPH::Vec3> knockbacks_;  // by input seq
  std::unordered_map<std::uint16_t, Remote> remotes_;
  JPH::RefConst<JPH::Shape> remote_standing_, remote_crouched_;
  JPH::Vec3 offset_ = JPH::Vec3::sZero();
  PredictorStats stats_;
};

}  // namespace dwell::player
