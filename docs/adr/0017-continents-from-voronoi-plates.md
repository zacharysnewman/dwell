# 0017. Continents from Voronoi plates: a plate layout, a separation clamp, a macro lattice, and `sqrt`

- Status: Accepted
- Date: 2026-10-06
- Resolves: Phase 10 of the implementation plan (continents from Voronoi plates)
- Amends: [0010](0010-worldgen-noise-numerics.md) (strict IEEE numerics: a correctly rounded `sqrt` is
  allowed)
- Builds on: [0011](0011-planet-scale-world.md) (the 8,192 km disc), [0012](0012-lod-octree.md)
  (the level of detail)

## Context

Land and sea were decided by noise: a 1.4 km fBm shifted by a 262 km one. That gives lakes, inland
seas and coasts anywhere, no landmass a player could call a continent, and nothing guaranteeing open
ocean between landmasses. Phase 10 replaces it with a layout of **6–14 distinct continents with at
least `OCEAN_GAP` of open ocean between any two**, natural coastlines at every scale, island chains,
an ocean ring at the rim and a per-continent character, and turns continentalness into a signed
distance to the coast ([`WORLD_GENERATION.md`](../WORLD_GENERATION.md) §2). The layout must obey the
generator's rules: a pure function of (seed, world coordinates), no neighbour reads, bit-identical
natively and in WASM, identical in chunks, point queries and the level of detail.

## Decision

1. **Two jittered-grid Voronoi layers** (`worldgen/continents.h`). *Continent cells* of 2,560 km and
   *plates* of 256 km, each grid centred on the origin (cell (0, 0) is the origin's), each cell
   holding one site hashed into the middle 60 % of the cell (jitter 0.2–0.8: with that, the nearest
   site always lies in the 3 × 3 cells around a point, which a wider jitter does not guarantee). The
   origin's continent cell has its site on the origin, so the spawn is deep inland. Both lookups run
   on a **domain-warped** point: a 2-octave vector noise, 76.8 km at a 1,000 km wavelength, whole
   metres. Hashes of grid cells mix the coordinates in sequence (`CellHash`): `Hash2`, an xor of
   two products, repeats for small coordinates (49 cells around the origin give 39 distinct values),
   which would repeat sites between neighbours.
2. **Which cells are land** is a function of the seed alone, decided in the layout's constructor: the
   origin's cell, then the cells hashing lowest among those whose sites keep clear of the rim ocean,
   until a seed-hashed count of 12–13 land cells is reached. A count rather than a per-cell chance:
   independent chances give 6–15 land cells at random, and the land share (a quarter to a third of
   the disc, which 12–13 continents of this size need) would fail for some seeds. Over 64 seeds the
   land share is 25.9–33.4 % (mean 29.8 %).
3. **Plates.** A plate belongs to the continent cell nearest its site (in warped space). It is *land*
   when that cell is land and its site lies `PLATE_INSET` (90 km) inside the cell's border (a plate
   within twice that of the border turns to sea with 8 % chance: bays and gulfs), *sea* otherwise,
   and an *island plate* (5 %) in an ocean cell when its site is 350 km from every land cell's
   border. Islands are blobs of 8–24 km radius on a 40 km grid inside island plates, each kept only
   if its centre is 750 km (plus its radius) from every land cell's border; offshore of an island
   the seabed falls ten times faster than the island rises, so an island's own seabed is bounded
   within the blobs searched.
4. **The signed coast distance** `s` (m, positive on land) is `(D_sea − D_land) / 2`, where D are the
   distances to the nearest sea (or island) and land plate sites in the 5 × 5 plates around the
   point, each capped at 512 km (every site within 2.2 plates is among them, so the cap is exact).
   It is continuous everywhere, zero exactly on the bisectors between land and sea plates, and
   saturates at ±256 km. The bisector distance the design suggests is discontinuous where one land
   plate gives way to another, since each plate has its own sea neighbours; this form has no such
   jump (and underestimates the true distance by a factor ≥ cos θ along an edge: cosmetic).
5. **Coast detail** is 9 octaves of Perlin noise from 400 km down to 1.6 km (amplitude 100 km
   falling ×0.65 per octave, so ≈ 13 km at 25 km and 3 km at 3 km), added to `s`, faded out beyond
   50–150 km from the coast (far from it the detail has nothing to say, and must not make land at
   sea). The 1.4 km octaves of the old continentalness stay on the 4 m lattice (900 m of coast per
   unit). A coastline measured with a 1 km ruler is 1.9–2.3 × the length measured with a 16 km one.
