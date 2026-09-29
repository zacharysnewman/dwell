#include "dwell/core/lod.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>

#include "dwell/worldgen/terrain.h"

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
  if (c.j < 0 || c.j >= LodRows(c.level)) return false;
  // The section's square against the disc: its point nearest the origin.
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t s = LodSectionSize(c.level);
  const auto nearest = [](std::int64_t lo, std::int64_t hi) -> std::int64_t {
    return lo > 0 ? lo : hi - 1 < 0 ? hi - 1 : 0;
  };
  return InsideWorldDisc64(nearest(o.x, o.x + s), nearest(o.z, o.z + s));
}

bool LodSolid(MaterialId m) { return m != M::kAir && !GetMaterial(m).liquid; }

MaterialId DownsampleBlock(const MaterialId (&cells)[8]) {
  int solid = 0, liquid = 0;
  for (const MaterialId m : cells) {
    if (LodSolid(m)) {
      ++solid;
    } else if (m != M::kAir) {
      ++liquid;
    }
  }
  const bool want_solid = solid >= 4;
  if (!want_solid && liquid < 4) return M::kAir;
  const auto qualifies = [&](MaterialId m) {
    return want_solid ? LodSolid(m) : (m != M::kAir && !LodSolid(m));
  };
  // The top qualifying cell of each column; upper candidates first (ties go to them).
  MaterialId candidates[4];
  int n = 0;
  for (int pass = 0; pass < 2; ++pass) {
    for (int col = 0; col < 4; ++col) {
      const int dx = col & 1, dz = col >> 1;
      const MaterialId top = cells[dx | 2 | dz << 2], bottom = cells[dx | dz << 2];
      const bool from_upper = qualifies(top);
      if (from_upper != (pass == 0)) continue;
      if (!from_upper && !qualifies(bottom)) continue;
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

void DownsampleOctant(const std::function<MaterialId(int, int, int)>& child_at, int octant,
                      LodCells& parent) {
  constexpr int H = kLodSectionCells / 2;
  const int ox = (octant & 1) * H, oy = ((octant >> 1) & 1) * H, oz = ((octant >> 2) & 1) * H;
  MaterialId block[8];
  for (int y = 0; y < H; ++y)
    for (int z = 0; z < H; ++z)
      for (int x = 0; x < H; ++x) {
        for (int b = 0; b < 8; ++b) {
          block[b] = child_at(2 * x + (b & 1), 2 * y + ((b >> 1) & 1), 2 * z + ((b >> 2) & 1));
        }
        parent[static_cast<std::size_t>(LodCell(ox + x, oy + y, oz + z))] = DownsampleBlock(block);
      }
}

void DownsampleChunkOctant(const Chunk& chunk, int octant, LodCells& parent) {
  const auto& v = chunk.voxels();
  DownsampleOctant(
      [&](int x, int y, int z) { return v[static_cast<std::size_t>(LocalIndex(x, y, z))]; }, octant,
      parent);
}

void DownsampleSectionOctant(const LodCells& child, int octant, LodCells& parent) {
  DownsampleOctant(
      [&](int x, int y, int z) { return child[static_cast<std::size_t>(LodCell(x, y, z))]; },
      octant, parent);
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

LodKind LodKindFromBounds(const LodCoord& c, const LodBounds& b) {
  if (!LodInWorld(c) || !b.any_inside) return LodKind::kEmpty;
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t cell = LodCellSize(c.level);
  // Cell rows −1..32 (with the apron) sample at their bottom voxel.
  const double lowest = static_cast<double>(o.y - cell);
  const double highest = static_cast<double>(o.y + kLodSectionCells * cell);
  if (lowest > b.hi) return LodKind::kEmpty;
  if (highest < b.lo) return LodKind::kBuried;
  return LodKind::kContent;
}

namespace {

// Surface strata of the flat world by depth below the top (m): grass, three of dirt, stone.
MaterialId FlatMaterial(std::int64_t y, std::int64_t depth_m) {
  if (y < kWorldMinY + kBedrockLayers) return M::kBedrock;
  return depth_m == 0 ? M::kGrass : depth_m < 4 ? M::kDirt : M::kStone;
}

// Fills a buried section: stone, bedrock at the bottom of the world.
void FillBuried(const LodCoord& c, LodCells& cells) {
  const LodOrigin o = LodSectionOrigin(c);
  const std::int64_t cell = LodCellSize(c.level);
  for (int y = -1; y <= kLodSectionCells; ++y) {
    const std::int64_t a = o.y + y * cell;
    const MaterialId m = a < kWorldMinY + kBedrockLayers ? M::kBedrock : M::kStone;
    std::fill_n(cells.begin() + LodCell(-1, y, -1), kLodPad * kLodPad, m);
  }
}

}  // namespace

LodBounds FlatLodBounds(int level, std::int32_t i, std::int32_t k) {
  // Solid below y = 0 inside the disc.
  const LodCoord c{level, i, 0, k};
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
        if (a < kWorldMinY) {
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
    auto terrain = std::make_shared<worldgen::TerrainGenerator>(world_seed);
    return [terrain](const LodCoord& c, LodCells& cells) { return terrain->GenerateLod(c, cells); };
  }
  return GenerateFlatLod;
}

LodBoundsFn LodBoundsFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    auto terrain = std::make_shared<worldgen::TerrainGenerator>(world_seed);
    return [terrain](int level, std::int32_t i, std::int32_t k) {
      return terrain->LodBoundsAt(level, i, k);
    };
  }
  return FlatLodBounds;
}

}  // namespace dwell::core
