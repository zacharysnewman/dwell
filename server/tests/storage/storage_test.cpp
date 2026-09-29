// World persistence (ARCHITECTURE.md §6.4, Phase 3e): the world file's schema and migrations,
// chunk and player records, atomic saves under a crash at every write, the cross-platform file
// (the native and WASM builds each open a file the other wrote), and the server saving and
// reloading an edited world.
#include <doctest/doctest.h>

#include <sqlite3.h>

#include <cstdio>
#include <filesystem>
#include <fstream>

#include "../server_fixture.h"
#include "crash_vfs.h"
#include "dwell/storage/world_db.h"
#include "dwell/storage/world_store.h"

using namespace dwell::test;
using namespace dwell::storage;
namespace fs = std::filesystem;

namespace {

#ifdef __EMSCRIPTEN__
constexpr OpenOptions kPlatform{nullptr, /*wal=*/false};  // the OPFS VFS (Node: a file shim)
#else
constexpr OpenOptions kPlatform{nullptr, /*wal=*/true};
#endif

// A world file path in a scratch directory, removed (with its journal files) first.
std::string Scratch(const std::string& name) {
  const fs::path dir = fs::temp_directory_path() / "dwell-storage-tests";
  fs::create_directories(dir);
  const fs::path path = dir / (name + std::string(kWorldExtension));
  for (const char* suffix : {"", "-journal", "-wal", "-shm"}) fs::remove(path.string() + suffix);
  return path.string();
}

std::unique_ptr<WorldDb> OpenOrFail(const std::string& path, OpenOptions options = kPlatform) {
  std::string error;
  auto db = WorldDb::Open(path, error, options);
  INFO(error);
  REQUIRE(db);
  return db;
}

std::vector<std::uint16_t> Filled(std::uint16_t base) {
  std::vector<std::uint16_t> v(dwell::core::kChunkVolume);
  for (int i = 0; i < dwell::core::kChunkVolume; ++i)
    v[static_cast<std::size_t>(i)] = i < 1024 ? base : 0;
  v[12345] = static_cast<std::uint16_t>(base + 1);
  return v;
}

// Chunk contents that compress poorly, so a save spans many database pages.
std::vector<std::uint16_t> Noisy(std::uint32_t seed) {
  std::vector<std::uint16_t> v(dwell::core::kChunkVolume);
  std::uint32_t h = seed * 2654435761u + 1;
  for (auto& m : v) {
    h ^= h << 13;
    h ^= h >> 17;
    h ^= h << 5;
    m = static_cast<std::uint16_t>(h % 20);
  }
  return v;
}

PublicKey Key(std::uint8_t b) {
  PublicKey k{};
  k.fill(b);
  return k;
}

// The contents every golden world file holds (written natively and in WASM; both builds read both).
SaveBatch GoldenBatch() {
  SaveBatch b;
  b.meta = WorldMeta{0xDEADBEEFCAFEull, 3, std::array<double, 3>{7999488.5, 12.0, -3.25}, 4321,
                     1700000000,        0};
  b.chunks.push_back({{0, 0, 0}, 3, Filled(2)});
  b.chunks.push_back({{-250000, 191, 250000}, 1, Filled(16)});
  PlayerRecord p;
  p.key = Key(7);
  p.display_name = "Golden";
  p.feet = {1.5, 64.0, -2.25};
  p.health = 87;
  p.first_seen = 1700000000;
  p.last_seen = 1700000100;
  b.players.push_back(p);
  return b;
}

void CheckGolden(WorldDb& db) {
  const auto meta = db.LoadMeta();
  REQUIRE(meta);
  CHECK(meta->world_seed == 0xDEADBEEFCAFEull);
  CHECK(meta->generator_version == 3);
  REQUIRE(meta->spawn);
  CHECK((*meta->spawn)[0] == 7999488.5);
  CHECK(meta->world_tick == 4321);
  CHECK(meta->created_at == 1700000000);
  CHECK(db.ChunkIndex().size() == 2);
  const auto far = db.LoadChunk({-250000, 191, 250000});
  REQUIRE(far);
  CHECK(far->revision == 1);
  CHECK(far->voxels == Filled(16));
  const auto player = db.LoadPlayer(Key(7));
  REQUIRE(player);
  CHECK(player->display_name == "Golden");
  CHECK(player->health == 87);
  CHECK(player->feet[2] == -2.25);
  CHECK(db.Setting("name") == std::optional<std::string>("Golden world"));
  CHECK(db.IntegrityCheck() == "ok");
}

// Copies a (closed) world file to `to`.
void CopyWorld(const std::string& from, const std::string& to) {
  for (const char* suffix : {"", "-journal", "-wal"}) {
    fs::remove(to + suffix);
    if (fs::exists(from + suffix)) fs::copy_file(from + suffix, to + suffix);
  }
}

}  // namespace

