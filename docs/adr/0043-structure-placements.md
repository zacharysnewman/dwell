# 0043. Structure placements in world generation, as data

- Status: Accepted (the owner's ask and answers, 2026-10-20: [`WORLD_GENERATION.md`](../WORLD_GENERATION.md) §2)
- Revisits: [0025](0025-baseline-world.md)'s baseline decision **B10** (no special terrain
  features) — **reversed further, for committed structures**: world generation stands trees from
  `shared/structures`; [0036](0036-render-modes-and-cross-geometry.md) had reversed it for plants
- Amends: [0031](0031-data-driven-structures.md) (worldgen placement, step S4, was deferred until
  after Phase 17) and [0033](0033-tree-growth.md) (procedural trees as a layer composed at load)
- Date: 2026-10-20
- Design: [`WORLD_GENERATION.md`](../WORLD_GENERATION.md) §2, [`STRUCTURES.md`](../STRUCTURES.md) §8

## Context

The owner (2026-10-20): generate the two hand-built pines (`dwell:pine_tree`, `dwell:tall_pine_tree`,
committed structures of the format of ADR 0031) with the world, in scattered groves, more on higher
ground, a random quarter turn and a 60/40 mix, built as an extendable "structure placement" layer.
ADR 0031 deferred placement until Phase 17's terrain definitions exist; ADR 0033 planned procedural
trees as a layer composed at chunk load (tombstones for chopped trees, `growers` for planted ones)
so that growing trees stay out of the chunk voxels.

## Options considered

- **Where the rules live**: in Phase 17's terrain definitions (don't exist yet), in the structure
  files (one structure could then not serve several rules), or **a data file beside the decorations**
  (`shared/worldgen/placements.json`; chosen: the interim home, as `decorations.json` is, one list
  to move into the definitions).
- **How trees reach the world**: the tree layer of ADR 0033 (records, tombstones, a protocol and
  world-file change), or **plain voxels of the generated chunk** (chosen: these trees don't grow, so
  a chunk stays a pure function of the seed; an edited chunk is saved whole as before, so a chopped
  tree stays chopped with no new table or message).
- **Spacing**: a plain jittered grid (trees could overlap or the grid shows), a global Poisson
  sampling (order-dependent), or **a hashed candidate per grid cell with a local priority rule**
  (chosen: of two candidates whose boxes come within the spacing, the lower priority hash is
  dropped whether or not the higher one survives, so a chunk decides from its neighbours alone, in
  any order).

## Decision

- `shared/worldgen/placements.json`: per entry the structures by name with weights, the ground
  blocks (`on`), the footprint rule (`on`: every column under the structure; `anchor`), the grid
  (m), the density, the grove noise (wavelength, coverage, the share kept outside), the height band
  (m above sea level) and its rise, the slope limit (degrees), the spacing (blocks) and the
  rotations. `shared/worldgen/gen.mjs` validates it (named errors; the structures by
  `shared/structures`' own validator) and emits `placements.gen.rs`, stale-checked in CI.
- `dwell-worldgen` `placements.rs` is the engine: a small fixed vocabulary (grid candidates, the
  rolls, the footprint, the slope by central differences, the priority rule). The terrain generator
  stamps the trees reaching a chunk over air and replaceable cells; the sky test leaves room for
  them; the spawn's level patch is clear of them.
- The LOD draws the trees at level 1 (2 m cells) from their voxels, exactly the chunks' trees, and at
  coarser levels tints a share of columns (the trees' cover there) with the canopy's block.
- Generator version 17; a new compatibility line (0.19.0), which the owner raises.

## Consequences

- More structures, more placement entries and new rules are data; a new rule kind (a mask, a
  cluster shape) is one name in the validator and its code in `placements.rs`.
- Trees are seed terrain: when tree growth (ADR 0033 G3) arrives, procedural trees move into its
  layer (their id is `(placement, grid cell)` already) and the generator version changes again.
- Cost (seed 7, native, release): ground chunks 0.46 → 1.01 ms to generate, LOD sections of levels 1–4
  1.3 → 2.3 ms; chunk mesh vertices around the spawn +9 %, LOD section vertices +17 %.
- Reversal: an empty `placements` list gives version 16's terrain back.
