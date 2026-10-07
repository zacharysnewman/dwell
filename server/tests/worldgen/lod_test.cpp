// Level of detail (ARCHITECTURE.md §6.6, ADR 0012): the section grid, Downsample, GenerateLod and
// its golden hashes, which pin native and WASM builds to bit-identical output. Runs natively
// (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <optional>
#include <sstream>
#include <string>
#include <tuple>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/protocol/messages.h"
#include "dwell/worldgen/terrain.h"

#include "biome_search.h"
#include "water_search.h"

using namespace dwell;
using core::Chunk;
using core::ChunkCoord;
using core::LodCell;
using core::LodCells;
using core::LodCoord;
using core::LodKind;
using core::MaterialId;
using testing::FindBiome;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

constexpr int N = core::kLodSectionCells;

// A level-L section built bottom-up from generated chunks: level 1 downsamples 8 chunks, level 2
// 8 of those, and so on (what the server's propagation computes for modified sections).
LodCells DownsampledFromChunks(const core::ChunkGenerator& gen, const LodCoord& c) {
  LodCells out(core::kLodVolume, M::kAir);
  for (int o = 0; o < 8; ++o) {
    const LodCoord child = core::LodChild(c, o);
    if (child.level == 0) {
      Chunk chunk;
      gen(core::ChunkOfLod(child), chunk);
      core::DownsampleChunkOctant(chunk, o, out);
    } else {
      core::DownsampleSectionOctant(DownsampledFromChunks(gen, child), o, out);
    }
  }
  return out;
}

// The section (at `level`) holding world point (x, y, z).
LodCoord SectionAt(int level, std::int64_t x, std::int64_t y, std::int64_t z) {
  const std::int64_t s = core::LodSectionSize(level);
  const auto div = [&](std::int64_t v, std::int64_t o) {
    const std::int64_t d = v - o;
    return static_cast<std::int32_t>(d >= 0 ? d / s : -((-d + s - 1) / s));
  };
  return {level, div(x, core::kLodOriginX), div(y, core::kLodOriginY), div(z, core::kLodOriginZ)};
}

int ClassOf(MaterialId m) { return m == M::kAir ? 0 : core::LodSolid(m) ? 2 : 1; }

struct Agreement {
  double class_match = 0;     // share of interior cells with the same class (air, liquid, solid)
  double surface_within = 0;  // share of columns whose top non-air cell is within one cell
  double surface_mean = 0;    // mean |difference| of the top non-air cell, in cells
};

Agreement Compare(const LodCells& a, const LodCells& b) {
  int same = 0, within = 0;
  double total = 0;
  for (int z = 0; z < N; ++z)
    for (int x = 0; x < N; ++x) {
      int ta = -1, tb = -1;
      for (int y = 0; y < N; ++y) {
        const auto i = static_cast<std::size_t>(LodCell(x, y, z));
        same += ClassOf(a[i]) == ClassOf(b[i]);
        if (a[i] != M::kAir) ta = y;
        if (b[i] != M::kAir) tb = y;
      }
      within += std::abs(ta - tb) <= 1;
      total += std::abs(ta - tb);
    }
  return {same / static_cast<double>(N * N * N), within / static_cast<double>(N * N),
          total / (N * N)};
}

}  // namespace

