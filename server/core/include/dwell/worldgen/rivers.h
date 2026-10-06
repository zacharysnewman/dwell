#pragma once

#include <cstdint>

#include "dwell/worldgen/noise.h"

// Rivers, lakes and water above sea level (WORLD_GENERATION.md §3.2–3.3, Phase 11a): drainage-
// consistent terrain after Epic Terrain's technique, re-implemented. Three tiers of rivers are the
// zero contours of three single-octave noises (no simulation, no neighbour reads), so a channel is
// a long, meandering line that always runs along a valley floor; mountains stand away from the
// channels (the distance factor D), and the channels are carved into the valley floor V. River and
// lake water is static, at a terraced surface below the valley floor: where two terraces meet in a
// channel the upper pool ends in a vertical face over the lower one, a waterfall.
//
// Everything here is a pure function of (seed, world coordinates) under ADR 0010: +, −, × and
// comparisons only (the sqrt-free kind), so chunks, point queries and the level of detail agree,
// natively and in WASM. All the numbers are in this header's tables (WORLD_GENERATION.md §5), so
// the real world style can retune them without touching the pipeline.
namespace dwell::worldgen {

namespace rivers {

// A channel's depth falls from its full value at |noise| = core to zero at `bank` as (1 − s)⁴, with
// s the smoothstep of |noise| between them: the water, which stands a metre or more below the
// banks, keeps to the bed and the lower banks, and the carve's tail is a gentle slope.
inline constexpr int kProfileSharpness = 4;

// One tier of rivers: the zero contour of a single-octave Perlin noise. A column is in the channel
// where |noise| < core and on its banks up to `bank` (the bed's width is ~core × wavelength / 2,
// the banks' ~bank × wavelength / 2); the relief that mountains add reaches its full height at
// |noise| = full (the valley's width). A channel exists where the undamped relief (`potential`, m
// above the valley floor) is below `fade_hi`, fading out from `fade_lo`: streams become dry gullies
// high up, and rivers start at springs.
struct Tier {
  std::int32_t wavelength;  // m: the noise's lattice spacing
  float core, bank, full;   // |noise| thresholds
  float depth;              // m: channel depth below the valley floor
  float fade_lo, fade_hi;   // m of potential relief
  // Level of detail: a cell this wide or wider drops the tier — its valley and its channel (0:
  // never). The valley (the distance factor) is resolved by cells well under the tier's wavelength.
  std::int64_t drop_cell;
  // Level of detail: a cell this wide or wider shows no channel — no carve, no water — though it
  // keeps the valley. The channel stays at its own width, never widened to a cell: a cell is water
  // only where full detail is mostly water (what a downsample keeps), so a stream's thread of water
  // is not drawn as a cell-wide river from afar (0: never).
  std::int64_t channel_cell;
  // Springs: the channel exists where the spring noise (kSpringWavelength) is above `spring_hi` and
  // fades out down to `spring_lo`, so small rivers begin and end (as dry gullies) instead of
  // looping everywhere. The great river has none (−2, −1).
  float spring_lo, spring_hi;
};
inline constexpr Tier kGreat{200'000, 0.0006f, 0.008f, 0.06f, 14.0f, 3000.0f,
                             5500.0f, 0,       0,      -2.0f, -1.0f};
inline constexpr Tier kRiver{6'000,  0.006f, 0.10f, 0.15f,  6.0f,  150.0f,
                             800.0f, 2048,   0,     -0.30f, -0.05f};
inline constexpr Tier kStream{1'500,   0.004f, 0.06f, 0.10f,  2.5f, 600.0f,
                              1800.0f, 1024,   32,    -0.10f, 0.15f};
// The distance factor D of an unresolved tier (a dropped one): the mean of its ramp, which is about
// 0.78 for the river tier and 0.85 for the stream tier (measured over the world); one value.
inline constexpr float kDroppedFactor = 0.81f;

// Meanders: each tier samples its noise at a position displaced by a gentle vector noise (the two
// small tiers share one).
inline constexpr std::int32_t kMeanderWavelength = 700;
inline constexpr float kMeanderAmplitude = 150.0f;  // m
inline constexpr std::int32_t kGreatMeanderWavelength = 30'000;
inline constexpr float kGreatMeanderAmplitude = 2'500.0f;  // m
// The spring noise of the small tiers.
inline constexpr std::int32_t kSpringWavelength = 12'000;

// River surface = the terraced valley floor, this far below it (m): banks stand at least this high.
inline constexpr float kFreeboard = 1.0f;
// Terraces: boundaries every kTerraceStep m, each displaced by a hashed −1, 0 or +1 m, so a step
// between pools is 2 to 6 m. Below the first boundary the surface is sea level.
inline constexpr float kTerraceStep = 4.0f;

// The valley floor V (m) above the coast's lowland: a saturating rise with the distance inland s
// (m), (s / (s + kValleyHalf))² × kValleyRise — flat near the coast, so rivers reach the sea at sea
// level — plus uplift belts along convergent plate edges, plus a share of the mountain relief.
inline constexpr float kValleyRise = 250.0f;
inline constexpr float kValleyHalf = 150'000.0f;
inline constexpr float kCoastPlain = 2.0f;       // m: V at the shore
inline constexpr float kPlainStart = 5'000.0f;   // m inland: the coast's own lowland begins
inline constexpr float kPlainEnd = 30'000.0f;    // m inland: and is complete
inline constexpr float kBeltReach = 60'000.0f;   // m from a convergent plate edge
inline constexpr float kBeltCore = 10'000.0f;    // m: full strength within
inline constexpr float kBeltValley = 120.0f;     // m raised in V by a belt
inline constexpr float kBeltRelief = 900.0f;     // m of relief a belt adds (× ridged field)
inline constexpr float kMountainValley = 0.12f;  // share of range relief that lifts V

// Lakes: a jittered grid of cells with at most one lake each, on land at least kLakeInland from the
// coast. The lake's surface is the terraced valley floor at its centre, kLakeFreeboard m lower;
// the bed is a bowl; a low berm rims the shore so that no lake spills over a lower shore.
inline constexpr std::int32_t kLakeCell = 12'288;  // m
inline constexpr float kLakeChance = 0.35f;
inline constexpr float kLakeInland = 20'000.0f;
inline constexpr float kLakeMinRadius = 700.0f, kLakeRadiusRange = 1300.0f;  // m (≤ 2,000)
inline constexpr float kLakeMinDepth = 4.0f, kLakeDepthRange = 8.0f;         // m
inline constexpr float kLakeFreeboard = 3.0f;
inline constexpr float kShoreNoise = 0.45f;  // the shoreline's wobble, in squared radii
inline constexpr std::int32_t kShoreWavelength = 600;
inline constexpr float kBermHeight = 1.5f;                                  // m above the surface
inline constexpr float kBermFrom = 1.0f, kBermPeak = 1.1f, kBermTo = 1.4f;  // squared radii
// Rivers stop at a lake's shore (squared radii): a channel's full strength beyond kLakeRiverFull.
inline constexpr float kLakeRiverZero = 0.7f, kLakeRiverFull = 1.1f;
inline constexpr float kNoLake = 4.0f;    // squared radius with no lake near
inline constexpr float kNoLevel = -1e9f;  // lake level of a corner with no lake

// Caves stay this far (m) below a river, lake or shallow sea floor (added to the 3 m the cave fade
// starts below the surface).
inline constexpr float kCaveClearance = 12.0f;
inline constexpr float kShallowSea = 40.0f;  // m: sea floors shallower than this are shallow

// Level of detail: cells this wide (m) or wider (levels 12 and up) no longer show lakes or the
// great river's windings, which are smaller than a cell.
inline constexpr std::int64_t kLakeSkipCell = 4096;

// The surface of the terrace containing the valley-floor height v (m, an integer-valued float):
// sea level (0) below the first boundary.
float TerraceSurface(std::uint32_t seed, float v);

// The raw fields at one point, before the terrain turns them into heights.
struct Corner {
  float rg = 1.0f, r1 = 1.0f, r2 = 1.0f;  // signed noise of each tier (1: dropped for the cell)
  // Level of detail: 1 where a tier's channel is shown, 0 where the cell is too wide for it (its
  // valley, which the distance factor reads from the noise itself, stays).
  float cg = 1.0f, c1 = 1.0f, c2 = 1.0f;
  float spring = 1.0f;          // the small tiers' spring noise
  float lake_q = kNoLake;       // squared radius of the nearest lake (≥ kNoLake: none)
  float lake_level = kNoLevel;  // m: the surface of that lake
  float lake_depth = 0.0f;      // m: its deepest point below the surface
};

// The seeds of the river noises.
struct Seeds {
  std::uint32_t great, river, stream, meander_x, meander_z, terrace, lake, shore, spring,
      great_meander_x, great_meander_z;
  // Each tier's lattice is shifted by a hashed offset (m, within a wavelength): Perlin noise is
  // exactly zero at its lattice points, so unshifted tiers would all cross at the origin — the
  // spawn — in every world.
  std::int64_t offset_x[3], offset_z[3];
};
Seeds MakeSeeds(std::uint64_t world_seed);

// Supplies a lake's surface from its centre (the terrain's valley floor, which this stage does not
// know): the terraced valley floor there minus the freeboard, or kNoLevel if no lake can stand
// there (on or near the sea).
class LevelOracle {
 public:
  virtual float LakeLevel(std::int64_t x, std::int64_t z) const = 0;

 protected:
  ~LevelOracle() = default;
};

// The tiers' noise and the nearest lake at (x, z). `cell` (m, 0: exact) is the level of detail's
// cell width: a channel narrower than a cell is not shown, and a tier too narrow for the cell drops
// out.
Corner Sample(const Seeds& s, std::int64_t x, std::int64_t z, std::int64_t cell,
              const LevelOracle& oracle);

}  // namespace rivers

}  // namespace dwell::worldgen
