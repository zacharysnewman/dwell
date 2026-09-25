#pragma once

#include <array>
#include <cstdint>
#include <deque>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <unordered_map>
#include <vector>

#include "dwell/core/entropy.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/terrain_collision.h"
#include "dwell/core/voxel.h"
#include "dwell/player/controller.h"
#include "dwell/protocol/messages.h"

// The authoritative server core (ARCHITECTURE.md §4). It has no sockets, threads, or OS calls: the
// host feeds it transport events, calls Step() at SIM_HZ, and drains the outbox (§4.3). The same
// class runs in the native server, in the browser's local mode, and in friend-world hosts.
namespace dwell::core {

using SessionId = std::uint32_t;
using TransportBinding = std::array<std::uint8_t, 32>;

struct ServerConfig {
  std::string name = "Dwell Server";
  std::string motd = "";
  std::uint16_t max_players = 16;
  std::uint64_t world_seed = 0;
  std::uint32_t generator_version = kGeneratorTerrain;  // §6.3; 1 = movement playground
  std::string client_version_note = "";
  // Feet position; players spread out around it. Unset: the generator's spawn point.
  std::optional<std::array<float, 3>> spawn = std::nullopt;
};

struct Outgoing {
  enum class Kind : std::uint8_t { kReliable, kDatagram, kClose };
  SessionId session = 0;
  Kind kind = Kind::kReliable;
  protocol::Channel channel = protocol::Channel::kControl;  // reliable only
  std::vector<std::uint8_t> bytes;                          // empty for kClose
};

// Per-session counters, for tests and diagnostics.
struct SessionStats {
  std::uint32_t inputs_received = 0;
  std::uint32_t inputs_rejected = 0;    // out of range (§11)
  std::uint32_t datagrams_dropped = 0;  // over the rate limit
  std::uint32_t inputs_skipped = 0;     // dropped to bound input latency
  std::uint32_t ticks_starved = 0;      // no input queued: the last one repeated
};

class Server {
 public:
  Server(ServerConfig config, Entropy& entropy, JPH::JobSystem& jobs);
  ~Server();

  // --- transport events (from the host) ---
  void OnConnected(SessionId session, protocol::TransportKind kind,
                   const TransportBinding& binding);
  void OnDisconnected(SessionId session);
  void OnReliable(SessionId session, protocol::Channel channel,
                  std::span<const std::uint8_t> bytes);
  void OnDatagram(SessionId session, std::span<const std::uint8_t> bytes);

  // Advances the simulation by one 1/SIM_HZ step.
  void Step();

  // Messages produced since the last call, in order.
  std::vector<Outgoing> TakeOutbox();

  // Server-originated velocity change for a player (knockback), applied before this tick's physics
  // step and announced to its client as PlayerEvent(Knockback) for predicted replay (§9.3).
  void Knockback(std::uint16_t player_id, JPH::Vec3 delta_v);

  std::uint32_t tick() const { return tick_; }
  double time_ms() const { return tick_ * (1000.0 / protocol::kSimHz); }
  std::size_t joined_players() const;
  VoxelWorld& world() { return world_; }
  PhysicsWorld& physics() { return physics_; }
  player::Players& players() { return players_; }
  const player::PlayerControllerConfig& player_config() const { return player_config_; }

  // Test and diagnostics access by player id.
  std::optional<player::PlayerHandle> PlayerHandleOf(std::uint16_t player_id) const;
  std::optional<SessionStats> StatsOf(std::uint16_t player_id) const;
  int HealthOf(std::uint16_t player_id) const;  // −1 when unknown

 private:
  enum class Phase : std::uint8_t { kAwaitingHello, kAwaitingAuth, kJoined };
  struct QueuedInput {
    std::uint32_t seq;
    player::Input input;
  };
  struct Session {
    protocol::TransportKind kind;
    TransportBinding binding;
    Phase phase = Phase::kAwaitingHello;
    protocol::Nonce nonce{};
    protocol::PublicKey public_key{};
    std::string display_name;
    std::uint16_t player_id = 0;
    // Player (joined only).
    std::optional<player::PlayerHandle> handle;  // empty while dead
    std::deque<QueuedInput> inputs;              // ordered by seq
    bool primed = false;  // jitter buffer: consume only once kInputBuffer inputs are queued
    std::uint32_t last_processed_seq = 0;
    player::Input last_input;
    int health = protocol::kMaxHealth;
    std::uint32_t respawn_tick = 0;  // while dead
    float death_position[3] = {0, 0, 0};
    std::uint32_t launch_ready_tick = 0;
    std::uint32_t last_knockback_seq = 0;
    std::uint32_t rate_window_tick = 0;
    std::uint32_t rate_window_count = 0;
    SessionStats stats;
  };

  void HandleControl(SessionId id, Session& s, const protocol::Message& m);
  void HandleInput(Session& s, const protocol::PlayerInput& m);
  void SendReliable(SessionId id, const protocol::Message& m,
                    protocol::Channel channel = protocol::Channel::kControl);
  void SendDatagram(SessionId id, const protocol::Message& m);
  void BroadcastWorld(const protocol::Message& m);
  void Reject(SessionId id, protocol::RejectReason reason, std::string message);
  void RemoveSession(SessionId id);
  std::uint16_t AllocatePlayerId();

  void SpawnPlayer(Session& s);
  void Kill(Session& s, protocol::DamageCause cause);
  void Damage(Session& s, int amount, protocol::DamageCause cause);
  void AfterControllerTick(Session& s);
  void SendSnapshots();
  Session* SessionOfPlayer(std::uint16_t player_id);
  const Session* SessionOfPlayer(std::uint16_t player_id) const;
  std::uint16_t PlayerIdOfBody(std::uint32_t body_id) const;

  ServerConfig config_;
  Entropy& entropy_;
  PhysicsWorld physics_;
  VoxelWorld world_;
  TerrainCollision terrain_;
  player::PlayerControllerConfig player_config_;
  player::Players players_;
  std::unordered_map<SessionId, Session> sessions_;
  std::vector<Outgoing> outbox_;
  std::vector<std::pair<std::uint16_t, JPH::Vec3>> knockbacks_;  // applied in the next Step
  std::uint32_t tick_ = 0;
  std::uint16_t next_player_id_ = 1;
};

}  // namespace dwell::core