TEST_SUITE("lod: grid") {
  TEST_CASE("level 0 is the chunk grid; the root holds the whole disc") {
    for (const ChunkCoord c :
         {ChunkCoord{0, 0, 0}, ChunkCoord{-1, -64, 5}, ChunkCoord{255999, 191, -256000}}) {
      const LodCoord l = core::LodOfChunk(c);
      CHECK(core::ChunkOfLod(l) == c);
      const core::LodOrigin o = core::LodSectionOrigin(l);
      CHECK(o.x == std::int64_t{c.x} * 32);
      CHECK(o.y == std::int64_t{c.y} * 32);
      CHECK(o.z == std::int64_t{c.z} * 32);
    }
    const LodCoord root{core::kLodMaxLevel, 0, 0, 0};
    CHECK(core::LodInWorld(root));
    CHECK(core::LodSectionSize(core::kLodMaxLevel) == (std::int64_t{1} << 24));
    CHECK(core::LodSectionOrigin(root).x == -(std::int64_t{1} << 23));
    CHECK_FALSE(core::LodInWorld({core::kLodMaxLevel, 1, 0, 0}));
    // Every chunk's ancestor at the root level is the root.
    CHECK(core::LodAncestor(core::LodOfChunk({-256000, -64, 255999}), core::kLodMaxLevel) == root);
  }

  TEST_CASE("from level 8 a section spans the world's height: one row") {
    CHECK(core::LodRows(0) == 256);
    CHECK(core::LodRows(7) == 2);
    CHECK(core::LodRows(8) == 1);
    CHECK(core::LodRows(19) == 1);
    CHECK(core::LodInWorld({8, 1024, 0, 1024}));
    CHECK_FALSE(core::LodInWorld({8, 1024, 1, 1024}));
    CHECK_FALSE(core::LodInWorld({7, 2048, 2, 2048}));
  }

  TEST_CASE("children nest in their parent and cover it") {
    const LodCoord p{5, 1000, 3, 1001};
    const core::LodOrigin po = core::LodSectionOrigin(p);
    for (int o = 0; o < 8; ++o) {
      const LodCoord c = core::LodChild(p, o);
      CHECK(core::LodParent(c) == p);
      const core::LodOrigin co = core::LodSectionOrigin(c);
      const std::int64_t h = core::LodSectionSize(4);
      CHECK(co.x == po.x + (o & 1) * h);
      CHECK(co.y == po.y + ((o >> 1) & 1) * h);
      CHECK(co.z == po.z + ((o >> 2) & 1) * h);
    }
  }

  TEST_CASE("sections beyond the rim are outside the world") {
    // The disc's rim at +x: a level-10 section just past it.
    const LodCoord inside = SectionAt(10, 8'000'000, 0, 0);
    const LodCoord beyond = SectionAt(10, 8'230'000, 0, 0);
    CHECK(core::LodInWorld(inside));
    CHECK_FALSE(core::LodInWorld(beyond));
    // Near the diagonal, the square overlaps the disc only at its nearest corner.
    CHECK_FALSE(core::LodInWorld(SectionAt(10, 5'840'000, 0, 5'840'000)));
  }
}

TEST_SUITE("lod: downsample") {
  const auto block = [](std::function<MaterialId(int, int, int)> f) {
    MaterialId b[8];
    for (int i = 0; i < 8; ++i) b[i] = f(i & 1, (i >> 1) & 1, (i >> 2) & 1);
    return core::DownsampleBlock(b);
  };

  TEST_CASE("a one-voxel floor and wall survive a level; a thinner pillar does not") {
    // Floor: the bottom layer, or the top layer, of the block.
    CHECK(block([](int, int y, int) { return y == 0 ? M::kStone : M::kAir; }) == M::kStone);
    CHECK(block([](int, int y, int) { return y == 1 ? M::kStone : M::kAir; }) == M::kStone);
    // Wall: one x slice, either one.
    CHECK(block([](int x, int, int) { return x == 0 ? M::kDirt : M::kAir; }) == M::kDirt);
    CHECK(block([](int x, int, int) { return x == 1 ? M::kDirt : M::kAir; }) == M::kDirt);
    // A 1×1 pillar: 2 of 8.
    CHECK(block([](int x, int, int z) { return x == 0 && z == 0 ? M::kLog : M::kAir; }) == M::kAir);
    // Three of eight is air too.
    CHECK(block([](int x, int y, int z) {
            return x + y + z <= 1 && !(x == 0 && y == 0 && z == 0) ? M::kStone : M::kAir;
          }) == M::kAir);
  }

  TEST_CASE("surface materials win: the top of each column, ties to the upper cells") {
    // Grass over dirt: 4 against 4, the top layer wins.
    CHECK(block([](int, int y, int) { return y == 1 ? M::kGrass : M::kDirt; }) == M::kGrass);
    // Three columns of stone to one of sand on top: the columns' tops count, so stone.
    CHECK(block([](int x, int y, int z) {
            return y == 1 && x == 0 && z == 0 ? M::kSand : y == 1 ? M::kAir : M::kStone;
          }) == M::kStone);
    // A floor whose top half is empty: the lower cells are the columns' tops.
    CHECK(block([](int x, int y, int) {
            return y == 1 ? M::kAir : x == 0 ? M::kSand : M::kGravel;
          }) == M::kSand);
  }

  TEST_CASE("liquids count as filled: a sea keeps its surface, a floor under water hides") {
    // Water over sand: the water on top is what shows from above.
    CHECK(block([](int, int y, int) { return y == 1 ? M::kWater : M::kSand; }) == M::kWater);
    CHECK(block([](int x, int y, int z) {
            return y == 0 && x == 0 && z == 0 ? M::kSand : M::kWater;
          }) == M::kWater);
    // Two water and two sand cells, the rest air: filled, the top of each column counts.
    CHECK(block([](int x, int y, int) {
            return y == 1 ? M::kAir : x == 0 ? M::kWater : M::kSand;
          }) != M::kAir);
    CHECK(block([](int x, int y, int) { return y == 0 && x == 0 ? M::kWater : M::kAir; }) ==
          M::kAir);
  }

  TEST_CASE("a chunk downsamples into its octant of the level-1 section") {
    Chunk chunk;
    // A 1 m wall at x = 7 and a 1 m pillar at (20, ·, 20), in the chunk that is octant 5 (+x, +z).
    for (int y = 0; y < 32; ++y)
      for (int z = 0; z < 32; ++z) chunk.Set(7, y, z, M::kStone);
    for (int y = 0; y < 32; ++y) chunk.Set(20, y, 20, M::kLog);
    LodCells section(core::kLodVolume, M::kAir);
    core::DownsampleChunkOctant(chunk, 5, section);
    CHECK(section[LodCell(16 + 3, 0, 16 + 4)] == M::kStone);  // x 7 → cell 3 of the octant
    CHECK(section[LodCell(16 + 3, 15, 16 + 15)] == M::kStone);
    CHECK(section[LodCell(16 + 10, 5, 16 + 10)] == M::kAir);  // the pillar is gone
    CHECK(section[LodCell(3, 0, 4)] == M::kAir);              // other octants untouched
  }
}

