#include "dwell/storage/world_db.h"

#include <sqlite3.h>
#include <zstd.h>

#include <algorithm>
#include <cmath>
#include <ctime>

#include "dwell/protocol/bytes.h"

namespace dwell::storage {
namespace {

// Ordered schema migrations: kMigrations[i] upgrades a file from format i to i + 1. Later formats
// add tables here (bodies, Phase 5; lod_sections, Phase 4) without touching earlier steps.
constexpr const char* kMigrations[] = {
    // 0 → 1: schema v1 (§6.4).
    R"sql(
      CREATE TABLE meta (key TEXT PRIMARY KEY, value) WITHOUT ROWID;
      CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
      CREATE TABLE chunks (
        cx INTEGER NOT NULL, cy INTEGER NOT NULL, cz INTEGER NOT NULL,
        revision INTEGER NOT NULL,
        generator_version INTEGER NOT NULL,
        data BLOB NOT NULL,
        PRIMARY KEY (cx, cy, cz)) WITHOUT ROWID;
      CREATE TABLE players (
        public_key BLOB PRIMARY KEY,
        display_name TEXT NOT NULL,
        state BLOB NOT NULL,
        first_seen INTEGER NOT NULL,
        last_seen INTEGER NOT NULL) WITHOUT ROWID;
      CREATE TABLE permissions (
        public_key BLOB NOT NULL,
        kind TEXT NOT NULL CHECK (kind IN ('op', 'ban', 'allow')),
        reason TEXT NOT NULL DEFAULT '',
        granted_by BLOB,
        at INTEGER NOT NULL,
        PRIMARY KEY (public_key, kind)) WITHOUT ROWID;
    )sql",
    // 1 → 2: the LOD cache (Phase 4, §6.6).
    R"sql(
      CREATE TABLE lod_sections (
        level INTEGER NOT NULL, i INTEGER NOT NULL, j INTEGER NOT NULL, k INTEGER NOT NULL,
        revision INTEGER NOT NULL,
        dirty INTEGER NOT NULL,
        generator_version INTEGER NOT NULL,
        data BLOB NOT NULL,
        PRIMARY KEY (level, i, j, k)) WITHOUT ROWID;
    )sql",
};
static_assert(std::size(kMigrations) == kFormatVersion);

constexpr int kZstdLevel = 3;
// Palette + RLE of one LOD section is at most palette (2 + 2·65535) + runs (4 per cell).
constexpr std::size_t kMaxLodBytes = 2 + 2 * 65535 + 5ull * core::kLodVolume;

const char* KindName(PermissionKind k) {
  return k == PermissionKind::kOp ? "op" : k == PermissionKind::kBan ? "ban" : "allow";
}

// A prepared statement, finalized on scope exit.
class Stmt {
 public:
  Stmt(sqlite3* db, const char* sql) { sqlite3_prepare_v2(db, sql, -1, &s_, nullptr); }
  ~Stmt() { sqlite3_finalize(s_); }
  Stmt(const Stmt&) = delete;
  Stmt& operator=(const Stmt&) = delete;
  explicit operator bool() const { return s_ != nullptr; }
  sqlite3_stmt* get() { return s_; }

  Stmt& Int(int i, std::int64_t v) {
    sqlite3_bind_int64(s_, i, v);
    return *this;
  }
  Stmt& Real(int i, double v) {
    sqlite3_bind_double(s_, i, v);
    return *this;
  }
  Stmt& Text(int i, std::string_view v) {
    sqlite3_bind_text(s_, i, v.data(), static_cast<int>(v.size()), SQLITE_TRANSIENT);
    return *this;
  }
  Stmt& Blob(int i, std::span<const std::uint8_t> v) {
    if (v.empty()) {
      sqlite3_bind_zeroblob(s_, i, 0);  // an empty blob, not NULL
    } else {
      sqlite3_bind_blob(s_, i, v.data(), static_cast<int>(v.size()), SQLITE_TRANSIENT);
    }
    return *this;
  }
  // SQLITE_ROW, SQLITE_DONE, or an error.
  int Step() { return s_ ? sqlite3_step(s_) : SQLITE_ERROR; }
  void Reset() {
    sqlite3_reset(s_);
    sqlite3_clear_bindings(s_);
  }

