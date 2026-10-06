#include "dwell/worldgen/continents.h"

#include <algorithm>
#include <cmath>  // std::sqrt only: ADR 0017 amends ADR 0010 to allow the correctly rounded root
#include <limits>
#include <vector>

#include "dwell/core/voxel.h"

namespace dwell::worldgen {

using namespace continents;

namespace {

enum PlateKind : std::uint8_t { kSea = 0, kLand = 1, kIsland = 2 };

constexpr std::int64_t FloorDiv64(std::int64_t a, std::int64_t b) {
  std::int64_t q = a / b;
  if (a % b != 0 && ((a < 0) != (b < 0))) --q;
  return q;
}

constexpr std::int64_t Sq(std::int64_t v) { return v * v; }

// Distance from a point to the bisector of sites a and b, for a point nearer a: from the squared
// distances to a and b and between the sites. The numerator is exact in int64 and in double (all
// below 2^53); the root and the quotient are correctly rounded.
double LineDistance(std::int64_t to_a2, std::int64_t to_b2, std::int64_t ab2) {
  return static_cast<double>(to_b2 - to_a2) / (2.0 * std::sqrt(static_cast<double>(ab2)));
}

constexpr double kInf = std::numeric_limits<double>::infinity();

// Hash of a grid cell. Hash2's xor of two products repeats for small coordinates (49 cells around
// the origin give 39 distinct values), which would repeat sites and records between neighbouring
// cells; mixing the coordinates in sequence does not.
constexpr std::uint32_t CellHash(std::uint32_t seed, std::int32_t i, std::int32_t j) {
  const std::uint32_t h = Mix32(seed + static_cast<std::uint32_t>(i) * 0x9E3779B1u);
  return Mix32(h ^ (static_cast<std::uint32_t>(j) * 0x85EBCA77u + 0x27D4EB2Fu));
}

// A jittered grid site in the middle 60 % of cell (i, j); cell (0, 0) is centred on the origin.
// With jitter in [0.2, 0.8] the nearest site is always within the 3×3 cells around a point.
struct GridPoint {
  std::int64_t x, z;
};
GridPoint GridSite(std::uint32_t seed, std::int32_t i, std::int32_t j, std::int64_t cell) {
  const std::uint32_t h = CellHash(seed, i, j);
  const std::uint32_t h2 = Mix32(h ^ 0x9E3779B9u);
  const std::int64_t lo = cell / 5, span = cell * 3 / 5;
  const std::int64_t ox = std::int64_t{i} * cell - cell / 2, oz = std::int64_t{j} * cell - cell / 2;
  return {ox + lo + static_cast<std::int64_t>(h % static_cast<std::uint32_t>(span)),
          oz + lo + static_cast<std::int64_t>(h2 % static_cast<std::uint32_t>(span))};
}

// Per-thread caches of pure functions of (world seed, grid cell): they change speed, never values.
struct PlateEntry {
  std::uint64_t seed = 0;
  std::int32_t i = 0, j = 0;
  bool valid = false;
  std::int64_t x = 0, z = 0;
  std::int32_t cell = 0;
  std::uint8_t kind = 0;
};
constexpr std::size_t kPlateCache = 1024;
thread_local PlateEntry g_plates[kPlateCache];

struct CornerEntry {
  std::uint64_t seed = 0;
  std::int32_t mx = 0, mz = 0;
  bool valid = false;
  MacroCorner value;
};
constexpr std::size_t kCornerCache = 2048;
thread_local CornerEntry g_corners[kCornerCache];

std::size_t CacheSlot(std::uint64_t seed, std::int32_t a, std::int32_t b, std::size_t size) {
  return CellHash(static_cast<std::uint32_t>(seed ^ (seed >> 32)), a, b) & (size - 1);
}

}  // namespace

ContinentLayout::ContinentLayout(std::uint64_t world_seed) : world_seed_(world_seed) {
  s_.warp[0] = SeedWord(world_seed, 101);
  s_.warp[1] = SeedWord(world_seed, 102);
  s_.coast = SeedWord(world_seed, 103);
  s_.site = SeedWord(world_seed, 104);
  s_.plate = SeedWord(world_seed, 105);
  s_.rank = SeedWord(world_seed, 106);
  s_.record = SeedWord(world_seed, 107);
  s_.bay = SeedWord(world_seed, 108);
  s_.blob = SeedWord(world_seed, 109);
  s_.edge = SeedWord(world_seed, 110);

  for (int j = -kSiteRange; j <= kSiteRange; ++j)
    for (int i = -kSiteRange; i <= kSiteRange; ++i)
      sites_[j + kSiteRange][i + kSiteRange] = CellSite(i, j);

  // Which continent cells are land: the origin's, and then those hashing lowest among the cells
  // whose sites keep clear of the rim ocean, until the seed's continent count is reached.
  struct Candidate {
    std::uint32_t rank;
    int i, j;
  };
  std::vector<Candidate> candidates;
  const double reach = static_cast<double>(core::kWorldRadius) - kRimOcean - 400'000.0;
  for (int j = -4; j <= 4; ++j)
    for (int i = -4; i <= 4; ++i) {
      if (i == 0 && j == 0) continue;
      const Site c = CellSite(i, j);
      const double d2 = static_cast<double>(Sq(c.x) + Sq(c.z));
      if (d2 > reach * reach) continue;
      candidates.push_back({CellHash(s_.rank, i, j), i, j});
    }
  std::sort(candidates.begin(), candidates.end(), [](const Candidate& a, const Candidate& b) {
    return a.rank != b.rank ? a.rank < b.rank : (a.j != b.j ? a.j < b.j : a.i < b.i);
  });
  const int span = kMaxContinents - kMinContinents + 1;
  const int want = kMinContinents + static_cast<int>(Mix32(s_.rank + 77u) % span);
  land_[4][4] = true;
  count_ = 1;
  for (const Candidate& c : candidates) {
    if (count_ >= want) break;
    land_[c.j + 4][c.i + 4] = true;
    ++count_;
  }
}

bool ContinentLayout::CellIsLand(int i, int j) const {
  return i >= -4 && i <= 4 && j >= -4 && j <= 4 && land_[j + 4][i + 4];
}

ContinentLayout::Site ContinentLayout::CellSite(std::int32_t i, std::int32_t j) const {
  if (i == 0 && j == 0) return {0, 0};  // the spawn continent is centred on the origin
  const GridPoint p = GridSite(s_.site, i, j, kContinentCell);
  return {p.x, p.z};
}

ContinentRecord ContinentLayout::Record(std::int32_t id) const {
  ContinentRecord r;
  if (id < 0) return r;
  const int i = id / 16 - 8, j = id % 16 - 8;
  const std::uint32_t h = CellHash(s_.record, i, j);
  const auto u = [&](std::uint32_t k) { return Unit(Mix32(h + k * 0x9E3779B9u)); };
  r.elevation = -8.0f + 38.0f * u(1);
  r.mountainousness = u(2);
  r.temperature_bias = -10.0f + 20.0f * u(3);
  r.humidity_bias = -1.0f + 2.0f * u(4);
  r.wind = static_cast<int>(Mix32(h + 5u * 0x9E3779B9u) & 7u);
  r.shelf = 80'000.0f + 80'000.0f * u(6);
  return r;
}

ContinentLayout::Plate ContinentLayout::ComputePlate(std::int32_t i, std::int32_t j) const {
  const GridPoint s = GridSite(s_.plate, i, j, kPlateCell);
  // The continent cell whose site is nearest the plate's site, and the distance from the site to
  // that cell's borders: to any neighbour's, and to a land neighbour's.
  const std::int32_t ci =
      static_cast<std::int32_t>(FloorDiv64(s.x + kContinentCell / 2, kContinentCell));
  const std::int32_t cj =
      static_cast<std::int32_t>(FloorDiv64(s.z + kContinentCell / 2, kContinentCell));
  Site sites[9];
  std::int64_t d2[9];
  int nearest = 0;
  for (int n = 0; n < 9; ++n) {
    sites[n] = SiteOf(ci - 1 + n % 3, cj - 1 + n / 3);
    d2[n] = Sq(s.x - sites[n].x) + Sq(s.z - sites[n].z);
    if (d2[n] < d2[nearest]) nearest = n;
  }
  const std::int32_t ai = ci - 1 + nearest % 3, aj = cj - 1 + nearest / 3;
  double g_any = kInf, g_land = kInf;
  for (int n = 0; n < 9; ++n) {
    if (n == nearest) continue;
    const double g = LineDistance(
        d2[nearest], d2[n], Sq(sites[n].x - sites[nearest].x) + Sq(sites[n].z - sites[nearest].z));
    g_any = std::min(g_any, g);
    if (CellIsLand(ci - 1 + n % 3, cj - 1 + n / 3)) g_land = std::min(g_land, g);
  }
  Plate p{s.x, s.z, i, j, IdOf(ai, aj), kSea};
  const float roll = Unit(CellHash(s_.bay, i, j));
  if (CellIsLand(ai, aj)) {
    if (g_any >= kPlateInset) {
      p.kind = kLand;
      if (g_any < 2.0 * kPlateInset && roll < kBayChance) p.kind = kSea;  // bays and gulfs
    }
  } else if (roll < kIslandPlateChance && g_land >= kIslandBorder) {
    p.kind = kIsland;
  }
  return p;
}

ContinentLayout::Plate ContinentLayout::PlateOf(std::int32_t i, std::int32_t j) const {
  PlateEntry& e = g_plates[CacheSlot(world_seed_, i, j, kPlateCache)];
  if (!e.valid || e.seed != world_seed_ || e.i != i || e.j != j) {
    const Plate p = ComputePlate(i, j);
    e = {world_seed_, i, j, true, p.x, p.z, p.cell, p.kind};
  }
  return {e.x, e.z, i, j, e.cell, e.kind};
}

float ContinentLayout::CoastNoise(std::int64_t x, std::int64_t z, int first, int kept) const {
  const int last = kept < 0 ? kCoastOctaves : std::min(kept, kCoastOctaves);
  float sum = 0.0f, amplitude = kCoastAmplitude;
  for (int o = 0; o < last; ++o) {
    if (o >= first) {
      const std::uint32_t seed = Mix32(s_.coast + static_cast<std::uint32_t>(o) * 0x9E3779B9u);
      sum += Perlin2(seed, Lattice(x, kCoastWavelength, o), Lattice(z, kCoastWavelength, o)) *
             amplitude;
    }
    amplitude *= kCoastPersistence;
  }
  return sum;
}

ContinentLayout::Offset ContinentLayout::Warp(std::int64_t px, std::int64_t pz) const {
  // A vector noise field, whole metres, shared by both Voronoi lookups.
  const float wx = Fbm2(s_.warp[0], px, pz, kWarpWavelength, 2) * kWarpAmplitude;
  const float wz = Fbm2(s_.warp[1], px, pz, kWarpWavelength, 2) * kWarpAmplitude;
  return {FloorToInt(wx), FloorToInt(wz)};
}

MacroCorner ContinentLayout::At(std::int64_t px, std::int64_t pz, int kept, bool edges) const {
  const Offset w = Warp(px, pz);
  const Site q{px + w.x, pz + w.z};

  // Level 1: the continent cell nearest the warped point (the 5 × 5 cells around it are kept, for
  // the separation clamp below).
  const std::int32_t ci =
      static_cast<std::int32_t>(FloorDiv64(q.x + kContinentCell / 2, kContinentCell));
  const std::int32_t cj =
      static_cast<std::int32_t>(FloorDiv64(q.z + kContinentCell / 2, kContinentCell));
  Site sites[25];
  std::int64_t d2[25];
  int nearest = 0;
  for (int n = 0; n < 25; ++n) {
    sites[n] = SiteOf(ci - 2 + n % 5, cj - 2 + n / 5);
    d2[n] = Sq(q.x - sites[n].x) + Sq(q.z - sites[n].z);
    if (d2[n] < d2[nearest]) nearest = n;
  }
  const std::int32_t ai = ci - 2 + nearest % 5, aj = cj - 2 + nearest / 5;
  const bool own_land = CellIsLand(ai, aj);

  // Level 2: the 5 × 5 plates around the point (every site within 2.2 plate cells is among them).
  const std::int32_t pi = static_cast<std::int32_t>(FloorDiv64(q.x + kPlateCell / 2, kPlateCell));
  const std::int32_t pj = static_cast<std::int32_t>(FloorDiv64(q.z + kPlateCell / 2, kPlateCell));
  Plate plates[25];
  std::int64_t pd2[25];
  int near_land = -1, near_sea = -1;
  bool any_island = false;
  for (int n = 0; n < 25; ++n) {
    plates[n] = PlateOf(pi - 2 + n % 5, pj - 2 + n / 5);
    pd2[n] = Sq(q.x - plates[n].x) + Sq(q.z - plates[n].z);
    if (plates[n].kind == kLand) {
      if (near_land < 0 || pd2[n] < pd2[near_land]) near_land = n;
    } else {
      any_island |= plates[n].kind == kIsland;
      if (near_sea < 0 || pd2[n] < pd2[near_sea]) near_sea = n;
    }
  }
  const double cap = 2.0 * kCoastCap;
  const double d_land =
      near_land < 0 ? cap : std::min(cap, std::sqrt(static_cast<double>(pd2[near_land])));
  const double d_sea =
      near_sea < 0 ? cap : std::min(cap, std::sqrt(static_cast<double>(pd2[near_sea])));

  MacroCorner out;
  // Which continent the point belongs to: its own cell if that is land, else (at sea, or land
  // spilling over a border into an ocean cell) the cell of the nearest land plate.
  std::int32_t label = kNoContinent;
  if (own_land) {
    label = IdOf(ai, aj);
  } else if (near_land >= 0) {
    label = plates[near_land].cell;
  }

  // Signed distance to the coast: half the difference of the distances to the nearest sea and
  // land sites (zero on the bisector between them), then the fractal coast, which fades out far
  // from the coast (deep inland or at sea the coast detail has nothing to say).
  const float s0 = static_cast<float>((d_sea - d_land) * 0.5);
  const float reach = s0 < 0.0f ? -s0 : s0;
  const float noise = reach >= 150'000.0f ? 0.0f
                                          : CoastNoise(px, pz, 0, kept) *
                                                (1.0f - SmoothStep(50'000.0f, 150'000.0f, reach));
  float s = s0 + noise;

  // The separation clamp: land of one continent stays kOceanGap (widened by the warp's stretch)
  // from every other land cell's side of the border. For a point of continent L the distance
  // (signed) to the bisector with each other land cell B bounds its land; two points of different
  // continents then lie on opposite sides of their continents' bisector, each at least half the
  // gap from it.
  if (label >= 0) {
    const std::int32_t li = label / 16 - 8, lj = label % 16 - 8;
    const Site ls = SiteOf(li, lj);
    const std::int64_t dl2 = Sq(q.x - ls.x) + Sq(q.z - ls.z);
    double border_land = kInf;
    for (int n = 0; n < 25; ++n) {
      const std::int32_t i = ci - 2 + n % 5, j = cj - 2 + n / 5;
      if ((i == li && j == lj) || !CellIsLand(i, j)) continue;
      border_land = std::min(
          border_land, LineDistance(dl2, d2[n], Sq(sites[n].x - ls.x) + Sq(sites[n].z - ls.z)));
    }
    const float margin = (1.0f + kWarpLipschitz) * (kOceanGap * 0.5f) + kClampSlack;
    // Where the clamp sets the coast, the coast detail recedes it (never advances it), so these
    // coasts are as ragged as the rest: the same noise, 0 to 60 km inland of the clamp.
    const float recede = std::max(0.0f, std::min(60'000.0f, 30'000.0f - noise));
    s = std::min(s, static_cast<float>(border_land) - margin - recede);
  }

  // Islands: blobs on a fine grid, kept where the plate nearest their centre is an island plate.
  bool island = false;
  if (any_island) {
    const std::int32_t bi =
        static_cast<std::int32_t>(FloorDiv64(q.x + kIslandCell / 2, kIslandCell));
    const std::int32_t bj =
        static_cast<std::int32_t>(FloorDiv64(q.z + kIslandCell / 2, kIslandCell));
    double best = -kInf;
    for (int dj = -1; dj <= 1; ++dj)
      for (int di = -1; di <= 1; ++di) {
        const std::int32_t i = bi + di, j = bj + dj;
        const std::uint32_t h = CellHash(s_.blob, i, j);
        if (Unit(h) >= kIslandChance) continue;
        const GridPoint c = GridSite(s_.blob, i, j, kIslandCell);
        int owner = 0;
        std::int64_t best_d2 = std::numeric_limits<std::int64_t>::max();
        for (int n = 0; n < 25; ++n) {
          const std::int64_t d = Sq(c.x - plates[n].x) + Sq(c.z - plates[n].z);
          if (d < best_d2) {
            best_d2 = d;
            owner = n;
          }
        }
        if (plates[owner].kind != kIsland) continue;
        const std::int64_t radius =
            kIslandMinRadius + static_cast<std::int64_t>(Mix32(h + 3u) % kIslandRadiusRange);
        // An island keeps clear of land cells: its centre is kIslandClearance (and its radius)
        // from every land cell's border, whatever the continents' coasts do beyond them.
        {
          const std::int32_t ni =
              static_cast<std::int32_t>(FloorDiv64(c.x + kContinentCell / 2, kContinentCell));
          const std::int32_t nj =
              static_cast<std::int32_t>(FloorDiv64(c.z + kContinentCell / 2, kContinentCell));
          Site cs[9];
          std::int64_t cd2[9];
          int cn = 0;
          for (int n = 0; n < 9; ++n) {
            cs[n] = SiteOf(ni - 1 + n % 3, nj - 1 + n / 3);
            cd2[n] = Sq(c.x - cs[n].x) + Sq(c.z - cs[n].z);
            if (cd2[n] < cd2[cn]) cn = n;
          }
          if (CellIsLand(ni - 1 + cn % 3, nj - 1 + cn / 3)) continue;
          double clear = kInf;
          for (int n = 0; n < 9; ++n) {
            if (n == cn || !CellIsLand(ni - 1 + n % 3, nj - 1 + n / 3)) continue;
            clear = std::min(clear, LineDistance(cd2[cn], cd2[n],
                                                 Sq(cs[n].x - cs[cn].x) + Sq(cs[n].z - cs[cn].z)));
          }
          if (clear < static_cast<double>(kIslandClearance) + static_cast<double>(radius)) continue;
        }
        // Land inside the radius; offshore the bed falls steeply (kIslandSlope), reaching the
        // abyss within the lattice window the neighbouring blobs are searched in.
        double blob = static_cast<double>(radius) -
                      std::sqrt(static_cast<double>(Sq(q.x - c.x) + Sq(q.z - c.z)));
        if (blob < 0.0) blob *= kIslandSlope;
        best = std::max(best, blob);
      }
    if (best > -kInf) {
      const float blob = static_cast<float>(best) + CoastNoise(px, pz, kIslandFirstOctave, kept);
      if (blob > s) {
        s = blob;
        island = true;  // the island's own seabed, and its land where s > 0
      }
    }
  }

  // The rim ocean: no land within kRimOcean of the edge of the disc.
  const double r = std::sqrt(static_cast<double>(Sq(px) + Sq(pz)));
  s = std::min(s, static_cast<float>(static_cast<double>(core::kWorldRadius) - r - kRimOcean));
  s = std::max(-kCoastCap, std::min(kCoastCap, s));

  // Land belongs to a continent or an island (at sea the label names the nearest land's, for the
  // shelf and elevation); land with neither is shaved to sea.
  if (s > 0.0f && !island && label == kNoContinent) s = -2000.0f;
  out.continent = island ? kIslandId : label;
  const ContinentRecord rec = Record(island ? kNoContinent : label);
  out.coast = s;
  out.island_plates_near = any_island;
  out.elevation = rec.elevation;
  out.shelf = rec.shelf;

  // Internal plate edges (land): the distance to the nearest edge between two land plates of the
  // same continent, and that edge's hashed convergence.
  if (edges && s > 0.0f && near_land >= 0 && out.continent >= 0) {
    const Plate& a = plates[near_land];
    double edge = kInf;
    int other = -1;
    for (int n = 0; n < 25; ++n) {
      if (n == near_land || plates[n].kind != kLand || plates[n].cell != a.cell) continue;
      const double g =
          LineDistance(pd2[near_land], pd2[n], Sq(plates[n].x - a.x) + Sq(plates[n].z - a.z));
      if (g < edge) {
        edge = g;
        other = n;
      }
    }
    if (other >= 0) {
      const Plate& b = plates[other];
      const bool a_first = a.j != b.j ? a.j < b.j : a.i < b.i;
      const Plate& lo = a_first ? a : b;
      const Plate& hi = a_first ? b : a;
      const std::uint32_t h =
          Mix32(CellHash(s_.edge, lo.i, lo.j) ^ CellHash(s_.edge + 1u, hi.i, hi.j));
      out.plate_edge = static_cast<float>(std::min<double>(edge, kCoastCap));
      out.convergence = Unit(h) * 2.0f - 1.0f;
    }
  }
  return out;
}

MacroCorner ContinentLayout::Corner(std::int32_t mx, std::int32_t mz) const {
  CornerEntry& e = g_corners[CacheSlot(world_seed_, mx, mz, kCornerCache)];
  if (!e.valid || e.seed != world_seed_ || e.mx != mx || e.mz != mz) {
    e.value = At(std::int64_t{mx} * kMacroStep, std::int64_t{mz} * kMacroStep);
    e.seed = world_seed_;
    e.mx = mx;
    e.mz = mz;
    e.valid = true;
  }
  return e.value;
}

MacroCorner ContinentLayout::Sample(std::int64_t x, std::int64_t z) const {
  const auto mx = static_cast<std::int32_t>(FloorDiv64(x, kMacroStep));
  const auto mz = static_cast<std::int32_t>(FloorDiv64(z, kMacroStep));
  const std::int32_t fx = static_cast<std::int32_t>(x - std::int64_t{mx} * kMacroStep);
  const std::int32_t fz = static_cast<std::int32_t>(z - std::int64_t{mz} * kMacroStep);
  const float tx = static_cast<float>(fx) * (1.0f / kMacroStep);
  const float tz = static_cast<float>(fz) * (1.0f / kMacroStep);
  const MacroCorner c[4] = {Corner(mx, mz), Corner(mx + 1, mz), Corner(mx, mz + 1),
                            Corner(mx + 1, mz + 1)};
  const auto bi = [&](float MacroCorner::*f) {
    return Lerp(Lerp(c[0].*f, c[1].*f, tx), Lerp(c[2].*f, c[3].*f, tx), tz);
  };
  MacroCorner m;
  m.coast = bi(&MacroCorner::coast);
  m.plate_edge = bi(&MacroCorner::plate_edge);
  m.convergence = bi(&MacroCorner::convergence);
  m.elevation = bi(&MacroCorner::elevation);
  m.shelf = bi(&MacroCorner::shelf);
  m.continent = c[(fx >= kMacroStep / 2 ? 1 : 0) + (fz >= kMacroStep / 2 ? 2 : 0)].continent;
  return m;
}

}  // namespace dwell::worldgen
