# 0022. Climate with continent biases and rain shadows, the biome table, wetland ponds and colourful vegetation

- Status: Accepted
- Date: 2026-10-07
- Amends: [0019](0019-continental-scale-climate.md) decisions 2–3 (the snow line's numbers; the mountains
  biome from relief is replaced by altitude bands)
- Builds on: [0017](0017-continents-from-voronoi-plates.md) (the continents' records),
  [0018](0018-drainage-consistent-terrain.md) (rivers and lakes), [0021](0021-mountain-detail-cascade.md)

## Context

Phase 11c of `WORLD_GENERATION.md` (§3.5–3.7): the climate was one noise pair with a lapse rate and
a snow line; biomes were seven fixed classes chosen in code. The continents already carry a record
(`ContinentRecord`: a temperature bias, a humidity bias, a prevailing wind) that nothing read, and the
reference image's character comes from vegetation colour — accent trees in clumps — which needs new
blocks.

## Decision

1. **Climate.** *Temperature* is the continental noise plus the continent's bias (±10 °C × 0.55, so
   a hot continent and a cold one differ by about 0.5 in field units) plus a land offset of +0.12;
   the ground's is that less the lapse rate. *Humidity* is the noise plus the continent's bias plus
   nearness to the sea (`1 / (1 + coast / 300 km)`) less a **rain shadow**: the highest of the
   smooth ground heights 20, 60 and 150 km *upwind* (against the continent's wind; weights 1,
   0.85, 0.65) over the ground here, from the valley floor's rise, the uplift belts and two octaves
   of the ranges — ≥ 150 m higher starts a shadow, 2 km higher is a full one (−0.9 humidity). It
   is evaluated on a lattice (8 km; at the level of detail 16 cells, to cost 1–2 % of generation)
   and interpolated, cached per thread — a pure function of the seed and the lattice point.
   A small noise (± 0.012, 37 and 53 m) roughens the borders between biomes.
2. **The biome table (`biomes.h`, `biomes.cpp`) is data.** Selection: snowfield below −0.45 ground
   temperature at any height; on ground ≥ 250 m, alpine meadow below the tree line (−0.28), bare
   rock below −0.36; otherwise 11 rectangles of a temperature × humidity diagram, first match
   (autumn woods, wetland, tundra, conifer forest, dunes, savanna, blossom grove, broadleaf forest,
   meadow). The terrain then overrides: beach at the shore, sea cliff within 400 m of the coast and
   above 12 m, and riverbank or lake shore on the dry ground within 3 m of a channel's or lake's
   water. In the sea: ocean, deep ocean (< −300 m), frozen ocean (colder than −0.45). 19 biomes. Each
   row names its surface layers, steep-ground rule, tree chance and kinds, accent foliage, boulder
   chance and whether a distant forest draws a canopy; the real world style replaces rows, not code.
3. **The mountains biome (ADR 0019 decision 3) is gone.** Relief is not a biome: a mountainside passes
   through the zones by its temperature — forest to the tree line, then the alpine bands. The
   terrain's `mountain` weight (spawn avoidance, overhangs) is unchanged.
4. **Wetland ponds** are small lakes (5–16 m, 1.2–2.6 m deep, one in two 128 m cells) where the
   centre of the cell is wetland-humid (≥ 0.55), inland, and at most 8 m above the valley floor:
   a bowl, and a 0.6 m berm. The surface is the integer height of the valley floor at the centre,
   and the ground is never below the valley floor, so a pond cannot spill; it is wholly inside its
   cell (the site lies 24 m in), so only the point's own cell is read.
5. **Colourful vegetation.** Seven blocks: `leaves_bright`, `leaves_autumn`, `leaves_red`,
   `leaves_blossom`, `leaves_violet`, and the grass variants `grass_meadow` (flecks of flowers) and
   `grass_golden`, the grasses with slope families like `grass`. Two tree kinds: the blossom tree
   (a short trunk, a wide round crown) and the autumn tree (a broadleaf with autumn leaves). A
   **grove** noise (~320 m) is the share of a biome's trees that are accents, a second slow noise
   (~640 m) picks the colour of the patch, and each tree's hash decides: accents come in clumps of
   one colour, 5–12 % of the trees overall. **A distant forest keeps its colour**: above the 4 m
   cells real trees are drawn in, a forested column's surface is the canopy leaf its grove gives
   there, dithered by a hash per cell.
6. **Generator version 10.** Goldens, the storage golden world files and the block vectors regenerate
   (block ids moved: the new blocks sit among the explicit ones).

## Consequences

- **A breaking change to the terrain a seed generates and to block ids**, so a new compatibility
  line; the owner raises `package.json` (`CLAUDE.md`).
- Over 8 seeds, the shares of the land: meadow 14–19 %, savanna 8–13 %, snowfield 11–21 %, dunes
  4–14 %, wetland 6–11 %, tundra 6–11 %, broadleaf 5–10 %, blossom grove 6–8 %, autumn woods 5–8 %,
  conifer 4–8 %, riverbank 1.5–2 %, alpine meadow and bare rock about 1.5 % each.
- Chunk generation costs ~23 % more than 11b's, LOD sections 11–25 % (`bench`, near the spawn).
- Reversal: replace the zone table and the rows; the climate fields stay.