TEST_SUITE("lod: generation") {
  TEST_CASE("the flat world's LOD equals the downsample of its chunks") {
    for (int level = 1; level <= 3; ++level) {
      const LodCoord c = SectionAt(level, 5, -1, -7);
      LodCells generated;
      REQUIRE(core::GenerateFlatLod(c, generated) == LodKind::kContent);
      const LodCells down = DownsampledFromChunks(core::GenerateFlatChunk, c);
      int differ = 0;
      for (int y = 0; y < N; ++y)
        for (int z = 0; z < N; ++z)
          for (int x = 0; x < N; ++x) {
            const auto i = static_cast<std::size_t>(LodCell(x, y, z));
            differ += generated[i] != down[i];
          }
      CHECK_MESSAGE(differ == 0, "level " << level);
    }
    // Sky above and rock below classify without content.
    LodCells cells;
    CHECK(core::GenerateFlatLod(SectionAt(2, 0, 200, 0), cells) == LodKind::kEmpty);
    CHECK(core::GenerateFlatLod(SectionAt(2, 0, -1000, 0), cells) == LodKind::kBuried);
    CHECK(core::GenerateFlatLod({1, 0, 0, 0}, cells) == LodKind::kEmpty);  // beyond the rim
  }

  TEST_CASE("GenerateLod is a pure function; bounds decide empty and buried sections exactly") {
    const TerrainGenerator gen(0);
    const auto ground = [&](std::int64_t x, std::int64_t z) {
      return static_cast<std::int64_t>(
          gen.ColumnAt(static_cast<std::int32_t>(x), static_cast<std::int32_t>(z)).height);
    };
    for (int level : {1, 3, 5}) {
      const LodCoord c = SectionAt(level, 100, ground(100, -40), -40);
      LodCells a, b;
      CHECK(gen.GenerateLod(c, a) == LodKind::kContent);
      CHECK(gen.GenerateLod(c, b) == LodKind::kContent);
      CHECK(a == b);
      const core::LodBounds bounds = gen.LodBoundsAt(level, c.i, c.k);
      for (int j = 0; j < core::LodRows(level); j += 1 + core::LodRows(level) / 16) {
        const LodCoord s{level, c.i, j, c.k};
        LodCells cells;
        const LodKind kind = gen.GenerateLod(s, cells);
        CHECK(kind == core::LodKindFromBounds(s, bounds));
        if (kind == LodKind::kEmpty) {
          CHECK(std::all_of(cells.begin(), cells.end(), [](MaterialId m) { return m == M::kAir; }));
        }
      }
      // Above the terrain's reach: empty; far below: buried.
      CHECK(core::LodKindFromBounds({level, c.i, core::LodRows(level) - 1, c.k}, bounds) ==
            (level < 8 ? LodKind::kEmpty : LodKind::kContent));
    }
    // A buried section generated in full would indeed be all solid.
    const LodCoord deep = SectionAt(2, 64, -1500, 64);
    LodCells cells;
    CHECK(gen.GenerateLod(deep, cells) == LodKind::kBuried);
  }

  TEST_CASE("a generated section agrees with the downsample of generated chunks") {
    // Tolerance (§6.6), at the spawn and a site of each biome, levels 1–2 (3 in the mountains):
    // at least 95% of the cells have the same class (air, liquid, solid) and at least 95% of the
    // columns' top non-air cell is within one cell; over all sites the mean surface difference
    // is under half a cell. Differences come from cave
    // air (LOD caves stop three cells below the surface), noise sampled at cell centres instead
    // of a 4 m lattice, and features smaller than a cell.
    const TerrainGenerator gen(0);
    const auto chunks = core::GeneratorFor(core::kGeneratorTerrain, 0);
    struct Site {
      std::string name;
      std::int32_t x, z;
    };
    std::vector<Site> sites = {{"spawn", 0, 0}};
    for (const worldgen::Biome biome :
         {worldgen::Biome::kBroadleaf, worldgen::Biome::kBareRock, worldgen::Biome::kOcean,
          worldgen::Biome::kBeach, worldgen::Biome::kDunes, worldgen::Biome::kSnowfield}) {
      if (const auto at = FindBiome(gen, biome)) {
        // A gentle spot of the biome near it: the first hit of a far region can lie on a cliff,
        // where a cell-centre sample and a downsample differ by design.
        std::pair<std::int32_t, std::int32_t> site = *at;
        for (int d = 0; d < 64 * 64; ++d) {
          const std::int32_t x = at->first + (d % 64) * 8 - 256,
                             z = at->second + (d / 64) * 8 - 256;
          const auto c = gen.ColumnAt(x, z);
          if (c.biome != biome || c.wet > 0.0f) continue;
          float steepest = 0.0f;
          for (const auto& [dx, dz] :
               {std::pair{8, 0}, std::pair{-8, 0}, std::pair{0, 8}, std::pair{0, -8}}) {
            steepest = std::max(steepest, std::abs(gen.ColumnAt(x + dx, z + dz).height - c.height));
          }
          if (steepest <= 4.0f) {
            site = {x, z};
            break;
          }
        }
        sites.push_back({worldgen::BiomeName(biome), site.first, site.second});
      }
    }
    // Water above sea level (Phase 11a): a lake, a river and a great river.
    {
      const auto wl = testing::FindWaterLandmarks(gen);
      REQUIRE(wl.found);
      sites.push_back({"lake", wl.lake.x, wl.lake.z});
      sites.push_back({"river", wl.river.x, wl.river.z});
      sites.push_back({"great river", wl.great.x, wl.great.z});
    }
    CHECK(sites.size() == 10);
    double mean_sum = 0;
    int count = 0;
    for (const Site& site : sites) {
      const auto column = gen.ColumnAt(site.x, site.z);
      const auto h =
          static_cast<std::int64_t>(std::max(column.height, static_cast<float>(column.water)));
      for (int level = 1; level <= (site.name == "mountains" ? 3 : 2); ++level) {
        // The section holding the surface there.
        const LodCoord c = SectionAt(level, site.x, std::max<std::int64_t>(h, -1), site.z);
        LodCells generated;
        gen.GenerateLod(c, generated);
        const Agreement a = Compare(generated, DownsampledFromChunks(chunks, c));
        MESSAGE(site.name << " level " << level << ": classes " << a.class_match
                          << ", surface within 1 cell " << a.surface_within << ", mean "
                          << a.surface_mean);
        CHECK(a.class_match >= 0.95);
        // Dense spruces (the snowy biome) have crowns narrower than a 2–4 m cell: a cell-centre
        // sample and a downsample round them differently in ~6–12 % of the columns.
        CHECK(a.surface_within >= (site.name == "snowy" ? 0.85 : 0.95));
        mean_sum += a.surface_mean;
        ++count;
      }
    }
    CHECK(mean_sum / count < 0.5);
  }
}

