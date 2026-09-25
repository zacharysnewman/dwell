// Worldgen noise numerics prototype (open decision #8, ADR 0010): the shipped strict-IEEE float
// gradient noise (worldgen/noise.h) against a 16.16 fixed-point version of the same algorithm.
// Prints a checksum of each over the same sample points (compare native vs WASM output) and the
// time per 3D evaluation.
//   dwell_noise_numerics [samples]
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>

#include "dwell/worldgen/noise.h"

namespace {

using dwell::worldgen::Hash3;

// --- 16.16 fixed point ---
using Fix = std::int32_t;
constexpr int kFrac = 16;
constexpr Fix kOne = 1 << kFrac;
constexpr Fix Mul(Fix a, Fix b) {
  return static_cast<Fix>((static_cast<std::int64_t>(a) * b) >> kFrac);
}
constexpr Fix FadeFix(Fix t) {
  // 6t⁵ − 15t⁴ + 10t³ = t³(t(6t − 15) + 10)
  return Mul(Mul(Mul(t, t), t), Mul(t, 6 * t - 15 * kOne) + 10 * kOne);
}
constexpr Fix LerpFix(Fix a, Fix b, Fix t) { return a + Mul(b - a, t); }
constexpr Fix Grad3Fix(std::uint32_t h, Fix x, Fix y, Fix z) {
  switch (h & 15u) {
    case 0:
      return x + y;
    case 1:
      return -x + y;
    case 2:
      return x - y;
    case 3:
      return -x - y;
    case 4:
      return x + z;
    case 5:
      return -x + z;
    case 6:
      return x - z;
    case 7:
      return -x - z;
    case 8:
      return y + z;
    case 9:
      return -y + z;
    case 10:
      return y - z;
    case 11:
      return -y - z;
    case 12:
      return x + y;
    case 13:
      return -x + y;
    case 14:
      return -y + z;
    default:
      return -y - z;
  }
}
// Coordinates in 16.16; the lattice cell is the integer part (arithmetic shift floors).
Fix Perlin3Fix(std::uint32_t seed, Fix x, Fix y, Fix z) {
  const std::int32_t x0 = x >> kFrac, y0 = y >> kFrac, z0 = z >> kFrac;
  const Fix fx = x & (kOne - 1), fy = y & (kOne - 1), fz = z & (kOne - 1);
  const Fix u = FadeFix(fx), v = FadeFix(fy), w = FadeFix(fz);
  const auto g = [&](int i, int j, int k) {
    return Grad3Fix(Hash3(seed, x0 + i, y0 + j, z0 + k), fx - i * kOne, fy - j * kOne,
                    fz - k * kOne);
  };
  const Fix x00 = LerpFix(g(0, 0, 0), g(1, 0, 0), u);
  const Fix x10 = LerpFix(g(0, 1, 0), g(1, 1, 0), u);
  const Fix x01 = LerpFix(g(0, 0, 1), g(1, 0, 1), u);
  const Fix x11 = LerpFix(g(0, 1, 1), g(1, 1, 1), u);
  return LerpFix(LerpFix(x00, x10, v), LerpFix(x01, x11, v), w);
}

std::uint64_t Fnv(std::uint64_t h, std::uint32_t v) {
  for (int b = 0; b < 4; ++b) {
    h ^= (v >> (8 * b)) & 0xFFu;
    h *= 0x100000001b3ull;
  }
  return h;
}

}  // namespace

int main(int argc, char** argv) {
  const int n = argc > 1 ? std::atoi(argv[1]) : 2'000'000;
  // Sample points spread over ±65 536 m (the world bound) at voxel-lattice frequencies.
  const auto coord = [](int i, int axis) {
    const std::uint32_t h = dwell::worldgen::Hash2(99u + static_cast<std::uint32_t>(axis), i, axis);
    return static_cast<std::int32_t>(h % 131072u) - 65536;
  };

  std::uint64_t float_sum = 0xcbf29ce484222325ull, fixed_sum = float_sum;
  float max_diff = 0;
  auto t0 = std::chrono::steady_clock::now();
  for (int i = 0; i < n; ++i) {
    const float v = dwell::worldgen::Perlin3(7, static_cast<float>(coord(i, 0)) * (1.0f / 28.0f),
                                             static_cast<float>(coord(i, 1)) * (1.0f / 20.0f),
                                             static_cast<float>(coord(i, 2)) * (1.0f / 28.0f));
    std::uint32_t bits;
    __builtin_memcpy(&bits, &v, 4);
    float_sum = Fnv(float_sum, bits);
  }
  const double float_ns =
      std::chrono::duration<double, std::nano>(std::chrono::steady_clock::now() - t0).count() / n;

  t0 = std::chrono::steady_clock::now();
  for (int i = 0; i < n; ++i) {
    // Voxel coordinate × frequency in 16.16: (c << 16) / period, exact integer division.
    const Fix x = static_cast<Fix>((static_cast<std::int64_t>(coord(i, 0)) << kFrac) / 28);
    const Fix y = static_cast<Fix>((static_cast<std::int64_t>(coord(i, 1)) << kFrac) / 20);
    const Fix z = static_cast<Fix>((static_cast<std::int64_t>(coord(i, 2)) << kFrac) / 28);
    const Fix v = Perlin3Fix(7, x, y, z);
    fixed_sum = Fnv(fixed_sum, static_cast<std::uint32_t>(v));
  }
  const double fixed_ns =
      std::chrono::duration<double, std::nano>(std::chrono::steady_clock::now() - t0).count() / n;

  // Agreement between the two (outside the timed loops).
  for (int i = 0; i < 100000; ++i) {
    const Fix v =
        Perlin3Fix(7, static_cast<Fix>((static_cast<std::int64_t>(coord(i, 0)) << kFrac) / 28),
                   static_cast<Fix>((static_cast<std::int64_t>(coord(i, 1)) << kFrac) / 20),
                   static_cast<Fix>((static_cast<std::int64_t>(coord(i, 2)) << kFrac) / 28));
    const float f = dwell::worldgen::Perlin3(7, static_cast<float>(coord(i, 0)) * (1.0f / 28.0f),
                                             static_cast<float>(coord(i, 1)) * (1.0f / 20.0f),
                                             static_cast<float>(coord(i, 2)) * (1.0f / 28.0f));
    const float d = static_cast<float>(v) / kOne - f;
    max_diff = d > max_diff ? d : -d > max_diff ? -d : max_diff;
  }

  std::printf("samples %d\n", n);
  std::printf("float  checksum %016llx  %.1f ns/eval\n", static_cast<unsigned long long>(float_sum),
              float_ns);
  std::printf("fixed  checksum %016llx  %.1f ns/eval\n", static_cast<unsigned long long>(fixed_sum),
              fixed_ns);
  std::printf("max |fixed - float| %.5f\n", max_diff);
  return 0;
}
