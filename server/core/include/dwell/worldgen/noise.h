#pragma once

#include <cstdint>

// Deterministic noise for world generation (ARCHITECTURE.md §6.3, ADR 0010). Gradients come from
// integer hashing, and evaluation is strict IEEE float: only +, −, × and comparisons, compiled with
// -ffp-contract=off and no fast-math, and no library calls (no sin, exp, pow). Native and WASM
// builds therefore produce bit-identical results, which the worldgen golden test checks.
namespace dwell::worldgen {

// Avalanche mix of a 32-bit value (murmur3 finalizer).
constexpr std::uint32_t Mix32(std::uint32_t h) {
  h ^= h >> 16;
  h *= 0x85EBCA6Bu;
  h ^= h >> 13;
  h *= 0xC2B2AE35u;
  h ^= h >> 16;
  return h;
}

// Hash of an integer lattice point under a seed: coordinates times large odd constants, xor-ed
// together with the seed, then one avalanche mix.
constexpr std::uint32_t Hash2(std::uint32_t seed, std::int32_t x, std::int32_t z) {
  return Mix32(seed ^ (static_cast<std::uint32_t>(x) * 0x27D4EB2Fu) ^
               (static_cast<std::uint32_t>(z) * 0x165667B1u));
}

constexpr std::uint32_t Hash3(std::uint32_t seed, std::int32_t x, std::int32_t y, std::int32_t z) {
  return Mix32(seed ^ (static_cast<std::uint32_t>(x) * 0x27D4EB2Fu) ^
               (static_cast<std::uint32_t>(y) * 0xD3A2646Du) ^
               (static_cast<std::uint32_t>(z) * 0x165667B1u));
}

// Uniform [0, 1) from a hash (24 bits, exact in float).
constexpr float Unit(std::uint32_t h) { return static_cast<float>(h >> 8) * (1.0f / 16777216.0f); }

constexpr std::int32_t FloorToInt(float v) {
  const auto i = static_cast<std::int32_t>(v);
  return v < static_cast<float>(i) ? i - 1 : i;
}

// Integer floor division and modulo (towards −∞).
constexpr std::int32_t FloorDiv(std::int32_t a, std::int32_t b) {
  return a / b - ((a % b != 0) && ((a < 0) != (b < 0)) ? 1 : 0);
}
constexpr std::int32_t FloorMod(std::int32_t a, std::int32_t b) { return a - FloorDiv(a, b) * b; }

constexpr float Lerp(float a, float b, float t) { return a + (b - a) * t; }
constexpr float Clamp01(float v) { return v < 0.0f ? 0.0f : v > 1.0f ? 1.0f : v; }
// Cubic smoothstep of v over [e0, e1] (e0 > e1 allowed: a falling edge).
constexpr float SmoothStep(float e0, float e1, float v) {
  const float t = Clamp01((v - e0) / (e1 - e0));
  return t * t * (3.0f - 2.0f * t);
}

// Gradient (Perlin) noise, roughly in [−1, 1], zero at lattice points.
float Perlin2(std::uint32_t seed, float x, float z);
float Perlin3(std::uint32_t seed, float x, float y, float z);

// Fractal sums, normalised by the total amplitude (so roughly in [−1, 1]). Each octave doubles the
// frequency, halves the amplitude, and uses its own seed.
float Fbm2(std::uint32_t seed, float x, float z, int octaves);
float Fbm3(std::uint32_t seed, float x, float y, float z, int octaves);
// Ridged fractal: (1 − |noise|)² per octave, weighted by the previous octave; in [0, 1]. Sharp
// crests for mountain ranges.
float Ridged2(std::uint32_t seed, float x, float z, int octaves);

}  // namespace dwell::worldgen
