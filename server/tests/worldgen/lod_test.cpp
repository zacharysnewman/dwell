// Level of detail (ARCHITECTURE.md §6.6, ADR 0012): the section grid, Downsample, GenerateLod and
// its golden hashes, which pin native and WASM builds to bit-identical output. Runs natively
// (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <cmath>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/protocol/messages.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;
using core::Chunk;
using core::ChunkCoord;
using core::LodCell;
using core::LodCells;
using core::LodCoord;
using core::LodKind;
using core::MaterialId;
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

// A column of each biome near the origin, on a coarse grid (as the worldgen tests find them).
std::optional<std::pair<std::int32_t, std::int32_t>> FindBiome(const TerrainGenerator& gen,
                                                               worldgen::Biome biome) {
  for (int r = 0; r < 4000; r += 48)
    for (int x = -r; x <= r; x += 48)
      for (const int z : {-r, r}) {
        if (gen.ColumnAt(x, z).biome == biome) return std::pair{x, z};
        if (gen.ColumnAt(z, x).biome == biome) return std::pair{z, x};
      }
  return std::nullopt;
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
         {worldgen::Biome::kForest, worldgen::Biome::kMountains, worldgen::Biome::kOcean,
          worldgen::Biome::kBeach, worldgen::Biome::kDesert, worldgen::Biome::kSnowy}) {
      if (const auto at = FindBiome(gen, biome)) {
        sites.push_back({worldgen::BiomeName(biome), at->first, at->second});
      }
    }
    CHECK(sites.size() == 7);
    double mean_sum = 0;
    int count = 0;
    for (const Site& site : sites) {
      const auto h = static_cast<std::int64_t>(gen.ColumnAt(site.x, site.z).height);
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
        CHECK(a.surface_within >= 0.95);
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
            grassy += m == M::kGrass || m == M::kSand || m == M::kSnow || m == M::kGravel;
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
    // A deep ocean (a planet-scale basin), found on a coarse grid.
    std::optional<std::pair<std::int32_t, std::int32_t>> ocean;
    for (std::int32_t r = 0; !ocean && r < 600'000; r += 8192)
      for (std::int32_t x = -r; !ocean && x <= r; x += 8192)
        if (gen.ColumnAt(x, r).height < -200.0f) ocean = std::pair{x, r};
    REQUIRE(ocean);
    for (const int level : {6, 8, 10}) {
      CAPTURE(level);
      const LodCoord c = SectionAt(level, ocean->first, -1, ocean->second);
      LodCells cells;
      REQUIRE(gen.GenerateLod(c, cells) == LodKind::kContent);
      int tops = 0, water = 0;
      for (int z = 0; z < N; ++z)
        for (int x = 0; x < N; ++x) {
          if (gen.ColumnAt(static_cast<std::int32_t>(core::LodSectionOrigin(c).x +
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
    const std::vector<Case> cases = {
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
      out << "# seed level i j k kind fnv1a64(kind, cells) - generator version 4\n";
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
      const LodCoord c = SectionAt(level, 4000 + s * 37000, 0, 3000 + s * 23000);
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
          CHECK(sf.wet == (sf.height < 0));  // its own (coarse) column's shore, not full detail's
          error_surface += sf.height - truth;
        }
    }
    REQUIRE(columns > 0);
    MESSAGE("level " << level << ": cell tops " << error_cells / columns << " m above the surface, "
                     << "column surfaces " << (valid ? error_surface / valid : 0) << " m (" << valid
                     << "/" << columns << " columns)");
    // Levels 1–2 keep a whole cell where 3D noise raised the ground above the column's height.
    CHECK(valid >= columns * (level <= 2 ? 75 : 95) / 100);
    // Unbiased within a few metres (coarse columns drop octaves finer than the cell).
    CHECK(std::abs(error_surface / std::max(valid, 1)) < 8.0 + 0.002 * static_cast<double>(cell));
  }
}
