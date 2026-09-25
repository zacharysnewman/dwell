#include "dwell/core/server.h"

#include <algorithm>
#include <cmath>
#include <variant>

#include "dwell/core/crypto.h"
#include "dwell/player/net.h"

namespace dwell::core {

using namespace protocol;
using JPH::Vec3;
using player::PlayerHandle;

namespace {

constexpr int kSnapshotEvery = kSimHz / kSnapshotHz;  // ticks
constexpr std::size_t kMaxQueuedInputs = 16;          // hard cap per session
constexpr std::size_t kInputBuffer = 2;               // jitter buffer depth (ticks)
constexpr std::size_t kMaxBufferedInputs = 6;         // beyond this, skip to bound input latency
constexpr std::uint32_t kMaxSeqAhead = 256;           // inputs further ahead are rejected
constexpr std::uint32_t kInputDatagramsPerSecond = 2 * kSimHz;  // §11: SIM_HZ + redundancy
constexpr int kLaunchCooldownTicks = kSimHz / 2;

// §11: move within the unit circle (plus quantization slack), pitch within ±90°.
bool InputInRange(const InputFrame& f) {
  const int mx = f.move_x, my = f.move_y;
  return mx >= -127 && my >= -127 && mx * mx + my * my <= 127 * 127 + 254 && f.pitch >= -32767;
}

}  // namespace

Server::Server(ServerConfig config, Entropy& entropy, JPH::JobSystem& jobs)
    : config_(std::move(config)),
      entropy_(entropy),
      physics_(jobs),
      world_(GeneratorFor(config_.generator_version, config_.world_seed)),
      terrain_(world_, physics_),
      players_(world_, physics_, &terrain_) {
  if (!config_.spawn) config_.spawn = SpawnPointFor(config_.generator_version, config_.world_seed);
}

Server::~Server() = default;

void Server::OnConnected(SessionId id, TransportKind kind, const TransportBinding& binding) {
  Session session;
  session.kind = kind;
  session.binding = binding;
  sessions_[id] = std::move(session);
}

void Server::OnDisconnected(SessionId id) { RemoveSession(id); }

void Server::RemoveSession(SessionId id) {
  const auto it = sessions_.find(id);
  if (it == sessions_.end()) return;
  if (it->second.handle) players_.Despawn(*it->second.handle);
  sessions_.erase(it);
}

void Server::OnReliable(SessionId id, Channel channel, std::span<const std::uint8_t> bytes) {
  const auto it = sessions_.find(id);
  if (it == sessions_.end()) return;
  // Clients may only send on the control channel; the world channel is server → client.
  const auto message = channel == Channel::kControl ? Decode(bytes) : std::nullopt;
  if (!message) {
    Reject(id, RejectReason::kMalformed, "Malformed message.");
    return;
  }
  HandleControl(id, it->second, *message);
}

void Server::OnDatagram(SessionId id, std::span<const std::uint8_t> bytes) {
  const auto it = sessions_.find(id);
  if (it == sessions_.end() || it->second.phase != Phase::kJoined) return;
  Session& s = it->second;
  // Datagrams are unreliable: malformed ones are dropped, not fatal.
  const auto message = Decode(bytes);
  if (!message) return;
  if (const auto* ping = std::get_if<DatagramPing>(&*message)) {
    SendDatagram(id, DatagramPong{ping->seq, ping->client_time_ms, tick_});
  } else if (const auto* input = std::get_if<PlayerInput>(&*message)) {
    HandleInput(s, *input);
  }
}

void Server::HandleInput(Session& s, const PlayerInput& m) {
  // Rate limit (§11): at most kInputDatagramsPerSecond per SIM_HZ-tick window.
  if (tick_ - s.rate_window_tick >= static_cast<std::uint32_t>(kSimHz)) {
    s.rate_window_tick = tick_;
    s.rate_window_count = 0;
  }
  if (++s.rate_window_count > kInputDatagramsPerSecond) {
    ++s.stats.datagrams_dropped;
    return;
  }
  for (const InputFrame& f : m.inputs) {
    if (f.seq <= s.last_processed_seq) continue;  // redundant copy of a processed input
    const bool queued = std::any_of(s.inputs.begin(), s.inputs.end(),
                                    [&](const QueuedInput& q) { return q.seq == f.seq; });
    if (queued) continue;
    if (!InputInRange(f) || f.seq > s.last_processed_seq + kMaxSeqAhead) {
      ++s.stats.inputs_rejected;
      continue;
    }
    ++s.stats.inputs_received;
    const auto at = std::find_if(s.inputs.begin(), s.inputs.end(),
                                 [&](const QueuedInput& q) { return q.seq > f.seq; });
    s.inputs.insert(at, {f.seq, player::DequantizeInput(f)});
    while (s.inputs.size() > kMaxQueuedInputs) s.inputs.pop_front();
  }
}

void Server::HandleControl(SessionId id, Session& s, const Message& m) {
  if (std::holds_alternative<StatusRequest>(m)) {
    SendReliable(id, StatusResponse{kProtocolVersion, config_.name, config_.motd,
                                    static_cast<std::uint16_t>(joined_players()),
                                    config_.max_players, /*flags=*/0});
    return;
  }
  if (const auto* ping = std::get_if<Ping>(&m)) {
    SendReliable(id, Pong{ping->seq, ping->client_time_ms, tick_, time_ms()});
    return;
  }

  switch (s.phase) {
    case Phase::kAwaitingHello: {
      const auto* hello = std::get_if<ClientHello>(&m);
      if (!hello) break;
      if (hello->protocol_version != kProtocolVersion) {
        Reject(id, RejectReason::kProtocolVersion,
               "Server runs protocol " + std::to_string(kProtocolVersion) + ", client runs " +
                   std::to_string(hello->protocol_version) + ".");
        return;
      }
      if (joined_players() >= config_.max_players) {
        Reject(id, RejectReason::kFull, "Server is full.");
        return;
      }
      s.public_key = hello->public_key;
      s.display_name = hello->display_name.empty() ? "Player" : hello->display_name;
      entropy_.Fill(s.nonce);
      s.phase = Phase::kAwaitingAuth;
      SendReliable(id, Challenge{s.nonce});
      return;
    }
    case Phase::kAwaitingAuth: {
      const auto* auth = std::get_if<ClientAuth>(&m);
      if (!auth) break;
      const auto transcript = AuthTranscript(s.nonce, s.binding, s.public_key);
      if (!VerifyEd25519(auth->signature, s.public_key, transcript)) {
        Reject(id, RejectReason::kAuthFailed, "Identity check failed.");
        return;
      }
      // The signature proves key ownership, so a new login replaces any older session for the
      // same player (e.g. one whose connection dropped but hasn't timed out yet).
      std::vector<SessionId> replaced;
      for (const auto& [other_id, other] : sessions_) {
        if (other_id != id && other.phase == Phase::kJoined && other.public_key == s.public_key) {
          replaced.push_back(other_id);
        }
      }
      for (const auto other_id : replaced) {
        Reject(other_id, RejectReason::kReplaced, "Signed in from another connection.");
      }
      Session& joined = sessions_.at(id);  // Reject() may have rehashed the map
      joined.player_id = AllocatePlayerId();
      joined.phase = Phase::kJoined;
      SendReliable(id,
                   Welcome{joined.player_id, config_.world_seed, config_.generator_version, tick_});
      SpawnPlayer(joined);
      return;
    }
    case Phase::kJoined:
      // Joined clients send gameplay input as datagrams; no further control messages yet.
      return;
  }
  Reject(id, RejectReason::kMalformed, "Unexpected message.");
}

void Server::SpawnPlayer(Session& s) {
  // Spread players around the spawn point so they don't start inside each other.
  const int slot = (s.player_id - 1) % 8;
  const auto& at = *config_.spawn;
  const Vec3 spawn(at[0] + static_cast<float>(slot % 4) - 1.5f, at[1],
                   at[2] + static_cast<float>(slot / 4) * 1.5f - 0.75f);
  s.handle = players_.Spawn(player_config_, spawn);
  s.health = kMaxHealth;
  s.inputs.clear();
  s.primed = false;
  s.last_input = {};
}

void Server::Knockback(std::uint16_t player_id, Vec3 delta_v) {
  knockbacks_.emplace_back(player_id, delta_v);
}

void Server::Damage(Session& s, int amount, DamageCause cause) {
  if (!s.handle || amount <= 0) return;
  s.health = std::max(0, s.health - amount);
  PlayerEvent e;
  e.kind = PlayerEventKind::kDamage;
  e.player_id = s.player_id;
  e.server_tick = tick_;
  e.input_seq = s.last_processed_seq;
  e.amount = static_cast<std::uint8_t>(std::min(amount, 255));
  e.cause = cause;
  BroadcastWorld(e);
  if (s.health == 0) Kill(s, cause);
}

void Server::Kill(Session& s, DamageCause cause) {
  if (!s.handle) return;
  const Vec3 p = players_.Position(*s.handle);
  s.death_position[0] = p.GetX();
  s.death_position[1] = p.GetY() - players_.HalfHeight(*s.handle);
  s.death_position[2] = p.GetZ();
  players_.Despawn(*s.handle);
  s.handle.reset();
  s.health = 0;
  s.inputs.clear();
  s.respawn_tick = tick_ + static_cast<std::uint32_t>(kRespawnSeconds * kSimHz);
  PlayerEvent e;
  e.kind = PlayerEventKind::kDeath;
  e.player_id = s.player_id;
  e.server_tick = tick_;
  e.input_seq = s.last_processed_seq;
  e.cause = cause;
  BroadcastWorld(e);
}

// Server-side effects of the controller tick: fall damage, the void, and debug launch pads.
void Server::AfterControllerTick(Session& s) {
  if (!s.handle) return;
  const PlayerHandle h = *s.handle;
  const player::PlayerController& c = players_.controller(h);
  if (c.events & player::Events::kLanded) {
    const auto& d = player_config_.damage;
    if (c.landed_speed > d.fall_damage_min_speed) {
      Damage(s,
             static_cast<int>(
                 std::lround((c.landed_speed - d.fall_damage_min_speed) * d.fall_damage_per_speed)),
             DamageCause::kFall);
      if (!s.handle) return;
    }
  }
  const Vec3 p = players_.Position(h);
  const float feet = p.GetY() - players_.HalfHeight(h);
  if (feet < static_cast<float>(kWorldMinY - 16)) {
    Kill(s, DamageCause::kFall);
    return;
  }
  // Debug launch pad (Phase 2): a server-originated knockback, predicted by replay on the client.
  if (c.ground.grounded && c.ground.ground.kind == player::GroundRef::kTerrain &&
      tick_ >= s.launch_ready_tick) {
    const auto x = static_cast<std::int32_t>(std::floor(p.GetX()));
    const auto y = static_cast<std::int32_t>(std::floor(feet - 0.05f));
    const auto z = static_cast<std::int32_t>(std::floor(p.GetZ()));
    const float launch = GetMaterial(world_.GetVoxel(x, y, z)).launch_speed;
    if (launch > 0.0f) {
      Knockback(s.player_id, Vec3(0, launch - players_.Velocity(h).GetY(), 0));
      s.launch_ready_tick = tick_ + kLaunchCooldownTicks;
    }
  }
}

void Server::Step() {
  // 1. One input per player per tick, in sequence order (§9.3). Starved: repeat the last one.
  for (auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined || !s.handle) continue;
    while (s.inputs.size() > kMaxBufferedInputs) {
      s.last_processed_seq = s.inputs.front().seq;
      s.inputs.pop_front();
      ++s.stats.inputs_skipped;
    }
    // Jitter buffer: after starving, wait until kInputBuffer inputs are queued again.
    if (!s.primed && s.inputs.size() >= kInputBuffer) s.primed = true;
    if (s.primed && !s.inputs.empty()) {
      s.last_input = s.inputs.front().input;
      s.last_processed_seq = s.inputs.front().seq;
      s.inputs.pop_front();
    } else {
      s.primed = false;
      ++s.stats.ticks_starved;
    }
    players_.SetInput(*s.handle, s.last_input);
  }

