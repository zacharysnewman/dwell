// Worldgen inspection (Phase 3 debug tooling): an ASCII height map around a point, generation
// timings, and the spawn point.
//   dwell_worldgen_inspect [seed] [centre_x] [centre_z] [metres per character] [slice]
// With "slice", prints a 1:1 vertical section along x through the centre instead of the map.
// Coordinates may be anywhere in the world's disc.
//
// Timings (chunk and LOD generation budgets, reported in every worldgen change):
//   dwell_worldgen_inspect <seed> bench
// Best of five runs of chunk generation around the spawn and of LOD sections at several levels.
//
// Layout statistics over many seeds: the island's extent and land share.
//   dwell_worldgen_inspect <first seed> stats [seeds (16)] [m per sample (250)]
//
// Local map image (height map only: the baseline has no biomes):
//   dwell_worldgen_inspect <seed> map <out.png|ppm> <centre_x> <centre_z> [pixels (512)] [m per
//   pixel (8)]
// Writes an image of the area (north up): the relief lit from the north-west, tinted by height,
// with the shore in sand and the sea in blue — turquoise where shallow, deep blue where deep.
// Prints the share of water columns.
//
// First-person view (screenshots without a browser, in seconds):
//   dwell_worldgen_inspect <seed> view <out.png|ppm> <x> <y> <z> <yaw> <pitch> [width (640)]
//                          [height (360)] [range m (40000)] [fov degrees (70)]
// Ray-marches the generator's terrain from the camera at (x, y, z) (feet; the eye is 1.6 m up): yaw
// 0 looks along +z, 90 along +x, pitch up is positive. The ground is shaded by its slope and
// coloured by height, the sea is blue by depth, and distance fades to the sky's haze. Columns are
// sampled on grids that coarsen with distance (cached per thread, rows split across threads), so a
// 640×360 view takes seconds. This is the terrain's shape and colour only — no blocks or lighting
// model of the game — for reviewing generator changes; the browser script
// (`client/scripts/shots.ts`) is for the game's own look.
//
// Island image (whole island and the sea around it):
//   dwell_worldgen_inspect [seed] disc <out.ppm> [m per pixel (100)] [coast|height]
// Writes a PPM of the area within 40 km of the origin (north up) and prints the land share and
// the island's extent. `coast` shows land and sea by the layout's coast distance; `height` is a
// hill-shaded height map from the full terrain pipeline.
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <vector>

#include "dwell/worldgen/noise.h"
#include "dwell/worldgen/terrain.h"
#include "image.h"

using namespace dwell;

