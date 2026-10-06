// Worldgen inspection (Phase 3 debug tooling): an ASCII biome/height map around a point, biome
// shares, generation timings, and the spawn point.
//   dwell_worldgen_inspect [seed] [centre_x] [centre_z] [metres per character] [slice]
// With "slice", prints a 1:1 vertical section along x through the centre instead of the map.
// Coordinates may be anywhere in the 8,192 km world (e.g. 7999488 0 for ~8,000 km east).
//
// Timings (Phase 10: chunk and LOD generation budgets, reported in every worldgen change):
//   dwell_worldgen_inspect <seed> bench
// Best of five runs of chunk generation around the spawn and of LOD sections at several levels.
//
// Layout statistics over many seeds (Phase 10): continents, land share, islands.
//   dwell_worldgen_inspect <first seed> stats [seeds (16)] [km per sample (32)]
//
// Whole-disc image (Phase 10, WORLD_GENERATION.md §2.4):
//   dwell_worldgen_inspect [seed] disc <out.ppm> [km per pixel (8)] [continents|height]
// Writes a PPM of the whole disc (north up) and prints the continent count, land share and each
// continent's area. `continents` colours land by continent id (islands gold), darkens the sea with
// distance from the coast, and draws internal plate edges; `height` is a hill-shaded height map
// from the full terrain pipeline (slower: use 16 km per pixel or more).
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "dwell/worldgen/noise.h"
#include "dwell/worldgen/terrain.h"

using namespace dwell;

namespace {

// Whole-disc image: one pixel per `km` kilometres, north (−z) up.
int WriteDisc(const worldgen::TerrainGenerator& gen, const std::string& path, int km,
              const std::string& mode) {
  const int n = 2 * (core::kWorldRadius / 1000) / km;
  const bool height = mode == "height";
  std::vector<std::uint8_t> rgb(static_cast<std::size_t>(n) * n * 3, 0);
  std::map<std::int32_t, long> area;  // continent id → pixels
  long land = 0, inside = 0;
  std::vector<float> heights;
  if (height) heights.assign(static_cast<std::size_t>(n) * n, 0.0f);
  const auto at = [&](int px, int py, int& x, int& z) {
    x = (px - n / 2) * km * 1000 + km * 500;
    z = (py - n / 2) * km * 1000 + km * 500;
  };
  for (int py = 0; py < n; ++py)
    for (int px = 0; px < n; ++px) {
      int x, z;
      at(px, py, x, z);
      if (!core::InsideWorldDisc(x, z)) continue;
      ++inside;
      std::uint8_t* o = &rgb[(static_cast<std::size_t>(py) * n + px) * 3];
      if (height) {
        const auto c = gen.ColumnAt(x, z);
        heights[static_cast<std::size_t>(py) * n + px] = c.height;
        land += c.height >= 0.0f;
        continue;
      }
      const auto l = gen.LandAt(x, z);
      if (l.coast > 0.0f) {
        ++land;
        ++area[l.continent];
        const std::uint32_t h =
            worldgen::Mix32(static_cast<std::uint32_t>(l.continent + 7) * 2654435761u);
        float r = 90 + (h & 127), g = 90 + ((h >> 8) & 127), b = 60 + ((h >> 16) & 95);
        if (l.continent == worldgen::continents::kIslandId) r = 230, g = 190, b = 60;
        if (l.plate_edge < 8000.0f) r *= 0.55f, g *= 0.55f, b *= 0.55f;  // internal plate edges
        o[0] = static_cast<std::uint8_t>(std::min(255.0f, r));
        o[1] = static_cast<std::uint8_t>(std::min(255.0f, g));
        o[2] = static_cast<std::uint8_t>(std::min(255.0f, b));
      } else {
        const float depth = std::min(1.0f, -l.coast / 250000.0f);
        o[0] = static_cast<std::uint8_t>(60 - 40 * depth);
        o[1] = static_cast<std::uint8_t>(130 - 80 * depth);
        o[2] = static_cast<std::uint8_t>(200 - 90 * depth);
      }
    }
  if (height) {
    for (int py = 0; py < n; ++py)
      for (int px = 0; px < n; ++px) {
        int x, z;
        at(px, py, x, z);
        if (!core::InsideWorldDisc(x, z)) continue;
        const float h = heights[static_cast<std::size_t>(py) * n + px];
        const float east = heights[static_cast<std::size_t>(py) * n + std::min(px + 1, n - 1)];
        float shade = 1.0f - std::max(-0.4f, std::min(0.4f, (east - h) / (km * 40.0f)));
        std::uint8_t* o = &rgb[(static_cast<std::size_t>(py) * n + px) * 3];
        if (h < 0.0f) {
          const float d = std::min(1.0f, -h / 1500.0f);
          o[0] = static_cast<std::uint8_t>(70 - 50 * d);
          o[1] = static_cast<std::uint8_t>(140 - 90 * d);
          o[2] = static_cast<std::uint8_t>(210 - 100 * d);
        } else {
          const float t = std::min(1.0f, h / 3000.0f);
          o[0] = static_cast<std::uint8_t>(std::min(255.0f, (110 + 120 * t) * shade));
          o[1] = static_cast<std::uint8_t>(std::min(255.0f, (160 - 40 * t) * shade));
          o[2] = static_cast<std::uint8_t>(std::min(255.0f, (70 + 140 * t) * shade));
        }
      }
  }
  std::FILE* f = std::fopen(path.c_str(), "wb");
  if (!f) {
    std::fprintf(stderr, "cannot write %s\n", path.c_str());
    return 1;
  }
  std::fprintf(f, "P6\n%d %d\n255\n", n, n);
  std::fwrite(rgb.data(), 1, rgb.size(), f);
  std::fclose(f);
  std::printf("%s: %d x %d, %d km per pixel (%s)\n", path.c_str(), n, n, km, mode.c_str());
  std::printf("land share %.1f%%\n", 100.0 * land / std::max(1L, inside));
  if (!height) {
    const double px_km2 = static_cast<double>(km) * km;
    int continents = 0;
    for (const auto& [id, px] : area) {
      if (id < 0) {
        std::printf("islands: %.0f km^2\n", px * px_km2);
      } else {
        ++continents;
        std::printf("continent %3d: %9.0f km^2\n", id, px * px_km2);
      }
    }
    std::printf("%d continents (layout says %d)\n", continents, gen.Continents().ContinentCount());
  }
  return 0;
}

}  // namespace