TEST_SUITE("lod: surface") {
  TEST_CASE(
      "coarse cells taller than the relief show the ground's surface, not the world's floor") {
    // Regression: from level ~12 a cell spans from the world's floor past the terrain, so its
    // bottom-voxel sample is bedrock; the column's top cell must still look like its surface.
    const TerrainGenerator gen(0);
    for (const int level : {12, 13, 15}) {
      CAPTURE(level);
      const LodCoord c = SectionAt(level, 0, 0, 0);
      LodCells cells;
      REQUIRE(gen.GenerateLod(c, cells) == LodKind::kContent);
      int tops = 0, bedrock = 0, grassy = 0;
      for (int z = 0; z < N; ++z)
        for (int x = 0; x < N; ++x) {
          for (int y = N - 1; y >= 0; --y) {
            const MaterialId m = cells[static_cast<std::size_t>(LodCell(x, y, z))];
            if (m == M::kAir || m == M::kWater) continue;
            ++tops;
            bedrock += m == M::kBedrock;
            grassy += m == M::kGrass || m == M::kSand || m == M::kSnow || m == M::kGravel ||
                      m == M::kLeaves;
            break;
          }
        }
      CHECK(tops > 0);
      CHECK(bedrock == 0);
      CHECK(grassy > tops / 2);
    }
  }
}

