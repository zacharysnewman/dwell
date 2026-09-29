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
constexpr std::size_t kMaxQueuedEdits = 8;      // per session per tick; more are dropped
// Block edit rate (§6.5, §11): a token bucket refilled once per BLOCK_EDIT_INTERVAL_MS, holding a
// small burst so edits bunched by the network are not rejected.
constexpr double kEditsPerTick = 1000.0 / (kBlockEditIntervalMs * kSimHz);
constexpr double kEditBurst = 3.0;
constexpr int kMaxSavedLoadsPerTick = 16;  // saved chunks read from the world file per tick
// Level of detail (§6.6).
constexpr std::uint32_t kIndexUpdateEveryTicks =
    static_cast<std::uint32_t>(kLodIndexUpdateMs * kSimHz / 1000);
constexpr std::size_t kMaxQueuedLodRequests = 256;  // per session; more are dropped
constexpr double kLodRequestsPerTick = static_cast<double>(kLodRequestsPerSecond) / kSimHz;

std::uint64_t IndexKey(std::int32_t i, std::int32_t k) {
  return (static_cast<std::uint64_t>(static_cast<std::uint32_t>(i)) << 32) |
         static_cast<std::uint32_t>(k);
}

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
    : config_(WithSavedWorld(std::move(config))),
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
                config_.worldgen_threads),
      lod_(
          LodGeneratorFor(config_.generator_version, config_.world_seed),
          config_.lod_threads >= 0 ? config_.lod_threads : (config_.worldgen_threads > 0 ? 1 : 0)) {
  if (!config_.spawn) config_.spawn = SpawnPointFor(config_.generator_version, config_.world_seed);
  const auto& at = *config_.spawn;
  const ChunkCoord spawn_chunk = ChunkAt(at[0], at[1], at[2]);
  verification_chunk_ = ChunkAt(at[0], at[1] - 1.0, at[2]);

  InitStorage();

  // Pre-generate the spawn region, so the first players never wait for terrain. The verification
  // chunk is always generated: clients check their generator against it, edited or not.
  std::vector<ChunkCoord> region;
  for (const ChunkCoord& o : CubeOffsets(config_.pregen_radius_chunks)) {
    if (const ChunkCoord c = Add(spawn_chunk, o); !IsAir(c) && !IsSaved(c)) region.push_back(c);
  }
  region.push_back(verification_chunk_);
  worldgen_.SetWanted(region);
  std::vector<WorldgenPool::Result> ready;
  worldgen_.Drain(ready);
  for (auto& [coord, chunk] : ready) {
    if (coord == verification_chunk_) verification_hash_ = ChunkHash(*chunk);
    if (!IsSaved(coord)) world_.Put(coord, std::move(chunk));
  }
  if (!verification_hash_) {
    Chunk generated;
    (config_.generator_override ? config_.generator_override
                                : GeneratorFor(config_.generator_version, config_.world_seed))(
        verification_chunk_, generated);
    verification_hash_ = ChunkHash(generated);
  }

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
  if (config_.store && it->second.phase == Phase::kJoined)
    departed_.push_back(RecordOf(it->second));
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
    player::Input input = player::DequantizeInput(f);
    input.fly = input.fly && MayFly(s);  // creative flight is the server's to allow (§8.3)
    s.inputs.insert(at, {f.seq, input});
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
      const auto listed = [&](const std::vector<PublicKey>& keys) {
        return std::find(keys.begin(), keys.end(), s.public_key) != keys.end();
      };
      if (listed(config_.banned)) {
        Reject(id, RejectReason::kBanned, "You are banned from this server.");
        return;
      }
      if (config_.allow_list && !listed(*config_.allow_list)) {
        Reject(id, RejectReason::kNotAllowListed, "This server only admits listed players.");
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
      joined.edit_credit = kEditBurst;
      SendReliable(
          id,
          Welcome{joined.player_id,
                  config_.world_seed,
                  config_.generator_version,
                  tick_,
                  {verification_chunk_.x, verification_chunk_.y, verification_chunk_.z},
                  static_cast<std::uint8_t>(MayFly(joined) ? protocol::WelcomeFlags::kFlight : 0)});
      // A returning player continues where it left the world (§6.4), unless it was dead.
      std::optional<storage::PlayerRecord> saved;
      if (config_.store) saved = config_.store->db().LoadPlayer(joined.public_key);
      if (saved && saved->health > 0) {
        SpawnPlayer(joined, saved->feet);
        joined.health = saved->health;
      } else {
        SpawnPlayer(joined);
      }
      return;
    }
    case Phase::kJoined:
      // Gameplay input comes as datagrams. On control: the one WorldgenCheck, which picks how the
      // client receives chunks (§6.3), block edits (§6.5) and resync requests; anything else is
      // ignored.
      if (const auto* check = std::get_if<WorldgenCheck>(&m);
          check && s.chunk_mode == ChunkMode::kAwaitingCheck) {
        s.chunk_mode = check->hash != 0 && check->hash == verification_hash_ ? ChunkMode::kGenerated
                                                                             : ChunkMode::kFull;
        SendLodIndex(id, s);  // §6.6: the modified sections, right after the check
      } else if (const auto* edit = std::get_if<BlockEditRequest>(&m)) {
        if (s.edits.size() < kMaxQueuedEdits) {
          s.edits.push_back(*edit);
        } else {
          ++s.stats.edits_rejected;
        }
      } else if (const auto* resync = std::get_if<ChunkResync>(&m)) {
        Resync(id, s, *resync);
      } else if (const auto* lod = std::get_if<LodRequest>(&m)) {
        // Rate-limited (LOD_REQUESTS_PER_SECOND, a token bucket); answered within the lod budget.
        for (const LodSectionRequest& r : lod->sections) {
          if (s.chunk_mode == ChunkMode::kAwaitingCheck || s.lod_request_credit < 1.0 ||
              s.lod_requests.size() >= kMaxQueuedLodRequests) {
            ++s.lod_stats.requests_dropped;
            continue;
          }
          s.lod_request_credit -= 1.0;
          ++s.lod_stats.requests;
          s.lod_requests.push_back(r);
        }
      }
      return;
  }
  Reject(id, RejectReason::kMalformed, "Unexpected message.");
}

