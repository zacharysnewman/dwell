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

#include "dwell/core/voxel.h"
#include "dwell/worldgen/noise.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;
using core::Chunk;
using core::ChunkCoord;
using core::MaterialId;
using worldgen::Biome;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

constexpr int S = core::kChunkSize;

bool IsSolid(MaterialId m) { return m != M::kAir && m != M::kWater; }

Chunk Generated(const TerrainGenerator& gen, ChunkCoord c,
                std::uint8_t stages = TerrainGenerator::kAllStages) {
  Chunk chunk;
  gen.Generate(c, chunk, stages);
  return chunk;
}

// FNV-1a over the voxel ids (little-endian u16).
std::uint64_t HashChunk(const Chunk& chunk) {
  std::uint64_t h = 0xcbf29ce484222325ull;
  for (const MaterialId m : chunk.voxels()) {
    for (int b = 0; b < 2; ++b) {
      h ^= static_cast<std::uint8_t>(m >> (8 * b));
      h *= 0x100000001b3ull;
    }
  }
  return h;
}

// A point of each biome near the origin (seed 0), found on a coarse grid.
std::optional<std::pair<int, int>> FindBiome(const TerrainGenerator& gen, Biome biome) {
  for (int r = 0; r < 4000; r += 48)
    for (int x = -r; x <= r; x += 48)
      for (const int z : {-r, r}) {
        if (gen.ColumnAt(x, z).biome == biome) return std::pair{x, z};
        if (gen.ColumnAt(z, x).biome == biome) return std::pair{z, x};
      }
  return std::nullopt;
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

  TEST_CASE("fractal sums stay in range") {
    for (int i = 0; i < 5000; ++i) {
      const float x = static_cast<float>(i) * 0.37f, z = static_cast<float>(i) * -0.21f;
      const float f = worldgen::Fbm2(11, x, z, 5);
      const float r = worldgen::Ridged2(11, x, z, 5);
      CHECK(std::abs(f) <= 1.1f);
      CHECK(r >= 0.0f);
      CHECK(r <= 1.0f);
    }
  }
}