6. **The separation clamp (the guarantee).** A point of continent `L` (its cell, or at sea the cell
   of the nearest land plate; land that spills over a border into an ocean cell keeps its
   continent) is clamped to `s ≤ g − M`, where `g` is the signed distance, in warped space, from
   the point to the bisector with each other land cell `B` (the minimum), and
   `M = (1 + L_w) · OCEAN_GAP / 2 + slack` with `L_w` the warp's Lipschitz bound (0.3; measured
   0.23, a test checks it) and a 3 km slack for the lattice's interpolation and the fine octaves
   added after it. Two points of different continents lie on opposite sides of the bisector of
   their continents, each at least `M` from it in warped space, so at least `OCEAN_GAP` apart
   in the world. (A first version clamped by the point's own cell only; land spilling into an ocean
   cell between two continents then came within the gap of the other — found by the separation
   test.) Where the clamp sets the coast, the same coast detail *recedes* it (0–60 km inland, never
   outward), so those coasts are as ragged as the rest. The rim ring clamps `s ≤ (R − r) − RIM_OCEAN`
   (512 km), using the root.
7. **Per-continent record** (`ContinentRecord`, hashed from the id): interior elevation (−8 to
   30 m), mountainousness, temperature and humidity bias, prevailing wind (8 directions), shelf width
   (80–160 km). Phase 10 uses the elevation and shelf; the rest, and the internal plate-edge distance
   and its hashed convergence (−1..1: convergent edges for mountain belts, divergent for rifts), are
   exported in `Column` for Phase 11.
8. **Terrain from the coast distance.** On land continentalness rises as `s / (s + 40 km)`
   (saturating; 0.7–1.0 of it modulated by the local noise); at sea it falls to −1 over 200 km. The
   base height is, at sea, the shelf (to −150 m at its edge, one shelf width out), the continental
   slope (a quarter of a width further) and the abyss (−1,500 ± 300 m), and on land the coastal
   lowland rising to the continent's interior plus its elevation. The land weight (hills, ranges)
   is zero at the coast and full 4 km inland, so mountains do not reach out to sea.
9. **The macro lattice.** The layout is evaluated exactly at the corners of a 256 m lattice
   (`ContinentLayout::Corner`, cached per thread in a direct-mapped table: caches of pure functions,
   which change speed and never values), bilinear in between (`Sample`); the chunk path's 4 m
   lattice corners and the point queries read it, so chunks and point queries agree exactly (256 is a
   multiple of 4). Plates are cached the same way.
10. **Level of detail.** Cells narrower than 256 m read the same lattice. Cells of 256 m and wider
    evaluate the layout exactly at their centres with the coast octaves they can resolve
    (`LodLayout`): at anchors every 4 columns, with the columns between interpolated only where the
    four anchors lie inside one continent's interior (> 80 km from the coast) or in the deep sea
    (beyond the foot of any slope) with no island plate near, and every column exact otherwise.
    The 700 m erosion and 360 m ridged fields the mountains use are thresholded, so a cell too wide
    for any of their octaves reads their world means (erosion −0.078, ridges 0.61) rather than
    zero: with zero, every interior was drawn flat from afar (found by the column-surface test,
    which put coarse LOD columns 26 m too low).
11. **`sqrt`.** The bisector distances need `|b − a|`. IEEE 754 requires `sqrt` to be correctly
    rounded and both x86-64 (`sqrtsd`) and WebAssembly (`f64.sqrt`) implement it exactly, so ADR
    0010's ban on library calls is amended to allow **`std::sqrt` and nothing else from `<cmath>`**
    in `server/core/src/worldgen`, guarded by the golden hashes as before. The layout works in
    `double` (the squared distances are exact in `int64` and in `double`: all below 2⁵³; a whole
    world coordinate is still never converted to float, ADR 0011) and stores `float`s.

## Consequences

- **Generator version 6, a new compatibility line.** The terrain a seed generates changes
  everywhere, and so the saved worlds' meaning: `package.json` goes to 0.4.0 (RELEASES.md §6). The
  chunk and LOD goldens are regenerated with coast, ocean-gap, island, interior and abyss entries;
  the client's mirror of the generator version moves to 6.
- **Cost.** A chunk costs ≈ +5 % (the layout is four cached lattice corners per chunk); LOD
  sections cost the same as before in interiors and open sea, and up to about twice as much at
  coasts and channels, where the coast detail is rougher than any interpolation allows (every
  column is evaluated exactly): a follow-up could cache a section's layout between the bounds query
  and the generation, or evaluate the plates once per section.
- **Looks.** Continents are polygonal at their borders — the guarantee leaves straight, constant-width
  channels between neighbours — and ragged along open coasts. Their sizes are 2.3–7.5 million km²
  (seeds 0–7), all of one order (the grid's cell size). The ocean floor is deep (−1,500 m), where it was
  −42 m: the world's 2,048 m of depth is used, and far-offshore chunks are water over rock to the
  bedrock. All prototype content (§6.1 of ARCHITECTURE.md), tunable from the constants in
  `continents.h` and the tables in `terrain.cpp`.
- **Tests find landmarks by scanning the layout** (`biome_search.h`): the sea is hundreds of
  kilometres from the origin, so the old "a biome near the origin" searches go to a coast.
- Reversal: the layout is one class behind `Column`'s fields; the old noise decision is in git
  (generator version 5).
