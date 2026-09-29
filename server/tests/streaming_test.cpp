// Terrain streaming (ARCHITECTURE.md §6.3, Phases 3b–3c): the verification check, Generated vs.
// Explicit vs. Air chunks, the spherical view, nearest-first order, the bandwidth budget,
// unloading, eviction, and the worldgen pool.
#include <doctest/doctest.h>

#include <cmath>

#include "dwell/core/worldgen_pool.h"
#include "dwell/player/net.h"
#include "server_fixture.h"

using namespace dwell::test;

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
                                   static_cast<std::int32_t>(std::floor(feet[1] + 0.9)),
                                   static_cast<std::int32_t>(std::floor(feet[2])));
  CHECK(CoordOf(chunks[0]) == spawn);
  int previous = 0;
  bool ordered = true;
  const auto air = AirTestFor(kGeneratorTerrain, 99);
  int air_chunks = 0;
  for (const ChunkData& c : chunks) {
    // Chunks the generator leaves empty travel as Air; the rest as Generated.
    CHECK(c.form == (air(CoordOf(c)) ? ChunkForm::kAir : ChunkForm::kGenerated));
    air_chunks += c.form == ChunkForm::kAir;
    const int dx = c.coord[0] - spawn.x, dy = c.coord[1] - spawn.y, dz = c.coord[2] - spawn.z;
    const int d = dx * dx + dy * dy + dz * dz;
    ordered = ordered && d >= previous;
    previous = d;
  }
  CHECK(ordered);
  CHECK(air_chunks > 0);  // the sky above the spawn
  // The whole view: a sphere of kViewRadiusChunks.
  constexpr int r = kViewRadiusChunks;
  int expected = 0;
  for (int y = -r; y <= r; ++y)
    for (int z = -r; z <= r; ++z)
      for (int x = -r; x <= r; ++x)
        if (x * x + y * y + z * z <= r * r + r && spawn.y + y >= kMinChunkY &&
            spawn.y + y <= kMaxChunkY)
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
    Fixture f(ServerConfig{.world_seed = 7, .view_radius_chunks = 2});
    f.Join(1, Client(1));
    f.Send(1, WorldgenCheck{hash});
    const auto chunks = f.Chunks(1, 120);
    REQUIRE(chunks.size() > 20);
    const auto generate = GeneratorFor(kGeneratorTerrain, 7);
    int explicit_chunks = 0;
    for (const ChunkData& c : chunks) {
      CHECK(c.revision == 0);
      Chunk expected;
      generate(CoordOf(c), expected);
      if (c.form == ChunkForm::kAir) {  // empty sky needs no payload in either mode
        for (const auto v : expected.voxels()) REQUIRE(v == Materials::kAir);
        continue;
      }
      REQUIRE(c.form == ChunkForm::kExplicit);
      ++explicit_chunks;
      CHECK(std::equal(c.voxels.begin(), c.voxels.end(), expected.voxels().begin()));
    }
    CHECK(explicit_chunks > 10);
  }
}

TEST_CASE("streaming: open sky costs nothing; the view is a sphere across the world's rows") {
  // A player high in the sky (the world is 256 chunk rows tall): the view is all Air chunks, and
  // the server neither generates nor stores them.
  Fixture f(ServerConfig{.world_seed = 99,
                         .spawn = std::array<double, 3>{0.5, 3000.0, 0.5},
                         .worldgen_budget_us = 1'000'000});
  f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});  // full mode: still no payload for air
  const std::size_t stored_before = f.server.world().loaded_chunks();
  const auto chunks = f.Chunks(1, 60);
  constexpr int r = kViewRadiusChunks;
  int sphere = 0;
  for (int y = -r; y <= r; ++y)
    for (int z = -r; z <= r; ++z)
      for (int x = -r; x <= r; ++x) sphere += x * x + y * y + z * z <= r * r + r;
  REQUIRE(chunks.size() == static_cast<std::size_t>(sphere));
  for (const ChunkData& c : chunks) CHECK(c.form == ChunkForm::kAir);
  CHECK(f.server.StreamStatsOf(1)->air_sent == static_cast<std::uint32_t>(sphere));
  // Falling through the sky generates nothing either (the player keeps falling for a while).
  CHECK(f.server.world().loaded_chunks() <= stored_before);
  CHECK(f.server.world().generated_on_access() == 0);
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
                         .spawn = std::array<double, 3>{0.5, 0.0, 0.5},
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
  Fixture f(ServerConfig{.world_seed = 11, .worldgen_threads = 2, .view_radius_chunks = 2});
  f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});
  std::size_t count = 0;
  for (int i = 0; i < 600 && count < 30; ++i) count += f.Chunks(1, 1).size();
  CHECK(count >= 30);
}

TEST_CASE("world rim: walking off the edge of the disc falls into the void and kills") {
  // The flat world near the rim, east of the origin: the last solid column is x = 8 191 999.
  constexpr double kSpawnX = dwell::core::kWorldRadius - 9.5;
  Fixture f(ServerConfig{.generator_version = kGeneratorFlat,
                         .spawn = std::array<double, 3>{kSpawnX, 0.0, 0.5},
                         .worldgen_budget_us = 1'000'000});
  const Welcome welcome = f.Join(1, Client(1));
  f.Send(1, WorldgenCheck{0});
  dwell::player::Input walk_east;
  walk_east.move_y = 1.0f;
  walk_east.look_yaw = 90.0f;  // +X
  std::uint32_t seq = 0;
  double last_x = 0, lowest_feet = 1e9;
  bool died = false;
  for (int tick = 0; tick < 60 * 60 && !died; ++tick) {
    PlayerInput input;
    input.inputs.push_back(dwell::player::QuantizeInput(walk_east, ++seq));
    f.server.OnDatagram(1, Encode(input));
    f.server.Step();
    f.server.TakeOutbox();
    if (const auto h = f.server.PlayerHandleOf(welcome.player_id)) {
      last_x = f.server.players().Position(*h).GetX();
      lowest_feet = std::min<double>(lowest_feet, f.server.players().Feet(*h));
    }
    died = f.server.HealthOf(welcome.player_id) == 0;
  }
  CHECK(died);
  CHECK(last_x > dwell::core::kWorldRadius);     // it went over the edge …
  CHECK(lowest_feet < dwell::core::kWorldMinY);  // … and fell past the bottom of the world
}
