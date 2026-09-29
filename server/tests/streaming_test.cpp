// Terrain streaming (ARCHITECTURE.md §6.3, Phase 3b): the verification check, Generated vs.
// Explicit chunks, nearest-first order, the bandwidth budget, unloading, eviction, and the
// worldgen pool.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <monocypher-ed25519.h>

#include <cmath>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
#include "dwell/core/worldgen_pool.h"

using namespace dwell::core;
using namespace dwell::protocol;

namespace {

class CountingEntropy final : public Entropy {
 public:
  void Fill(std::span<std::uint8_t> out) override {
    for (auto& b : out) b = next_++;
  }

 private:
  std::uint8_t next_ = 1;
};

struct Client {
  std::array<std::uint8_t, 64> secret{};
  PublicKey public_key{};
  explicit Client(std::uint8_t seed_byte) {
    std::array<std::uint8_t, 32> seed;
    seed.fill(seed_byte);
    crypto_ed25519_key_pair(secret.data(), public_key.data(), seed.data());
  }
};

struct Fixture {
  JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  CountingEntropy entropy;
  Server server;
  TransportBinding binding{};

  explicit Fixture(ServerConfig config) : server(std::move(config), entropy, jobs) {}

  void Send(SessionId id, const Message& m) { server.OnReliable(id, Channel::kControl, Encode(m)); }

  // Messages to `id` since the last call (outbox drained), with their encoded sizes.
  std::vector<std::pair<Message, std::size_t>> Take(SessionId id) {
    std::vector<std::pair<Message, std::size_t>> out;
    for (auto& o : server.TakeOutbox()) {
      if (o.session != id || o.kind != Outgoing::Kind::kReliable) continue;
      auto m = Decode(o.bytes);
      REQUIRE(m.has_value());
      out.emplace_back(std::move(*m), o.bytes.size());
    }
    return out;
  }

  Welcome Join(SessionId id, const Client& who) {
    server.OnConnected(id, TransportKind::kWebTransport, binding);
    Send(id, ClientHello{kProtocolVersion, "test", who.public_key, "Tester"});
    const auto nonce = std::get<Challenge>(Take(id).at(0).first).nonce;
    const auto transcript = AuthTranscript(nonce, binding, who.public_key);
    Signature sig{};
    crypto_ed25519_sign(sig.data(), who.secret.data(), transcript.data(), transcript.size());
    Send(id, ClientAuth{sig});
    return std::get<Welcome>(Take(id).at(0).first);
  }

  // Chunk messages over `ticks` ticks.
  std::vector<ChunkData> Chunks(SessionId id, int ticks,
                                std::vector<ChunkUnload>* unloads = nullptr) {
    std::vector<ChunkData> out;
    for (int i = 0; i < ticks; ++i) {
      server.Step();
      for (auto& [m, size] : Take(id)) {
        if (auto* c = std::get_if<ChunkData>(&m)) out.push_back(std::move(*c));
        if (auto* u = std::get_if<ChunkUnload>(&m); u && unloads) unloads->push_back(*u);
      }
    }
    return out;
  }

  void MoveTo(std::uint16_t player_id, float x, float feet_y, float z) {
    const auto h = *server.PlayerHandleOf(player_id);
    auto& players = server.players();
    players.Teleport(h, JPH::Vec3(x, feet_y + players.HalfHeight(h), z), JPH::Vec3::sZero());
  }
};

ServerConfig Flat() {
  return {.generator_version = kGeneratorFlat, .spawn = std::array<float, 3>{0.5f, 0.0f, 0.5f}};
}

ChunkCoord CoordOf(const ChunkData& m) { return {m.coord[0], m.coord[1], m.coord[2]}; }

}  // namespace

TEST_CASE("streaming: nothing is sent before the client's worldgen check") {
  Fixture f(Flat());
  f.Join(1, Client(1));
  CHECK(f.Chunks(1, 30).empty());
}

