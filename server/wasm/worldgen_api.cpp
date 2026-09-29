// C exports of the terrain generator alone, for the client's worldgen workers (ARCHITECTURE.md
// §5.1, §6.3; client/src/worldgen/worker.ts). The same C++ as the server's generator, without Jolt
// or the rest of the sim core, so each worker's module stays small.
#include <emscripten/emscripten.h>

#include <cstdint>
#include <memory>

#include "dwell/core/voxel.h"

namespace {

dwell::core::ChunkGenerator g_generator;
std::unique_ptr<dwell::core::Chunk> g_chunk;

}  // namespace

extern "C" {

// Selects the world (generator version and u64 seed from Welcome). Returns 1 on success.
EMSCRIPTEN_KEEPALIVE int dwell_worldgen_create(std::uint32_t generator_version,
                                               std::uint32_t seed_lo, std::uint32_t seed_hi) {
  g_generator = dwell::core::GeneratorFor(generator_version,
                                          (static_cast<std::uint64_t>(seed_hi) << 32) | seed_lo);
  g_chunk = std::make_unique<dwell::core::Chunk>();
  return 1;
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

}  // extern "C"