  // 2. Controller pipeline (PLAYER_CONTROLLER.md §4), then its server-side consequences.
  players_.Tick();
  for (auto& [id, s] : sessions_) {
    if (s.phase == Phase::kJoined) AfterControllerTick(s);
  }
  for (const auto& [player_id, delta_v] : knockbacks_) {
    Session* s = SessionOfPlayer(player_id);
    if (!s || !s->handle) continue;
    players_.AddVelocity(*s->handle, delta_v);
    s->last_knockback_seq = s->last_processed_seq;
    PlayerEvent e;
    e.kind = PlayerEventKind::kKnockback;
    e.player_id = player_id;
    e.server_tick = tick_;
    e.input_seq = s->last_processed_seq;
    e.vector[0] = delta_v.GetX();
    e.vector[1] = delta_v.GetY();
    e.vector[2] = delta_v.GetZ();
    BroadcastWorld(e);
  }
  knockbacks_.clear();

  // 3. Physics.
  physics_.Step(1.0f / kSimHz);
  ++tick_;

  // 4. Respawns, then snapshots at SNAPSHOT_HZ.
  for (auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined || s.handle || tick_ < s.respawn_tick) continue;
    SpawnPlayer(s);
    const Vec3 p = players_.Position(*s.handle);
    PlayerEvent e;
    e.kind = PlayerEventKind::kRespawn;
    e.player_id = s.player_id;
    e.server_tick = tick_;
    e.input_seq = s.last_processed_seq;
    e.vector[0] = p.GetX();
    e.vector[1] = p.GetY() - players_.HalfHeight(*s.handle);
    e.vector[2] = p.GetZ();
    BroadcastWorld(e);
  }
  if (tick_ % kSnapshotEvery == 0) SendSnapshots();
}

