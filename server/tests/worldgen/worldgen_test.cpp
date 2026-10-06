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
  TEST_CASE("the tallest ranges reach super tall peaks, rarely, and stay inside the world") {
    // Prototype relief (§6.1): massifs in the cores of the largest ranges rise to ~5.5 km.
    for (const std::uint64_t seed : {0ull, 42ull}) {
      CAPTURE(seed);
      const TerrainGenerator gen(seed);
      float highest = -1e9f;
      std::int32_t peak_x = 0, peak_z = 0;
      int land = 0, above_3km = 0;
      constexpr std::int32_t kStep = 8192, kHalf = 2'000'000;
      for (std::int32_t z = -kHalf; z <= kHalf; z += kStep)
        for (std::int32_t x = -kHalf; x <= kHalf; x += kStep) {
          const float h = gen.ColumnAt(x, z).height;
          if (h > highest) {
            highest = h;
            peak_x = x;
            peak_z = z;
          }
          if (h > 0) ++land;
          if (h > 3000) ++above_3km;
        }
      MESSAGE("highest sampled column " << highest << " m at (" << peak_x << ", " << peak_z << "); "
                                        << above_3km << " of " << land
                                        << " land samples above 3 km");
      CHECK(highest > 4500.0f);
      CHECK(highest < static_cast<float>(core::kWorldMaxY - 400));
      CHECK(above_3km < land / 50);  // super tall peaks are rare
    }
  }

  TEST_CASE("a chunk is a pure function of seed and coordinate") {
    const TerrainGenerator a(123), b(123), c(124);
    const ChunkCoord coord{1, 0, -1};  // at the surface (sea level is y = 0)
    CHECK(Generated(a, coord).voxels() == Generated(b, coord).voxels());
    CHECK(Generated(a, coord).voxels() != Generated(c, coord).voxels());
    CHECK(Generated(a, coord).revision() == 0);
    // Order of generation within a world does not matter.
    core::VoxelWorld w1(core::GeneratorFor(core::kGeneratorTerrain, 123));
    core::VoxelWorld w2(core::GeneratorFor(core::kGeneratorTerrain, 123));
    const ChunkCoord n{2, 0, -1};
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
    std::vector<ChunkCoord> coords = {{0, 0, 0}, {0, -1, 0}, {-1, -3, 3}};
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

  TEST_CASE("beyond the rim of the disc nothing is generated, not even bedrock") {
    const TerrainGenerator gen(5);
    // A chunk column straddling the rim on the diagonal (x = z ≈ 5 792 km).
    const ChunkCoord column{181019, 0, 181019};
    REQUIRE(core::ChunkDiscOverlap(column.x, column.z) == core::DiscOverlap::kPartial);
    const int bottom = core::kWorldMinY / S;
    int inside = 0, outside = 0;
    for (const int cy : {bottom, 0}) {
      const Chunk chunk = Generated(gen, {column.x, cy, column.z});
      for (int z = 0; z < S; ++z)
        for (int x = 0; x < S; ++x) {
          const bool in = core::InsideWorldDisc(column.x * S + x, column.z * S + z);
          (in ? inside : outside) += cy == bottom;
          for (int y = 0; y < S; ++y) {
            if (!in) {
              CHECK(chunk.Get(x, y, z) == M::kAir);
            } else if (cy == bottom && y < core::kBedrockLayers) {
              CHECK(chunk.Get(x, y, z) == M::kBedrock);
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
    CHECK_FALSE(gen.SolidAt(256001 * S, core::kWorldMinY, 0));
    CHECK(gen.SolidAt(255990 * S, core::kWorldMinY, 0));  // bedrock just inside the rim
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
           {std::pair{0, 0}, {5, -3}, {-14, -36}, {40, -12}, {249990, 10}, {-3, 249000}}) {
        const int surface = static_cast<int>(gen.ColumnAt(cx * S, cz * S).height) / S;
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
    // Flat and playground worlds: the same guarantee.
    for (const std::uint32_t version : {core::kGeneratorFlat, core::kGeneratorPlayground}) {
      const auto generate = core::GeneratorFor(version);
      const auto air = core::AirTestFor(version);
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

  // The material a voxel is made of: a slope or slab state reads as its plain block.
  MaterialId SurfaceMaterialOf(MaterialId m) {
    const std::string name(core::StateString(m));
    for (const char* suffix : {"_slope[", "_slab["}) {
      if (const auto at = name.find(suffix); at != std::string::npos) {
        return *core::ParseState(name.substr(0, at));
      }
    }
    return m;
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
          // The ground's top cell is the material or a slope or slab of it.
          CHECK(SurfaceMaterialOf(m) == expected);
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
        if (m == M::kCoalOre) CHECK(y <= 138);
        if (m == M::kIronOre) CHECK(y <= 9);
        if (m == M::kGoldOre) CHECK(y <= -47);
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
    // Surface, caves, deep rock, bedrock, sky and the top of the world, ocean, mountains; the
    // rim of the disc; terrain ~8,000 km out (its surface chunk found from the column); and a
    // super tall massif.
    constexpr int kSurface = 1 << 20;  // y placeholder: the chunk holding the column's surface
    std::vector<Case> cases = {
        {0, {0, 0, 0}},
        {0, {0, -1, 0}},
        {0, {0, -3, 0}},
        {0, {0, -6, 0}},
        {0, {0, -40, 0}},
        {0, {0, -64, 0}},
        {0, {0, 6, 0}},
        {0, {0, 191, 0}},
        {0, {-14, 3, -36}},
        {0, {-14, 2, -36}},
        {0, {40, -1, -12}},
        {0, {-7, -1, 19}},
        {20260925, {0, 0, 0}},
        {20260925, {5, -1, -3}},
        {20260925, {-2, -2, 9}},
        {0, {181019, -64, 181019}},
        {0, {249990, kSurface, 10}},
        {20260925, {-3, kSurface, 249000}},
        {0, {-249990, kSurface, -5}},
        {0, {3036, kSurface, 36828}},  // a massif's slopes, ~5.4 km up
    };
    std::vector<std::string> actual;
    for (auto& k : cases) {
      const TerrainGenerator gen(k.seed);
      if (k.c.y == kSurface) {
        k.c.y = worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(k.c.x * S, k.c.z * S).height), S);
      }
      std::ostringstream line;
      line << k.seed << ' ' << k.c.x << ' ' << k.c.y << ' ' << k.c.z << ' ' << std::hex
           << ChunkHash(Generated(gen, k.c));
      actual.push_back(line.str());
    }
    const std::string path = DWELL_WORLDGEN_GOLDEN;
    if (const char* update = std::getenv("DWELL_UPDATE_GOLDEN");
        update && std::string(update) == "1") {
      std::ofstream out(path);
      out << "# seed chunk_x chunk_y chunk_z fnv1a64(voxels) - generator version 5\n";
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
