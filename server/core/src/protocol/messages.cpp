#include "dwell/protocol/messages.h"

#include <algorithm>
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
template <>
constexpr MessageType TypeOf<PlayerInput>() {
  return MessageType::kPlayerInput;
}
template <>
constexpr MessageType TypeOf<PhysicsSnapshot>() {
  return MessageType::kPhysicsSnapshot;
}
template <>
constexpr MessageType TypeOf<PlayerEvent>() {
  return MessageType::kPlayerEvent;
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

void Write(ByteWriter& w, const PlayerInput& m) {
  w.U32(m.last_snapshot_tick);
  const std::size_t count = std::min(m.inputs.size(), kMaxInputsPerDatagram);
  w.U8(static_cast<std::uint8_t>(count));
  for (std::size_t i = m.inputs.size() - count; i < m.inputs.size(); ++i) {
    const InputFrame& f = m.inputs[i];
    w.U32(f.seq);
    w.I8(f.move_x);
    w.I8(f.move_y);
    w.U16(f.buttons);
    w.I16(f.yaw);
    w.I16(f.pitch);
  }
}

void Write3(ByteWriter& w, const float (&v)[3]) {
  for (float x : v) w.F32(x);
}

void Write(ByteWriter& w, const ControllerState& c) {
  w.U8(c.flags);
  for (float v :
       {c.current_x, c.current_z, c.external_x, c.external_z, c.contribution_x, c.contribution_z,
        c.accumulated_y, c.platform_y, c.target_y, c.ground_velocity_y}) {
    w.F32(v);
  }
  w.U8(static_cast<std::uint8_t>(c.ground_kind));
  w.U16(c.ground_id);
  w.U8(c.buffer_ticks);
  w.U8(c.coyote_ticks);
  w.U8(c.step_grace);
  if (c.flags & ControllerFlags::kClimbing) {
    w.I32(c.ladder_x);
    w.I32(c.ladder_y);
    w.I32(c.ladder_z);
  }
  if (c.flags & ControllerFlags::kHasReleased) {
    w.I32(c.released_x);
    w.I32(c.released_z);
  }
}

void Write(ByteWriter& w, const PhysicsSnapshot& m) {
  w.U32(m.server_tick);
  w.U32(m.ack_input_seq);
  Write3(w, m.local.position);
  Write3(w, m.local.velocity);
  w.U8(m.local.flags);
  w.U8(m.local.health);
  w.U8(static_cast<std::uint8_t>(m.local.state));
  w.U8(m.local.input_buffer);
  w.U32(m.local.last_knockback_seq);
  Write(w, m.local.controller);
  w.U8(static_cast<std::uint8_t>(std::min<std::size_t>(m.remotes.size(), 255)));
  for (std::size_t i = 0; i < m.remotes.size() && i < 255; ++i) {
    const RemotePlayerState& r = m.remotes[i];
    w.U16(r.player_id);
    Write3(w, r.position);
    for (float v : r.velocity) w.F16(v);
    w.I16(r.yaw);
    w.I16(r.pitch);
    w.U8(static_cast<std::uint8_t>(r.state));
    w.U8(r.flags);
  }
}

void Write(ByteWriter& w, const PlayerEvent& m) {
  w.U8(static_cast<std::uint8_t>(m.kind));
  w.U16(m.player_id);
  w.U32(m.server_tick);
  w.U32(m.input_seq);
  switch (m.kind) {
    case PlayerEventKind::kKnockback:
    case PlayerEventKind::kRespawn:
      Write3(w, m.vector);
      break;
    case PlayerEventKind::kDamage:
      w.U8(m.amount);
      w.U8(static_cast<std::uint8_t>(m.cause));
      break;
    case PlayerEventKind::kDeath:
      w.U8(static_cast<std::uint8_t>(m.cause));
      break;
  }
}

bool IsRejectReason(std::uint8_t v) { return v >= 1 && v <= kMaxRejectReason; }

void Read3(ByteReader& r, float (&v)[3]) {
  for (float& x : v) x = r.F32();
}

PlayerState ReadState(ByteReader& r) {
  const auto v = r.U8();
  r.Check(v <= kMaxPlayerState);
  return static_cast<PlayerState>(v);
}

DamageCause ReadCause(ByteReader& r) {
  const auto v = r.U8();
  r.Check(v >= 1 && v <= kMaxDamageCause);
  return static_cast<DamageCause>(v);
}

ControllerState ReadController(ByteReader& r) {
  ControllerState c;
  c.flags = r.U8();
  r.Check((c.flags & ~ControllerFlags::kAll) == 0);
  for (float* v :
       {&c.current_x, &c.current_z, &c.external_x, &c.external_z, &c.contribution_x,
        &c.contribution_z, &c.accumulated_y, &c.platform_y, &c.target_y, &c.ground_velocity_y}) {
    *v = r.F32();
  }
  const auto kind = r.U8();
  r.Check(kind <= kMaxGroundKind);
  c.ground_kind = static_cast<GroundKind>(kind);
  c.ground_id = r.U16();
  c.buffer_ticks = r.U8();
  c.coyote_ticks = r.U8();
  c.step_grace = r.U8();
  if (c.flags & ControllerFlags::kClimbing) {
    c.ladder_x = r.I32();
    c.ladder_y = r.I32();
    c.ladder_z = r.I32();
  }
  if (c.flags & ControllerFlags::kHasReleased) {
    c.released_x = r.I32();
    c.released_z = r.I32();
  }
  return c;
}

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
    case MessageType::kPlayerInput: {
      PlayerInput m;
      m.last_snapshot_tick = r.U32();
      const auto count = r.U8();
      r.Check(count >= 1 && count <= kMaxInputsPerDatagram);
      for (int i = 0; i < count && r.ok(); ++i) {
        InputFrame f;
        f.seq = r.U32();
        f.move_x = r.I8();
        f.move_y = r.I8();
        f.buttons = r.U16();
        f.yaw = r.I16();
        f.pitch = r.I16();
        r.Check((f.buttons & ~InputButtons::kAll) == 0);
        m.inputs.push_back(f);
      }
      out = std::move(m);
      break;
    }
    case MessageType::kPhysicsSnapshot: {
      PhysicsSnapshot m;
      m.server_tick = r.U32();
      m.ack_input_seq = r.U32();
      Read3(r, m.local.position);
      Read3(r, m.local.velocity);
      m.local.flags = r.U8();
      r.Check((m.local.flags & ~PlayerFlags::kAll) == 0);
      m.local.health = r.U8();
      m.local.state = ReadState(r);
      m.local.input_buffer = r.U8();
      m.local.last_knockback_seq = r.U32();
      m.local.controller = ReadController(r);
      const auto count = r.U8();
      for (int i = 0; i < count && r.ok(); ++i) {
        RemotePlayerState p;
        p.player_id = r.U16();
        Read3(r, p.position);
        for (float& v : p.velocity) v = r.F16();
        p.yaw = r.I16();
        p.pitch = r.I16();
        p.state = ReadState(r);
        p.flags = r.U8();
        r.Check((p.flags & ~PlayerFlags::kAll) == 0);
        m.remotes.push_back(p);
      }
      out = std::move(m);
      break;
    }
    case MessageType::kPlayerEvent: {
      PlayerEvent m;
      const auto kind = r.U8();
      r.Check(kind >= 1 && kind <= kMaxPlayerEventKind);
      m.kind = static_cast<PlayerEventKind>(kind);
      m.player_id = r.U16();
      m.server_tick = r.U32();
      m.input_seq = r.U32();
      if (!r.ok()) return std::nullopt;
      switch (m.kind) {
        case PlayerEventKind::kKnockback:
        case PlayerEventKind::kRespawn:
          Read3(r, m.vector);
          break;
        case PlayerEventKind::kDamage:
          m.amount = r.U8();
          m.cause = ReadCause(r);
          break;
        case PlayerEventKind::kDeath:
          m.cause = ReadCause(r);
          break;
      }
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