void Server::SendSnapshots() {
  // Remote view of every joined player (dead ones where they died).
  std::vector<std::pair<std::uint16_t, RemotePlayerState>> views;
  for (const auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    RemotePlayerState r;
    r.player_id = s.player_id;
    if (s.handle) {
      const player::PlayerController& c = players_.controller(*s.handle);
      const Vec3 p = players_.Position(*s.handle), v = players_.Velocity(*s.handle);
      r.position[0] = p.GetX();
      r.position[1] = p.GetY() - players_.HalfHeight(*s.handle);  // feet
      r.position[2] = p.GetZ();
      r.velocity[0] = v.GetX();
      r.velocity[1] = v.GetY();
      r.velocity[2] = v.GetZ();
      r.yaw = player::QuantizeYaw(c.input.look_yaw);
      r.pitch = player::QuantizePitch(c.input.look_pitch);
      r.state = static_cast<PlayerState>(c.state);
      r.flags = player::PlayerFlagsOf(c, false);
    } else {
      std::copy(std::begin(s.death_position), std::end(s.death_position), r.position);
      r.flags = PlayerFlags::kDead;
    }
    views.emplace_back(s.player_id, r);
  }

  const player::GroundToNet ground_to_net = [this](const player::GroundRef& g) -> std::uint16_t {
    return g.kind == player::GroundRef::kPlayer ? PlayerIdOfBody(g.id) : 0;
  };
  for (const auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    PhysicsSnapshot snap;
    snap.server_tick = tick_;
    snap.ack_input_seq = s.last_processed_seq;
    LocalPlayerState& l = snap.local;
    l.health = static_cast<std::uint8_t>(s.health);
    l.input_buffer = static_cast<std::uint8_t>(std::min<std::size_t>(s.inputs.size(), 255));
    l.last_knockback_seq = s.last_knockback_seq;
    if (s.handle) {
      const player::PlayerController& c = players_.controller(*s.handle);
      const Vec3 p = players_.Position(*s.handle), v = players_.Velocity(*s.handle);
      l.position[0] = p.GetX();
      l.position[1] = p.GetY();  // capsule centre (resumes the body exactly)
      l.position[2] = p.GetZ();
      l.velocity[0] = v.GetX();
      l.velocity[1] = v.GetY();
      l.velocity[2] = v.GetZ();
      l.flags = player::PlayerFlagsOf(c, false);
      l.state = static_cast<PlayerState>(c.state);
      l.controller = player::ToNet(c, ground_to_net);
    } else {
      std::copy(std::begin(s.death_position), std::end(s.death_position), l.position);
      l.flags = PlayerFlags::kDead;
    }
    // Remote players, nearest first, as many as fit in one datagram.
    constexpr std::size_t kLocalBytes = 9 + 32 + 47 + 20 + 1;
    constexpr std::size_t kRemoteBytes = 26;
    const std::size_t room = (kMaxDatagramBytes - kLocalBytes) / kRemoteBytes;
    std::vector<RemotePlayerState> remotes;
    for (const auto& [player_id, view] : views) {
      if (player_id != s.player_id) remotes.push_back(view);
    }
    auto distance = [&](const RemotePlayerState& r) {
      float d = 0;
      for (int i = 0; i < 3; ++i)
        d += (r.position[i] - l.position[i]) * (r.position[i] - l.position[i]);
      return d;
    };
    std::sort(remotes.begin(), remotes.end(),
              [&](const auto& a, const auto& b) { return distance(a) < distance(b); });
    if (remotes.size() > room) remotes.resize(room);
    snap.remotes = std::move(remotes);
    SendDatagram(id, snap);
  }
}