void Server::SpawnPlayer(Session& s, std::optional<std::array<double, 3>> feet) {
  // Spread players around the spawn point so they don't start inside each other.
  const int slot = (s.player_id - 1) % 8;
  const auto& at = *config_.spawn;
  const RVec3 spawn = feet
                          ? RVec3((*feet)[0], (*feet)[1], (*feet)[2])
                          : RVec3(at[0] + (slot % 4) - 1.5, at[1], at[2] + (slot / 4) * 1.5 - 0.75);
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
  // 0. Chunks generated since the last tick (off-thread, or here within the budget), and saves
  // the I/O thread finished.
  {
    std::vector<WorldgenPool::Result> ready;
    worldgen_.Collect(ready, std::chrono::microseconds(config_.worldgen_budget_us));
    for (auto& [coord, chunk] : ready) {
      if (!world_.Find(coord) && !IsSaved(coord)) world_.Put(coord, std::move(chunk));
    }
  }
  CollectSaves();

  // 1. Block edits queued since the last tick (§6.5); collision rebuilds in the controller pass.
  ApplyEdits();

  // 2. One input per player per tick, in sequence order (§9.3). Starved: repeat the last one.
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

  // 3. Controller pipeline (PLAYER_CONTROLLER.md §4), then its server-side consequences.
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

  // 4. Physics.
  physics_.Step(1.0f / kSimHz);
  ++tick_;

  // 5. Respawns, then snapshots at SNAPSHOT_HZ.
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

  // 6. Terrain streaming, then what to generate next and what to forget.
  explicit_wanted_.clear();
  for (auto& [id, s] : sessions_) StreamChunks(id, s);
  UpdateWorldgen();

  // 6b. Level of detail: propagation off the tick, index updates, answers to requests (§6.6).
  UpdateLod();

  // 7. Autosave (§6.4): prepared here, committed off the tick.
  if (config_.store && tick_ >= next_save_tick_) SaveNow();
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
  // Saved chunks are read from the world file instead (a few per tick).
  std::vector<ChunkCoord> wanted;
  int loads = 0;
  for (const ChunkCoord& o : PrefetchOffsets()) {
    for (const ChunkCoord& center : centers) {
      const ChunkCoord c = Add(center, o);
      if (c.y < kMinChunkY || c.y > kMaxChunkY || world_.Find(c)) continue;
      if (IsSaved(c)) {
        if (loads++ < kMaxSavedLoadsPerTick) world_.GetOrCreate(c);
      } else if (!IsAir(c)) {
        wanted.push_back(c);
      }
    }
  }
  wanted.insert(wanted.end(), explicit_wanted_.begin(), explicit_wanted_.end());
  worldgen_.SetWanted(wanted);

  if (tick_ % kEvictEveryTicks == 0) {
    // Unmodified chunks can be generated again, and saved ones read again; modified chunks not
    // yet saved stay.
    world_.Evict([&](const ChunkCoord& c, const Chunk& chunk) {
      if (c == verification_chunk_ || dirty_.count(c)) return false;
      const auto saved = saved_.find(c);
      const bool clean =
          chunk.revision() == 0 || (saved != saved_.end() && saved->second == chunk.revision());
      return clean && std::none_of(centers.begin(), centers.end(), [&](const ChunkCoord& center) {
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
    auto message = ChunkMessage(s, c, /*generate=*/false);
    if (!message) {
      // Full mode: generate it first (a later tick sends it).
      if (explicit_wanted_.size() < kMaxExplicitWanted) explicit_wanted_.push_back(c);
      continue;
    }
    const ChunkData& m = *message;
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

std::optional<ChunkData> Server::ChunkMessage(const Session& s, const ChunkCoord& c,
                                              bool generate) {
  const Chunk* chunk = world_.Find(c);
  ChunkData m;
  m.coord = {c.x, c.y, c.z};
  if (!chunk && IsSaved(c)) chunk = &world_.GetOrCreate(c);  // read from the world file
  const bool modified = chunk && chunk->revision() > 0;
  if (!modified && IsAir(c)) {
    m.form = ChunkForm::kAir;  // nothing to generate or store, in either mode
  } else if (s.chunk_mode == ChunkMode::kGenerated && !modified) {
    m.form = ChunkForm::kGenerated;
  } else {
    if (!chunk) {
      if (!generate) return std::nullopt;
      chunk = &world_.GetOrCreate(c);
    }
    m.form = ChunkForm::kExplicit;
    m.revision = chunk->revision();
    m.voxels.assign(chunk->voxels().begin(), chunk->voxels().end());
  }
  return m;
}

void Server::Resync(SessionId id, Session& s, const ChunkResync& m) {
  // Only chunks the client has: the rest arrive by streaming anyway.
  for (const ChunkCoordNet& at : m.coords) {
    const ChunkCoord c{at[0], at[1], at[2]};
    if (!s.streamed.count(c)) continue;
    if (auto message = ChunkMessage(s, c, /*generate=*/true)) {
      SendReliable(id, *message, Channel::kWorld);
      ++s.stats.resyncs;
    }
  }
}

bool Server::MayEdit(const Session& s) const { return Allowed(config_.edits, s); }
bool Server::MayFly(const Session& s) const { return Allowed(config_.flight, s); }

bool Server::Allowed(EditPolicy policy, const Session& s) const {
  switch (policy) {
    case EditPolicy::kEveryone:
      return true;
    case EditPolicy::kOps:
      return std::find(config_.ops.begin(), config_.ops.end(), s.public_key) != config_.ops.end();
    case EditPolicy::kNobody:
      return false;
  }
  return false;
}

std::array<double, 3> Server::EyeOf(const Session& s) const {
  const PlayerHandle h = *s.handle;
  const RVec3 p = players_.Position(h);
  const auto& body = player_config_.body;
  const float eye =
      players_.controller(h).crouch.crouching ? body.crouch_eye_height : body.eye_height;
  return {p.GetX(), p.GetY() - players_.HalfHeight(h) + eye, p.GetZ()};
}

void Server::ApplyEdits() {
  std::vector<EditCapsule> capsules;
  bool capsules_ready = false;
  // Changes of this tick per chunk, in the order chunks were first touched.
  std::vector<ChunkChanges> changed;
  std::unordered_map<ChunkCoord, std::size_t, ChunkCoordHash> index;

  for (auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    s.edit_credit = std::min(s.edit_credit + kEditsPerTick, kEditBurst);
    for (const BlockEditRequest& edit : s.edits) {
      if (!s.handle || !MayEdit(s) || s.edit_credit < 1.0) {
        ++s.stats.edits_rejected;
        continue;
      }
      if (!capsules_ready) {
        for (const auto& [other_id, other] : sessions_) {
          if (!other.handle) continue;
          const RVec3 c = players_.Position(*other.handle);
          const float half = players_.HalfHeight(*other.handle);
          const float radius = player_config_.body.radius;
          capsules.push_back({{c.GetX(), c.GetY(), c.GetZ()}, radius, half - radius});
        }
        capsules_ready = true;
      }
      const EditOutcome outcome = CheckBlockEdit(world_, edit, EyeOf(s), capsules);
      s.stats.last_edit_check = outcome.check;
      if (outcome.check != EditCheck::kOk) {
        ++s.stats.edits_rejected;
        continue;
      }
      s.edit_credit -= 1.0;
      ++s.stats.edits_applied;
      const auto& cell = outcome.cell;
      const ChunkCoord coord = ChunkOf(cell[0], cell[1], cell[2]);
      const int local = LocalIndex(cell[0] - coord.x * kChunkSize, cell[1] - coord.y * kChunkSize,
                                   cell[2] - coord.z * kChunkSize);
      world_.GetOrCreate(coord).SetAt(local, outcome.material);
      dirty_.insert(coord);
      lod_.MarkChunk(coord);
      auto [at, inserted] = index.try_emplace(coord, changed.size());
      if (inserted) changed.push_back({{coord.x, coord.y, coord.z}, 0, {}});
      auto& changes = changed[at->second].changes;
      const auto same = std::find_if(changes.begin(), changes.end(),
                                     [&](const VoxelChange& v) { return v.index == local; });
      if (same != changes.end()) {
        same->material = outcome.material;
      } else {
        changes.push_back({static_cast<std::uint16_t>(local), outcome.material});
      }
    }
    s.edits.clear();
  }
  if (changed.empty()) return;

  // One revision per chunk per modification, then each client gets the chunks it streams.
  for (ChunkChanges& c : changed) {
    Chunk& chunk = world_.GetOrCreate({c.coord[0], c.coord[1], c.coord[2]});
    chunk.BumpRevision();
    c.revision = chunk.revision();
  }
  for (const auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    VoxelModification m;
    m.reason = VoxelModificationReason::kEdit;
    m.server_tick = tick_;
    for (const ChunkChanges& c : changed) {
      if (s.streamed.count({c.coord[0], c.coord[1], c.coord[2]})) m.chunks.push_back(c);
    }
    if (!m.chunks.empty()) SendReliable(id, m, Channel::kWorld);
  }
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

std::vector<std::array<double, 3>> Server::PlayerPositions() const {
  std::vector<std::array<double, 3>> out;
  for (const auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    if (s.handle) {
      const RVec3 p = players_.Position(*s.handle);
      out.push_back({p.GetX(), p.GetY(), p.GetZ()});
    } else {
      out.push_back({s.death_position[0], s.death_position[1], s.death_position[2]});
    }
  }
  return out;
}

LodPropagation::ModifiedChunk Server::ModifiedChunkLookup() {
  return [this](const ChunkCoord& c) -> const Chunk* {
    const Chunk* chunk = world_.Find(c);
    if (!chunk && IsSaved(c)) chunk = &world_.GetOrCreate(c);  // read from the world file
    return chunk && chunk->revision() > 0 ? chunk : nullptr;
  };
}

void Server::DrainLod() {
  lod_.Drain(PlayerPositions(), ModifiedChunkLookup());
  UpdateLod();
}

void Server::UpdateLod() {
  lod_.Step(PlayerPositions(), ModifiedChunkLookup(), kLodPropagationSectionsPerTick,
            std::chrono::microseconds(config_.lod_budget_us));
  for (const LodCoord& c : lod_.TakeWritten()) {
    lod_unsaved_.insert(c);
    if (c.level == kLodIndexLevel) {
      index_updates_[IndexKey(c.i, c.k)] = {c.i, c.k, lod_.Find(c)->revision};
    }
  }
  // Index changes, coalesced to at most one LodIndexUpdate per LOD_INDEX_UPDATE_MS.
  if (!index_updates_.empty() && tick_ >= next_index_update_tick_) {
    next_index_update_tick_ = tick_ + kIndexUpdateEveryTicks;
    std::vector<LodIndexEntry> entries;
    for (const auto& [key, e] : index_updates_) entries.push_back(e);
    index_updates_.clear();
    for (std::size_t from = 0; from < entries.size(); from += kMaxLodIndexEntries) {
      LodIndexUpdate m;
      m.entries.assign(entries.begin() + static_cast<std::ptrdiff_t>(from),
                       entries.begin() + static_cast<std::ptrdiff_t>(
                                             std::min(entries.size(), from + kMaxLodIndexEntries)));
      const auto bytes = Encode(m);
      for (auto& [id, s] : sessions_) {
        if (!s.lod_index_sent) continue;
        ++s.lod_stats.index_updates;
        s.lod_stats.index_entries += static_cast<std::uint32_t>(m.entries.size());
        s.lod_stats.bytes += bytes.size();
        outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kLod, bytes});
      }
    }
  }
  for (auto& [id, s] : sessions_) {
    if (s.phase != Phase::kJoined) continue;
    s.lod_request_credit = std::min(s.lod_request_credit + kLodRequestsPerTick,
                                    static_cast<double>(kLodRequestsPerSecond));
    ServeLod(id, s);
  }
}

void Server::SendLodIndex(SessionId id, Session& s) {
  std::vector<LodIndexEntry> entries;
  for (const auto& [c, section] : lod_.sections()) {
    if (c.level == kLodIndexLevel) entries.push_back({c.i, c.k, section.revision});
  }
  std::size_t from = 0;
  do {
    LodIndex m;
    const std::size_t to = std::min(entries.size(), from + kMaxLodIndexEntries);
    m.entries.assign(entries.begin() + static_cast<std::ptrdiff_t>(from),
                     entries.begin() + static_cast<std::ptrdiff_t>(to));
    m.last = to == entries.size();
    const auto bytes = Encode(m);
    s.lod_stats.index_entries += static_cast<std::uint32_t>(m.entries.size());
    s.lod_stats.bytes += bytes.size();
    outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kLod, bytes});
    from = to;
  } while (from < entries.size());
  s.lod_index_sent = true;
}

void Server::ServeLod(SessionId id, Session& s) {
  const double per_tick = static_cast<double>(config_.lod_bytes_per_second) / kSimHz;
  s.lod_credit = std::min(s.lod_credit + per_tick, per_tick);
  while (!s.lod_requests.empty() && s.lod_credit > 0) {
    const LodSectionRequest& r = s.lod_requests.front();
    const LodCoord c{r.level, r.section[0], r.section[1], r.section[2]};
    LodData m;
    m.level = r.level;
    m.section = r.section;
    m.form = LodForm::kGenerated;
    if (!LodInWorld(c)) {
      // Nothing there: the client's generator says so too.
    } else if (const LodSection* section = lod_.Find(c)) {
      m.revision = section->revision;
      if (r.known_revision == section->revision) {
        m.form = LodForm::kUnchanged;
      } else {
        m.form = LodForm::kExplicit;
        m.cells = lod_.CellsForClient(c);
      }
    } else if (s.chunk_mode == ChunkMode::kFull) {
      // Unmodified (or not written yet), and the client cannot generate: send it generated.
      const auto* encoded = lod_.Generated(c);
      if (!encoded) break;  // generating; answered on a later tick, in order
      m.form = LodForm::kExplicit;
      m.cells = *DecodeLodCells(*encoded);
    }
    const auto bytes = Encode(m);
    s.lod_credit -= static_cast<double>(bytes.size());
    s.lod_stats.bytes += bytes.size();
    ++(m.form == LodForm::kGenerated  ? s.lod_stats.generated_sent
       : m.form == LodForm::kExplicit ? s.lod_stats.explicit_sent
                                      : s.lod_stats.unchanged_sent);
    outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kLod, std::move(bytes)});
    s.lod_requests.pop_front();
  }
}

bool Server::IsAir(const ChunkCoord& c) const { return air_test_ && !IsSaved(c) && air_test_(c); }

ServerConfig Server::WithSavedWorld(ServerConfig config) {
  if (!config.store) return config;
  if (const auto meta = config.store->db().LoadMeta()) {
    // A saved world keeps its own seed, generator and spawn (launch options apply to new worlds).
    config.world_seed = meta->world_seed;
    config.generator_version = meta->generator_version;
    if (meta->spawn) config.spawn = meta->spawn;
  }
  return config;
}

void Server::InitStorage() {
  if (!config_.store) return;
  storage::WorldDb& db = config_.store->db();
  const bool fresh = !db.LoadMeta();
  if (const auto meta = db.LoadMeta()) tick_ = meta->world_tick;
  for (const auto& [coord, revision] : db.ChunkIndex()) saved_[coord] = revision;
  // The LOD cache (§6.6): restored as saved, rebuilt from the chunks when it belongs to another
  // generator version, and derived for saved chunks it does not cover (older worlds).
  bool stale = false;
  for (auto& row : db.LodSections(config_.generator_version, stale)) {
    if (!row.encoded.empty()) lod_.Restore(row.coord, {row.revision, std::move(row.encoded)});
    if (row.dirty || !lod_.Find(row.coord)) lod_.MarkDirty(row.coord);
  }
  lod_rebuild_ = stale;
  for (const auto& [coord, revision] : saved_) {
    if (!lod_.Modified(LodParent(LodOfChunk(coord)))) lod_.MarkChunk(coord);
  }
  world_.SetSaved({[this](const ChunkCoord& c) { return IsSaved(c); },
                   [this](const ChunkCoord& c) -> std::unique_ptr<Chunk> {
                     const auto saved = config_.store->db().LoadChunk(c);
                     if (!saved) return nullptr;
                     auto chunk = std::make_unique<Chunk>();
                     std::copy(saved->voxels.begin(), saved->voxels.end(),
                               chunk->generation_voxels().begin());
                     chunk->SetRevision(saved->revision);
                     return chunk;
                   }});
  next_save_tick_ = tick_ + static_cast<std::uint32_t>(config_.autosave_seconds * kSimHz);
  if (fresh) SaveNow();  // record the new world's seed, generator and spawn right away
}

storage::PlayerRecord Server::RecordOf(const Session& s) const {
  storage::PlayerRecord p;
  p.key = s.public_key;
  p.display_name = s.display_name;
  p.health = s.handle ? s.health : 0;
  if (s.handle) {
    const RVec3 at = players_.Position(*s.handle);
    p.feet = {at.GetX(), at.GetY() - players_.HalfHeight(*s.handle), at.GetZ()};
  } else {
    p.feet = {s.death_position[0], s.death_position[1], s.death_position[2]};
  }
  return p;
}

void Server::SaveNow() {
  if (!config_.store) return;
  next_save_tick_ = tick_ + static_cast<std::uint32_t>(config_.autosave_seconds * kSimHz);
  storage::SaveBatch batch;
  batch.meta =
      storage::WorldMeta{config_.world_seed, config_.generator_version, config_.spawn, tick_};
  std::vector<std::pair<ChunkCoord, std::uint32_t>> revisions;
  for (const ChunkCoord& c : dirty_) {
    const Chunk* chunk = world_.Find(c);
    if (!chunk) continue;
    batch.chunks.push_back(
        {c, chunk->revision(), {chunk->voxels().begin(), chunk->voxels().end()}});
    revisions.emplace_back(c, chunk->revision());
  }
  dirty_.clear();
  batch.players = std::exchange(departed_, {});
  // LOD sections written since the last save, and those still to compute (saved dirty).
  std::vector<LodCoord> lod_saved;
  for (const LodCoord& c : lod_unsaved_) {
    const LodSection* section = lod_.Find(c);
    if (!section) continue;
    batch.lod_sections.push_back(
        {c, section->revision, lod_.dirty().count(c) != 0, section->encoded});
    lod_saved.push_back(c);
  }
  // Sections still to compute (dirty, or computing now) are saved dirty, to recompute on load.
  for (const auto* pending : {&lod_.dirty(), &lod_.in_flight()}) {
    for (const LodCoord& c : *pending) {
      const LodSection* section = lod_.Find(c);
      if (lod_unsaved_.count(c) && section) {
        continue;  // saved above, flagged dirty
      }
      batch.lod_sections.push_back({c, section ? section->revision : 0, true,
                                    section ? section->encoded : std::vector<std::uint8_t>{}});
    }
  }
  lod_unsaved_.clear();
  batch.clear_lod = std::exchange(lod_rebuild_, false);
  for (const auto& [id, s] : sessions_) {
    if (s.phase == Phase::kJoined) batch.players.push_back(RecordOf(s));
  }
  const std::uint64_t save_id = config_.store->Save(std::move(batch));
  in_flight_[save_id] = std::move(revisions);
  lod_in_flight_[save_id] = std::move(lod_saved);
  CollectSaves();  // saves commit inline without an I/O thread
}

void Server::CollectSaves() {
  if (!config_.store) return;
  for (const auto& result : config_.store->TakeCompleted()) {
    const auto it = in_flight_.find(result.id);
    if (it == in_flight_.end()) continue;
    if (const auto lod = lod_in_flight_.find(result.id); lod != lod_in_flight_.end()) {
      if (!result.ok) lod_unsaved_.insert(lod->second.begin(), lod->second.end());
      lod_in_flight_.erase(lod);
    }
    if (result.ok) {
      ++save_stats_.saves;
      for (const auto& [coord, revision] : it->second) {
        auto& saved = saved_[coord];
        saved = std::max(saved, revision);
      }
    } else {
      ++save_stats_.failed;
      save_stats_.last_error = result.error;
      for (const auto& [coord, revision] : it->second) dirty_.insert(coord);  // try again
    }
    in_flight_.erase(it);
  }
}

SaveStats Server::save_stats() const {
  SaveStats out = save_stats_;
  out.dirty = dirty_.size();
  out.saved_chunks = saved_.size();
  out.loaded = world_.loaded_saved();
  return out;
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

std::optional<LodStats> Server::LodStatsOf(std::uint16_t player_id) const {
  const Session* s = SessionOfPlayer(player_id);
  return s ? std::optional<LodStats>(s->lod_stats) : std::nullopt;
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