TEST_SUITE("lod: sea") {
  TEST_CASE("a sea shows its surface, not its floor, at levels deeper than the sea") {
    // Regression: at coarse levels a cell spans the sea floor and the sea; with liquids counted
    // apart from solids the floor won and oceans looked like dry land from afar.
    const TerrainGenerator gen(0);
    // The abyss: open ocean far from every coast, found on the layout.
    const auto landmarks = testing::FindLandmarks(gen);
    REQUIRE(landmarks.found);
    const std::optional<std::pair<std::int32_t, std::int32_t>> ocean = landmarks.abyss;
    REQUIRE(ocean);
    for (const int level : {6, 8, 10}) {
      CAPTURE(level);
      const LodCoord c = SectionAt(level, ocean->first, -1, ocean->second);
      LodCells cells;
      REQUIRE(gen.GenerateLod(c, cells) == LodKind::kContent);
      int tops = 0, water = 0;
      for (int z = 0; z < N; ++z)
        for (int x = 0; x < N; ++x) {
          if (level <= 7 && gen.ColumnAt(static_cast<std::int32_t>(core::LodSectionOrigin(c).x +
                                                                   x * core::LodCellSize(level)),
                                         static_cast<std::int32_t>(core::LodSectionOrigin(c).z +
                                                                   z * core::LodCellSize(level)))
                                    .height >= -30.0f) {
            continue;  // land, shore or shallows (coarse columns are smoothed)
          }
          for (int y = N - 1; y >= 0; --y) {
            const MaterialId m = cells[static_cast<std::size_t>(LodCell(x, y, z))];
            if (m == M::kAir) continue;
            ++tops;
            water += m == M::kWater;
            break;
          }
        }
      REQUIRE(tops > 0);
      CHECK(water >= tops * 9 / 10);
    }
  }
}