namespace {

// Ground colour by height (m above sea level) and the shore's sand.
void GroundColour(float h, float* rgb) {
  if (h < 2.0f) {
    rgb[0] = 232, rgb[1] = 214, rgb[2] = 150;  // sand
    return;
  }
  const float t = std::clamp(h / 160.0f, 0.0f, 1.0f);  // lowland green → upland
  rgb[0] = 95 + 70 * t, rgb[1] = 160 - 20 * t, rgb[2] = 70 + 30 * t;
}

// Island image: the area within kDiscReach m of the origin, one pixel per `m_per_px` metres,
// north (−z) up.
constexpr int kDiscReach = 40'000;
int WriteDisc(const worldgen::TerrainGenerator& gen, const std::string& path, int m_per_px,
              const std::string& mode) {
  const int n = 2 * kDiscReach / m_per_px;
  const bool height = mode == "height";
  std::vector<std::uint8_t> rgb(static_cast<std::size_t>(n) * n * 3, 0);
  std::vector<float> heights(static_cast<std::size_t>(n) * n, 0.0f);
  long land = 0, inside = 0;
  int reach = 0, nearest_sea = kDiscReach;
  const auto at = [&](int px, int py, int& x, int& z) {
    x = (px - n / 2) * m_per_px + m_per_px / 2;
    z = (py - n / 2) * m_per_px + m_per_px / 2;
  };
  for (int py = 0; py < n; ++py)
    for (int px = 0; px < n; ++px) {
      int x, z;
      at(px, py, x, z);
      if (!core::InsideWorldDisc(x, z)) continue;
      ++inside;
      const float h = height ? gen.ColumnAt(x, z).height : gen.CoastAt(x, z);
      heights[static_cast<std::size_t>(py) * n + px] = h;
      const int r = static_cast<int>(std::sqrt(static_cast<double>(x) * x + static_cast<double>(z) * z));
      if (h > 0.0f) {
        ++land;
        reach = std::max(reach, r);
      } else {
        nearest_sea = std::min(nearest_sea, r);
      }
    }
  for (int py = 0; py < n; ++py)
    for (int px = 0; px < n; ++px) {
      int x, z;
      at(px, py, x, z);
      if (!core::InsideWorldDisc(x, z)) continue;
      const float h = heights[static_cast<std::size_t>(py) * n + px];
      std::uint8_t* o = &rgb[(static_cast<std::size_t>(py) * n + px) * 3];
      float c[3];
      if (h <= 0.0f) {
        const float d = height ? std::min(1.0f, -h / 20.0f) : std::min(1.0f, -h / 10000.0f);
        c[0] = 70 - 50 * d, c[1] = 170 - 100 * d, c[2] = 215 - 90 * d;
      } else if (height) {
        const float east = heights[static_cast<std::size_t>(py) * n + std::min(px + 1, n - 1)];
        const float shade =
            1.0f - std::clamp((east - h) / (static_cast<float>(m_per_px) * 0.5f), -0.4f, 0.4f);
        GroundColour(h, c);
        for (float& v : c) v *= shade;
      } else {
        const float t = std::min(1.0f, h / 15000.0f);
        c[0] = 120 + 60 * t, c[1] = 170 - 40 * t, c[2] = 80;
      }
      for (int k = 0; k < 3; ++k) o[k] = static_cast<std::uint8_t>(std::clamp(c[k], 0.0f, 255.0f));
    }
  std::FILE* f = std::fopen(path.c_str(), "wb");
  if (!f) {
    std::fprintf(stderr, "cannot write %s\n", path.c_str());
    return 1;
  }
  std::fprintf(f, "P6\n%d %d\n255\n", n, n);
  std::fwrite(rgb.data(), 1, rgb.size(), f);
  std::fclose(f);
  std::printf("%s: %d x %d, %d m per pixel (%s)\n", path.c_str(), n, n, m_per_px, mode.c_str());
  std::printf("land share %.1f%% within %d km; land reaches %.1f km, sea from %.1f km\n",
              100.0 * land / std::max(1L, inside), kDiscReach / 1000, reach / 1000.0,
              nearest_sea / 1000.0);
  return 0;
}

}  // namespace

// Best-of-five ms per chunk (a 5 × 5 × 11 block around the surface at the origin) and per LOD
// section (four sections near the origin) at several levels. Uses only what every generator version
// has, so the same code times a baseline.
int Bench(std::uint64_t seed) {
  const worldgen::TerrainGenerator gen(seed);
  const int surface = worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(0, 0).height), 32);
  double best = 1e30;
  for (int run = 0; run < (std::getenv("DWELL_BENCH_ONLY") ? 0 : 5); ++run) {
    std::chrono::steady_clock::duration elapsed{};
    int chunks = 0;
    for (int x = -2; x <= 2; ++x)
      for (int z = -2; z <= 2; ++z)
        for (int y = surface - 4; y <= surface + 6; ++y) {
          core::Chunk c;
          const auto start = std::chrono::steady_clock::now();
          gen.Generate({x, y, z}, c);
          elapsed += std::chrono::steady_clock::now() - start;
          ++chunks;
        }
    best = std::min(best, std::chrono::duration<double, std::milli>(elapsed).count() / chunks);
  }
  std::printf("chunk: %.3f ms\n", best);
  // LOD sections around (DWELL_BENCH_AT=x,z), default the origin: sections at a coast cost more
  // than in an interior. `coast` names the point where the land ends east of the origin.
  std::int64_t at_x = 0, at_z = 0;
  if (std::getenv("DWELL_BENCH_AT") && std::string_view(std::getenv("DWELL_BENCH_AT")) == "coast") {
    while (gen.CoastAt(static_cast<std::int32_t>(at_x), 0) > 0.0f) at_x += 250;
    std::printf("LOD sections at the coast (%ld, 0)\n", static_cast<long>(at_x));
  } else if (const char* at = std::getenv("DWELL_BENCH_AT"))
    std::sscanf(at, "%ld,%ld", &at_x, &at_z);
  for (const int level : {1, 3, 5, 8, 10, 12}) {
    double best_lod = 1e30;
    for (int run = 0; run < 5; ++run) {
      std::chrono::steady_clock::duration elapsed{};
      int sections = 0;
      for (int i = 0; i < 2; ++i)
        for (int k = 0; k < 2; ++k) {
          const std::int64_t size = core::LodSectionSize(level);
          const core::LodCoord c{level,
                                 static_cast<std::int32_t>((at_x - core::kLodOriginX) / size) + i,
                                 static_cast<std::int32_t>((-64 - core::kLodOriginY) / size),
                                 static_cast<std::int32_t>((at_z - core::kLodOriginZ) / size) + k};
          core::LodCells cells;
          const auto start = std::chrono::steady_clock::now();
          gen.GenerateLod(c, cells);
          elapsed += std::chrono::steady_clock::now() - start;
          ++sections;
        }
      best_lod =
          std::min(best_lod, std::chrono::duration<double, std::milli>(elapsed).count() / sections);
    }
    std::printf("lod level %2d: %.3f ms/section\n", level, best_lod);
  }
  return 0;
}

