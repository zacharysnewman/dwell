#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <variant>
#include <vector>

#include "dwell/protocol/constants.gen.h"

// Protocol v0 messages used in Phase 1 (ARCHITECTURE.md §8.3). Every message starts with a u8
// MessageType. Layouts are pinned by golden vectors in shared/protocol/vectors.txt.
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

using Message = std::variant<DatagramPing, DatagramPong, StatusRequest, StatusResponse, ClientHello,
                             Challenge, ClientAuth, Welcome, Reject, Ping, Pong>;

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
