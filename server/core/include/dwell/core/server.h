#pragma once

#include <array>
#include <cstdint>
#include <deque>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "dwell/core/block_edit.h"
#include "dwell/core/entropy.h"
#include "dwell/core/lod_propagation.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/terrain_collision.h"
#include "dwell/core/voxel.h"
#include "dwell/core/worldgen_pool.h"
#include "dwell/player/controller.h"
#include "dwell/protocol/messages.h"
#include "dwell/storage/world_store.h"

// The authoritative server core (ARCHITECTURE.md §4). It has no sockets, threads, or OS calls: the
// host feeds it transport events, calls Step() at SIM_HZ, and drains the outbox (§4.3). The same
// class runs in the native server, in the browser's local mode, and in friend-world hosts.
namespace dwell::core {

using SessionId = std::uint32_t;
using TransportBinding = std::array<std::uint8_t, 32>;

// Who may break and place blocks (§6.5, §11).
enum class EditPolicy : std::uint8_t { kEveryone, kOps, kNobody };

struct ServerConfig {
  std::string name = "Dwell Server";
  std::string motd = "";
  std::uint16_t max_players = 16;
  std::uint64_t world_seed = 0;
  std::uint32_t generator_version = kGeneratorTerrain;  // §6.3; 1 = movement playground
  std::string client_version_note = "";
  // Feet position; players spread out around it. Unset: the generator's spawn point.
  std::optional<std::array<double, 3>> spawn = std::nullopt;

  // Terrain generation and streaming (§6.3).
  int worldgen_threads = 0;       // 0: generate on the tick thread within worldgen_budget_us
  int worldgen_budget_us = 4000;  // per tick, without threads
  int pregen_radius_chunks = 2;   // around the spawn, generated at startup
  int view_radius_chunks = protocol::kViewRadiusChunks;         // sphere streamed to each client
  int chunk_bytes_per_second = protocol::kChunkBytesPerSecond;  // per client

  // Level of detail (§6.6): propagation threads (−1: one when worldgen has threads, else none —
  // then it runs on the tick within lod_budget_us), and the per-client `lod` stream budget.
  int lod_threads = -1;
  int lod_budget_us = 2000;
  int lod_bytes_per_second = protocol::kLodBytesPerSecond;

  // Block edits (§6.5): who may edit, and the players (device public keys) who are ops.
  EditPolicy edits = EditPolicy::kEveryone;
  std::vector<protocol::PublicKey> ops = {};
  // Access (§10.1): banned keys are refused; with an allow-list, only its keys may join.
  std::vector<protocol::PublicKey> banned = {};
  std::optional<std::vector<protocol::PublicKey>> allow_list = std::nullopt;

  // World persistence (§6.4). With a store, a saved world's seed, generator and spawn replace the
  // ones above, modified chunks and players load from it, and dirty data saves every
  // autosave_seconds. Without one the world lives in memory only.
  std::shared_ptr<storage::WorldStore> store = nullptr;
  int autosave_seconds = protocol::kAutosaveSeconds;

