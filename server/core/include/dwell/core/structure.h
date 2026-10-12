#pragma once

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "dwell/core/voxel.h"

// Structure files (ARCHITECTURE.md §6.3): hand-built voxel geometry as data, stamped over a base
// generator. The movement playground the controller tests walk is one
// (server/tests/player/playground.structure). Text, one command per line, `#` starts a comment:
//
//   fill x0 y0 z0 x1 y1 z1 <block state>   # the inclusive box, e.g. dwell:stone or
//                                          # dwell:ladder[facing=north]
//
// Blocks are named by canonical state string (block_registry.h), never by id; later commands
// overwrite earlier ones. A structure only replaces voxels inside its boxes, so the base world
// shows everywhere else.
namespace dwell::core {

struct Structure {
  struct Fill {
    std::int32_t x0, y0, z0, x1, y1, z1;  // inclusive, x0 ≤ x1 etc.
    MaterialId material;
  };
  std::vector<Fill> fills;

  // The material the structure puts at a voxel, if any (the last fill containing it).
  std::optional<MaterialId> At(std::int32_t x, std::int32_t y, std::int32_t z) const;
  // True when no fill touches the chunk.
  bool Misses(const ChunkCoord& coord) const;
};

// Parses a structure; nullopt with `error` (line number and reason) on a malformed line, an
// unknown command or an unknown block state.
std::optional<Structure> ParseStructure(std::string_view text, std::string* error = nullptr);
// Reads and parses a structure file.
std::optional<Structure> LoadStructure(const std::string& path, std::string* error = nullptr);

// `base` with the structure stamped over it.
ChunkGenerator WithStructure(ChunkGenerator base, std::shared_ptr<const Structure> structure);
// `base_air` except for chunks the structure touches (which are never all air by this test).
AirChunkTest WithStructure(AirChunkTest base_air, std::shared_ptr<const Structure> structure);

}  // namespace dwell::core
