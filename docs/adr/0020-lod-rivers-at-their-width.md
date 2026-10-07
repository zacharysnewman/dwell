# 0020. Level-of-detail rivers at their own width; water drawn at its level

- Status: Accepted
- Date: 2026-10-06
- Amends: [0018](0018-drainage-consistent-terrain.md) decision 7 (rivers at the level of detail)
- Builds on: [0012](0012-lod-octree.md) (the level of detail), [0017](0017-continents-from-voronoi-plates.md)
  (plate edges)

## Context

Playtests after Phase 11a: distant terrain changed shape and colour as levels switched, a wide river
seen from afar turned into a dry gully with pools up close, and distant water stood over its banks.
Measured against full detail around river centrelines (`lod: river valleys and their water`):

1. Tiers dropped whole at 16 m (streams) and 128 m (rivers) cells took their *valleys* with them —
   the relief they flatten came back, up to ~80 m along a river, and with ADR 0019 the mountains
   biome and snow, which read that relief, came back too.
2. A channel narrower than a cell was *widened* to one: a 3 m stream was 64 m of water at 64 m cells;
   the share of land drawn as water grew with the cell, to five times full detail's at 512 m cells.
3. A liquid's top was drawn at its cell's top, right only for the sea (whose level is a cell
   boundary at every level): a river at 121 m was drawn at 128 m in 16 m cells and at 512 m in
   512 m cells.
4. Coarse columns (256 m cells and wider) read the continent layout without the internal plate
   edges, so the uplift belts (Phase 11a) were missing: their surfaces averaged 55–80 m below full
   detail's, recorded at the time as a limit of dropped octaves.

## Options

- Keep widening, but draw the water at its level: fixes 3 only; the widened water still floods
  valleys from afar.
- Drop channels by a fixed rule of thumb (a cell at most twice the bed): measured to lose the great
  river and rivers whose water spreads well beyond their beds.
- Keep valleys while the cell resolves them, sample channels at their own width, and draw water at
  its level; give coarse columns their plate edges.

## Decision

The last option. `rivers::Tier::drop_cell` drops a tier (valley and channel) from 1,024 m
cells (stream) and 2,048 m (river); the great river is never dropped. Channels are not widened:
`rivers::Tier::channel_cell` stops drawing a channel at 32 m cells for the stream (a few metres of
water, otherwise scattered single cells far away) and never for the rivers. `core::LodSurface`
carries each wet column's water level (the WASM surface is 4 floats per column), and the client's
mesher draws the water's top and side faces at it. `LodLayout` evaluates the plate edges. Chunks are
unchanged (their goldens and the terrain a seed generates); the LOD goldens are regenerated.

## Consequences

- Around rivers coarse columns match full detail within a metre up to 128 m cells and a few metres
  beyond, every wet column's water level equals full detail's, and no level draws water where full
  detail is mostly dry or misses it where full detail is mostly wet.
- Narrow streams are not drawn from 32 m cells; a stream's valley still is.
- LOD generation costs 5–7 % more at levels 1–5 and 11–17 % less at levels 8–12 (`bench`).