std::vector<Outgoing> Server::TakeOutbox() { return std::exchange(outbox_, {}); }

std::size_t Server::joined_players() const {
  return static_cast<std::size_t>(std::count_if(sessions_.begin(), sessions_.end(), [](auto& kv) {
    return kv.second.phase == Phase::kJoined;
  }));
}

Server::Session* Server::SessionOfPlayer(std::uint16_t player_id) {
  for (auto& [id, s] : sessions_) {
    if (s.phase == Phase::kJoined && s.player_id == player_id) return &s;
  }
  return nullptr;
}

const Server::Session* Server::SessionOfPlayer(std::uint16_t player_id) const {
  for (const auto& [id, s] : sessions_) {
    if (s.phase == Phase::kJoined && s.player_id == player_id) return &s;
  }
  return nullptr;
}

std::uint16_t Server::PlayerIdOfBody(std::uint32_t body_id) const {
  for (const auto& [id, s] : sessions_) {
    if (s.handle && players_.body(*s.handle).GetIndexAndSequenceNumber() == body_id) {
      return s.player_id;
    }
  }
  return 0;
}

std::optional<PlayerHandle> Server::PlayerHandleOf(std::uint16_t player_id) const {
  const Session* s = SessionOfPlayer(player_id);
  return s ? s->handle : std::nullopt;
}

