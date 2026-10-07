#include "dwell/worldgen/bifacial.h"

#include <algorithm>
#include <memory>

#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {

using core::Chunk;
using core::ChunkCoord;
using core::Face;
using core::kChunkSize;

std::uint64_t FaceSeed(std::uint64_t world_seed, Face face) {
  if (face == Face::kA) return world_seed;
  // Two independent words from streams no generator stage uses (the generator's own are 1..~30).
  const std::uint64_t lo = SeedWord(world_seed, 0xFACEB001u);
  const std::uint64_t hi = SeedWord(world_seed, 0xFACEB002u);
  return (hi << 32) | lo;
}

BifacialTerrain::BifacialTerrain(std::uint64_t world_seed)
    : a_(FaceSeed(world_seed, Face::kA)), b_(FaceSeed(world_seed, Face::kB)) {}

void BifacialTerrain::Generate(const ChunkCoord& coord, Chunk& chunk, std::uint8_t stages) const {
  if (core::FaceOfChunkY(coord.y) == Face::kA) {
    a_.Generate(coord, chunk, stages);
    return;
  }
  // Face B: the face-local chunk, flipped. Row ly of the local chunk is row S − 1 − ly of this one
  // (core::MirrorY maps the voxel; the chunk rows pair up exactly).
  const ChunkCoord local = core::FaceLocalChunk(coord);
  auto scratch = std::make_unique<Chunk>();
  b_.Generate(local, *scratch, stages);
  const auto& src = scratch->voxels();
  auto& dst = chunk.generation_voxels();
  constexpr int S = kChunkSize;
  for (int z = 0; z < S; ++z)
    for (int y = 0; y < S; ++y)
      for (int x = 0; x < S; ++x) {
        dst[static_cast<std::size_t>(core::LocalIndex(x, S - 1 - y, z))] =
            core::MirrorMaterial(src[static_cast<std::size_t>(core::LocalIndex(x, y, z))]);
      }
}

core::LodBounds BifacialTerrain::LodBoundsAt(int level, std::int32_t i, std::int32_t k) const {
  const core::LodBounds a = a_.LodBoundsAt(level, i, k), b = b_.LodBoundsAt(level, i, k);
  core::LodBounds out;
  out.lo = a.lo;
  out.hi = a.hi;
  out.lo_b = b.lo;
  out.hi_b = b.hi;
  out.any_inside = a.any_inside || b.any_inside;
  return out;
}

core::LodKind BifacialTerrain::GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                                           core::LodSurfaces* surface,
                                           core::LodSurfaces* surface_b) const {
  using core::kLodSectionCells;
  using core::LodCell;
  cells.assign(core::kLodVolume, core::Materials::kAir);
  if (surface) surface->assign(static_cast<std::size_t>(core::kLodPad * core::kLodPad), {});
  if (surface_b) surface_b->assign(static_cast<std::size_t>(core::kLodPad * core::kLodPad), {});
  if (!core::LodInWorld(c)) return core::LodKind::kEmpty;
  const std::int64_t y0 = core::LodSectionOrigin(c).y;
  // Rows −1..32 split at the midplane: face B owns rows below first_a, face A the rest.
  int first_a = kLodSectionCells + 1;
  for (int r = -1; r <= kLodSectionCells; ++r) {
    if (core::LodRowFace(c, r) == Face::kA) {
      first_a = r;
      break;
    }
  }
  core::LodBounds bounds;
  core::LodCells local;
  const auto columns = [&](int row_lo, int row_hi, auto&& row_of) {
    for (int r = row_lo; r <= row_hi; ++r) {
      const int from = row_of(r);
      for (int z = -1; z <= kLodSectionCells; ++z)
        for (int x = -1; x <= kLodSectionCells; ++x) {
          cells[static_cast<std::size_t>(LodCell(x, r, z))] =
              local[static_cast<std::size_t>(LodCell(x, from, z))];
        }
    }
  };
  if (first_a <= kLodSectionCells) {
    core::LodBounds b;
    a_.GenerateLodAt(c, y0, local, surface, &b);
    bounds.lo = b.lo;
    bounds.hi = b.hi;
    bounds.any_inside = b.any_inside;
    columns(first_a, kLodSectionCells, [](int r) { return r; });
  }
  if (first_a >= 0) {
    // Face-local row r' is world row 31 − r': the section's mirror image starts at −4,096 − y0 − S.
    const std::int64_t origin_l = -4096 - y0 - core::LodSectionSize(c.level);
    core::LodBounds b;
    b_.GenerateLodAt(c, origin_l, local, surface_b, &b);
    bounds.lo_b = b.lo;
    bounds.hi_b = b.hi;
    bounds.any_inside = bounds.any_inside || b.any_inside;
    columns(-1, std::min(first_a - 1, kLodSectionCells),
            [](int r) { return kLodSectionCells - 1 - r; });
  }
  const core::LodKind kind = core::LodKindFromBounds(c, bounds);
  // A section the bounds decide is canonical whatever the passes generated.
  if (kind == core::LodKind::kEmpty) {
    cells.assign(core::kLodVolume, core::Materials::kAir);
  } else if (kind == core::LodKind::kBuried) {
    cells.assign(core::kLodVolume, core::Materials::kStone);
  }
  return kind;
}

bool BifacialTerrain::IsAirChunk(const ChunkCoord& coord) const {
  const Face face = core::FaceOfChunkY(coord.y);
  return Local(face).IsAirChunk(core::FaceLocalChunk(coord));
}

bool BifacialTerrain::SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const {
  return core::FaceOfY(y) == Face::kA ? a_.SolidAt(x, y, z) : b_.SolidAt(x, core::MirrorY(y), z);
}

}  // namespace dwell::worldgen
