// The bifacial world's terrain (BIFACIAL_WORLD.md §2, §5, ADR 0023): the mirror mapping, face B as
// the flip of a face-local chunk, its own terrain, no bedrock at the midplane, and the air tests.
// Runs natively (dwell_tests) and under Node (dwell_worldgen_tests.js, CI).
#include <doctest/doctest.h>

#include <algorithm>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/voxel.h"
#include "dwell/worldgen/bifacial.h"

using namespace dwell;
using core::Chunk;
using core::ChunkCoord;
using core::Face;
using core::MaterialId;
using worldgen::BifacialTerrain;
using worldgen::TerrainGenerator;
namespace M = core::Materials;

namespace {

constexpr int S = core::kChunkSize;

Chunk Generate(const BifacialTerrain& gen, ChunkCoord c) {
  Chunk chunk;
  gen.Generate(c, chunk);
  return chunk;
}

}  // namespace

TEST_SUITE("bifacial: mirror") {
  TEST_CASE("the mirror maps voxels onto voxels and chunks onto chunks, and is its own inverse") {
    CHECK(core::kMidplaneY == -2048);
    CHECK(core::kMidplaneY % S == 0);
    CHECK(core::MirrorY(-2048) == -2049);  // the two layers either side of the midplane swap
    CHECK(core::MirrorY(-2049) == -2048);
    CHECK(core::MirrorY(-4096) == -1);  // face B's sea surface (y = −4,096) is the top water layer
    CHECK(core::MirrorY(-4097) == 0);   // … and the voxel just beyond it the first of the sky
    CHECK(core::MirrorChunkY(-64) == -65);
    CHECK(core::MirrorChunkY(-65) == -64);
    for (const int y : {-10240, -5000, -2049, -2048, -1, 0, 6143}) {
      CHECK(core::MirrorY(core::MirrorY(y)) == y);
      // A chunk's rows mirror within the mirrored chunk.
      const int cy = y >= 0 ? y / S : -((-y + S - 1) / S);
      const int local = core::MirrorY(y);
      CHECK(core::MirrorChunkY(cy) * S <= local);
      CHECK(local < (core::MirrorChunkY(cy) + 1) * S);
    }
    CHECK(core::FaceOfY(-2048) == Face::kA);
    CHECK(core::FaceOfY(-2049) == Face::kB);
    CHECK(core::FaceOfChunkY(-64) == Face::kA);
    CHECK(core::FaceOfChunkY(-65) == Face::kB);
    // Face B's ground band is [−10,240, −2,048): its lowest row is face-local 6,143.
    CHECK(core::MirrorY(core::kWorldBottomY) == core::kWorldMaxY - 1);
    CHECK(core::kMinChunkY == -320);
    CHECK(core::MirrorChunkY(core::kMinChunkY) == core::kMaxChunkY);
    const ChunkCoord b{3, -70, -4};
    CHECK(core::FaceLocalChunk(b) == ChunkCoord{3, -59, -4});
    const ChunkCoord a{3, 12, -4};
    CHECK(core::FaceLocalChunk(a) == a);
  }

  TEST_CASE("turning a block upside down swaps the half of slabs and slopes only") {
    const auto state = [](const char* text) {
      const auto id = core::ParseState(text);
      REQUIRE(id.has_value());
      return *id;
    };
    const MaterialId slab_bottom = state("dwell:stone_slab[flooded=false,half=bottom]");
    const MaterialId slab_top = state("dwell:stone_slab[flooded=false,half=top]");
    CHECK(core::MirrorMaterial(slab_bottom) == slab_top);
    CHECK(core::MirrorMaterial(slab_top) == slab_bottom);
    const MaterialId slope =
        state("dwell:grass_slope[facing=east,flooded=true,half=bottom,shape=outer]");
    const MaterialId flipped =
        state("dwell:grass_slope[facing=east,flooded=true,half=top,shape=outer]");
    CHECK(core::MirrorMaterial(slope) == flipped);
    CHECK(core::MirrorMaterial(flipped) == slope);
    for (const MaterialId m :
         {M::kAir, M::kStone, M::kWater, M::kGrass, M::kLadderN, M::kLadderW}) {
      CHECK(core::MirrorMaterial(m) == m);
    }
    // An involution over every state.
    for (MaterialId m = 0; m < M::kCount; ++m)
      CHECK(core::MirrorMaterial(core::MirrorMaterial(m)) == m);
  }
}

