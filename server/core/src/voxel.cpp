#include "dwell/core/voxel.h"

namespace dwell::core {
namespace {

constexpr int kWorldMinY = -128;  // ARCHITECTURE.md §6.3
constexpr int kBedrockLayers = 4;

using enum VoxelShape;

constexpr std::array<MaterialInfo, Materials::kCount> kMaterials{{
    {"air", 0.0f, false, false},
    {"bedrock", 3000.0f, true, true, kFull},
    {"stone", 2400.0f, true, false, kFull},
    {"dirt", 1500.0f, true, false, kFull},
    {"grass", 1400.0f, true, false, kFull},
    {"stone_slab", 2400.0f, true, false, kSlabBottom},
    {"ladder_n", 600.0f, false, false, kEmpty, true, Facing::kNorth},
    {"ladder_e", 600.0f, false, false, kEmpty, true, Facing::kEast},
    {"ladder_s", 600.0f, false, false, kEmpty, true, Facing::kSouth},
    {"ladder_w", 600.0f, false, false, kEmpty, true, Facing::kWest},
    {"water", 1000.0f, false, false, kEmpty, false, Facing::kNone, 1.0f, true},
    {"launch_pad", 2400.0f, true, false, kFull, false, Facing::kNone, 1.0f, false, 14.0f},
}};

MaterialId FlatMaterial(std::int32_t y) {
  if (y < kWorldMinY) return Materials::kAir;
  if (y < kWorldMinY + kBedrockLayers) return Materials::kBedrock;
  if (y < -4) return Materials::kStone;
  if (y < -1) return Materials::kDirt;
  if (y == -1) return Materials::kGrass;
  return Materials::kAir;
}

bool In(std::int32_t v, std::int32_t lo, std::int32_t hi) { return v >= lo && v <= hi; }

// Movement playground (generator version 1). Pure function of the voxel coordinate.
MaterialId PlaygroundMaterial(std::int32_t x, std::int32_t y, std::int32_t z) {
  using namespace Materials;
  // Slab stairs (x −10..−8): slab, block, block + slab, two blocks — 0.5 m per step.
  if (In(x, -10, -8)) {
    if (z == 6 && y == 0) return kStoneSlab;
    if (In(z, 7, 10) && y == 0) return kStone;
    if (z == 8 && y == 1) return kStoneSlab;
    if (In(z, 9, 10) && y == 1) return kStone;
  }
  // A 1 m block step (x −6..−4): needs a jump.
  if (In(x, -6, -4) && In(z, 6, 8) && y == 0) return kStone;
  // Wall with a 1-wide, 2-tall doorway at x = 0 (x −2..2, z = 12, 4 tall).
  if (In(x, -2, 2) && z == 12 && In(y, 0, 3) && !(x == 0 && y <= 1)) return kStone;
  // Crawlspace: a roof 1 m above the floor (x 4..6, z 6..9).
  if (In(x, 4, 6) && In(z, 6, 9) && y == 1) return kStone;
  // Ladder (x = 9, z = 9, y 0..3, facing north) up a 4 m wall to a ledge (x 8..10, z 10..13).
  if (In(x, 8, 10) && In(z, 10, 13) && In(y, 0, 3)) return kStone;
  if (x == 9 && z == 9 && In(y, 0, 3)) return kLadderN;
  // Water pool, 2 m deep (x 12..15, z 6..9).
  if (In(x, 12, 15) && In(z, 6, 9) && In(y, -2, -1)) return kWater;
  // Launch pad flush with the ground at (0, −1, −6).
  if (x == 0 && z == -6 && y == -1) return kLaunchPad;
  return FlatMaterial(y);
}

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
    const MaterialId m = FlatMaterial(coord.y * kChunkSize + ly);
    if (m == Materials::kAir) continue;
    for (int lz = 0; lz < kChunkSize; ++lz) {
      for (int lx = 0; lx < kChunkSize; ++lx) chunk.Set(lx, ly, lz, m);
    }
  }
}

void GenerateEmptyChunk(const ChunkCoord&, Chunk&) {}

void GeneratePlaygroundChunk(const ChunkCoord& coord, Chunk& chunk) {
  // The playground lies within x −16..15, y −32..31, z −16..31; elsewhere it is the flat world.
  if (coord.x < -1 || coord.x > 0 || coord.y < -1 || coord.y > 0 || coord.z < -1 || coord.z > 0) {
    GenerateFlatChunk(coord, chunk);
    return;
  }
  for (int lz = 0; lz < kChunkSize; ++lz) {
    for (int ly = 0; ly < kChunkSize; ++ly) {
      for (int lx = 0; lx < kChunkSize; ++lx) {
        const MaterialId m = PlaygroundMaterial(
            coord.x * kChunkSize + lx, coord.y * kChunkSize + ly, coord.z * kChunkSize + lz);
        if (m != Materials::kAir) chunk.Set(lx, ly, lz, m);
      }
    }
  }
}

ChunkGenerator GeneratorFor(std::uint32_t generator_version) {
  return generator_version == kGeneratorPlayground ? ChunkGenerator(GeneratePlaygroundChunk)
                                                   : ChunkGenerator(GenerateFlatChunk);
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
