#pragma once

#include <array>
#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <unordered_map>
#include <vector>

#include "dwell/core/entropy.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/voxel.h"
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
  std::uint32_t generator_version = 0;  // 0 = flat test world (Phase 1)
  std::string client_version_note = "";
};

struct Outgoing {
  enum class Kind : std::uint8_t { kReliable, kDatagram, kClose };
  SessionId session = 0;
  Kind kind = Kind::kReliable;
  protocol::Channel channel = protocol::Channel::kControl;  // reliable only
  std::vector<std::uint8_t> bytes;                          // empty for kClose
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

  std::uint32_t tick() const { return tick_; }
  double time_ms() const { return tick_ * (1000.0 / protocol::kSimHz); }
  std::size_t joined_players() const;
  VoxelWorld& world() { return world_; }
  PhysicsWorld& physics() { return physics_; }

 private:
  enum class Phase : std::uint8_t { kAwaitingHello, kAwaitingAuth, kJoined };
  struct Session {
    protocol::TransportKind kind;
    TransportBinding binding;
    Phase phase = Phase::kAwaitingHello;
    protocol::Nonce nonce{};
    protocol::PublicKey public_key{};
    std::string display_name;
    std::uint16_t player_id = 0;
  };

  void HandleControl(SessionId id, Session& s, const protocol::Message& m);
  void SendReliable(SessionId id, const protocol::Message& m);
  void SendDatagram(SessionId id, const protocol::Message& m);
  void Reject(SessionId id, protocol::RejectReason reason, std::string message);
  std::uint16_t AllocatePlayerId();

  ServerConfig config_;
  Entropy& entropy_;
  PhysicsWorld physics_;
  VoxelWorld world_;
  std::unordered_map<SessionId, Session> sessions_;
  std::vector<Outgoing> outbox_;
  std::uint32_t tick_ = 0;
  std::uint16_t next_player_id_ = 1;
};

}  // namespace dwell::core
