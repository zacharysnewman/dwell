#pragma once

#include <array>
#include <cstdint>
#include <functional>
#include <memory>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "dwell/core/block_types.h"
#include "dwell/core/blocks.gen.h"
#include "dwell/protocol/constants.gen.h"

// Voxel world storage (ARCHITECTURE.md §6.1): materials, 32³ chunks, the flat test world and the
// playground; the procedural terrain generator lives in dwell/worldgen (§6.3).
namespace dwell::core {

// World bounds and terrain constants (§6.3, §7.4). The world is bifacial (BIFACIAL_WORLD.md): face
// A is the ground band [kMidplaneY, kWorldMaxY) with sea level kSeaLevel, face B its mirror image
// below the midplane, [kWorldBottomY, kMidplaneY), with its own terrain. There is no void below
// the world and no bedrock: the rock at the midplane is ordinary.
inline constexpr int kMidplaneY = protocol::kMidplaneY;  // the halfway plane: face A starts here
inline constexpr int kWorldBottomY = protocol::kWorldBottomY;  // face B's lowest voxel row
inline constexpr int kWorldMaxY = protocol::kWorldMaxY;        // generated terrain stays below this
// Layers of the flat and playground test worlds' floor at the bottom of face A (generated terrain
// has none: its core is ordinary rock).
inline constexpr int kBedrockLayers = 4;
inline constexpr int kSeaLevel = protocol::kSeaLevel;  // terrain: water fills open space below
inline constexpr int kWorldRadius = protocol::kWorldRadius;  // world disc radius (m); beyond: void

// Block states: ids by name (Materials::k…), the table and the registry are generated from
// shared/blocks/*.json (block_registry.h).
// Directional aliases of the ladder's states (facing north = −Z, mounted on the cell's +Z side).
namespace Materials {
inline constexpr MaterialId kLadderN = kLadder;
inline constexpr MaterialId kLadderE = kLadderFacingEast;
inline constexpr MaterialId kLadderS = kLadderFacingSouth;
inline constexpr MaterialId kLadderW = kLadderFacingWest;
}  // namespace Materials

// Shared state table; unknown ids resolve to air.
const MaterialInfo& GetMaterial(MaterialId id);

inline constexpr int kChunkSize = protocol::kChunkSize;
inline constexpr int kChunkVolume = kChunkSize * kChunkSize * kChunkSize;

struct ChunkCoord {
  std::int32_t x = 0, y = 0, z = 0;
  bool operator==(const ChunkCoord&) const = default;
};

struct ChunkCoordHash {
  std::size_t operator()(const ChunkCoord& c) const noexcept;
};

// World-space voxel coordinate → chunk coordinate and local index (floor division).
ChunkCoord ChunkOf(std::int32_t x, std::int32_t y, std::int32_t z);
inline int LocalIndex(int lx, int ly, int lz) { return lx | (ly << 5) | (lz << 10); }

class Chunk {
 public:
  Chunk() { voxels_.fill(Materials::kAir); }

  MaterialId Get(int lx, int ly, int lz) const { return voxels_[LocalIndex(lx, ly, lz)]; }
  void Set(int lx, int ly, int lz, MaterialId m) {
    voxels_[LocalIndex(lx, ly, lz)] = m;
    ++revision_;
  }
  std::uint32_t revision() const { return revision_; }
  // Edits (§6.5): voxels changed without per-voxel revision bumps, then one bump for the batch, so
  // a chunk's revision advances by one per VoxelModification that touches it.
  void SetAt(int index, MaterialId m) { voxels_[static_cast<std::size_t>(index)] = m; }
  void BumpRevision() { ++revision_; }
  // Called once after generation: an unmodified generated chunk is revision 0 (§6.1, §6.3).
  void ResetRevision() { revision_ = 0; }
  // Streamed chunks take the revision the server sent.
  void SetRevision(std::uint32_t revision) { revision_ = revision; }
  const std::array<MaterialId, kChunkVolume>& voxels() const { return voxels_; }
  // Generators write here directly (no revision bumps).
  std::array<MaterialId, kChunkVolume>& generation_voxels() { return voxels_; }