// Best-of-five ms per chunk (a 5 × 5 × 11 block around the surface at the origin) and per LOD
// section (four sections near the origin) at several levels. Uses only what every generator version
// has, so the same code times a baseline.
int Bench(std::uint64_t seed) {
  const worldgen::TerrainGenerator gen(seed);
  const int surface = worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(0, 0).height), 32);
  double best = 1e30;
  for (int run = 0; run < (std::getenv("DWELL_BENCH_ONLY") ? 0 : 5); ++run) {
    std::chrono::steady_clock::duration elapsed{};
    int chunks = 0;
    for (int x = -2; x <= 2; ++x)
      for (int z = -2; z <= 2; ++z)
        for (int y = surface - 4; y <= surface + 6; ++y) {
          core::Chunk c;
          const auto start = std::chrono::steady_clock::now();
          gen.Generate({x, y, z}, c);
          elapsed += std::chrono::steady_clock::now() - start;
          ++chunks;
        }
    best = std::min(best, std::chrono::duration<double, std::milli>(elapsed).count() / chunks);
  }
  std::printf("chunk: %.3f ms\n", best);
  if (const char* layout_runs = std::getenv("DWELL_BENCH_LAYOUT")) {
    // The continent layout alone: exact evaluations and lattice samples per microsecond.
    const auto& layout = gen.Continents();
    const int n = std::max(1, std::atoi(layout_runs));
    for (int mode = 0; mode < 2; ++mode) {
      const auto start = std::chrono::steady_clock::now();
      double sink = 0;
      for (int i = 0; i < n; ++i) {
        // A grid of points, as a LOD section's columns are: neighbours share their plates.
        const std::int64_t x = (i % 200) * 1024LL - 100000, z = (i / 200 % 200) * 1024LL - 100000;
        sink += mode == 0 ? layout.At(x, z).coast : layout.Sample(x, z).coast;
      }
      const double us =
          std::chrono::duration<double, std::micro>(std::chrono::steady_clock::now() - start)
              .count();
      std::printf("%s: %.3f us each (%g)\n", mode == 0 ? "At" : "Sample", us / n, sink);
    }
    if (std::getenv("DWELL_BENCH_ONLY")) return 0;
  }
  // LOD sections around (DWELL_BENCH_AT=x,z), default the origin: sections at a coast cost more
  // than in an interior. `coast` names the point where the land ends east of the origin.
  std::int64_t at_x = 0, at_z = 0;
  if (std::getenv("DWELL_BENCH_AT") && std::string_view(std::getenv("DWELL_BENCH_AT")) == "coast") {
    while (gen.LandAt(static_cast<std::int32_t>(at_x), 0).coast > 0.0f) at_x += 1000;
    std::printf("LOD sections at the coast (%ld, 0)\n", static_cast<long>(at_x));
  } else if (const char* at = std::getenv("DWELL_BENCH_AT"))
    std::sscanf(at, "%ld,%ld", &at_x, &at_z);
  for (const int level : {1, 3, 5, 8, 10, 12}) {
    double best_lod = 1e30;
    for (int run = 0; run < 5; ++run) {
      std::chrono::steady_clock::duration elapsed{};
      int sections = 0;
      for (int i = 0; i < 2; ++i)
        for (int k = 0; k < 2; ++k) {
          const std::int64_t size = core::LodSectionSize(level);
          const core::LodCoord c{level,
                                 static_cast<std::int32_t>((at_x - core::kLodOriginX) / size) + i,
                                 static_cast<std::int32_t>((-64 - core::kLodOriginY) / size),
                                 static_cast<std::int32_t>((at_z - core::kLodOriginZ) / size) + k};
          core::LodCells cells;
          const auto start = std::chrono::steady_clock::now();
          gen.GenerateLod(c, cells);
          elapsed += std::chrono::steady_clock::now() - start;
          ++sections;
        }
      best_lod =
          std::min(best_lod, std::chrono::duration<double, std::milli>(elapsed).count() / sections);
    }
    std::printf("lod level %2d: %.3f ms/section\n", level, best_lod);
  }
  return 0;
}

