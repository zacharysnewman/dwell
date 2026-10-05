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
using ChunkCoordNet = std::array<std::int32_t, 3>;

struct Welcome {
  std::uint16_t player_id = 0;
  std::uint64_t world_seed = 0;
  std::uint32_t generator_version = 0;
  std::uint32_t server_tick = 0;
  // Chunk the client generates and hashes for WorldgenCheck (§6.3).
  ChunkCoordNet verification_chunk{};
  std::uint8_t flags = 0;  // WelcomeFlags
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

// C→S reliable (`control`), once after Welcome: FNV-1a 64 hash of the client-generated
// verification chunk (ChunkHash in dwell/core/chunk_codec.h). 0 asks for full-chunk mode.
struct WorldgenCheck {
  std::uint64_t hash = 0;
};

// --- Terrain streaming (Phase 3b, §6.3, §8.3) ---

inline constexpr int kChunkVolume = kChunkSize * kChunkSize * kChunkSize;
// Cells of a LOD section with its one-cell apron (§6.6): 34³.
inline constexpr int kLodCellCount =
    (kLodSectionCells + 2) * (kLodSectionCells + 2) * (kLodSectionCells + 2);

// S→C reliable (`world`). Generated: the client generates the chunk itself (no payload). Explicit:
// the voxels travel as palette + RLE. Air: an unmodified chunk the generator leaves all air (no
// payload; neither side generates or stores it). `voxels` holds kChunkVolume materials in chunk
// index order (x | y << 5 | z << 10) for Explicit and is empty otherwise.
struct ChunkData {
  ChunkForm form = ChunkForm::kGenerated;
  ChunkCoordNet coord{};
  std::uint32_t revision = 0;
  std::vector<std::uint16_t> voxels;
};

// S→C reliable (`control`, Phase 5c, §10.2): a friend-world host's page was hidden (the world
// paused) or shown again.
struct HostStatus {
  HostState state = HostState::kPaused;
};

// S→C reliable (`world`): chunks that left the client's view; it drops them.
struct ChunkUnload {
  std::vector<ChunkCoordNet> coords;  // 1..65535
};

// --- Block edits (Phase 3d, §6.5, §8.3) ---

// C→S reliable (`control`): break the targeted cell, or place `material` against its `face`
// (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z). `material` travels only for Place.
struct BlockEditRequest {
  BlockEditAction action = BlockEditAction::kBreak;
  std::array<std::int32_t, 3> cell{};
  std::uint8_t face = 0;
  std::uint16_t material = 0;
};

struct VoxelChange {
  std::uint16_t index = 0;  // chunk-local x | y << 5 | z << 10
  std::uint16_t material = 0;
  bool operator==(const VoxelChange&) const = default;
};
struct ChunkChanges {
  ChunkCoordNet coord{};
  std::uint32_t revision = 0;        // the chunk's revision after the changes
  std::vector<VoxelChange> changes;  // 1..65535
};

// S→C reliable (`world`): voxel changes, applied by clients in order. A chunk's revision advances
// by one per modification that touches it, so a client can detect a gap and resync (§6.3).
struct VoxelModification {
  VoxelModificationReason reason = VoxelModificationReason::kEdit;
  std::uint32_t server_tick = 0;
  std::vector<ChunkChanges> chunks;  // 1..65535
};

// C→S reliable (`control`): chunks the client wants sent again (a revision
// gap), 1..kMaxResyncChunks.
struct ChunkResync {
  std::vector<ChunkCoordNet> coords;
};

// C→S reliable (`control`): chunks beyond the view the client wants to draw at full detail
// (§6.6), within RENDER_RADIUS_CHUNKS; 1..kMaxResyncChunks. The server streams them like the
// view's chunks (and sends their edits) until they leave that radius.
struct ChunkRequest {
  std::vector<ChunkCoordNet> coords;
};

// --- Level of detail (Phase 4b, §6.6, §8.3) ---

// A section at LOD_INDEX_LEVEL (one row, so no j) holding modified chunks, and its lodRevision.
struct LodIndexEntry {
  std::int32_t i = 0, k = 0;
  std::uint32_t revision = 0;
  bool operator==(const LodIndexEntry&) const = default;
};

// S→C reliable (`lod`), after WorldgenCheck: the modified sections at LOD_INDEX_LEVEL, over one or
// more messages (≤ kMaxLodIndexEntries each), the last one flagged.
struct LodIndex {
  bool last = true;
  std::vector<LodIndexEntry> entries;
};

// S→C reliable (`lod`): index entries written since the last update (coalesced, at most one per
// LOD_INDEX_UPDATE_MS). 1..kMaxLodIndexEntries entries.
struct LodIndexUpdate {
  std::vector<LodIndexEntry> entries;
};

struct LodSectionRequest {
  std::uint8_t level = 1;  // 1..LOD_MAX_LEVEL
  std::array<std::int32_t, 3> section{};
  std::uint32_t known_revision = 0;  // the revision the client holds (0 = none)
  bool operator==(const LodSectionRequest&) const = default;
};

// C→S reliable (`control`): sections whose content the client needs (1..LOD_MAX_REQUEST_SECTIONS).
struct LodRequest {
  std::vector<LodSectionRequest> sections;
};

// S→C reliable (`lod`): the answer to one requested section. Generated: nothing below it is
// modified (the client generates it and everything under it). Explicit: its content, 34³ cells
// with the apron (`cells`, core::LodCell order). Unchanged: the client's revision is current.
struct LodData {
  LodForm form = LodForm::kGenerated;
  std::uint8_t level = 1;
  std::array<std::int32_t, 3> section{};
  std::uint32_t revision = 0;
  std::vector<std::uint16_t> cells;
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

// World positions (protocol v4, ADR 0011): `pos64` (f64×3) where prediction must match the server
// exactly, `posfix` (i32×3 in 1/kPositionFixedScale m) elsewhere. Both decode to doubles.
struct LocalPlayerState {
  double position[3] = {0, 0, 0};  // capsule centre (pos64)
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
  double position[3] = {0, 0, 0};  // feet (posfix)
  float velocity[3] = {0, 0, 0};   // f16 on the wire
  std::int16_t yaw = 0, pitch = 0;
  PlayerState state = PlayerState::kIdle;
  std::uint8_t flags = 0;  // PlayerFlags
};

// S→C datagram, SNAPSHOT_HZ. Tier 1 entities join in Phase 10.
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
  std::uint32_t input_seq = 0;     // that player's input processed on server_tick (for replay)
  float vector[3] = {0, 0, 0};     // Knockback: velocity change
  double position[3] = {0, 0, 0};  // Respawn: feet position (pos64)
  std::uint8_t amount = 0;         // Damage
  DamageCause cause = DamageCause::kFall;  // Damage, Death
};

// Chunk voxels as palette + RLE (the ChunkData Explicit payload, §6.1): kChunkVolume materials in
// chunk index order. Also the world file's chunk encoding (before zstd, §6.4).
std::vector<std::uint8_t> EncodeChunkVoxels(const std::vector<std::uint16_t>& voxels);
// nullopt unless `bytes` is exactly one canonical-length payload.
std::optional<std::vector<std::uint16_t>> DecodeChunkVoxels(std::span<const std::uint8_t> bytes);

// LOD section content as palette + RLE (the LodData Explicit payload, §6.6; before zstd, the
// lod_sections encoding): kLodCellCount materials, already in layer order (core::LodCell), so
// taken as they are.
std::vector<std::uint8_t> EncodeLodCells(const std::vector<std::uint16_t>& cells);
std::optional<std::vector<std::uint16_t>> DecodeLodCells(std::span<const std::uint8_t> bytes);

// posfix: nearest multiple of 1/kPositionFixedScale m (halves round up), clamped to i32.
std::int32_t ToFixedPosition(double v);
inline double FromFixedPosition(std::int32_t v) {
  return v / static_cast<double>(kPositionFixedScale);
}

using Message = std::variant<DatagramPing, DatagramPong, StatusRequest, StatusResponse, ClientHello,
                             Challenge, ClientAuth, Welcome, Reject, Ping, Pong, PlayerInput,
                             PhysicsSnapshot, PlayerEvent, WorldgenCheck, ChunkData, ChunkUnload,
                             BlockEditRequest, VoxelModification, ChunkResync, LodIndex,
                             LodIndexUpdate, LodRequest, LodData, ChunkRequest, HostStatus>;

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