 private:
  std::array<MaterialId, kChunkVolume> voxels_;
  std::uint32_t revision_ = 0;
};

// Fills a freshly created chunk. Must be a pure function of the coordinate (§6.3).
using ChunkGenerator = std::function<void(const ChunkCoord&, Chunk&)>;
// True for chunks a generator leaves all air, known without generating them (§6.3). Streaming sends
// them as payload-free `Air`, and a generated world reads them as air without storing them.
using AirChunkTest = std::function<bool(const ChunkCoord&)>;

// Flat test world: bedrock below WORLD_MIN_Y + 4, stone, three layers of dirt, and grass whose top
// face is at y = 0.
void GenerateFlatChunk(const ChunkCoord& coord, Chunk& chunk);

// All air (tests build their geometry voxel by voxel).
void GenerateEmptyChunk(const ChunkCoord& coord, Chunk& chunk);

// The flat world plus a small movement playground near the spawn (generator version 1, Phase 2):
// slab and block steps, a 1×2 doorway, a 1-tall crawlspace, a ladder to a ledge, a water pool and
// a launch pad. See PlaygroundFeatures() for positions.
void GeneratePlaygroundChunk(const ChunkCoord& coord, Chunk& chunk);

// Generator versions announced in Welcome (§6.3): 0 = flat test world, 1 = playground,
// 8 = procedural terrain (worldgen::TerrainGenerator, uses the world seed; 2 was the
// pre-planet-scale terrain, 3 the terrain before super tall massifs and 4 the terrain before
// slopes, all retired). Unknown
// versions fall back to the flat world. Bump the terrain version for any change that alters its
// output.
inline constexpr std::uint32_t kGeneratorFlat = 0;
inline constexpr std::uint32_t kGeneratorPlayground = 1;
inline constexpr std::uint32_t kGeneratorTerrain = 8;
ChunkGenerator GeneratorFor(std::uint32_t generator_version, std::uint64_t world_seed = 0);
// The generator's all-air test. The terrain's caches per chunk column (not thread-safe: one per
// thread).
AirChunkTest AirTestFor(std::uint32_t generator_version, std::uint64_t world_seed = 0);

// Feet position players spawn at for a generator: near the origin, on open level ground.
std::array<double, 3> SpawnPointFor(std::uint32_t generator_version, std::uint64_t world_seed);

// FNV-1a 64 over a chunk's voxel ids (u16 little-endian, chunk index order): the worldgen golden
// test and the WorldgenCheck verification hash (§6.3).
std::uint64_t ChunkHash(const Chunk& chunk);

// "Regenerate and diff" (Phase 3e debug tooling): the voxels where `current` differs from the
// chunk as generated, in chunk index order.
struct VoxelDiff {
  int index;  // x | y << 5 | z << 10
  MaterialId generated, current;
};
std::vector<VoxelDiff> DiffChunk(const Chunk& generated, const Chunk& current);

// The world's disc (ADR 0011): a column (x, z) is inside when x² + z² < WORLD_RADIUS². Beyond it,
// generators produce nothing (the void).
inline bool InsideWorldDisc(std::int32_t x, std::int32_t z) {
  const std::int64_t r = kWorldRadius;
  return std::int64_t{x} * x + std::int64_t{z} * z < r * r;
}
enum class DiscOverlap : std::uint8_t { kInside, kPartial, kOutside };
// How a chunk column's 32 × 32 voxel columns lie relative to the disc.
DiscOverlap ChunkDiscOverlap(std::int32_t cx, std::int32_t cz);

// Chunk rows the world can hold anything in (kWorldBottomY..kWorldMaxY); outside is air.
inline constexpr int kMinChunkY = kWorldBottomY / kChunkSize;
inline constexpr int kMaxChunkY = kWorldMaxY / kChunkSize - 1;

// The two faces of the disc (BIFACIAL_WORLD.md §2). Face A is the world at or above the midplane,
// face B the mirror image below it. A voxel at y on face B corresponds to face-local height
// kMirrorSum − y (voxels map onto voxels); a chunk row cy to kMirrorChunkSum − cy (chunks onto
// chunks). The mirror is its own inverse.
enum class Face : std::uint8_t { kA = 0, kB = 1 };
inline constexpr int kMirrorSum = 2 * kMidplaneY - 1;  // −4097
static_assert(kMidplaneY % kChunkSize == 0, "the midplane lies on a chunk boundary");
inline constexpr int kMidplaneChunkY = kMidplaneY / kChunkSize;  // the first row of face A: −64
inline constexpr int kMirrorChunkSum = 2 * kMidplaneChunkY - 1;  // −129
inline Face FaceOfY(std::int64_t y) { return y >= kMidplaneY ? Face::kA : Face::kB; }
inline Face FaceOfChunkY(std::int32_t cy) { return cy >= kMidplaneChunkY ? Face::kA : Face::kB; }
inline std::int32_t MirrorY(std::int32_t y) { return kMirrorSum - y; }
inline std::int32_t MirrorChunkY(std::int32_t cy) { return kMirrorChunkSum - cy; }
// The face-local chunk a chunk of face B is the flip of (face A's chunks are their own).
inline ChunkCoord FaceLocalChunk(const ChunkCoord& c) {
  return FaceOfChunkY(c.y) == Face::kA ? c : ChunkCoord{c.x, MirrorChunkY(c.y), c.z};
}
// The state a block takes when turned upside down (vertical mirror): slabs and slopes swap
// `half`; every other state is its own mirror. Used where face B's chunks are generated from
// face-local ones.
MaterialId MirrorMaterial(MaterialId m);

// Chebyshev distance between chunk coordinates.
inline int ChunkDistance(const ChunkCoord& a, const ChunkCoord& b) {
  const int dx = a.x > b.x ? a.x - b.x : b.x - a.x;
  const int dy = a.y > b.y ? a.y - b.y : b.y - a.y;
  const int dz = a.z > b.z ? a.z - b.z : b.z - a.z;
  return dx > dy ? (dx > dz ? dx : dz) : (dy > dz ? dy : dz);
}

// Chunks kept outside memory — the world file (§6.4): `has` names them, `load` reads one (null on
// failure: the chunk is generated instead). A world loads them rather than generating them, and
// never reads them as air.
struct SavedChunks {
  std::function<bool(const ChunkCoord&)> has;
  std::function<std::unique_ptr<Chunk>(const ChunkCoord&)> load;
};

// Voxel grid. With a generator (the server; tests) missing chunks are generated on first access,
// and unmodified ones can be evicted and regenerated later (§6.3). Without one (a *streamed* world:
// the client) chunks arrive through Put() and missing chunks read as air.
class VoxelWorld {
 public:
  explicit VoxelWorld(ChunkGenerator generator = GenerateFlatChunk, AirChunkTest air = nullptr)
      : generator_(std::move(generator)), air_(std::move(air)) {}