int Stats(std::uint64_t first, int seeds, int km) {
  for (int k = 0; k < seeds; ++k) {
    const worldgen::TerrainGenerator gen(first + static_cast<std::uint64_t>(k));
    long land = 0, inside = 0, island = 0;
    std::map<std::int32_t, long> area;
    for (int z = -core::kWorldRadius + km * 500; z < core::kWorldRadius; z += km * 1000)
      for (int x = -core::kWorldRadius + km * 500; x < core::kWorldRadius; x += km * 1000) {
        if (!core::InsideWorldDisc(x, z)) continue;
        ++inside;
        const auto l = gen.LandAt(x, z);
        if (l.coast <= 0.0f) continue;
        ++land;
        if (l.continent == worldgen::continents::kIslandId) ++island;
        ++area[l.continent];
      }
    int continents = 0;
    for (const auto& [id, n] : area) continents += id >= 0 && n > 0;
    std::printf("seed %3llu: layout %2d continents, seen %2d, land %.1f%%, islands %.2f%%\n",
                static_cast<unsigned long long>(first) + static_cast<unsigned long long>(k),
                gen.Continents().ContinentCount(), continents, 100.0 * land / inside,
                100.0 * island / inside);
  }
  return 0;
}

int main(int argc, char** argv) {
  const std::uint64_t seed = argc > 1 ? std::strtoull(argv[1], nullptr, 10) : 0;
  if (argc > 2 && std::string_view(argv[2]) == "bench") return Bench(seed);
  if (argc > 2 && std::string_view(argv[2]) == "stats") {
    return Stats(seed, argc > 3 ? std::atoi(argv[3]) : 16,
                 argc > 4 ? std::max(1, std::atoi(argv[4])) : 32);
  }
  if (argc > 3 && std::string_view(argv[2]) == "disc") {
    const worldgen::TerrainGenerator gen(seed);
    return WriteDisc(gen, argv[3], argc > 4 ? std::max(1, std::atoi(argv[4])) : 8,
                     argc > 5 ? argv[5] : "continents");
  }
  const int cx = argc > 2 ? std::atoi(argv[2]) : 0;
  const int cz = argc > 3 ? std::atoi(argv[3]) : 0;
  const int step = argc > 4 ? std::atoi(argv[4]) : 32;
  const worldgen::TerrainGenerator gen(seed);

  if (argc > 5 && std::string_view(argv[5]) == "slice") {
    // Generated chunks, so the section includes surface materials, ores, and features.
    core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
    const auto base = gen.ColumnAt(cx, cz).height;
    const int top = static_cast<int>(base) + 40;
    for (int y = top; y > top - 70; --y) {
      std::printf("%4d ", y);
      for (int x = cx - 60; x < cx + 60; ++x) {
        const auto m = world.GetVoxel(x, y, cz);
        char ch = '?';
        switch (m) {
          case core::Materials::kAir:
            ch = ' ';
            break;
          case core::Materials::kStone:
            ch = '#';
            break;
          case core::Materials::kDirt:
            ch = ':';
            break;
          case core::Materials::kGrass:
            ch = '"';
            break;
          case core::Materials::kWater:
            ch = '~';
            break;
          case core::Materials::kSand:
            ch = '.';
            break;
          case core::Materials::kSandstone:
            ch = '=';
            break;
          case core::Materials::kGravel:
            ch = ',';
            break;
          case core::Materials::kSnow:
            ch = '*';
            break;
          case core::Materials::kLog:
            ch = '|';
            break;
          case core::Materials::kLeaves:
            ch = '%';
            break;
          case core::Materials::kBedrock:
            ch = 'B';
            break;
          default:
            ch = 'o';
            break;  // ores
        }
        std::putchar(ch);
      }
      std::putchar('\n');
    }
    return 0;
  }

  // Map: biome letter, M for the upper slopes of ranges, blank beyond the rim.
  std::map<worldgen::Biome, int> counts;
  float lo = 1e9f, hi = -1e9f;
  for (int row = -24; row < 24; ++row) {
    for (int col = -48; col < 48; ++col) {
      const auto c = gen.ColumnAt(cx + col * step, cz + row * step);
      if (c.outside) {
        std::putchar(' ');
        continue;
      }
      ++counts[c.biome];
      lo = std::min(lo, c.height);
      hi = std::max(hi, c.height);
      char ch = "~bpfdsm"[static_cast<int>(c.biome)];
      if (c.height > 500.0f) ch = 'M';
      std::putchar(ch);
    }
    std::putchar('\n');
  }
  std::printf(
      "~ ocean  b beach  p plains  f forest  d desert  s snowy  m mountains  M > 500 m  (blank: "
      "beyond the rim)\n");
  std::printf("height %.1f .. %.1f\n", lo, hi);
  for (const auto& [b, n] : counts)
    std::printf("%-10s %5.1f%%\n", worldgen::BiomeName(b), 100.0 * n / (48 * 96));

  // Timings: a 5×5 column of chunks from 4 below to 6 above the centre's surface.
  core::Chunk chunk;
  int chunks = 0;
  std::map<core::MaterialId, long> materials;
  std::chrono::steady_clock::duration elapsed{};
  const int surface = worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(cx, cz).height), 32);
  for (int x = -2; x <= 2; ++x)
    for (int z = -2; z <= 2; ++z)
      for (int y = surface - 4; y <= surface + 6; ++y) {
        core::Chunk c;
        const auto start = std::chrono::steady_clock::now();
        gen.Generate({worldgen::FloorDiv(cx, 32) + x, y, worldgen::FloorDiv(cz, 32) + z}, c);
        elapsed += std::chrono::steady_clock::now() - start;
        for (const auto m : c.voxels()) ++materials[m];
        ++chunks;
      }
  const double ms = std::chrono::duration<double, std::milli>(elapsed).count();
  std::printf("%d chunks, %.3f ms/chunk\n", chunks, ms / chunks);
  for (const auto& [m, n] : materials)
    std::printf("  %-10s %ld\n", core::GetMaterial(m).name.data(), n);
  const auto s = gen.SpawnPoint();
  std::printf(
      "spawn %.1f %.1f %.1f (%s)\n", s[0], s[1], s[2],
      worldgen::BiomeName(gen.ColumnAt(static_cast<int>(s[0]), static_cast<int>(s[2])).biome));
  return 0;
}