// The island over many seeds: its extent along 64 rays from the origin (the farthest land and the
// nearest sea) and its land share within 40 km.
int Stats(std::uint64_t first, int seeds, int step) {
  for (int k = 0; k < seeds; ++k) {
    const worldgen::TerrainGenerator gen(first + static_cast<std::uint64_t>(k));
    double near = 1e9, far = 0;
    for (int a = 0; a < 64; ++a) {
      const double angle = a * (2.0 * 3.14159265358979323846 / 64);
      double first_sea = -1, last_land = 0;
      for (int r = 0; r <= kDiscReach; r += step) {
        const auto x = static_cast<std::int32_t>(std::lround(r * std::cos(angle)));
        const auto z = static_cast<std::int32_t>(std::lround(r * std::sin(angle)));
        if (gen.CoastAt(x, z) > 0.0f) {
          last_land = r;
        } else if (first_sea < 0) {
          first_sea = r;
        }
      }
      near = std::min(near, first_sea < 0 ? kDiscReach : first_sea);
      far = std::max(far, last_land);
    }
    long land = 0, total = 0;
    for (int z = -kDiscReach; z < kDiscReach; z += step * 4)
      for (int x = -kDiscReach; x < kDiscReach; x += step * 4) {
        ++total;
        land += gen.CoastAt(x, z) > 0.0f;
      }
    const auto spawn = gen.SpawnPoint();
    std::printf(
        "seed %3llu: sea from %5.1f km, land to %5.1f km, land %.1f%% within %d km; spawn %.1f %.1f "
        "%.1f\n",
        static_cast<unsigned long long>(first) + static_cast<unsigned long long>(k), near / 1000,
        far / 1000, 100.0 * land / total, kDiscReach / 1000, spawn[0], spawn[1], spawn[2]);
  }
  return 0;
}

