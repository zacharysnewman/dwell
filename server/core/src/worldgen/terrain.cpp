#include "dwell/worldgen/terrain.h"

#include <algorithm>
#include <cstdlib>
#include <limits>
#include <vector>

#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {

using core::Chunk;
using core::ChunkCoord;
using core::kChunkSize;
using core::kSeaLevel;
using core::kWorldMaxY;
using core::MaterialId;
namespace M = core::Materials;

namespace {

// A generator works in face-local coordinates (BIFACIAL_WORLD.md §5): its ground band is
// [kFaceMinY, kWorldMaxY) with sea level 0; the face below the midplane is the other face's.
constexpr int kFaceMinY = core::kMidplaneY;

// Lattice spacing of the interpolated 2D fields (voxels).
constexpr int kLattice = 4;
constexpr float kLatticeStep = 0.25f;  // 1 / kLattice, exact

// The ground meets sea level at the coast. The land's rise inland (m, approached far inland) and
// the distance inland (m) at which it is half done.
constexpr float kInlandRise = 90.0f;
constexpr float kRiseHalf = 6'000.0f;
// The sea floor: its depth (m below sea level) offshore, reached linearly over this distance (m).
constexpr float kSeaFloor = -20.0f;
constexpr float kShelf = 2'500.0f;
// Hills on land, faded in from kHillFadeFrom to kHillFadeTo m inland: amplitude (m) and wavelength
// (m).
constexpr float kHillHeight = 30.0f;
constexpr std::int32_t kHillWavelength = 2'048;
constexpr int kHillOctaves = 4;
constexpr float kHillFadeFrom = 500.0f, kHillFadeTo = 4'000.0f;
// Small bumps everywhere: amplitude (m) and wavelength (m).
constexpr float kBumpHeight = 2.5f;
constexpr std::int32_t kBumpWavelength = 128;
constexpr int kBumpOctaves = 3;

// The fill by depth below the surface (m): sand at the shore (a top voxel lower than
// kShoreHeight above sea level, under water included) to kSandDepth, else grass over dirt to
// kDirtDepth; stone below.
constexpr int kShoreHeight = 2;
constexpr int kSandDepth = 3;
constexpr int kDirtDepth = 3;

// The top voxel of a column of height h: ceil(h) − 1.
std::int32_t TopVoxel(float h) {
  const std::int32_t f = FloorToInt(h);
  return static_cast<float>(f) == h ? f - 1 : f;
}

// The material `depth` m below the surface of a column whose top voxel is `top`.
MaterialId Fill(std::int32_t top, std::int64_t depth) {
  if (top < kSeaLevel + kShoreHeight) return depth < kSandDepth ? M::kSand : M::kStone;
  if (depth == 0) return M::kGrass;
  return depth <= kDirtDepth ? M::kDirt : M::kStone;
}

}  // namespace

TerrainGenerator::TerrainGenerator(std::uint64_t world_seed) : island_(world_seed) {
  seeds_.hills = SeedWord(world_seed, 1);
  seeds_.bumps = SeedWord(world_seed, 2);
  offset_x_ = NoiseOffset(SeedWord(world_seed, 3));
  offset_z_ = NoiseOffset(SeedWord(world_seed, 4));
}

// --- 1–2: layout and height -------------------------------------------------------------------

TerrainGenerator::Corner TerrainGenerator::SampleCorner(std::int32_t lx, std::int32_t lz) const {
  // World coordinates stay integers; noise splits them per octave (noise.h, ADR 0011).
  const std::int64_t x = std::int64_t{lx} * kLattice, z = std::int64_t{lz} * kLattice;
  const std::int64_t nx = x + offset_x_, nz = z + offset_z_;
  return {island_.CoastAt(x, z), Fbm2(seeds_.hills, nx, nz, kHillWavelength, kHillOctaves),
          Fbm2(seeds_.bumps, nx, nz, kBumpWavelength, kBumpOctaves)};
}

