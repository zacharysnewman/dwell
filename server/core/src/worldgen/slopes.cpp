#include "dwell/worldgen/slopes.h"

#include <array>
#include <string>
#include <unordered_map>
#include <vector>

#include "dwell/core/block_registry.h"
#include "dwell/core/block_shape.h"

namespace dwell::worldgen::slopes {
namespace {

using core::MaterialId;
namespace M = core::Materials;

constexpr int kPatterns = 81;

struct Tables {
  // Pattern → whether a piece with exactly these corner heights exists.
  std::array<bool, kPatterns> allowed{};
  // Pattern → the pattern of the piece used for it.
  std::array<std::uint8_t, kPatterns> nearest{};
  // Allowed pattern → (facing, shape) property values of its slope state ("" shape = the slab).
  std::array<std::string, kPatterns> facing, shape;
  // Cube material → per pattern and flooded flag, the shaped state (0 = none).
  std::unordered_map<MaterialId, std::array<MaterialId, kPatterns * 2>> states;
};

int Distance(int a, int b) {
  int d = 0;
  for (int i = 0; i < 4; ++i) {
    const int x = (a / (i == 0 ? 1 : i == 1 ? 3 : i == 2 ? 9 : 27)) % 3;
    const int y = (b / (i == 0 ? 1 : i == 1 ? 3 : i == 2 ? 9 : 27)) % 3;
    d += x > y ? x - y : y - x;
  }
  return d;
}

int Height(int pattern) {
  return pattern % 3 + (pattern / 3) % 3 + (pattern / 9) % 3 + (pattern / 27) % 3;
}

const Tables& GetTables() {
  static const Tables tables = [] {
    Tables t;
    // The registry's own upright shapes of a reference material give the allowed patterns.
    for (const char* facing : {"east", "south", "west", "north"}) {
      for (const char* shape :
           {"wedge", "outer", "inner", "gentle_low", "gentle_high", "gentle_outer_low",
            "gentle_outer_high", "gentle_inner_low", "gentle_inner_high"}) {
        const auto state = core::ParseState(std::string("dwell:stone_slope[facing=") + facing +
                                            ",flooded=false,half=bottom,shape=" + shape + "]");
        if (!state) continue;
        const auto& c = core::ShapeOf(*state).corners;
        const int p = PatternIndex(c[0], c[1], c[2], c[3]);
        if (!t.allowed[p]) {
          t.allowed[p] = true;
          t.facing[p] = facing;
          t.shape[p] = shape;
        }
      }
    }
    for (const int uniform : {0, PatternIndex(1, 1, 1, 1), PatternIndex(2, 2, 2, 2)}) {
      t.allowed[uniform] = true;
    }
    t.shape[PatternIndex(1, 1, 1, 1)] = "slab";
    for (int p = 0; p < kPatterns; ++p) {
      int best = -1;
      for (int q = 0; q < kPatterns; ++q) {
        if (!t.allowed[q]) continue;
        if (best < 0) {
          best = q;
          continue;
        }
        const int dq = Distance(p, q), db = Distance(p, best);
        if (dq < db || (dq == db && Height(q) > Height(best))) best = q;
      }
      t.nearest[p] = static_cast<std::uint8_t>(best);
    }
    // States per cube material: the materials with `<id>_slope` and `<id>_slab` blocks.
    for (MaterialId cube = 0; cube < M::kCount; ++cube) {
      const core::BlockDef& block = core::BlockOf(cube);
      if (block.state_count != 1) continue;
      const std::string id(block.id);
      if (!core::FindBlock(id + "_slope")) continue;
      auto& row = t.states[cube];
      row.fill(cube);
      for (int p = 0; p < kPatterns; ++p) {
        if (!t.allowed[p] || t.shape[p].empty() || p == 0 || p == PatternIndex(2, 2, 2, 2)) {
          continue;
        }
        for (int flooded = 0; flooded < 2; ++flooded) {
          const char* f = flooded ? "true" : "false";
          const std::string text = t.shape[p] == "slab"
                                       ? id + "_slab[flooded=" + f + ",half=bottom]"
                                       : id + "_slope[facing=" + t.facing[p] + ",flooded=" + f +
                                             ",half=bottom,shape=" + t.shape[p] + "]";
          if (const auto state = core::ParseState(text)) row[p * 2 + flooded] = *state;
        }
      }
    }
    return t;
  }();
  return tables;
}

}  // namespace

Piece PieceFor(int nw, int ne, int se, int sw) {
  const Tables& t = GetTables();
  const int p = t.nearest[PatternIndex(nw, ne, se, sw)];
  if (p == 0) return {Kind::kAir, 0};
  if (p == PatternIndex(2, 2, 2, 2)) return {Kind::kFull, 0};
  return {Kind::kShaped, static_cast<std::uint8_t>(p)};
}

void CornersOf(const Piece& piece, int (&corners)[4]) {
  const int p = piece.kind == Kind::kFull ? PatternIndex(2, 2, 2, 2) : piece.pattern;
  corners[0] = p % 3;
  corners[1] = (p / 3) % 3;
  corners[2] = (p / 9) % 3;
  corners[3] = (p / 27) % 3;
}

bool HasFamily(MaterialId cube) { return GetTables().states.count(cube) != 0; }

MaterialId StateFor(MaterialId cube, const Piece& piece, bool flooded) {
  if (piece.kind == Kind::kAir) return M::kAir;
  if (piece.kind == Kind::kFull) return cube;
  const Tables& t = GetTables();
  const auto it = t.states.find(cube);
  if (it == t.states.end()) return cube;
  return it->second[piece.pattern * 2 + (flooded ? 1 : 0)];
}

}  // namespace dwell::worldgen::slopes
