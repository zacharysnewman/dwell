#pragma once

#include <array>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/protocol/messages.h"

struct sqlite3;

// World persistence (ARCHITECTURE.md §6.4, ADR 0006): one SQLite database file per world holding
// all of its data. The same code runs natively (SQLite's file VFS, WAL) and in the browser's
// local-mode worker (an OPFS VFS, server/wasm/opfs_vfs.cpp, with a rollback journal).
namespace dwell::storage {

// Schema version written by this build (PRAGMA user_version, mirrored in meta.format_version).
// Opening a new file creates it at this version; a newer one is refused. Files of formats 1 and 2
// predate the block registry (chunk palettes of runtime ids, no `block_states` table) and are
// refused too: worlds are version-locked (RELEASES.md §6), not converted. Format 3 stores chunk
// palettes as world state ids (BLOCK_REGISTRY.md §4).
inline constexpr int kFormatVersion = 3;

// File extension of world files (and of the export format: the database itself).
inline constexpr std::string_view kWorldExtension = ".dwellworld";

struct WorldMeta {
  std::uint64_t world_seed = 0;
  std::uint32_t generator_version = 0;
  std::optional<std::array<double, 3>> spawn;  // feet; unset: the generator's spawn point
  std::uint32_t world_tick = 0;                // simulation time
  std::int64_t created_at = 0;                 // unix seconds
  std::int64_t saved_at = 0;
  // The app versions (RELEASES.md §6) that created the world and last saved it: they lock it to
  // their compatibility line. Empty in a file saved before versioned releases. On save, `created`
  // is kept if the file already has one.
  std::string app_version_created = {};
  std::string app_version_last = {};
};

struct SavedChunk {
  core::ChunkCoord coord;
  std::uint32_t revision = 0;
  std::vector<std::uint16_t> voxels;  // kChunkVolume runtime state ids, chunk index order
};

// A modified LOD section (§6.6): a cache derivable from the chunks. `encoded` is the palette + RLE
// of its 34³ cells (protocol::EncodeLodCells); empty for a section still to compute (`dirty`).
struct SavedLodSection {
  core::LodCoord coord;
  std::uint32_t revision = 0;
  bool dirty = false;
  std::vector<std::uint8_t> encoded;
};

// A player's saved state, keyed by device public key (§10.4).
struct PlayerRecord {
  protocol::PublicKey key{};
  std::string display_name;
  std::array<double, 3> feet{};
  int health = protocol::kMaxHealth;
  std::int64_t first_seen = 0;  // unix seconds; 0 on save: keep the stored value (or now)
  std::int64_t last_seen = 0;
};

enum class PermissionKind : std::uint8_t { kOp, kBan, kAllow };
struct PermissionEntry {
  protocol::PublicKey key{};
  PermissionKind kind = PermissionKind::kOp;
  std::string reason;
  std::optional<protocol::PublicKey> by;  // who granted it (unset: the server's operator)
  std::int64_t at = 0;                    // unix seconds; 0 on save: now
};

// Everything one autosave writes, committed in one transaction.
struct SaveBatch {
  std::optional<WorldMeta> meta;
  std::vector<SavedChunk> chunks;
  std::vector<PlayerRecord> players;
  // LOD cache rows written or re-marked since the last save; `clear_lod` drops the table first
  // (a rebuild after a generator version change).
  std::vector<SavedLodSection> lod_sections;
  bool clear_lod = false;
};

struct OpenOptions {
  // SQLite VFS name; null = the platform default.
  const char* vfs = nullptr;
  // Write-ahead log (native: readers never wait for a save). Off: a rollback journal in TRUNCATE
  // mode with exclusive locking (the browser's OPFS VFS has no shared memory).
  bool wal = true;
};

class WorldDb {
 public:
  // Opens or creates the world file at `path` and migrates it to kFormatVersion. On failure
  // returns null and sets `error`.
  static std::unique_ptr<WorldDb> Open(const std::string& path, std::string& error,
                                       const OpenOptions& options = {});
  ~WorldDb();
  WorldDb(const WorldDb&) = delete;
  WorldDb& operator=(const WorldDb&) = delete;

  // Unset for a new world (no meta written yet).
  std::optional<WorldMeta> LoadMeta();
  // Modified chunks in the file and their revisions.
  std::vector<std::pair<core::ChunkCoord, std::uint32_t>> ChunkIndex();
  std::optional<SavedChunk> LoadChunk(const core::ChunkCoord& coord);
  // The world's block states: world state id → canonical string (BLOCK_REGISTRY.md §4). Chunks are
  // stored with these ids, so a world outlives changes to the code's runtime ids.
  std::vector<std::string> BlockStates();

  // The LOD cache (format 2). Rows of another generator version are skipped and `stale` set: the
  // cache must be rebuilt from the chunks.
  std::vector<SavedLodSection> LodSections(std::uint32_t generator_version, bool& stale);
  std::optional<PlayerRecord> LoadPlayer(const protocol::PublicKey& key);
  std::optional<std::string> Setting(std::string_view key);
  bool SetSetting(std::string_view key, std::string_view value);
  std::vector<PermissionEntry> Permissions();
  bool SetPermission(const PermissionEntry& entry);
  bool RemovePermission(const protocol::PublicKey& key, PermissionKind kind);

  // Writes the batch in one transaction: all of it or (on any failure) none of it.
  bool Save(const SaveBatch& batch, std::string& error);

  int format_version();
  // "ok" when the file is intact (PRAGMA integrity_check).
  std::string IntegrityCheck();

  // Tests: called inside Save's transaction, just before COMMIT.
  std::function<void()> before_commit;

 private:
  explicit WorldDb(sqlite3* db) : db_(db) {}
  bool Exec(const char* sql, std::string* error = nullptr);
  // Reloads `block_states` and the id maps between runtime and world state ids.
  void RefreshStates();
  sqlite3* db_;
  // World state id → runtime id (-1: a string this build's registry does not know).
  std::vector<std::int32_t> to_runtime_;
  // Runtime id → world state id (-1: not in the world file yet).
  std::vector<std::int32_t> to_world_;
  std::vector<std::string> world_states_;
};

// Player state blob (players.state), version 1: u8 1, f64×3 feet, u8 health.
std::vector<std::uint8_t> EncodePlayerState(const PlayerRecord& player);
bool DecodePlayerState(std::span<const std::uint8_t> bytes, PlayerRecord& player);

// zstd (level 3) of a blob, and back (nullopt if corrupt or larger than `max_size`).
std::vector<std::uint8_t> CompressBytes(std::span<const std::uint8_t> raw);
std::optional<std::vector<std::uint8_t>> DecompressBytes(std::span<const std::uint8_t> blob,
                                                         std::size_t max_size);

// Chunk blob (chunks.data): zstd of the palette + RLE voxels (ChunkData Explicit payload). Free
// functions code whatever ids they are given; WorldDb translates between runtime and world state
// ids around them.
std::vector<std::uint8_t> CompressChunk(const std::vector<std::uint16_t>& voxels);
std::optional<std::vector<std::uint16_t>> DecompressChunk(std::span<const std::uint8_t> blob);

}  // namespace dwell::storage