TEST_CASE("storage: a new world file is created at the current format with schema v1") {
  const std::string path = Scratch("fresh");
  auto db = OpenOrFail(path);
  CHECK(db->format_version() == kFormatVersion);
  CHECK_FALSE(db->LoadMeta());
  CHECK(db->ChunkIndex().empty());
  CHECK(db->IntegrityCheck() == "ok");
}

TEST_CASE("storage: files from a newer build are refused, and non-world files too") {
  const std::string path = Scratch("newer");
  { auto db = OpenOrFail(path); }
  sqlite3* raw = nullptr;
  REQUIRE(sqlite3_open(path.c_str(), &raw) == SQLITE_OK);
  sqlite3_exec(raw, "PRAGMA user_version = 99;", nullptr, nullptr, nullptr);
  sqlite3_close(raw);
  std::string error;
  CHECK_FALSE(WorldDb::Open(path, error, kPlatform));
  CHECK(error.find("newer") != std::string::npos);

  const std::string junk = Scratch("junk");
  std::ofstream(junk) << "this is not a database, just some text that is long enough to fail";
  CHECK_FALSE(WorldDb::Open(junk, error, kPlatform));
}

TEST_CASE("storage: meta, chunks, players, settings and permissions round-trip") {
  const std::string path = Scratch("roundtrip");
  {
    auto db = OpenOrFail(path);
    std::string error;
    REQUIRE(db->Save(GoldenBatch(), error));
    REQUIRE(db->SetSetting("name", "Golden world"));
    REQUIRE(db->SetPermission({Key(1), PermissionKind::kBan, "griefing", std::nullopt, 0}));
    REQUIRE(db->SetPermission({Key(2), PermissionKind::kOp, "", Key(1), 0}));
  }
  auto db = OpenOrFail(path);
  CheckGolden(*db);
  auto perms = db->Permissions();
  REQUIRE(perms.size() == 2);
  const auto op = std::find_if(perms.begin(), perms.end(),
                               [](const auto& p) { return p.kind == PermissionKind::kOp; });
  REQUIRE(op != perms.end());
  CHECK(op->by == std::optional<PublicKey>(Key(1)));
  CHECK(db->RemovePermission(Key(2), PermissionKind::kOp));
  perms = db->Permissions();
  REQUIRE(perms.size() == 1);
  CHECK(perms[0].kind == PermissionKind::kBan);
  CHECK(perms[0].reason == "griefing");
  CHECK(perms[0].at > 0);

  // A later save updates a chunk and a player (last seen) without touching the rest.
  SaveBatch next;
  next.chunks.push_back({{0, 0, 0}, 4, Filled(3)});
  PlayerRecord p = GoldenBatch().players[0];
  p.health = 12;
  p.first_seen = 0;  // keep the stored value
  next.players.push_back(p);
  std::string error;
  REQUIRE(db->Save(next, error));
  CHECK(db->LoadChunk({0, 0, 0})->revision == 4);
  CHECK(db->LoadPlayer(Key(7))->health == 12);
  CHECK(db->LoadPlayer(Key(7))->first_seen == 1700000000);
  CHECK(db->LoadMeta()->world_tick == 4321);
}

TEST_CASE("storage: chunk blobs are zstd-compressed palette + RLE, and corrupt ones are rejected") {
  const auto voxels = Filled(2);
  const auto blob = CompressChunk(voxels);
  CHECK(blob.size() < 100);
  CHECK(DecompressChunk(blob) == voxels);
  auto broken = blob;
  broken[broken.size() / 2] ^= 0xFF;
  CHECK_FALSE(DecompressChunk(broken));
  CHECK_FALSE(DecompressChunk(std::vector<std::uint8_t>{1, 2, 3}));
}

