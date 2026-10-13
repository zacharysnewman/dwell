#include "dwell/core/voxel.h"

#include "dwell/core/block_registry.h"
#include "dwell/worldgen/bifacial.h"

namespace dwell::core {
namespace {

MaterialId FlatMaterial(std::int32_t y) {
  if (y < kMidplaneY) return Materials::kAir;
  if (y < -4) return Materials::kStone;
  if (y < -1) return Materials::kDirt;
  if (y == -1) return Materials::kGrass;
  return Materials::kAir;
}

std::int32_t FloorDiv(std::int32_t a, std::int32_t b) {
  const std::int32_t q = a / b;
  return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q;
}

}  // namespace

MaterialId MirrorMaterial(MaterialId m) {
  // Slabs and slopes hang from the other side: the same state with `half` swapped. Built once.
  static const std::vector<MaterialId> table = [] {
    std::vector<MaterialId> t(Materials::kCount);
    for (std::size_t i = 0; i < t.size(); ++i) {
      const auto id = static_cast<MaterialId>(i);
      t[i] = id;
      const auto half = StateProperty(id, "half");
      if (!half) continue;
      const auto flipped = WithProperty(id, "half", *half == "bottom" ? "top" : "bottom");
      if (flipped) t[i] = *flipped;
    }
    return t;
  }();
  return m < table.size() ? table[m] : m;
}

DiscOverlap ChunkDiscOverlap(std::int32_t cx, std::int32_t cz) {
  // Nearest and farthest voxel columns of the chunk from the origin, per axis.
  const auto nearest = [](std::int32_t c) -> std::int64_t {
    const std::int64_t lo = std::int64_t{c} * kChunkSize, hi = lo + kChunkSize - 1;
    return lo > 0 ? lo : hi < 0 ? hi : 0;
  };
  const auto farthest = [](std::int32_t c) -> std::int64_t {
    const std::int64_t lo = std::int64_t{c} * kChunkSize, hi = lo + kChunkSize - 1;
    return -lo > hi ? lo : hi;
  };
  const std::int64_t r2 = std::int64_t{kWorldRadius} * kWorldRadius;
  const std::int64_t nx = nearest(cx), nz = nearest(cz);
  if (nx * nx + nz * nz >= r2) return DiscOverlap::kOutside;
  const std::int64_t fx = farthest(cx), fz = farthest(cz);
  return fx * fx + fz * fz < r2 ? DiscOverlap::kInside : DiscOverlap::kPartial;
}

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
  const DiscOverlap disc = ChunkDiscOverlap(coord.x, coord.z);
  if (disc == DiscOverlap::kOutside) return;  // the void beyond the rim
  for (int ly = 0; ly < kChunkSize; ++ly) {
    const MaterialId m = FlatMaterial(coord.y * kChunkSize + ly);
    if (m == Materials::kAir) continue;
    for (int lz = 0; lz < kChunkSize; ++lz) {
      for (int lx = 0; lx < kChunkSize; ++lx) {
        if (disc == DiscOverlap::kPartial &&
            !InsideWorldDisc(coord.x * kChunkSize + lx, coord.z * kChunkSize + lz)) {
          continue;
        }
        chunk.Set(lx, ly, lz, m);
      }
    }
  }
}

namespace {
bool FlatIsAir(const ChunkCoord& c) {
  const int y0 = c.y * kChunkSize;
  return y0 >= 0 || y0 + kChunkSize <= kMidplaneY ||
         ChunkDiscOverlap(c.x, c.z) == DiscOverlap::kOutside;
}
}  // namespace

void GenerateEmptyChunk(const ChunkCoord&, Chunk&) {}

ChunkGenerator GeneratorFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    auto terrain = std::make_shared<const worldgen::BifacialTerrain>(world_seed);
    return [terrain](const ChunkCoord& coord, Chunk& chunk) { terrain->Generate(coord, chunk); };
  }
  return GenerateFlatChunk;
}

AirChunkTest AirTestFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    // Sky floors are cached per chunk column and face (bounded: cleared when large). The key's y
    // holds the face, so a column's two floors do not collide.
    struct Cache {
      worldgen::BifacialTerrain terrain;
      std::unordered_map<ChunkCoord, float, ChunkCoordHash> sky_floor;
    };
    auto cache = std::make_shared<Cache>(Cache{worldgen::BifacialTerrain(world_seed), {}});
    return [cache](const ChunkCoord& coord) {
      const Face face = FaceOfChunkY(coord.y);
      const ChunkCoord local = FaceLocalChunk(coord);
      const std::int32_t y0 = local.y * kChunkSize;
      if (y0 < kSeaLevel || y0 >= kWorldMaxY ||
          ChunkDiscOverlap(coord.x, coord.z) == DiscOverlap::kOutside) {
        return cache->terrain.IsAirChunk(coord);
      }
      const ChunkCoord column{coord.x, static_cast<std::int32_t>(face), coord.z};
      auto it = cache->sky_floor.find(column);
      if (it == cache->sky_floor.end()) {
        if (cache->sky_floor.size() >= 4096) cache->sky_floor.clear();
        it = cache->sky_floor.emplace(column, cache->terrain.SkyFloorAt(coord.x, coord.z, face))
                 .first;
      }
      return worldgen::BifacialTerrain::IsAirChunk(coord, it->second);
    };
  }
  return FlatIsAir;
}