TEST_SUITE("worldgen: terrain") {
  TEST_CASE("a chunk is a pure function of seed and coordinate") {
    const TerrainGenerator a(123), b(123), c(124);
    const ChunkCoord coord{1, 2, -1};
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
    const auto mountain = FindBiome(gen, Biome::kMountains);
    REQUIRE(mountain);
    std::vector<ChunkCoord> coords = {{0, 2, 0}, {0, 1, 0}, {-1, -1, 3}};
    const auto m = *mountain;
    const auto h = static_cast<int>(gen.ColumnAt(m.first, m.second).height);
    coords.push_back(core::ChunkOf(m.first, h, m.second));
    for (const ChunkCoord& c : coords) {
      const Chunk chunk = Generated(gen, c, 0);  // terrain stages only
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

  TEST_CASE("world bounds: bedrock floor, void below, nothing at or above WORLD_MAX_Y") {
    const TerrainGenerator gen(5);
    const int bottom = core::kWorldMinY / S;  // chunk y whose first layer is WORLD_MIN_Y
    const Chunk floor = Generated(gen, {0, bottom, 0});
    for (int z = 0; z < S; ++z)
      for (int x = 0; x < S; ++x)
        for (int y = 0; y < core::kBedrockLayers; ++y) CHECK(floor.Get(x, y, z) == M::kBedrock);
    for (const int cy : {bottom - 1, core::kWorldMaxY / S}) {
      const Chunk empty = Generated(gen, {0, cy, 0});
      for (const MaterialId v : empty.voxels()) CHECK(v == M::kAir);
    }
  }

  TEST_CASE("water only below sea level; small floating pieces are removed") {
    const TerrainGenerator gen(0);
    const auto ocean = FindBiome(gen, Biome::kOcean);
    const auto mountain = FindBiome(gen, Biome::kMountains);
    REQUIRE(ocean);
    REQUIRE(mountain);
    int water = 0;
    for (const auto& [px, pz] : {*ocean, *mountain}) {
      const int h = static_cast<int>(gen.ColumnAt(px, pz).height);
      for (int dy = -1; dy <= 1; ++dy) {
        const ChunkCoord c = core::ChunkOf(px, h + dy * S, pz);
        const Chunk chunk = Generated(gen, c, TerrainGenerator::kStageStability);
        for (int z = 0; z < S; ++z)
          for (int y = 0; y < S; ++y)
            for (int x = 0; x < S; ++x)
              if (chunk.Get(x, y, z) == M::kWater) {
                ++water;
                CHECK(c.y * S + y < core::kSeaLevel);
              }
        // Every solid component touches a chunk face or has at least 48 voxels.
        std::vector<std::uint8_t> seen(core::kChunkVolume, 0);
        for (int start = 0; start < core::kChunkVolume; ++start) {
          if (seen[start] || !IsSolid(chunk.voxels()[start])) continue;
          std::vector<int> stack{start};
          seen[start] = 1;
          int size = 0;
          bool face = false;
          while (!stack.empty()) {
            const int i = stack.back();
            stack.pop_back();
            ++size;
            const int x = i & 31, y = (i >> 5) & 31, z = i >> 10;
            face |= x == 0 || y == 0 || z == 0 || x == S - 1 || y == S - 1 || z == S - 1;
            for (const int d : {1, -1, S, -S, S * S, -S * S}) {
              const int j = i + d;
              const int jx = j & 31, jy = (j >> 5) & 31, jz = j >> 10;
              if (j < 0 || j >= core::kChunkVolume ||
                  std::abs(jx - x) + std::abs(jy - y) + std::abs(jz - z) != 1) {
                continue;
              }
              if (!seen[j] && IsSolid(chunk.voxels()[j])) {
                seen[j] = 1;
                stack.push_back(j);
              }
            }
          }
          CHECK((face || size >= 48));
        }
      }
    }
    CHECK(water > 200);  // the ocean chunks hold sea
  }

  TEST_CASE("the biomes all occur, with surface materials to match") {
    const TerrainGenerator gen(0);
    for (const Biome b : {Biome::kOcean, Biome::kBeach, Biome::kPlains, Biome::kForest,
                          Biome::kDesert, Biome::kSnowy, Biome::kMountains}) {
      CAPTURE(worldgen::BiomeName(b));
      CHECK(FindBiome(gen, b).has_value());
    }
    // The top of level plains ground is grass over dirt; desert ground is sand.
    core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, 0));
    const auto top = [&](Biome b, MaterialId expected) {
      const auto p = FindBiome(gen, b);
      REQUIRE(p);
      int found = 0;
      for (int d = 0; d < 64 && found < 5; ++d) {
        const int x = p->first + d, z = p->second;
        if (gen.ColumnAt(x, z).biome != b) continue;
        if (const auto g = gen.GroundY(x, z)) {
          const MaterialId m = world.GetVoxel(x, *g, z);
          if (m == M::kLog || m == M::kLeaves || m == M::kStone)
            continue;  // trees, boulders, cliffs
          CHECK(m == expected);
          ++found;
        }
      }
      CHECK(found > 0);
    };
    top(Biome::kPlains, M::kGrass);
    top(Biome::kDesert, M::kSand);
    top(Biome::kSnowy, M::kSnow);
  }

  TEST_CASE("trees stand on the ground, and cross chunk borders intact") {
    const TerrainGenerator gen(0);
    const auto forest = FindBiome(gen, Biome::kForest);
    REQUIRE(forest);
    core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, 0));
    const int cx0 = worldgen::FloorDiv(forest->first, TerrainGenerator::kTreeCell);
    const int cz0 = worldgen::FloorDiv(forest->second, TerrainGenerator::kTreeCell);
    int trees = 0, crossing = 0;
    for (int cz = cz0 - 15; cz <= cz0 + 15; ++cz)
      for (int cx = cx0 - 15; cx <= cx0 + 15; ++cx) {
        const auto t = gen.TreeInCell(cx, cz);
        if (!t) continue;
        ++trees;
        CHECK(IsSolid(world.GetVoxel(t->x, t->y - 1, t->z)));
        for (int y = t->y; y < t->y + t->size; ++y) CHECK(world.GetVoxel(t->x, y, t->z) == M::kLog);
        // Leaves around the upper trunk (oak crowns, spruce cones), including in the neighbouring
        // chunk.
        const int top = t->y + t->size - 1;
        int leaves = 0;
        for (int y = top - 2; y <= top; ++y)
          for (int dz = -1; dz <= 1; ++dz)
            for (int dx = -1; dx <= 1; ++dx)
              leaves += world.GetVoxel(t->x + dx, y, t->z + dz) == M::kLeaves;
        CHECK(leaves >= 8);
        const int lx = worldgen::FloorMod(t->x, S);
        if (lx == 0 || lx == S - 1) ++crossing;
      }
    CHECK(trees > 20);
    CHECK(crossing > 0);  // their leaves (checked above) reach into the next chunk
    MESSAGE(trees << " trees, " << crossing << " on a chunk border");
  }

  TEST_CASE("ores are embedded in stone within their depth ranges") {
    const TerrainGenerator gen(0);
    int ores = 0;
    for (int cy = -3; cy <= 1; ++cy) {
      const Chunk chunk = Generated(gen, {3, cy, -2});
      for (int i = 0; i < core::kChunkVolume; ++i) {
        const MaterialId m = chunk.voxels()[i];
        const int y = cy * S + ((i >> 5) & 31);
        if (m == M::kCoalOre) CHECK(y <= 202);
        if (m == M::kIronOre) CHECK(y <= 73);
        if (m == M::kGoldOre) CHECK(y <= 17);
        ores += m == M::kCoalOre || m == M::kIronOre || m == M::kGoldOre;
      }
    }
    CHECK(ores > 100);
  }

  TEST_CASE("players spawn on level open ground near the origin") {
    for (const std::uint64_t seed : {0ull, 1ull, 42ull}) {
      CAPTURE(seed);
      const auto s = core::SpawnPointFor(core::kGeneratorTerrain, seed);
      core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
      const int x = static_cast<int>(std::floor(s[0])), y = static_cast<int>(s[1]),
                z = static_cast<int>(std::floor(s[2]));
      CHECK(std::abs(x) < 512);
      CHECK(std::abs(z) < 512);
      CHECK(y > core::kSeaLevel);
      for (int dz = -2; dz <= 2; ++dz)
        for (int dx = -2; dx <= 2; ++dx) {
          CHECK(IsSolid(world.GetVoxel(x + dx, y - 1, z + dz)));
          CHECK(world.GetVoxel(x + dx, y, z + dz) == M::kAir);
          CHECK(world.GetVoxel(x + dx, y + 1, z + dz) == M::kAir);
        }
    }
    // Other generators keep the flat spawn.
    CHECK(core::SpawnPointFor(core::kGeneratorPlayground, 9)[1] == 0.0f);
  }
}

