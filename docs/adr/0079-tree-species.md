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
   (sapling, the mature range a seed draws from, the ½ m step), trunk widths (drawn as the log's
   widths from 0.52.59), a palm's lean, branches, crown, `variety` (crown scale; lightness and hue kept for
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

- **Thin trunks (S2, 0.52.59):** `dwell:log` gained a `width` property (`4`, `3`, `2`, `1`: ¼ m steps,
  `4` the default and today's full log), named by the block's `crossSection.widths`
  (`{ property, metres }`, validated: every value mapped into (0, 1], the default 1). Each state's
  `CrossSection::width` narrows a standing log's octagon about its cell's centre (`octagon::outline_width`),
  so the mesher and the terrain collision (both `state_faces`) follow it; a narrow log joins nothing and
  keeps its sides beside a cube (they are inside its cell); caps between logs of one width hide, a
  narrower log's foot on a wider one shows the wider top as a ring. A lying log is drawn full width.
  Registry sync carries it as tag 14 (sent when below 1). Not breaking (CLAUDE.md, ADR 0063: a property
  added with its default meaning "as before"); state ids after the log moved, so the goldens were
  regenerated, and an ignored test (`golden.rs`, `chunks_hold_by_name_against_an_older_registry`)
  shows every golden chunk identical by name against 0.52.58's registry. The species builder stands
  each trunk at its stage's `trunk.width` (whole ¼ m), picking the log state by its data width.
  Not yet: felled pieces, the LOD's octagon columns and the brushes draw a narrow log full width.
- **Generation cost (0.52.59):** the species' wide crowns raised the placements' ground margin (3 ×
  the widest reach), and `trees_in` computed every column of it: the starter's tree spots took 1.3 s
  for 256 chunks against 0.85 s at 0.52.57. It now asks each column only when a candidate needs it,
  cached: 0.5 s (`starter_world.rs` `time_tree_spots`), the trees unchanged.
- Outstanding: growth over time (below); per-tree leaf tint (`variety.lightness`, `hue`: the mesher
  draws leaves by cell and does not know a cell's tree; needs a per-tree tint the chunk carries, e.g.
  the placed tree's seed in the generated chunk's tree list, read by the leaf mesher; not cheap); frond
  blades; a pine needle block; climate-fitted palms (a warm-coast vegetation kind).
- Growth over time, the plan (not built): (a) a per-tree record in the world, keyed by the tree's
  anchor, holding its species, seed, the stage it was generated or last grown at and the world time it
  reached it, written when a chunk holding the anchor is first saved (generated trees need none until
  then: their stage is the placement's draw); (b) the `growers` layer of ADR 0033 §5: on a chunk's load
  (and once a world day for loaded ones) a tree whose record is `daysPerStage` × n days old grows n
  stages, `grow(species, seed, stage)` diffed against its previous stage's cells, each new cell placed
  only where the world still holds what the old stage put there or air (a player's edit wins; a felled
  tree, its foot log gone, grows no more); (c) a saved sapling item planting a record at stage 0. Its
  tests: growth is a pure function of the record and the clock; an edited cell is never overwritten;
  both faces.
- Each species is `variants` structures in memory per placement; generated trees repeat across a world
  (16 per species × 4 turns).
