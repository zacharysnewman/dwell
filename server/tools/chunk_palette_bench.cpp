// Paletted chunk containers vs the plain u16 array (Phase 8 measurement, ADR 0015): memory and the
// cost of a collision-style scan over generated terrain chunks. Build the release preset for
// meaningful timings: build/release/tools/dwell_chunk_palette_bench.
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <map>
#include <memory>
#include <vector>
#include "dwell/core/voxel.h"
#include "dwell/worldgen/terrain.h"
using namespace dwell::core;

struct Packed {
  std::vector<std::uint16_t> palette;
  std::vector<std::uint64_t> words;
  int bits = 0;
  explicit Packed(const Chunk& c) {
    std::map<std::uint16_t, int> idx;
    for (auto m : c.voxels())
      if (!idx.count(m)) {
        idx[m] = 0;
      }
    for (auto& [m, i] : idx) {
      i = (int)palette.size();
      palette.push_back(m);
    }
    int n = (int)palette.size();
    bits = n <= 1 ? 0 : 1;
    while ((1 << bits) < n) ++bits;
    if (bits == 0) return;
    words.assign((kChunkVolume * bits + 63) / 64 + 1, 0);
    for (int i = 0; i < kChunkVolume; ++i) {
      std::uint64_t v = idx[c.voxels()[i]];
      long bit = (long)i * bits;
      words[bit >> 6] |= v << (bit & 63);
      if ((bit & 63) + bits > 64) words[(bit >> 6) + 1] |= v >> (64 - (bit & 63));
    }
  }
  std::uint16_t Get(int i) const {
    if (bits == 0) return palette[0];
    long bit = (long)i * bits;
    int sh = bit & 63;
    std::uint64_t v = words[bit >> 6] >> sh;
    if (sh + bits > 64) v |= words[(bit >> 6) + 1] << (64 - sh);
    return palette[v & ((1ull << bits) - 1)];
  }
  std::size_t Bytes() const { return palette.size() * 2 + words.size() * 8; }
};

template <class Get>
long Scan(
    Get get) {  // the collision/meshing access pattern: each voxel and its in-chunk neighbours
  long solid_faces = 0;
  for (int z = 1; z < 31; ++z)
    for (int y = 1; y < 31; ++y)
      for (int x = 1; x < 31; ++x) {
        int i = LocalIndex(x, y, z);
        auto m = get(i);
        if (m == 0) continue;
        const int d[6] = {1, -1, 32, -32, 1024, -1024};
        for (int k = 0; k < 6; ++k) solid_faces += get(i + d[k]) == 0;
      }
  return solid_faces;
}

int main() {
  dwell::worldgen::TerrainGenerator gen(0);
  std::vector<std::unique_ptr<Chunk>> chunks;
  for (int cx = -12; cx < 12; ++cx)
    for (int cz = -12; cz < 12; ++cz)
      for (int cy = -8; cy < 8; ++cy) {
        if (gen.IsAirChunk({cx * 7, cy, cz * 7})) continue;
        auto c = std::make_unique<Chunk>();
        gen.Generate({cx * 7, cy, cz * 7}, *c);
        chunks.push_back(std::move(c));
      }
  std::map<int, int> hist;
  std::size_t plain = 0, packed = 0, nonuniform = 0;
  std::vector<Packed> ps;
  for (auto& c : chunks) {
    ps.emplace_back(*c);
    plain += sizeof(c->voxels());
    packed += ps.back().Bytes();
    hist[(int)ps.back().palette.size()]++;
    if (ps.back().palette.size() > 1) ++nonuniform;
  }
  std::printf("chunks %zu (non-uniform %zu)\nplain %.1f MiB, paletted %.1f MiB (%.2fx)\n",
              chunks.size(), nonuniform, plain / 1048576.0, packed / 1048576.0,
              (double)plain / packed);
  for (auto [n, k] : hist) std::printf("  palette %2d: %d chunks\n", n, k);
  auto time = [&](auto fn) {
    auto t0 = std::chrono::steady_clock::now();
    long r = 0;
    for (int rep = 0; rep < 5; ++rep) r += fn();
    auto t1 = std::chrono::steady_clock::now();
    return std::make_pair(std::chrono::duration<double, std::milli>(t1 - t0).count() / 5, r);
  };
  auto [tp, rp] = time([&] {
    long r = 0;
    for (auto& c : chunks) {
      const auto& v = c->voxels();
      r += Scan([&](int i) { return v[i]; });
    }
    return r;
  });
  auto [tk, rk] = time([&] {
    long r = 0;
    for (auto& p : ps) r += Scan([&](int i) { return p.Get(i); });
    return r;
  });
  std::printf("scan all chunks: plain %.1f ms, paletted %.1f ms (%.2fx), checksum %s\n", tp, tk,
              tk / tp, rp == rk ? "equal" : "DIFFERENT");
}
