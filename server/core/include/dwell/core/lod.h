#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <vector>

#include "dwell/core/voxel.h"
#include "dwell/protocol/constants.gen.h"

// Level of detail: the whole-world view (ARCHITECTURE.md §6.6, ADR 0012). A 3D octree of sections:
// at level L a cell is a 2^L m cube and a section is 32³ cells, so level 0 is the chunk grid.
// Section coordinates (L, i, j, k) count from the corner (−2²³, WORLD_MIN_Y, −2²³); one root at
// LOD_MAX_LEVEL holds the whole disc. Section content is 34³ materials: the section and a one-cell
// apron from its neighbours, in layer order (x fastest, then z, then y) — the order the palette +
// RLE codec takes cells in, and the client mesher's padded layout.
namespace dwell::core {

inline constexpr int kLodSectionCells = protocol::kLodSectionCells;
inline constexpr int kLodMaxLevel = protocol::kLodMaxLevel;
inline constexpr int kLodIndexLevel = protocol::kLodIndexLevel;
inline constexpr int kLodPad = kLodSectionCells + 2;
inline constexpr int kLodVolume = kLodPad * kLodPad * kLodPad;
inline constexpr std::int64_t kLodOriginX = -(std::int64_t{1} << 23);
inline constexpr std::int64_t kLodOriginY = kWorldMinY;
inline constexpr std::int64_t kLodOriginZ = -(std::int64_t{1} << 23);

struct LodCoord {
  int level = 0;
  std::int32_t i = 0, j = 0, k = 0;
  bool operator==(const LodCoord&) const = default;
};

struct LodCoordHash {
  std::size_t operator()(const LodCoord& c) const noexcept;
};

// Index of cell (x, y, z) of a section's content, each in −1..32 (−1 and 32: the apron).
inline constexpr int LodCell(int x, int y, int z) {
  return (x + 1) + kLodPad * ((z + 1) + kLodPad * (y + 1));
}

inline constexpr std::int64_t LodCellSize(int level) { return std::int64_t{1} << level; }
inline constexpr std::int64_t LodSectionSize(int level) {
  return std::int64_t{kLodSectionCells} << level;
}
// Sections of a level along x and z (the root level has one).
inline constexpr std::int64_t LodSectionsAcross(int level) {
  return std::int64_t{1} << (kLodMaxLevel - level);
}
// Rows of sections at a level that hold part of the world's height (8,192 m: one from level 8).
inline constexpr int LodRows(int level) {
  const std::int64_t rows = (kWorldMaxY - kWorldMinY) / LodSectionSize(level);
  return rows < 1 ? 1 : static_cast<int>(rows);
}

struct LodOrigin {
  std::int64_t x = 0, y = 0, z = 0;  // world metres of the section's min corner
};
LodOrigin LodSectionOrigin(const LodCoord& c);

// Octant bits: 1 = +x half, 2 = +y half, 4 = +z half.
LodCoord LodChild(const LodCoord& c, int octant);
LodCoord LodParent(const LodCoord& c);
LodCoord LodAncestor(const LodCoord& c, int level);
LodCoord LodOfChunk(const ChunkCoord& c);  // the level-0 section that is this chunk
ChunkCoord ChunkOfLod(const LodCoord& c);  // level 0 only

// Inside the octree, within the world's rows, and overlapping the world's disc.
bool LodInWorld(const LodCoord& c);

using LodCells = std::vector<MaterialId>;  // kLodVolume, LodCell order

// The surface of each column of a generated section (kLodPad² columns, index
// (z + 1) · kLodPad + (x + 1)), at full vertical precision: a cell is kLodCellSize tall, which far
// away is kilometres, so drawing a column's top at its top cell's top would lift distant land and
// seas by up to a cell. The client draws the top at `height` instead (ARCHITECTURE.md §6.6).
struct LodSurface {
  bool valid = false;  // the column's topmost filled cell in the section holds its surface
  bool wet = false;    // under water: `height` is the floor, `material` the floor's
  float height = 0;    // metres
  MaterialId material = 0;
  // Where wet: the water's surface (metres; open voxels below it are water) — the sea's, or a
  // river's or lake's above sea level, which is not on the cells' grid.
  float water = 0;
};
using LodSurfaces = std::vector<LodSurface>;  // kLodPad²

// What a generated section turned out to be: all air (nothing to draw), buried (solid with no
// exposed face: nothing to draw), or content to mesh.
enum class LodKind : std::uint8_t { kEmpty, kBuried, kContent };

// A 2×2×2 block of cells → one cell of the next level (§6.6): filled if at least 4 of 8 are not
// air (solid or liquid alike, so seas keep their surface at coarse levels), else air. The material
// is the most common one among the top filled cell of each of the block's four columns (the
// surface seen from above), ties to the upper cells. `cells` index: dx | dy << 1 | dz << 2.
MaterialId DownsampleBlock(const MaterialId (&cells)[8]);
bool LodSolid(MaterialId m);  // not air, not liquid

// Downsamples a child's 32³ interior (child_at(x, y, z), each 0..31) into the octant of `parent`
// (LodCell order) it covers.
void DownsampleOctant(const std::function<MaterialId(int, int, int)>& child_at, int octant,
                      LodCells& parent);
void DownsampleChunkOctant(const Chunk& chunk, int octant, LodCells& parent);
void DownsampleSectionOctant(const LodCells& child, int octant, LodCells& parent);

// FNV-1a 64 over the cells (u16 little-endian, LodCell order) and the kind: the golden test.
std::uint64_t LodHash(LodKind kind, const LodCells& cells);

// A section of a world as the generator would leave it, evaluated at level L's resolution
// (GenerateLod, §6.6): a pure function of the coordinate, bit-identical natively and in WASM.
// `cells` is resized to kLodVolume.
using LodGenerator = std::function<LodKind(const LodCoord&, LodCells&)>;
LodGenerator LodGeneratorFor(std::uint32_t generator_version, std::uint64_t world_seed);

// Height bounds of a column of sections (L, i, ·, k), from the generator's columns (§6.6): cells
// whose sample lies above `hi` are air (or sea), below `lo` solid. Sections are classified from
// them without generating anything (LodKindFromBounds).
struct LodBounds {
  double lo = 0, hi = 0;
  bool any_inside = true;  // some column of the section (with its apron) is inside the disc
};
using LodBoundsFn = std::function<LodBounds(int level, std::int32_t i, std::int32_t k)>;
LodBoundsFn LodBoundsFor(std::uint32_t generator_version, std::uint64_t world_seed);
// kEmpty or kBuried when the bounds decide the section (with its apron), else kContent.
LodKind LodKindFromBounds(const LodCoord& c, const LodBounds& b);

// The flat test world (generators 0 and 1: the playground's features are far below a cell).
LodKind GenerateFlatLod(const LodCoord& c, LodCells& cells);
LodBounds FlatLodBounds(int level, std::int32_t i, std::int32_t k);

// A world column (x, z) in metres, possibly beyond int32, inside the disc.
inline bool InsideWorldDisc64(std::int64_t x, std::int64_t z) {
  const std::int64_t r = kWorldRadius;
  return x * x + z * z < r * r;
}

}  // namespace dwell::core
