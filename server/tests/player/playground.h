#pragma once

// The movement playground (playground.structure) over the flat test world: the controller and
// netcode tests' obstacle course (PLAYER_CONTROLLER.md §9; formerly generator version 1).
#include <memory>
#include <stdexcept>
#include <string>

#include "dwell/core/structure.h"

namespace dwell::test {

inline std::shared_ptr<const core::Structure> PlaygroundStructure() {
  static const std::shared_ptr<const core::Structure> structure = [] {
    std::string error;
    auto s = core::LoadStructure(DWELL_PLAYGROUND_STRUCTURE, &error);
    if (!s) throw std::runtime_error(error);
    return std::make_shared<const core::Structure>(std::move(*s));
  }();
  return structure;
}

// The flat world with the playground stamped over it.
inline core::ChunkGenerator PlaygroundGenerator() {
  return core::WithStructure(core::ChunkGenerator(core::GenerateFlatChunk), PlaygroundStructure());
}

// Fills a chunk of the playground world (for APIs that take a plain function).
inline void GeneratePlaygroundChunk(const core::ChunkCoord& coord, core::Chunk& chunk) {
  PlaygroundGenerator()(coord, chunk);
}

}  // namespace dwell::test
