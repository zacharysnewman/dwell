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
}  // namespace Materials

struct MaterialInfo {
  std::string_view name;
  float density_kg_m3;  // per 1 m³ voxel (mass for Tier 1 clusters, §7.1)
  bool solid;
  bool indestructible;  // bedrock: the structural-integrity anchor (§6.3)
};

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
