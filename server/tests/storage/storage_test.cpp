// World persistence (ARCHITECTURE.md §6.4, Phase 3e): the world file's schema and migrations,
// chunk and player records, atomic saves under a crash at every write, the cross-platform file
// (the native and WASM builds each open a file the other wrote), and the server saving and
// reloading an edited world.
#include <doctest/doctest.h>

#include <sqlite3.h>

#include <algorithm>
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

TEST_CASE("storage: a new world file is created at the current format (schema v3)") {
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
  // DWELL_WRITE_GOLDEN=wasm-style (native build only): writes wasm.dwellworld with the browser's
  // journal settings (no WAL, exclusive locking) when no WASM build is at hand.
  if (const char* style = std::getenv("DWELL_WRITE_GOLDEN");
      style && std::string(style) == "wasm-style") {
    const std::string wasm = std::string(DWELL_STORAGE_GOLDEN) + "/wasm.dwellworld";
    for (const char* suffix : {"", "-journal", "-wal", "-shm"}) fs::remove(wasm + suffix);
    auto db = OpenOrFail(wasm, OpenOptions{nullptr, /*wal=*/false});
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

TEST_CASE("storage: worlds saved before the block registry (formats 1 and 2) are refused") {
  for (const int format : {1, 2}) {
    CAPTURE(format);
    const std::string path = Scratch("predates");
    { auto db = OpenOrFail(path); }
    sqlite3* raw = nullptr;
    REQUIRE(sqlite3_open(path.c_str(), &raw) == SQLITE_OK);
    REQUIRE(sqlite3_exec(
                raw,
                ("DROP TABLE block_states; PRAGMA user_version = " + std::to_string(format) + ";")
                    .c_str(),
                nullptr, nullptr, nullptr) == SQLITE_OK);
    sqlite3_close(raw);
    std::string error;
    CHECK_FALSE(WorldDb::Open(path, error, kPlatform));
    CHECK(error.find("block registry") != std::string::npos);
  }
}

namespace {

std::string Exec(const std::string& path, const std::string& sql) {
  sqlite3* raw = nullptr;
  REQUIRE(sqlite3_open(path.c_str(), &raw) == SQLITE_OK);
  char* message = nullptr;
  const int rc = sqlite3_exec(raw, sql.c_str(), nullptr, nullptr, &message);
  std::string error = rc == SQLITE_OK ? "" : message;
  sqlite3_free(message);
  sqlite3_close(raw);
  return error;
}

// A chunk with every state of the registry in it (and runs between).
std::vector<std::uint16_t> EveryState() {
  std::vector<std::uint16_t> v(dwell::core::kChunkVolume, dwell::core::Materials::kStone);
  for (std::uint16_t id = 0; id < dwell::core::Materials::kCount; ++id) v[id * 7] = id;
  return v;
}

}  // namespace

TEST_CASE("storage: a world saved and reopened is identical, every voxel's canonical string") {
  const std::string path = Scratch("strings");
  const auto voxels = EveryState();
  {
    auto db = OpenOrFail(path);
    SaveBatch b;
    b.meta = WorldMeta{1, 4, std::nullopt, 0, 0, 0};
    b.chunks.push_back({{1, 2, 3}, 5, voxels});
    std::string error;
    REQUIRE(db->Save(b, error));
  }
  auto db = OpenOrFail(path);
  const auto chunk = db->LoadChunk({1, 2, 3});
  REQUIRE(chunk);
  CHECK(chunk->voxels == voxels);
  for (std::size_t i = 0; i < voxels.size(); ++i) {
    if (chunk->voxels[i] != voxels[i]) FAIL("voxel " << i);  // not CHECK: 32,768 of them
  }
  // The file names states by canonical string, once each, in order of first use.
  const auto states = db->BlockStates();
  REQUIRE(states.size() == dwell::core::Materials::kCount);
  CHECK(states.front() == "dwell:air");  // the first state in the first voxel
  CHECK(std::count(states.begin(), states.end(), "dwell:ladder[facing=east,flooded=true]") == 1);
}

TEST_CASE("storage: a chunk's meaning is its strings, whatever the code's runtime ids are") {
  const std::string path = Scratch("meaning");
  {
    auto db = OpenOrFail(path);
    SaveBatch b;
    b.meta = WorldMeta{1, 4, std::nullopt, 0, 0, 0};
    b.chunks.push_back({{0, 0, 0}, 1, Filled(2)});  // stone and dirt
    std::string error;
    REQUIRE(db->Save(b, error));
  }
  {
    // Stored with world ids, not runtime ids: ids 2 and 3 are not the file's ids for them.
    auto db = OpenOrFail(path);
    const auto states = db->BlockStates();
    REQUIRE(states.size() == 3);  // stone, air, dirt: in order of first use
    CHECK(states == std::vector<std::string>{"dwell:stone", "dwell:air", "dwell:dirt"});
  }
  // A registry that numbered the blocks differently reads the same strings as the same blocks:
  // here the file's "stone" becomes "dirt" and its "dirt" becomes "stone".
  REQUIRE(Exec(path,
               "UPDATE block_states SET state = 'x' WHERE state = 'dwell:stone';"
               "UPDATE block_states SET state = 'dwell:stone' WHERE state = 'dwell:dirt';"
               "UPDATE block_states SET state = 'dwell:dirt' WHERE state = 'x';")
              .empty());
  auto db = OpenOrFail(path);
  const auto chunk = db->LoadChunk({0, 0, 0});
  REQUIRE(chunk);
  CHECK(chunk->voxels[0] == dwell::core::Materials::kDirt);
  CHECK(chunk->voxels[12345] == dwell::core::Materials::kStone);
  // A state this build does not know cannot be read (the chunk is generated again instead).
  REQUIRE(Exec(path, "UPDATE block_states SET state = 'dwell:removed' WHERE state = 'dwell:dirt';")
              .empty());
  auto reopened = OpenOrFail(path);
  CHECK_FALSE(reopened->LoadChunk({0, 0, 0}));
}

TEST_CASE("storage: a second connection's new states are visible to the first") {
  const std::string path = Scratch("twoconn");
  auto reader = OpenOrFail(path);
  auto writer = OpenOrFail(path);
  SaveBatch b;
  b.meta = WorldMeta{1, 4, std::nullopt, 0, 0, 0};
  b.chunks.push_back({{0, 0, 0}, 1, Filled(16)});
  std::string error;
  REQUIRE(writer->Save(b, error));
  const auto chunk = reader->LoadChunk({0, 0, 0});
  REQUIRE(chunk);
  CHECK(chunk->voxels == Filled(16));
}

TEST_CASE("storage: the LOD cache is a cache: dropped when the registry hash changes") {
  const std::string path = Scratch("lodcache");
  bool stale = true;
  {
    auto db = OpenOrFail(path);
    SaveBatch b;
    b.meta = WorldMeta{1, 0, std::nullopt, 0, 0, 0};
    b.lod_sections.push_back({{3, 1, 2, 3}, 9, false, {1, 2, 3}});
    b.lod_sections.push_back({{4, 5, 6, 7}, 0, true, {}});
    std::string error;
    REQUIRE(db->Save(b, error));
    CHECK(db->LodSections(0, stale).size() == 2);
  }
  {
    auto db = OpenOrFail(path);  // the same registry: kept
    CHECK(db->LodSections(0, stale).size() == 2);
    CHECK(db->LodSections(3, stale).empty());  // another generator version: stale
    CHECK(stale);
  }
  REQUIRE(Exec(path, "UPDATE meta SET value = '1' WHERE key = 'registry_hash';").empty());
  auto db = OpenOrFail(path);  // another registry: dropped
  CHECK(db->LodSections(0, stale).empty());
  CHECK_FALSE(stale);
}

namespace {

// Server + store over one world file, reopened by each call.
std::shared_ptr<WorldStore> OpenStore(const std::string& path) {
  std::string error;
  auto db = WorldDb::Open(path, error, kPlatform);
  REQUIRE(db);
  return std::make_shared<WorldStore>(std::move(db));  // saves inline
}

}  // namespace

TEST_CASE("persistence: LOD sections survive a restart, and are rebuilt when stale or missing") {
  const std::string path = Scratch("lod");
  std::map<LodCoord, std::uint32_t, bool (*)(const LodCoord&, const LodCoord&)> saved(
      [](const LodCoord& a, const LodCoord& b) {
        return std::tie(a.level, a.i, a.j, a.k) < std::tie(b.level, b.i, b.j, b.k);
      });
  {
    ServerConfig config = Flat();
    config.store = OpenStore(path);
    Fixture f(config);
    f.Join(1, Client(1));
    f.server.Step();
    f.Send(1, BlockEditRequest{BlockEditAction::kPlace, {-1, -1, 2}, 2, Materials::kLog});
    f.server.Step();
    f.server.DrainLod();
    REQUIRE(f.server.lod().sections().size() ==
            static_cast<std::size_t>(dwell::core::kLodMaxLevel));
    for (const auto& [c, s] : f.server.lod().sections()) saved[c] = s.revision;
    f.server.SaveNow();
  }
  {
    // Restored as saved: nothing to recompute, the same revisions.
    ServerConfig config = Flat();
    config.store = OpenStore(path);
    Fixture f(config);
    CHECK(f.server.lod().pending() == 0);
    REQUIRE(f.server.lod().sections().size() == saved.size());
    for (const auto& [c, s] : f.server.lod().sections()) CHECK(saved[c] == s.revision);
  }
  // A world saved before the LOD cache existed (no rows): derived from its chunks.
  {
    sqlite3* raw = nullptr;
    REQUIRE(sqlite3_open(path.c_str(), &raw) == SQLITE_OK);
    REQUIRE(sqlite3_exec(raw, "DELETE FROM lod_sections;", nullptr, nullptr, nullptr) == SQLITE_OK);
    sqlite3_close(raw);
    ServerConfig config = Flat();
    config.store = OpenStore(path);
    Fixture f(config);
    CHECK(f.server.lod().pending() == 1);  // the edited chunk's level-1 section
    f.server.DrainLod();
    CHECK(f.server.lod().sections().size() == saved.size());
    f.server.SaveNow();
  }
  // Rows of another generator version: dropped and rebuilt from the chunks.
  {
    sqlite3* raw = nullptr;
    REQUIRE(sqlite3_open(path.c_str(), &raw) == SQLITE_OK);
    REQUIRE(sqlite3_exec(raw, "UPDATE lod_sections SET generator_version = 99;", nullptr, nullptr,
                         nullptr) == SQLITE_OK);
    sqlite3_close(raw);
    ServerConfig config = Flat();
    config.store = OpenStore(path);
    Fixture f(config);
    CHECK(f.server.lod().sections().empty());
    CHECK(f.server.lod().pending() == 1);
    f.server.DrainLod();
    f.server.SaveNow();
    bool stale = true;
    CHECK(config.store->db().LodSections(kGeneratorFlat, stale).size() == saved.size());
    CHECK_FALSE(stale);
  }
}