  // Tests and tools: replaces GeneratorFor(generator_version, world_seed), e.g. to move a test
  // world far from the origin (clients must generate the same chunks). Without an air test to
  // match, every chunk is generated and streamed.
  ChunkGenerator generator_override = nullptr;
  AirChunkTest air_test_override = nullptr;
};

// Terrain streaming counters, for tests and diagnostics.
struct StreamStats {
  std::uint32_t generated_sent = 0;
  std::uint32_t explicit_sent = 0;
  std::uint32_t air_sent = 0;
  std::uint32_t unloaded = 0;
  std::size_t streamed = 0;  // chunks the client currently has
};

struct Outgoing {
  enum class Kind : std::uint8_t { kReliable, kDatagram, kClose };
  SessionId session = 0;
  Kind kind = Kind::kReliable;
  protocol::Channel channel = protocol::Channel::kControl;  // reliable only
  std::vector<std::uint8_t> bytes;                          // empty for kClose
};

// Level-of-detail traffic per client (§6.6), for tests and diagnostics.
struct LodStats {
  std::uint32_t requests = 0;          // sections requested and accepted
  std::uint32_t requests_dropped = 0;  // over LOD_REQUESTS_PER_SECOND (or the queue), or too early
  std::uint32_t generated_sent = 0;
  std::uint32_t explicit_sent = 0;
  std::uint32_t unchanged_sent = 0;
  std::uint32_t index_entries = 0;  // in LodIndex and LodIndexUpdate messages
  std::uint32_t index_updates = 0;  // LodIndexUpdate messages
  std::uint64_t bytes = 0;          // on the lod stream
};

// Per-session counters, for tests and diagnostics.
struct SessionStats {
  std::uint32_t inputs_received = 0;
  std::uint32_t inputs_rejected = 0;    // out of range (§11)
  std::uint32_t datagrams_dropped = 0;  // over the rate limit
  std::uint32_t inputs_skipped = 0;     // dropped to bound input latency
  std::uint32_t ticks_starved = 0;      // no input queued: the last one repeated
  std::uint32_t edits_applied = 0;      // block edits (§6.5)
  std::uint32_t edits_rejected = 0;     // failed validation, cooldown, or permission
  EditCheck last_edit_check = EditCheck::kOk;
  std::uint32_t resyncs = 0;  // chunks re-sent on the client's request (§6.3)
};

// World persistence counters, for tests and diagnostics.
struct SaveStats {
  std::uint32_t saves = 0;       // batches committed
  std::uint32_t failed = 0;      // batches that failed (their chunks stay dirty)
  std::size_t dirty = 0;         // modified chunks not yet committed
  std::size_t saved_chunks = 0;  // chunks in the world file
  std::uint64_t loaded = 0;      // chunks read back from the file
  std::string last_error;
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

  // Saves dirty chunks, players and meta now (committed off the tick natively). No-op without a
  // store. Hosts call it before shutting down, then WorldStore::Flush.
  void SaveNow();
  SaveStats save_stats() const;

  std::uint32_t tick() const { return tick_; }
  double time_ms() const { return tick_ * (1000.0 / protocol::kSimHz); }
  std::size_t joined_players() const;
  VoxelWorld& world() { return world_; }
  const TerrainCollision& terrain() const { return terrain_; }
  PhysicsWorld& physics() { return physics_; }
  player::Players& players() { return players_; }
  const player::PlayerControllerConfig& player_config() const { return player_config_; }

  // Test and diagnostics access by player id.
  std::optional<player::PlayerHandle> PlayerHandleOf(std::uint16_t player_id) const;
  std::optional<SessionStats> StatsOf(std::uint16_t player_id) const;
  std::optional<StreamStats> StreamStatsOf(std::uint16_t player_id) const;
  std::optional<LodStats> LodStatsOf(std::uint16_t player_id) const;
  const LodPropagation& lod() const { return lod_; }
  // Runs LOD propagation until nothing is dirty (tests, tools).
  void DrainLod();
  const ChunkCoord& verification_chunk() const { return verification_chunk_; }
  int HealthOf(std::uint16_t player_id) const;  // −1 when unknown

 private:
  enum class Phase : std::uint8_t { kAwaitingHello, kAwaitingAuth, kJoined };
  // How a client receives chunks (§6.3): nothing until its WorldgenCheck arrives, then Generated
  // messages for unmodified chunks when its generator matched, otherwise every chunk explicitly.
  enum class ChunkMode : std::uint8_t { kAwaitingCheck, kGenerated, kFull };
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
    double death_position[3] = {0, 0, 0};
    std::uint32_t launch_ready_tick = 0;
    std::uint32_t last_knockback_seq = 0;
    std::uint32_t rate_window_tick = 0;
    std::uint32_t rate_window_count = 0;
    std::vector<protocol::BlockEditRequest> edits;  // applied at the start of the next Step
    double edit_credit = 0;                         // rate limit (token bucket)
    SessionStats stats;
    // Terrain streaming.
    ChunkMode chunk_mode = ChunkMode::kAwaitingCheck;
    std::unordered_set<ChunkCoord, ChunkCoordHash> streamed;  // sent and not unloaded
    std::optional<ChunkCoord> stream_center;
    bool stream_complete = false;  // everything in view sent (until the center moves)
    double chunk_credit = 0;       // bytes the client may still receive this tick
    StreamStats stream_stats;
    // Level of detail (§6.6).
    bool lod_index_sent = false;
    std::deque<protocol::LodSectionRequest> lod_requests;
    double lod_credit = 0;                                        // bytes, like chunk_credit
    double lod_request_credit = protocol::kLodRequestsPerSecond;  // token bucket
    LodStats lod_stats;
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

