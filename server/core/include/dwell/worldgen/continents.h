#pragma once

#include <cstdint>

#include "dwell/worldgen/noise.h"

// Continents from Voronoi plates (WORLD_GENERATION.md §2, ADR 0017): the disc's land/ocean layout.
// Two nested jittered-grid Voronoi layers — continent cells (~2,560 km) and plates (~256 km) —
// decide which plates are land; a signed distance to the coast (metres, positive on land) follows
// from the plates, with fractal coast detail added and a clamp that keeps an ocean gap between any
// two continents, around island plates, and along the rim. Every value is a pure function of (world
// seed, world coordinates), integer-hashed (noise.h), so chunks, point queries and the level of
// detail agree and native and WASM builds are bit-identical (ADR 0010, amended by 0017: the
// bisector distances use a correctly rounded sqrt).
namespace dwell::worldgen {

namespace continents {

// Layout scales (m). Cell (0, 0) of each grid is centred on the origin.
inline constexpr std::int64_t kContinentCell = 2'560'000;
inline constexpr std::int64_t kPlateCell = 256'000;
inline constexpr std::int64_t kIslandCell = 40'000;
// Spacing (m) of the lattice the coarse fields live on, shared by chunks, point queries and the
// level of detail; a power of two and a multiple of the terrain's 4 m lattice.
inline constexpr std::int32_t kMacroStep = 256;
// Open ocean (m) guaranteed between any two continents, and between continents and islands.
inline constexpr float kOceanGap = 300'000.0f;
// Ocean ring (m) along the rim of the disc.
inline constexpr float kRimOcean = 512'000.0f;
// Plates are land only when their site lies this far (m) inside their continent cell's border.
inline constexpr float kPlateInset = 90'000.0f;
// Edge plates (within twice the inset of the border) turn to sea with this chance: bays and gulfs.
inline constexpr float kBayChance = 0.08f;
// Ocean-cell plates become island plates with this chance, when their site lies at least
// kIslandBorder (m) from the nearest continent cell's border.
inline constexpr float kIslandPlateChance = 0.05f;
inline constexpr float kIslandBorder = 350'000.0f;
// An island's centre lies at least this far (m) beyond its radius from every land cell's border:
// continents spill past their cells' borders by the plates' reach and the coast detail.
inline constexpr float kIslandClearance = 750'000.0f;
// Within an island plate: islands per island cell, and their radii (m).
inline constexpr float kIslandChance = 0.45f;
inline constexpr std::int32_t kIslandMinRadius = 8'000;
inline constexpr std::int32_t kIslandRadiusRange = 16'000;
// Offshore of an island the seabed falls this many times faster than the island rises.
inline constexpr float kIslandSlope = 10.0f;
// Continents per world (land cells, including the origin's): a hash of the seed picks one.
inline constexpr int kMinContinents = 12;
inline constexpr int kMaxContinents = 13;

// Ids: a continent is its cell, packed as (i + 8) * 16 + (j + 8) in 0..255.
inline constexpr std::int32_t kNoContinent = -1;  // open sea
inline constexpr std::int32_t kIslandId = -2;     // land on an island plate

// Coast detail: octaves of Perlin noise from this wavelength (m), halving each octave, with
// amplitudes falling by kCoastPersistence per octave.
inline constexpr std::int32_t kCoastWavelength = 400'000;
inline constexpr int kCoastOctaves = 9;
inline constexpr float kCoastAmplitude = 100'000.0f;
inline constexpr float kCoastPersistence = 0.65f;
// Islands take only the octaves from this one on (their wavelength 25 km).
inline constexpr int kIslandFirstOctave = 4;

// The domain warp's amplitude (m) and wavelength (m): 2 octaves of a vector noise field. The
// separation clamp widens by its Lipschitz bound (kWarpLipschitz, verified by a test).
inline constexpr float kWarpAmplitude = 76'800.0f;
inline constexpr std::int32_t kWarpWavelength = 1'000'000;
inline constexpr float kWarpLipschitz = 0.3f;
// Slack (m) the separation clamp keeps for the lattice's interpolation and the terrain's fine
// coast octaves, which are added after it.
inline constexpr float kClampSlack = 3'000.0f;

// Coast distances saturate at this magnitude (m): the fields look no further than the plates
// around a point.
inline constexpr float kCoastCap = 256'000.0f;

}  // namespace continents

// One continent's character (hashed from its id): used by the generator (elevation, shelf width)
// and exported for the climate and mountain stages of Phase 11.
struct ContinentRecord {
  float elevation = 0.0f;         // m, added to the interior's base height
  float mountainousness = 0.5f;   // 0..1
  float temperature_bias = 0.0f;  // °C
  float humidity_bias = 0.0f;     // −1..1
  int wind = 0;                   // prevailing wind direction, 0..7 (eight compass directions)
  float shelf = 100'000.0f;       // m, width of the continental shelf
};

// The coarse fields at one point.
struct MacroCorner {
  float coast = -continents::kCoastCap;      // signed distance to the coast (m): > 0 land, < 0 sea
  float plate_edge = continents::kCoastCap;  // m, to the nearest internal plate edge (land)
  float convergence = 0.0f;  // −1..1 at that edge: > 0 convergent (mountain belts), < 0 divergent
  float elevation = 0.0f;    // m: the continent's record (at sea, the nearest continent's)
  float shelf = 100'000.0f;  // m: the continent's shelf width
  std::int32_t continent = continents::kNoContinent;  // id of the continent here, or kIslandId
  bool island_plates_near = false;  // an island plate lies within the plates searched (At only)
};

class ContinentLayout {
 public:
  explicit ContinentLayout(std::uint64_t world_seed);

