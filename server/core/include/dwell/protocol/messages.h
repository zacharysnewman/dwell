#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <variant>
#include <vector>

#include "dwell/protocol/constants.gen.h"

// Protocol messages (ARCHITECTURE.md §8.3). Every message starts with a u8 MessageType. Layouts are
// pinned by golden vectors in shared/protocol/vectors.txt.
namespace dwell::protocol {

using PublicKey = std::array<std::uint8_t, 32>;
using Nonce = std::array<std::uint8_t, 32>;
using Signature = std::array<std::uint8_t, 64>;

struct DatagramPing {
  std::uint32_t seq = 0;
  double client_time_ms = 0;
};
struct DatagramPong {
  std::uint32_t seq = 0;
  double client_time_ms = 0;
  std::uint32_t server_tick = 0;
};
struct StatusRequest {};
struct StatusResponse {
  static constexpr std::uint8_t kFlagOnlineMode = 1u << 0;
  std::uint16_t protocol_version = 0;
  std::string server_name;
  std::string motd;
  std::uint16_t players = 0;
  std::uint16_t max_players = 0;
  std::uint8_t flags = 0;
};
struct ClientHello {
  std::uint16_t protocol_version = 0;
  std::string client_version;
  PublicKey public_key{};
  std::string display_name;
};
struct Challenge {
  Nonce nonce{};
};
struct ClientAuth {
  Signature signature{};
};
struct Welcome {
  std::uint16_t player_id = 0;
  std::uint64_t world_seed = 0;
  std::uint32_t generator_version = 0;
  std::uint32_t server_tick = 0;
};
struct Reject {
  RejectReason reason = RejectReason::kMalformed;
  std::string message;
};
struct Ping {
  std::uint32_t seq = 0;
  double client_time_ms = 0;
};
struct Pong {
  std::uint32_t seq = 0;
  double client_time_ms = 0;
  std::uint32_t server_tick = 0;
  double server_time_ms = 0;
};

// --- Players (Phase 2, PLAYER_CONTROLLER.md §8.4) ---

// One tick of quantized input: move ∈ [−127, 127]² (|move| ≤ 127), buttons (InputButtons), yaw as
// a wrapped fraction of a turn (65536 per 360°), pitch in [−32767, 32767] for ±90°.
struct InputFrame {
  std::uint32_t seq = 0;
  std::int8_t move_x = 0, move_y = 0;
  std::uint16_t buttons = 0;
  std::int16_t yaw = 0, pitch = 0;
  bool operator==(const InputFrame&) const = default;
};

// C→S datagram: the newest inputs (1..kMaxInputsPerDatagram, oldest first) for redundancy.
struct PlayerInput {
  std::uint32_t last_snapshot_tick = 0;
  std::vector<InputFrame> inputs;
};

// Local controller state needed to resume simulation exactly (PLAYER_CONTROLLER.md §8.4).
struct ControllerState {
  std::uint8_t flags = 0;  // ControllerFlags
  float current_x = 0, current_z = 0, external_x = 0, external_z = 0;
  float contribution_x = 0, contribution_z = 0;
  float accumulated_y = 0, platform_y = 0, target_y = 0, ground_velocity_y = 0;
  GroundKind ground_kind = GroundKind::kNone;
  std::uint16_t ground_id = 0;  // player id for GroundKind::kPlayer
  std::uint8_t buffer_ticks = 0, coyote_ticks = 0, step_grace = 0;
  std::int32_t ladder_x = 0, ladder_y = 0, ladder_z = 0;  // on the wire only while climbing
  std::int32_t released_x = 0, released_z = 0;            // only when hasReleased
};

struct LocalPlayerState {
  float position[3] = {0, 0, 0};  // capsule centre
  float velocity[3] = {0, 0, 0};
  std::uint8_t flags = 0;  // PlayerFlags
  std::uint8_t health = 0;
  PlayerState state = PlayerState::kIdle;
  std::uint8_t input_buffer = 0;         // inputs queued on the server (client clock steering)
  std::uint32_t last_knockback_seq = 0;  // input seq of the latest knockback applied (0 = none)
  ControllerState controller;
};

struct RemotePlayerState {
  std::uint16_t player_id = 0;
  float position[3] = {0, 0, 0};
  float velocity[3] = {0, 0, 0};  // f16 on the wire
  std::int16_t yaw = 0, pitch = 0;
  PlayerState state = PlayerState::kIdle;
  std::uint8_t flags = 0;  // PlayerFlags
};

// S→C datagram, SNAPSHOT_HZ. Tier 1 entities join in Phase 4.
struct PhysicsSnapshot {
  std::uint32_t server_tick = 0;
  std::uint32_t ack_input_seq = 0;  // last input of this client processed
  LocalPlayerState local;
  std::vector<RemotePlayerState> remotes;
};

// S→C reliable (`world`): server-originated effects on a player.
struct PlayerEvent {
  PlayerEventKind kind = PlayerEventKind::kKnockback;
  std::uint16_t player_id = 0;
  std::uint32_t server_tick = 0;
  std::uint32_t input_seq = 0;  // that player's input processed on server_tick (for replay)
  float vector[3] = {0, 0, 0};  // Knockback: velocity change; Respawn: position
  std::uint8_t amount = 0;      // Damage
  DamageCause cause = DamageCause::kFall;  // Damage, Death
};

using Message = std::variant<DatagramPing, DatagramPong, StatusRequest, StatusResponse, ClientHello,
                             Challenge, ClientAuth, Welcome, Reject, Ping, Pong, PlayerInput,
                             PhysicsSnapshot, PlayerEvent>;

// Appends the encoded message to `out`. Strings longer than their limit are truncated at a UTF-8
// boundary, so encoding never produces a message the peer would reject.
void Encode(const Message& message, std::vector<std::uint8_t>& out);
std::vector<std::uint8_t> Encode(const Message& message);

// Decodes exactly one message; nullopt on unknown type, truncation, trailing bytes, over-limit
// strings, invalid UTF-8, or out-of-range enum values.
std::optional<Message> Decode(std::span<const std::uint8_t> bytes);

// Bytes signed by the client in ClientAuth (ADR 0004):
//   authDomainTag ‖ nonce ‖ transportBinding (32) ‖ publicKey
std::vector<std::uint8_t> AuthTranscript(const Nonce& nonce,
                                         const std::array<std::uint8_t, 32>& transport_binding,
                                         const PublicKey& public_key);

}  // namespace dwell::protocol
