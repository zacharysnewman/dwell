#include "dwell/core/structure.h"

#include <algorithm>
#include <charconv>
#include <fstream>
#include <sstream>

#include "dwell/core/block_registry.h"

namespace dwell::core {
namespace {

std::int32_t FloorDiv(std::int32_t a, std::int32_t b) {
  const std::int32_t q = a / b;
  return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q;
}

// Splits a line into whitespace-separated words.
std::vector<std::string_view> Words(std::string_view line) {
  std::vector<std::string_view> out;
  std::size_t i = 0;
  while (i < line.size()) {
    while (i < line.size() && (line[i] == ' ' || line[i] == '\t' || line[i] == '\r')) ++i;
    const std::size_t start = i;
    while (i < line.size() && line[i] != ' ' && line[i] != '\t' && line[i] != '\r') ++i;
    if (i > start) out.push_back(line.substr(start, i - start));
  }
  return out;
}

bool ParseInt(std::string_view s, std::int32_t& out) {
  const auto [p, ec] = std::from_chars(s.data(), s.data() + s.size(), out);
  return ec == std::errc() && p == s.data() + s.size();
}

}  // namespace

std::optional<MaterialId> Structure::At(std::int32_t x, std::int32_t y, std::int32_t z) const {
  for (auto it = fills.rbegin(); it != fills.rend(); ++it) {
    if (x >= it->x0 && x <= it->x1 && y >= it->y0 && y <= it->y1 && z >= it->z0 && z <= it->z1) {
      return it->material;
    }
  }
  return std::nullopt;
}

bool Structure::Misses(const ChunkCoord& c) const {
  return std::none_of(fills.begin(), fills.end(), [&](const Fill& f) {
    return FloorDiv(f.x0, kChunkSize) <= c.x && FloorDiv(f.x1, kChunkSize) >= c.x &&
           FloorDiv(f.y0, kChunkSize) <= c.y && FloorDiv(f.y1, kChunkSize) >= c.y &&
           FloorDiv(f.z0, kChunkSize) <= c.z && FloorDiv(f.z1, kChunkSize) >= c.z;
  });
}

std::optional<Structure> ParseStructure(std::string_view text, std::string* error) {
  Structure s;
  int number = 0;
  const auto fail = [&](const std::string& why) -> std::optional<Structure> {
    if (error) *error = "line " + std::to_string(number) + ": " + why;
    return std::nullopt;
  };
  while (!text.empty()) {
    ++number;
    const std::size_t end = text.find('\n');
    std::string_view line = text.substr(0, end);
    text = end == std::string_view::npos ? std::string_view{} : text.substr(end + 1);
    if (const std::size_t hash = line.find('#'); hash != std::string_view::npos) {
      line = line.substr(0, hash);
    }
    const std::vector<std::string_view> w = Words(line);
    if (w.empty()) continue;
    if (w[0] != "fill") return fail("unknown command '" + std::string(w[0]) + "'");
    if (w.size() != 8) return fail("fill takes x0 y0 z0 x1 y1 z1 and a block state");
    std::int32_t v[6];
    for (int i = 0; i < 6; ++i) {
      if (!ParseInt(w[static_cast<std::size_t>(i + 1)], v[i])) {
        return fail("bad coordinate '" + std::string(w[static_cast<std::size_t>(i + 1)]) + "'");
      }
    }
    std::string why;
    const auto m = ParseState(w[7], &why);
    if (!m) return fail("block state '" + std::string(w[7]) + "': " + why);
    s.fills.push_back({std::min(v[0], v[3]), std::min(v[1], v[4]), std::min(v[2], v[5]),
                       std::max(v[0], v[3]), std::max(v[1], v[4]), std::max(v[2], v[5]), *m});
  }
  return s;
}

std::optional<Structure> LoadStructure(const std::string& path, std::string* error) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    if (error) *error = "cannot open " + path;
    return std::nullopt;
  }
  std::ostringstream text;
  text << in.rdbuf();
  auto s = ParseStructure(text.str(), error);
  if (!s && error) *error = path + ": " + *error;
  return s;
}

ChunkGenerator WithStructure(ChunkGenerator base, std::shared_ptr<const Structure> structure) {
  return [base = std::move(base), structure](const ChunkCoord& coord, Chunk& chunk) {
    base(coord, chunk);
    if (structure->Misses(coord)) return;
    auto& voxels = chunk.generation_voxels();
    for (int lz = 0; lz < kChunkSize; ++lz)
      for (int ly = 0; ly < kChunkSize; ++ly)
        for (int lx = 0; lx < kChunkSize; ++lx) {
          const auto m = structure->At(coord.x * kChunkSize + lx, coord.y * kChunkSize + ly,
                                       coord.z * kChunkSize + lz);
          if (m) voxels[static_cast<std::size_t>(LocalIndex(lx, ly, lz))] = *m;
        }
  };
}

AirChunkTest WithStructure(AirChunkTest base_air, std::shared_ptr<const Structure> structure) {
  return [base_air = std::move(base_air), structure](const ChunkCoord& coord) {
    return structure->Misses(coord) && base_air(coord);
  };
}

}  // namespace dwell::core
