# 0048. Bedrock between the faces, and portals through it

- Status: Accepted (the owner's request, 2026-10-21)
- Date: 2026-10-21
- Supersedes: ADR 0023's "no crossing routes" and "no bedrock" (BIFACIAL_WORLD.md §4, §9)
- Design: [`BIFACIAL_WORLD.md`](../BIFACIAL_WORLD.md) §12; plan: Phase 48

## Context

Phase 13 decided (owner, 2026-10-05) that the bifacial world has no bedrock and no crossing
routes: a player digs through the core or goes round the rim. The owner (2026-10-21): "Perhaps for
now, let's add a few random portals to the other side and add bedrock back in between the two
faces. So the "portals" are holes through the bed rock leading up to the surface on both sides."

## Decision

- **Bedrock** (`dwell:bedrock`, the registry's existing `indestructible`) fills each face's first
  rows from the midplane (1 m each, data). No edit, region tool or brush changes it; the client
  predicts edits with the server's check, so it never shows it broken. The structural anchor stays
  the positional core zone (Phase 20), which now holds the bedrock.
- **Portals** are generated from the seed: a few (4–8) vertical shafts, sited where both faces
  have low land, spaced apart, each open from one face's ground to the other's with a ladder up its
  wall the whole way and a rim round both openings. Everything about them is data
  (`shared/worldgen/midplane.json`), validated by `shared/worldgen/gen.mjs`; the engine
  (`dwell-worldgen` `portals.rs`) knows a round shaft, a rim, a ladder and clear ground.
- **The ladder is the way through** (the crossing works on ladders since 0.24.1). The flip band
  still has no brake, so a fall into a shaft swings across; a water column or drag in a shaft are
  open alternatives.

## Consequences

- Generator version 20: the terrain of every seed changes, and the registry gains a block: a new
  compatibility line.
- Digging down now stops at the bedrock: the faces meet only at the portals and round the rim.
- Each chunk finds what portals reach it from the seed alone; the sites are searched once per seed
  (milliseconds) and cached.
