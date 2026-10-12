# 0025. The baseline world: a 50 km bifacial disc, a 10-layer core, a spherical build volume, six blocks

- Status: Accepted
- Date: 2026-10-12
- Resolves: Phase 14 of the implementation plan (the baseline's decisions D1–D3, D5–D9,
  [`BASELINE_EVALUATION.md`](../BASELINE_EVALUATION.md) §7)
- Supersedes: [0011](0011-planet-scale-world.md) decision 1 (the 8,192 km disc and its vertical
  bounds; its precision decisions 2–6 stand); [0017](0017-continents-from-voronoi-plates.md) (the
  multi-continent plate layout; its Voronoi coast-distance algorithm and `sqrt` amendment to 0010
  are kept, parameterised by [0026](0026-terrain-definitions.md));
  [0018](0018-drainage-consistent-terrain.md), [0019](0019-continental-scale-climate.md),
  [0020](0020-lod-rivers-at-their-width.md), [0021](0021-mountain-detail-cascade.md),
  [0022](0022-climate-biome-table-vegetation.md) (rivers, climate, biomes, mountains, vegetation)
- Amends: [0023](0023-bifacial-world.md) decisions 1–3 and 8 (the midplane at 0, the crust's
  thickness, face B's terrain by definition, the level of detail over a thin crust);
  [0016](0016-slope-blocks.md) (the placed slope and slab families and the slope stage are removed;
  the shape table stays)
- Builds on: [0015](0015-block-registry.md) (the block registry), [0024](0024-adaptive-natural-blocks.md)
  (natural slopes)

## Context

Phases 9–13 built a planet-scale world (an 8,192 km disc) with continents from plates, rivers,
climate, 19 biomes, mountains, caves, ores, trees and placed slopes — most of it constants and
branches in C++, and more than the game asked for. On 2026-10-12 the owner set a smaller
**baseline** (B1–B11 in `BASELINE_EVALUATION.md` §1): a two-sided disc of 50 km radius, ten layers
of stone per face, a spherical build limit, simple noise terrain with one Voronoi island clear of
the rim, slopes as in the Shape Playground, WebGPU, no biomes, no special terrain features, and six
blocks. A part counts as done only when it is an **extendable system** — *if you can't reasonably
build a tool to add something to it, it's not an extendable system* (`BASELINE_EVALUATION.md` §1.1).

The evaluation left ten decisions open (D1–D10). The owner answered them on 2026-10-12. This ADR
records the world's shape and content (D1–D3, D5–D9); [0026](0026-terrain-definitions.md) records
how terrain is described (D10), the amended [0024](0024-adaptive-natural-blocks.md) the slopes, and
[0027](0027-webgpu-renderer.md) the renderer (D4).

## Options considered

The evaluation proposed one answer per decision and named the alternatives (§7); the owner chose:

| # | Decision | Alternatives | Chosen (owner, 2026-10-12) |
|---|---|---|---|
| D1 | What "10 layers of stone" measures | Exactly 10 stone blocks under the soil everywhere (the crust's thickness then follows the terrain); 10 distinct stone strata | **A fixed core of 10 stone layers per face from the midplane**; higher ground is stone under a soil skin |
| D2 | Build sphere radius | Smaller than the disc (the rim unreachable for building); larger | **50 km**, equal to the world radius |
| D3 | Flight at the sphere's surface | Held at the boundary; pushed back inward | **No movement limit**: the sphere bounds building, not movement |
| D5 | Face B's terrain | A mirror of face A; a flat underside | **The same definition as face A with its own seed** (its own island), selectable per face |
| D6 | Launch pads | Keep as a block | **Remove** the block, `launch_speed` and its tests |
| D7 | The movement playground world | Cut; controller tests build their own fixtures | **A structure file** the controller tests load |
| D8 | Terrain scale | A smaller island; taller land | **As proposed**: island ~20–30 km radius, ≥ 8 km of sea to the rim, sea level `h` = 32, land to `h` ≈ 96–160, tuned with the inspect tool |
| D9 | The midplane | Keep −2,048 | **y = 0**: the centre of the disc, the midplane and the build sphere |

On D3 the evaluation proposed holding flight at the boundary. The owner chose no limit: there is
nothing to build or see past the sphere, so a wall there would only get in the way, and players
flying out simply see the disc from further away.

## Decision

1. **Geometry.** The world is a **disc of radius 50,000 m** (`worldRadius`) centred on the origin,
   with **two faces** (ADR 0023) either side of the **midplane `y = 0`**. Face-local height `h` is
   measured from the midplane: on face A `h = y`; on face B `h = −1 − y` (the mirror about the
   midplane, voxel onto voxel, chunk onto chunk; the midplane stays a chunk boundary). Gravity
   points toward the midplane and the flip band carries bodies across it, as in ADR 0023.
2. **The core (D1).** Each face has a **core of 10 stone layers**, `h` 0–9, never carved by
   generation, so the disc is at least 20 layers thick everywhere. Above the core the terrain is
   stone under a skin of soil (grass over dirt, sand at the coast and under water). The core's
   thickness, its block, the skin and sea level are **data in the terrain definition**
   ([0026](0026-terrain-definitions.md)), not constants. There is no bedrock; the core is ordinary,
   diggable stone. ADR 0023's core anchor zone (`CORE_ANCHOR_LAYERS`, for structural integrity in
   Phase 20) is re-checked against the 10-layer core in Phase 16.
3. **The build volume (D2, D3).** Everything that bounds **building and storage** reads one
   predicate, `InsideBuildVolume`, driven by data in `constants.json`:
   `"bounds": { "shape": "sphere", "center": [0, 0, 0], "radius": 50000 }`. The shape is a small
   closed set in code (a sphere for the world; a slab kept for tests). It replaces `worldBottomY`,
   `worldMaxY` and the slab checks: edits outside it are rejected by the server and not sent by the
   client; chunks and LOD sections wholly outside it are air — never generated, stored or requested.
   **Movement is not bounded by it**: `flightCeiling` is removed and players may fly anywhere. The
   only bound on a position is the wire format's existing numeric range (`POS64_LIMIT`, 33,554 km,
   ADR 0011's precision decisions), which is not a gameplay limit.
4. **Terrain (D5, D8).** Each face names a **terrain definition** and a seed stream
   ([0026](0026-terrain-definitions.md)). The baseline definition, `dwell:baseline`, is noise terrain
   with **one Voronoi island** around the origin, ~20–30 km in radius, never within 8 km of the rim;
   sea level `h` = 32, sea floor at or above `h` = 12, land to `h` ≈ 96–160. Both faces use it, face A
   with the world seed and face B with `FaceSeed(seed, B)`, so face B has its own island; a world may
   name another definition for either face. These numbers are starting values tuned with the inspect
   tool in Phase 17, and like all terrain style they are prototype content (`ARCHITECTURE.md` §6.1).
5. **No biomes and no features.** Rivers, lakes, ponds, waterfalls, climate, biomes, tints, snow
   lines, ridged mountains, overhangs, caves, ores, boulders, trees and sky islands are removed, not
   kept behind a switch. New features return only as entries in a system (a node kind, a layer, a
   look), each by its own decision.
6. **Six blocks (and air).** The registry holds **air, grass, dirt, stone, sand, water and ladder**.
   Bedrock, launch_pad (with the `launch_speed` property and its controller code, D6), stone_slab,
   every slope and slab family, sandstone, gravel, snow, ice, log, leaves and the three ores are
   removed with their texture tiles. The registry's `shapeFamilies` mechanism stays, unused, so a
   future crafted block can opt in by data. Natural slopes come from adaptive blocks (ADR 0024).
   The blocks are prototype content; the registry is the system.
7. **The movement playground (D7)** becomes a **structure file** — voxels by block name, loaded by
   the controller tests — instead of a generator hand-placed in `voxel.cpp`. `kGeneratorPlayground`
   goes; the structure format is the seed of a general one (saved builds, prefabs) but only what the
   tests need is built now.
8. **Kept from ADR 0011**: double-precision Jolt, f64 and 1/256 m fixed-point wire positions,
   region-anchored terrain collision and the integer/remainder noise lattice. They work at any
   radius, are built and tested, and cost little; they are revisited only for a measured cost.
   `lod.maxLevel` drops to 12 (a 131 km root covers the disc and the sphere's useful height).

## Consequences

- The terrain every seed generates, the block registry (and so saved palettes) and protocol
  constants all change: Phases 15–18 together are a **new compatibility line** (0.9.0 under today's
  numbering), raised in `client/package.json` by the owner. Worlds of the 0.1–0.8 lines keep opening
  in their own versions through the launcher (ADR 0014).
- Most of the worldgen code goes: `rivers.*` and `biomes.*`, most of `continents.*`, the 3D density,
  caves, ores, vegetation, the slope stage and the tint pipeline, with their tests and goldens
  (Phase 15). `ARCHITECTURE.md`, `WORLD_GENERATION.md` and `SLOPE_BLOCKS.md` lose the matching
  sections as the code goes.
- The build sphere spans ~3,125 chunk rows over the centre, tapering toward the rim (~10 km of
  height at 49 km out, ~3 km at 49.9 km); nearly all of it is air, so `LodBounds` and the air-chunk
  index must stay cheap (Phase 16 measures it).
- **Risk — the level of detail over a thin crust.** With a crust of tens of metres, sections from
  level ~2 straddle the midplane. The straddling path (meshed for the viewer's face, the other face
  culled) must hold at every level without holes or the other face showing through (Phase 16).
- The rim becomes a cliff a few tens of metres tall between the two seas instead of ~4 km.
- With no flight limit, a player can fly far outside the sphere and the disc; streaming and the LOD
  must stay correct there (no requests for chunks outside the volume) and the far-origin tests move
  to ~49 km for terrain and keep a far-away case for flight.
- Reversal: the bounds shape, radius, core, island and blocks are data; changing them is a data
  change plus, for the world's terrain, a new compatibility line.