TEST_CASE("streaming: a matching verification hash gets Generated chunks, nearest first") {
  Fixture f(ServerConfig{.world_seed = 99});
  const Welcome welcome = f.Join(1, Client(1));
  // The client generates the verification chunk itself and reports its hash.
  Chunk verification;
  const ChunkCoord vc{welcome.verification_chunk[0], welcome.verification_chunk[1],
                      welcome.verification_chunk[2]};
  GeneratorFor(welcome.generator_version, welcome.world_seed)(vc, verification);
  f.Send(1, WorldgenCheck{ChunkHash(verification)});

  const auto chunks = f.Chunks(1, 60);
  REQUIRE(!chunks.empty());
  const auto feet = SpawnPointFor(kGeneratorTerrain, 99);
  const ChunkCoord spawn = ChunkOf(static_cast<std::int32_t>(std::floor(feet[0])),
                                   static_cast<std::int32_t>(std::floor(feet[1] + 0.9f)),
                                   static_cast<std::int32_t>(std::floor(feet[2])));
  CHECK(CoordOf(chunks[0]) == spawn);
  int previous = 0;
  bool ordered = true;
  for (const ChunkData& c : chunks) {
    CHECK(c.form == ChunkForm::kGenerated);
    const int dx = c.coord[0] - spawn.x, dy = c.coord[1] - spawn.y, dz = c.coord[2] - spawn.z;
    const int d = dx * dx + dy * dy + dz * dz;
    ordered = ordered && d >= previous;
    previous = d;
  }
  CHECK(ordered);
  // The whole view: a cylinder of kViewRadiusChunks, kViewHeightChunks up and down.
  int expected = 0;
  for (int y = -kViewHeightChunks; y <= kViewHeightChunks; ++y)
    for (int z = -kViewRadiusChunks; z <= kViewRadiusChunks; ++z)
      for (int x = -kViewRadiusChunks; x <= kViewRadiusChunks; ++x)
        if (x * x + z * z <= kViewRadiusChunks * kViewRadiusChunks + kViewRadiusChunks &&
            spawn.y + y >= kMinChunkY && spawn.y + y <= kMaxChunkY)
          ++expected;
  CHECK(chunks.size() == static_cast<std::size_t>(expected));
  CHECK(f.server.StreamStatsOf(1)->streamed == static_cast<std::size_t>(expected));
  // Every chunk once.
  std::unordered_set<ChunkCoord, ChunkCoordHash> unique;
  for (const ChunkData& c : chunks) unique.insert(CoordOf(c));
  CHECK(unique.size() == chunks.size());
}

TEST_CASE("streaming: a mismatched hash (or 0) gets every chunk explicitly, as generated") {
  for (const std::uint64_t hash : {std::uint64_t{0}, std::uint64_t{12345}}) {
    CAPTURE(hash);
    Fixture f(ServerConfig{.world_seed = 7, .view_radius_chunks = 2, .view_height_chunks = 1});
    f.Join(1, Client(1));
    f.Send(1, WorldgenCheck{hash});
    const auto chunks = f.Chunks(1, 120);
    REQUIRE(chunks.size() > 20);
    const auto generate = GeneratorFor(kGeneratorTerrain, 7);
    for (const ChunkData& c : chunks) {
      REQUIRE(c.form == ChunkForm::kExplicit);
      CHECK(c.revision == 0);
      Chunk expected;
      generate(CoordOf(c), expected);
      CHECK(std::equal(c.voxels.begin(), c.voxels.end(), expected.voxels().begin()));
    }
  }
}

TEST_CASE("streaming: chunks per tick and bytes per tick stay within the budget") {
  ServerConfig config{.world_seed = 3, .chunk_bytes_per_second = 60 * 20000};
  Fixture f(config);
  f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});  // full mode: large explicit chunks
  std::size_t largest = 0;
  for (int tick = 0; tick < 120; ++tick) {
    f.server.Step();
    std::size_t bytes = 0, count = 0;
    for (auto& [m, size] : f.Take(1)) {
      if (!std::holds_alternative<ChunkData>(m)) continue;
      bytes += size;
      ++count;
      largest = std::max(largest, size);
    }
    CHECK(count <= static_cast<std::size_t>(kMaxChunksPerTick));
    // The budget may be overdrawn by the one chunk that crossed it.
    CHECK(bytes <= 20000 + largest);
  }
  CHECK(largest > 100);
}