  // The fields exactly at (x, z), without the lattice. With `kept` ≥ 0 (level of detail), only the
  // first `kept` coast octaves.
  // `edges` false skips the internal plate edges (plate_edge, convergence stay at their defaults):
  // the level of detail has no use for them.
  MacroCorner At(std::int64_t x, std::int64_t z, int kept = -1, bool edges = true) const;
  // The lattice corner (mx, mz) = At(mx · kMacroStep, mz · kMacroStep), cached per thread.
  MacroCorner Corner(std::int32_t mx, std::int32_t mz) const;
  // The fields at (x, z) interpolated bilinearly between the four surrounding lattice corners
  // (the continent from the nearest corner): what chunks and point queries use.
  MacroCorner Sample(std::int64_t x, std::int64_t z) const;

  // The domain warp at (x, z): the offset (m, whole metres) both Voronoi lookups see.
  struct Offset {
    std::int32_t x, z;
  };
  Offset Warp(std::int64_t x, std::int64_t z) const;

  // Land continents in this world, the origin's included.
  int ContinentCount() const { return count_; }
  bool CellIsLand(int i, int j) const;
  ContinentRecord Record(std::int32_t id) const;
  static std::int32_t IdOf(int i, int j) { return (i + 8) * 16 + (j + 8); }

 private:
  struct Site {
    std::int64_t x, z;
  };
  struct Plate {
    std::int64_t x, z;
    std::int32_t i, j;  // plate grid cell
    std::int32_t cell;  // IdOf the continent cell the site lies in
    std::uint8_t kind;  // 0 sea, 1 land, 2 island
  };
  struct Seeds {
    std::uint32_t warp[2], coast, site, plate, rank, record, bay, blob, edge;
  };

  Site CellSite(std::int32_t i, std::int32_t j) const;
  Plate ComputePlate(std::int32_t i, std::int32_t j) const;
  Plate PlateOf(std::int32_t i, std::int32_t j) const;  // cached per thread
  float CoastNoise(std::int64_t x, std::int64_t z, int first, int kept) const;

  // The continent cells' sites for cells −kSiteRange..kSiteRange (the disc and the cells beside
  // it); cells beyond are hashed on demand.
  static constexpr int kSiteRange = 6;
  Site SiteOf(std::int32_t i, std::int32_t j) const {
    return i >= -kSiteRange && i <= kSiteRange && j >= -kSiteRange && j <= kSiteRange
               ? sites_[j + kSiteRange][i + kSiteRange]
               : CellSite(i, j);
  }

  std::uint64_t world_seed_;
  Seeds s_{};
  bool land_[9][9] = {};  // continent cells −4..4
  Site sites_[2 * kSiteRange + 1][2 * kSiteRange + 1] = {};
  int count_ = 0;
};

}  // namespace dwell::worldgen