// Local map image: see the header comment.
int WriteMap(const worldgen::TerrainGenerator& gen, const std::string& path, int cx, int cz, int n,
             int m_per_px) {
  std::vector<std::uint8_t> rgb(static_cast<std::size_t>(n) * n * 3, 0);
  std::vector<worldgen::Column> cols(static_cast<std::size_t>(n + 2) * (n + 2));
  const auto col_at = [&](int px, int py) -> worldgen::Column& {
    return cols[static_cast<std::size_t>(py + 1) * (n + 2) + px + 1];
  };
  for (int py = -1; py <= n; ++py)
    for (int px = -1; px <= n; ++px) {
      col_at(px, py) = gen.ColumnAt(cx + (px - n / 2) * m_per_px, cz + (py - n / 2) * m_per_px);
    }
  long water = 0, total = 0;
  for (int py = 0; py < n; ++py)
    for (int px = 0; px < n; ++px) {
      const worldgen::Column& c = col_at(px, py);
      const float h = c.height;
      // Lit from the north-west: the slope towards it, per pixel.
      const float dx = col_at(px + 1, py).height - col_at(px - 1, py).height;
      const float dz = col_at(px, py + 1).height - col_at(px, py - 1).height;
      const float shade = std::clamp(
          0.75f + 0.5f * (-(dx + dz) / (2.0f * static_cast<float>(m_per_px)) * 3.0f), 0.35f, 1.15f);
      float r, g, b;
      const bool wet = c.height < static_cast<float>(core::kSeaLevel);
      ++total;
      if (wet) {
        ++water;
        const float depth = std::clamp(-c.height / 20.0f, 0.0f, 1.0f);
        r = 80 - 50 * depth, g = 190 - 90 * depth, b = 220 - 60 * depth;
      } else {
        float k[3];
        GroundColour(h, k);
        r = k[0] * shade, g = k[1] * shade, b = k[2] * shade;
      }
      std::uint8_t* o = &rgb[(static_cast<std::size_t>(py) * n + px) * 3];
      o[0] = static_cast<std::uint8_t>(std::clamp(r, 0.0f, 255.0f));
      o[1] = static_cast<std::uint8_t>(std::clamp(g, 0.0f, 255.0f));
      o[2] = static_cast<std::uint8_t>(std::clamp(b, 0.0f, 255.0f));
    }
  if (!tools::WriteImage(path, n, n, rgb)) {
    std::fprintf(stderr, "cannot write %s\n", path.c_str());
    return 1;
  }
  std::printf("%s: %dx%d px at %d m, %.2f%% water\n", path.c_str(), n, n, m_per_px,
              100.0 * static_cast<double>(water) / static_cast<double>(total));
  return 0;
}

// First-person terrain view: see the header comment.
struct Sky {
  static void Colour(float dy, float* rgb) {
    const float t = std::clamp(dy * 2.5f, 0.0f, 1.0f);  // horizon haze → zenith
    rgb[0] = 229.0f + (100.0f - 229.0f) * t;
    rgb[1] = 229.0f + (154.0f - 229.0f) * t;
    rgb[2] = 225.0f + (218.0f - 225.0f) * t;
  }
};

class ViewSampler {
 public:
  explicit ViewSampler(const worldgen::TerrainGenerator& gen) : gen_(gen) {}

  struct Sample {
    float ground, water;  // metres; water 0 = sea level
  };

  // The column grid at spacing 4 << level, bilinear in the ground.
  Sample At(int level, float x, float z) {
    const int sp = 4 << level;
    const float fx = x / static_cast<float>(sp), fz = z / static_cast<float>(sp);
    const auto ix = static_cast<std::int32_t>(std::floor(fx)),
               iz = static_cast<std::int32_t>(std::floor(fz));
    const float tx = fx - static_cast<float>(ix), tz = fz - static_cast<float>(iz);
    const worldgen::Column& a = Cell(level, ix, iz);
    const worldgen::Column& b = Cell(level, ix + 1, iz);
    const worldgen::Column& c = Cell(level, ix, iz + 1);
    const worldgen::Column& d = Cell(level, ix + 1, iz + 1);
    const float ground = (a.height * (1 - tx) + b.height * tx) * (1 - tz) +
                         (c.height * (1 - tx) + d.height * tx) * tz;
    return {ground, static_cast<float>(core::kSeaLevel)};
  }

 private:
  const worldgen::Column& Cell(int level, std::int32_t ix, std::int32_t iz) {
    const std::uint64_t key = (static_cast<std::uint64_t>(level) << 56) ^
                              (static_cast<std::uint64_t>(static_cast<std::uint32_t>(ix)) << 28) ^
                              static_cast<std::uint64_t>(static_cast<std::uint32_t>(iz));
    auto it = cells_.find(key);
    if (it == cells_.end()) {
      const std::int64_t sp = 4 << level;
      const auto x = static_cast<std::int32_t>(std::int64_t{ix} * sp);
      const auto z = static_cast<std::int32_t>(std::int64_t{iz} * sp);
      it = cells_.emplace(key, gen_.ColumnAt(x, z)).first;
    }
    return it->second;
  }
  const worldgen::TerrainGenerator& gen_;
  std::unordered_map<std::uint64_t, worldgen::Column> cells_;
};