TerrainGenerator::Corner TerrainGenerator::SampleLod(std::int64_t x, std::int64_t z,
                                                     std::int64_t cell) const {
  const std::int64_t nx = x + offset_x_, nz = z + offset_z_;
  return {
      island_.CoastAt(x, z, OctavesResolved(island::kCoastWavelength, island::kCoastOctaves, cell)),
      Fbm2(seeds_.hills, nx, nz, kHillWavelength, kHillOctaves,
           OctavesResolved(kHillWavelength, kHillOctaves, cell)),
      Fbm2(seeds_.bumps, nx, nz, kBumpWavelength, kBumpOctaves,
           OctavesResolved(kBumpWavelength, kBumpOctaves, cell))};
}

Column TerrainGenerator::Finish(const Corner& c) {
  Column col;
  col.coast = c.coast;
  // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
  const float base = c.coast > 0.0f ? kInlandRise * (c.coast / (c.coast + kRiseHalf))
                                    : kSeaFloor * Clamp01(-c.coast * (1.0f / kShelf));
  const float land = SmoothStep(kHillFadeFrom, kHillFadeTo, c.coast);
  col.height = base + land * kHillHeight * 2.0f * c.hills + kBumpHeight * 2.0f * c.bumps;
  return col;
}

Column TerrainGenerator::Interp(const Corner (&c)[4], int fx, int fz) {
  const float tx = static_cast<float>(fx) * kLatticeStep,
              tz = static_cast<float>(fz) * kLatticeStep;
  const auto bi = [&](float Corner::*f) {
    return Lerp(Lerp(c[0].*f, c[1].*f, tx), Lerp(c[2].*f, c[3].*f, tx), tz);
  };
  return Finish({bi(&Corner::coast), bi(&Corner::hills), bi(&Corner::bumps)});
}

Column TerrainGenerator::ColumnAt(std::int32_t x, std::int32_t z) const {
  const std::int32_t lx = FloorDiv(x, kLattice), lz = FloorDiv(z, kLattice);
  const Corner c[4] = {SampleCorner(lx, lz), SampleCorner(lx + 1, lz), SampleCorner(lx, lz + 1),
                       SampleCorner(lx + 1, lz + 1)};
  Column col = Interp(c, FloorMod(x, kLattice), FloorMod(z, kLattice));
  col.outside = !core::InsideWorldDisc(x, z);
  return col;
}

bool TerrainGenerator::SolidAt(std::int32_t x, std::int32_t y, std::int32_t z) const {
  if (y < kFaceMinY || y >= kWorldMaxY) return false;
  const Column col = ColumnAt(x, z);
  return !col.outside && y <= TopVoxel(col.height);
}

std::optional<std::int32_t> TerrainGenerator::GroundY(std::int32_t x, std::int32_t z) const {
  const Column col = ColumnAt(x, z);
  if (col.outside) return std::nullopt;
  return std::clamp(TopVoxel(col.height), kFaceMinY, kWorldMaxY - 1);
}

// --- 3: chunks --------------------------------------------------------------------------------

void TerrainGenerator::ChunkColumns(std::int32_t x0, std::int32_t z0,
                                    std::vector<Column>& cols) const {
  constexpr int S = kChunkSize, nl = S / kLattice + 1;
  const std::int32_t lx0 = FloorDiv(x0, kLattice), lz0 = FloorDiv(z0, kLattice);
  std::vector<Corner> corners(static_cast<std::size_t>(nl * nl));
  for (int j = 0; j < nl; ++j)
    for (int i = 0; i < nl; ++i) corners[j * nl + i] = SampleCorner(lx0 + i, lz0 + j);
  cols.resize(S * S);
  for (int z = 0; z < S; ++z)
    for (int x = 0; x < S; ++x) {
      const int i = x / kLattice, j = z / kLattice;
      const Corner c[4] = {corners[j * nl + i], corners[j * nl + i + 1], corners[(j + 1) * nl + i],
                           corners[(j + 1) * nl + i + 1]};
      Column& col = cols[z * S + x];
      col = Interp(c, x % kLattice, z % kLattice);
      col.outside = !core::InsideWorldDisc(x0 + x, z0 + z);
    }
}

