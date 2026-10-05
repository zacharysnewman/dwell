# Dwell — World Generation Plans: Look, Continents, Terrain, Sky Islands

> **Status: [planned].** This is the design reference for implementation Phases 10–13
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
1. [Art direction and colour (Phase 10)](#1-art-direction-and-colour-phase-10)
2. [Continents from Voronoi plates (Phase 11)](#2-continents-from-voronoi-plates-phase-11)
3. [Natural terrain: rivers, mountains, biomes (Phase 12)](#3-natural-terrain-rivers-mountains-biomes-phase-12)
4. [Sky islands in a dome (Phase 13, provisional)](#4-sky-islands-in-a-dome-phase-13-provisional)
5. [Cross-cutting rules for all four phases](#5-cross-cutting-rules-for-all-four-phases)

Where things are today (generator version 4, `ARCHITECTURE.md` §6.3): an 8,192 km disc, sea level
at y = 0, the world from −2,048 to 6,144 m; continentalness from fBm plus a 262 km field; ridged
mountains and 49 km ranges with 5.4 km massifs; biomes ocean, beach, plains, forest, desert, snowy,
mountains; surfaces grass/dirt/sand/sandstone/gravel/snow/stone; oak and spruce trees, boulders.
Colours come from `client/src/render/textures.ts` (procedural tiles) and `client/src/world/materials.ts`;
LOD colours are tile averages (`averageTileColor`); faces get a fixed scalar shade in both meshers
(top 1.0, ±X 0.8, ±Z 0.7, bottom 0.55); lights are a hemisphere (`0xdfefff` / `0x4a3b2a`, 1.4) and
a white sun (1.6); the sky is a flat clear colour `0x87b5e0`, which the height fog also fades to.

---

## 1. Art direction and colour (Phase 10)

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

### 1.4 What to change

**Scope: colours only, no terrain generation change.** Phase 10 is a first pass at better
colours: it retunes how the existing materials and the sky are drawn. It adds no materials, does
not touch the generator (no version bump, goldens unchanged) and changes no landforms or
vegetation placement. Colourful vegetation — accent trees and new leaf and grass materials — is part
of Phase 12's biome work (§3.7); clouds are with the sky islands (§4.8).

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

### 1.5 How Phase 10 is checked

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

## 2. Continents from Voronoi plates (Phase 11)

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
4. *Shelf and abyss.* Phase 11 maps `s` to the existing continentalness range with a spline:
   a continental shelf (`s` from 0 to −`SHELF_WIDTH` ≈ 80–200 km, varying per continent, depth to
   ~−150 m), a continental slope, and the abyss (~−1,200 to −1,800 m; the world is 2,048 m deep).
   Land rises with `s` toward the continent's interior (`s / (s + K)`, saturating, no `exp`).

**Per-continent character.** Hash the continent id into a small record used by later stages:
base interior elevation, mountainousness, temperature bias (−10 to +10 °C), humidity bias,
prevailing-wind direction (one of 8, so no trig), shelf width. Phase 11 uses the elevation and
shelf width; Phase 12 the rest.

**Plate boundaries inside continents** (exported for Phase 12). For land points, also compute
the distance to the nearest **internal plate edge** (between two land plates of the same
continent) and a hashed per-edge "convergence" value in [−1, 1]. Phase 12 raises mountain belts
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
exactly. Phase 11's ADR amends ADR 0010 to allow `std::sqrt` (and nothing else from `<cmath>`),
with the golden tests as the guard. (Fallback if ever needed: a fixed-iteration Newton square root
using only `+ − × /`.)

### 2.4 What else changes

- **Generator version bump**; golden chunk and LOD hashes regenerated, with new entries: a coast,
  a point inside an ocean gap, an island plate, a continent interior.
- **The 262 km continentalness field** stops deciding land vs. sea; it may remain as a relief
  modulator until Phase 12 replaces relief.
- **Tools.** `dwell_worldgen_inspect` gains a whole-disc image mode (PPM, ~1 pixel per 4–8 km)
  coloured by continent id, plate edges, and height; the in-game F4 map can zoom out to the whole
  disc. These are how continents are reviewed.
- **Spawn** is unchanged in mechanism (spiral from the origin) and guaranteed on land by the
  forced origin cell.

### 2.5 How Phase 11 is checked

- **Separation test** (the key one): for 8 seeds, take ~10,000 random land points; for each, sample
  64 directions × 4 radii up to `0.99 × OCEAN_GAP`; every sample is sea or the same continent.
- **Shape statistics** for 8 seeds: continent count in [6, 14]; land fraction in [0.25, 0.35]; the
  origin on land; no land within `RIM_OCEAN` of the rim; coast length per continent shows
  fractal detail (a test: the coastline's measured length grows by ≥ 1.5× from a 16 km to a 1 km
  ruler).
- Determinism (native = WASM goldens) and the timing budget.
- Manual: whole-disc PPMs for 3 seeds attached to the PR.

---

## 3. Natural terrain: rivers, mountains, biomes (Phase 12)

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
**above** sea level inland (valley floors rise with the continent), and Phase 11's plates, not
noise, decide continents.

### 3.2 Height model

Every term is a 2D field per column (on the existing lattice, or the macro lattice for smooth
ones), combined in one fixed order:

```
V  = valley floor        — base elevation of the drainage network (smooth, macro lattice):
                           0 at the coast, rising inland with Phase 11's s (saturating), plus
                           plate-boundary uplift (convergent belts raise V into high valleys and
                           plateaus) and the per-continent interior elevation.
Rg, R1, R2               — river noises for three tiers (§3.3).
D  = distance-from-rivers factor, Epic Terrain's M generalised to three tiers:
     D = f(|Rg|, |R1|, |R2|) — ~0 on any channel, growing away from all of them.
U  = uplift              — how mountainous this place may be: plate convergence belts (Phase 11),
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
accent palette (§3.7), and ground cover — so the real content can replace these rows later.

### 3.6 Surfaces

Extend `SurfaceMaterial` to read the biome table: top/filler/depth per biome; steep → rock
(stone; sandstone in dry biomes); scree/gravel at the foot of steep slopes; sand on beaches and
lake shores in warm climates, gravel in cold; snow above the snow line where the slope allows;
riverbeds gravel/sand. Keep the top-down column pass and its rules (cave air does not start a
surface).

### 3.7 Colourful vegetation (Phase 12c)

The reference image's character (§1.1) comes largely from vegetation colour variety: accent trees
following §1.2 rule 4. This needs new materials and the generator to place them, so it belongs
with the biome table rather than Phase 10's colour pass:

- **Leaf materials:** `leaves` (green, retuned), `leaves_bright` (yellow-green), `leaves_autumn`
  (orange), `leaves_red` (rust-red), `leaves_blossom` (pink), `leaves_violet`; each with its own
  procedural tile. **Grass variants:** `grass_meadow` (grass with sparse pink/white/yellow flower
  flecks in the top tile), `grass_golden` (warm dry grass for dry areas). All are full cubes using
  the existing looks; placeable like their base materials. C++ table and the TypeScript mirror
  (`materials.ts`) change together, checked by the existing mirror test.
- **Tree kinds:** the existing broadleaf (oak) and conifer (spruce), plus a **blossom tree**
  (shorter, wide round crown) and an **autumn tree** (broadleaf with autumn leaves). Crown
  colour is chosen per tree from the area: a low-frequency "grove" noise (~150–400 m) picks which
  accent dominates a patch, and a per-tree hash picks accent vs. green, so accents come in
  clumps (§1.2 rule 4). The biome table (§3.5) decides the allowed set and accent share per biome.
- **Distant forests keep their colour.** Trees exist in LOD only up to 4 m cells today, so
  beyond ~1 km forests read as grass. In `GenerateLod`, at levels above the tree limit, forested
  columns take a **canopy material** (the leaf material that the grove noise would give there,
  with accent dithering by hash) as their surface, so distant hillsides look like the image's
  colourful canopy. The downsample of real chunks agrees in class (solid), so the existing
  LOD agreement tests still apply; add a test that a forested site's level-4 surface is mostly
  leaf materials.

### 3.8 What else changes

- **Generator version bump(s).** Phase 12 is large: it may ship as 12a (height model, rivers,
  lakes, water above sea level), 12b (detail cascade), 12c (climate and biome table), each a
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

### 3.9 How Phase 12 is checked

- **Rivers lie in valleys:** for random points on channel centrelines, the bed is no higher than
  the terrain at every point 50–500 m to either side, perpendicular to the channel.
- **Rivers reach the sea:** along great-river centrelines within 5 km of the coast, the water
  surface is at sea level.
- **Water never floats:** in generated chunks, every water voxel has water or solid below it, and
  every horizontal water/air contact is at a terrace step (a waterfall), counted and bounded.
- **No caves under water:** no cave air within the suppression depth below any water.
- **Climate:** snow appears only above the altitude its temperature implies; a range's lee side is
  drier than its windward side (sampled across several ranges).
- **Vegetation:** new materials mirrored (C++/TypeScript) and placeable; accent trees are clumped
  (the fraction of accent trees whose nearest tree is also an accent is well above the overall
  accent fraction); a forested site's level-4 LOD surface is mostly leaf materials.
- **Biome shares** within tolerance bands for 8 seeds (no biome missing, none above ~35 %).
- Determinism goldens; chunk ≤ +25 % and LOD section ≤ +25 % of today's time; the LOD agreement
  thresholds still met.
- Manual: walk a river from a spring to the sea; fly over a range; screenshots for the owner.

---

## 4. Sky islands in a dome (Phase 13, provisional)

> **Provisional.** The owner will provide more detail on the Aether mod's floating islands (the
> mod files were too large to attach). This section records the intent and the architectural
> consequences, which do not depend on those details. **Update §4.2–§4.4 from that reference
> before Phase 13 starts**, and resolve the open questions in §4.7.

### 4.1 Intent

Separate floating islands in the sky that, together, form a **half-dome over the whole world**:
seen from afar, the islands' envelope rises from low near the rim to its highest over the centre.
Terrain reference: the Aether mod's sky islands (Minecraft) — only the islands' landforms, not its
dungeons, creatures or items. General character to aim for (to be refined from the reference):
flat-ish, grassy, rolling tops with trees and small lakes; craggy rock undersides that **taper
downward** to points or hanging spurs (inverted cones / teardrops); a range of sizes from small
islets to large islands; clear air between islands; puffy clouds nearby; water spilling off edges.

### 4.2 Placement (sketch)

- **Envelope.** The dome's height above the disc: `y_dome(r) = Y_RIM + (Y_TOP − Y_RIM) × (1 − r²/R²)`
  (a paraboloid; a hemisphere-like profile `sqrt(1 − r²/R²)` is allowed once `sqrt` is, §2.3).
  Islands occupy a shell of thickness `T` below the envelope. Placeholders: `Y_TOP` ≈ 5,000 m,
  `Y_RIM` ≈ 1,500 m, `T` ≈ 1,500 m — subject to §4.7.
- **Cells.** Jittered-grid cells per size class (e.g. islets in ~300 m cells, islands in ~2 km
  cells, great islands in ~12 km cells), one hashed candidate per cell, its radius at most
  `cell / 2 − gap` so islands of one class never touch; classes are kept apart by a clearance
  check against the coarser classes' cells (a few hashes). Altitude: hashed within the shell;
  density highest near the envelope so the dome silhouette reads from afar.
- **Clearance from the ground.** An island exists only if its underside is at least `CLEARANCE`
  (≈ 300 m) above the highest terrain in its footprint (`LodBoundsAt` at a coarse level gives that
  bound cheaply); otherwise it is lifted (within `WORLD_MAX_Y` minus its height) or dropped.

### 4.3 Island shape (sketch)

Per island (centre `c`, radius `ρ`, top altitude `y_c`):
- **Plan mask** `m = 1 − |p − c|² / ρ²` plus domain-warp noise (sqrt-free); island where `m > 0`.
- **Top** `y_c + A_top × m × hills(p)`: gentle hills; optional hashed lake basin.
- **Underside** `y_c − D × m²` (deep in the middle, tapering steeply to the edge) plus 3D noise for
  crags and hanging spurs. `D` ≈ 0.6–1.2 × `ρ`.
- **Materials:** island top/filler from the biome table (cooler, by altitude; Phase 12's lapse
  rate), rock body (a distinct sky-rock material is a content question for the reference).
- **Features:** trees on tops (`GroundY` generalised to "the surface below this height"),
  decorative waterfalls from lakes off edges (static water curtains, §3.3), clouds (§4.8, or voxel
  clouds — §4.7).

### 4.4 What the architecture must change (independent of the reference)

Sky islands break the generator's "one surface per column" assumption. Each item becomes part of
Phase 13's ADR and of `ARCHITECTURE.md` §6.3/§6.6/§7.1 when built:

1. **Column model.** `Column` gains up to two **island spans** (bottom, top) from island cells
   overlapping the column; evaluated only for chunks whose y-range intersects the shell band, so
   most chunks pay nothing.
2. **Air chunks.** `SkyFloor`/`IsAirChunk`: a chunk is air if it is above the ground's sky floor
   **and** outside every island span's bounds. The air shortcut that makes open sky free must keep
   working above, between and below islands.
3. **Surface pass.** "Open sky" for the ground must not be blocked by an island overhead (today
   air under an overhang is treated like cave air and does not start a surface). The pass starts a
   surface from the column's 2D ground height and, separately, from each island span's top.
4. **Point queries and features.** `GroundY` (ground) plus an island-top query; trees and spawn
   use the right one; the spawn stays on the ground.
5. **LOD bounds.** `LodBounds` becomes a ground interval **plus** an island band; sections between
   the ground and the band are `Empty`; `LodKindFromBounds` and its tests extend. Islands must be
   visible from afar (the great-island class exists partly for that).
6. **Stability pass.** Generated islands must survive it: every island is ≥ 48 voxels (`kMinComponent`)
   or the pass exempts island voxels.
7. **Structural integrity (Phase 6).** Islands are not connected to bedrock, so the first edit
   would detach the whole island. Islands need an **anchor**: e.g. an indestructible anchor core
   at each island's centre (like bedrock), or a generated-island flag treated as grounded. Decide in
   Phase 13's ADR; Phase 6's anchor definition must allow it (noted in Phase 6's deliverables).
8. **Streaming and memory.** Island chunks high up are real chunks; the view sphere already covers
   them. Measure chunk counts in a flight over an island field against today's budgets.
9. **Players.** Falls from islands use existing fall damage; nothing new is assumed.

### 4.5 How Phase 13 is checked (sketch)

Separation between islands of all classes (no two islands touch); clearance above the ground
everywhere; the dome envelope (island top altitudes vs. `y_dome(r)` within the shell) across the
disc; air chunks still skipped between islands (a flight test counts generated chunks); LOD
sections of the band classified correctly; islands survive the stability pass; determinism
goldens with island chunks; manual flight screenshots.

### 4.6 Ordering

Phase 13 builds on Phase 12 (biomes by altitude, lakes, waterfalls). Its integrity anchor touches
Phase 6; whichever lands first defines the anchor rule and the other follows it.

### 4.7 Open questions (resolve with the Aether reference)

1. **"Half dome" shape:** a dome *envelope* of islands over the whole disc (this draft), or a
   different arrangement (e.g. a dome over each continent, or a hemispherical *shell* seen from
   the ground as a vault)?
2. **Altitude vs. the 5 km massifs.** The world's ceiling is 6,144 m and massifs reach ~5.6 km.
   Either the dome stays below typical massif heights and islands skip massifs (the clearance
   rule), or Phase 12 keeps massifs out from under the dome's top, or the world's ceiling rises.
3. **Island sizes and spacing** (Aether's are tens to a few hundred metres; the dome needs some
   kilometre-scale ones to read from afar).
4. **Distinct materials** (sky rock, sky grass, cloud blocks with special collision?) and whether
   clouds are voxels (synchronised, editable) or render-only.
5. **Under-island shading** and darkness under large islands (no shadows today).

### 4.8 Clouds (render-only)

Large white cumulus are a big part of the reference. Prototype: a render-only cloud layer of
instanced, flattened puffy shapes (or noise-textured impostors) at ~1,500–2,500 m, placed by a
deterministic hash grid around the camera (client only, not in the voxel world, not
synchronised), lit warm on top and blue-grey underneath, faded by the same haze. They must not
cost more than ~1 ms of frame time on a phone (measure with the F3 readout) and must not hide the
LOD terrain from the flight ceiling (fade them out above them). Sky islands want clouds near them;
whether some clouds are voxels instead is §4.7 question 4. Built in Phase 13, first, since it is
separable and render-only.

---

## 5. Cross-cutting rules for all four phases

- **Determinism (ADR 0010, 0011).** Integer hashes for every placement decision; only `+ − × /`,
  comparisons and (after Phase 11's ADR) `sqrt`; no float conversion of whole world coordinates;
  one evaluation order shared by chunks, point queries and LOD. Every output change bumps the
  generator version and regenerates goldens (`DWELL_UPDATE_GOLDEN=1`); old versions retire as
  today (a world saved with a retired version loads as the flat world).
- **No neighbour reads.** Every stage is a function of world coordinates and hashes only (chunks
  generate in any order). Rivers, lakes, plates and islands are all designed to satisfy this.
- **LOD parity.** Every new field has an LOD evaluation with octaves and features finer than a
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
| `CONTINENT_CELL` | 2,560 km | 11 |
| `CONTINENT_LAND_CHANCE` | 0.35 | 11 |
| `PLATE_CELL` | 256 km | 11 |
| `PLATE_INSET` | 120 km | 11 |
| `BAY_CHANCE` / `ISLAND_PLATE_CHANCE` | 0.08 / 0.04 | 11 |
| `OCEAN_GAP` | 300 km | 11 |
| `ISLAND_CLEARANCE` | 150 km | 11 |
| `RIM_OCEAN` | 400 km | 11 |
| `SHELF_WIDTH` | 80–200 km per continent | 11 |
| Coast warp (λ / amplitude) | 1,000 km / 0.3 × `PLATE_CELL` | 11 |
| Macro lattice | 256 m | 11 |
| River tiers (λ) | ~200 km / ~6 km / ~1.5 km | 12 |
| River terrace step | 2–6 m (hashed) | 12 |
| Lake cells | 3–10 km | 12 |
| Lapse rate | 6.5 °C / km | 12 |
| Rain-shadow samples | 20 / 60 / 150 km upwind | 12 |
| Cave suppression under water | 12 m | 12 |
| Overhang amplitude | 1–3 m (land), more on cliffs | 12 |
| Dome `Y_TOP` / `Y_RIM` / shell `T` | 5,000 / 1,500 / 1,500 m (provisional) | 13 |
| Island classes (cell size) | 300 m / 2 km / 12 km (provisional) | 13 |
| Island ground clearance | 300 m | 13 |
