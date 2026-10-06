#pragma once

#include <cstdint>

#include "dwell/core/voxel.h"

// Slope shaping of generated terrain (docs/SLOPE_BLOCKS.md §5): which piece a cell becomes from the
// heights of its four corners. Corner heights are quantised to halves of a cell (0, 1, 2) relative
// to the cell's floor, in the order NW, NE, SE, SW. Pure and deterministic: the tables are built
// from the block registry's shape table with integer arithmetic only.
namespace dwell::worldgen::slopes {

enum class Kind : std::uint8_t {
  kAir,     // every corner at the floor: nothing
  kFull,    // every corner at the ceiling: a full cube
  kShaped,  // a slab or one of the slope shapes
};

struct Piece {
  Kind kind = Kind::kAir;
  // kShaped: the corner-height pattern of the piece actually used (index a + 3b + 9c + 27d).
  std::uint8_t pattern = 0;
};

constexpr int PatternIndex(int nw, int ne, int se, int sw) {
  return nw + 3 * ne + 9 * se + 27 * sw;
}

// The piece for the corner heights (each 0..2): the shape with exactly those corners if there is
// one (nine shapes in four orientations, and the slab), otherwise the nearest by total distance in
// halves, preferring the higher on a tie (raise the lowest corner), then the lower pattern index.
Piece PieceFor(int nw, int ne, int se, int sw);

// The corner heights (halves, NW NE SE SW) of a shaped piece.
void CornersOf(const Piece& piece, int (&corners)[4]);

// Whether a cube material (stone, dirt, grass, sand, …) has slope and slab families.
bool HasFamily(core::MaterialId cube);

// The state of `piece` (upright) made of the cube material, `flooded` when water fills its open
// part. A cube for kFull, air for kAir; the cube itself for a material without families.
core::MaterialId StateFor(core::MaterialId cube, const Piece& piece, bool flooded);

}  // namespace dwell::worldgen::slopes
