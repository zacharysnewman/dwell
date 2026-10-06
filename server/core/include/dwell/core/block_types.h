#pragma once

#include <cstdint>
#include <string_view>

// The types behind the block registry (docs/BLOCK_REGISTRY.md, ARCHITECTURE.md §6.1): a voxel is a
// runtime *state id*; blocks.gen.h (generated from shared/blocks/*.json) holds the tables.
namespace dwell::core {

// Runtime id of a block state (dense, assigned at build time; never persisted as meaning).
using MaterialId = std::uint16_t;

// Collision shape of a voxel inside its 1 m cell (PLAYER_CONTROLLER.md §5).
enum class VoxelShape : std::uint8_t { kEmpty, kFull, kSlabBottom };

// Compass facing of directional materials (ladders). North = −Z, east = +X.
enum class Facing : std::uint8_t { kNone, kNorth, kEast, kSouth, kWest };

// Per-state behaviour, generated.
struct MaterialInfo {
  std::string_view name;  // canonical state string, `dwell:ladder[facing=north,flooded=false]`
  float density_kg_m3;    // per 1 m³ voxel (mass for Tier 1 clusters, §7.1)
  bool solid;             // has collision (shape != kEmpty)
  bool indestructible;    // bedrock: the structural-integrity anchor (§6.3)
  VoxelShape shape = VoxelShape::kEmpty;
  bool climbable = false;         // ladders, vines (PLAYER_CONTROLLER.md §6.3)
  Facing facing = Facing::kNone;  // climbable: direction the climbing side faces
  float climb_speed_scale = 1.0f;
  bool liquid = false;        // water: swim layer (PLAYER_CONTROLLER.md §6.4)
  float launch_speed = 0.0f;  // debug launch pad: upward knockback when stood on (m/s)
  bool placeable = false;     // in the infinite creative palette (§6.5)
  std::uint16_t block = 0;    // index into kBlocks
};

// A typed property of a block: an enum of named values (booleans are "false" / "true").
struct PropertyDef {
  std::string_view name;
  std::uint16_t value_begin;  // into kPropertyValues
  std::uint16_t value_count;
};

// A block: its namespaced id, the run of state ids it owns and its properties (alphabetical).
struct BlockDef {
  std::string_view id;  // `dwell:ladder`
  std::uint16_t first_state;
  std::uint16_t state_count;
  std::uint16_t property_begin;  // into kProperties
  std::uint16_t property_count;
};

// Height of the solid part of a voxel shape within its cell (0 = empty, 1 = full cube).
inline float ShapeHeight(VoxelShape shape) {
  return shape == VoxelShape::kFull ? 1.0f : shape == VoxelShape::kSlabBottom ? 0.5f : 0.0f;
}

}  // namespace dwell::core
