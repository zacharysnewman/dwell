# 0022. Climate with continent biases and rain shadows, the biome table, wetland ponds and biome-tinted vegetation

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
reference image's character comes from vegetation colour that varies by region.

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
   row names its surface layers, steep-ground rule, tree chance and shapes, grass and foliage tints, boulder
   chance and whether a distant forest draws a canopy; the real world style replaces rows, not code.
3. **The mountains biome (ADR 0019 decision 3) is gone.** Relief is not a biome: a mountainside passes
   through the zones by its temperature — forest to the tree line, then the alpine bands. The
   terrain's `mountain` weight (spawn avoidance, overhangs) is unchanged.
4. **Wetland ponds** are small lakes (5–16 m, 1.2–2.6 m deep, one in two 128 m cells) where the
   centre of the cell is wetland-humid (≥ 0.55), inland, and at most 8 m above the valley floor:
   a bowl, and a 0.6 m berm. The surface is the integer height of the valley floor at the centre,
   and the ground is never below the valley floor, so a pond cannot spill; it is wholly inside its
   cell (the site lies 24 m in), so only the point's own cell is read.
5. **Colourful vegetation is a tint, not blocks** (the owner's decision, 2026-10-07; first built as
   seven variant blocks, which are gone). `grass` and `leaves` are plain blocks, marked `tint`
   in the block data; their colour is the texture multiplied by the grass or foliage colour of the
   **biome** (`BiomeDef::grass` / `foliage`, in 1/64: meadow yellow-green, autumn woods orange
   foliage and golden grass, blossom grove pink, conifer teal, savanna olive and gold, …). Nothing is
   stored in the voxel and nothing is sent: the client asks the generator, a pure function of the
   seed, for the tint of the columns it draws. `TintGrid(cx, cz)` returns the biomes' colours on a
   16 m lattice blurred 3 × 3 — 3 × 3 points per chunk column, sharing their edges with the
   neighbours' — so colours change smoothly over borders; the level of detail's surface data carries
   each column's tint. The mesher gives each vertex of a tinted block the bilinear tint of the grid
   (merged quads blend between their corners); the texture's alpha is 1 − the share of a texel the
   tint colours (the grass top and the leaves wholly, the grass side's fringe but not its dirt),
   which the chunk shader reads (the opaque passes ignore alpha). Sections the player has modified
   have no surface data and draw untinted from afar. **Trees are not customised yet** (the owner will
   design them): every tree is as green or as tinted as its biome; the blossom tree is a shape the
   blossom grove uses. Per-tree accents and groves are not built, and a tree's colour cannot differ
   from its neighbours' — a limit of tinting by biome. A distant forest keeps its colour: above the
   4 m cells real trees are drawn in, a forested column's surface is `leaves`, tinted.
6. **Generator version 10.** Goldens regenerate; the block registry is unchanged from version 9 (the
   `tint` mark is client-side data).

## Consequences

- **A breaking change to the terrain a seed generates**, so a new compatibility line; the owner
  raises `package.json` (`CLAUDE.md`). Block ids are unchanged.
- Over 8 seeds, the shares of the land: meadow 14–19 %, savanna 8–13 %, snowfield 11–21 %, dunes
  4–14 %, wetland 6–11 %, tundra 6–11 %, broadleaf 5–10 %, blossom grove 6–8 %, autumn woods 5–8 %,
  conifer 4–8 %, riverbank 1.5–2 %, alpine meadow and bare rock about 1.5 % each.
- Tinting costs a tint grid per chunk column (25 biome lookups, cached by the client) and one more
  vertex attribute; the level of detail gains two floats per column.
- Chunk generation costs ~23 % more than 11b's, LOD sections 11–25 % (`bench`, near the spawn).
- Reversal: replace the zone table and the rows; the climate fields stay.