  std::int64_t ColInt(int i) { return sqlite3_column_int64(s_, i); }
  double ColReal(int i) { return sqlite3_column_double(s_, i); }
  bool ColNull(int i) { return sqlite3_column_type(s_, i) == SQLITE_NULL; }
  std::string ColText(int i) {
    const auto* p = sqlite3_column_text(s_, i);
    return p ? std::string(reinterpret_cast<const char*>(p),
                           static_cast<std::size_t>(sqlite3_column_bytes(s_, i)))
             : std::string();
  }
  std::span<const std::uint8_t> ColBlob(int i) {
    const auto* p = static_cast<const std::uint8_t*>(sqlite3_column_blob(s_, i));
    return {p, static_cast<std::size_t>(sqlite3_column_bytes(s_, i))};
  }

 private:
  sqlite3_stmt* s_ = nullptr;
};

std::int64_t Now() { return static_cast<std::int64_t>(std::time(nullptr)); }

}  // namespace

std::unique_ptr<WorldDb> WorldDb::Open(const std::string& path, std::string& error,
                                       const OpenOptions& options) {
  sqlite3* raw = nullptr;
  const int rc =
      sqlite3_open_v2(path.c_str(), &raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, options.vfs);
  std::unique_ptr<WorldDb> db(new WorldDb(raw));
  if (rc != SQLITE_OK) {
    error = raw ? sqlite3_errmsg(raw) : "out of memory";
    return nullptr;
  }
  sqlite3_busy_timeout(raw, 5000);
  // Journal: WAL natively; in the browser a rollback journal with exclusive locking (no shared
  // memory). Either converts a file written by the other on open.
  const char* setup = options.wal ? "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;"
                                  : "PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=TRUNCATE;"
                                    " PRAGMA synchronous=FULL;";
  if (!db->Exec(setup, &error)) return nullptr;

  const int version = db->format_version();
  if (version < 0) {
    error = "not a world file";
    return nullptr;
  }
  if (version > kFormatVersion) {
    error = "world file format " + std::to_string(version) + " is newer than this build (" +
            std::to_string(kFormatVersion) + ")";
    return nullptr;
  }
  for (int v = version; v < kFormatVersion; ++v) {
    const std::string sql = std::string("BEGIN IMMEDIATE;") + kMigrations[v] +
                            "INSERT OR REPLACE INTO meta VALUES ('format_version', " +
                            std::to_string(v + 1) +
                            "); PRAGMA user_version = " + std::to_string(v + 1) + "; COMMIT;";
    if (!db->Exec(sql.c_str(), &error)) {
      db->Exec("ROLLBACK;");
      error = "migration to format " + std::to_string(v + 1) + " failed: " + error;
      return nullptr;
    }
  }
  return db;
}

WorldDb::~WorldDb() { sqlite3_close_v2(db_); }

bool WorldDb::Exec(const char* sql, std::string* error) {
  char* message = nullptr;
  const int rc = sqlite3_exec(db_, sql, nullptr, nullptr, &message);
  if (rc != SQLITE_OK && error) *error = message ? message : sqlite3_errstr(rc);
  sqlite3_free(message);
  return rc == SQLITE_OK;
}

int WorldDb::format_version() {
  Stmt s(db_, "PRAGMA user_version;");
  return s.Step() == SQLITE_ROW ? static_cast<int>(s.ColInt(0)) : -1;
}

std::string WorldDb::IntegrityCheck() {
  Stmt s(db_, "PRAGMA integrity_check;");
  return s.Step() == SQLITE_ROW ? s.ColText(0) : sqlite3_errmsg(db_);
}

