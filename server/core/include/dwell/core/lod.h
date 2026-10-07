#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <vector>

#include "dwell/core/voxel.h"
#include "dwell/protocol/constants.gen.h"

// Level of detail: the whole-world view (ARCHITECTURE.md §6.6, ADR 0012). A 3D octree of sections:
// at level L a cell is a 2^L m cube and a section is 32³ cells, so level 0 is the chunk grid.
// Section coordinates (L, i, j, k) count from the corner (−2²³, −2²³, −2²³) in all three axes, so
// the octree covers the whole bifacial world with both domes (BIFACIAL_WORLD.md §2); one root at
// LOD_MAX_LEVEL holds the whole disc. Sections up to level 6 (2,048 m) lie wholly on one face of
// the midplane; coarser ones straddle it and give each cell the face of its centre. Section content
// is 34³ materials: the section and a one-cell apron from its neighbours, in layer order (x
// fastest, then z, then y) — the order the palette + RLE codec takes cells in, and the client
// mesher's padded layout.
namespace dwell::core {

inline constexpr int kLodSectionCells = protocol::kLodSectionCells;
inline constexpr int kLodMaxLevel = protocol::kLodMaxLevel;
inline constexpr int kLodIndexLevel = protocol::kLodIndexLevel;
inline constexpr int kLodPad = kLodSectionCells + 2;
inline constexpr int kLodVolume = kLodPad * kLodPad * kLodPad;
inline constexpr std::int64_t kLodOriginX = -(std::int64_t{1} << 23);
inline constexpr std::int64_t kLodOriginY = -(std::int64_t{1} << 23);
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
// The first and last rows of sections at a level that hold part of the world's height
// (kWorldBottomY up to kWorldMaxY): 16,384 m tall, so one row from level 9.
inline constexpr std::int64_t LodFloorDiv(std::int64_t a, std::int64_t b) {
  return a / b - ((a % b != 0) && ((a < 0) != (b < 0)) ? 1 : 0);
}
inline constexpr std::int32_t LodFirstRow(int level) {
  return static_cast<std::int32_t>(LodFloorDiv(kWorldBottomY - kLodOriginY, LodSectionSize(level)));
}
inline constexpr std::int32_t LodLastRow(int level) {
  return static_cast<std::int32_t>(
      LodFloorDiv(kWorldMaxY - 1 - kLodOriginY, LodSectionSize(level)));
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
  // The tint (worldgen/biomes.h) of the column's grass and foliage, as 0xRRGGBB in 1/64 units (0:
  // none), smoothed over neighbouring columns: the client multiplies tinted blocks' colours by it.
  std::uint32_t tint_grass = 0, tint_foliage = 0;
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

// On face B the surface seen is the lowest filled cell of each column, so the block is read
// upside down (`face`).
MaterialId DownsampleBlock(const MaterialId (&cells)[8], Face face);

// Downsamples a child's 32³ interior (child_at(x, y, z), each 0..31) into the octant of the
// `parent` section (LodCell order) it covers; each parent cell takes its face from its centre's
// side of the midplane.
void DownsampleOctant(const std::function<MaterialId(int, int, int)>& child_at,
                      const LodCoord& parent_coord, int octant, LodCells& parent);
void DownsampleChunkOctant(const Chunk& chunk, const LodCoord& parent_coord, int octant,
                           LodCells& parent);
void DownsampleSectionOctant(const LodCells& child, const LodCoord& parent_coord, int octant,
                             LodCells& parent);

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
// Face B's are in its face-local frame (heights measured as on face A: its sea level at 0, its sky
// toward +h), since each face's terrain is generated that way.
struct LodBounds {
  double lo = 0, hi = 0;      // face A
  double lo_b = 0, hi_b = 0;  // face B, face-local
  bool any_inside = true;     // some column of the section (with its apron) is inside the disc
  // False for the flat test worlds, which have no face B: one terrain from lo to hi in world
  // terms, whichever face the section's rows lie on.
  bool bifacial = true;
};
using LodBoundsFn = std::function<LodBounds(int level, std::int32_t i, std::int32_t k)>;
LodBoundsFn LodBoundsFor(std::uint32_t generator_version, std::uint64_t world_seed);
// kEmpty or kBuried when the bounds decide the section (with its apron), else kContent. Each face
// is judged over the rows of the section it owns (a row belongs to the face of its centre); a
// section straddling the midplane is empty or buried only when both parts are.
LodKind LodKindFromBounds(const LodCoord& c, const LodBounds& b);
// One face's part of the section, in face-local terms: a face-local section whose cell rows start
// at `origin_y` (the face-local height of its row 0), judged against that face's (lo, hi).
LodKind LodKindOfFace(int level, std::int64_t origin_y, double lo, double hi);

// The face of cell row r (−1..32) of a section: its centre's side of the midplane.
inline Face LodRowFace(const LodCoord& c, int row) {
  const std::int64_t cell = LodCellSize(c.level);
  const std::int64_t centre2 =
      2 * (kLodOriginY + c.j * LodSectionSize(c.level) + row * cell) + cell;
  return centre2 >= 2 * std::int64_t{kMidplaneY} ? Face::kA : Face::kB;
}
// The face-local height of the bottom voxel of a face-B row's cell (BIFACIAL_WORLD.md §2): the
// mirror of the cell's top.
inline std::int64_t LodMirrorRowBottom(std::int64_t bottom, std::int64_t cell) {
  return kMirrorSum + 1 - bottom - cell;
}

// The flat test world (generators 0 and 1: the playground's features are far below a cell).
LodKind GenerateFlatLod(const LodCoord& c, LodCells& cells);
LodBounds FlatLodBounds(int level, std::int32_t i, std::int32_t k);

// A world column (x, z) in metres, possibly beyond int32, inside the disc.
inline bool InsideWorldDisc64(std::int64_t x, std::int64_t z) {
  const std::int64_t r = kWorldRadius;
  return x * x + z * z < r * r;
}

}  // namespace dwell::core
