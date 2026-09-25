#include "dwell/core/voxel.h"

namespace dwell::core {
namespace {

constexpr int kWorldMinY = -128;  // ARCHITECTURE.md §6.3
constexpr int kBedrockLayers = 4;

constexpr std::array<MaterialInfo, 5> kMaterials{{
    {"air", 0.0f, false, false},
    {"bedrock", 3000.0f, true, true},
    {"stone", 2400.0f, true, false},
    {"dirt", 1500.0f, true, false},
    {"grass", 1400.0f, true, false},
}};

std::int32_t FloorDiv(std::int32_t a, std::int32_t b) {
  const std::int32_t q = a / b;
  return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q;
}

}  // namespace

const MaterialInfo& GetMaterial(MaterialId id) {
  return id < kMaterials.size() ? kMaterials[id] : kMaterials[Materials::kAir];
}

std::size_t ChunkCoordHash::operator()(const ChunkCoord& c) const noexcept {
  std::uint64_t h = static_cast<std::uint32_t>(c.x);
  h = h * 0x9E3779B97F4A7C15ull ^ static_cast<std::uint32_t>(c.y);
  h = h * 0x9E3779B97F4A7C15ull ^ static_cast<std::uint32_t>(c.z);
  return static_cast<std::size_t>(h ^ (h >> 32));
}

ChunkCoord ChunkOf(std::int32_t x, std::int32_t y, std::int32_t z) {
  return {FloorDiv(x, kChunkSize), FloorDiv(y, kChunkSize), FloorDiv(z, kChunkSize)};
}

void GenerateFlatChunk(const ChunkCoord& coord, Chunk& chunk) {
  for (int ly = 0; ly < kChunkSize; ++ly) {
    const int y = coord.y * kChunkSize + ly;
    MaterialId m = Materials::kAir;
    if (y < kWorldMinY) {
      m = Materials::kAir;
    } else if (y < kWorldMinY + kBedrockLayers) {
      m = Materials::kBedrock;
    } else if (y < -4) {
      m = Materials::kStone;
    } else if (y < -1) {
      m = Materials::kDirt;
    } else if (y == -1) {
      m = Materials::kGrass;
    }
    if (m == Materials::kAir) continue;
    for (int lz = 0; lz < kChunkSize; ++lz) {
      for (int lx = 0; lx < kChunkSize; ++lx) chunk.Set(lx, ly, lz, m);
    }
  }
}

Chunk& VoxelWorld::GetOrCreate(const ChunkCoord& coord) {
  auto [it, inserted] = chunks_.try_emplace(coord);
  if (inserted) {
    it->second = std::make_unique<Chunk>();
    generator_(coord, *it->second);
    it->second->ResetRevision();
  }
  return *it->second;
}

const Chunk* VoxelWorld::Find(const ChunkCoord& coord) const {
  const auto it = chunks_.find(coord);
  return it == chunks_.end() ? nullptr : it->second.get();
}

MaterialId VoxelWorld::GetVoxel(std::int32_t x, std::int32_t y, std::int32_t z) {
  const auto c = ChunkOf(x, y, z);
  return GetOrCreate(c).Get(x - c.x * kChunkSize, y - c.y * kChunkSize, z - c.z * kChunkSize);
}

void VoxelWorld::SetVoxel(std::int32_t x, std::int32_t y, std::int32_t z, MaterialId m) {
  const auto c = ChunkOf(x, y, z);
  GetOrCreate(c).Set(x - c.x * kChunkSize, y - c.y * kChunkSize, z - c.z * kChunkSize, m);
}

}  // namespace dwell::core
