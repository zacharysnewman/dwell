// Procedural terrain (ARCHITECTURE.md §6.3): noise, the generator pipeline, features, spawn, and
// the golden chunk hashes that pin native and WASM builds to bit-identical output (ADR 0010).
// Runs natively (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <cmath>
#include <cstdlib>
#include <fstream>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/bifacial.h"
#include "dwell/worldgen/noise.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;
using core::Chunk;
using core::ChunkCoord;
using core::MaterialId;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

constexpr int S = core::kChunkSize;

bool IsSolid(MaterialId m) { return m != M::kAir && m != M::kWater; }

Chunk Generated(const TerrainGenerator& gen, ChunkCoord c) {
  Chunk chunk;
  gen.Generate(c, chunk);
  return chunk;
}

// A chunk of the bifacial world (either face).
Chunk GeneratedWorld(const worldgen::BifacialTerrain& gen, ChunkCoord c) {
  Chunk chunk;
  gen.Generate(c, chunk);
  return chunk;
}

// Where the island's coast crosses the +x axis (the first sea column, 16 m steps from the origin).
std::pair<std::int32_t, std::int32_t> CoastOnAxis(const TerrainGenerator& gen) {
  std::int32_t x = 0;
  while (gen.ColumnAt(x, 0).height >= 0.0f) x += 16;
  return {x, 0};
}

}  // namespace

TEST_SUITE("worldgen: noise") {
  TEST_CASE("gradient noise is zero on the lattice, bounded, continuous and deterministic") {
    for (int i = -3; i <= 3; ++i) {
      CHECK(worldgen::Perlin2(7, static_cast<float>(i), static_cast<float>(2 * i)) == 0.0f);
      CHECK(worldgen::Perlin3(7, static_cast<float>(i), 1.0f, static_cast<float>(-i)) == 0.0f);
    }
    float lo = 0, hi = 0, max_step = 0;
    float prev = worldgen::Perlin3(3, -50.0f, 0.3f, 0.7f);
    for (int i = 1; i < 20000; ++i) {
      const float x = -50.0f + static_cast<float>(i) * 0.005f;
      const float v = worldgen::Perlin3(3, x, 0.3f, 0.7f);
      lo = std::min(lo, v);
      hi = std::max(hi, v);
      max_step = std::max(max_step, std::abs(v - prev));
      prev = v;
    }
    CHECK(lo > -1.1f);
    CHECK(hi < 1.1f);
    CHECK(hi - lo > 0.8f);    // it varies
    CHECK(max_step < 0.02f);  // and is continuous
    CHECK(worldgen::Perlin3(3, 1.25f, 2.5f, -3.75f) == worldgen::Perlin3(3, 1.25f, 2.5f, -3.75f));
    CHECK(worldgen::Perlin3(3, 1.25f, 2.5f, -3.75f) != worldgen::Perlin3(4, 1.25f, 2.5f, -3.75f));
  }

  TEST_CASE("noise is as detailed ~8,000 km from the origin as at it") {
    // Regression for ADR 0011. Generator version 2 converted world coordinates to float and
    // scaled them per octave; ~8,000 km out that loses most of a fine octave's lattice offset
    // (float coordinates, first loop: large errors against the exact split value). The generator's
    // split coordinates (integer lattice cell + float offset) are exact everywhere.
    constexpr std::int64_t kFar = 7999488;
    const auto float_error = [](std::int64_t origin) {
      float worst = 0.0f;
      for (int i = 0; i < 256; ++i) {
        const std::int64_t x = origin + i * 4;  // the 2D lattice's sample spacing
        // The finest hills octave: a 12 m lattice (96 m / 2³).
        const float by_float =
            worldgen::Perlin2(9, static_cast<float>(x) * (1.0f / 96.0f) * 8.0f, 0.3f);
        const float exact = worldgen::Perlin2(9, worldgen::Lattice(x, 96, 3), {0, 0.3f});
        worst = std::max(worst, std::abs(by_float - exact));
      }
      return worst;
    };
    CHECK(float_error(0) < 1e-3f);
    CHECK(float_error(kFar) > 0.05f);  // why the generator no longer does this
    for (const std::int64_t origin : {std::int64_t{0}, kFar, -kFar}) {
      CAPTURE(origin);
      int repeats = 0;
      float largest_step = 0.0f;
      for (int i = 0; i < 1024; ++i) {
        const float a = worldgen::Fbm2(9, origin + i, 17, 28, 3);
        const float b = worldgen::Fbm2(9, origin + i + 1, 17, 28, 3);
        repeats += a == b;
        largest_step = std::max(largest_step, std::abs(b - a));
      }
      CHECK(repeats == 0);
      CHECK(largest_step < 0.2f);  // smooth at 1 m steps
      // The lattice split is exact: cell and offset of x equal those of x's offset in its cell.
      const auto l = worldgen::Lattice(origin + 13, 28, 2);
      CHECK(l.frac ==
            worldgen::Lattice(worldgen::FloorMod(static_cast<std::int32_t>((origin + 13) % 28), 28),
                              28, 2)
                .frac);
    }
  }

  TEST_CASE("fractal sums stay in range") {
    for (int i = 0; i < 5000; ++i) {
      const std::int64_t x = i * 37, z = i * -21;
      const float f = worldgen::Fbm2(11, x, z, 100, 5);
      const float r = worldgen::Ridged2(11, x, z, 100, 5);
      CHECK(std::abs(f) <= 1.1f);
      CHECK(r >= 0.0f);
      CHECK(r <= 1.0f);
    }
  }
}