std::optional<WorldMeta> WorldDb::LoadMeta() {
  Stmt s(db_, "SELECT key, value FROM meta;");
  WorldMeta m;
  bool any = false;
  std::array<std::optional<double>, 3> spawn;
  while (s.Step() == SQLITE_ROW) {
    const std::string key = s.ColText(0);
    if (key == "world_seed") {
      m.world_seed = static_cast<std::uint64_t>(s.ColInt(1));
      any = true;
    } else if (key == "generator_version") {
      m.generator_version = static_cast<std::uint32_t>(s.ColInt(1));
    } else if (key == "world_tick") {
      m.world_tick = static_cast<std::uint32_t>(s.ColInt(1));
    } else if (key == "created_at") {
      m.created_at = s.ColInt(1);
    } else if (key == "saved_at") {
      m.saved_at = s.ColInt(1);
    } else if (key.starts_with("spawn_") && key.size() == 7 && !s.ColNull(1)) {
      spawn[static_cast<std::size_t>(key[6] - 'x')] = s.ColReal(1);
    }
  }
  if (!any) return std::nullopt;
  if (spawn[0] && spawn[1] && spawn[2])
    m.spawn = std::array<double, 3>{*spawn[0], *spawn[1], *spawn[2]};
  return m;
}

std::vector<std::pair<core::ChunkCoord, std::uint32_t>> WorldDb::ChunkIndex() {
  std::vector<std::pair<core::ChunkCoord, std::uint32_t>> out;
  Stmt s(db_, "SELECT cx, cy, cz, revision FROM chunks;");
  while (s.Step() == SQLITE_ROW) {
    out.push_back({{static_cast<std::int32_t>(s.ColInt(0)), static_cast<std::int32_t>(s.ColInt(1)),
                    static_cast<std::int32_t>(s.ColInt(2))},
                   static_cast<std::uint32_t>(s.ColInt(3))});
  }
  return out;
}

std::optional<SavedChunk> WorldDb::LoadChunk(const core::ChunkCoord& c) {
  Stmt s(db_, "SELECT revision, data FROM chunks WHERE cx = ? AND cy = ? AND cz = ?;");
  s.Int(1, c.x).Int(2, c.y).Int(3, c.z);
  if (s.Step() != SQLITE_ROW) return std::nullopt;
  auto voxels = DecompressChunk(s.ColBlob(1));
  if (!voxels) return std::nullopt;
  return SavedChunk{c, static_cast<std::uint32_t>(s.ColInt(0)), std::move(*voxels)};
}

std::vector<SavedLodSection> WorldDb::LodSections(std::uint32_t generator_version, bool& stale) {
  stale = false;
  std::vector<SavedLodSection> out;
  Stmt s(db_, "SELECT level, i, j, k, revision, dirty, generator_version, data FROM lod_sections;");
  while (s.Step() == SQLITE_ROW) {
    if (static_cast<std::uint32_t>(s.ColInt(6)) != generator_version) {
      stale = true;
      continue;
    }
    SavedLodSection l;
    l.coord = {static_cast<int>(s.ColInt(0)), static_cast<std::int32_t>(s.ColInt(1)),
               static_cast<std::int32_t>(s.ColInt(2)), static_cast<std::int32_t>(s.ColInt(3))};
    l.revision = static_cast<std::uint32_t>(s.ColInt(4));
    l.dirty = s.ColInt(5) != 0;
    if (const auto blob = s.ColBlob(7); !blob.empty()) {
      auto raw = DecompressBytes(blob, kMaxLodBytes);
      if (!raw) {
        l.dirty = true;  // unreadable: compute it again
      } else {
        l.encoded = std::move(*raw);
      }
    }
    out.push_back(std::move(l));
  }
  return out;
}

std::optional<PlayerRecord> WorldDb::LoadPlayer(const protocol::PublicKey& key) {
  Stmt s(db_,
         "SELECT display_name, state, first_seen, last_seen FROM players WHERE public_key = ?;");
  s.Blob(1, key);
  if (s.Step() != SQLITE_ROW) return std::nullopt;
  PlayerRecord p;
  p.key = key;
  p.display_name = s.ColText(0);
  if (!DecodePlayerState(s.ColBlob(1), p)) return std::nullopt;
  p.first_seen = s.ColInt(2);
  p.last_seen = s.ColInt(3);
  return p;
}

std::optional<std::string> WorldDb::Setting(std::string_view key) {
  Stmt s(db_, "SELECT value FROM settings WHERE key = ?;");
  s.Text(1, key);
  if (s.Step() != SQLITE_ROW) return std::nullopt;
  return s.ColText(0);
}

bool WorldDb::SetSetting(std::string_view key, std::string_view value) {
  Stmt s(db_, "INSERT OR REPLACE INTO settings VALUES (?, ?);");
  s.Text(1, key).Text(2, value);
  return s.Step() == SQLITE_DONE;
}