int WriteView(const worldgen::TerrainGenerator& gen, const std::string& path, double cx, double cy,
              double cz, double yaw_deg, double pitch_deg, int w, int h, double range,
              double fov_deg) {
  constexpr double kDeg = 3.14159265358979323846 / 180.0;
  const double yaw = yaw_deg * kDeg, pitch = pitch_deg * kDeg;
  const double fwd[3] = {std::sin(yaw) * std::cos(pitch), std::sin(pitch),
                         std::cos(yaw) * std::cos(pitch)};
  const double right[3] = {std::cos(yaw), 0.0, -std::sin(yaw)};
  const double up[3] = {-std::sin(yaw) * std::sin(pitch), std::cos(pitch),
                        -std::cos(yaw) * std::sin(pitch)};
  const double half = std::tan(fov_deg * kDeg / 2.0);
  const double eye[3] = {cx, cy + 1.6, cz};
  std::vector<std::uint8_t> rgb(static_cast<std::size_t>(w) * h * 3);
  const float sun[3] = {-0.5f, 0.75f, -0.43f};

  const auto row_block = [&](int y0, int y1) {
    ViewSampler cols(gen);
    for (int py = y0; py < y1; ++py)
      for (int px = 0; px < w; ++px) {
        const double sx = ((px + 0.5) / w * 2.0 - 1.0) * half;
        const double sy = (1.0 - (py + 0.5) / h * 2.0) * half * h / w;
        double d[3];
        double len = 0;
        for (int k = 0; k < 3; ++k) {
          d[k] = fwd[k] + right[k] * sx + up[k] * sy;
          len += d[k] * d[k];
        }
        len = std::sqrt(len);
        for (double& v : d) v /= len;
        float col[3];
        Sky::Colour(static_cast<float>(d[1]), col);
        // March: the step and the sampling grid grow with distance.
        double t = 0.5, prev_t = 0.0;
        bool hit = false;
        ViewSampler::Sample s{};
        int level = 0;
        for (int i = 0; i < 4000 && t < range; ++i) {
          level = std::clamp(static_cast<int>(std::floor(std::log2(std::max(t, 1.0) / 96.0))) + 0,
                             0, 7);
          const double x = eye[0] + d[0] * t, y = eye[1] + d[1] * t, z = eye[2] + d[2] * t;
          s = cols.At(level, static_cast<float>(x), static_cast<float>(z));
          const float top = std::max(s.ground, s.water);
          if (y < top) {
            // Refine the hit between the last two steps.
            double lo = prev_t, hi = t;
            for (int k = 0; k < 6; ++k) {
              const double m = (lo + hi) / 2;
              const auto sm = cols.At(level, static_cast<float>(eye[0] + d[0] * m),
                                      static_cast<float>(eye[2] + d[2] * m));
              (eye[1] + d[1] * m < std::max(sm.ground, sm.water) ? hi : lo) = m;
            }
            t = hi;
            s = cols.At(level, static_cast<float>(eye[0] + d[0] * t),
                        static_cast<float>(eye[2] + d[2] * t));
            hit = true;
            break;
          }
          prev_t = t;
          const double clearance = y - top;
          t += std::max(static_cast<double>(2 << level) * 0.5, clearance * 0.35);
          if (d[1] > 0.0 && y > 7000.0) break;  // above the world, looking up
        }
        if (hit) {
          const int sp = 4 << level;
          const float x = static_cast<float>(eye[0] + d[0] * t),
                      z = static_cast<float>(eye[2] + d[2] * t);
          const float gx = 2.0f *
                           (cols.At(level, x + sp, z).ground - cols.At(level, x - sp, z).ground) /
                           (2.0f * sp);
          const float gz = 2.0f *
                           (cols.At(level, x, z + sp).ground - cols.At(level, x, z - sp).ground) /
                           (2.0f * sp);
          const float inv = 1.0f / std::sqrt(gx * gx + 1.0f + gz * gz);
          const float nx = -gx * inv, ny = inv, nz = -gz * inv;
          float lit = std::clamp(nx * sun[0] + ny * sun[1] + nz * sun[2], 0.0f, 1.0f);
          float base[3];
          if (s.water > s.ground) {
            const float depth = std::clamp((s.water - s.ground) / 25.0f, 0.0f, 1.0f);
            base[0] = 90 - 55 * depth, base[1] = 195 - 100 * depth, base[2] = 225 - 60 * depth;
            lit = 0.85f + 0.15f * ny;  // water: nearly flat
          } else {
            float k[3];
            GroundColour(s.ground, k);
            const float rock = std::clamp((gx * gx + gz * gz - 0.5f) * 1.2f, 0.0f, 1.0f);  // steep
            for (int c = 0; c < 3; ++c) {
              const float stone = c == 0 ? 168.0f : c == 1 ? 150.0f : 142.0f;
              base[c] = k[c] * (1 - rock) + stone * rock;
            }
          }
          // Warm light, cool shadow.
          const float shade = 0.45f + 0.75f * lit;
          const float cool = 1.0f - lit;
          float surface[3] = {base[0] * shade * (1.0f - 0.10f * cool), base[1] * shade,
                              base[2] * shade * (1.0f + 0.18f * cool)};
          // Haze: distance fades to the horizon colour.
          const float haze = 1.0f - std::exp(-static_cast<float>(t) / 16000.0f);
          float horizon[3];
          Sky::Colour(0.0f, horizon);
          for (int c = 0; c < 3; ++c) col[c] = surface[c] * (1 - haze) + horizon[c] * haze;
        }
        std::uint8_t* o = &rgb[(static_cast<std::size_t>(py) * w + px) * 3];
        for (int c = 0; c < 3; ++c)
          o[c] = static_cast<std::uint8_t>(std::clamp(col[c], 0.0f, 255.0f));
      }
  };
  const auto t0 = std::chrono::steady_clock::now();
  const int threads = static_cast<int>(std::max(1u, std::thread::hardware_concurrency()));
  std::vector<std::thread> pool;
  for (int k = 0; k < threads; ++k)
    pool.emplace_back(row_block, h * k / threads, h * (k + 1) / threads);
  for (auto& t : pool) t.join();
  const double ms =
      std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  if (!tools::WriteImage(path, w, h, rgb)) {
    std::fprintf(stderr, "cannot write %s\n", path.c_str());
    return 1;
  }
  std::printf("%s: %dx%d, %.0f ms on %d threads\n", path.c_str(), w, h, ms, threads);
  return 0;
}

