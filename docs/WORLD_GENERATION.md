# Dwell — World Generation Plans: Look, Continents, Terrain, Sky Islands

> **Status: [planned]; §1 (Phase 7), §2 (Phase 10) and §3.2–3.3 (Phase 11a) are [built].** This is the design reference for implementation Phases 7 and 10–12
> ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)). It describes what to build and why, in
> enough detail that no outside material is needed. The reference image and the Epic Terrain mod
> it was written from are **not** in the repository and will not be available when the phases are
> implemented: everything needed from them is written down here (§1.1, §3.1).
>
> As each phase lands, the built mechanisms move into [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.3
> (generation), §6.6 (LOD) and §5 (rendering), with status tags updated, and this file keeps the
> design rationale and the tunables. Parts of it that are superseded by what was actually built
> are edited or removed in the same change, as `CLAUDE.md` requires for the architecture.

**Content is still prototype.** `ARCHITECTURE.md` §6.1 applies: block names, colours, biomes and
landforms here are placeholders that give the systems a direction, not Dwell's final content
design. What *is* a decision is the **art direction** (§1: warm, colourful, high fantasy) and the
**world shape** (§2–§4: separated continents, drainage-consistent terrain, a dome of sky islands).
Keep every new number in a parameter table (one header per stage, `server/core/src/worldgen/`) so
the real block set and world style can replace the placeholders without touching the pipeline.
Do not borrow content from other games: the mods cited below are references for *techniques*,
re-implemented from the descriptions here; none of their data or files are copied (Epic Terrain is
"All Rights Reserved").

Contents
1. [Art direction and colour (Phase 7)](#1-art-direction-and-colour-phase-7)
2. [Continents from Voronoi plates (Phase 10)](#2-continents-from-voronoi-plates-phase-10)
3. [Natural terrain: rivers, mountains, biomes (Phase 11)](#3-natural-terrain-rivers-mountains-biomes-phase-11)
4. [Sky islands in a dome (Phase 12)](#4-sky-islands-in-a-dome-phase-12)
5. [Cross-cutting rules for all four phases](#5-cross-cutting-rules-for-all-four-phases)

Where things are today (generator version 10, `ARCHITECTURE.md` §6.3): an 8,192 km disc, sea level
at y = 0, the world from −2,048 to 6,144 m; 12–13 continents from Voronoi plates with a signed coast
distance behind continentalness (§2, built); a smooth valley floor with rivers (three tiers of noise
contours), lakes and terraced static water above sea level (§3.2–3.3, Phase 11a, built, §3.10);
the mountains' detail from a derivative-damped ridged cascade (§3.4, Phase 11b, built, §3.11) on
49 km ranges with 5.4 km massifs; a climate with continent biases, a lapse rate and rain shadows and
a table of 19 biomes (§3.5–3.7, Phase 11c, built, §3.12); surfaces grass
/dirt/sand/sandstone/gravel/snow/stone; oak, spruce and blossom trees, grass and leaves tinted by biome,
boulders and wetland ponds. (The text below is the design these were built from.)
Colours come from `client/src/render/textures.ts` (procedural tiles) and `client/src/world/materials.ts`;
LOD colours are tile averages (`averageTileColor`); faces get a fixed scalar shade in both meshers
(top 1.0, ±X 0.8, ±Z 0.7, bottom 0.55); lights are a hemisphere (`0xdfefff` / `0x4a3b2a`, 1.4) and
a white sun (1.6); the sky is a flat clear colour `0x87b5e0`, which the height fog also fades to.

---

## 1. Art direction and colour (Phase 7)

### 1.1 The reference image (described, since it will not be available)

A painted, storybook high-fantasy vista in late-morning light. Foreground: a clear lake whose
water is **turquoise in the shallows and saturated cobalt in the depths**, with warm-grey and
mossy boulders at the water's edge. Middle ground: terraced cliffs of **warm, pinkish-tan rock**
draped in vegetation, with **many white waterfalls** dropping in stages into the lake; lush,
**rounded broadleaf trees** in yellow-green and emerald, a few dark **conifers**, and frequent
**accent trees**: autumn **orange and rust-red** (upper left), **cherry-blossom pink and
magenta** bushes and trees (lower right, lower left), a **violet/purple** tree (lower left).
Background: very tall, thin **karst spires and needle peaks** (like Zhangjiajie or Guilin) fading
into a **pale blue-lavender haze**; their lit faces read pink-cream, their shaded faces pale blue.
Sky: **saturated cerulean at the zenith grading to near-white at the horizon**, with large white
**cumulus clouds**; warm **sunlight from the upper left** with visible light shafts. (Castles with
red conical roofs and white birds are in the picture but are not terrain and are out of scope.)

The mood to reproduce: **warm, saturated, cheerful, luminous** — never grey, muddy or gloomy.

### 1.2 Colour rules (what makes it read as "happy high fantasy")

1. **Warm light, cool shadow.** Sunlit surfaces lean warm (cream/gold); shaded faces lean **blue
   or violet**, not just darker. This single rule does most of the work.
2. **Saturated mid-tones.** Foliage is vivid yellow-green on top, deeper green/teal in shade.
   Avoid desaturated olive and neutral grey anywhere in the near and middle distance.
3. **Aerial perspective.** Distance desaturates and lightens toward a **pale blue-lavender**
   haze that matches the sky *at the horizon*, so far terrain dissolves into the sky behind it.
4. **Accents, sparingly.** About 5–12 % of trees in temperate areas are accent colours (autumn
   orange/red, blossom pink, magenta, violet), in clumps rather than scattered singly.
5. **Water glows.** Turquoise/cyan where shallow, deep blue where deep, white where it falls.
6. **Rock is warm.** Light warm grey with pink-tan in the light, blue-grey in shade — never
   cold neutral grey. Dirt is a warm reddish brown.
7. **Snow is warm white** in the light and pale blue in shade.

### 1.3 Measured palette (sRGB, sampled from the reference)

Sampled from the image (hue-family medians of the vivid pixels, and point samples). These are the
**targets** that tuning should land near on screen (after lighting), not raw texture values.

| Element | Light | Mid | Shadow / deep |
|---|---|---|---|
| Sky (zenith → horizon) | horizon haze `#E5E5E1` | `#91B7E6` → `#B3CCE6` | zenith `#649ADA` |
| Clouds | lit `#F4F7FA` | `#D3E1EE` | `#C2D8ED` |
| Distant rock in haze | lit `#D2B9B6` | `#B1BAD2` | `#B5CDDE` |
| Near rock / cliffs | `#D9C5AD` | `#C0A8A1` | `#8A7F98` (blue-violet) |
| Foliage, yellow-green | `#E1DF6A` | `#BFB949` | `#8C8F27` |
| Foliage, green | `#95B04F` | `#819F4C` | `#334D29` |
| Foliage, teal shade | `#6AA98E` | `#4C7563` | `#2C5C59` |
| Autumn | `#DC852F` | `#AC5627` | `#8B391C` |
| Blossom pink / rose | `#E85F8B` / `#E87B88` | `#B24162` | `#8E3D59` |
| Violet / magenta | `#9E459D` | `#7E348A` | `#542D62` |
| Water | shallow `#87BFEC` | `#4B9DD3` / `#4898C1` | deep `#4072A9` → `#2F5181` |
| Waterfall / foam | `#F2F8FC` | `#CAE1F0` | — |
| Sunlit cream highlight | `#F5D9A2` | — | — |

### 1.4 What was changed (Phase 7, built)

Built in `client/src/render/look.ts` (the live values), `textures.ts`, `materials.ts` and `render/three/sky.ts`; the tables below are the starting points the work was tuned from, kept for the rationale.

**Scope: colours only, no terrain generation change.** Phase 7 is a first pass at better
colours: it retunes how the existing materials and the sky are drawn. It adds no materials, does
not touch the generator (no version bump, goldens unchanged) and changes no landforms or
vegetation placement. Colourful vegetation — accent trees and new leaf and grass materials — is part
of Phase 11's biome work (§3.7); clouds are with the sky islands (§4.9).

**Material base colours and texture tiles.** Retune `textures.ts` tile generators and
`materials.ts` colours toward §1.3. Starting points (tune on screen):

| Material | Today | Target base (texture varies ± around it) |
|---|---|---|
| grass (top) | `#5E9C3A`, dry `#8FAE3F` | `#8DBF3F`, highlights `#B5CF4F`, low `#5F9A3A` |
| grass side | grass over dirt | same greens over the new dirt, a fringe of 3–4 texels |
| dirt | `#7A5534` | `#8A5A3A` (warm red-brown) |
| stone | `#A4A7AB`, cool `#959CA8` | `#BBAFA6` warm grey, flecks `#D6C6B4` and `#9C95A8` |
| sand | `#DBCF9A` | `#EED9A4` |
| sandstone | `#C9B37A` | `#E0BE86` |
| gravel | `#8C8580` | `#A3968E` |
| snow | `#F2F5F8` | `#FBF8F2` |
| leaves (oak) | `#3F7D2C` | `#6FA83A`, highlights `#A9C94A` |
| log / bark | `#6B4A2B` | `#7A5236` |
| water | `#3F7FD8` / `#2F6FD0` | `#3FB2D6` (turquoise; depth reads through transparency) |
| bedrock | `#4A4A50` | `#4E4858` (keeps reading as "the floor") |

The LOD follows automatically (`lodColor` averages tiles in linear light).

**Per-face tint instead of a scalar shade.** Replace the scalar face shade in `mesh/mesher.ts`
and `mesh/lodMesher.ts` with an RGB multiplier per face direction, defined **once** in a shared
module both meshers import (so chunk and LOD faces stay identical — they are compared in tests
today). Starting values: top `(1.00, 0.97, 0.90)` (warm), ±X `(0.86, 0.84, 0.88)`, ±Z
`(0.74, 0.77, 0.90)` (cooler), bottom `(0.50, 0.55, 0.74)` (blue). Pick the sun direction so the
brighter side faces match it.

**Lights.** Sun warm `#FFF1D6`, raised intensity, from a fixed direction ~45° up (upper-left in
the default spawn view); hemisphere sky `#9CC6F0`, ground `#7A8A4A` (green bounce, not brown).

**Tone mapping.** Turn on `renderer.toneMapping` (ACES Filmic or AgX; compare both) with an
exposure uniform. Three.js applies it in the material shaders, so it costs no extra pass. A small
saturation/warmth adjustment may go in the same shader chunk the height fog already patches
(`render/three/heightFog.ts`); avoid a post-processing pass (mobile budget).

**Sky gradient and horizon-matched haze.** Replace the flat clear colour with a sky dome (a
background shader or large inverted sphere drawn first): zenith `#649ADA` → mid `#91B7E6` →
horizon `#E5E5E1`, plus a soft warm glow around the sun direction. The height fog currently
fades to `SKY`; make it fade to **the sky gradient's colour in the fragment's view direction**
(near-horizon views → horizon haze), so distant terrain dissolves into exactly the sky behind it.
`render/fog.ts` stays the reference for haze amount; add the colour function beside it so it is
unit-testable.

**Water.** Turquoise base; keep the transparent look so beds show through in shallows (the
image's turquoise-to-blue comes from depth). LOD water (already tinted) uses the same base.
Optional if cheap: a slight fresnel brightening at grazing angles.

### 1.5 How Phase 7 was checked

- **Screenshots.** An e2e script captures the same fixed views (seed, position, look direction,
  time) before and after: the spawn, a forest edge, a coast, mountains from a distance, the
  whole disc from the flight ceiling. They are attached to the PR for the owner's review; this is
  the real acceptance check for "looks like the reference".
- **Unit tests** pin the mechanisms, not taste: the shared face-tint table is used by both meshers
  (a chunk face and an LOD face of the same material and direction get identical colours); top
  is warmer than the sides and the bottom bluest; the fog colour at the horizon equals the sky
  gradient's horizon colour.
- **Generation untouched:** the worldgen golden hashes (chunks and LOD) are unchanged.
- **Frame time** within ±5 % of before on the F3 readout (desktop and phone).

---

## 2. Continents from Voronoi plates (Phase 10)

### 2.1 Goal

Replace the fBm land/ocean decision with a **plate layout**: the disc is partitioned by Voronoi
cells into **continents** and **ocean**, so that (1) continents are distinct landmasses with
**guaranteed open ocean between them**, (2) coastlines are irregular and natural at every scale,
not polygonal, and (3) each continent can have its own character. The rest of the generator then
reads continentalness from a **signed distance to the coast** instead of raw noise.

Disc area ≈ 2.1 × 10⁸ km² (about 41 % of Earth's surface). Targets: **6–14 continents** per seed,
land fraction **25–35 %**, continents from ~0.5 to ~8 million km², plus island chains in the
oceans; an open-ocean ring along the rim.

### 2.2 Algorithm (all deterministic, integer-hashed, per column)

Two nested jittered-grid Voronoi layers, both pure functions of `(seed, cell coordinates)`:

**Level 1 — continent cells** (cell size `CONTINENT_CELL` ≈ 2,560 km, ~32 cells in the disc).
Each grid cell `(I, J)` has one site at `cell origin + jitter`, jitter = hash → `[0.15, 0.85]` of
the cell (bounded jitter keeps the nearest site and the nearest border within the 3×3
neighbourhood, so lookups are exact with 9 cells). Each cell is hashed **land** with probability
`CONTINENT_LAND_CHANCE` (≈ 0.35) else ocean; a land cell is **one continent**, with id = the cell
coordinates. Forced rules: the cell containing the origin is land (spawn); a cell whose site lies
beyond `WORLD_RADIUS − RIM_OCEAN` is ocean.

**Level 2 — plates** (cell size `PLATE_CELL` ≈ 256 km). Each plate site is assigned to the level-1
cell nearest to it (evaluated *at the site*, in warped space, below), so a plate belongs wholly to
one continent cell. A plate is **land** when its continent cell is land and its site is at least
`PLATE_INSET` (≈ 120 km) inside that cell's border; a per-plate hash (`BAY_CHANCE` ≈ 0.08) turns
a few edge plates to sea (bays and gulfs). Ocean plates far from any continent become **island
plates** with `ISLAND_PLATE_CHANCE` (≈ 0.04): their land comes from a third, finer hashed layer
(~40 km cells of small blob islands), never within `ISLAND_CLEARANCE` (≈ 150 km) of a continent.

**Warp.** Before both lookups, the sample position is domain-warped by a 2-octave fBm vector
field (wavelength ≈ 1,000 km, amplitude ≈ 0.3 × `PLATE_CELL`). Plates' assignment to continents
uses the same warp, so the continent outline follows plate shapes, not the straight level-1
borders.

**Signed coast distance `s(p)`** (metres; positive on land, negative at sea):

1. *Plate edge distance.* For the plate containing `p` (site `a`) and each neighbouring plate of
   the other kind (sites `b`), the distance to their bisector is
   `((p − (a + b)/2) · (b − a)) / |b − a|`; take the minimum. Positive if `a` is land, negative
   if sea. (Exact for the Voronoi edge; a good approximation of the distance to the land/sea
   boundary as a whole, and continuous across cells.)
2. *Coastline detail.* Add an fBm whose amplitude falls with wavelength: ≈ 60 km at 400 km,
   20 km at 100 km, 5 km at 25 km, and the existing fine octaves down to ~100 m, so coasts are
   fractal at every zoom (headlands, bays, coves). Apply the large octaves on the coarse macro
   lattice (below) and the small ones on the existing 4-column lattice.
3. *Separation clamp* (the guarantee). With `g(p)` = distance from the warped `p` to the nearest
   level-1 border between two **different land continents** (∞ if none nearby):
   `s ← min(s, g − OCEAN_GAP / 2)`. Every point within `OCEAN_GAP / 2` of a border between two
   continents is sea, so any two continents are separated by at least `OCEAN_GAP` (≈ 300 km) of
   open water, whatever the noise does. The same clamp keeps continents and island plates apart
   (`ISLAND_CLEARANCE`) and keeps the rim ring open (`s ← min(s, RIM_OCEAN − (R − r))`, with
   `R − r` approximated by `(R² − r²) / 2R` near the rim to stay sqrt-free).
4. *Shelf and abyss.* Phase 10 maps `s` to the existing continentalness range with a spline:
   a continental shelf (`s` from 0 to −`SHELF_WIDTH` ≈ 80–200 km, varying per continent, depth to
   ~−150 m), a continental slope, and the abyss (~−1,200 to −1,800 m; the world is 2,048 m deep).
   Land rises with `s` toward the continent's interior (`s / (s + K)`, saturating, no `exp`).

**Per-continent character.** Hash the continent id into a small record used by later stages:
base interior elevation, mountainousness, temperature bias (−10 to +10 °C), humidity bias,
prevailing-wind direction (one of 8, so no trig), shelf width. Phase 10 uses the elevation and
shelf width; Phase 11 the rest.

**Plate boundaries inside continents** (exported for Phase 11). For land points, also compute
the distance to the nearest **internal plate edge** (between two land plates of the same
continent) and a hashed per-edge "convergence" value in [−1, 1]. Phase 11 raises mountain belts
along convergent edges and rifts/lowlands along divergent ones, so ranges have plausible
continental-scale placement instead of free noise.

### 2.3 Evaluation cost and lattices

Per sample: level-1, 3×3 sites; level-2, 3×3 plates, each needing its own level-1 owner (another
3×3, cacheable per plate). This is too costly per 4 m lattice point but the fields are smooth at
plate scale, so they live on a **macro lattice** (≈ 256 m, i.e. every 8 chunk columns): exact
values at macro corners, bilinear in between, then fine coast octaves on the existing lattice.
The chunk path, the point queries (`ColumnAt`, `GroundY`, `SolidAt`) and `GenerateLod` all
evaluate the macro lattice the same way (same corner set, same interpolation order — ADR 0010's
rule). At LOD levels whose cells exceed 256 m, sample the corners directly at cell centres.

Budget: chunk generation stays within +10 % of today (~1.2 ms native, ~1.5 ms WASM); LOD section
generation within +10 %. Cache macro corners per chunk column on the server and in the worldgen
worker if needed (a cache of pure-function results does not affect determinism).

**`sqrt`.** The bisector distance needs `|b − a|`. ADR 0010 bans library calls because their
results are not specified to the last bit — but IEEE 754 requires `sqrt` to be correctly rounded,
and both x86-64 SSE (`sqrtss`/`sqrtsd`) and WebAssembly (`f32.sqrt`/`f64.sqrt`) implement it
exactly. Phase 10's ADR amends ADR 0010 to allow `std::sqrt` (and nothing else from `<cmath>`),
with the golden tests as the guard. (Fallback if ever needed: a fixed-iteration Newton square root
using only `+ − × /`.)

### 2.4 What else changes

- **Generator version bump**; golden chunk and LOD hashes regenerated, with new entries: a coast,
  a point inside an ocean gap, an island plate, a continent interior.
- **The 262 km continentalness field** stops deciding land vs. sea; it may remain as a relief
  modulator until Phase 11 replaces relief.
- **Tools.** `dwell_worldgen_inspect` gains a whole-disc image mode (PPM, ~1 pixel per 4–8 km)
  coloured by continent id, plate edges, and height; the in-game F4 map can zoom out to the whole
  disc. These are how continents are reviewed.
- **Spawn** is unchanged in mechanism (spiral from the origin) and guaranteed on land by the
  forced origin cell.

### 2.5 How Phase 10 is checked

- **Separation test** (the key one): for 8 seeds, take ~10,000 random land points; for each, sample
  64 directions × 4 radii up to `0.99 × OCEAN_GAP`; every sample is sea or the same continent.
- **Shape statistics** for 8 seeds: continent count in [6, 14]; land fraction in [0.25, 0.35]; the
  origin on land; no land within `RIM_OCEAN` of the rim; coast length per continent shows
  fractal detail (a test: the coastline's measured length grows by ≥ 1.5× from a 16 km to a 1 km
  ruler).
- Determinism (native = WASM goldens) and the timing budget.
- Manual: whole-disc PPMs for 3 seeds attached to the PR.

### 2.6 What was built (Phase 10), and where it differs from the design above

[ADR 0017](./adr/0017-continents-from-voronoi-plates.md) records the decisions; the code is
`server/core/src/worldgen/continents.cpp` (the layout) and `terrain.cpp` (the terrain from it). Each
difference below is for a reason found in building or testing it:

- **Which cells are land** is a seed-hashed count (12–13, the origin's cell first) of the cells
  hashing lowest, not a per-cell chance of 0.35. The design's numbers do not fit each other: with
  independent chances a seed has 6–15 land cells, and a quarter to a third of the disc is land only
  for 12–14 continents of this cell size (a continent can fill at most ~70 % of its cell once the
  gap and the coast are taken out). Land share over 64 seeds: 25.9–33.4 % (mean 29.8 %); continents 2.3–7.5 million km² (seeds 0–7).
- **Cells are centred on the origin** (cell (0, 0) holds the origin and its site is the origin), and
  **jitter is 0.2–0.8**, not 0.15–0.85: that is what makes the nearest site always lie in the 3 × 3
  cells around a point.
- **The coast distance** is `(D_sea − D_land) / 2`, not a minimum over plate bisectors: the bisector
  form jumps where one land plate gives way to another (each has its own sea neighbours); this one
  is continuous, zero on the land/sea bisectors, and saturates at ±256 km.
- **The clamp is relative to the point's continent** (its cell, or at sea the nearest land plate's
  cell), over the bisectors with every other land cell, and is widened by the warp's Lipschitz bound
  (measured 0.23, bound 0.3). Clamping by the point's own cell alone let land spilling into an ocean
  cell between two continents come within the gap of the other. The coast detail *recedes* a clamped
  coast (0–60 km, never outward) so it is ragged too, and fades out beyond 50–150 km from the coast.
- **A plate's continent** is its site's cell; land and the sea around it keep that continent
  (`Column::continent`), land that spills into an ocean cell included.
- **Islands** keep 750 km from land cells' borders (not 150 km): continents spill past their cells'
  borders, and the separation test samples every continent point out to 0.99 × the gap — islands
  within that would fail it. The seabed offshore of an island falls ten times faster than the
  island rises, so an island's own seabed stays within the blobs searched.
- **Constants:** `PLATE_INSET` 90 km, `BAY_CHANCE` 0.08, `ISLAND_PLATE_CHANCE` 0.05, `RIM_OCEAN`
  512 km, coast detail 9 octaves from 400 km (amplitude 100 km, ×0.65 per octave: ~13 km at 25 km), warp
  76.8 km at 1,000 km, shelf 80–160 km, the abyss −1,500 ± 300 m, the inland rise `s / (s + 40 km)`.
- **Cell hashing** uses its own `CellHash`: `Hash2` repeats for small coordinates (39 distinct values
  in the 49 cells around the origin).
- **The macro lattice** holds every field of the layout, evaluated exactly at its corners and cached
  per thread; the level of detail reads it below 256 m cells and evaluates the layout at cell
  centres above (anchors every 4 columns, interpolated only in interiors and the deep sea), and a
  cell too wide for the erosion and ridged octaves reads their means.
- **Checks:** the separation test samples 500 land points per seed for 8 seeds in CI
  (`DWELL_SEPARATION_POINTS=10000` runs the exit criterion's count: 0 violations in 20.5 million
  samples); the shape statistics, the coastline ruler (1 km sees 1.9–2.3 × the 16 km length), the
  Lipschitz bound of the warp, cache purity, the shelf/slope/abyss profile and the landmarks the
  goldens use are in `continents_test.cpp`. Whole-disc images: `dwell_worldgen_inspect seed disc`.

---

## 3. Natural terrain: rivers, mountains, biomes (Phase 11)

### 3.1 The reference: what Epic Terrain does (described, since the mod will not be available)

Epic Terrain (v0.2.5, a Minecraft 1.20 data pack by wonderfulAi_Chen) rewrites the vanilla
overworld's noise router — the set of 2D "density functions" that drive height and biomes —
to get realistic, efficient terrain. Studying its files gives these techniques (Minecraft
blocks = metres; its world is 576 m tall with sea level at 63, so its relief is at most ~450 m):

1. **A pure heightfield.** It removes vanilla's 3D "jaggedness" and squashing factor: the final
   density is just a vertical gradient plus a 2D offset. Terrain is a smooth 2.5D surface: no
   floating bits, believable slopes. (Caves are still carved separately.)
2. **Continents.** One very low-frequency, multi-octave continentalness noise (lowest wavelength
   ~8 km). Only in the **coastal band** (from just offshore to the shoreline) is the sample
   position **domain-warped** — by a single-octave noise scaled to a few hundred metres — so
   coastlines are irregular while the interior stays unwarped. Very rare extreme-negative values
   raise small islands out of the deepest ocean.
3. **Rivers as noise contours.** Two independent single-octave noises: **main rivers** where
   `|R1|` ≈ 0 (wavelength ~5 km) and **minor rivers** where `|R2|` ≈ 0 (~1.5 km). A zero
   contour of smooth noise is a long, meandering, never-ending line: a river network for free,
   with no simulation and no neighbour reads. The channel profile is a smooth spline of `|R|`:
   main channel full depth for `|R1| < 0.002`, easing to zero by 0.03; minor full for
   `|R2| < 0.012`, zero by 0.06 (flat bed, sloping banks).
4. **Mountains grow away from rivers — the core idea.** The mountain factor is
   `M = |R2|² + (|R2| + 0.15) × 1.5 × clamp(C + |R1|, −0.18, 1)` (C = continentalness). Height rises
   with distance from the nearest river and with inlandness, so **every river runs along a valley
   floor** and ridges form between rivers: a drainage-consistent landscape without erosion
   simulation.
5. **Rivers fade inland.** Channel depth is offset by inlandness (`clamp(channel + 0.4 C +
   0.6 |R2|, −1, 0)` for main rivers, and the minor channels vanish at moderate inlandness), so
   rivers are deepest at the coast and peter out in the highlands — they read as flowing from
   the hills to the sea.
6. **Height mapping.** A spline from continentalness gives the deep ocean (~50 m below sea), the
   ocean floor, a shelf just below sea level, the coast just above it; inland, river centrelines
   map to below sea level (so they hold water), and elsewhere a **smoothstep from the mountain
   factor** spans from ~15 m below sea level (wet lowlands, lakes) to ~450 m above it.
7. **Layered mountain detail with slope damping.** Three noise layers at decreasing wavelengths
   (~850 m, ~256 m, ~128 m). Each is used in **ridged** form (`1 − 1.5 |n|`) and multiplied by
   `(1 − k × |∇n|)`, where the gradient magnitude is estimated by finite differences 3 m apart,
   so steep noise slopes are flattened and noise crests and troughs survive — sharp ridges and
   smooth valleys, an eroded look. Each finer layer's amplitude is **multiplied by the coarser
   result and by `M`**, so detail appears only on mountains and grows with height; lowlands stay
   smooth.
8. **Biomes follow the terrain.** Humidity is computed as `(1 − C)(T + 1) − 1`: wetter near
   coasts and in warm places, drier inland. The biome "erosion" input is derived from `M` and
   humidity; the "weirdness/ridges" input is zero exactly in river channels, so **river biomes
   sit exactly on the carved channels** and peak biomes on high `M`. Tiny extra noises (×0.01–0.05)
   are added to the biome inputs only, for variety without changing the terrain.
9. **Caves respect rivers.** Cave entrances and tunnels are disabled where `|R2| < 0.15`, so caves
   never breach river beds.
10. **Water features.** River biomes get frequent small ponds and water patches dug into the
   banks (and frozen variants in the cold); swamps get many small lakes.

What Dwell takes: 1 (mostly), 3, 4, 5, 7, 8, 9, 10 — re-derived and re-tuned for a world with
5 km mountains, a sea level of 0, and a 16,000 km disc. What Dwell does differently: rivers flow
**above** sea level inland (valley floors rise with the continent), and Phase 10's plates, not
noise, decide continents.

### 3.2 Height model

Every term is a 2D field per column (on the existing lattice, or the macro lattice for smooth
ones), combined in one fixed order:

```
V  = valley floor        — base elevation of the drainage network (smooth, macro lattice):
                           0 at the coast, rising inland with Phase 10's s (saturating), plus
                           plate-boundary uplift (convergent belts raise V into high valleys and
                           plateaus) and the per-continent interior elevation.
Rg, R1, R2               — river noises for three tiers (§3.3).
D  = distance-from-rivers factor, Epic Terrain's M generalised to three tiers:
     D = f(|Rg|, |R1|, |R2|) — ~0 on any channel, growing away from all of them.
U  = uplift              — how mountainous this place may be: plate convergence belts (Phase 10),
                           Dwell's existing 49 km ranges and massifs, continent mountainousness.
Hd = mountain detail     — the derivative-damped ridged cascade (§3.4), amplitude ∝ D × U.
h  = V + D × U × A + Hd − carve      (A = relief scale; carve from the channel profiles)
```

So a mountain is high because it is far from rivers **and** in an uplifted belt; rivers always
lie at `V` in valley bottoms. Lowlands (low `U`) stay gently rolling; plains along great rivers are
broad and flat. Coastal plains, deltas, and cliffs fall out of `V → 0` meeting `U`: low `U` at the
coast gives beaches; high `U` gives sea cliffs (surface rules, §3.6).

Overhangs: keep the 3D overhang noise but small (≈ 1–3 m) on most land and larger only on steep
mountain faces and cliffs, for voxel interest without floating debris (the stability pass still
runs). Today's overhang (up to ~17 m) is reduced.

### 3.3 Rivers and lakes

**Three tiers**, each the zero contour of its own single-octave noise (2D gradient noise, no
warp needed; optionally a gentle domain warp for meanders):

| Tier | Noise wavelength | Channel width (bed / banks) | Depth | Exists where |
|---|---|---|---|---|
| Great river | ~150–300 km | 80–300 m / 1–3 km valley | 8–20 m | from deep inland to the sea; widens toward the coast |
| River | ~6 km | 15–40 m / 200–400 m | 3–6 m | lowlands and mid-elevations; fades above a height |
| Stream | ~1.5 km | 3–8 m / 30–80 m | 1–2 m | everywhere except high peaks; fades at altitude |

(Width is set by the threshold on `|R|` divided by the noise's local gradient; with a
fixed threshold, width varies naturally. Use the noise's analytic gradient, §3.4, to keep width
roughly constant where needed.) Each tier's channel depth fades with height above its valley
floor and with inlandness (technique 5), so streams become dry gullies high up and rivers start
at springs. Channels are carved **into `V`**, the valley floor, so they always run along valleys.

**Water above sea level.** Today water only fills open space below `SEA_LEVEL`. Rivers need water
at their own level:

- River surface `W(x, z) = V(x, z) − freeboard` (≈ 1 m), **quantised into terraces** (e.g. every
  2–6 m of `V`, step size hashed per terrace). Within a terrace the surface is flat.
- Water fills a column's open space from its bed up to `W` wherever the channel profile is
  non-zero; the banks are higher than `W` by construction (`h ≥ V` outside channels).
- Where a channel crosses from one terrace to the next, the upper pool's water ends in a vertical
  face over the lower pool: a **waterfall step** — exactly the image's stepped falls. Static water
  is fine for this: the face renders as a water curtain.
- At the coast `V → 0` so `W → 0`: rivers meet the sea at sea level.
- Water is still static (no flow simulation); this is a generation rule only. The integrity and
  water behaviour of edits are unchanged.

**Lakes.** Jittered-grid lake cells (≈ 3–10 km), hashed presence, more frequent in wet
lowlands and at the meeting of tiers. A lake has a flat surface at the quantised `V` at its centre,
a radius from the hash, a domain-warped shoreline (squared-distance mask plus noise, sqrt-free),
and a bowl-shaped bed. Rivers passing through take the lake's surface. Swampy/wet biomes add many
small ponds (technique 10): shallow hashed blobs of water in low, flat ground.

**Caves** are suppressed within ~12 m below any river bed, lake bed, or shallow sea floor near
coasts (technique 9), so no cave drains or breaches water.

### 3.4 Mountain detail without `exp`, `pow` or library calls

Port technique 7 with **analytic derivatives** instead of finite differences: the quintic-fade
gradient noise in `noise.h` has a closed-form derivative using only `+ − ×`, so each octave returns
`(value, ∂x, ∂z)` for little extra cost. The cascade per octave `i` (coarse to fine):

```
n_i, g_i = noise_i(p)                        (value and gradient)
G       += g_i                               (accumulated gradient)
ridge_i  = 1 − 1.5 |n_i|                     (ridged)
damp_i   = 1 / (1 + k × (G · G))             (derivative damping: rational, no sqrt/exp)
Hd      += a_i × ridge_i × damp_i × prev     (prev = the coarser layers' normalised result:
                                              detail multiplies with height, technique 7)
```

Octaves from ~4 km down to ~16 m (more octaves than Epic Terrain, for kilometre-scale relief);
the LOD drops octaves finer than its cell as today. This gives sharp crests, smooth valleys and
talus-like slopes, all deterministic under ADR 0010.

**Fantasy landforms** (stretch, after the realistic base is approved; the reference image's
spires): a rare "karst" province (hashed per plate or by a low-frequency mask) where tall, thin
**spires** rise from lowland — cellular noise (jittered points ~60–200 m apart, steep cones from
squared distance) with heights 100–600 m, flat tops with vegetation, and waterfalls from terraced
basins between them. Also mesas/buttes in dry provinces (terraced height quantisation).

### 3.5 Climate and biomes

**Temperature** (°C, for readability): per-continent bias + a very low-frequency noise
(~2,000 km) + local noise − **lapse rate** 6.5 °C per km of height above sea level. Snow lines and
tree lines then come from altitude naturally and work for 5 km massifs.

**Humidity** (0–1): coastal proximity `1 / (1 + max(s, 0) / L)` (L ≈ 300 km), plus noise, plus
wetness near rivers and lakes, minus a **rain shadow**: sample `V + U × A` (the smooth large-scale
height, not detail) at 3 points upwind along the continent's prevailing wind (20, 60, 150 km);
humidity drops in proportion to how much higher the upwind barrier is than here. All
macro-lattice, so cheap.

**Biome selection** is a data table, not code: a temperature × humidity grid (a Whittaker-style
diagram) gives the base biome, then **overrides** from terrain: altitude above the tree line →
alpine meadow → bare rock → snowfield; slope above a threshold → cliff; within a channel or on a
lake shore → riverbank/shore; within a few metres of sea level at the coast → beach (low slope) or
sea cliff (high slope); tiny noises vary inputs (technique 8). Borders are dithered by hash so
they are not straight lines.

Prototype biome set (names are placeholders): meadow, broadleaf forest, blossom grove (warm,
humid), autumn woods (cool, moderately humid), conifer forest, marsh/wetland, savanna (warm, dry),
dunes (hot, very dry), tundra (cold, dry), alpine meadow, bare rock / scree, snowfield and
glacier, beach, sea cliff, riverbank, lake shore, ocean, deep ocean, frozen ocean. Each biome
row in the table names its surface materials (top, filler, under-water), tree kinds and density,
grass and foliage tints (§3.7), and ground cover — so the real content can replace these rows later.

### 3.6 Surfaces

Extend `SurfaceMaterial` to read the biome table: top/filler/depth per biome; steep → rock
(stone; sandstone in dry biomes); scree/gravel at the foot of steep slopes; sand on beaches and
lake shores in warm climates, gravel in cold; snow above the snow line where the slope allows;
riverbeds gravel/sand. Keep the top-down column pass and its rules (cave air does not start a
surface).

### 3.7 Colourful vegetation (Phase 11c)

The reference image's character (§1.1) comes largely from vegetation colour that varies by region.
It is built as a **tint by biome** (the owner's decision, 2026-10-07; first built as seven variant
blocks — five leaf colours and two grasses — which were removed): the blocks stay plain `grass` and
`leaves`, and each biome row of the table (§3.5) holds a grass and a foliage colour that multiplies
their texture where it is tinted (meadow yellow-green, autumn woods orange leaves and golden grass,
blossom grove pink, conifer teal, savanna olive and gold, tundra grey-green). Nothing is stored in
the voxels or sent over the network: the generator, a pure function of the seed, gives the tint of
any column (§3.12), and the client asks it. The design's per-tree accents (a grove noise picking
which colour dominates a patch) are not built: a tree is the colour of its biome, which is also why
trees are not customised yet — the owner will design them. The blossom tree (a short trunk and a wide
round crown) is a shape the blossom grove uses.

- **Distant forests keep their colour.** Trees exist in LOD only up to 4 m cells, so beyond ~1 km
  forests would read as grass. In `GenerateLod`, at levels above the tree limit, forested columns
  (broadleaf, blossom, autumn and conifer biomes) take `leaves` as their surface, tinted like the
  trees, so distant hillsides keep the canopy's colour. A test checks a forested site's level-4
  surface is leaves.

### 3.8 What else changes

- **Generator version bump(s).** Phase 11 is large: it may ship as 11a (height model, rivers,
  lakes, water above sea level), 11b (detail cascade), 11c (climate and biome table), each a
  version bump with regenerated goldens.
- **Air chunks and LOD bounds.** `SkyFloor`/`IsAirChunk` and `LodBoundsAt` must include water
  above sea level (a chunk above the ground but below a river surface is not air). Their tests
  extend to rivers and lakes.
- **LOD.** `GenerateLod` evaluates the same fields with octaves dropped by cell size; river
  channels narrower than a cell disappear at that level (streams at 16 m+ cells), while great
  rivers and lakes remain visible from altitude. The agreement tests gain a river and a lake
  site.
- **Tools.** The inspect map shows rivers, lakes and biomes; add a hillshade image mode (PPM) for
  reviewing relief and drainage.
- **Spawn** keeps its rules (level, open, tree-free land) and should prefer a spot near water.

### 3.9 How Phase 11 is checked

- **Rivers lie in valleys:** for random points on channel centrelines, the bed is no higher than
  the terrain at every point 50–500 m to either side, perpendicular to the channel.
- **Rivers reach the sea:** along great-river centrelines within 5 km of the coast, the water
  surface is at sea level.
- **Water never floats:** in generated chunks, every water voxel has water or solid below it, and
  every horizontal water/air contact is at a terrace step (a waterfall), counted and bounded.
- **No caves under water:** no cave air within the suppression depth below any water.
- **Climate:** snow appears only above the altitude its temperature implies; a range's lee side is
  drier than its windward side (sampled across several ranges).
- **Vegetation:** the biomes' tints agree between neighbouring chunks, blend smoothly over borders
  and match the level of detail's; a forested site's level-4 LOD surface is leaves. (The design's
  clumped accent trees are not built: colour is by biome, §3.7.)
- **Biome shares** within tolerance bands for 8 seeds (no biome missing, none above ~35 %).
- Determinism goldens; chunk ≤ +25 % and LOD section ≤ +25 % of today's time; the LOD agreement
  thresholds still met.
- Manual: walk a river from a spring to the sea; fly over a range; screenshots for the owner.

### 3.10 What was built (Phase 11a), and where it differs from the design above

[ADR 0018](./adr/0018-drainage-consistent-terrain.md) records the decisions; the code is
`server/core/src/worldgen/rivers.cpp` (the river noise, terraces and lakes) and `Finish` in
`terrain.cpp` (the height model). Each difference is for a reason found in building or testing it:

- **`V` is the smooth part of the old base height.** It reads the layout's macro-lattice coast
  distance, never the local 1.4 km octaves (those roughened it by ±3 m and put river beds above
  their banks), and everything but the shore's 2 m is faded in from 5 to 30 km inland, so a great
  river is at sea level within 5 km of the coast whatever ranges stand near. Its terms: the coast's
  lowland and the continent's elevation, a slow rise `250 m × (s / (s + 150 km))²`, uplift belts
  (120 m, within 60 km of a convergent plate edge) and 12 % of the ranges' relief, so valley floors
  climb into the mountains and mountain streams have terraces — and waterfalls — often.
- **`U` is the belts only** (900 m of relief within 60 km of a convergent edge, scaled by the
  ridged field) plus the old ranges: the per-continent mountainousness is not in the layout's
  `MacroCorner` and is left to 11b/11c.
- **The hills never dip below `V`** (they are `0..1 ×` their amplitude, not `−1..1`): the ground
  outside a channel is at least `V`, which is what keeps every river's water at least a metre below
  its banks. Land near a coast is therefore a metre or so higher than the sea's side of the shore.
- **The channel profile** is `(1 − s)⁴` of the smoothstep `s` between `core` and `bank`, not the bare
  smoothstep: the bare form left a great river's water 5 km wide on its gentle banks. Great-river
  `bank` is 0.008 (not 0.03); the thresholds are in `rivers.h`'s `Tier` table.
- **Spring noise** (not in the design): the two small tiers exist only where a 12 km noise is high,
  so streams and rivers begin and end (as dry gullies, the distance factor still carving the
  valley) instead of looping everywhere. Without it a 130 km view was a maze of closed loops.
- **Lattice offsets** (not in the design): Perlin noise is zero at its lattice points, so every
  tier crossed at the origin in every world — a river junction at the spawn.
- **Meanders** for the great river too (2.5 km at 30 km), so it is not a straight line across a
  continent.
- **Lakes** are on a 12.3 km grid (the design's 3–10 km cannot hold a 4 km lake and its berm
  without neighbours' domains touching), 0.7–2.0 km in radius, 4–12 m deep. A lake's surface is the
  terrace of `V` at its centre less 3 m; a 1.5 m berm rims the shore. **Rivers stop at a lake's
  shore** and end in a step into it: they do not take the lake's surface through the lake.
  **Wetland ponds** are left to 11c (they depend on the biome table).
- **Closed loops remain** for the river tier: a zero contour of Perlin noise is mostly a loop of
  about its wavelength. With the spring mask they are arcs that begin and end, but they do not run
  from a source to the sea; only the great river does. A flow-aware network would need a hierarchy of
  sources (ADR 0018, Consequences).
- **Not done in 11a:** the great river widening toward the coast; rivers taking a lake's surface.
- **Checks:** `rivers_test.cpp` — terraces 2–6 m; no tier through the origin; beds no higher than
  the ground to either side; great-river mouths at sea level (12 over 4 seeds); no floating water and
  every horizontal water/air contact a step down (143,000 water voxels, 564 contacts at steps, 0
  elsewhere); no cave air in the 12 m under water; the spawn beside water.

### 3.11 What was built (Phase 11b), and where it differs from the design above

[ADR 0021](./adr/0021-mountain-detail-cascade.md) records the decisions; the code is `Perlin2d` and
`DampedRidges2` in `noise.h` / `noise.cpp` and `Finish` in `terrain.cpp`. Differences from §3.4:

- **It replaces the ridged field's contribution rather than adding a layer.** `Hd = D × U × A` is
  the existing relief terms with the cascade in place of `c.ridges`: the uplift belts (900 m), the
  upland weight (18 m + 150 m) and, new, 300 m on the ranges. The relief stays non-negative (the
  ground never dips below `V`, which keeps the rivers' freeboard).
- **Octaves:** nine, from a 4 km wavelength to a 16 m lattice, damping `k = 0.6` over a 600 m
  reference relief; `prev` is the coarser octaves' normalised result and 1 for the first octave.
- **Cost control:** the cascade is evaluated at the 4 m lattice corners only on land where an uplift
  weight is not zero (`CascadeWanted`); the level of detail keeps the octaves a cell resolves and
  reads the cascade's mean (0.55) for cells that resolve fewer than two octaves (4 km and up). Its lattice is shifted by a hashed offset (as the river tiers' are): the coarse levels' cell centres would otherwise all sit on its zeros. A power-of-two wavelength splits
  coordinates by shifts, not divisions.
- **Checks** (`cascade_test.cpp`): the analytic gradient against central differences (worst error
  0.0003); the cascade in [0, 1] with relief; stronger damping lowers the roughness; the finer
  octaves add more over high coarse ground than over low; LOD octave dropping keeps the coarse shape
  (correlation 0.9997); mountains are ~19 times rougher than lowland over 16 m; the mean cascade is
  the level of detail's constant; channels stay near the valley floor (20 m of relief against 470 m
  beside them).

### 3.12 What was built (Phase 11c), and where it differs from the design above

[ADR 0022](./adr/0022-climate-biome-table-vegetation.md) records the decisions; the code is
`biomes.h` / `biomes.cpp` (the table), `SampleClimate`, `RainShadow` and `Finish` in `terrain.cpp`,
the ponds in `rivers.cpp`, and the blocks in `shared/blocks/dwell.json`. Differences from §3.5–3.7:

- **The rain shadow** is read from a *smooth ground height* of the layout (the valley floor's rise,
  the uplift belts) plus two octaves of the planet-scale ranges, not from `V + U × A`, which is not
  available per point cheaply; it is evaluated on an 8 km lattice (16 cells at the level of detail)
  and interpolated, cached per thread — a pure function of the seed and the lattice point. The
  barrier must stand 150 m over the ground here to cast a shadow and 2 km over it for a full one.
- **Temperature gets a land offset** (+0.12) and the records' biases at 0.55: with the first
  numbers snowfields covered 26 % of the land. The tree line, bare-rock and snow temperatures are
  −0.28, −0.36 and −0.45 (design: tree line at 620 m at sea-level 0; now 860 m, bare rock 1,110 m,
  snow 1,385 m), and the zone table's rows cover the diagram without gaps (a test).
- **Borders** are roughened by two noises of 37 and 53 m wavelength and ±0.012 (field units), not a
  hash per column: a per-column hash would speckle a 10–50 km transition band, and the existing
  local noise pair (5 % of each field) already does so on a smaller scale.
- **Ponds** are decided by the oracle at each cell's *centre* (wetland humidity, flat, inland), not
  per column: every column of a pond agrees on its level without reading the biome table. A pond
  never has terrace steps and never reaches a neighbouring cell. Wetland is 6–11 % of the land;
  a pond is a handful of metres in 1–2 % of it.
- **Colour is a tint, not blocks** (decided after the first build, which had seven variant blocks and
  accent groves): `TintGrid` gives each chunk column 3 × 3 points 16 m apart (the biomes' colours
  blurred 3 × 3 so neighbouring chunks agree and borders blend); the mesher gives tinted blocks'
  vertices the grid's bilinear value and the shader applies it where the atlas alpha says so (the
  grass side's fringe but not its dirt); the level of detail carries each column's tint in its
  surface data. Sections the player has modified draw untinted from afar. Per-tree accents are
  dropped; the trees are the owner's to design later.
- **Not built:** `frozen_ocean` is a label that changes the sea floor to gravel (there is no ice
  block); the cliff biome by slope (steep ground is stone in every biome's row instead); per-
  continent mountainousness (the record's `mountainousness` is still unused — 11d's karst
  provinces would read it).
- **Checks:** see `ARCHITECTURE.md` §6.3 *Climate, biomes and vegetation*. Cost against 11b (Release,
  near the spawn, `bench`): a chunk 0.78 → 0.96 ms (+23 %), LOD sections +11–25 % (level 1
  1.43 → 1.78 ms, level 5 1.47 → 1.79, level 8 1.70 → 2.02, level 12 0.79 → 0.88).

---

## 4. Sky islands in a dome (Phase 12)

> **Decided, with open details.** The world's shape (§4.1–§4.2) and the island terrain (§4.3–§4.4,
> after the owner's Aether spec in [`reference/aether-floating-islands.md`](./reference/aether-floating-islands.md))
> are decided (2026-10-05). What remains open is listed in §4.8.

### 4.1 Intent (decided)

The world **extends upward into a full hemispherical dome over the disc**: a half-sphere whose
radius is the disc's radius, `DOME_RADIUS` = `WORLD_RADIUS` = 8,192 km, centred on the disc's centre
at sea level. It reaches **8,192 km above the centre** and comes down to meet the ground at the
rim. The land below is unchanged (ground terrain stays between −2,048 and 6,144 m); the vast sky
inside the dome is **sparsely populated with separate floating islands**, everywhere above the
terrain and inside the dome, so the world reads as if it were enclosed in a full spherical dome
above the land.

Terrain reference: the Aether mod's sky islands (Minecraft) — only the landforms, not its
dungeons, creatures or items — described by the owner's spec (§4.3). The character: separate
islands with abrupt cliffs and ragged outlines, grassy tops (with grass on lower ledges too), bare
stone undersides pinching off into hanging points, small sealed lakes, waterfalls from springs in
cliffs, small round-canopied trees, cloud banks floating beneath the islands (§4.9).

Phase 13 ([`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md)) later mirrors the terrain and this dome onto
the disc's underside, with gravity toward the midplane.

### 4.2 World bounds: from an 8 km slab to a dome (the architectural change)

Today (ADR 0011) the world is the disc × [`WORLD_MIN_Y` −2,048, `WORLD_MAX_Y` 6,144). Phase 12
supersedes the vertical part with a new ADR:

- **Two bounds instead of one.** `TERRAIN_MAX_Y` = 6,144: the top of the **ground band**, the only
  place ground terrain (and its generator shortcuts) exists — today's `WORLD_MAX_Y` keeps this
  meaning. The **world** is the disc's ground band plus the dome: a voxel with `y ≥ 0` is inside
  when `x² + y² + z² < DOME_RADIUS²` (`InsideWorldDome`, squared integers in 64 bits). Nothing is
  generated outside it; what happens at the dome's surface (an invisible wall, a kill boundary, a
  visible sky shell) is §4.8 question 2. Every use of `kWorldMaxY` / `worldMaxY` is classified as
  "ground band" (the generator's sky floor, the fly-speed band, `LodBoundsAt`'s terrain bounds) or
  "world bound" (edit validation `InWorldRows`, `kMaxChunkY`/`MAX_CHUNK_Y`, streaming row clipping,
  LOD row counts, air-chunk bounds) and switched to the right constant; tests cover both.
- **Precision already fits.** World y up to 8,192,000 m is the same magnitude as the disc's x and
  z: Jolt is double precision; the local player's wire positions are f64 and `posfix` (1/256 m,
  ±8,388,608 m) covers the dome's top; chunk y up to 256,000 is well within `int32`; noise splits
  y into lattice cell and offset exactly like x and z (ADR 0011 point 4); terrain collision regions
  are already 2,048 m **cubes**, so bodies near islands keep small offsets. The creative-flight
  ceiling is already 24,000 km. Tests run the player and island suites near the dome's top.
- **Streaming.** The view sphere is clipped to the world (ground band ∪ dome) instead of to rows.
  The air-chunk shortcut must keep almost the entire dome free: a chunk above the ground band is
  air unless an island's bounding box overlaps it (§4.5).
- **LOD becomes truly 3D.** From level 8 up, sections no longer span the world's height in a single
  row (the octree "behaves as a quadtree" today): at level 8 the dome is ~1,000 rows tall. The root
  (level 19, a 16,777 km cube from (−2²³, `WORLD_MIN_Y`, −2²³)) still contains the whole dome.
  `LodIndex`/`LodIndexUpdate` gain the row `j` per entry (today `i32 i, i32 k`, "one row, so no
  j") — a **protocol version bump** with golden vectors. Section classification extends from column
  bounds to the ground band's bounds **plus** island bounds (§4.5).
- **Rendering.** The world is already drawn from the 24,000 km flight ceiling, so depth range and
  the near/far pass split exist; measure them with islands at all altitudes and inside the dome
  looking up. The sky gradient (Phase 7) and haze are defined for any altitude.
- **Physics.** Gravity stays uniform and downward everywhere; a fall from an island far up lasts a
  long time at terminal speed and ends in fall damage as today. How players reach islands without
  creative flight is gameplay, out of scope here (§4.8).

### 4.3 The island field: Aether's density, in sparse archipelagos

**Reference.** The owner's engine-agnostic spec of the Aether mod's floating-island terrain
(Aether 1.5.11, Minecraft 1.21.1) is kept verbatim in
[`reference/aether-floating-islands.md`](./reference/aether-floating-islands.md); its numbers are
the mod's own. Read it alongside this section. In short: islands are **not** shapes placed one by
one. They are the positive part of a 3D noise density inside a 128 m band, faded toward empty near
the band's top and bottom so the noise breaks up into separate islands with tapered undersides.
Everything else (soil, lakes, springs, trees, clouds) is decoration on top of that field.

**Shape field (taken as is; units are metres = voxels):**

- Two shape fields `A`, `B`: fractal gradient noise, longest wavelength 191.5 m, each octave halving
  wavelength and amplitude; 4–5 octaves suffice on the lattice below (Dwell's LOD drops finer ones
  by cell size anyway). One selector `S`: 8 octaves, longest wavelength 59.85 m horizontally and
  119.7 m vertically. `q = clamp(0.5 + 12.8 S, 0, 1)`, `N = A + (B − A) q` — a near-binary switch
  (~85 % of samples pure A or pure B) whose seams make abrupt cliffs and ragged outlines.
- Height gain (band-relative `y`): `N × (1 + 0.9 × clamp((y − 32) / 96, 0, 1))`. Without it high
  terrain mostly vanishes.
- Vertical shaping: `bottom(y) = clamp((y − 8) / 32, 0, 1)`, `top(y) = clamp((128 − y) / 72, 0, 1)`;
  `d = N × gain − 0.13`; `d = −0.2 + top × (d + 0.2)`; `d = −0.1 + bottom × (d + 0.1)`;
  `density = d − 0.05`; solid where `density > 0`. Short steep bottom ramp → undersides pinch off
  into hanging points; long shallow top ramp → most tops just above the core, rare high peaks.
- Lattice: density at points every 8 m horizontally and 4 m vertically, trilinear in between.
  This is Dwell's own pattern (a coarse lattice plus interpolation), so the chunk path, `SolidAt`
  and the LOD share one evaluation order as ADR 0010 requires.
- Targets from the spec (per band, for tuning tests): ~6 % of the band solid (peak ~17 % at
  y = 56); ~1/3 of columns with land above; islands typically 15–20 m thick (tail to ~50); ~1 in 5
  land columns with a second walkable layer under an overhang. Coverage dial: the core threshold
  0.18 (= 0.13 + 0.05), set at the matching percentile of Dwell's own `N` (the spec's 0.18 sits near
  the 85th percentile in the core band). The final `d/2 − d³/24` squash does not change the sign
  and is skipped.

**Archipelagos — fitting a 128 m band into an 8,192 km dome (Dwell's addition).** Filling the
whole dome with one band is impossible and filling its volume with the field would not be sparse.
Instead, the field runs inside **archipelagos**: local instances of the Aether band placed sparsely
throughout the dome.

- **Layout.** 3D jittered-grid cells (`ARCHIPELAGO_CELL` ≈ 16 km × 2 km tall × 16 km); one hashed
  candidate per cell, present with probability `p(altitude)` (the density profile, a tunable; start
  uniform). A candidate has a centre, a plan radius (≈ 2–6 km, warped outline from a squared-
  distance mask plus noise, sqrt-free), a band base `y_b`, and a **scale** `s` (hashed: 1 most
  often, 2 sometimes, 4 rarely) that multiplies every length of the field (wavelengths, band
  height, ramps, lattice spacing), so some archipelagos have islands up to ~800 m across and
  60–80 m thick that read from far away. Each archipelago's own noise seeds come from its cell
  hash.
- **Footprint fade.** Inside the archipelago the band-relative `y` is `(y − y_b) / s`; toward the
  plan edge, the density is faded to empty like the vertical ramps (a horizontal ramp over the
  outer ~20 % of the radius), so no island is cut off by the archipelago's boundary.
- **Separation.** An archipelago's footprint and band fit inside its cell with a gap, so
  archipelagos never touch; islands within one are separated by the field itself, as in Aether.
- **Altitude.** Every archipelago lies wholly between `ISLAND_MIN_Y` (≈ 7,000 m, above the ground
  band, so never touching even the 5.6 km massifs) and the dome.
- **Reachability within a cluster.** An archipelago is ~1/3 covered in islands, so islands within
  it are tens of metres apart; archipelagos are kilometres apart. How players cross between them
  is gameplay (§4.8).

### 4.4 Surface layering and decoration

**Material roles map to existing blocks — no new blocks** (the reference's rule): base stone →
`stone`, grass-topped soil → `grass`, soil → `dirt`, edge-shelf sand → `sand`, ice-stone → `snow`,
common / mid-tier / rare ore → `coal_ore` / `iron_ore` / `gold_ore`, water → `water`, trees →
`log` + a leaf material (Phase 11c's leaf variants when they exist). Clouds have no block (§4.9).

**Surface layering** (taken as is): per column from the top down, *every* solid cell with air
directly above is a floor → `grass` (or `dirt` if water is above); the next soil-depth solid cells
below each floor → `dirt` (depth ≈ 3, varied 1–5 by a slow 2D noise); undersides stay bare stone;
no sea level and no groundwater on islands. Dwell's column pass gains this rule for island cells
(ground cells keep theirs, where cave air does not start a surface); `kSurfacePad` (8) already
covers the cells above a chunk's top that the rule reads.

**Region climate** (taken as is, optional): two 2D noises — temperature (wavelengths ~1,024 and
~256 m, weights 1.5 : 1) and humidity (~512 and ~256 m, equal), lightly domain-warped — select
Meadow / Grove / Forest / Woodland by the reference's table, which sets **tree attempts per
16 × 16 m region** (1; 2–3; 6–7; 5–6) and leaf colour. This replaces the lapse-rate climate on
islands (which would freeze everything above ~7 km). Sampled every 4 m like Dwell's 2D fields.

**Decoration without neighbour reads (the main adaptation).** The reference decorates a 16 × 16
region after its neighbours' terrain exists and may write one region beyond its own. Dwell chunks
must stay pure functions of their coordinates (§5), so every decoration is restated as a
**deterministic feature function**: a hash of `(seed, archipelago, region cell, pass salt)` picks
its candidates, and its preconditions are tested with **point queries on the density field**
(`SolidAt` and a floor query, exact and cheap on the 8 × 4 × 8 lattice) instead of reading
generated chunks. A chunk then writes only its own voxels of every feature that reaches it, in a
fixed pass order, as trees and boulders do today. Interactions between passes are resolved by
asking the earlier pass's function (e.g. trees ask "is there a lake here?"). Passes, in order, with
the reference's frequencies per 16 × 16 region (heights band-relative):

| Pass | Frequency | Rule in Dwell |
|---|---|---|
| Edge shelf | 1 in 5 regions, y 0–48 | For each column, the first air cell with `grass` above and air above that gets a flat `sand` disk of radius 3.46 (squared-distance test), air cells only |
| Lake | 1 in 15 regions | 16 × 8 × 16 box 4 m below the surface; union of 4–7 ellipsoids (3–9 wide, 2–6 tall); **leak check** by point queries on the shape's border in the lower 4 layers; lower 4 layers water, upper 4 cleared, rim re-grassed |
| Ores and pockets | dirt 20 × ≤33, snow 10 × ≤32, coal 20 × ≤16, iron 14 × ≤5 (y ≤ 75), gold 5 × ≤3 (y ≤ 74) and 7 × ≤4 (triangular, peak y 8) | Replace stone only; iron and gold discard half their air-exposed cells. Attempts pick random points in the band, so ore per rock scales with the rock, as in the reference |
| Spring | 30 attempts, y 8–128 | Stone/dirt above and below, exactly 3 solid horizontal neighbours and 1 air: a **static waterfall** — water from the source out one cell and straight down until solid, or for at most `WATERFALL_MAX` (≈ 48 m × s) when it falls off the island (§4.8) |
| Trees | by region type, every walkable layer | Layered ground finder: layer n with a 1 in 2 chance picks a column and its n-th floor from the top with ≥ 4 air above; 99 % small (trunk 4–6, round canopy radius 2), 1 % large (trunk 10, radius 3); soil below, no water |
| Ground cover | ~10 grass patches, flowers/bushes 1 in 8–16 | Skipped until Dwell has cross-shaped plant looks (no new blocks now) |
| Small clouds, cloud banks | see §4.9 | Render-only |
| Tiny islet | 1 in 50 regions, y 32–96 | Only if its tree fits: a radius-2 diamond (13 cells) of grass over stone, a 5-cell plus of stone below, one small tree. Exempt from the stability pass |

Scaled archipelagos (`s` > 1) scale the field, not the decorations: trees, lakes and ores keep
their metre sizes, but region cells and frequencies stay per 16 × 16 m of island, so bigger
islands simply carry more of them.

### 4.5 What else the generator must change

Sky islands break the generator's "one surface per column" assumption. Each item becomes part of
Phase 12's ADR and of `ARCHITECTURE.md` §6.3/§6.6/§7.1 when built:

1. **Chunks above the ground band** look up the archipelago cells overlapping them; ground chunks
   pay nothing.
2. **Air chunks.** `IsAirChunk`: a chunk is air if it is above the ground's sky floor **and**
   outside every overlapping archipelago's bounding box (footprint × band). This keeps the dome
   cheap. Inside a box most chunks are still air: a cheaper bound (e.g. the band's empty zones
   below y 8 and the lattice maximum) may skip them — measure first.
3. **Surface pass:** the island rule above; ground surfaces unchanged (islands never reach the
   ground band).
4. **Point queries:** `GroundY` for the ground; an island floor query ("n-th floor from the top")
   for decorations; the spawn stays on the ground.
5. **LOD.** Sections above the ground band are `Empty` unless an archipelago's box overlaps them;
   inside, the field is evaluated per cell with octaves finer than the cell dropped (like caves),
   decorations only where at least a cell wide. Archipelagos with `s` = 4 (islands up to ~800 m) keep at least two
   cells per island to about 100 km (level 9, 512 m cells); beyond that the dome is empty sky
   unless §4.8 question 1 adds a distant impression.
6. **Stability pass.** The field can leave small fragments; the pass removes those under 48 voxels
   inside a chunk, which is acceptable. Tiny islets (18 cells) are exempt by construction (the pass
   skips voxels a feature function placed).
7. **Structural integrity (Phase 14).** Islands are not connected to bedrock, so the first edit
   would detach a whole island. Islands need an **anchor**: e.g. the generated island field's
   voxels count as grounded while unmodified components remain larger than a threshold, or an
   indestructible core per island; finding "per island" is hard with a density field, so a
   grounded flag on generated island material is the likelier answer. Decided in Phase 12's ADR;
   Phase 14's anchor definition follows it.
8. **Streaming and memory.** Measure chunk counts and memory in a flight through archipelagos
   against today's budgets.

### 4.6 How Phase 12 is checked

- **Bounds:** nothing generated outside the dome; edits accepted up to the dome and refused
  outside it; streaming and LOD rows reach the dome's top; the player suites pass near the top.
- **Field statistics** against the reference's targets, per band over several seeds and
  archipelagos: solid share by band height (overall ~6 %, peak ~17 % near y 56, within a few
  points), columns with land ~1/3, island thickness distribution (typical 15–20 m), second
  walkable layer ~1 in 5 land columns; `N` centred on 0.
- **Archipelagos:** none touch; none below `ISLAND_MIN_Y` or crossing the dome; presence matches
  the profile; no island cut by a footprint edge (density ≤ 0 at the footprint boundary).
- **Decorations:** lakes never leak (every lake water cell bordered by solid or water below and
  beside, except the open top); springs satisfy their neighbour rule; trees stand on soil and not
  in water; ores replace only stone; each pass is order-independent across chunks (a feature
  crossing a chunk border is identical from both sides).
- **Cost:** open sky still skipped as air chunks (a flight test counts generated chunks); chunk
  and LOD times inside archipelagos reported.
- Islands survive the stability pass; protocol golden vectors for the new `LodIndex`; determinism
  goldens with island chunks and sections.
- Manual: a flight through an archipelago and views of the dome from the ground and from high up,
  reviewed by the owner.

### 4.7 Ordering

Phase 12 builds on Phase 11 (leaf variants, the water rules). The dome bounds (§4.2) can be built
first, as 12a; then 12b islands (field, archipelagos, surface, decorations); 12c clouds (separable,
render-only).

### 4.8 Open questions

Resolved 2026-10-05: the dome covers the **whole disc**; the world's ceiling is **raised to a full
hemispherical dome** (8,192 km); islands follow the Aether reference and use **existing blocks
only**; island climate is Aether's region climate, not the lapse rate. Still open:

1. **Archipelago layout:** cell size, presence and its profile over altitude, and how much of the
   dome is visible from the ground (larger `s` for some, or a render-only distant impression).
2. **The dome's surface:** an invisible boundary, a kill boundary like the void, or a visible sky
   shell; and whether creative flight may leave the dome.
3. **Waterfalls off islands:** static water cannot fall forever; the draft ends them after
   `WATERFALL_MAX` (or at a cloud bank). Revisit if water ever flows.
4. **Under-island shading** and darkness under large islands (no shadows today).
5. **Reaching islands** outside creative flight (gameplay; likely a later phase).
6. **Island anchors** for integrity (§4.5 item 7).

### 4.9 Clouds (render-only, 12c)

The reference places clouds as blocks: **small clouds** (blob walks of 16 / 8 / 4 steps in 1 in 7 /
1 in 24 / 1 in 75 regions, band y 32–96, the rarest 96–128; each step moves 0–1 per horizontal
axis on a fixed diagonal heading and half the time ±1 vertically, stamping a 3–4 × 2 box clipped
to Manhattan distance 4–5) and long **cloud banks** under each band (sites on a 96 m jittered grid
with a 48 m minimum gap; 64-step walks with a fixed drift of −1/0/+1 per axis, stamping 9–12 m × 2 m
blobs, height changing 1 step in 10; starting at band y 0–32, so they read as a cloud floor). With
no new blocks, Dwell keeps these **render-only**: the same deterministic walks (a hash per site,
so any chunk can find the cloud cells inside it) produce a cloud mask that the client's worldgen
worker returns beside a chunk's voxels, meshed as soft, translucent white geometry with no
collision, not stored or synchronised, lit warm on top and blue-grey underneath and faded by the
haze. Ground-level skies get a sparse layer of the same banks at ~1,500–2,500 m (the reference
image's cumulus). Budget ≤ ~1 ms per frame on a phone (F3 readout); clouds must not hide the LOD
terrain from the flight ceiling. If clouds later need to be solid or editable, they become a
material then.

---

## 5. Cross-cutting rules for all four phases

- **Determinism (ADR 0010, 0011).** Integer hashes for every placement decision; only `+ − × /`,
  comparisons and (ADR 0017) the correctly rounded `sqrt`; no float conversion of whole world
  coordinates;
  one evaluation order shared by chunks, point queries and LOD. Every output change bumps the
  generator version and regenerates goldens (`DWELL_UPDATE_GOLDEN=1`). No phase migrates saved
  worlds: saves from before the version launcher (Phase 6) are not carried over, and after it
  each world is locked to its app version's compatibility line (`RELEASES.md` §6).
- **No neighbour reads.** Every stage is a function of world coordinates and hashes only (chunks
  generate in any order). Rivers, lakes, plates and islands are all designed to satisfy this.
- **Slopes are already in place.** Slope blocks (Phase 9, [`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md))
  land before Phases 10–12, so every new terrain stage keeps the surface-shaping rule (slopes from
  the continuous height at cell corners, flooded below water) working, and its tests and LOD slopes
  pass, as part of the phase that changes the terrain.
- **LOD parity. Every new field has an LOD evaluation with octaves and features finer than a
  cell dropped; the generate-vs-downsample agreement tests get a site for each new landform.
- **Budgets.** Chunk generation and LOD section generation times are measured in every phase
  (`dwell_worldgen_inspect` timings) and reported in the PR.
- **Parameters.** Each stage's tunables live in one table; prototype values are expected to change
  in playtesting without design churn.
- **Docs.** When a phase lands, `ARCHITECTURE.md` (§6.3, §6.6, §5, §6.1's material list, §7.1 for
  island anchors) and this file are updated in the same change, and the phase's ADR is written.

### Parameter summary (starting points)

| Name | Value | Phase |
|---|---|---|
| `CONTINENT_CELL` | 2,560 km | 10 (built) |
| Continents (land cells) | 12–13, seed-hashed (was `CONTINENT_LAND_CHANCE` 0.35) | 10 (built) |
| `PLATE_CELL` | 256 km | 10 (built) |
| `PLATE_INSET` | 90 km | 10 (built) |
| `BAY_CHANCE` / `ISLAND_PLATE_CHANCE` | 0.08 / 0.05 | 10 (built) |
| `OCEAN_GAP` | 300 km | 10 (built) |
| `ISLAND_CLEARANCE` (from land cells' borders) | 750 km | 10 (built) |
| `RIM_OCEAN` | 512 km | 10 (built) |
| `SHELF_WIDTH` | 80–160 km per continent | 10 (built) |
| Coast warp (λ / amplitude) | 1,000 km / 76.8 km (0.3 × `PLATE_CELL`) | 10 (built) |
| Coast detail | 9 octaves from 400 km, 100 km × 0.65 per octave | 10 (built) |
| Macro lattice | 256 m | 10 (built) |
| River tiers (λ) | ~200 km / ~6 km / ~1.5 km | 11 |
| River terrace step | 2–6 m (hashed) | 11 |
| Lake cells | 3–10 km | 11 |
| Lapse rate | 6.5 °C / km | 11 |
| Rain-shadow samples | 20 / 60 / 150 km upwind | 11 |
| Cave suppression under water | 12 m | 11 |
| Overhang amplitude | 1–3 m (land), more on cliffs | 11 |
| `DOME_RADIUS` | = `WORLD_RADIUS`, 8,192 km (decided) | 12 |
| `TERRAIN_MAX_Y` (ground band top, today's `WORLD_MAX_Y`) | 6,144 m | 12 |
| `ISLAND_MIN_Y` | ~7,000 m | 12 |
| Island field | the reference's twelve parameters ([`reference/aether-floating-islands.md`](./reference/aether-floating-islands.md), "Parameter reference") | 12 |
| `ARCHIPELAGO_CELL` | 16 km × 2 km × 16 km (provisional) | 12 |
| Archipelago radius / scale `s` | 2–6 km / 1, 2 or 4 (provisional) | 12 |
| `WATERFALL_MAX` | 48 m × `s` (provisional) | 12 |