float TerrainGenerator::SkyFloor(const std::vector<Column>& cols) {
  // The highest ground of the columns inside the disc: chunks starting above it (and not below sea
  // level) are air.
  float top = -1e9f;
  for (const Column& col : cols) {
    if (!col.outside) top = std::max(top, col.height);
  }
  return top;
}

float TerrainGenerator::SkyFloorAt(std::int32_t cx, std::int32_t cz) const {
  std::vector<Column> cols;
  ChunkColumns(cx * kChunkSize, cz * kChunkSize, cols);
  return SkyFloor(cols);
}

bool TerrainGenerator::IsAirChunk(const ChunkCoord& coord, float sky_floor) {
  const std::int32_t y0 = coord.y * kChunkSize;
  if (y0 + kChunkSize <= kFaceMinY || y0 >= kWorldMaxY) return true;
  if (core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) return true;
  return y0 >= kSeaLevel && static_cast<float>(y0) > sky_floor;
}

bool TerrainGenerator::IsAirChunk(const ChunkCoord& coord) const {
  const std::int32_t y0 = coord.y * kChunkSize;
  if (y0 < kSeaLevel || y0 >= kWorldMaxY ||
      core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) {
    return IsAirChunk(coord, 0.0f);  // decided without the columns
  }
  return IsAirChunk(coord, SkyFloorAt(coord.x, coord.z));
}

void TerrainGenerator::Generate(const ChunkCoord& coord, Chunk& chunk) const {
  constexpr int S = kChunkSize;
  const std::int32_t x0 = coord.x * S, y0 = coord.y * S, z0 = coord.z * S;
  auto& voxels = chunk.generation_voxels();  // all air
  if (y0 + S <= kFaceMinY || y0 >= kWorldMaxY) return;
  // Beyond the rim of the world's disc: nothing (the rim is open air down to the midplane).
  if (core::ChunkDiscOverlap(coord.x, coord.z) == core::DiscOverlap::kOutside) return;
  std::vector<Column> cols;
  ChunkColumns(x0, z0, cols);
  // Sky above all ground and sea: all air (IsAirChunk is this same test).
  if (static_cast<float>(y0) > SkyFloor(cols) && y0 >= kSeaLevel) return;
  for (int z = 0; z < S; ++z)
    for (int x = 0; x < S; ++x) {
      const Column& col = cols[z * S + x];
      if (col.outside) continue;
      const std::int32_t top = TopVoxel(col.height);
      for (int ly = 0; ly < S; ++ly) {
        const std::int32_t y = y0 + ly;
        if (y < kFaceMinY || y >= kWorldMaxY) continue;
        MaterialId& v = voxels[core::LocalIndex(x, ly, z)];
        if (y <= top) {
          v = Fill(top, top - y);
        } else if (y < kSeaLevel) {
          v = M::kWater;
        }
      }
    }
}

// --- level of detail (§6.6) ------------------------------------------------------------------

namespace {

int LodCol(int x, int z) { return (z + 1) * core::kLodPad + x + 1; }

template <class ColumnOf>
core::LodBounds LodBoundsOf(ColumnOf&& column) {
  core::LodBounds b;
  b.lo = std::numeric_limits<double>::infinity();
  b.hi = -std::numeric_limits<double>::infinity();
  b.any_inside = false;
  bool all_inside = true;
  for (int z = -1; z <= core::kLodSectionCells; ++z)
    for (int x = -1; x <= core::kLodSectionCells; ++x) {
      const Column& col = column(x, z);
      if (col.outside) {
        all_inside = false;
        continue;
      }
      b.any_inside = true;
      // A cell is solid when its bottom voxel lies below the column's height, so samples below
      // `lo` are solid and samples at or above `hi` are open (air or sea).
      b.hi = std::max(b.hi, static_cast<double>(col.height) + 1.0);
      b.lo = std::min(b.lo, static_cast<double>(col.height) - 1.0);
    }
  if (!b.any_inside) return b;
  b.hi = std::max(b.hi, static_cast<double>(kSeaLevel - 1));
  if (!all_inside) b.lo = -std::numeric_limits<double>::infinity();
  return b;
}

}  // namespace