std::vector<PermissionEntry> WorldDb::Permissions() {
  std::vector<PermissionEntry> out;
  Stmt s(db_, "SELECT public_key, kind, reason, at, granted_by FROM permissions;");
  while (s.Step() == SQLITE_ROW) {
    const auto key = s.ColBlob(0);
    if (key.size() != 32) continue;
    PermissionEntry e;
    std::copy(key.begin(), key.end(), e.key.begin());
    const std::string kind = s.ColText(1);
    e.kind = kind == "op"    ? PermissionKind::kOp
             : kind == "ban" ? PermissionKind::kBan
                             : PermissionKind::kAllow;
    e.reason = s.ColText(2);
    e.at = s.ColInt(3);
    if (const auto by = s.ColBlob(4); by.size() == 32) {
      e.by.emplace();
      std::copy(by.begin(), by.end(), e.by->begin());
    }
    out.push_back(std::move(e));
  }
  return out;
}

bool WorldDb::SetPermission(const PermissionEntry& e) {
  Stmt s(db_, "INSERT OR REPLACE INTO permissions VALUES (?, ?, ?, ?, ?);");
  s.Blob(1, e.key).Text(2, KindName(e.kind)).Text(3, e.reason);
  if (e.by) s.Blob(4, *e.by);
  s.Int(5, e.at ? e.at : Now());
  return s.Step() == SQLITE_DONE;
}

bool WorldDb::RemovePermission(const protocol::PublicKey& key, PermissionKind kind) {
  Stmt s(db_, "DELETE FROM permissions WHERE public_key = ? AND kind = ?;");
  s.Blob(1, key).Text(2, KindName(kind));
  return s.Step() == SQLITE_DONE;
}

bool WorldDb::Save(const SaveBatch& batch, std::string& error) {
  if (!Exec("BEGIN IMMEDIATE;", &error)) return false;
  auto fail = [&](const char* what) {
    error = std::string(what) + ": " + sqlite3_errmsg(db_);
    Exec("ROLLBACK;");
    return false;
  };
  const std::int64_t now = Now();
  std::uint32_t generator_version = 0;
  if (batch.meta) {
    const WorldMeta& m = *batch.meta;
    generator_version = m.generator_version;
    Stmt put(db_, "INSERT OR REPLACE INTO meta VALUES (?, ?);");
    Stmt keep(db_, "INSERT OR IGNORE INTO meta VALUES (?, ?);");
    auto set_int = [&](Stmt& s, const char* key, std::int64_t v) {
      s.Text(1, key).Int(2, v);
      const int rc = s.Step();
      s.Reset();
      return rc == SQLITE_DONE;
    };
    bool ok = set_int(put, "world_seed", static_cast<std::int64_t>(m.world_seed)) &&
              set_int(put, "generator_version", m.generator_version) &&
              set_int(put, "world_tick", m.world_tick) &&
              set_int(keep, "created_at", m.created_at ? m.created_at : now) &&
              set_int(put, "saved_at", now);
    for (int i = 0; i < 3 && ok; ++i) {
      const char key[] = {'s', 'p', 'a', 'w', 'n', '_', static_cast<char>('x' + i), '\0'};
      put.Text(1, key);
      if (m.spawn) {
        put.Real(2, (*m.spawn)[static_cast<std::size_t>(i)]);
      } else {
        sqlite3_bind_null(put.get(), 2);
      }
      ok = put.Step() == SQLITE_DONE;
      put.Reset();
    }
    if (!ok) return fail("meta");
  } else {
    Stmt s(db_, "SELECT value FROM meta WHERE key = 'generator_version';");
    if (s.Step() == SQLITE_ROW) generator_version = static_cast<std::uint32_t>(s.ColInt(0));
  }

  if (!batch.chunks.empty()) {
    Stmt put(db_, "INSERT OR REPLACE INTO chunks VALUES (?, ?, ?, ?, ?, ?);");
    for (const SavedChunk& c : batch.chunks) {
      put.Int(1, c.coord.x).Int(2, c.coord.y).Int(3, c.coord.z).Int(4, c.revision);
      put.Int(5, generator_version).Blob(6, CompressChunk(c.voxels));
      if (put.Step() != SQLITE_DONE) return fail("chunks");
      put.Reset();
    }
  }
  if (batch.clear_lod && !Exec("DELETE FROM lod_sections;", &error)) return fail("lod_sections");
  if (!batch.lod_sections.empty()) {
    Stmt put(db_, "INSERT OR REPLACE INTO lod_sections VALUES (?, ?, ?, ?, ?, ?, ?, ?);");
    for (const SavedLodSection& l : batch.lod_sections) {
      put.Int(1, l.coord.level).Int(2, l.coord.i).Int(3, l.coord.j).Int(4, l.coord.k);
      put.Int(5, l.revision).Int(6, l.dirty ? 1 : 0).Int(7, generator_version);
      put.Blob(8, l.encoded.empty() ? std::vector<std::uint8_t>{} : CompressBytes(l.encoded));
      if (put.Step() != SQLITE_DONE) return fail("lod_sections");
      put.Reset();
    }
  }
  if (!batch.players.empty()) {
    Stmt put(db_,
             "INSERT INTO players VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (public_key) DO UPDATE "
             "SET display_name = ?2, state = ?3, last_seen = ?5;");
    for (const PlayerRecord& p : batch.players) {
      put.Blob(1, p.key).Text(2, p.display_name).Blob(3, EncodePlayerState(p));
      put.Int(4, p.first_seen ? p.first_seen : now).Int(5, p.last_seen ? p.last_seen : now);
      if (put.Step() != SQLITE_DONE) return fail("players");
      put.Reset();
    }
  }
  if (before_commit) before_commit();
  if (!Exec("COMMIT;", &error)) {
    Exec("ROLLBACK;");
    return false;
  }
  return true;
}