TEST_SUITE("worldgen: golden") {
  // Chunks across the pipeline: surface, caves, bedrock, sky, ocean, mountain. The same hashes
  // must come out natively and under WASM (CI runs this suite in both); any change to the
  // generator's output needs a new generator version (ARCHITECTURE.md §6.3).
  TEST_CASE("generated chunks match the golden hashes") {
    struct Case {
      std::uint64_t seed;
      ChunkCoord c;
    };
    const std::vector<Case> cases = {
        {0, {0, 2, 0}},   {0, {0, 1, 0}},        {0, {0, -1, 0}},        {0, {0, -4, 0}},
        {0, {0, 8, 0}},   {0, {-14, 5, -36}},    {0, {-14, 4, -36}},     {0, {40, 1, -12}},
        {0, {-7, 2, 19}}, {20260925, {0, 2, 0}}, {20260925, {5, 1, -3}}, {20260925, {-2, 0, 9}},
    };
    std::vector<std::string> actual;
    for (const auto& k : cases) {
      const TerrainGenerator gen(k.seed);
      std::ostringstream line;
      line << k.seed << ' ' << k.c.x << ' ' << k.c.y << ' ' << k.c.z << ' ' << std::hex
           << HashChunk(Generated(gen, k.c));
      actual.push_back(line.str());
    }
    const std::string path = DWELL_WORLDGEN_GOLDEN;
    if (const char* update = std::getenv("DWELL_UPDATE_GOLDEN");
        update && std::string(update) == "1") {
      std::ofstream out(path);
      out << "# seed chunk_x chunk_y chunk_z fnv1a64(voxels) - generator version 2\n";
      for (const auto& line : actual) out << line << '\n';
      MESSAGE("golden hashes written to " << path);
      return;
    }
    std::ifstream in(path);
    REQUIRE_MESSAGE(in.good(), "missing " << path << "; run with DWELL_UPDATE_GOLDEN=1");
    std::vector<std::string> expected;
    for (std::string line; std::getline(in, line);) {
      if (!line.empty() && line[0] != '#') expected.push_back(line);
    }
    REQUIRE(expected.size() == actual.size());
    for (std::size_t i = 0; i < expected.size(); ++i) CHECK(actual[i] == expected[i]);
  }
}