std::optional<SessionStats> Server::StatsOf(std::uint16_t player_id) const {
  const Session* s = SessionOfPlayer(player_id);
  return s ? std::optional<SessionStats>(s->stats) : std::nullopt;
}

int Server::HealthOf(std::uint16_t player_id) const {
  const Session* s = SessionOfPlayer(player_id);
  return s ? s->health : -1;
}

void Server::SendReliable(SessionId id, const Message& m, Channel channel) {
  outbox_.push_back({id, Outgoing::Kind::kReliable, channel, Encode(m)});
}

void Server::SendDatagram(SessionId id, const Message& m) {
  outbox_.push_back({id, Outgoing::Kind::kDatagram, Channel::kControl, Encode(m)});
}

void Server::BroadcastWorld(const Message& m) {
  const auto bytes = Encode(m);
  for (const auto& [id, s] : sessions_) {
    if (s.phase == Phase::kJoined)
      outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kWorld, bytes});
  }
}

void Server::Reject(SessionId id, RejectReason reason, std::string message) {
  SendReliable(id, protocol::Reject{reason, std::move(message)});
  outbox_.push_back({id, Outgoing::Kind::kClose, Channel::kControl, {}});
  RemoveSession(id);
}

std::uint16_t Server::AllocatePlayerId() {
  // Player ids are unique among joined players; skip 0 and ids in use.
  for (;;) {
    const std::uint16_t candidate = next_player_id_++;
    if (candidate == 0) continue;
    const bool used = std::any_of(sessions_.begin(), sessions_.end(), [&](auto& kv) {
      return kv.second.phase == Phase::kJoined && kv.second.player_id == candidate;
    });
    if (!used) return candidate;
  }
}

}  // namespace dwell::core
