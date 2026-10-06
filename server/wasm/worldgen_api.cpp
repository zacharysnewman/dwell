// C exports of the terrain generator alone, for the client's worldgen workers (ARCHITECTURE.md
// §5.1, §6.3; client/src/worldgen/worker.ts). The same C++ as the server's generator, without Jolt
// or the rest of the sim core, so each worker's module stays small.
#include <emscripten/emscripten.h>

#include <algorithm>
#include <cstdint>
#include <memory>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/terrain.h"

namespace {

dwell::core::ChunkGenerator g_generator;
std::unique_ptr<dwell::core::Chunk> g_chunk;
std::unique_ptr<dwell::worldgen::TerrainGenerator> g_terrain;  // the terrain generator only
std::vector<std::uint8_t> g_map;
dwell::core::LodGenerator g_lod;
dwell::core::LodBoundsFn g_lod_bounds;
dwell::core::LodCells g_lod_cells;
dwell::core::LodSurfaces g_lod_surface;
std::vector<float> g_lod_surface_out;

}  // namespace

extern "C" {

// Selects the world (generator version and u64 seed from Welcome). Returns 1 on success.
EMSCRIPTEN_KEEPALIVE int dwell_worldgen_create(std::uint32_t generator_version,
                                               std::uint32_t seed_lo, std::uint32_t seed_hi) {
  g_generator = dwell::core::GeneratorFor(generator_version,
                                          (static_cast<std::uint64_t>(seed_hi) << 32) | seed_lo);
  g_chunk = std::make_unique<dwell::core::Chunk>();
  g_terrain.reset();
  const std::uint64_t seed = (static_cast<std::uint64_t>(seed_hi) << 32) | seed_lo;
  g_lod = dwell::core::LodGeneratorFor(generator_version, seed);
  g_lod_bounds = dwell::core::LodBoundsFor(generator_version, seed);
  if (generator_version == dwell::core::kGeneratorTerrain) {
    g_terrain = std::make_unique<dwell::worldgen::TerrainGenerator>(
        (static_cast<std::uint64_t>(seed_hi) << 32) | seed_lo);
  }
  return 1;
}

// Biome/height map for the in-game overlay (Phase 3e debug tooling): n × n columns from (x0, z0)
// every `step` metres, row-major along x then z, 4 bytes each — i16 base height (m), u8 biome
// (worldgen::Biome), u8 flags (1 = beyond the world's disc). Null for generators without a
// terrain map. Valid until the next call.
EMSCRIPTEN_KEEPALIVE const std::uint8_t* dwell_worldgen_map(int x0, int z0, int step, int n) {
  if (!g_terrain || n <= 0 || n > 512 || step <= 0) return nullptr;
  g_map.assign(static_cast<std::size_t>(n) * n * 4, 0);
  std::size_t i = 0;
  for (int j = 0; j < n; ++j) {
    for (int k = 0; k < n; ++k, i += 4) {
      const auto c = g_terrain->ColumnAt(x0 + k * step, z0 + j * step);
      const auto h = static_cast<std::int16_t>(std::clamp(c.height, -32768.0f, 32767.0f));
      g_map[i] = static_cast<std::uint8_t>(h);
      g_map[i + 1] = static_cast<std::uint8_t>(static_cast<std::uint16_t>(h) >> 8);
      g_map[i + 2] = static_cast<std::uint8_t>(c.biome);
      g_map[i + 3] = c.outside ? 1 : 0;
    }
  }
  return g_map.data();
}

// Generates a chunk; returns its kChunkVolume u16 materials (chunk index order), valid until the
// next call.
EMSCRIPTEN_KEEPALIVE const std::uint16_t* dwell_worldgen_generate(int cx, int cy, int cz) {
  *g_chunk = dwell::core::Chunk();
  g_generator({cx, cy, cz}, *g_chunk);
  return g_chunk->voxels().data();
}

// ChunkHash of the last generated chunk, as two u32 halves at `out` (low first).
EMSCRIPTEN_KEEPALIVE void dwell_worldgen_hash(std::uint32_t* out) {
  const std::uint64_t h = dwell::core::ChunkHash(*g_chunk);
  out[0] = static_cast<std::uint32_t>(h);
  out[1] = static_cast<std::uint32_t>(h >> 32);
}

// Level of detail (§6.6): generates section (level, i, j, k) — GenerateLod — and returns its kind
// (0 empty, 1 buried, 2 content). The 34³ cells (LodCell order) are at dwell_worldgen_lod_cells()
// until the next call.
EMSCRIPTEN_KEEPALIVE int dwell_worldgen_lod(int level, int i, int j, int k) {
  g_lod_surface.clear();
  if (g_terrain) {
    return static_cast<int>(g_terrain->GenerateLod({level, i, j, k}, g_lod_cells, &g_lod_surface));
  }
  return static_cast<int>(g_lod({level, i, j, k}, g_lod_cells));
}
// The last section's column surfaces (core::LodSurface), 34² × 3 floats: height (m), material,
// flags (1 valid, 2 wet). Null when the generator has none (flat worlds: their cells are exact).
EMSCRIPTEN_KEEPALIVE const float* dwell_worldgen_lod_surface() {
  if (g_lod_surface.empty()) return nullptr;
  g_lod_surface_out.resize(g_lod_surface.size() * 3);
  for (std::size_t n = 0; n < g_lod_surface.size(); ++n) {
    const auto& s = g_lod_surface[n];
    g_lod_surface_out[n * 3] = s.height;
    g_lod_surface_out[n * 3 + 1] = static_cast<float>(s.material);
    g_lod_surface_out[n * 3 + 2] = static_cast<float>((s.valid ? 1 : 0) | (s.wet ? 2 : 0));
  }
  return g_lod_surface_out.data();
}
EMSCRIPTEN_KEEPALIVE const std::uint16_t* dwell_worldgen_lod_cells() { return g_lod_cells.data(); }

// Height bounds of the column of sections (level, i, ·, k): f64 lo, hi, and 1.0 when any of its
// columns is inside the world's disc, written at `out`.
EMSCRIPTEN_KEEPALIVE void dwell_worldgen_lod_bounds(int level, int i, int k, double* out) {
  const dwell::core::LodBounds b = g_lod_bounds(level, i, k);
  out[0] = b.lo;
  out[1] = b.hi;
  out[2] = b.any_inside ? 1.0 : 0.0;
}

}  // extern "C"