std::vector<std::uint8_t> EncodePlayerState(const PlayerRecord& p) {
  std::vector<std::uint8_t> out;
  protocol::ByteWriter w(out);
  w.U8(1);
  for (double v : p.feet) w.F64(v);
  w.U8(static_cast<std::uint8_t>(std::clamp(p.health, 0, 255)));
  return out;
}

bool DecodePlayerState(std::span<const std::uint8_t> bytes, PlayerRecord& p) {
  protocol::ByteReader r(bytes);
  r.Check(r.U8() == 1);
  for (double& v : p.feet) v = r.F64();
  p.health = r.U8();
  for (double v : p.feet) r.Check(std::isfinite(v));
  return r.AtEnd();
}

std::vector<std::uint8_t> CompressBytes(std::span<const std::uint8_t> raw) {
  std::vector<std::uint8_t> out(ZSTD_compressBound(raw.size()));
  const std::size_t n = ZSTD_compress(out.data(), out.size(), raw.data(), raw.size(), kZstdLevel);
  if (ZSTD_isError(n)) return {};
  out.resize(n);
  return out;
}

std::optional<std::vector<std::uint8_t>> DecompressBytes(std::span<const std::uint8_t> blob,
                                                         std::size_t max_size) {
  const unsigned long long size = ZSTD_getFrameContentSize(blob.data(), blob.size());
  if (size == ZSTD_CONTENTSIZE_ERROR || size == ZSTD_CONTENTSIZE_UNKNOWN || size > max_size) {
    return std::nullopt;
  }
  std::vector<std::uint8_t> raw(size);
  const std::size_t n = ZSTD_decompress(raw.data(), raw.size(), blob.data(), blob.size());
  if (ZSTD_isError(n) || n != size) return std::nullopt;
  return raw;
}

std::vector<std::uint8_t> CompressChunk(const std::vector<std::uint16_t>& voxels) {
  return CompressBytes(protocol::EncodeChunkVoxels(voxels));
}

std::optional<std::vector<std::uint16_t>> DecompressChunk(std::span<const std::uint8_t> blob) {
  // Palette + RLE of one chunk is at most palette (2 + 2·32768) + runs (4 per voxel): bounded.
  constexpr std::size_t kMaxRaw = 2 + 2 * core::kChunkVolume + 5ull * core::kChunkVolume;
  const auto raw = DecompressBytes(blob, kMaxRaw);
  if (!raw) return std::nullopt;
  return protocol::DecodeChunkVoxels(*raw);
}

}  // namespace dwell::storage