TEST_SUITE("worldgen: terrain") {
  TEST_CASE("heights: a low shore, land rising inland, a shallow sea floor") {
    const TerrainGenerator gen(0);
    const auto [cx, cz] = CoastOnAxis(gen);
    float highest = -1e9f, lowest = 1e9f;
    for (std::int32_t z = -30'000; z <= 30'000; z += 500)
      for (std::int32_t x = -30'000; x <= 30'000; x += 500) {
        const float h = gen.ColumnAt(x, z).height;
        highest = std::max(highest, h);
        lowest = std::min(lowest, h);
      }
    MESSAGE("heights " << lowest << " .. " << highest << " m; coast at x = " << cx);
    CHECK(highest > 60.0f);
    CHECK(highest < 200.0f);
    CHECK(lowest > -30.0f);
    CHECK(std::abs(gen.ColumnAt(cx, cz).height) < 6.0f);  // the shore is low
  }

  TEST_CASE("a chunk is a pure function of seed and coordinate") {
    const TerrainGenerator a(123), b(123), c(124);
    const ChunkCoord coord{1, 2, -1};  // at the surface near the origin (~75 m up)
    CHECK(Generated(a, coord).voxels() == Generated(b, coord).voxels());
    CHECK(Generated(a, coord).voxels() != Generated(c, coord).voxels());
    CHECK(Generated(a, coord).revision() == 0);
    // Order of generation within a world does not matter.
    core::VoxelWorld w1(core::GeneratorFor(core::kGeneratorTerrain, 123));
    core::VoxelWorld w2(core::GeneratorFor(core::kGeneratorTerrain, 123));
    const ChunkCoord n{2, 2, -1};
    w1.GetOrCreate(coord);
    w1.GetOrCreate(n);
    w2.GetOrCreate(n);
    w2.GetOrCreate(coord);
    CHECK(w1.Find(coord)->voxels() == w2.Find(coord)->voxels());
    CHECK(w1.Find(n)->voxels() == w2.Find(n)->voxels());
  }

  TEST_CASE("point queries agree with the chunk path voxel for voxel") {
    const TerrainGenerator gen(0);
    const auto [cx, cz] = CoastOnAxis(gen);
    std::vector<ChunkCoord> coords = {{0, 0, 0}, {0, 2, 0}, {-1, -3, 3}};
    for (const auto& [x, z] :
         {std::pair{cx, cz}, std::pair{cx + 600, cz}, std::pair{-9000, 4000}}) {
      coords.push_back(core::ChunkOf(x, static_cast<int>(gen.ColumnAt(x, z).height), z));
    }
    for (const ChunkCoord& c : coords) {
      const Chunk chunk = Generated(gen, c);
      int solid = 0, checked = 0;
      for (int z = 0; z < S; z += 3)
        for (int y = 0; y < S; y += 2)
          for (int x = 0; x < S; x += 3) {
            const bool expect = IsSolid(chunk.Get(x, y, z));
            const bool point = gen.SolidAt(c.x * S + x, c.y * S + y, c.z * S + z);
            ++checked;
            solid += expect;
            if (expect != point) {
              FAIL_CHECK("chunk (" << c.x << "," << c.y << "," << c.z << ") voxel " << x << "," << y
                                   << "," << z);
            }
          }
      MESSAGE("chunk " << c.x << "," << c.y << "," << c.z << ": " << solid << "/" << checked
                       << " solid");
    }
  }

  TEST_CASE("world bounds: stone floor (no bedrock), nothing below the face or at WORLD_MAX_Y") {
    const TerrainGenerator gen(5);
    const int bottom = core::kMidplaneY / S;  // the face-local chunk row at the midplane
    const Chunk floor = Generated(gen, {0, bottom, 0});
    for (int z = 0; z < S; ++z)
      for (int x = 0; x < S; ++x)
        for (int y = 0; y < 4; ++y) CHECK(floor.Get(x, y, z) == M::kStone);
    for (const int cy : {bottom - 1, core::kWorldMaxY / S}) {
      const Chunk empty = Generated(gen, {0, cy, 0});
      for (const MaterialId v : empty.voxels()) CHECK(v == M::kAir);
    }
  }

  TEST_CASE("beyond the rim of the disc nothing is generated") {
    const TerrainGenerator gen(5);
    // A chunk column straddling the rim on the diagonal (x = z ≈ 5 792 km).
    const ChunkCoord column{181019, 0, 181019};
    REQUIRE(core::ChunkDiscOverlap(column.x, column.z) == core::DiscOverlap::kPartial);
    const int bottom = core::kMidplaneY / S;
    int inside = 0, outside = 0;
    for (const int cy : {bottom, -1}) {
      const Chunk chunk = Generated(gen, {column.x, cy, column.z});
      for (int z = 0; z < S; ++z)
        for (int x = 0; x < S; ++x) {
          const bool in = core::InsideWorldDisc(column.x * S + x, column.z * S + z);
          (in ? inside : outside) += cy == bottom;
          for (int y = 0; y < S; ++y) {
            if (!in) {
              CHECK(chunk.Get(x, y, z) == M::kAir);
            } else if (cy == bottom && y < 4) {
              CHECK(chunk.Get(x, y, z) == M::kStone);
            }
          }
        }
    }
    CHECK(inside > 0);
    CHECK(outside > 0);
    // Wholly outside: all air at every height, and the air test knows it without generating.
    const auto air = core::AirTestFor(core::kGeneratorTerrain, 5);
    for (const int cy : {bottom, -1, 0, 3}) {
      const ChunkCoord c{256001, cy, 0};
      CHECK(core::ChunkDiscOverlap(c.x, c.z) == core::DiscOverlap::kOutside);
      const Chunk chunk = Generated(gen, c);
      for (const MaterialId v : chunk.voxels()) CHECK(v == M::kAir);
      CHECK(air(c));
      CHECK(gen.IsAirChunk(c));
    }
    // Point queries agree.
    CHECK(gen.ColumnAt(256001 * S, 0).outside);
    CHECK_FALSE(gen.SolidAt(256001 * S, core::kMidplaneY, 0));
    CHECK(gen.SolidAt(255990 * S, core::kMidplaneY, 0));  // rock just inside the rim
  }

  TEST_CASE("chunks the air test reports as air generate as all air, and sky is skipped") {
    // Every chunk the air test calls air must be generated as all air (streaming never sends it);
    // above the terrain most chunks are air.
    for (const std::uint64_t seed : {0ull, 20260925ull}) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      const auto air = core::AirTestFor(core::kGeneratorTerrain, seed);
      int sky = 0, checked = 0;
      for (const auto& [cx, cz] :
           {std::pair{0, 0}, {5, -3}, {-14, -36}, {700, -12}, {249990, 10}, {-3, 249000}}) {
        const int surface =
            worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(cx * S, cz * S).height), S);
        for (int cy = surface - 2; cy <= surface + 6; ++cy) {
          const ChunkCoord c{cx, cy, cz};
          CAPTURE(c.x);
          CAPTURE(c.y);
          CAPTURE(c.z);
          const bool is_air = air(c);
          CHECK(is_air == gen.IsAirChunk(c));
          ++checked;
          if (!is_air) continue;
          ++sky;
          const Chunk chunk = Generated(gen, c);
          for (const MaterialId v : chunk.voxels()) REQUIRE(v == M::kAir);
        }
      }
      CHECK(sky > checked / 4);
    }
    // The flat world: the same guarantee.
    const auto generate = core::GeneratorFor(core::kGeneratorFlat);
    const auto air = core::AirTestFor(core::kGeneratorFlat);
    for (int cy = -3; cy <= 2; ++cy)
      for (int cx = -2; cx <= 1; ++cx) {
        const ChunkCoord c{cx, cy, 0};
        if (!air(c)) continue;
        Chunk chunk;
        generate(c, chunk);
        for (const MaterialId v : chunk.voxels()) REQUIRE(v == M::kAir);
      }
    CHECK(air({0, 1, 0}));
    CHECK_FALSE(air({0, -1, 0}));
  }

  TEST_CASE(
      "the fill: grass over dirt inland, sand at the shore and under water, sea to its level") {
    const TerrainGenerator gen(0);
    const auto [cx, cz] = CoastOnAxis(gen);
    int water = 0, grass = 0, sand = 0;
    // Across the coast, from 200 m inland to 200 m out to sea.
    for (std::int32_t x = cx - 200; x <= cx + 200; x += 40) {
      const int h = static_cast<int>(gen.ColumnAt(x, cz).height);
      for (const int cy :
           {worldgen::FloorDiv(h, S) - 1, worldgen::FloorDiv(h, S), worldgen::FloorDiv(h, S) + 1}) {
        const ChunkCoord c = core::ChunkOf(x, cy * S, cz);
        const Chunk chunk = Generated(gen, c);
        for (int z = 0; z < S; ++z)
          for (int lx = 0; lx < S; ++lx) {
            const auto ground = gen.GroundY(c.x * S + lx, c.z * S + z);
            REQUIRE(ground);
            for (int y = 0; y < S; ++y) {
              const int wy = c.y * S + y;
              const MaterialId m = chunk.Get(lx, y, z);
              const int depth = *ground - wy;
              if (depth < 0) {
                // Above the ground: water below sea level, air above it.
                CHECK(m == (wy < core::kSeaLevel ? M::kWater : M::kAir));
                water += m == M::kWater;
              } else if (*ground < core::kSeaLevel + 2) {
                CHECK(m == (depth < 3 ? M::kSand : M::kStone));  // the shore and the sea floor
                sand += m == M::kSand;
              } else {
                CHECK(m == (depth == 0 ? M::kGrass : depth <= 3 ? M::kDirt : M::kStone));
                grass += m == M::kGrass;
              }
            }
          }
      }
    }
    CHECK(water > 1000);
    CHECK(grass > 100);
    CHECK(sand > 100);
  }

  TEST_CASE("players spawn on level open ground near the origin") {
    // Level for 4 columns around the point: the server spreads up to eight players 1.5 m
    // either side of it, and each can place a block within reach without meeting a step (a block
    // placed beside a step can land inside its player, which the server refuses).
    for (std::uint64_t seed = 0; seed < 16; ++seed) {
      CAPTURE(seed);
      const auto s = core::SpawnPointFor(core::kGeneratorTerrain, seed);
      core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
      const int x = static_cast<int>(std::floor(s[0])), y = static_cast<int>(s[1]),
                z = static_cast<int>(std::floor(s[2]));
      CHECK(std::abs(x) < 512);
      CHECK(std::abs(z) < 512);
      CHECK(y > core::kSeaLevel);
      const int r = 4;  // 1.5 m of spread + reach to the block ahead, rounded up
      for (int dz = -r; dz <= r; ++dz)
        for (int dx = -r; dx <= r; ++dx) {
          CHECK(IsSolid(world.GetVoxel(x + dx, y - 1, z + dz)));
          CHECK(world.GetVoxel(x + dx, y, z + dz) == M::kAir);
          CHECK(world.GetVoxel(x + dx, y + 1, z + dz) == M::kAir);
        }
    }
    // The flat world keeps the flat spawn.
    CHECK(core::SpawnPointFor(core::kGeneratorFlat, 9)[1] == 0.0f);
  }
}