TEST_CASE("storage: a crash at any point of a save leaves the previous save or the new one") {
  CrashVfs::Register();
  const std::string base = Scratch("crash-base");
  {
    auto db = OpenOrFail(base);
    std::string error;
    REQUIRE(db->Save(GoldenBatch(), error));
    REQUIRE(db->SetSetting("name", "Golden world"));
  }
  SaveBatch next;
  next.chunks.push_back({{0, 0, 0}, 4, Filled(5)});
  for (int i = 0; i < 12; ++i) {
    next.chunks.push_back({{i, 1, 0}, 1, Noisy(static_cast<std::uint32_t>(i))});
  }
  const std::string work = Scratch("crash-work");
  int crashes = 0;
  for (int budget = 0;; ++budget) {
    CAPTURE(budget);
    CopyWorld(base, work);
    bool saved = false;
    {
      OpenOptions crash = kPlatform;
      crash.vfs = "crash";
      auto db = OpenOrFail(work, crash);
      CrashVfs::Arm(budget);
      std::string error;
      saved = db->Save(next, error);
    }  // closing after the crash changes nothing on disk either
    const bool crashed = CrashVfs::crashed;
    CrashVfs::Disarm();
    auto db = OpenOrFail(work);
    CHECK(db->IntegrityCheck() == "ok");
    const auto chunk = db->LoadChunk({0, 0, 0});
    REQUIRE(chunk);
    const bool updated = chunk->revision == 4;
    CHECK(db->ChunkIndex().size() == (updated ? 14u : 2u));  // all of the save or none of it
    if (updated) CHECK(db->LoadChunk({11, 1, 0})->voxels == Noisy(11));
    if (saved) CHECK(updated);  // a save that reported success holds
    CHECK(db->LoadPlayer(Key(7))->health == 87);
    if (!crashed) {
      CHECK(saved);
      break;
    }
    ++crashes;
    REQUIRE(budget < 10000);
  }
  MESSAGE(crashes << " crash points checked");
  CHECK(crashes > 50);
}

TEST_CASE("storage: saves commit on the store's I/O thread when it has one") {
  const std::string path = Scratch("threaded");
  std::string error;
#ifdef __EMSCRIPTEN__
  auto store = std::make_unique<WorldStore>(OpenOrFail(path));
  CHECK_FALSE(store->threaded());
#else
  auto store = WorldStore::OpenFile(path, error);
  REQUIRE(store);
  CHECK(store->threaded());
#endif
  const auto a = store->Save(GoldenBatch());
  SaveBatch more;
  more.chunks.push_back({{9, 9, 9}, 1, Filled(4)});
  const auto b = store->Save(more);
  store->Flush();
  const auto done = store->TakeCompleted();
  REQUIRE(done.size() == 2);
  CHECK(done[0].id == a);
  CHECK(done[1].id == b);
  CHECK(done[0].ok);
  CHECK(done[1].ok);
  CHECK(store->db().ChunkIndex().size() == 3);  // the tick's connection sees the commits
}

// Golden world files: each build writes its own when asked (DWELL_WRITE_GOLDEN=1) and reads both,
// so a file saved natively opens in the browser build and vice versa.
TEST_CASE("storage: world files written natively and in WASM open in both builds") {
#ifdef __EMSCRIPTEN__
  const std::string own = std::string(DWELL_STORAGE_GOLDEN) + "/wasm.dwellworld";
#else
  const std::string own = std::string(DWELL_STORAGE_GOLDEN) + "/native.dwellworld";
#endif
  if (std::getenv("DWELL_WRITE_GOLDEN")) {
    for (const char* suffix : {"", "-journal", "-wal", "-shm"}) fs::remove(own + suffix);
    auto db = OpenOrFail(own);
    std::string error;
    REQUIRE(db->Save(GoldenBatch(), error));
    REQUIRE(db->SetSetting("name", "Golden world"));
  }
  for (const char* name : {"native", "wasm"}) {
    CAPTURE(name);
    const std::string golden = std::string(DWELL_STORAGE_GOLDEN) + "/" + name + ".dwellworld";
    REQUIRE(fs::exists(golden));
    const std::string copy = Scratch(std::string("golden-") + name);
    CopyWorld(golden, copy);  // opening converts the journal mode: leave the golden untouched
    auto db = OpenOrFail(copy);
    CheckGolden(*db);
  }
}

