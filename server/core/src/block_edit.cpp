#include "dwell/core/block_edit.h"

#include "dwell/core/block_shape.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace dwell::core {
namespace {

constexpr float kInf = std::numeric_limits<float>::infinity();

// The cell face a surface normal points through: its dominant axis, with ties going to the
// vertical (so a 45° slope's surface is "the top"). 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
int FaceOfNormal(const float (&n)[3]) {
  const float ax = std::fabs(n[0]), ay = std::fabs(n[1]), az = std::fabs(n[2]);
  if (ay >= ax && ay >= az) return n[1] > 0.0f ? 2 : 3;
  if (ax >= az) return n[0] > 0.0f ? 0 : 1;
  return n[2] > 0.0f ? 4 : 5;
}

bool InWorldRows(std::int32_t y) { return y >= kWorldMinY && y < kWorldMaxY; }

}  // namespace

bool Targetable(MaterialId m) { return m != Materials::kAir && !GetMaterial(m).liquid; }

bool Placeable(MaterialId m) { return m < Materials::kCount && GetMaterial(m).placeable; }

std::optional<BlockHit> RaycastBlock(VoxelWorld& world, const std::array<double, 3>& origin,
                                     const std::array<float, 3>& dir, float max_distance) {
  // Amanatides–Woo walk relative to the origin's cell (`base`), like VoxelQuery::CastVoxels.
  std::int32_t base[3], cell[3], step[3];
  float o[3], d[3] = {dir[0], dir[1], dir[2]}, t_max[3], t_delta[3];
  for (int i = 0; i < 3; ++i) {
    base[i] = static_cast<std::int32_t>(std::floor(origin[i]));
    o[i] = static_cast<float>(origin[i] - static_cast<double>(base[i]));
    cell[i] = 0;
    if (d[i] > 0.0f) {
      step[i] = 1;
      t_delta[i] = 1.0f / d[i];
      t_max[i] = (1.0f - o[i]) / d[i];
    } else if (d[i] < 0.0f) {
      step[i] = -1;
      t_delta[i] = -1.0f / d[i];
      t_max[i] = -o[i] / d[i];
    } else {
      step[i] = 0;
      t_delta[i] = kInf;
      t_max[i] = kInf;
    }
  }
  for (float t_cell = 0.0f; t_cell <= max_distance;) {
    const MaterialId m = world.GetVoxel(base[0] + cell[0], base[1] + cell[1], base[2] + cell[2]);
    if (Targetable(m)) {
      const float corner[3] = {static_cast<float>(cell[0]), static_cast<float>(cell[1]),
                               static_cast<float>(cell[2])};
      if (const auto hit = RayEnterShape(ShapeOf(m), corner, o, d, max_distance);
          hit && hit->t <= max_distance) {
        BlockHit out;
        for (int i = 0; i < 3; ++i) out.cell[i] = base[i] + cell[i];
        out.face = FaceOfNormal(hit->normal);
        out.distance = hit->t;
        return out;
      }
    }
    const int axis =
        t_max[0] < t_max[1] ? (t_max[0] < t_max[2] ? 0 : 2) : (t_max[1] < t_max[2] ? 1 : 2);
    t_cell = t_max[axis];
    cell[axis] += step[axis];
    t_max[axis] += t_delta[axis];
  }
  return std::nullopt;
}

