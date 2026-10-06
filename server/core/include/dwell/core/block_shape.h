#pragma once

#include <optional>
#include <span>

#include "dwell/core/block_types.h"
#include "dwell/core/blocks.gen.h"

// Voxel shapes: the solid geometry of a block state inside its 1 m cell (docs/SLOPE_BLOCKS.md).
// Cubes, slabs and slopes share one description (`ShapeInfo`, baked into blocks.gen.h from
// shared/blocks/shapes.mjs), so collision, targeting, edit checks and the meshers all read the same
// numbers. Every function here uses only `+ − × /`, `min` and `max` on floats (ADR 0010), so server
// and WASM client agree bit for bit.
namespace dwell::core {

inline const ShapeInfo& ShapeOf(MaterialId state) { return kShapes[kMaterials[state].shape_index]; }

inline std::span<const ShapeFace> FacesOf(const ShapeInfo& shape) {
  return std::span<const ShapeFace>(kShapeFaces).subspan(shape.first_face, shape.face_count);
}

// The solid's extent in y at the horizontal point (fx, fz) ∈ [0, 1]²: upright pieces fill
// [0, h], inverted ones [1 − h, 1], where h is the piecewise-planar surface height. `hi − lo` is 0
// where the solid has no thickness.
struct SolidSpan {
  float lo, hi;
};
SolidSpan SolidSpanAt(const ShapeInfo& shape, float fx, float fz);

// Height of the top of the solid at (fx, fz) within the cell (0 for an empty cell): the
// walkable surface of an upright piece, 1 under an inverted one.
inline float SurfaceHeightAt(MaterialId state, float fx, float fz) {
  const ShapeInfo& shape = ShapeOf(state);
  return shape.face_count == 0 ? 0.0f : SolidSpanAt(shape, fx, fz).hi;
}

// Whether the point (cell coordinates) is inside the solid or on its boundary, within `eps`.
bool PointInSolid(const ShapeInfo& shape, float x, float y, float z, float eps);

// Cell face indices as the mesher numbers them: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
inline constexpr int OppositeFace(int face) { return face ^ 1; }

// Whether the part of `shape` that lies on its cell face `face` is completely covered by the
// opposite face of `neighbour`. Faces that are empty are covered by anything (nothing to draw).
bool FaceCovered(const ShapeInfo& shape, int face, const ShapeInfo& neighbour);

// The nearest entry of the ray `origin + t·dir`, t ∈ [min_t, max_t], into the solid of the cell
// whose minimum corner is `cell` (origin and cell share a frame, so far from the world origin the
// caller passes both relative to a nearby base): the distance and the outward normal of the face
// entered. A ray that starts inside the solid does not hit its own surface. If `neighbours` (the
// shapes across faces +X −X +Y −Y +Z −Z) is given, a point of a cell face that lies against the
// neighbour's solid is internal — the ray is already inside — and is not an entry.
struct ShapeHit {
  float t;
  float normal[3];
};
std::optional<ShapeHit> RayEnterShape(const ShapeInfo& shape, const float (&cell)[3],
                                      const float (&origin)[3], const float (&dir)[3], float max_t,
                                      const ShapeInfo* const* neighbours = nullptr,
                                      float min_t = 0.0f);

// Squared shortest distance from the vertical segment (x, y0..y1, z) — a capsule axis, cell
// coordinates — to the shape's surface, 0 if the segment touches or lies inside the solid.
float VerticalSegmentDistanceSq(const ShapeInfo& shape, float x, float z, float y0, float y1);

}  // namespace dwell::core
