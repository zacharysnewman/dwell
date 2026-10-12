#pragma once

#include <cstdint>

// The interim land layout (Phase 15, ARCHITECTURE.md §6.3): one island at the origin, replacing the
// continents of Voronoi plates (ADR 0017). Its coast is a circle of kRadius m whose radius varies by
// smooth noise, seen through a domain warp, with fractal coast detail added. Everything is a pure
// function of (world seed, world coordinates), integer-hashed (noise.h), so chunks, point queries
// and the level of detail agree and native and WASM builds are bit-identical (ADR 0010; the distance
// from the origin uses the correctly rounded square root, as ADR 0017's bisectors did).
//
// The parameters are code constants for now: Phase 17 moves the layout into the terrain definition
// (data, ADR 0026), so this is a placeholder, not a system to extend.
namespace dwell::worldgen {

namespace island {

// The island's mean radius (m) and how far (m) the radius noise moves its coast in or out, at that
// noise's wavelength (m).
inline constexpr float kRadius = 25'000.0f;
inline constexpr float kRadiusVariation = 5'000.0f;
inline constexpr std::int32_t kRadiusWavelength = 20'000;
// The domain warp: amplitude (m) and wavelength (m), 2 octaves.
inline constexpr float kWarpAmplitude = 3'000.0f;
inline constexpr std::int32_t kWarpWavelength = 6'000;
// Coast detail: octaves of fractal noise from this wavelength (m), and their amplitude (m).
inline constexpr std::int32_t kCoastWavelength = 4'000;
inline constexpr int kCoastOctaves = 5;
inline constexpr float kCoastAmplitude = 1'600.0f;

}  // namespace island

class IslandLayout {
 public:
  explicit IslandLayout(std::uint64_t world_seed);

  // The signed distance to the coast (m) at (x, z): > 0 on the island, < 0 at sea. Approximately a
  // distance (the warp and the noise bend it a little). With `kept` ≥ 0 (level of detail, §6.6),
  // only the coast detail's first `kept` octaves; the warp and the radius noise are always whole.
  float CoastAt(std::int64_t x, std::int64_t z, int kept = -1) const;

 private:
  std::uint32_t radius_seed_, warp_x_seed_, warp_z_seed_, coast_seed_;
  // The noise lattices are shifted by a hashed offset (m): gradient noise is zero at its lattice
  // points, which the origin and round coordinates would otherwise sit on in every world.
  std::int64_t offset_x_ = 0, offset_z_ = 0;
};

}  // namespace dwell::worldgen