TEST_SUITE("worldgen: golden") {
  // Chunks across the pipeline: surface, rock, the face's floor, sky, sea, coast. The same hashes
  // must come out natively and under WASM (CI runs this suite in both); any change to the
  // generator's output needs a new generator version (ARCHITECTURE.md §6.3).
  TEST_CASE("generated chunks match the golden hashes") {
    struct Case {
      std::uint64_t seed;
      ChunkCoord c;
    };
    // Surface, deep rock, the face's floor, sky and the top of the world, the coast, the sea; the
    // rim of the disc; the sea ~8,000 km out (its floor's chunk found from the column).
    constexpr int kSurface = 1 << 20;  // y placeholder: the chunk holding the column's surface
    constexpr int kSurfaceB = kSurface + 1;  // … of face B's terrain (a mirrored chunk row)
    std::vector<Case> cases = {
        {0, {0, kSurface, 0}},
        {0, {0, 0, 0}},
        {0, {0, -1, 0}},
        {0, {0, -6, 0}},
        {0, {0, -64, 0}},
        {0, {0, 6, 0}},
        {0, {0, 191, 0}},
        {0, {-14, kSurface, -36}},
        {0, {40, kSurface, -12}},
        {20260925, {0, kSurface, 0}},
        {20260925, {5, -1, -3}},
        {20260925, {-300, kSurface, 450}},
        {0, {181019, -64, 181019}},
        {0, {249990, kSurface, 10}},
        {20260925, {-3, kSurface, 249000}},
        {0, {1500, kSurface, 0}},  // the open sea, ~48 km out
        {0, {1500, -1, 0}},
        // Face B (BIFACIAL_WORLD.md §5): the chunk rows either side of the midplane (no bedrock),
        // flipped rock, the mirror image of face A's sky, and surfaces of face B's own island.
        {0, {0, -65, 0}},
        {0, {0, -66, 0}},
        {0, {0, -129, 0}},
        {0, {0, -191, 0}},
        {0, {0, -320, 0}},
        {0, {0, kSurfaceB, 0}},
        {0, {-14, kSurfaceB, -36}},
        {20260925, {5, kSurfaceB, -3}},
        {0, {181019, -65, 181019}},
        {0, {249990, kSurfaceB, 10}},
    };
    // The coast of each face's island where it crosses the +x axis, and just inland and offshore.
    for (const std::uint64_t seed : {std::uint64_t{0}, std::uint64_t{20260925}}) {
      for (const core::Face face : {core::Face::kA, core::Face::kB}) {
        const auto [x, z] = CoastOnAxis(worldgen::BifacialTerrain(seed).Local(face));
        for (const std::int32_t dx : {-96, 0, 96}) {
          cases.push_back(
              {seed,
               {worldgen::FloorDiv(x + dx, S), face == core::Face::kA ? kSurface : kSurfaceB,
                worldgen::FloorDiv(z, S)}});
        }
      }
    }
    std::vector<std::string> actual;
    for (auto& k : cases) {
      const worldgen::BifacialTerrain gen(k.seed);
      if (k.c.y == kSurface || k.c.y == kSurfaceB) {
        const bool b = k.c.y == kSurfaceB;
        const auto& local = gen.Local(b ? core::Face::kB : core::Face::kA);
        k.c.y =
            worldgen::FloorDiv(static_cast<int>(local.ColumnAt(k.c.x * S, k.c.z * S).height), S);
        if (b) k.c.y = core::MirrorChunkY(k.c.y);
      }
      std::ostringstream line;
      line << k.seed << ' ' << k.c.x << ' ' << k.c.y << ' ' << k.c.z << ' ' << std::hex
           << ChunkHash(GeneratedWorld(gen, k.c));
      actual.push_back(line.str());
    }
    const std::string path = DWELL_WORLDGEN_GOLDEN;
    if (const char* update = std::getenv("DWELL_UPDATE_GOLDEN");
        update && std::string(update) == "1") {
      std::ofstream out(path);
      out << "# seed chunk_x chunk_y chunk_z fnv1a64(voxels) - generator version 12\n";
      out << "# registry " << std::hex << core::kRegistryHash << '\n';
      for (const auto& line : actual) out << line << '\n';
      MESSAGE("golden hashes written to " << path);
      return;
    }
    std::ifstream in(path);
    REQUIRE_MESSAGE(in.good(), "missing " << path << "; run with DWELL_UPDATE_GOLDEN=1");
    std::vector<std::string> expected;
    std::string registry;  // the block registry hash the golden was written with
    for (std::string line; std::getline(in, line);) {
      if (line.starts_with("# registry ")) registry = line.substr(11);
      if (!line.empty() && line[0] != '#') expected.push_back(line);
    }
    std::ostringstream now;
    now << std::hex << core::kRegistryHash;
    CHECK_MESSAGE(registry == now.str(),
                  "the block registry changed (golden "
                      << registry << ", now " << now.str()
                      << "): ids moved; regenerate with DWELL_UPDATE_GOLDEN=1");
    REQUIRE(expected.size() == actual.size());
    for (std::size_t i = 0; i < expected.size(); ++i) CHECK(actual[i] == expected[i]);
  }
}