int main(int argc, char** argv) {
  const std::uint64_t seed = argc > 1 ? std::strtoull(argv[1], nullptr, 10) : 0;
  if (argc > 2 && std::string_view(argv[2]) == "bench") return Bench(seed);
  if (argc > 2 && std::string_view(argv[2]) == "stats") {
    return Stats(seed, argc > 3 ? std::atoi(argv[3]) : 16,
                 argc > 4 ? std::max(1, std::atoi(argv[4])) : 250);
  }
  if (argc > 8 && std::string_view(argv[2]) == "view") {
    const worldgen::TerrainGenerator gen(seed);
    return WriteView(
        gen, argv[3], std::atof(argv[4]), std::atof(argv[5]), std::atof(argv[6]),
        std::atof(argv[7]), std::atof(argv[8]), argc > 9 ? std::max(16, std::atoi(argv[9])) : 640,
        argc > 10 ? std::max(16, std::atoi(argv[10])) : 360,
        argc > 11 ? std::atof(argv[11]) : 40000.0, argc > 12 ? std::atof(argv[12]) : 70.0);
  }
  if (argc > 5 && std::string_view(argv[2]) == "map") {
    const worldgen::TerrainGenerator gen(seed);
    return WriteMap(gen, argv[3], std::atoi(argv[4]), std::atoi(argv[5]),
                    argc > 6 ? std::max(16, std::atoi(argv[6])) : 512,
                    argc > 7 ? std::max(1, std::atoi(argv[7])) : 8);
  }
  if (argc > 3 && std::string_view(argv[2]) == "disc") {
    const worldgen::TerrainGenerator gen(seed);
    return WriteDisc(gen, argv[3], argc > 4 ? std::max(1, std::atoi(argv[4])) : 100,
                     argc > 5 ? argv[5] : "coast");
  }
  const int cx = argc > 2 ? std::atoi(argv[2]) : 0;
  const int cz = argc > 3 ? std::atoi(argv[3]) : 0;
  const int step = argc > 4 ? std::atoi(argv[4]) : 32;
  const worldgen::TerrainGenerator gen(seed);

  if (argc > 5 && std::string_view(argv[5]) == "slice") {
    // Generated chunks, so the section includes the surface materials.
    core::VoxelWorld world(core::GeneratorFor(core::kGeneratorTerrain, seed));
    const auto base = gen.ColumnAt(cx, cz).height;
    const int top = static_cast<int>(base) + 40;
    for (int y = top; y > top - 70; --y) {
      std::printf("%4d ", y);
      for (int x = cx - 60; x < cx + 60; ++x) {
        const auto m = world.GetVoxel(x, y, cz);
        char ch = '?';
        switch (m) {
          case core::Materials::kAir:
            ch = ' ';
            break;
          case core::Materials::kStone:
            ch = '#';
            break;
          case core::Materials::kDirt:
            ch = ':';
            break;
          case core::Materials::kGrass:
            ch = '"';
            break;
          case core::Materials::kWater:
            ch = '~';
            break;
          case core::Materials::kSand:
            ch = '.';
            break;
          default:
            ch = 'o';
            break;
        }
        std::putchar(ch);
      }
      std::putchar('\n');
    }
    return 0;
  }

  // Map: height bands (~ sea, . shore, then 0–9 for each 16 m above it), blank beyond the rim.
  float lo = 1e9f, hi = -1e9f;
  for (int row = -24; row < 24; ++row) {
    for (int col = -48; col < 48; ++col) {
      const auto c = gen.ColumnAt(cx + col * step, cz + row * step);
      if (c.outside) {
        std::putchar(' ');
        continue;
      }
      lo = std::min(lo, c.height);
      hi = std::max(hi, c.height);
      const char ch = c.height < 0.0f   ? '~'
                      : c.height < 2.0f ? '.'
                                        : static_cast<char>('0' + std::min(9, static_cast<int>(
                                                                                 c.height / 16)));
      std::putchar(ch);
    }
    std::putchar('\n');
  }
  std::printf("~ sea  . shore  0-9 height / 16 m  (blank: beyond the rim)\n");
  std::printf("height %.1f .. %.1f\n", lo, hi);

  // Timings: a 5×5 column of chunks from 4 below to 6 above the centre's surface.
  core::Chunk chunk;
  int chunks = 0;
  std::map<core::MaterialId, long> materials;
  std::chrono::steady_clock::duration elapsed{};
  const int surface = worldgen::FloorDiv(static_cast<int>(gen.ColumnAt(cx, cz).height), 32);
  for (int x = -2; x <= 2; ++x)
    for (int z = -2; z <= 2; ++z)
      for (int y = surface - 4; y <= surface + 6; ++y) {
        core::Chunk c;
        const auto start = std::chrono::steady_clock::now();
        gen.Generate({worldgen::FloorDiv(cx, 32) + x, y, worldgen::FloorDiv(cz, 32) + z}, c);
        elapsed += std::chrono::steady_clock::now() - start;
        for (const auto m : c.voxels()) ++materials[m];
        ++chunks;
      }
  const double ms = std::chrono::duration<double, std::milli>(elapsed).count();
  std::printf("%d chunks, %.3f ms/chunk\n", chunks, ms / chunks);
  for (const auto& [m, n] : materials)
    std::printf("  %-10s %ld\n", core::GetMaterial(m).name.data(), n);
  const auto s = gen.SpawnPoint();
  std::printf("spawn %.1f %.1f %.1f\n", s[0], s[1], s[2]);
  return 0;
}
