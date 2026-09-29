#include "dwell/protocol/messages.h"

#include <algorithm>
#include <climits>
#include <cmath>
#include <string_view>
#include <type_traits>
#include <unordered_map>

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
template <>
constexpr MessageType TypeOf<WorldgenCheck>() {
  return MessageType::kWorldgenCheck;
}
template <>
constexpr MessageType TypeOf<ChunkData>() {
  return MessageType::kChunkData;
}
template <>
constexpr MessageType TypeOf<ChunkUnload>() {
  return MessageType::kChunkUnload;
}

template <>
constexpr MessageType TypeOf<BlockEditRequest>() {
  return MessageType::kBlockEditRequest;
}
template <>
constexpr MessageType TypeOf<VoxelModification>() {
  return MessageType::kVoxelModification;
}
template <>
constexpr MessageType TypeOf<ChunkResync>() {
  return MessageType::kChunkResync;
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
  for (std::int32_t v : m.verification_chunk) w.I32(v);
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

void WritePos64(ByteWriter& w, const double (&v)[3]) {
  for (double x : v) w.F64(x);
}

void WritePosFix(ByteWriter& w, const double (&v)[3]) {
  for (double x : v) w.I32(ToFixedPosition(x));
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
  WritePos64(w, m.local.position);
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
    WritePosFix(w, r.position);
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
      Write3(w, m.vector);
      break;
    case PlayerEventKind::kRespawn:
      WritePos64(w, m.position);
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

void Write(ByteWriter& w, const WorldgenCheck& m) { w.U64(m.hash); }

// --- Chunk voxels: palette + RLE (§6.1) ---
// Voxels go on the wire in layer order (x fastest, then z, then y) so horizontal strata make long
// runs. Palette: u16 count (1..kChunkVolume), then the materials in order of first appearance.
// Runs until the chunk is full: LEB128 length (1..kChunkVolume, minimal encoding), then the palette
// index as u8 (palette ≤ 256 entries) or u16. The encoding is canonical: maximal runs, palette in
// first-appearance order, so encode(decode(bytes)) == bytes.
constexpr int WireToIndex(int i) {
  return (i & 31) | (((i >> 10) & 31) << 5) | (((i >> 5) & 31) << 10);
}

void WriteVoxels(ByteWriter& w, const std::vector<std::uint16_t>& voxels) {
  auto at = [&](int wire) -> std::uint16_t {
    const auto i = static_cast<std::size_t>(WireToIndex(wire));
    return i < voxels.size() ? voxels[i] : std::uint16_t{0};
  };
  // Runs first; the palette (first-appearance order) then needs one lookup per run.
  std::vector<std::pair<std::uint16_t, std::uint32_t>> runs;
  for (int i = 0; i < kChunkVolume; ++i) {
    const auto m = at(i);
    if (!runs.empty() && runs.back().first == m) {
      ++runs.back().second;
    } else {
      runs.emplace_back(m, 1);
    }
  }
  std::vector<std::uint16_t> palette;
  std::unordered_map<std::uint16_t, std::uint16_t> index;
  for (const auto& [m, length] : runs) {
    if (index.try_emplace(m, static_cast<std::uint16_t>(palette.size())).second) {
      palette.push_back(m);
    }
  }
  w.U16(static_cast<std::uint16_t>(palette.size()));
  for (std::uint16_t m : palette) w.U16(m);
  const bool wide = palette.size() > 256;
  for (const auto& [m, length] : runs) {
    for (std::uint32_t v = length;;) {
      if (v < 0x80) {
        w.U8(static_cast<std::uint8_t>(v));
        break;
      }
      w.U8(static_cast<std::uint8_t>((v & 0x7F) | 0x80));
      v >>= 7;
    }
    if (wide) {
      w.U16(index[m]);
    } else {
      w.U8(static_cast<std::uint8_t>(index[m]));
    }
  }
}

std::uint32_t ReadVarint(ByteReader& r) {
  std::uint32_t v = 0;
  for (int shift = 0; shift < 21; shift += 7) {
    const std::uint8_t b = r.U8();
    r.Check(shift == 0 || b != 0);  // minimal encoding only
    v |= static_cast<std::uint32_t>(b & 0x7F) << shift;
    if ((b & 0x80) == 0) return v;
  }
  r.Check(false);
  return 0;
}

std::vector<std::uint16_t> ReadVoxels(ByteReader& r) {
  const std::uint16_t count = r.U16();
  r.Check(count >= 1 && count <= kChunkVolume);
  if (!r.ok()) return {};
  std::vector<std::uint16_t> palette(count);
  for (auto& m : palette) m = r.U16();
  const bool wide = count > 256;
  std::vector<std::uint16_t> voxels(kChunkVolume);
  for (int filled = 0; filled < kChunkVolume && r.ok();) {
    const std::uint32_t run = ReadVarint(r);
    const std::uint16_t i = wide ? r.U16() : r.U8();
    r.Check(run >= 1 && run <= static_cast<std::uint32_t>(kChunkVolume - filled) && i < count);
    if (!r.ok()) break;
    for (std::uint32_t k = 0; k < run; ++k) voxels[WireToIndex(filled++)] = palette[i];
  }
  return voxels;
}

void WriteCoord(ByteWriter& w, const ChunkCoordNet& c) {
  for (std::int32_t v : c) w.I32(v);
}
ChunkCoordNet ReadCoord(ByteReader& r) { return {r.I32(), r.I32(), r.I32()}; }

void Write(ByteWriter& w, const ChunkData& m) {
  w.U8(static_cast<std::uint8_t>(m.form));
  WriteCoord(w, m.coord);
  w.U32(m.revision);
  if (m.form == ChunkForm::kExplicit) WriteVoxels(w, m.voxels);
}

void Write(ByteWriter& w, const ChunkUnload& m) {
  const std::size_t count = std::min<std::size_t>(m.coords.size(), 0xFFFF);
  w.U16(static_cast<std::uint16_t>(count));
  for (std::size_t i = 0; i < count; ++i) WriteCoord(w, m.coords[i]);
}

void Write(ByteWriter& w, const BlockEditRequest& m) {
  w.U8(static_cast<std::uint8_t>(m.action));
  WriteCoord(w, m.cell);
  w.U8(m.face);
  if (m.action == BlockEditAction::kPlace) w.U16(m.material);
}

void Write(ByteWriter& w, const VoxelModification& m) {
  w.U8(static_cast<std::uint8_t>(m.reason));
  w.U32(m.server_tick);
  const std::size_t count = std::min<std::size_t>(m.chunks.size(), 0xFFFF);
  w.U16(static_cast<std::uint16_t>(count));
  for (std::size_t i = 0; i < count; ++i) {
    const ChunkChanges& c = m.chunks[i];
    WriteCoord(w, c.coord);
    w.U32(c.revision);
    const std::size_t n = std::min<std::size_t>(c.changes.size(), 0xFFFF);
    w.U16(static_cast<std::uint16_t>(n));
    for (std::size_t k = 0; k < n; ++k) {
      w.U16(c.changes[k].index);
      w.U16(c.changes[k].material);
    }
  }
}

void Write(ByteWriter& w, const ChunkResync& m) {
  const std::size_t count = std::min(m.coords.size(), kMaxResyncChunks);
  w.U16(static_cast<std::uint16_t>(count));
  for (std::size_t i = 0; i < count; ++i) WriteCoord(w, m.coords[i]);
}

bool IsRejectReason(std::uint8_t v) { return v >= 1 && v <= kMaxRejectReason; }

void Read3(ByteReader& r, float (&v)[3]) {
  for (float& x : v) x = r.F32();
}

// Positions must be finite and inside the i32 posfix range (±8 388 km), like every other world
// position a peer may send.
void ReadPos64(ByteReader& r, double (&v)[3]) {
  constexpr double kLimit = 2147483647.0 / kPositionFixedScale;
  for (double& x : v) {
    x = r.F64();
    r.Check(x >= -kLimit && x <= kLimit);  // also rejects NaN
  }
}

void ReadPosFix(ByteReader& r, double (&v)[3]) {
  for (double& x : v) x = FromFixedPosition(r.I32());
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
      m.verification_chunk = ReadCoord(r);
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
      ReadPos64(r, m.local.position);
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
        ReadPosFix(r, p.position);
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
          Read3(r, m.vector);
          break;
        case PlayerEventKind::kRespawn:
          ReadPos64(r, m.position);
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
    case MessageType::kWorldgenCheck:
      out = WorldgenCheck{r.U64()};
      break;
    case MessageType::kChunkData: {
      ChunkData m;
      const auto form = r.U8();
      r.Check(form <= kMaxChunkForm);
      m.form = static_cast<ChunkForm>(form);
      m.coord = ReadCoord(r);
      m.revision = r.U32();
      if (r.ok() && m.form == ChunkForm::kExplicit) m.voxels = ReadVoxels(r);
      out = std::move(m);
      break;
    }
    case MessageType::kChunkUnload: {
      ChunkUnload m;
      const std::uint16_t count = r.U16();
      r.Check(count >= 1);
      for (int i = 0; i < count && r.ok(); ++i) m.coords.push_back(ReadCoord(r));
      out = std::move(m);
      break;
    }
    case MessageType::kBlockEditRequest: {
      BlockEditRequest m;
      const auto action = r.U8();
      r.Check(action >= 1 && action <= kMaxBlockEditAction);
      m.action = static_cast<BlockEditAction>(action);
      m.cell = ReadCoord(r);
      m.face = r.U8();
      r.Check(m.face < 6);
      if (r.ok() && m.action == BlockEditAction::kPlace) m.material = r.U16();
      out = m;
      break;
    }
    case MessageType::kVoxelModification: {
      VoxelModification m;
      const auto reason = r.U8();
      r.Check(reason >= 1 && reason <= kMaxVoxelModificationReason);
      m.reason = static_cast<VoxelModificationReason>(reason);
      m.server_tick = r.U32();
      const std::uint16_t count = r.U16();
      r.Check(count >= 1);
      for (int i = 0; i < count && r.ok(); ++i) {
        ChunkChanges c;
        c.coord = ReadCoord(r);
        c.revision = r.U32();
        const std::uint16_t n = r.U16();
        r.Check(n >= 1);
        for (int k = 0; k < n && r.ok(); ++k) {
          VoxelChange v;
          v.index = r.U16();
          v.material = r.U16();
          r.Check(v.index < kChunkVolume);
          c.changes.push_back(v);
        }
        m.chunks.push_back(std::move(c));
      }
      out = std::move(m);
      break;
    }
    case MessageType::kChunkResync: {
      ChunkResync m;
      const std::uint16_t count = r.U16();
      r.Check(count >= 1 && count <= kMaxResyncChunks);
      for (int i = 0; i < count && r.ok(); ++i) m.coords.push_back(ReadCoord(r));
      out = std::move(m);
      break;
    }
    default:
      return std::nullopt;
  }
  if (!r.AtEnd()) return std::nullopt;
  return out;
}

std::vector<std::uint8_t> EncodeChunkVoxels(const std::vector<std::uint16_t>& voxels) {
  std::vector<std::uint8_t> out;
  ByteWriter w(out);
  WriteVoxels(w, voxels);
  return out;
}

std::optional<std::vector<std::uint16_t>> DecodeChunkVoxels(std::span<const std::uint8_t> bytes) {
  ByteReader r(bytes);
  auto voxels = ReadVoxels(r);
  if (!r.ok() || !r.AtEnd()) return std::nullopt;
  return voxels;
}

std::int32_t ToFixedPosition(double v) {
  const double scaled = std::floor(v * kPositionFixedScale + 0.5);
  if (!(scaled > -2147483648.0)) return INT32_MIN;  // also NaN
  if (scaled > 2147483647.0) return INT32_MAX;
  return static_cast<std::int32_t>(scaled);
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