TEST_SUITE("lod: golden") {
  // GenerateLod sections across levels: the surface near the spawn, mountains, ocean, the rim,
  // terrain ~8,000 km out, a level-8 (index level) section and the root. The same hashes must come
  // out natively and under WASM; any change needs a new generator version (§6.3).
  TEST_CASE("generated LOD sections match the golden hashes") {
    struct Case {
      std::uint64_t seed;
      int level;
      std::int64_t x, y, z;  // a world point inside the section
    };
    std::vector<Case> cases = {
        {0, 1, 0, 0, 0},
        {0, 2, 0, -64, 0},
        {0, 3, -440, 0, -1150},
        {0, 4, 1280, -64, -380},
        {0, 6, 0, 0, 0},
        {0, 8, 0, 0, 0},
        {0, 10, 8'180'000, 0, 0},
        {0, 12, 0, 0, 0},
        {20260925, 1, 31, 0, -97},
        {20260925, 5, -7'990'000, 0, 5000},
        {0, core::kLodMaxLevel, 0, 0, 0},
        {0, 6, 97152, 5400, 1178496},  // a massif's peak
        {0, 9, 97152, 5400, 1178496},
    };
    // The plate layout (Phase 10): a coast, an ocean gap between two continents, an island, another
    // continent's interior and the abyss, at several levels.
    for (const std::uint64_t seed : {std::uint64_t{0}, std::uint64_t{20260925}}) {
      const auto lm = testing::FindLandmarks(TerrainGenerator(seed));
      REQUIRE(lm.found);
      const auto at = [](std::pair<std::int32_t, std::int32_t> p) { return p; };
      for (const int level : {1, 4, 8})
        cases.push_back({seed, level, at(lm.coast).first, 0, at(lm.coast).second});
      for (const int level : {6, 9})
        cases.push_back({seed, level, at(lm.gap).first, -400, at(lm.gap).second});
      for (const int level : {2, 5})
        cases.push_back({seed, level, at(lm.island).first, 0, at(lm.island).second});
      cases.push_back({seed, 3, at(lm.interior).first, 0, at(lm.interior).second});
      cases.push_back({seed, 7, at(lm.abyss).first, -900, at(lm.abyss).second});
      // Water above sea level (Phase 11a): where each lies, at the levels that still show it.
      const TerrainGenerator gen(seed);
      const auto wl = testing::FindWaterLandmarks(gen);
      REQUIRE(wl.found);
      const auto surface = [&](const testing::Point& p) {
        const auto col = gen.ColumnAt(p.x, p.z);
        return static_cast<std::int64_t>(
            std::max<float>(col.height, static_cast<float>(col.water)));
      };
      for (const int level : {1, 3})
        cases.push_back({seed, level, wl.lake.x, surface(wl.lake), wl.lake.z});
      for (const int level : {1, 2})
        cases.push_back({seed, level, wl.stream.x, surface(wl.stream), wl.stream.z});
      for (const int level : {1, 3})
        cases.push_back({seed, level, wl.river.x, surface(wl.river), wl.river.z});
      for (const int level : {2, 5})
        cases.push_back({seed, level, wl.great.x, surface(wl.great), wl.great.z});
      cases.push_back({seed, 1, wl.waterfall.x, surface(wl.waterfall), wl.waterfall.z});
      // Climate and vegetation (Phase 11c): alpine ground and snow, and forests, whose canopy
      // above the 4 m cells is leaves.
      for (const worldgen::Biome b :
           {worldgen::Biome::kAlpineMeadow, worldgen::Biome::kBareRock, worldgen::Biome::kBroadleaf,
            worldgen::Biome::kBlossomGrove, worldgen::Biome::kAutumnWoods}) {
        const auto site = FindBiome(gen, b);
        REQUIRE(site);
        const auto col = gen.ColumnAt(site->first, site->second);
        const auto top = static_cast<std::int64_t>(std::max<float>(col.height, 0.0f));
        cases.push_back({seed, 1, site->first, top, site->second});
        cases.push_back({seed, 4, site->first, top, site->second});
      }
    }
    std::vector<std::string> actual;
    for (const Case& k : cases) {
      const TerrainGenerator gen(k.seed);
      const LodCoord c = SectionAt(k.level, k.x, k.y, k.z);
      LodCells cells;
      const LodKind kind = gen.GenerateLod(c, cells);
      std::ostringstream line;
      line << k.seed << ' ' << c.level << ' ' << c.i << ' ' << c.j << ' ' << c.k << ' '
           << static_cast<int>(kind) << ' ' << std::hex << core::LodHash(kind, cells);
      actual.push_back(line.str());
    }
    const std::string path = DWELL_LOD_GOLDEN;
    if (const char* update = std::getenv("DWELL_UPDATE_GOLDEN");
        update && std::string(update) == "1") {
      std::ofstream out(path);
      out << "# seed level i j k kind fnv1a64(kind, cells) - generator version 9\n";
      out << "# registry " << std::hex << core::kRegistryHash << '\n';
      for (const auto& line : actual) out << line << '\n';
      MESSAGE("golden LOD hashes written to " << path);
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

TEST_SUITE("lod: encoding") {
  TEST_CASE("section content round-trips through the chunk palette + RLE codec") {
    const TerrainGenerator gen(0);
    for (int level : {1, 4, 9}) {
      const LodCoord c = SectionAt(level, 0, 0, 0);
      LodCells cells;
      gen.GenerateLod(c, cells);
      const auto bytes = protocol::EncodeLodCells(cells);
      const auto back = protocol::DecodeLodCells(bytes);
      REQUIRE(back);
      CHECK(*back == cells);
      CHECK(protocol::EncodeLodCells(*back) == bytes);  // canonical
      MESSAGE("level " << level << ": " << bytes.size() << " bytes");
      CHECK(bytes.size() < 16384);
    }
    // Truncated or oversized payloads are rejected.
    const auto bytes = protocol::EncodeLodCells(LodCells(core::kLodVolume, M::kStone));
    CHECK_FALSE(protocol::DecodeLodCells(std::span(bytes).first(bytes.size() - 1)));
    auto extra = bytes;
    extra.push_back(0);
    CHECK_FALSE(protocol::DecodeLodCells(extra));
  }
}

TEST_CASE("lod: column surfaces put distant land and seas at their true height") {
  // Regression (playtest: the horizon, oceans included, looked too tall): a cell counts as filled
  // from its bottom voxel, so the top cell's top lifts the surface by up to a cell — hundreds of
  // metres to kilometres far away, and seas to +2,048 m (level 12) and +6,144 m (level 13). The
  // column surfaces give the exact height to draw instead.
  const TerrainGenerator gen(5);
  for (int level = 1; level <= 13; ++level) {
    CAPTURE(level);
    const std::int64_t cell = core::LodCellSize(level);
    double error_cells = 0, error_surface = 0;
    int columns = 0, valid = 0;
    for (int s = 0; s < 6; ++s) {
      // The section holding the ground there (inland the ground is well above sea level).
      // On low-relief land (the coarse levels' mean relief of ranges lies ~100 m below a point's:
      // a known limit of dropping their octaves, not measured here).
      std::int32_t px = 4000 + s * 37000;
      const std::int32_t pz = 3000 + s * 23000;
      for (int k = 0; k < 400; ++k, px += 4000) {
        const auto probe = gen.ColumnAt(px, pz);
        if (!probe.outside && probe.coast > 0 && probe.height - probe.valley < 25.0f) break;
      }
      const LodCoord c = SectionAt(
          level, px,
          std::max<std::int64_t>(static_cast<std::int64_t>(gen.ColumnAt(px, pz).height), 0), pz);
      LodCells cells;
      core::LodSurfaces surface;
      if (gen.GenerateLod(c, cells, &surface) != LodKind::kContent) continue;
      REQUIRE(surface.size() == static_cast<std::size_t>(core::kLodPad * core::kLodPad));
      const auto o = core::LodSectionOrigin(c);
      for (int z = 0; z < N; ++z)
        for (int x = 0; x < N; ++x) {
          int top = -1;
          for (int y = N - 1; y >= 0; --y)
            if (cells[core::LodCell(x, y, z)] != core::Materials::kAir) {
              top = y;
              break;
            }
          if (top < 0 || top == N - 1) continue;  // the surface is in another section
          const double truth = gen.ColumnAt(static_cast<std::int32_t>(o.x + x * cell + cell / 2),
                                            static_cast<std::int32_t>(o.z + z * cell + cell / 2))
                                   .height;
          const auto& sf = surface[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)];
          ++columns;
          error_cells += static_cast<double>(o.y + (top + 1) * cell) - std::max(truth, 0.0);
          if (!sf.valid) continue;
          ++valid;
          // Wet exactly where the floor lies below the water — the sea's, or a river's or a
          // lake's above sea level, whose level the surface carries for the client to draw it at.
          if (sf.wet) {
            CHECK(sf.height < sf.water);
            CHECK(sf.water >= 0.0f);
          } else {
            CHECK(sf.height >= 0.0f);
          }
          error_surface += sf.height - truth;
        }
    }
    REQUIRE(columns > 0);
    MESSAGE("level " << level << ": cell tops " << error_cells / columns << " m above the surface, "
                     << "column surfaces " << (valid ? error_surface / valid : 0) << " m (" << valid
                     << "/" << columns << " columns)");
    // Levels 1–3 keep a whole cell where 3D noise raised the ground above the column's height
    // (overhangs, which grow with the mountains of a continent's interior).
    CHECK(valid >= columns * (level <= 2 ? 75 : level <= 3 ? 90 : 95) / 100);
    // Unbiased within a few metres (coarse columns drop octaves finer than the cell), at every
    // level. (From 256 m cells the coarse columns lost the internal plate edges, and with them the
    // uplift belts: their mean lay 55–80 m below a point sample's.)
    CHECK(std::abs(error_surface / std::max(valid, 1)) < 8.0 + 0.002 * static_cast<double>(cell));
  }
}

TEST_CASE("lod: river valleys and their water look the same from afar as up close") {
  // Regression (playtest: distant rivers were wide water that turned into a dry gully with pools
  // up close, and their valleys were hills from afar). A coarse column dropped the stream and river
  // tiers whole — their valleys with their channels, so the relief they flatten came back — and
  // widened every channel it kept to a cell, so a 3 m stream was 64 m of water at 64 m cells.
  // Around the centrelines of each tier, every level must keep the ground where full detail has it
  // and show water where most of the cell is water in full detail, and only there.
  using testing::FindCentrelines;
  using testing::Tier;
  for (const std::uint64_t seed : {1u, 5u}) {
    const TerrainGenerator gen(seed);
    std::vector<testing::Point> sites;
    for (const auto& [tier, half, step] :
         {std::tuple{Tier::kStream, 12000, 200}, std::tuple{Tier::kRiver, 12000, 400},
          std::tuple{Tier::kGreat, 240000, 8000}}) {
      const auto found = FindCentrelines(gen, tier, 0, 0, half, step, 2);
      sites.insert(sites.end(), found.begin(), found.end());
    }
    REQUIRE(sites.size() >= 4);
    for (int level = 4; level <= 9; ++level) {
      CAPTURE(seed);
      CAPTURE(level);
      const std::int64_t cell = core::LodCellSize(level);
      // Full detail's wetness of a cell: the share of a k × k grid of its columns under water.
      const int k = static_cast<int>(std::min<std::int64_t>(cell, 8));
      double height_error = 0;
      int columns = 0, wet_truth = 0, false_wet = 0, missed_wet = 0, level_checked = 0,
          level_off = 0;
      for (const testing::Point& p : sites) {
        const auto ground = gen.ColumnAt(p.x, p.z);
        const LodCoord mid = SectionAt(level, p.x, static_cast<std::int64_t>(ground.height), p.z);
        // Each column's surface, from whichever row holds it.
        core::LodSurfaces surface(static_cast<std::size_t>(core::kLodPad * core::kLodPad));
        for (int dj = -1; dj <= 1; ++dj) {
          LodCells cells;
          core::LodSurfaces s;
          LodCoord c = mid;
          c.j += dj;
          if (gen.GenerateLod(c, cells, &s) != LodKind::kContent) continue;
          for (std::size_t n = 0; n < s.size(); ++n)
            if (s[n].valid && !surface[n].valid) surface[n] = s[n];
        }
        const auto o = core::LodSectionOrigin(mid);
        const int sx = static_cast<int>((p.x - o.x) / cell),
                  sz = static_cast<int>((p.z - o.z) / cell);
        for (int z = std::max(0, sz - 6); z <= std::min(N - 1, sz + 6); ++z)
          for (int x = std::max(0, sx - 6); x <= std::min(N - 1, sx + 6); ++x) {
            const auto& sf = surface[static_cast<std::size_t>((z + 1) * core::kLodPad + x + 1)];
            const std::int64_t x0 = o.x + x * cell, z0 = o.z + z * cell;
            const auto centre = gen.ColumnAt(static_cast<std::int32_t>(x0 + cell / 2),
                                             static_cast<std::int32_t>(z0 + cell / 2));
            if (!sf.valid || centre.outside || centre.coast <= 0.0f) continue;
            int wet = 0;
            for (int b = 0; b < k; ++b)
              for (int a = 0; a < k; ++a) {
                const auto col =
                    gen.ColumnAt(static_cast<std::int32_t>(x0 + (2 * a + 1) * cell / (2 * k)),
                                 static_cast<std::int32_t>(z0 + (2 * b + 1) * cell / (2 * k)));
                wet += col.height < static_cast<float>(col.water);
              }
            ++columns;
            height_error += std::abs(sf.height - centre.height);
            // Clearly wet or clearly dry in full detail (cells about half under water may go
            // either way).
            // A wet column carries its water's level (the client draws the water there, not at
            // its cell's top): full detail's at the cell's centre.
            if (sf.wet && centre.height < static_cast<float>(centre.water)) {
              ++level_checked;
              level_off += sf.water != static_cast<float>(centre.water);
            }
            if (4 * wet >= 3 * k * k) {
              ++wet_truth;
              missed_wet += !sf.wet;
            } else if (4 * wet <= k * k) {
              false_wet += sf.wet;
            }
          }
      }
      REQUIRE(columns > 0);
      MESSAGE("seed " << seed << " level " << level << ": |height| " << height_error / columns
                      << " m over " << columns << " columns; wet " << wet_truth << ", missed "
                      << missed_wet << ", false " << false_wet);
      // The ground: within a metre or so up to 128 m cells (the valleys kept, the channels' carve
      // at most a few metres), a few metres beyond (the octaves finer than a cell dropped).
      CHECK(height_error / columns < (level <= 7 ? 1.5 : level == 8 ? 5.0 : 8.0));
      // The water: no more than a sliver of columns wet that full detail shows dry, or dry that it
      // shows wet.
      CHECK(false_wet <= columns / 50);
      CHECK(missed_wet <= std::max(1, wet_truth / 10));
      MESSAGE("water levels: " << level_off << " of " << level_checked << " differ");
      CHECK(level_off == 0);
    }
  }
}
