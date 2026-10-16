# Dwell — World Generation: Look

> **Status:** §1 (the art direction and colour, Phase 7) is **[built]** and stands. The rest of this
> file — continents from Voronoi plates (Phase 10), rivers, mountains, climate and biomes (Phase 11)
> and sky islands (Phase 12) — was **cut by the baseline rework** (Phase 15,
> [`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md), [ADR 0025](./adr/0025-baseline-world.md)):
> the generator is an interim one-island height field ([`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.3)
> until Phase 17 replaces it with terrain definitions (data, [ADR 0026](./adr/0026-terrain-definitions.md)),
> and Phase 12 is cancelled. The removed designs remain in the repository's history (and in their
> ADRs, 0017–0022, superseded); `ARCHITECTURE.md` §6.3 describes what is built.

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