TEST_SUITE("bifacial: generation") {
  TEST_CASE("a chunk of face B is the vertical flip of its face-local chunk") {
    for (const std::uint64_t seed : {0ull, 20260925ull}) {
      CAPTURE(seed);
      const BifacialTerrain gen(seed);
      // Rows near the midplane, the rock between, and the sea and shore of face B (face-local rows
      // −1..1 are −129 − (−1)… i.e. −128..−130).
      for (const ChunkCoord c :
           {ChunkCoord{0, -65, 0}, ChunkCoord{4, -66, -7}, ChunkCoord{-3, -100, 5},
            ChunkCoord{0, -128, 0}, ChunkCoord{0, -129, 0}, ChunkCoord{0, -130, 0},
            ChunkCoord{9, -131, 2}}) {
        const Chunk flipped = Generate(gen, c);
        Chunk local;
        gen.Local(Face::kB).Generate(core::FaceLocalChunk(c), local);
        int solid = 0;
        for (int z = 0; z < S; ++z)
          for (int y = 0; y < S; ++y)
            for (int x = 0; x < S; ++x) {
              const MaterialId expect = core::MirrorMaterial(local.Get(x, S - 1 - y, z));
              if (flipped.Get(x, y, z) != expect) {
                FAIL_CHECK("chunk (" << c.x << "," << c.y << "," << c.z << ") voxel " << x << ","
                                     << y << "," << z);
              }
              solid += expect != M::kAir && expect != M::kWater;
            }
        CAPTURE(c.y);
        CHECK(solid > 0);
      }
    }
  }

  TEST_CASE("face A is unchanged above the midplane: the same chunk as the face-local generator") {
    const BifacialTerrain gen(7);
    for (const ChunkCoord c : {ChunkCoord{0, -64, 0}, ChunkCoord{0, -1, 0}, ChunkCoord{3, 0, -2}}) {
      Chunk local;
      gen.Local(Face::kA).Generate(c, local);
      CHECK(core::ChunkHash(Generate(gen, c)) == core::ChunkHash(local));
    }
    CHECK(gen.Local(Face::kA).SpawnPoint() == gen.SpawnPoint());
  }

  TEST_CASE("face B is its own landscape, not a reflection of face A's") {
    for (const std::uint64_t seed : {0ull, 5ull}) {
      CAPTURE(seed);
      const BifacialTerrain gen(seed);
      CHECK(worldgen::FaceSeed(seed, Face::kA) != worldgen::FaceSeed(seed, Face::kB));
      int differing = 0, columns = 0;
      for (int i = -20; i <= 20; ++i)
        for (int j = -20; j <= 20; ++j) {
          const std::int32_t x = i * 40000, z = j * 40000;
          const auto a = gen.Local(Face::kA).ColumnAt(x, z), b = gen.Local(Face::kB).ColumnAt(x, z);
          if (a.outside || b.outside) continue;
          ++columns;
          differing += std::abs(a.height - b.height) > 8.0f;
        }
      CHECK(columns > 500);
      CHECK(differing > columns / 2);
    }
  }

  TEST_CASE("no bedrock is generated and every voxel of the core can be dug") {
    for (const std::uint64_t seed : {0ull, 3ull}) {
      CAPTURE(seed);
      const BifacialTerrain gen(seed);
      int solid = 0, checked = 0;
      for (const ChunkCoord c :
           {ChunkCoord{0, -64, 0}, ChunkCoord{0, -65, 0}, ChunkCoord{200, -64, -30},
            ChunkCoord{200, -65, -30}, ChunkCoord{-12, -64, 40}, ChunkCoord{-12, -65, 40}}) {
        const Chunk chunk = Generate(gen, c);
        for (const MaterialId m : chunk.voxels()) {
          CHECK(m != M::kBedrock);
          CHECK_FALSE(core::GetMaterial(m).indestructible);
          ++checked;
          solid += m != M::kAir && m != M::kWater;
        }
      }
      // The core is rock across the midplane: the two layers either side of it are solid
      // everywhere (caves fade out above the face's floor).
      for (const std::int32_t x : {0, 32 * 200 + 5, -32 * 12 + 9}) {
        const std::int32_t z = x == 0 ? 0 : x > 0 ? -30 * 32 + 3 : 40 * 32 + 1;
        for (const std::int32_t y : {-2049, -2048}) {
          CHECK(gen.SolidAt(x, y, z));
        }
      }
      CHECK(solid > checked / 2);
    }
  }

  TEST_CASE("point queries by face agree with the chunk path") {
    const BifacialTerrain gen(11);
    // Terrain stages only: no slopes, stability or features, so solidity is the point query's.
    for (const ChunkCoord c :
         {ChunkCoord{0, -66, 0}, ChunkCoord{5, -129, 5}, ChunkCoord{0, -130, 0}}) {
      Chunk chunk;
      gen.Generate(c, chunk, 0);
      for (int z = 0; z < S; z += 3)
        for (int y = 0; y < S; ++y)
          for (int x = 0; x < S; x += 3) {
            const MaterialId m = chunk.Get(x, y, z);
            const bool expect = m != M::kAir && m != M::kWater;
            if (gen.SolidAt(c.x * S + x, c.y * S + y, c.z * S + z) != expect) {
              FAIL_CHECK("chunk (" << c.x << "," << c.y << "," << c.z << ") voxel " << x << "," << y
                                   << "," << z);
            }
          }
    }
  }

  TEST_CASE("the air test knows face B's sky, sea and rows outside the world") {
    const std::uint64_t seed = 5;
    const auto air = core::AirTestFor(core::kGeneratorTerrain, seed);
    const BifacialTerrain gen(seed);
    int sky = 0, checked = 0;
    for (int cx = -3; cx <= 3; ++cx)
      for (int cz = -3; cz <= 3; ++cz)
        for (const int cy : {-66, -65, -130, -129, -128, -127, -200, -320, -321}) {
          const ChunkCoord c{cx * 997, cy, cz * 1013};
          const bool is_air = air(c);
          ++checked;
          sky += is_air;
          CHECK(is_air == gen.IsAirChunk(c));
          if (is_air) {
            const Chunk chunk = Generate(gen, c);
            for (const MaterialId m : chunk.voxels()) CHECK(m == M::kAir);
          }
        }
    CHECK(sky > 0);
    CHECK(sky < checked);
    // Face B's high rows (the sky below the disc) are air, like face A's above.
    for (const int cy : {-129 - 100, -129 - 150, core::kMinChunkY}) {
      CHECK(air({0, cy, 0}));
      CHECK(air({1000, cy, -1000}));
    }
    // Rows beyond either end of the world.
    CHECK(air({0, core::kMinChunkY - 1, 0}));
    CHECK(air({0, core::kMaxChunkY + 1, 0}));
    // Every chunk the test calls air generates as air, on both sides (the shared cache is keyed by
    // face, so a column's two floors do not collide).
    for (const int cy : {-1, 0, 5, -65 - 1, -129, -129 - 5}) {
      for (const int cx : {0, 31, 90}) {
        const ChunkCoord c{cx, cy, 2 * cx + 1};
        if (air(c)) {
          const Chunk chunk = Generate(gen, c);
          for (const MaterialId m : chunk.voxels()) CHECK(m == M::kAir);
        }
      }
    }
  }
}
