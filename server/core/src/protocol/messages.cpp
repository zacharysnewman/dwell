#include "dwell/protocol/messages.h"

#include <string_view>
#include <type_traits>

#include "dwell/protocol/bytes.h"

namespace dwell::protocol {
namespace {

// Truncates to at most `max_bytes` without splitting a UTF-8 sequence.
std::string_view Clamp(std::string_view s, std::size_t max_bytes) {
  if (s.size() <= max_bytes) return s;
  std::size_t end = max_bytes;
  while (end > 0 && (static_cast<unsigned char>(s[end]) & 0xC0) == 0x80) --end;
  return s.substr(0, end);
}

template <typename T>
constexpr MessageType TypeOf();
template <>
constexpr MessageType TypeOf<DatagramPing>() {
  return MessageType::kDatagramPing;
}
template <>
constexpr MessageType TypeOf<DatagramPong>() {
  return MessageType::kDatagramPong;
}
template <>
constexpr MessageType TypeOf<StatusRequest>() {
  return MessageType::kStatusRequest;
}
template <>
constexpr MessageType TypeOf<StatusResponse>() {
  return MessageType::kStatusResponse;
}
template <>
constexpr MessageType TypeOf<ClientHello>() {
  return MessageType::kClientHello;
}
template <>
constexpr MessageType TypeOf<Challenge>() {
  return MessageType::kChallenge;
}
template <>
constexpr MessageType TypeOf<ClientAuth>() {
  return MessageType::kClientAuth;
}
template <>
constexpr MessageType TypeOf<Welcome>() {
  return MessageType::kWelcome;
}
template <>
constexpr MessageType TypeOf<Reject>() {
  return MessageType::kReject;
}
template <>
constexpr MessageType TypeOf<Ping>() {
  return MessageType::kPing;
}
template <>
constexpr MessageType TypeOf<Pong>() {
  return MessageType::kPong;
}

void Write(ByteWriter& w, const DatagramPing& m) {
  w.U32(m.seq);
  w.F64(m.client_time_ms);
}
void Write(ByteWriter& w, const DatagramPong& m) {
  w.U32(m.seq);
  w.F64(m.client_time_ms);
  w.U32(m.server_tick);
}
void Write(ByteWriter&, const StatusRequest&) {}
void Write(ByteWriter& w, const StatusResponse& m) {
  w.U16(m.protocol_version);
  w.Str(Clamp(m.server_name, kServerNameMaxBytes));
  w.Str(Clamp(m.motd, kMotdMaxBytes));
  w.U16(m.players);
  w.U16(m.max_players);
  w.U8(m.flags);
}
void Write(ByteWriter& w, const ClientHello& m) {
  w.U16(m.protocol_version);
  w.Str(Clamp(m.client_version, kClientVersionMaxBytes));
  w.Bytes(m.public_key);
  w.Str(Clamp(m.display_name, kDisplayNameMaxBytes));
}
void Write(ByteWriter& w, const Challenge& m) { w.Bytes(m.nonce); }
void Write(ByteWriter& w, const ClientAuth& m) { w.Bytes(m.signature); }
void Write(ByteWriter& w, const Welcome& m) {
  w.U16(m.player_id);
  w.U64(m.world_seed);
  w.U32(m.generator_version);
  w.U32(m.server_tick);
}
void Write(ByteWriter& w, const Reject& m) {
  w.U8(static_cast<std::uint8_t>(m.reason));
  w.Str(Clamp(m.message, kRejectMessageMaxBytes));
}
void Write(ByteWriter& w, const Ping& m) {
  w.U32(m.seq);
  w.F64(m.client_time_ms);
}
void Write(ByteWriter& w, const Pong& m) {
  w.U32(m.seq);
  w.F64(m.client_time_ms);
  w.U32(m.server_tick);
  w.F64(m.server_time_ms);
}

bool IsRejectReason(std::uint8_t v) { return v >= 1 && v <= 6; }

}  // namespace

void Encode(const Message& message, std::vector<std::uint8_t>& out) {
  ByteWriter w(out);
  std::visit(
      [&](const auto& m) {
        w.U8(static_cast<std::uint8_t>(TypeOf<std::decay_t<decltype(m)>>()));
        Write(w, m);
      },
      message);
}

std::vector<std::uint8_t> Encode(const Message& message) {
  std::vector<std::uint8_t> out;
  Encode(message, out);
  return out;
}

std::optional<Message> Decode(std::span<const std::uint8_t> bytes) {
  ByteReader r(bytes);
  const auto type = static_cast<MessageType>(r.U8());
  if (!r.ok()) return std::nullopt;

  Message out;
  switch (type) {
    case MessageType::kDatagramPing: {
      DatagramPing m;
      m.seq = r.U32();
      m.client_time_ms = r.F64();
      out = m;
      break;
    }
    case MessageType::kDatagramPong: {
      DatagramPong m;
      m.seq = r.U32();
      m.client_time_ms = r.F64();
      m.server_tick = r.U32();
      out = m;
      break;
    }
    case MessageType::kStatusRequest:
      out = StatusRequest{};
      break;
    case MessageType::kStatusResponse: {
      StatusResponse m;
      m.protocol_version = r.U16();
      m.server_name = r.Str(kServerNameMaxBytes);
      m.motd = r.Str(kMotdMaxBytes);
      m.players = r.U16();
      m.max_players = r.U16();
      m.flags = r.U8();
      out = std::move(m);
      break;
    }
    case MessageType::kClientHello: {
      ClientHello m;
      m.protocol_version = r.U16();
      m.client_version = r.Str(kClientVersionMaxBytes);
      m.public_key = r.Fixed<32>();
      m.display_name = r.Str(kDisplayNameMaxBytes);
      out = std::move(m);
      break;
    }
    case MessageType::kChallenge:
      out = Challenge{r.Fixed<32>()};
      break;
    case MessageType::kClientAuth:
      out = ClientAuth{r.Fixed<64>()};
      break;
    case MessageType::kWelcome: {
      Welcome m;
      m.player_id = r.U16();
      m.world_seed = r.U64();
      m.generator_version = r.U32();
      m.server_tick = r.U32();
      out = m;
      break;
    }
    case MessageType::kReject: {
      Reject m;
      const auto reason = r.U8();
      if (!IsRejectReason(reason)) return std::nullopt;
      m.reason = static_cast<RejectReason>(reason);
      m.message = r.Str(kRejectMessageMaxBytes);
      out = std::move(m);
      break;
    }
    case MessageType::kPing: {
      Ping m;
      m.seq = r.U32();
      m.client_time_ms = r.F64();
      out = m;
      break;
    }
    case MessageType::kPong: {
      Pong m;
      m.seq = r.U32();
      m.client_time_ms = r.F64();
      m.server_tick = r.U32();
      m.server_time_ms = r.F64();
      out = m;
      break;
    }
    default:
      return std::nullopt;
  }
  if (!r.AtEnd()) return std::nullopt;
  return out;
}

std::vector<std::uint8_t> AuthTranscript(const Nonce& nonce,
                                         const std::array<std::uint8_t, 32>& transport_binding,
                                         const PublicKey& public_key) {
  std::vector<std::uint8_t> out(kAuthDomainTag.begin(), kAuthDomainTag.end());
  out.insert(out.end(), nonce.begin(), nonce.end());
  out.insert(out.end(), transport_binding.begin(), transport_binding.end());
  out.insert(out.end(), public_key.begin(), public_key.end());
  return out;
}

}  // namespace dwell::protocol
