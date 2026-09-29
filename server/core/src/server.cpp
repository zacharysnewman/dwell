#include "dwell/core/server.h"

#include <algorithm>
#include <cmath>
#include <variant>

#include "dwell/core/crypto.h"
#include "dwell/player/net.h"

namespace dwell::core {

using namespace protocol;
using JPH::RVec3;
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
constexpr int kPrefetchChunks = 2;  // generated ahead around each player (collision reach + 1)
constexpr int kKeepChunks = 3;      // unmodified chunks kept around players; farther: evicted
constexpr std::uint32_t kEvictEveryTicks = 64;
constexpr std::size_t kMaxExplicitWanted = 64;  // full-mode chunks queued for generation per tick

ChunkCoord ChunkAt(double x, double y, double z) {
  return ChunkOf(static_cast<std::int32_t>(std::floor(x)), static_cast<std::int32_t>(std::floor(y)),
                 static_cast<std::int32_t>(std::floor(z)));
}

// Offsets within Chebyshev distance `r`, nearest first.
std::vector<ChunkCoord> CubeOffsets(int r) {
  std::vector<ChunkCoord> out;
  for (int y = -r; y <= r; ++y)
    for (int z = -r; z <= r; ++z)
      for (int x = -r; x <= r; ++x) out.push_back({x, y, z});
  std::stable_sort(out.begin(), out.end(), [](const ChunkCoord& a, const ChunkCoord& b) {
    return a.x * a.x + a.y * a.y + a.z * a.z < b.x * b.x + b.y * b.y + b.z * b.z;
  });
  return out;
}

const std::vector<ChunkCoord>& PrefetchOffsets() {
  static const std::vector<ChunkCoord> offsets = CubeOffsets(kPrefetchChunks);
  return offsets;
}

ChunkCoord Add(const ChunkCoord& a, const ChunkCoord& b) {
  return {a.x + b.x, a.y + b.y, a.z + b.z};
}

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
      air_test_(config_.generator_override
                    ? config_.air_test_override
                    : AirTestFor(config_.generator_version, config_.world_seed)),
      world_(config_.generator_override
                 ? config_.generator_override
                 : GeneratorFor(config_.generator_version, config_.world_seed),
             air_test_),
      terrain_(world_, physics_),
      players_(world_, physics_, &terrain_),
      worldgen_(config_.generator_override
                    ? config_.generator_override
                    : GeneratorFor(config_.generator_version, config_.world_seed),
                config_.worldgen_threads) {
  if (!config_.spawn) config_.spawn = SpawnPointFor(config_.generator_version, config_.world_seed);
  const auto& at = *config_.spawn;
  const ChunkCoord spawn_chunk = ChunkAt(at[0], at[1], at[2]);
  verification_chunk_ = ChunkAt(at[0], at[1] - 1.0, at[2]);

  // Pre-generate the spawn region, so the first players never wait for terrain.
  std::vector<ChunkCoord> region;
  for (const ChunkCoord& o : CubeOffsets(config_.pregen_radius_chunks)) {
    if (const ChunkCoord c = Add(spawn_chunk, o); !IsAir(c)) region.push_back(c);
  }
  region.push_back(verification_chunk_);
  worldgen_.SetWanted(region);
  std::vector<WorldgenPool::Result> ready;
  worldgen_.Drain(ready);
  for (auto& [coord, chunk] : ready) world_.Put(coord, std::move(chunk));
  verification_hash_ = ChunkHash(world_.Read(verification_chunk_));

  // The view: a sphere of view_radius_chunks around the center (§6.3).
  const int r = config_.view_radius_chunks;
  for (int y = -r; y <= r; ++y)
    for (int z = -r; z <= r; ++z)
      for (int x = -r; x <= r; ++x)
        if (x * x + y * y + z * z <= r * r + r) view_offsets_.push_back({x, y, z});
  std::stable_sort(view_offsets_.begin(), view_offsets_.end(),
                   [](const ChunkCoord& a, const ChunkCoord& b) {
                     return a.x * a.x + a.y * a.y + a.z * a.z < b.x * b.x + b.y * b.y + b.z * b.z;
                   });
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
                   Welcome{joined.player_id,
                           config_.world_seed,
                           config_.generator_version,
                           tick_,
                           {verification_chunk_.x, verification_chunk_.y, verification_chunk_.z}});
      SpawnPlayer(joined);
      return;
    }
    case Phase::kJoined:
      // Gameplay input comes as datagrams. On control: the one WorldgenCheck, which picks how the
      // client receives chunks (§6.3); anything else is ignored.
      if (const auto* check = std::get_if<WorldgenCheck>(&m);
          check && s.chunk_mode == ChunkMode::kAwaitingCheck) {
        s.chunk_mode = check->hash != 0 && check->hash == verification_hash_ ? ChunkMode::kGenerated
                                                                             : ChunkMode::kFull;
      }
      return;
  }
  Reject(id, RejectReason::kMalformed, "Unexpected message.");
}

