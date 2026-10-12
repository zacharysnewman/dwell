#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {
namespace {

// Quintic fade 6t⁵ − 15t⁴ + 10t³ (improved Perlin noise).
constexpr float Fade(float t) { return t * t * t * (t * (t * 6.0f - 15.0f) + 10.0f); }

// Eight unnormalised directions: axes and diagonals.
constexpr float Grad2(std::uint32_t h, float x, float z) {
  switch (h & 7u) {
    case 0:
      return x + z;
    case 1:
      return -x + z;
    case 2:
      return x - z;
    case 3:
      return -x - z;
    case 4:
      return x;
    case 5:
      return -x;
    case 6:
      return z;
    default:
      return -z;
  }
}

// Perlin's twelve cube-edge directions, padded to sixteen.
constexpr float Grad3(std::uint32_t h, float x, float y, float z) {
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

// Per-octave seeds: independent streams rather than shifted copies of one lattice.
constexpr std::uint32_t OctaveSeed(std::uint32_t seed, int octave) {
  return Mix32(seed + static_cast<std::uint32_t>(octave) * 0x9E3779B9u);
}

}  // namespace

float Perlin2(std::uint32_t seed, LatticeCoord x, LatticeCoord z) {
  const std::int32_t x0 = x.cell, z0 = z.cell;
  const float fx = x.frac, fz = z.frac;
  const float u = Fade(fx), v = Fade(fz);
  const float n00 = Grad2(Hash2(seed, x0, z0), fx, fz);
  const float n10 = Grad2(Hash2(seed, x0 + 1, z0), fx - 1.0f, fz);
  const float n01 = Grad2(Hash2(seed, x0, z0 + 1), fx, fz - 1.0f);
  const float n11 = Grad2(Hash2(seed, x0 + 1, z0 + 1), fx - 1.0f, fz - 1.0f);
  return Lerp(Lerp(n00, n10, u), Lerp(n01, n11, u), v);
}

float Perlin3(std::uint32_t seed, LatticeCoord x, LatticeCoord y, LatticeCoord z) {
  const std::int32_t x0 = x.cell, y0 = y.cell, z0 = z.cell;
  const float fx = x.frac, fy = y.frac, fz = z.frac;
  const float u = Fade(fx), v = Fade(fy), w = Fade(fz);
  const auto g = [&](std::int32_t i, std::int32_t j, std::int32_t k) {
    return Grad3(Hash3(seed, x0 + i, y0 + j, z0 + k), fx - static_cast<float>(i),
                 fy - static_cast<float>(j), fz - static_cast<float>(k));
  };
  const float x00 = Lerp(g(0, 0, 0), g(1, 0, 0), u);
  const float x10 = Lerp(g(0, 1, 0), g(1, 1, 0), u);
  const float x01 = Lerp(g(0, 0, 1), g(1, 0, 1), u);
  const float x11 = Lerp(g(0, 1, 1), g(1, 1, 1), u);
  return Lerp(Lerp(x00, x10, v), Lerp(x01, x11, v), w);
}

namespace {
LatticeCoord Split(float v) {
  const std::int32_t cell = FloorToInt(v);
  return {cell, v - static_cast<float>(cell)};
}
}  // namespace

float Perlin2(std::uint32_t seed, float x, float z) { return Perlin2(seed, Split(x), Split(z)); }

float Perlin3(std::uint32_t seed, float x, float y, float z) {
  return Perlin3(seed, Split(x), Split(y), Split(z));
}

float Fbm2(std::uint32_t seed, std::int64_t x, std::int64_t z, std::int32_t wavelength, int octaves,
           int kept) {
  float sum = 0.0f, norm = 0.0f, amplitude = 1.0f;
  for (int o = 0; o < octaves; ++o) {
    if (kept < 0 || o < kept) {
      sum += Perlin2(OctaveSeed(seed, o), Lattice(x, wavelength, o), Lattice(z, wavelength, o)) *
             amplitude;
    }
    norm += amplitude;
    amplitude *= 0.5f;
  }
  return sum / norm;
}

float Fbm3(std::uint32_t seed, std::int64_t x, std::int64_t y, std::int64_t z, std::int32_t wx,
           std::int32_t wy, std::int32_t wz, int octaves, int kept) {
  float sum = 0.0f, norm = 0.0f, amplitude = 1.0f;
  for (int o = 0; o < octaves; ++o) {
    if (kept < 0 || o < kept) {
      sum += Perlin3(OctaveSeed(seed, o), Lattice(x, wx, o), Lattice(y, wy, o), Lattice(z, wz, o)) *
             amplitude;
    }
    norm += amplitude;
    amplitude *= 0.5f;
  }
  return sum / norm;
}

float Ridged2(std::uint32_t seed, std::int64_t x, std::int64_t z, std::int32_t wavelength,
              int octaves, int kept) {
  float sum = 0.0f, norm = 0.0f, amplitude = 1.0f, weight = 1.0f;
  if (kept >= 0 && kept < octaves) octaves = kept;
  if (octaves == 0) return 0.0f;
  for (int o = 0; o < octaves; ++o) {
    const float n =
        Perlin2(OctaveSeed(seed, o), Lattice(x, wavelength, o), Lattice(z, wavelength, o));
    float r = 1.0f - (n < 0.0f ? -n : n);
    r = r * r * weight;
    weight = Clamp01(r * 2.0f);
    sum += r * amplitude;
    norm += amplitude;
    amplitude *= 0.5f;
  }
  return sum / norm;
}

}  // namespace dwell::worldgen
