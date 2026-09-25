// Worldgen inspection (Phase 3 debug tooling): an ASCII biome/height map around a point, biome
// shares, generation timings, and the spawn point.
//   dwell_worldgen_inspect [seed] [centre_x] [centre_z] [metres per character] [slice]
// With "slice", prints a 1:1 vertical section along x through the centre instead of the map.
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <string_view>

#include "dwell/worldgen/terrain.h"

using namespace dwell;

int main(int argc, char** argv) {
  const std::uint64_t seed = argc > 1 ? std::strtoull(argv[1], nullptr, 10) : 0;
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

  // Map: biome letter, upper case above 100 m.
  std::map<worldgen::Biome, int> counts;
  float lo = 1e9f, hi = -1e9f;
  for (int row = -24; row < 24; ++row) {
    for (int col = -48; col < 48; ++col) {
      const auto c = gen.ColumnAt(cx + col * step, cz + row * step);
      ++counts[c.biome];
      lo = std::min(lo, c.height);
      hi = std::max(hi, c.height);
      char ch = "~bpfdsm"[static_cast<int>(c.biome)];
      if (c.height > 130.0f) ch = 'M';
      std::putchar(ch);
    }
    std::putchar('\n');
  }
  std::printf("~ ocean  b beach  p plains  f forest  d desert  s snowy  m mountains  M > 130 m\n");
  std::printf("height %.1f .. %.1f\n", lo, hi);
  for (const auto& [b, n] : counts)
    std::printf("%-10s %5.1f%%\n", worldgen::BiomeName(b), 100.0 * n / (48 * 96));

  // Timings: a 5×5 column of chunks from y −4..6 around the centre.
  core::Chunk chunk;
  int chunks = 0;
  std::map<core::MaterialId, long> materials;
  std::chrono::steady_clock::duration elapsed{};
  for (int x = -2; x <= 2; ++x)
    for (int z = -2; z <= 2; ++z)
      for (int y = -4; y <= 6; ++y) {
        core::Chunk c;
        const auto start = std::chrono::steady_clock::now();
        gen.Generate({cx / 32 + x, y, cz / 32 + z}, c);
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