void Server::SpawnPlayer(Session& s) {
  // Spread players around the spawn point so they don't start inside each other.
  const int slot = (s.player_id - 1) % 8;
  const auto& at = *config_.spawn;
  const RVec3 spawn(at[0] + (slot % 4) - 1.5, at[1], at[2] + (slot / 4) * 1.5 - 0.75);
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
  const RVec3 p = players_.Position(*s.handle);
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
  const RVec3 p = players_.Position(h);
  const float feet = players_.Feet(h);
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
  // 0. Chunks generated since the last tick (off-thread, or here within the budget).
  {
    std::vector<WorldgenPool::Result> ready;
    worldgen_.Collect(ready, std::chrono::microseconds(config_.worldgen_budget_us));
    for (auto& [coord, chunk] : ready) {
      if (!world_.Find(coord)) world_.Put(coord, std::move(chunk));
    }
  }

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
    const RVec3 p = players_.Position(*s.handle);
    PlayerEvent e;
    e.kind = PlayerEventKind::kRespawn;
    e.player_id = s.player_id;
    e.server_tick = tick_;
    e.input_seq = s.last_processed_seq;
    e.position[0] = p.GetX();
    e.position[1] = p.GetY() - players_.HalfHeight(*s.handle);
    e.position[2] = p.GetZ();
    BroadcastWorld(e);
  }
  if (tick_ % kSnapshotEvery == 0) SendSnapshots();

  // 5. Terrain streaming, then what to generate next and what to forget.
  explicit_wanted_.clear();
  for (auto& [id, s] : sessions_) StreamChunks(id, s);
  UpdateWorldgen();
}

std::optional<ChunkCoord> Server::ViewCenter(const Session& s) const {
  if (s.phase != Phase::kJoined) return std::nullopt;
  if (s.handle) {
    const RVec3 p = players_.Position(*s.handle);
    return ChunkAt(p.GetX(), p.GetY(), p.GetZ());
  }
  return ChunkAt(s.death_position[0], s.death_position[1], s.death_position[2]);
}

void Server::UpdateWorldgen() {
  std::vector<ChunkCoord> centers;
  for (const auto& [id, s] : sessions_) {
    if (const auto c = ViewCenter(s)) centers.push_back(*c);
  }
  // Around players first (collision needs them), then chunks full-mode clients are waiting for.
  std::vector<ChunkCoord> wanted;
  for (const ChunkCoord& o : PrefetchOffsets()) {
    for (const ChunkCoord& center : centers) {
      const ChunkCoord c = Add(center, o);
      if (c.y >= kMinChunkY && c.y <= kMaxChunkY && !world_.Find(c) && !IsAir(c)) {
        wanted.push_back(c);
      }
    }
  }
  wanted.insert(wanted.end(), explicit_wanted_.begin(), explicit_wanted_.end());
  worldgen_.SetWanted(wanted);

  if (tick_ % kEvictEveryTicks == 0) {
    world_.EvictUnmodified([&](const ChunkCoord& c) {
      return c == verification_chunk_ ||
             std::any_of(centers.begin(), centers.end(), [&](const ChunkCoord& center) {
               return ChunkDistance(c, center) <= kKeepChunks;
             });
    });
  }
}

