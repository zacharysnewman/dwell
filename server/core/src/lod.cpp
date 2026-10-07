#include "dwell/core/lod.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>

#include "dwell/worldgen/bifacial.h"

namespace dwell::core {

namespace M = Materials;

std::size_t LodCoordHash::operator()(const LodCoord& c) const noexcept {
  std::uint64_t h = static_cast<std::uint32_t>(c.i);
  h = h * 0x9E3779B97F4A7C15ull ^ static_cast<std::uint32_t>(c.j);
  h = h * 0x9E3779B97F4A7C15ull ^ static_cast<std::uint32_t>(c.k);
  h = h * 0x9E3779B97F4A7C15ull ^ static_cast<std::uint32_t>(c.level);
  return static_cast<std::size_t>(h ^ (h >> 29));
}

LodOrigin LodSectionOrigin(const LodCoord& c) {
  const std::int64_t s = LodSectionSize(c.level);
  return {kLodOriginX + c.i * s, kLodOriginY + c.j * s, kLodOriginZ + c.k * s};
}

LodCoord LodChild(const LodCoord& c, int octant) {
  return {c.level - 1, c.i * 2 + (octant & 1), c.j * 2 + ((octant >> 1) & 1),
          c.k * 2 + ((octant >> 2) & 1)};
}

LodCoord LodParent(const LodCoord& c) { return {c.level + 1, c.i >> 1, c.j >> 1, c.k >> 1}; }

LodCoord LodAncestor(const LodCoord& c, int level) {
  const int d = level - c.level;
  return {level, c.i >> d, c.j >> d, c.k >> d};
}

namespace {
constexpr std::int32_t kChunkOffsetXZ = static_cast<std::int32_t>(-kLodOriginX / kChunkSize);
constexpr std::int32_t kChunkOffsetY = static_cast<std::int32_t>(-kLodOriginY / kChunkSize);
static_assert(kChunkOffsetY == (std::int32_t{1} << 18));
}  // namespace

LodCoord LodOfChunk(const ChunkCoord& c) {
  return {0, c.x + kChunkOffsetXZ, c.y + kChunkOffsetY, c.z + kChunkOffsetXZ};
}

ChunkCoord ChunkOfLod(const LodCoord& c) {
  return {c.i - kChunkOffsetXZ, c.j - kChunkOffsetY, c.k - kChunkOffsetXZ};
}

bool LodInWorld(const LodCoord& c) {
  if (c.level < 0 || c.level > kLodMaxLevel) return false;
  const std::int64_t across = LodSectionsAcross(c.level);
  if (c.i < 0 || c.k < 0 || c.i >= across || c.k >= across) return false;
  if (c.j < LodFirstRow(c.level) || c.j > LodLastRow(c.level)) return false;
  // The section's square against the disc: its point nearest the origin.
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t s = LodSectionSize(c.level);
  const auto nearest = [](std::int64_t lo, std::int64_t hi) -> std::int64_t {
    return lo > 0 ? lo : hi - 1 < 0 ? hi - 1 : 0;
  };
  return InsideWorldDisc64(nearest(o.x, o.x + s), nearest(o.z, o.z + s));
}

bool LodSolid(MaterialId m) { return m != M::kAir && !GetMaterial(m).liquid; }

MaterialId DownsampleBlock(const MaterialId (&cells)[8], Face face) {
  if (face == Face::kB) {
    // Face B's surface faces down: the same rule with the block turned over (dy flipped).
    MaterialId flipped[8];
    for (int b = 0; b < 8; ++b) flipped[b] = cells[b ^ 2];
    return DownsampleBlock(flipped);
  }
  return DownsampleBlock(cells);
}

MaterialId DownsampleBlock(const MaterialId (&cells)[8]) {
  // Filled (solid or liquid) at 4 of 8: liquids count, so a sea keeps its surface at levels
  // whose cells are deeper than the sea, instead of showing its floor.
  int filled = 0;
  for (const MaterialId m : cells) filled += m != M::kAir;
  if (filled < 4) return M::kAir;
  // The top filled cell of each column; upper candidates first (ties go to them).
  MaterialId candidates[4];
  int n = 0;
  for (int pass = 0; pass < 2; ++pass) {
    for (int col = 0; col < 4; ++col) {
      const int dx = col & 1, dz = col >> 1;
      const MaterialId top = cells[dx | 2 | dz << 2], bottom = cells[dx | dz << 2];
      const bool from_upper = top != M::kAir;
      if (from_upper != (pass == 0)) continue;
      if (!from_upper && bottom == M::kAir) continue;
      candidates[n++] = from_upper ? top : bottom;
    }
  }
  MaterialId best = candidates[0];
  int best_count = 0;
  for (int a = 0; a < n; ++a) {
    int count = 0;
    for (int b = 0; b < n; ++b) count += candidates[b] == candidates[a];
    if (count > best_count) {
      best = candidates[a];
      best_count = count;
    }
  }
  return best;
}

void DownsampleOctant(const std::function<MaterialId(int, int, int)>& child_at,
                      const LodCoord& parent_coord, int octant, LodCells& parent) {
  constexpr int H = kLodSectionCells / 2;
  const int ox = (octant & 1) * H, oy = ((octant >> 1) & 1) * H, oz = ((octant >> 2) & 1) * H;
  MaterialId block[8];
  for (int y = 0; y < H; ++y) {
    const Face face = LodRowFace(parent_coord, oy + y);
    for (int z = 0; z < H; ++z)
      for (int x = 0; x < H; ++x) {
        for (int b = 0; b < 8; ++b) {
          block[b] = child_at(2 * x + (b & 1), 2 * y + ((b >> 1) & 1), 2 * z + ((b >> 2) & 1));
        }
        parent[static_cast<std::size_t>(LodCell(ox + x, oy + y, oz + z))] =
            DownsampleBlock(block, face);
      }
  }
}

void DownsampleChunkOctant(const Chunk& chunk, const LodCoord& parent_coord, int octant,
                           LodCells& parent) {
  const auto& v = chunk.voxels();
  DownsampleOctant(
      [&](int x, int y, int z) { return v[static_cast<std::size_t>(LocalIndex(x, y, z))]; },
      parent_coord, octant, parent);
}

void DownsampleSectionOctant(const LodCells& child, const LodCoord& parent_coord, int octant,
                             LodCells& parent) {
  DownsampleOctant(
      [&](int x, int y, int z) { return child[static_cast<std::size_t>(LodCell(x, y, z))]; },
      parent_coord, octant, parent);
}

std::uint64_t LodHash(LodKind kind, const LodCells& cells) {
  std::uint64_t h = 0xcbf29ce484222325ull;
  const auto byte = [&](std::uint8_t b) {
    h ^= b;
    h *= 0x100000001b3ull;
  };
  byte(static_cast<std::uint8_t>(kind));
  for (const MaterialId m : cells) {
    byte(static_cast<std::uint8_t>(m));
    byte(static_cast<std::uint8_t>(m >> 8));
  }
  return h;
}

LodKind LodKindOfFace(int level, std::int64_t origin_y, double lo, double hi) {
  const std::int64_t cell = LodCellSize(level);
  // Cell rows −1..32 (with the apron) sample at their bottom voxel.
  const double lowest = static_cast<double>(origin_y - cell);
  const double highest = static_cast<double>(origin_y + kLodSectionCells * cell);
  if (lowest > hi) return LodKind::kEmpty;
  if (highest < lo) return LodKind::kBuried;
  return LodKind::kContent;
}

LodKind LodKindFromBounds(const LodCoord& c, const LodBounds& b) {
  if (!LodInWorld(c) || !b.any_inside) return LodKind::kEmpty;
  if (!b.bifacial) return LodKindOfFace(c.level, LodSectionOrigin(c).y, b.lo, b.hi);
  const std::int64_t cell = LodCellSize(c.level);
  // Rows −1..32 split at the midplane: face B owns the lower ones.
  int first_a = kLodSectionCells + 1;  // the lowest row of face A (33: none)
  for (int r = -1; r <= kLodSectionCells; ++r) {
    if (LodRowFace(c, r) == Face::kA) {
      first_a = r;
      break;
    }
  }
  const std::int64_t y0 = LodSectionOrigin(c).y;
  bool empty = true, buried = true;
  LodKind kinds[2] = {LodKind::kEmpty, LodKind::kEmpty};
  int have = 0;
  if (first_a <= kLodSectionCells) {
    // Face A's rows first_a..32: the lowest sampled voxel is that of row first_a.
    const double lowest = static_cast<double>(y0 + first_a * cell);
    const double highest = static_cast<double>(y0 + kLodSectionCells * cell);
    kinds[have++] = lowest > b.hi    ? LodKind::kEmpty
                    : highest < b.lo ? LodKind::kBuried
                                     : LodKind::kContent;
  }
  if (first_a >= 0) {
    // Face B's rows −1..first_a − 1, mirrored: its face-local rows run from the mirror of the
    // topmost B row's cell up to the mirror of row −1.
    const int top_row = std::min(first_a - 1, kLodSectionCells);
    const double lowest = static_cast<double>(LodMirrorRowBottom(y0 + top_row * cell, cell));
    const double highest = static_cast<double>(LodMirrorRowBottom(y0 - cell, cell));
    kinds[have++] = lowest > b.hi_b    ? LodKind::kEmpty
                    : highest < b.lo_b ? LodKind::kBuried
                                       : LodKind::kContent;
  }
  for (int i = 0; i < have; ++i) {
    if (kinds[i] != LodKind::kEmpty) empty = false;
    if (kinds[i] != LodKind::kBuried) buried = false;
  }
  if (empty) return LodKind::kEmpty;
  if (buried) return LodKind::kBuried;
  return LodKind::kContent;
}

namespace {

// Surface strata of the flat world by depth below the top (m): grass, three of dirt, stone.
MaterialId FlatMaterial(std::int64_t y, std::int64_t depth_m) {
  if (y < kMidplaneY + kBedrockLayers) return M::kBedrock;
  return depth_m == 0 ? M::kGrass : depth_m < 4 ? M::kDirt : M::kStone;
}

// Fills a buried section: stone, bedrock at the bottom of the world.
void FillBuried(const LodCoord& c, LodCells& cells) {
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t cell = LodCellSize(c.level);
  for (int y = -1; y <= kLodSectionCells; ++y) {
    const std::int64_t a = o.y + y * cell;
    const MaterialId m = a < kMidplaneY + kBedrockLayers ? M::kBedrock : M::kStone;
    std::fill_n(cells.begin() + LodCell(-1, y, -1), kLodPad * kLodPad, m);
  }
}

}  // namespace

LodBounds FlatLodBounds(int level, std::int32_t i, std::int32_t k) {
  // Solid below y = 0 inside the disc.
  const LodCoord c{level, i, LodFirstRow(level), k};
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t cell = LodCellSize(level), s = LodSectionSize(level);
  // The disc is convex: the corners of the section and its apron decide "all inside".
  bool all_inside = true;
  for (const std::int64_t x : {o.x - cell, o.x + s + cell})
    for (const std::int64_t z : {o.z - cell, o.z + s + cell})
      all_inside = all_inside && InsideWorldDisc64(x, z);
  LodBounds b;
  b.hi = -1;
  b.lo = all_inside ? 0 : -std::numeric_limits<double>::infinity();
  b.bifacial = false;
  b.any_inside = LodInWorld(c);
  return b;
}

LodKind GenerateFlatLod(const LodCoord& c, LodCells& cells) {
  cells.assign(kLodVolume, M::kAir);
  const LodKind kind = LodKindFromBounds(c, FlatLodBounds(c.level, c.i, c.k));
  if (kind == LodKind::kEmpty) return kind;
  if (kind == LodKind::kBuried) {
    FillBuried(c, cells);
    return kind;
  }
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t cell = LodCellSize(c.level);
  for (int z = -1; z <= kLodSectionCells; ++z)
    for (int x = -1; x <= kLodSectionCells; ++x) {
      if (!InsideWorldDisc64(o.x + x * cell + cell / 2, o.z + z * cell + cell / 2)) continue;
      for (int y = -1; y <= kLodSectionCells; ++y) {
        const std::int64_t a = o.y + y * cell;
        MaterialId m = M::kAir;
        if (a < kMidplaneY) {
          m = M::kBedrock;  // below the world: the apron reads solid, so the floor is not drawn
        } else if (a < 0) {
          // The top solid cell starts at or below −1; its depth in metres below the surface.
          m = FlatMaterial(a, -cell - a);
        }
        cells[static_cast<std::size_t>(LodCell(x, y, z))] = m;
      }
    }
  return kind;
}

LodGenerator LodGeneratorFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    auto terrain = std::make_shared<worldgen::BifacialTerrain>(world_seed);
    return [terrain](const LodCoord& c, LodCells& cells) { return terrain->GenerateLod(c, cells); };
  }
  return GenerateFlatLod;
}

LodBoundsFn LodBoundsFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    auto terrain = std::make_shared<worldgen::BifacialTerrain>(world_seed);
    return [terrain](int level, std::int32_t i, std::int32_t k) {
      return terrain->LodBoundsAt(level, i, k);
    };
  }
  return FlatLodBounds;
}

}  // namespace dwell::core