TEST_CASE("streaming: moving away unloads far chunks, and memory stays bounded") {
  Fixture f(Flat());
  f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});
  std::vector<ChunkUnload> unloads;
  f.Chunks(1, 60, &unloads);
  CHECK(unloads.empty());
  const std::size_t full_view = f.server.StreamStatsOf(1)->streamed;

  // Walk (teleporting 1 m per tick) 20 chunks east.
  std::size_t max_world = 0, max_collision = 0, max_streamed = 0;
  for (int i = 0; i < 20 * dwell::core::kChunkSize; ++i) {
    f.MoveTo(1, 0.5f + static_cast<float>(i), 0.0f, 0.5f);
    f.Chunks(1, 1, &unloads);
    max_world = std::max(max_world, f.server.world().loaded_chunks());
    max_collision = std::max(max_collision, f.server.terrain().built_chunks());
    max_streamed = std::max(max_streamed, f.server.StreamStatsOf(1)->streamed);
  }
  f.Chunks(1, 120, &unloads);
  const auto stats = *f.server.StreamStatsOf(1);
  CHECK(!unloads.empty());
  CHECK(stats.unloaded > 0);
  // What the client holds stays near one view: the view plus what is still within the unload
  // margin behind the player.
  CHECK(stats.streamed >= full_view);
  CHECK(max_streamed < full_view * 3 / 2);
  // The server holds the prefetch region around the player plus not-yet-evicted ones, not the
  // whole path (20 chunks × 5 × 5 × 5 would be thousands).
  CHECK(max_world < 600);
  CHECK(max_collision < 200);
  // Every unloaded chunk is outside the client's final view.
  for (const ChunkUnload& u : unloads) {
    for (const auto& c : u.coords) CHECK(c[0] < 20 - kViewRadiusChunks);
  }
}

TEST_CASE("streaming: chunks are generated ahead of a moving player, never on the tick") {
  // No threads, unlimited budget: the pool generates the prefetch region in Step, so the player's
  // collision never has to generate synchronously (walking streams without hitches).
  Fixture f(ServerConfig{.generator_version = kGeneratorFlat,
                         .spawn = std::array<float, 3>{0.5f, 0.0f, 0.5f},
                         .worldgen_budget_us = 1'000'000});
  f.Join(1, Client(1));
  const auto before = f.server.world().generated_on_access();
  for (int i = 0; i < 12 * dwell::core::kChunkSize; ++i) {
    f.MoveTo(1, 0.5f + static_cast<float>(i), 0.0f, 0.5f + static_cast<float>(i) * 0.4f);
    f.server.Step();
    f.server.TakeOutbox();
  }
  CHECK(f.server.world().generated_on_access() == before);
}

TEST_CASE("streaming: the worldgen pool generates on threads the same chunks") {
  const auto generate = GeneratorFor(kGeneratorTerrain, 5);
  WorldgenPool pool(generate, 3);
  std::vector<ChunkCoord> wanted;
  for (int i = 0; i < 24; ++i) wanted.push_back({i % 4, i / 8, (i / 4) % 2});
  pool.SetWanted(wanted);
  pool.SetWanted(wanted);  // chunks already queued, in flight, or done are not generated twice
  std::vector<WorldgenPool::Result> out;
  pool.Drain(out);
  REQUIRE(out.size() == wanted.size());
  for (auto& [coord, chunk] : out) {
    Chunk expected;
    generate(coord, expected);
    CHECK(ChunkHash(*chunk) == ChunkHash(expected));
    CHECK(chunk->revision() == 0);
  }
}

TEST_CASE("streaming: a server with worldgen threads streams to a full-mode client") {
  Fixture f(ServerConfig{
      .world_seed = 11, .worldgen_threads = 2, .view_radius_chunks = 2, .view_height_chunks = 1});
  f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});
  std::size_t count = 0;
  for (int i = 0; i < 600 && count < 30; ++i) count += f.Chunks(1, 1).size();
  CHECK(count >= 30);
}
