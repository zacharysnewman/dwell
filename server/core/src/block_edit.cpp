#include "dwell/core/block_edit.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace dwell::core {
namespace {

constexpr float kInf = std::numeric_limits<float>::infinity();

// Height of a cell's targeting box (slabs: the bottom half).
float BoxHeight(MaterialId m) {
  return GetMaterial(m).shape == VoxelShape::kSlabBottom ? 0.5f : 1.0f;
}

// Ray (origin o, direction d; relative coordinates) against the box [lo, hi]: entry distance and
// the axis entered through, or nothing. A box containing the origin is not hit.
std::optional<std::pair<float, int>> EnterBox(const float (&o)[3], const float (&d)[3],
                                              const float (&lo)[3], const float (&hi)[3]) {
  float t_enter = -kInf, t_exit = kInf;
  int axis = -1;
  for (int i = 0; i < 3; ++i) {
    if (d[i] == 0.0f) {
      if (o[i] < lo[i] || o[i] > hi[i]) return std::nullopt;
      continue;
    }
    float t0 = (lo[i] - o[i]) / d[i], t1 = (hi[i] - o[i]) / d[i];
    if (t0 > t1) std::swap(t0, t1);
    if (t0 > t_enter) {
      t_enter = t0;
      axis = i;
    }
    t_exit = std::min(t_exit, t1);
  }
  if (axis < 0 || t_enter > t_exit || t_enter < 0.0f) return std::nullopt;
  return std::make_pair(t_enter, axis);
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
      const float lo[3] = {static_cast<float>(cell[0]), static_cast<float>(cell[1]),
                           static_cast<float>(cell[2])};
      const float hi[3] = {lo[0] + 1.0f, lo[1] + BoxHeight(m), lo[2] + 1.0f};
      if (const auto hit = EnterBox(o, d, lo, hi); hit && hit->first <= max_distance) {
        const int axis = hit->second;
        BlockHit out;
        for (int i = 0; i < 3; ++i) out.cell[i] = base[i] + cell[i];
        out.face = axis * 2 + (d[axis] > 0.0f ? 1 : 0);  // entering +X-going: the −X face
        out.distance = hit->first;
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

  // Line of sight: the eye is in front of the targeted face, and a ray from it reaches the target
  // cell first at the face's centre or one of four points near its corners.
  const auto& n = kFaceDirs[request.face];
  const int axis = request.face / 2;
  const float height = BoxHeight(target);
  const double size[3] = {1.0, height, 1.0};
  const double plane = c[axis] + (n[axis] > 0 ? size[axis] : 0.0);
  if ((eye[axis] - plane) * n[axis] <= 0.0) return {EditCheck::kNoLineOfSight};
  const int u = (axis + 1) % 3, v = (axis + 2) % 3;
  constexpr double kSamples[5][2] = {
      {0.5, 0.5}, {0.15, 0.15}, {0.85, 0.15}, {0.15, 0.85}, {0.85, 0.85}};
  bool visible = false;
  for (const auto& s : kSamples) {
    std::array<double, 3> p;
    p[axis] = plane + 0.01 * n[axis];
    p[u] = c[u] + s[0] * size[u];
    p[v] = c[v] + s[1] * size[v];
    std::array<double, 3> delta{p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]};
    const double length =
        std::sqrt(delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]);
    if (length <= 0.0) continue;
    const std::array<float, 3> dir{static_cast<float>(delta[0] / length),
                                   static_cast<float>(delta[1] / length),
                                   static_cast<float>(delta[2] / length)};
    // On past the point: a grazing ray needs a while to cross the 1 cm to the face, and past the
    // plane it can only enter the target (the point lies well inside the face).
    const auto hit = RaycastBlock(world, eye, dir, static_cast<float>(length) + 1.0f);
    if (hit && hit->cell == c) {
      visible = true;
      break;
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
    // Vertical capsule segment against the placed shape's box (exact: the closest points separate
    // into the horizontal and vertical directions).
    const double lo[3] = {static_cast<double>(out.cell[0]), static_cast<double>(out.cell[1]),
                          static_cast<double>(out.cell[2])};
    const double hi[3] = {lo[0] + 1.0, lo[1] + ShapeHeight(placed.shape), lo[2] + 1.0};
    for (const EditCapsule& p : players) {
      const auto gap = [](double v, double a, double b) {
        return v < a ? a - v : v > b ? v - b : 0.0;
      };
      const double dx = gap(p.center[0], lo[0], hi[0]);
      const double dz = gap(p.center[2], lo[2], hi[2]);
      const double bottom = p.center[1] - p.half_cylinder, top = p.center[1] + p.half_cylinder;
      const double dy = top < lo[1] ? lo[1] - top : bottom > hi[1] ? bottom - hi[1] : 0.0;
      if (dx * dx + dy * dy + dz * dz < static_cast<double>(p.radius) * p.radius) {
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
