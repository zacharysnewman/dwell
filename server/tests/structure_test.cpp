// Structure files (core/structure.h): hand-built geometry as data, stamped over a base generator.
#include <doctest/doctest.h>

#include <memory>
#include <string>

#include "dwell/core/block_registry.h"
#include "dwell/core/structure.h"
#include "player/playground.h"

namespace dwell::core {

TEST_SUITE("structure files") {
  TEST_CASE("fills name blocks by state string, later fills win") {
    std::string error;
    const auto s = ParseStructure(
        "# a pillar with a ladder\n"
        "fill 0 0 0  0 3 0  dwell:stone\n"
        "fill 1 3 1  1 0 1  dwell:ladder[facing=east]   # corners in any order\n"
        "\n"
        "fill 0 3 0  0 3 0  dwell:sand\n",
        &error);
    REQUIRE_MESSAGE(s, error);
    REQUIRE(s->fills.size() == 3);
    CHECK(s->At(0, 0, 0) == Materials::kStone);
    CHECK(s->At(0, 3, 0) == Materials::kSand);
    CHECK(s->At(1, 2, 1) == Materials::kLadderE);
    CHECK_FALSE(s->At(2, 0, 0));
  }

  TEST_CASE("a bad line fails with its number and reason") {
    std::string error;
    CHECK_FALSE(ParseStructure("fill 0 0 0 1 1 1 dwell:stone\nfill 0 0 0 1 1 dwell:stone\n", &error));
    CHECK(error.find("line 2") != std::string::npos);
    CHECK_FALSE(ParseStructure("fill 0 0 0 1 1 1 dwell:bedrock\n", &error));
    CHECK(error.find("dwell:bedrock") != std::string::npos);
    CHECK_FALSE(ParseStructure("box 0 0 0 1 1 1 dwell:stone\n", &error));
    CHECK(error.find("unknown command") != std::string::npos);
    CHECK_FALSE(ParseStructure("fill 0 0 x 1 1 1 dwell:stone\n", &error));
    CHECK(error.find("bad coordinate") != std::string::npos);
  }

  TEST_CASE("stamped over a base generator, only inside its boxes") {
    auto s = ParseStructure("fill -1 0 -1  1 1 1  dwell:stone\n");
    REQUIRE(s);
    const auto structure = std::make_shared<const Structure>(std::move(*s));
    const ChunkGenerator gen = WithStructure(ChunkGenerator(GenerateFlatChunk), structure);
    VoxelWorld world(gen);
    CHECK(world.GetVoxel(0, 1, 0) == Materials::kStone);
    CHECK(world.GetVoxel(-1, 0, 1) == Materials::kStone);
    CHECK(world.GetVoxel(2, 0, 0) == Materials::kAir);
    CHECK(world.GetVoxel(5, -1, 5) == Materials::kGrass);  // the flat world elsewhere
    // The air test sees the structure's chunks as not air.
    const AirChunkTest air = WithStructure(AirTestFor(kGeneratorFlat), structure);
    CHECK_FALSE(air({0, 0, 0}));
    CHECK_FALSE(air({-1, 0, -1}));
    CHECK(air({3, 0, 3}));
  }

  TEST_CASE("the movement playground file loads") {
    const auto playground = test::PlaygroundStructure();
    CHECK(playground->At(9, 0, 9) == Materials::kLadderN);   // the ladder
    CHECK(playground->At(0, 0, 12) == Materials::kAir);      // the doorway
    CHECK(playground->At(13, -1, 7) == Materials::kWater);   // the pool
  }
}

}  // namespace dwell::core
