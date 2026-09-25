#pragma once

#include <array>
#include <cstdint>
#include <functional>
#include <memory>
#include <string_view>
#include <unordered_map>

#include "dwell/protocol/constants.gen.h"

// Voxel world storage (ARCHITECTURE.md §6.1). Phase 1: materials, 32³ chunks, and a flat test
// world; procedural generation arrives in Phase 3.
namespace dwell::core {

using MaterialId = std::uint16_t;

namespace Materials {
inline constexpr MaterialId kAir = 0;
inline constexpr MaterialId kBedrock = 1;
inline constexpr MaterialId kStone = 2;
inline constexpr MaterialId kDirt = 3;
inline constexpr MaterialId kGrass = 4;
inline constexpr MaterialId kStoneSlab = 5;  // bottom half of the cell
inline constexpr MaterialId kLadderN =
    6;  // climbable; faces north (−Z), mounted on the cell's +Z side
inline constexpr MaterialId kLadderE = 7;  // faces east (+X)
inline constexpr MaterialId kLadderS = 8;  // faces south (+Z)
inline constexpr MaterialId kLadderW = 9;  // faces west (−X)
inline constexpr MaterialId kWater = 10;
inline constexpr MaterialId kLaunchPad = 11;  // debug: launches players standing on it (Phase 2)
inline constexpr MaterialId kCount = 12;
}  // namespace Materials

// Collision shape of a voxel inside its 1 m cell (PLAYER_CONTROLLER.md §5).
enum class VoxelShape : std::uint8_t { kEmpty, kFull, kSlabBottom };

// Compass facing of directional materials (ladders). North = −Z, east = +X.
enum class Facing : std::uint8_t { kNone, kNorth, kEast, kSouth, kWest };

struct MaterialInfo {
  std::string_view name;
  float density_kg_m3;  // per 1 m³ voxel (mass for Tier 1 clusters, §7.1)
  bool solid;           // has collision (shape != kEmpty)
  bool indestructible;  // bedrock: the structural-integrity anchor (§6.3)
  VoxelShape shape = VoxelShape::kEmpty;
  bool climbable = false;         // ladders, vines (PLAYER_CONTROLLER.md §6.3)
  Facing facing = Facing::kNone;  // climbable: direction the climbing side faces
  float climb_speed_scale = 1.0f;
  bool liquid = false;        // water: swim layer (PLAYER_CONTROLLER.md §6.4)
  float launch_speed = 0.0f;  // debug launch pad: upward knockback when stood on (m/s)
};

// Height of the solid part of a voxel shape within its cell (0 = empty, 1 = full cube).
inline float ShapeHeight(VoxelShape shape) {
  return shape == VoxelShape::kFull ? 1.0f : shape == VoxelShape::kSlabBottom ? 0.5f : 0.0f;
}

// Shared material table; unknown ids resolve to air.
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
  // Called once after generation: an unmodified generated chunk is revision 0 (§6.1, §6.3).
  void ResetRevision() { revision_ = 0; }
  const std::array<MaterialId, kChunkVolume>& voxels() const { return voxels_; }

 private:
  std::array<MaterialId, kChunkVolume> voxels_;
  std::uint32_t revision_ = 0;
};

// Fills a freshly created chunk. Must be a pure function of the coordinate (§6.3).
using ChunkGenerator = std::function<void(const ChunkCoord&, Chunk&)>;

// Flat test world: bedrock below WORLD_MIN_Y + 4, stone, three layers of dirt, and grass whose top
// face is at y = 0.
void GenerateFlatChunk(const ChunkCoord& coord, Chunk& chunk);

// All air (tests build their geometry voxel by voxel).
void GenerateEmptyChunk(const ChunkCoord& coord, Chunk& chunk);

// The flat world plus a small movement playground near the spawn (generator version 1, Phase 2):
// slab and block steps, a 1×2 doorway, a 1-tall crawlspace, a ladder to a ledge, a water pool and
// a launch pad. See PlaygroundFeatures() for positions.
void GeneratePlaygroundChunk(const ChunkCoord& coord, Chunk& chunk);

// Generator versions announced in Welcome (§6.3): 0 = flat test world, 1 = playground. Unknown
// versions fall back to the flat world.
inline constexpr std::uint32_t kGeneratorFlat = 0;
inline constexpr std::uint32_t kGeneratorPlayground = 1;
ChunkGenerator GeneratorFor(std::uint32_t generator_version);

// Master voxel grid: chunks generated on first access.
class VoxelWorld {
 public:
  explicit VoxelWorld(ChunkGenerator generator = GenerateFlatChunk)
      : generator_(std::move(generator)) {}

  Chunk& GetOrCreate(const ChunkCoord& coord);
  const Chunk* Find(const ChunkCoord& coord) const;
  MaterialId GetVoxel(std::int32_t x, std::int32_t y, std::int32_t z);
  void SetVoxel(std::int32_t x, std::int32_t y, std::int32_t z, MaterialId m);
  std::size_t loaded_chunks() const { return chunks_.size(); }

 private:
  ChunkGenerator generator_;
  std::unordered_map<ChunkCoord, std::unique_ptr<Chunk>, ChunkCoordHash> chunks_;
};

}  // namespace dwell::core