Column TerrainGenerator::ColumnLod(std::int64_t x, std::int64_t z, std::int64_t cell) const {
  Column col = Finish(SampleLod(x, z, cell));
  col.outside = !core::InsideWorldDisc64(x, z);
  return col;
}

core::LodBounds TerrainGenerator::LodBoundsAt(int level, std::int32_t i, std::int32_t k) const {
  const core::LodOrigin o = core::LodSectionOrigin({level, i, 0, k});
  const std::int64_t cell = core::LodCellSize(level);
  std::vector<Column> cols(static_cast<std::size_t>(core::kLodPad * core::kLodPad));
  for (int z = -1; z <= core::kLodSectionCells; ++z)
    for (int x = -1; x <= core::kLodSectionCells; ++x) {
      cols[static_cast<std::size_t>(LodCol(x, z))] =
          ColumnLod(o.x + x * cell + cell / 2, o.z + z * cell + cell / 2, cell);
    }
  return LodBoundsOf(
      [&](int x, int z) -> const Column& { return cols[static_cast<std::size_t>(LodCol(x, z))]; });
}

core::LodKind TerrainGenerator::GenerateLod(const core::LodCoord& c, core::LodCells& cells,
                                            core::LodSurfaces* surface) const {
  if (!core::LodInWorld(c)) {
    cells.assign(core::kLodVolume, M::kAir);
    if (surface) surface->assign(static_cast<std::size_t>(core::kLodPad * core::kLodPad), {});
    return core::LodKind::kEmpty;
  }
  return GenerateLodAt(c, core::LodSectionOrigin(c).y, cells, surface);
}