  // At the spawn point, or at `feet` (a saved position).
  void SpawnPlayer(Session& s, std::optional<std::array<double, 3>> feet = std::nullopt);
  void Kill(Session& s, protocol::DamageCause cause);
  void Damage(Session& s, int amount, protocol::DamageCause cause);
  void AfterControllerTick(Session& s);
  void SendSnapshots();
  // Block edits (§6.5): validation, then one VoxelModification per tick to the clients streaming
  // the changed chunks.
  void ApplyEdits();
  bool MayEdit(const Session& s) const;
  std::array<double, 3> EyeOf(const Session& s) const;
  // Terrain: generation around players, eviction, and per-client streaming (§6.3).
  std::optional<ChunkCoord> ViewCenter(const Session& s) const;
  void UpdateWorldgen();
  bool IsAir(const ChunkCoord& c) const;
  void StreamChunks(SessionId id, Session& s);
  // The message streaming chunk `c` to a client: Air, Generated or Explicit. Empty while a
  // full-mode client's chunk is not generated yet (unless `generate`).
  std::optional<protocol::ChunkData> ChunkMessage(const Session& s, const ChunkCoord& c,
                                                  bool generate);
  void Resync(SessionId id, Session& s, const protocol::ChunkResync& m);
  // Level of detail (§6.6): propagation, the index and its updates, and answering requests.
  std::vector<std::array<double, 3>> PlayerPositions() const;
  LodPropagation::ModifiedChunk ModifiedChunkLookup();
  void UpdateLod();
  void SendLodIndex(SessionId id, Session& s);
  void ServeLod(SessionId id, Session& s);
  Session* SessionOfPlayer(std::uint16_t player_id);
  const Session* SessionOfPlayer(std::uint16_t player_id) const;
  std::uint16_t PlayerIdOfBody(std::uint32_t body_id) const;
  // Persistence (§6.4).
  static ServerConfig WithSavedWorld(ServerConfig config);
  void InitStorage();
  void CollectSaves();
  storage::PlayerRecord RecordOf(const Session& s) const;
  bool IsSaved(const ChunkCoord& c) const { return saved_.count(c) != 0; }

  ServerConfig config_;
  Entropy& entropy_;
  PhysicsWorld physics_;
  AirChunkTest air_test_;  // main thread only (caches)
  VoxelWorld world_;
  TerrainCollision terrain_;
  player::PlayerControllerConfig player_config_;
  player::Players players_;
  std::unordered_map<SessionId, Session> sessions_;
  std::vector<Outgoing> outbox_;
  std::vector<std::pair<std::uint16_t, JPH::Vec3>> knockbacks_;  // applied in the next Step
  std::uint32_t tick_ = 0;
  std::uint16_t next_player_id_ = 1;
  WorldgenPool worldgen_;
  ChunkCoord verification_chunk_;
  std::optional<std::uint64_t> verification_hash_;
  std::vector<ChunkCoord> view_offsets_;     // chunks in view relative to the center, nearest first
  std::vector<ChunkCoord> explicit_wanted_;  // full-mode chunks to generate for streaming
  // Persistence: chunks in the world file (revision saved), modified chunks not saved yet, saves in
  // flight (their chunk revisions), and players who left since the last save.
  std::unordered_map<ChunkCoord, std::uint32_t, ChunkCoordHash> saved_;
  std::unordered_set<ChunkCoord, ChunkCoordHash> dirty_;
  std::unordered_map<std::uint64_t, std::vector<std::pair<ChunkCoord, std::uint32_t>>> in_flight_;
  std::vector<storage::PlayerRecord> departed_;
  std::uint32_t next_save_tick_ = 0;
  SaveStats save_stats_;
  // Level of detail: sections, index entries changed since the last LodIndexUpdate, sections to
  // save (and those in saves in flight), and whether the saved cache must be dropped (rebuild).
  LodPropagation lod_;
  std::unordered_map<std::uint64_t, protocol::LodIndexEntry> index_updates_;
  std::uint32_t next_index_update_tick_ = 0;
  std::unordered_set<LodCoord, LodCoordHash> lod_unsaved_;
  std::unordered_map<std::uint64_t, std::vector<LodCoord>> lod_in_flight_;
  bool lod_rebuild_ = false;
};

}  // namespace dwell::core