EditOutcome CheckBlockEdit(VoxelWorld& world, const protocol::BlockEditRequest& request,
                           const std::array<double, 3>& eye,
                           const std::vector<EditCapsule>& players, float reach) {
  EditOutcome out;
  const auto& c = request.cell;
  // Reach first: nothing far away is read (reading would generate it).
  double gap2 = 0.0;
  for (int i = 0; i < 3; ++i) {
    const double lo = c[i], hi = lo + 1.0;
    const double g = eye[i] < lo ? lo - eye[i] : eye[i] > hi ? eye[i] - hi : 0.0;
    gap2 += g * g;
  }
  if (gap2 > static_cast<double>(reach) * reach) return {EditCheck::kOutOfReach};

  const MaterialId target = world.GetVoxel(c[0], c[1], c[2]);
  if (!Targetable(target)) return {EditCheck::kNothingThere};

  const auto& n = kFaceDirs[request.face];

  // Line of sight: the eye is in front of a surface of the target whose normal points through the
  // requested face (a cell face, or a sloped face whose normal mostly does), and a ray from it
  // reaches the target cell first at the polygon's centre or one of its corners' neighbourhoods.
  const ShapeInfo& target_shape = ShapeOf(target);
  bool visible = false;
  for (const ShapeFace& polygon : FacesOf(target_shape)) {
    if (visible) break;
    const float pts[4][3] = {{polygon.v[0][0], polygon.v[0][1], polygon.v[0][2]},
                             {polygon.v[1][0], polygon.v[1][1], polygon.v[1][2]},
                             {polygon.v[2][0], polygon.v[2][1], polygon.v[2][2]},
                             {polygon.v[3][0], polygon.v[3][1], polygon.v[3][2]}};
    float normal[3];  // cross(p1 − p0, p2 − p0)
    {
      const float e1[3] = {pts[1][0] - pts[0][0], pts[1][1] - pts[0][1], pts[1][2] - pts[0][2]};
      const float e2[3] = {pts[2][0] - pts[0][0], pts[2][1] - pts[0][1], pts[2][2] - pts[0][2]};
      normal[0] = e1[1] * e2[2] - e1[2] * e2[1];
      normal[1] = e1[2] * e2[0] - e1[0] * e2[2];
      normal[2] = e1[0] * e2[1] - e1[1] * e2[0];
    }
    const float length_n =
        std::sqrt(normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]);
    if (length_n <= 0.0f) continue;
    for (float& v : normal) v /= length_n;
    if (FaceOfNormal(normal) != request.face) continue;
    // The eye must be on the outer side of the polygon's plane.
    const double side = (eye[0] - (c[0] + pts[0][0])) * normal[0] +
                        (eye[1] - (c[1] + pts[0][1])) * normal[1] +
                        (eye[2] - (c[2] + pts[0][2])) * normal[2];
    if (side <= 0.0) continue;
    float centre[3] = {0, 0, 0};
    for (int i = 0; i < polygon.count; ++i) {
      for (int k = 0; k < 3; ++k) centre[k] += pts[i][k] / static_cast<float>(polygon.count);
    }
    for (int sample = -1; sample < polygon.count && !visible; ++sample) {
      std::array<double, 3> p;
      for (int k = 0; k < 3; ++k) {
        // The centre, then 70 % of the way from it to each corner (the old 0.15 / 0.85 points).
        const float local =
            sample < 0 ? centre[k] : centre[k] + 0.7f * (pts[sample][k] - centre[k]);
        p[k] = c[k] + static_cast<double>(local) + 0.01 * normal[k];
      }
      std::array<double, 3> delta{p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]};
      const double length =
          std::sqrt(delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]);
      if (length <= 0.0) continue;
      const std::array<float, 3> dir{static_cast<float>(delta[0] / length),
                                     static_cast<float>(delta[1] / length),
                                     static_cast<float>(delta[2] / length)};
      // On past the point: a grazing ray needs a while to cross the 1 cm to the face, and past
      // the plane it can only enter the target (the point lies well inside the face).
      const auto hit = RaycastBlock(world, eye, dir, static_cast<float>(length) + 1.0f);
      if (hit && hit->cell == c) visible = true;
    }
  }
  if (!visible) return {EditCheck::kNoLineOfSight};

  if (request.action == protocol::BlockEditAction::kBreak) {
    if (GetMaterial(target).indestructible) return {EditCheck::kUnbreakable};
    out.cell = c;
    out.material = Materials::kAir;
    return out;
  }

  if (!Placeable(request.material)) return {EditCheck::kNotPlaceable};
  out.cell = {c[0] + n[0], c[1] + n[1], c[2] + n[2]};
  out.material = request.material;
  if (!InWorldRows(out.cell[1]) || !InsideWorldDisc(out.cell[0], out.cell[2])) {
    return {EditCheck::kOutOfWorld};
  }
  const MaterialId existing = world.GetVoxel(out.cell[0], out.cell[1], out.cell[2]);
  if (Targetable(existing)) return {EditCheck::kOccupied};
  const MaterialInfo& placed = GetMaterial(out.material);
  if (placed.solid) {
    // Each player's vertical capsule axis against the placed shape's true surface, in the cell's
    // own coordinates (the closest points of a vertical segment and a polygon never need the
    // far-away absolute position).
    const ShapeInfo& shape = ShapeOf(out.material);
    for (const EditCapsule& p : players) {
      const auto x = static_cast<float>(p.center[0] - out.cell[0]);
      const auto z = static_cast<float>(p.center[2] - out.cell[2]);
      const auto y = p.center[1] - out.cell[1];
      const float bottom = static_cast<float>(y - p.half_cylinder);
      const float top = static_cast<float>(y + p.half_cylinder);
      if (VerticalSegmentDistanceSq(shape, x, z, bottom, top) < p.radius * p.radius) {
        return {EditCheck::kIntoPlayer};
      }
    }
  }
  return out;
}

void ApplyChunkChanges(VoxelWorld& world, const protocol::ChunkChanges& changes) {
  Chunk& chunk = world.GetOrCreate({changes.coord[0], changes.coord[1], changes.coord[2]});
  for (const protocol::VoxelChange& v : changes.changes) {
    if (v.index < kChunkVolume) chunk.SetAt(v.index, v.material);
  }
  chunk.SetRevision(changes.revision);
}

}  // namespace dwell::core
