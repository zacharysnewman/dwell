# 0079 — Tree species: trees grown from data by (species, seed, stage)

**Status:** Accepted (2026-11-11; S1 built in 0.52.58: species data, the builder, worldgen placement in
the starter world). Builds on ADR 0033 (tree growth), ADR 0043 (placements), ADR 0072 (octagonal logs),
ADR 0074 (biomes) and docs/TREE_GROWTH.md §9–§11.

## Context

Owner (2026-11-11): "trees start with small trunks, thin, same shape, but shorter, smaller leaves … it
should grow over time"; "Oak trees …, Pine trees …, and Palm trees"; "Seeds should result in different
max heights, a little more leaf variety"; block-grid palms "look pretty good", no 2×2 flat knees. The
Tree Growth prototype (`docs/prototypes/tree-growth.html`) showed species as procedural data, a tree a
pure function of (species, seed, stage). The world's trees were hand-built structures (ADR 0043).

What gets added later: more species (data), more stages or tunings (data), new crown or branch kinds
(code, a small vocabulary), growth over time in a world. Who: a developer or a later editor page
editing `shared/trees/*.json`. Fixed in code: the trunk, branch (`heavy`, `whorl`, `none`) and crown
(`lumpy`, `cone`, `fronds`) kinds. Per build: the species; per world: which biomes stand them.

## Decision

1. **Data.** `shared/trees/<name>.json`, one species per file: its blocks (log, leaf), trunk heights
   (sapling, the mature range a seed draws from, the ½ m step), trunk widths (kept for thin trunks,
   not drawn yet), a palm's lean, branches, crown, `variety` (crown scale; lightness and hue kept for
   per-tree tints, not drawn yet) and `growth` (`stages`, `daysPerStage`, `wild`: each stage's weight
   among generated trees, `variants`: seeds per placement). `shared/trees/gen.mjs` validates every
   field (a bad entry fails naming the species and field) and emits `species.gen.rs`; the fixture
   `shared/trees/fixtures/shrub.json` is a species added by data alone (Rust and node tests).
2. **One builder.** `dwell_worldgen::species::grow(species, seed, stage)` ports the prototype's
   `growTree` onto the ½ m grid: a pure function, cells relative to the trunk's foot; crown roughness
   by a per-cell hash, so a crown grows rather than reshuffles. Branch logs take the axis of their
   step (ADR 0072 squares the joins). Palms: each ½ m segment centred in a cell, the lean in
   whole-cell steps on one axis at a time; a step's knee is one horizontal log, never a 2 × 2 slab.
3. **Placement.** A placement's `structures` may name a species; the planter grows its
   `growth.variants` seeds at stages drawn by `growth.wild` (fixed per build: every chunk and client
   agrees) into ordinary `Structure`s (`Structure::from_cells`), so footprints, rotation, variants
   (snowy leaves), the LOD canopy and spacing work unchanged.
4. **Compatibility.** The default world's pines stay the hand-built structures (goldens unchanged).
   `broadleaf_groves` (preset biomes only) now stands oaks, so preset worlds' trees change; the new
   `conifer_groves` and `palm_coasts` are listed by the starter world only.
5. **Palm fronds are leaf cells** for now: `dwell:leaves` is already passable, breakable, held by logs
   and drawn with the canopy look; a `dwell:palm_frond` block drawn as the prototype's blades is a
   look change (not breaking) for later.

## Consequences

- Outstanding: growth over time (a tree's stage advanced on the world clock by `daysPerStage`,
  regenerating cells nobody edited: needs the per-tree record and the `growers` layer of ADR 0033 §5,
  not built); thin trunks (`dwell:log` has no width property; a `width` state, a narrower octagon, is
  needed for saplings); per-tree leaf tint (`variety.lightness`, `hue`); frond blades; a pine needle
  block; climate-fitted palms (a warm-coast vegetation kind).
- Each species is `variants` structures in memory per placement; generated trees repeat across a world
  (16 per species × 4 turns).
