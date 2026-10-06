#pragma once

#include <array>
#include <cstdint>
#include <string_view>

// The types behind the block registry (docs/BLOCK_REGISTRY.md, ARCHITECTURE.md §6.1): a voxel is a
// runtime *state id*; blocks.gen.h (generated from shared/blocks/*.json) holds the tables.
namespace dwell::core {

// Runtime id of a block state (dense, assigned at build time; never persisted as meaning).
using MaterialId = std::uint16_t;

// Coarse class of a voxel's solid inside its 1 m cell (PLAYER_CONTROLLER.md §5). `kShaped` covers
// every slab and slope; its exact geometry is `ShapeOf(state)` (block_shape.h).
enum class VoxelShape : std::uint8_t { kEmpty, kFull, kShaped };

// One convex polygon (3 or 4 vertices, outward-wound, cell coordinates) of a shape's surface. `tag`
// is the cell face it lies on, mesher-style (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z) or kSurfaceTag
// for the sloped (or flat, partial-height) top.
inline constexpr std::uint8_t kSurfaceTag = 6;
struct ShapeFace {
  std::uint8_t tag;
  std::uint8_t count;
  float v[4][3];
};

// Baked geometry of a voxel shape (docs/SLOPE_BLOCKS.md §1.2, shared/blocks/shapes.mjs). Heights
// are halves of a cell (0..2) at the corners NW, NE, SE, SW (north = −Z, east = +X) of the solid's
// flat side: the floor for upright pieces, the ceiling for inverted ones.
struct ShapeInfo {
  std::uint16_t first_face;
  std::uint16_t face_count;
  std::array<std::uint8_t, 4> corners;
  bool inverted;
  std::uint8_t diagonal;  // top-surface split: 0 NW–SE, 1 NE–SW
  bool convex;
  float volume;  // m³ (a full cell is 1)
  // Side profiles for culling: heights (halves) at the lower and higher running coordinate (z for
  // ±X, x for ±Z) of faces +X, −X, +Z, −Z.
  std::array<std::array<std::uint8_t, 2>, 4> sides;
  bool full_top;       // the cell's +Y face is entirely solid
  bool full_bottom;    // the cell's −Y face is entirely solid
  float min_y, max_y;  // extent of the solid in y
};

// Compass facing of directional materials (ladders). North = −Z, east = +X.
enum class Facing : std::uint8_t { kNone, kNorth, kEast, kSouth, kWest };

// Per-state behaviour, generated.
struct MaterialInfo {
  std::string_view name;  // canonical state string, `dwell:ladder[facing=north,flooded=false]`
  float density_kg_m3;    // per 1 m³ voxel (mass for Tier 1 clusters, §7.1)
  bool solid;             // has collision (shape != kEmpty)
  bool indestructible;    // bedrock: the structural-integrity anchor (§6.3)
  VoxelShape shape = VoxelShape::kEmpty;
  std::uint16_t shape_index = 0;  // into kShapes (0 = no solid)
  bool flooded = false;           // water fills the open part of a shaped cell (§3.1)
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

}  // namespace dwell::core
