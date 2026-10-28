# Dwell — World Generation: Look

> **Status (generator 21, ADR 0049):** the terrain is the world map's — mountains, rivers and
> lakes (§4). The notes below on the interim height field describe its history; it survives only
> as the map's input.
>
> **Earlier status:** §1 (the art direction and colour, Phase 7) is **[built]** and stands. The rest of this
> file — continents from Voronoi plates (Phase 10), rivers, mountains, climate and biomes (Phase 11)
> and sky islands (Phase 12) — was **cut by the baseline rework** (Phase 15,
> [`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md), [ADR 0025](./adr/0025-baseline-world.md)):
> the generator is an interim one-island height field ([`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.3)
> until Phase 17 replaces it with terrain definitions (data, [ADR 0026](./adr/0026-terrain-definitions.md)),
> and Phase 12 is cancelled. The removed designs remain in the repository's history (and in their
> ADRs, 0017–0022, superseded); `ARCHITECTURE.md` §6.3 describes what is built.
>
> **The interim height field's relief (generator version 16, owner 2026-10-19: "I want terrain
> steep enough to slide on").** Its parameters are data, `shared/worldgen/terrain.json`: the inland
> rise (320 m, half done 6 km inland), the sea floor (−20 m over 2.5 km), hills (±225 m, 1,024 m,
> 5 octaves, faded in 0.5–4 km inland) and bumps (±5 m, 128 m, 3 octaves). The owner's targets:
> steeper hills everywhere (not mountain regions), about 15 % of the land's surface steeper than
> the slide's 44° (Phase 35's slide on the adaptive blocks' 45° ramps, ridge flanks and saddles,
> [`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md)), and peaks up to about 500 m above sea level.
> Sliding needs single-block steps between neighbouring columns (a gradient near 1; two blocks or
> more is a cliff), so the steepness comes from the hills' shorter wavelength and extra octave (each
> octave of a fractal sum adds about as much gradient), with the rise lifted so the deeper hollows
> stay dry; no noise term was added (a ridged or terraced one would wall basins in with slides,
> which a plain sum's varying steepness does not). Over 7 seeds and both faces (`cargo run
> --release -p dwell-worldgen --example relief`): sliding surface ~0.01 → ~15 %, highest ground
> ~194 → ~504 m, height standard deviation 36 → 91 m, mean slope 5.9° → 21.6°, island under water
> 0.6 → 0.8 %, columns too steep for tall grass 0 → 0.3 %; of the dry land within 512 m of the
> spawn, 91–100 % is reachable without climbing a slide (79–100 % both ways). The bumps and the
> coast are unchanged. Version 16 also reads the decorations' height band in metres above sea
> level, as `decorations.json` says (it had been compared with the face-local top). A test pins it
> (`the_hills_are_steep_enough_to_slide_and_reach_about_500_m`).
>
> **Generator version 18** (owner 2026-10-20): the owner's two pines in groves, placed by data
> (`shared/worldgen/placements.json`, §2 below, ADR 0043); the height field is unchanged.
>
> **Generator version 15** (owner 2026-10-19: "more height from the noise"): the inland rise 130 m,
> hills ±80 m, 2,048 m, 4 octaves, and bumps ±5 m. Version 14 had hills of ±30 m, bumps of
> ±2.5 m and a 90 m rise. Over 7 seeds and both faces the land's height standard deviation went
> from 21 to 36 m, the highest ground from ~105 to ~194 m above sea level and the mean slope from
> 2.7° to 5.9°; the island's coast is unchanged (the layout is untouched) and its hollows below sea
> level stay under 1 % of it. Prototype content (§6.1 of the architecture): tune by editing the
> file, regenerating, bumping the generator version and the goldens.

**Content is still prototype.** `ARCHITECTURE.md` §6.1 applies: block names and colours here are
placeholders that give the systems a direction, not Dwell's final content design. What *is* a
decision is the **art direction** (§1: warm, colourful, high fantasy).


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

Built in the Three.js client of the time (`client/src/render/look.ts`, `textures.ts`, `materials.ts`,
`render/three/sky.ts`, `mesh/mesher.ts`: the file names in this section are that build's), and
ported to Rust by Phase 24 ([ADR 0028](./adr/0028-rust-port.md)): the live values are in `dwell-look`
(`lib.rs`: palette, face tints, lights, the sky and its generated WGSL; `fog.rs`: the haze amount;
`sky_frame.rs`), the texture tiles in `dwell-mesh` `textures.rs`, the block colours in `dwell-mesh`
`style.rs`, and tone mapping, exposure and fog in the wgpu renderer's WGSL (`dwell-client`
`render/shaders.rs`). The tables below are the starting points the work was tuned from, kept for
the rationale.

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

## 2. Structure placements: the pines (generator version 18) **[built]**

The owner (2026-10-20): generate the two hand-built pines with the world — "theoretically small
versions of the conifer in the growth lab" ([`TREE_GROWTH.md`](./TREE_GROWTH.md)). They are
committed structures (`shared/structures/pine_tree.json`, 5 × 5 × 5, and `tall_pine_tree.json`,
7 × 9 × 7; [`STRUCTURES.md`](./STRUCTURES.md)), placed by a data-driven layer
([ADR 0043](./adr/0043-structure-placements.md)). Prototype content (ARCHITECTURE.md §6.1).

**The owner's answers.**

| Question | Answer | In the data |
|---|---|---|
| Density | Scattered groves: clustered by a noise field into forest patches with clearings between, sparse elsewhere. Only on grass tops: not sand, not slide-steep faces (> 44°), not under water, not overlapping each other or the shore | `groves` (192 m, 40 % coverage, 6 % kept outside), `density` 0.75 per 7 m `grid` cell, `on` grass, `footprint` "on" (every column under the tree is dry grass, which keeps it off the sand), `maxSlopeDeg` 44, `spacing` 1 |
| Altitude | More on higher ground, fewer near the shore, none on the highest bare peaks; the decorations' height band (metres above sea level) | `height` [3, 400] (tall grass stops at 400 m too), `rise`: 15 % of the chance at 3 m rising to all of it at 120 m |
| Variation | A random 90° turn and a mix of the small and tall pine at about 60/40, placed exactly as authored otherwise | `rotate` [0, 90, 180, 270]; `structures` weighted 55/45, which places 60/40 (the wider tall pine loses more overlaps) |
| What gets added later | More structures and placement entries by data; new conditions as named fields in the validator | the file is a list; `gen.mjs` validates it with named errors |

**How it works** (`dwell-worldgen` `placements.rs`; ARCHITECTURE.md §6.3 step 5). One candidate
per grid cell, at a hashed point in it; kept when its roll is under the density × the grove factor
× the height's rise, and the ground takes it: the anchor column one of `on`, dry, in the band, its
height field's gradient over ±2 m at most tan 44°, and every column under the turned structure one
of `on` and dry. Of two kept candidates whose boxes come within the spacing, the one with the lower
priority hash is dropped, whether or not the higher one survives — so the answer needs only the
neighbours, and a chunk finds every tree reaching into it from the cells around it (the chunk plus
three reaches and the spacing), the same whichever chunk generates first. The tree's voxels go over
air and replaceable cells only (tall grass), never into the ground or water. The sky test leaves
room over the highest ground for the tallest tree; the spawn's level patch is clear of trees.

**Measured** (seed 7 and others; `tests/placements.rs`): about 55 % of the candidates the data's
chances keep survive the slope, footprint and overlap tests; in groves ~0.28 trees per 49 m² cell
(about 56 a hectare), outside them ~0.03 (about 7); ground at or above 120 m holds about twice the
trees per cell of ground below 60 m; 40 % of the trees are tall pines, each quarter turn 25 ± 1 %.

**LOD.** At level 1 (2 m cells) the sections draw the chunks' own trees (`trees_in`, exactly) from
their voxels, one sample column per cell; at coarser levels a share of the columns as large as the
trees' cover there (the chance × the mean footprint over a grid cell × the 55 % kept) shows the
canopy's block (leaves) on its top cell — a tint, not trees. The tint is faint from 600 m: the
prototype leaves are only a little darker than the grass.

**Persistence.** The trees are plain voxels of the generated chunk, so a chunk is still a pure
function of the seed; an edited chunk is saved whole as before, and a chopped tree stays chopped.
This deviates from TREE_GROWTH.md §5a's tree layer (tombstones, `growers`), which is for trees that
grow: these don't. When G3 builds that layer, procedural trees move into it (their id is already
`(placement, grid cell)`), with a new generator version.

**Cost** (native, release, seed 7): ground chunks 0.46 → 1.01 ms to generate (the trees' ground is
computed for the chunk and a 15 m margin), LOD sections of levels 1–4 1.3 → 2.3 ms; around the spawn
(`--example mesh_cost`) chunk mesh vertices 1.85 M → 2.01 M (+9 %) and LOD section vertices
2.36 M → 2.77 M (+17 %). In the bench (Chrome, Apple GPU, unthrottled; a shared, busy machine, so
one noisy run each) `idle-spawn` and `fly-low-fast` stayed within their run-to-run noise
(`gpu.frame` 13.3 / 14.4 ms with trees against 13.8 / 17.2 ms without).

**Half-height voxels.** On ½ m cells (Phase 41), a 1 m structure maps onto ½ m cells
by doubling each layer vertically (each 1 m block becomes two ½ m cells, x and z unchanged), so the
trees keep their real size; recorded with that port.

## 4. Mountains, rivers and lakes: the world map (generator version 21) **[built]**

The owner (2026-10-21): "use that work in progress terrain generator with mountains, elevation,
rivers, etc … in game", mountains "up to 8000ft", snowy grass and snowy leaves. [ADR
0049](./adr/0049-world-map-terrain.md); ARCHITECTURE.md §6.3 (*The world map*). The hydrology
prototype's map (proposal layers 0–1) is the terrain: per face, the base field on a 100 m grid,
uplifted into ranges and eroded, its lakes and rivers found. Columns take its height (bicubic),
fine detail (`terrain.json` `detail`), river channels and valleys (`rivers`, `mouths`), flat lakes
(`lakes`) and their surface bands (`bands`: snow line 1,550 m ± 150 m, raised on slopes; sand to
2 m above sea and under water; rock where the landform is steep — generator 24, owner 2026-10-26:
"no random splotchy stone … structured random systems rooted in realism": the slope over ±
`rockWindow` 150 m, never less than an LOD cell's width, so rock comes as whole faces, not as
splotches along the world map's 100 m grid; against a threshold easing from `rockSlope` 0.85 at sea
level to `rockSlopeHigh` 0.55 at the snow line, as soil thins with height. The soil mantles the
slopes: under a grass or snow top the dirt reaches at least down to the lowest neighbour's top, so a
terrace's riser, in chunks and in the LOD's taller cells, shows soil, not stone). Rivers (generator
25, owner 2026-10-27: banks "1-2 block high … block off access to the ocean", "tall edges … where
rivers meet"): where the land beside a river is lower than its water a levee holds it `bank` ½ m
over the water and falls away `leveeFall` 0.15 m per m, a broad bank rather than a ridge, and none
where the river is at sea level (its water is the sea's, so a mouth opens into the sea). A river
joining another meets it at the other's level `joinMargin` 3 m plus its own half-width outside the
other's channel (the other's water backing up into it where it lies lower) and falls to it at most
`joinSlope` 0.25 m per m upstream (a short gorge where it hangs above), so the waters never stand
side by side at two levels. A valley is carved by every channel whose valley reaches the column,
the lowest (generator 26, docs/BUGS.md 14: carved toward the nearest channel alone, the ground
stepped by the two rivers' difference in level where the nearest changed, a wall up the hills
between two valleys). Planned (owner, the same day): rivers filling the lower land beside them
into wider stretches, lakes and pools instead of levees, FUTURE.md. The pines grow to 1,300 m on grass and snowy
grass, their leaves snowy from 1,050–1,250 m up (`placements.json` `variants`); meadows to 1,500 m.
Spawn: level dry lowland (3–80 m) within 400 m of a river, nearest the disc's centre. Measured
(seeds 1, 7, 42, 1234, both faces): highest ground 2,210–2,650 m, the largest river 11–21 m³/s;
both faces' maps 1.1–1.2 s natively; a column ~0.2 µs, a chunk 0.5–1.7 ms; a cold browser join
(maps built) 6.7 s to playing on an M1 under load 15. Deferred: moving water (the flow direction
is kept per cell), per-world settings, delta and fall polish.

## 3. Bedrock and portals (generator version 20) **[built]**

The owner (2026-10-21): bedrock back between the two faces, and "a few random portals to the other
side … holes through the bed rock leading up to the surface on both sides". The bedrock's block and
thickness and the portals' count, spacing, siting, radius, rim, ladder and clear ground are data,
`shared/worldgen/midplane.json` (validated by `gen.mjs`, generated into `dwell-worldgen`
`midplane.gen.rs`); the engine is `portals.rs`. The design, the numbers and the as-built details
are in [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md) §12 and ARCHITECTURE.md §6.3 (*Bedrock and
portals*); [ADR 0048](./adr/0048-bedrock-and-portals.md). Prototype content (ARCHITECTURE.md §6.1).

## 5. Terrain volumes: the cliffs (the terrain's default since 0.35.0) **[built]**

The proposal's layer 2 ([`proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md`](./proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md)
§3, §7): a 3D density per cell over the height field, first as an opt-in world type to iterate on
(0.31.4–0.34.0, generator 101), the terrain's default since 0.35.0 (owner, 2026-10-27: "Let's make
cliffs the default (replace terrain) but ensure it's configurable, we'll implement some kind of
biomes later on and this will most likely be a slider"): `volume.json` `terrain` names the volume
(null for the height field alone), and each volume's `strength` (0–1, the `strength` input its
density multiplies its shaping by) is the slider; at 0 the terrain is the height field cell for cell
(tested). The
owner's answers (2026-10-27): **an opt-in world type** ("Cliffs (prototype)" in New world and Play
advanced; the default terrain is unchanged); **cliffs and overhangs only** (no caves or floating
islands yet); **a node graph in JSON**; **the LOD draws the height field's top surface only** for
the prototype — a recorded deviation: "We definitely would want the full under area rendered not
just the top surface in the final version, but for the prototype let's render the top surface
only." Shipped to iterate on (owner: "Deploy as a release and we'll iterate on it").

- **Data** (`shared/worldgen/volume.json`, generated into `dwell-worldgen` `volume.gen.rs` by
  `shared/worldgen/gen.mjs`, validated, CI stale check): named volumes, each a `density` tree over a
  fixed vocabulary — constants, `depth` (below the height field's surface), `height` (above sea
  level), `surface` (the height field's surface above sea level, per column), `terrace` (a value
  stepped into uneven levels, as an offset: **[0.38.0]**), `slope` (the surface's gradient over a radius), 3D `noise` (wavelength, octaves, vertical
  stretch, stream), `add`, `mul`, `min`, `max`, `abs`, `clamp`, `smoothstep` — with a `reach` (only
  cells that near the surface are evaluated) and the `material` added cells take. Solid where the
  density is above 0.
- **The order: shape first, then the surface** **[0.33.0]** (owner, 2026-10-27: "this should be
  applied before grass/dirt and trees … Grass/dirt wouldn't apply inside of caves (overhung land)"):
  1. *Shape.* A column's top is the reshaped ground's topmost solid cell (`terrain.rs` `column` →
     `shaped`: the density scanned within `reach` of the height field), so everything that stands on
     a column stands on the final surface.
  2. *Surface.* The column is filled from that top as from the height field: the band's top block
     (grass, snow, sand or rock), at most `soilDepth` m of soil under it (never less than the
     terrain's `underDepth`, so a cliff face shows rock), stone below; ramps and decorations from
     the final tops.
  3. *Carve* (`carve_volume`). Under the top, the cells the density leaves empty become air, or
     water under the column's own water (never in a dry column). A floor left under rock is the top
     block where open sky lies within `floorGrass` m (3 m: an overhang's mouth; owner: "floors near
     opening get grass"), else the volume's `material` (stone), as is a ceiling.
  4. *Trees* stand on the final tops (only over air, so their trunks' base is always on solid
     ground: owner, "as long as the trunk base is on a solid block that's fine").
  5. *The LOD* samples the same columns, so it draws the final top surface (owner: "switch to final
     top surface"), and **[0.35.1]** its sections are carved by the same density: each cell under a
     column's top samples it at its middle row, empty where the chunks are, a ceiling over carved air
     rock, so an overhang's underside shows from afar (owner: "did we work in rendering undersides in
     cliffs LODs? If not that should be the priority"); a section is buried only below the volume's
     reach under every top (`lod_bounds_with`), and the soil is capped on cliff faces as in chunks, so
     far walls read as rock. Tested: round seed 7's hillsides sections at levels 1–3 hold hundreds of
     rock-over-air cells (none on 0.35.0). A wall or overhang thinner than a level's cell is lost at
     that level.
- **Cost:** a column whose density is the height field's alone (the 600 m field or the slope mask
  zero; the mask is listed first so the slope is measured only where it can matter) skips the scan;
  reshaped columns are kept per thread (`SHAPED`, a cache keyed by generator). **[0.38.1]** (owner,
  2026-10-27: "Go ahead and work on optimizing cliffs speed"), the same cells, bit for bit (the
  goldens unchanged): a reshaped column keeps which of its rows within the reach are solid, worked
  out once in one pass, so the carving and the LOD read bits instead of evaluating again (the
  carving rebuilt each column's slope and evaluated each row up to three times); the cells run a
  flat list of the column's per-cell nodes over its column values (`ColumnEval::program`) instead
  of walking the tree from the root, with no allocation per node; a per-cell noise runs down its
  column (`noise::ColumnFbm3`: each lattice cell's corner hashes and x/z gradient parts kept, tested
  bit for bit against `fbm3`); the generator shares identical subtrees (the cliffs: 50 nodes → 34);
  the slope's height-field columns are kept per thread too (`BASE`); and the caches hash cheaply.
  Measured natively, 867 chunks round seed 7's terraced hillside (~1430, 6830): 1.77 s → 0.95 s
  (plain terrain 0.40 s); round seed 0's spawn: 2.32 s → 1.02 s (0.80 s). In Chrome on the GPU, the
  seed-0 spawn view settled in 17.3–19.3 s (20.3–22.2 s before), three runs each. A coarse lattice
  for the 3D noise would change the terrain (a new compatibility line).
- **Off the water** **[0.35.0]** (owner, 2026-10-27, on 0.34.0: "big edges rising up on sides of
  river, and drop off edges without filling low areas"; docs/BUGS.md 15): a river valley's banks are
  steep, so the slope mask stood walls along them, raised over the water or sunk under it with the
  water open beside them. Water columns (rivers, lakes, the sea) are never reshaped; dry ground is
  never lowered or carved under the sea's surface; and the cliffs fade in from 8 to 30 m from a
  river channel's edge or a lake's shore (the `water` input, the column's `shore`). Waterfalls,
  where a river would fall over a cliff on purpose, wait for moving water (FUTURE.md).
- **The cliffs volume** **[0.38.0: terraces]** (owner, 2026-10-27, on 0.37.x: "they kind of look like
  a huge worm dug around the surface of the terrain or like an ant farm from the side … Redesign to
  look more like real cliffs"). 0.34.0–0.37.x stood a wall wherever a 2D noise crossed zero and
  lifted one side 14 m and sank the other 14 m: the zero lines wound across the hill in any
  direction, so the sunk sides were trenches and pits (56–64% of the walls on six of seed 7's
  hillsides had the uphill side high, about chance), and the 3D lean undercut them. The terraces
  step the height field's surface instead (the `terrace` node over the `surface` input), so the
  walls follow the hill's contours and a hillside only ever steps down. The owner's answers:
  **both forms, by region** (a 900 m field, leaning to the second in the mountains): stepped
  country, levels about 7 m apart (±60% per level: walls about 3–10 m), flat on top; and
  escarpments, levels about 22 m apart (±35%: walls about 10–21 m), whose treads keep 30% of the
  hill's slope; **walls of varied 4–20 m**; **a few slight overhangs** (a 24 m 3D noise moves a
  wall ±0.7 m with height: a lip, never air under rock in a column); **on steep ground, more in the
  mountains** (slope 0.3–0.7 over ±4 m, in the 600 m field's part of the land or anywhere 250–900 m
  up; off the water as before). The levels are warped ±3 m by a 90 m noise so they wander, and
  each level's thickness is hashed per level, like strata. Reach 14 m.
- **Tests** (`tests/volume.rs`, both faces): **[0.38.0]** every 4 m+ wall has its uphill side high on six
  steep hillsides (`cliffs_step_down_the_hill`, red on 0.37's data, kept as the fixture `OVERHANGS`,
  which also keeps the LOD's underside test on big overhangs); the terrace is monotone with uneven
  levels (`volume.rs` unit tests); overhangs, added and removed cells on steep ground over
  six hillsides; gentle ground is the plain terrain cell for cell; no decoration over a recess; a
  recess in dry ground is air (red on the first build: 210,834 water cells in dry ground on face A);
  no grass buried in rock, reshaped tops grassed like the land, trunks on solid ground, no soil
  deeper than `soilDepth`, walls not tunnels (no column with two gaps of air: red on 0.33.0's shape) (all four red on 0.32.0's order: 17,329 buried grass cells and 27 of 48
  trunks over air on face A). Checked in Chrome at seed 7 near the spawn (~(1840, 7015)), near and
  from 250 m up.
- **Known gaps, to iterate on:** a 3 m soil band shows under each grass lip on a cliff face (the
  terrain's soil depth; `soilDepth` can't go below `underDepth`); the LOD has no undersides;
  overhangs have no ramps under them; caves and floating islands are not started.