void Server::StreamChunks(SessionId id, Session& s) {
  if (s.chunk_mode == ChunkMode::kAwaitingCheck) return;
  const auto center = ViewCenter(s);
  if (!center) return;
  const double per_tick = static_cast<double>(config_.chunk_bytes_per_second) / kSimHz;
  s.chunk_credit = std::min(s.chunk_credit + per_tick, per_tick);

  if (!s.stream_center || !(*s.stream_center == *center)) {
    s.stream_center = center;
    s.stream_complete = false;
    // Chunks beyond the view plus a margin (hysteresis) leave the client.
    const int r = config_.view_radius_chunks + kUnloadMarginChunks;
    ChunkUnload unload;
    for (auto it = s.streamed.begin(); it != s.streamed.end();) {
      const int dx = it->x - center->x, dy = it->y - center->y, dz = it->z - center->z;
      if (dx * dx + dy * dy + dz * dz > r * r + r) {
        unload.coords.push_back({it->x, it->y, it->z});
        it = s.streamed.erase(it);
      } else {
        ++it;
      }
    }
    s.stream_stats.unloaded += static_cast<std::uint32_t>(unload.coords.size());
    while (!unload.coords.empty()) {
      const std::size_t n = std::min<std::size_t>(unload.coords.size(), 0xFFFF);
      ChunkUnload part;
      part.coords.assign(unload.coords.end() - static_cast<std::ptrdiff_t>(n), unload.coords.end());
      unload.coords.resize(unload.coords.size() - n);
      SendReliable(id, part, Channel::kWorld);
    }
  }
  if (s.stream_complete) return;

  // Nearest first, within the bandwidth budget (§6.3).
  bool complete = true;
  int sent = 0;
  for (const ChunkCoord& o : view_offsets_) {
    const ChunkCoord c = Add(*center, o);
    if (c.y < kMinChunkY || c.y > kMaxChunkY || s.streamed.count(c)) continue;
    complete = false;
    if (s.chunk_credit <= 0 || sent >= kMaxChunksPerTick) break;
    const Chunk* chunk = world_.Find(c);
    ChunkData m;
    m.coord = {c.x, c.y, c.z};
    const bool modified = chunk && chunk->revision() > 0;
    if (!modified && IsAir(c)) {
      m.form = ChunkForm::kAir;  // nothing to generate or store, in either mode
    } else if (s.chunk_mode == ChunkMode::kGenerated && !modified) {
      m.form = ChunkForm::kGenerated;
    } else if (chunk) {
      m.form = ChunkForm::kExplicit;
      m.revision = chunk->revision();
      m.voxels.assign(chunk->voxels().begin(), chunk->voxels().end());
    } else {
      // Full mode: generate it first (a later tick sends it).
      if (explicit_wanted_.size() < kMaxExplicitWanted) explicit_wanted_.push_back(c);
      continue;
    }
    auto bytes = Encode(m);
    s.chunk_credit -= static_cast<double>(bytes.size());
    outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kWorld, std::move(bytes)});
    s.streamed.insert(c);
    ++sent;
    ++(m.form == ChunkForm::kGenerated ? s.stream_stats.generated_sent
       : m.form == ChunkForm::kAir     ? s.stream_stats.air_sent
                                       : s.stream_stats.explicit_sent);
  }
  s.stream_complete = complete;
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
      const RVec3 p = players_.Position(*s.handle);
      const Vec3 v = players_.Velocity(*s.handle);
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
      const RVec3 p = players_.Position(*s.handle);
      const Vec3 v = players_.Velocity(*s.handle);
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
    constexpr std::size_t kLocalBytes = 9 + 44 + 47 + 20 + 1;  // position f64×3 (v4)
    constexpr std::size_t kRemoteBytes = 26;
    const std::size_t room = (kMaxDatagramBytes - kLocalBytes) / kRemoteBytes;
    std::vector<RemotePlayerState> remotes;
    for (const auto& [player_id, view] : views) {
      if (player_id != s.player_id) remotes.push_back(view);
    }
    auto distance = [&](const RemotePlayerState& r) {
      double d = 0;
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

bool Server::IsAir(const ChunkCoord& c) const { return air_test_ && air_test_(c); }

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

std::optional<StreamStats> Server::StreamStatsOf(std::uint16_t player_id) const {
  const Session* s = SessionOfPlayer(player_id);
  if (!s) return std::nullopt;
  StreamStats out = s->stream_stats;
  out.streamed = s->streamed.size();
  return out;
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