core::LodKind TerrainGenerator::GenerateLodAt(const core::LodCoord& c, std::int64_t origin_y,
                                              core::LodCells& cells, core::LodSurfaces* surface,
                                              core::LodBounds* bounds_out) const {
  using core::kLodSectionCells;
  using core::LodCell;
  cells.assign(core::kLodVolume, M::kAir);
  if (surface) surface->assign(static_cast<std::size_t>(core::kLodPad * core::kLodPad), {});
  core::LodOrigin o = core::LodSectionOrigin(c);
  o.y = origin_y;
  const std::int64_t cell = core::LodCellSize(c.level);
  const std::int64_t half = cell / 2;

  // 1–2. The height field at each column's centre.
  std::vector<Column> cols(static_cast<std::size_t>(core::kLodPad * core::kLodPad));
  for (int z = -1; z <= kLodSectionCells; ++z)
    for (int x = -1; x <= kLodSectionCells; ++x) {
      cols[static_cast<std::size_t>(LodCol(x, z))] =
          ColumnLod(o.x + x * cell + half, o.z + z * cell + half, cell);
    }
  const auto column = [&](int x, int z) -> const Column& {
    return cols[static_cast<std::size_t>(LodCol(x, z))];
  };
  const core::LodBounds bounds = LodBoundsOf(column);
  if (bounds_out) *bounds_out = bounds;
  const core::LodKind kind = bounds.any_inside
                                 ? core::LodKindOfFace(c.level, origin_y, bounds.lo, bounds.hi)
                                 : core::LodKind::kEmpty;
  if (kind == core::LodKind::kEmpty) return kind;
  if (kind == core::LodKind::kBuried) {
    for (int y = -1; y <= kLodSectionCells; ++y) {
      std::fill_n(cells.begin() + LodCell(-1, y, -1), core::kLodPad * core::kLodPad, M::kStone);
    }
    return kind;
  }

  // 3. Cells per column: a cell is solid when its bottom voxel is, and takes the fill at the depth
  // of its top voxel below the surface; open cells below sea level are water.
  for (int z = -1; z <= kLodSectionCells; ++z)
    for (int x = -1; x <= kLodSectionCells; ++x) {
      const Column& col = column(x, z);
      if (col.outside) continue;  // beyond the rim: the void
      const std::int32_t top = TopVoxel(col.height);
      const bool sea = col.height < static_cast<float>(kSeaLevel);
      for (int y = -1; y <= kLodSectionCells; ++y) {
        const std::int64_t a = o.y + y * cell;
        MaterialId& m = cells[static_cast<std::size_t>(LodCell(x, y, z))];
        if (a + cell <= kFaceMinY) {
          m = M::kStone;  // the apron under the face
          continue;
        }
        // A cell reaching below the face (coarse cells over the midplane) samples at the face's
        // lowest voxel, the solid floor.
        const std::int64_t at = std::max<std::int64_t>(a, kFaceMinY);
        if (at > top) {
          if (at < kSeaLevel) m = M::kWater;
          continue;
        }
        const std::int64_t depth = std::max<std::int64_t>(0, top - (a + cell - 1));
        if (depth > 0) {
          m = Fill(top, std::min<std::int64_t>(depth, 1000));
          continue;
        }
        // The top of the column: the material of its surface as seen from above. A sea whose water
        // no cell sampled (the cell above is not below sea level: this cell is deeper than the sea)
        // shows its water, as Downsample keeps it.
        const bool water_above = a + cell < kSeaLevel;
        m = sea && !water_above ? M::kWater : Fill(top, 0);
        // The column's surface, exactly: the ground's height (a sea's floor, whose cell may have
        // been drawn as water), which lies in this cell.
        if (surface) {
          core::LodSurface& surf = (*surface)[static_cast<std::size_t>(LodCol(x, z))];
          surf.valid = true;
          surf.wet = sea;
          surf.height = std::min(col.height, static_cast<float>(a + cell));
          surf.material = Fill(top, 0);
          surf.water = sea ? static_cast<float>(kSeaLevel) : 0.0f;
        }
      }
    }
  return kind;
}

// --- spawn ----------------------------------------------------------------------------------

std::array<double, 3> TerrainGenerator::SpawnPoint() const {
  std::optional<std::array<double, 3>> fallback;
  // A square spiral outwards from the origin in 8 m steps, for dry level ground: a patch of columns
  // kSpawnClear either side whose top voxels are all at one height above the shore.
  std::int32_t x = 0, z = 0, dx = 1, dz = 0, leg = 1, walked = 0, turns = 0;
  for (int n = 0; n < 40000; ++n) {
    const auto g = GroundY(x, z);
    if (g && *g >= kSeaLevel + kShoreHeight) {
      if (!fallback) fallback = std::array<double, 3>{x + 0.5, *g + 1.0, z + 0.5};
      bool level = true;
      for (std::int32_t pz = -kSpawnClear; level && pz <= kSpawnClear; ++pz)
        for (std::int32_t px = -kSpawnClear; level && px <= kSpawnClear; ++px)
          level = GroundY(x + px, z + pz) == g;
      if (level) return {x + 0.5, *g + 1.0, z + 0.5};
    }
    x += dx * 8;
    z += dz * 8;
    if (++walked == leg) {
      walked = 0;
      const std::int32_t t = dx;
      dx = -dz;
      dz = t;
      if (++turns % 2 == 0) ++leg;
    }
  }
  return fallback.value_or(std::array<double, 3>{0.5, kSeaLevel + 1.0, 0.5});
}

}  // namespace dwell::worldgen
