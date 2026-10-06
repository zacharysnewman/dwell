// Slope shaping of generated terrain: which piece a cell becomes from its corner heights
// (docs/SLOPE_BLOCKS.md §5).
#include <doctest/doctest.h>

#include <algorithm>
#include <set>

#include "dwell/core/block_registry.h"
#include "dwell/core/block_shape.h"
#include "dwell/worldgen/slopes.h"

using namespace dwell::core;
using namespace dwell::worldgen::slopes;

namespace {

int Distance(const int (&a)[4], const int (&b)[4]) {
  int d = 0;
  for (int i = 0; i < 4; ++i) d += std::abs(a[i] - b[i]);
  return d;
}

}  // namespace

TEST_SUITE("worldgen: slope pieces") {
  TEST_CASE("a corner pattern that has a piece gets exactly that piece") {
    // Every shape in every facing, plus cubes, slabs and air.
    std::set<int> allowed;
    for (const char* facing : {"north", "east", "south", "west"}) {
      for (const char* shape :
           {"wedge", "outer", "inner", "gentle_low", "gentle_high", "gentle_outer_low",
            "gentle_outer_high", "gentle_inner_low", "gentle_inner_high"}) {
        const auto state = ParseState(std::string("dwell:stone_slope[facing=") + facing +
                                      ",flooded=false,half=bottom,shape=" + shape + "]");
        REQUIRE(state);
        const auto& c = ShapeOf(*state).corners;
        const Piece piece = PieceFor(c[0], c[1], c[2], c[3]);
        REQUIRE(piece.kind == Kind::kShaped);
        int back[4];
        CornersOf(piece, back);
        for (int i = 0; i < 4; ++i) CHECK(back[i] == c[i]);
        // And it is the state the registry names: the shaped state of stone round-trips.
        CHECK(StateFor(Materials::kStone, piece, false) == *state);
        allowed.insert(PatternIndex(c[0], c[1], c[2], c[3]));
      }
    }
    CHECK(allowed.size() == 36);  // 9 shapes × 4 facings, all distinct
    CHECK(PieceFor(0, 0, 0, 0).kind == Kind::kAir);
    CHECK(PieceFor(2, 2, 2, 2).kind == Kind::kFull);
    const Piece slab = PieceFor(1, 1, 1, 1);
    REQUIRE(slab.kind == Kind::kShaped);
    CHECK(StateFor(Materials::kStone, slab, false) == Materials::kStoneSlab);
  }

  TEST_CASE("every other pattern maps to the nearest piece, raising the lowest corner on a tie") {
    std::vector<std::array<int, 4>> pieces;  // every allowed pattern
    for (int p = 0; p < 81; ++p) {
      const int c[4] = {p % 3, (p / 3) % 3, (p / 9) % 3, (p / 27) % 3};
      const Piece piece = PieceFor(c[0], c[1], c[2], c[3]);
      int got[4];
      CornersOf(piece, got);
      // Nothing nearer exists.
      for (int q = 0; q < 81; ++q) {
        const int other[4] = {q % 3, (q / 3) % 3, (q / 9) % 3, (q / 27) % 3};
        const Piece candidate = PieceFor(other[0], other[1], other[2], other[3]);
        int cc[4];
        CornersOf(candidate, cc);
        if (Distance(other, cc) == 0) {  // an allowed pattern
          CHECK(Distance(c, got) <= Distance(c, other));
        }
      }
      // The result is itself an allowed pattern (a fixed point).
      const Piece again = PieceFor(got[0], got[1], got[2], got[3]);
      int back[4];
      CornersOf(again, back);
      for (int i = 0; i < 4; ++i) CHECK(back[i] == got[i]);
    }
    // A saddle (high, low, high, low) cannot be a piece: it becomes a neighbouring one.
    const Piece saddle = PieceFor(2, 0, 2, 0);
    int c[4];
    CornersOf(saddle, c);
    CHECK(Distance({2, 0, 2, 0}, c) <= 2);
  }

  TEST_CASE("states keep the material, are upright, and flood on request") {
    for (const MaterialId cube : {Materials::kGrass, Materials::kSand, Materials::kSnow,
                                  Materials::kDirt, Materials::kStone, Materials::kGravel}) {
      CAPTURE(StateString(cube));
      REQUIRE(HasFamily(cube));
      const Piece wedge = PieceFor(2, 0, 0, 2);
      const MaterialId dry = StateFor(cube, wedge, false), wet = StateFor(cube, wedge, true);
      CHECK(dry != cube);
      CHECK(wet != dry);
      CHECK_FALSE(GetMaterial(dry).flooded);
      CHECK(GetMaterial(wet).flooded);
      CHECK_FALSE(ShapeOf(dry).inverted);
      CHECK(GetMaterial(dry).density_kg_m3 == GetMaterial(cube).density_kg_m3);
      CHECK(BlockOf(dry).id.substr(0, BlockOf(cube).id.size()) == BlockOf(cube).id);
    }
    // No family: unchanged.
    CHECK_FALSE(HasFamily(Materials::kLeaves));
    CHECK(StateFor(Materials::kLeaves, PieceFor(2, 0, 0, 2), false) == Materials::kLeaves);
    CHECK(StateFor(Materials::kStone, PieceFor(2, 2, 2, 2), true) == Materials::kStone);
    CHECK(StateFor(Materials::kStone, PieceFor(0, 0, 0, 0), true) == Materials::kAir);
  }
}