  bool streamed() const { return !generator_; }

  // Generates a missing chunk (streamed worlds: creates an all-air one).
  Chunk& GetOrCreate(const ChunkCoord& coord);
  const Chunk* Find(const ChunkCoord& coord) const;
  // A chunk to read: generated on demand, or all air while missing in a streamed world or when
  // the air test says the generator would leave it empty (not stored).
  const Chunk& Read(const ChunkCoord& coord);
  MaterialId GetVoxel(std::int32_t x, std::int32_t y, std::int32_t z);
  void SetVoxel(std::int32_t x, std::int32_t y, std::int32_t z, MaterialId m);

  // Inserts or replaces a chunk (streamed chunks; chunks generated off-thread).
  void Put(const ChunkCoord& coord, std::unique_ptr<Chunk> chunk);
  void Remove(const ChunkCoord& coord);
  // Drops unmodified (revision 0) chunks that `keep` rejects; returns how many.
  std::size_t EvictUnmodified(const std::function<bool(const ChunkCoord&)>& keep);
  // Drops the chunks `evict` selects (callers keep modified chunks that are not saved).
  std::size_t Evict(const std::function<bool(const ChunkCoord&, const Chunk&)>& evict);
  void SetSaved(SavedChunks saved) { saved_ = std::move(saved); }

  // Changes whenever a chunk is replaced or removed (or appears in a streamed world): pointers
  // and references to chunks obtained before a change may be stale.
  std::uint64_t epoch() const { return epoch_; }
  std::size_t loaded_chunks() const { return chunks_.size(); }
  // Chunks generated synchronously on access (the tick waited for them).
  std::uint64_t generated_on_access() const { return generated_on_access_; }
  // Chunks read from the world file (SavedChunks).
  std::uint64_t loaded_saved() const { return loaded_saved_; }

 private:
  ChunkGenerator generator_;
  AirChunkTest air_;
  SavedChunks saved_;
  std::unordered_map<ChunkCoord, std::unique_ptr<Chunk>, ChunkCoordHash> chunks_;
  std::uint64_t epoch_ = 0;
  std::uint64_t generated_on_access_ = 0;
  std::uint64_t loaded_saved_ = 0;
};

}  // namespace dwell::core