std::array<double, 3> SpawnPointFor(std::uint32_t generator_version, std::uint64_t world_seed) {
  if (generator_version == kGeneratorTerrain) {
    return worldgen::BifacialTerrain(world_seed).SpawnPoint();
  }
  return {0.5, 0.0, 0.5};
}

std::uint64_t ChunkHash(const Chunk& chunk) {
  std::uint64_t h = 0xcbf29ce484222325ull;
  for (const MaterialId m : chunk.voxels()) {
    for (int b = 0; b < 2; ++b) {
      h ^= static_cast<std::uint8_t>(m >> (8 * b));
      h *= 0x100000001b3ull;
    }
  }
  return h;
}

std::vector<VoxelDiff> DiffChunk(const Chunk& generated, const Chunk& current) {
  std::vector<VoxelDiff> out;
  const auto& a = generated.voxels();
  const auto& b = current.voxels();
  for (int i = 0; i < kChunkVolume; ++i) {
    const auto k = static_cast<std::size_t>(i);
    if (a[k] != b[k]) out.push_back({i, a[k], b[k]});
  }
  return out;
}

Chunk& VoxelWorld::GetOrCreate(const ChunkCoord& coord) {
  auto [it, inserted] = chunks_.try_emplace(coord);
  if (inserted) {
    if (saved_.has && saved_.has(coord)) {
      if ((it->second = saved_.load(coord))) {
        ++loaded_saved_;
        ++epoch_;
        return *it->second;
      }
    }
    it->second = std::make_unique<Chunk>();
    if (generator_) {
      generator_(coord, *it->second);
      ++generated_on_access_;
    }
    ++epoch_;  // readers may hold the shared air chunk for this coordinate
    it->second->ResetRevision();
  }
  return *it->second;
}

const Chunk& VoxelWorld::Read(const ChunkCoord& coord) {
  static const Chunk kAir;
  if (generator_) {
    if (air_ && !Find(coord) && !(saved_.has && saved_.has(coord)) && air_(coord)) return kAir;
    return GetOrCreate(coord);
  }
  const Chunk* chunk = Find(coord);
  return chunk ? *chunk : kAir;
}

void VoxelWorld::Put(const ChunkCoord& coord, std::unique_ptr<Chunk> chunk) {
  chunks_[coord] = std::move(chunk);
  ++epoch_;
}

void VoxelWorld::Remove(const ChunkCoord& coord) {
  if (chunks_.erase(coord)) ++epoch_;
}

std::size_t VoxelWorld::EvictUnmodified(const std::function<bool(const ChunkCoord&)>& keep) {
  return Evict(
      [&](const ChunkCoord& c, const Chunk& chunk) { return chunk.revision() == 0 && !keep(c); });
}

std::size_t VoxelWorld::Evict(const std::function<bool(const ChunkCoord&, const Chunk&)>& evict) {
  const std::size_t before = chunks_.size();
  std::erase_if(chunks_, [&](const auto& kv) { return evict(kv.first, *kv.second); });
  const std::size_t evicted = before - chunks_.size();
  if (evicted) ++epoch_;
  return evicted;
}

const Chunk* VoxelWorld::Find(const ChunkCoord& coord) const {
  const auto it = chunks_.find(coord);
  return it == chunks_.end() ? nullptr : it->second.get();
}

MaterialId VoxelWorld::GetVoxel(std::int32_t x, std::int32_t y, std::int32_t z) {
  const auto c = ChunkOf(x, y, z);
  return Read(c).Get(x - c.x * kChunkSize, y - c.y * kChunkSize, z - c.z * kChunkSize);
}

void VoxelWorld::SetVoxel(std::int32_t x, std::int32_t y, std::int32_t z, MaterialId m) {
  const auto c = ChunkOf(x, y, z);
  GetOrCreate(c).Set(x - c.x * kChunkSize, y - c.y * kChunkSize, z - c.z * kChunkSize, m);
}

}  // namespace dwell::core
