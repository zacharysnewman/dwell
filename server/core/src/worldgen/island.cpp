#include "dwell/worldgen/island.h"

#include <cmath>

#include "dwell/worldgen/noise.h"

namespace dwell::worldgen {

using namespace island;

IslandLayout::IslandLayout(std::uint64_t world_seed)
    : radius_seed_(SeedWord(world_seed, 0x1514A001u)),
      warp_x_seed_(SeedWord(world_seed, 0x1514A002u)),
      warp_z_seed_(SeedWord(world_seed, 0x1514A003u)),
      coast_seed_(SeedWord(world_seed, 0x1514A004u)),
      offset_x_(NoiseOffset(SeedWord(world_seed, 0x1514A005u))),
      offset_z_(NoiseOffset(SeedWord(world_seed, 0x1514A006u))) {}

float IslandLayout::CoastAt(std::int64_t x, std::int64_t z, int kept) const {
  // The noise reads shifted coordinates; the distance is from the origin itself.
  const std::int64_t nx = x + offset_x_, nz = z + offset_z_;
  // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
  const float wx = 2.0f * kWarpAmplitude * Fbm2(warp_x_seed_, nx, nz, kWarpWavelength, 2);
  const float wz = 2.0f * kWarpAmplitude * Fbm2(warp_z_seed_, nx, nz, kWarpWavelength, 2);
  const double px = static_cast<double>(x) + static_cast<double>(wx);
  const double pz = static_cast<double>(z) + static_cast<double>(wz);
  // Correctly rounded in IEEE double on every target (no fast-math): deterministic.
  const auto distance = static_cast<float>(std::sqrt(px * px + pz * pz));
  const float radius =
      kRadius + 2.0f * kRadiusVariation * Fbm2(radius_seed_, nx, nz, kRadiusWavelength, 2);
  const float detail =
      2.0f * kCoastAmplitude * Fbm2(coast_seed_, nx, nz, kCoastWavelength, kCoastOctaves, kept);
  return radius - distance + detail;
}

}  // namespace dwell::worldgen
