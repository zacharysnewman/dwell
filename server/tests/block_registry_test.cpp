// The block registry (docs/BLOCK_REGISTRY.md): canonical strings, parsing, properties and the
// registry hash, against the vector the TypeScript registry is checked with too
// (shared/blocks/vectors.txt).
#include <doctest/doctest.h>

#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/voxel.h"

using namespace dwell::core;

namespace {

struct Vector {
  std::uint64_t hash = 0;
  std::vector<std::string> states;
};

Vector ReadVector() {
  std::ifstream in(DWELL_BLOCK_VECTORS);
  REQUIRE_MESSAGE(in.good(), "missing " << DWELL_BLOCK_VECTORS);
  Vector v;
  for (std::string line; std::getline(in, line);) {
    if (line.empty() || line[0] == '#') continue;
    if (line.starts_with("hash ")) {
      v.hash = std::stoull(line.substr(5), nullptr, 16);
    } else {
      v.states.push_back(line.substr(line.find(' ') + 1));
    }
  }
  return v;
}

MaterialId Parse(std::string_view text) {
  std::string error;
  const auto id = ParseState(text, &error);
  REQUIRE_MESSAGE(id, error);
  return *id;
}

}  // namespace

TEST_CASE(
    "block registry: the states and hash match the shared vector (the TypeScript registry's)") {
  const Vector v = ReadVector();
  REQUIRE(v.states.size() == Materials::kCount);
  for (MaterialId id = 0; id < Materials::kCount; ++id) CHECK(StateString(id) == v.states[id]);
  CHECK(kRegistryHash == v.hash);
  CHECK(ComputeRegistryHash() == kRegistryHash);
}

TEST_CASE("block registry: every state's canonical string round-trips") {
  for (MaterialId id = 0; id < Materials::kCount; ++id) {
    CAPTURE(id);
    CHECK(ParseState(StateString(id)) == id);
  }
  CHECK(StateString(Materials::kStone) == "dwell:stone");
  CHECK(StateString(Materials::kLadderFacingEastFloodedTrue) ==
        "dwell:ladder[facing=east,flooded=true]");
  CHECK(StateString(Materials::kCount) == "dwell:air");  // unknown ids read as air
}

TEST_CASE("block registry: parsing takes any key order and defaults, and rejects bad text") {
  CHECK(Parse("dwell:ladder") == Materials::kLadder);
  CHECK(Parse("dwell:ladder[]") == Materials::kLadder);
  CHECK(Parse("dwell:ladder[flooded=true,facing=south]") ==
        Parse("dwell:ladder[facing=south,flooded=true]"));
  CHECK(Parse("dwell:ladder[facing=west]") == Materials::kLadderFacingWest);
  for (const char* bad :
       {"dwell:nothing", "stone", "dwell:stone[facing=north]", "dwell:ladder[facing=up]",
        "dwell:ladder[facing=north,facing=east]", "dwell:ladder[facing]",
        "dwell:ladder[facing=north,]", "dwell:ladder[facing=north", ""}) {
    CAPTURE(bad);
    std::string error;
    CHECK_FALSE(ParseState(bad, &error));
    CHECK_FALSE(error.empty());
  }
}

TEST_CASE("block registry: properties are read and set by name") {
  CHECK(StateProperty(Materials::kLadder, "facing") == "north");
  CHECK(StateProperty(Materials::kLadderFacingSouthFloodedTrue, "flooded") == "true");
  CHECK_FALSE(StateProperty(Materials::kLadder, "half"));
  CHECK_FALSE(StateProperty(Materials::kStone, "facing"));
  CHECK(WithProperty(Materials::kLadder, "facing", "east") == Materials::kLadderFacingEast);
  CHECK(WithProperty(Materials::kLadderFacingEast, "flooded", "true") ==
        Materials::kLadderFacingEastFloodedTrue);
  CHECK_FALSE(WithProperty(Materials::kLadder, "facing", "up"));
  CHECK_FALSE(WithProperty(Materials::kStone, "facing", "east"));
  CHECK(&BlockOf(Materials::kLadderFacingWest) == FindBlock("dwell:ladder"));
}

TEST_CASE("block registry: behaviour comes from the data files") {
  CHECK(GetMaterial(Materials::kLadderFacingEast).climbable);
  CHECK(GetMaterial(Materials::kLadderFacingEast).facing == Facing::kEast);
  CHECK(GetMaterial(Materials::kStoneSlab).shape == VoxelShape::kSlabBottom);
  CHECK(GetMaterial(Materials::kWater).liquid);
  CHECK(GetMaterial(Materials::kBedrock).indestructible);
  CHECK(GetMaterial(Materials::kLaunchPad).launch_speed == 14.0f);
  CHECK_FALSE(GetMaterial(Materials::kLadder).solid);
  CHECK(GetMaterial(Materials::kStone).solid);
}