TEST_CASE("persistence: an edited world survives a server restart") {
  const std::string path = Scratch("server");
  auto open_store = [&] {
    std::string error;
#ifdef __EMSCRIPTEN__
    auto db = WorldDb::Open(path, error, kPlatform);
    REQUIRE(db);
    return std::make_shared<WorldStore>(std::move(db));
#else
    auto store = WorldStore::OpenFile(path, error);
    REQUIRE(store);
    return std::shared_ptr<WorldStore>(std::move(store));
#endif
  };
  ServerConfig config = Flat();
  config.world_seed = 77;
  {
    config.store = open_store();
    Fixture f(config);
    f.Join(1, Client(1));
    f.server.Step();
    f.Send(1, BlockEditRequest{BlockEditAction::kPlace, {-1, -1, 2}, 2, Materials::kLog});
    f.server.Step();
    REQUIRE(f.server.world().GetVoxel(-1, 0, 2) == Materials::kLog);
    f.MoveTo(1, 3.5f, 0.0f, 4.5f);
    f.server.Step();
    CHECK(f.server.save_stats().dirty == 1);
    f.server.SaveNow();
    config.store->Flush();
    f.server.Step();
    CHECK(f.server.save_stats().dirty == 0);
    CHECK(f.server.save_stats().saved_chunks == 1);
    CHECK(f.server.save_stats().saves >= 2);  // the new world's meta, then this one
  }
  config.store.reset();

  // A new server on the same file: the launch options' seed is ignored for the saved world's.
  ServerConfig again = Flat();
  again.world_seed = 1;
  again.store = open_store();
  Fixture f(again);
  CHECK(f.server.world().GetVoxel(-1, 0, 2) == Materials::kLog);
  CHECK(f.server.world().Find({-1, 0, 0})->revision() == 1);
  CHECK(f.server.save_stats().loaded >= 1);
  // The returning player continues where it was.
  const Welcome w = f.Join(1, Client(1));
  CHECK(w.world_seed == 77);
  const auto h = *f.server.PlayerHandleOf(w.player_id);
  const auto p = f.server.players().Position(h);
  CHECK(p.GetX() == doctest::Approx(3.5));
  CHECK(p.GetZ() == doctest::Approx(4.5));
  // The edited chunk streams Explicit (in generated mode), its neighbours Generated.
  Chunk v;
  GenerateFlatChunk({w.verification_chunk[0], w.verification_chunk[1], w.verification_chunk[2]}, v);
  f.Send(1, WorldgenCheck{ChunkHash(v)});
  const auto chunks = f.Chunks(1, 10);
  const auto edited = std::find_if(chunks.begin(), chunks.end(), [](const ChunkData& m) {
    return m.coord == ChunkCoordNet{-1, 0, 0};
  });
  REQUIRE(edited != chunks.end());
  CHECK(edited->form == ChunkForm::kExplicit);
  CHECK(edited->revision == 1);
}

TEST_CASE("persistence: saved chunks far from players are evicted and read back when needed") {
  const std::string path = Scratch("evict");
  std::string error;
  auto db = WorldDb::Open(path, error, kPlatform);
  REQUIRE(db);
  ServerConfig config = Flat();
  config.store = std::make_shared<WorldStore>(std::move(db));  // saves inline
  Fixture f(config);
  f.Join(1, Client(1));
  f.server.Step();
  f.Send(1, BlockEditRequest{BlockEditAction::kPlace, {-1, -1, 2}, 2, Materials::kSand});
  f.server.Step();
  f.server.SaveNow();
  // Walk far away: the saved chunk is evicted, not lost.
  f.MoveTo(1, 500.5f, 0.0f, 0.5f);
  for (int i = 0; i < 130; ++i) f.server.Step();
  CHECK_FALSE(f.server.world().Find({-1, 0, 0}));
  f.MoveTo(1, 0.5f, 0.0f, 0.5f);
  for (int i = 0; i < 3; ++i) f.server.Step();
  REQUIRE(f.server.world().Find({-1, 0, 0}));
  CHECK(f.server.world().GetVoxel(-1, 0, 2) == Materials::kSand);
}

TEST_CASE("persistence: banned players are refused, and an allow-list admits only its keys") {
  ServerConfig config = Flat();
  config.banned = {Client(2).public_key};
  config.allow_list = std::vector<PublicKey>{Client(1).public_key, Client(2).public_key};
  Fixture f(config);
  f.server.OnConnected(2, TransportKind::kWebTransport, f.binding);
  f.Send(2, ClientHello{kProtocolVersion, "test", Client(2).public_key, "Banned"});
  const auto nonce = std::get<Challenge>(f.Take(2).at(0).first).nonce;
  const auto transcript = AuthTranscript(nonce, f.binding, Client(2).public_key);
  Signature sig{};
  crypto_ed25519_sign(sig.data(), Client(2).secret.data(), transcript.data(), transcript.size());
  f.Send(2, ClientAuth{sig});
  const auto reply = f.Take(2);
  REQUIRE(!reply.empty());
  CHECK(std::get<Reject>(reply[0].first).reason == RejectReason::kBanned);
  CHECK(f.server.joined_players() == 0);
  f.Join(1, Client(1));
  CHECK(f.server.joined_players() == 1);
}

TEST_CASE("debug tooling: regenerate and diff lists exactly the voxels changed since generation") {
  Chunk generated;
  GenerateFlatChunk({0, -1, 0}, generated);
  Chunk current = generated;
  CHECK(DiffChunk(generated, current).empty());
  current.SetAt(LocalIndex(3, 31, 4), Materials::kAir);  // grass broken
  current.SetAt(LocalIndex(0, 0, 0), Materials::kLog);
  const auto diff = DiffChunk(generated, current);
  REQUIRE(diff.size() == 2);
  CHECK(diff[0].index == LocalIndex(0, 0, 0));
  CHECK(diff[0].current == Materials::kLog);
  CHECK(diff[1].index == LocalIndex(3, 31, 4));
  CHECK(diff[1].generated == Materials::kGrass);
  CHECK(diff[1].current == Materials::kAir);
}
