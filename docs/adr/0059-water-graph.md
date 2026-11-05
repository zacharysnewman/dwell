# 0059. Water as data: the pool graph

- Superseded by [ADR 0060](0060-water-surface-grid.md): the basins were removed in 0.48.2, the river lips and the pool graph in 0.49.0.
- Status: Accepted (the owner, 2026-11-04: "Go for it, implement G1-3"); built G1–G3 (0.48.0);
  amends ADR 0057 and 0058
- Date: 2026-11-04
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md) §19 (from the proposal `WATER_GRAPH.md`)

## Context

Water was blocks first and data second: the generator wrote river and lake water as blocks, and the
engine rebuilt pools by flood-filling those blocks when an edit woke them, then inferred from the
blocks what each pool had been doing: what came in, what went out, what held it. Every water bug from
0.42 to 0.47 lived in that inference, and each fix added a mechanism nobody could see: feeds,
links, seeps, hidden walls, fronts. The owner asked what makes rivers hard and fragile, then for a
data-based design.

## Decision

- **The pool graph** (`dwell-worldgen` `pool_graph.rs`): per face, built from the world map's
  courses per river on demand (deterministic; ~0.25 s a face). A river is pools one row apart
  joined by lips (each with its crest cells, from the lip plans), its head a source, a junction a
  lip into the river it joins. Steps closer than a row per 2.5 m are a **chute** (`RiverRows`, read
  by both the terrain and the graph): no pools, a dry bed with a running stream, one lip from the
  pool above to the pool at its foot.
- **The terrain is carved from it**: lips, chutes and junction lips as the graph plans them.
- **The engine reads it** (`Rivers::pool`/`inflows`/`lip`/`graph_pool`): a woken generated pool's
  inflow is the graph's lips into it and its river's head; its lips out of sight are ways out
  from the graph. Pools nobody measured are **virtual**: implicit at rest, and stepped by the one
  law when something changes them, their lips feeding the pools below. That replaces fronts, seeps
  and catch-up for rivers. A virtual pool is measured when its drawn level moves in a loaded chunk.
- **No hidden walls**: `rest`/`held` are gone. Water under another body's water joins it, and lower
  water nobody holds yet is a way out.
- **One model for all water** (the owner: "player made water would just be another graph or part
  of the graph … Wouldn't it need to be to be consistent?"): pools edits make (a spill's puddle, a
  flooded pond, a dam's lake) are measured graph nodes in the same engine; flood-filling the blocks
  only measures a node's shape.

## Consequences

- The terrain changes (chutes): a new compatibility line (0.48.0). The water record is version 5;
  earlier records belong to earlier lines and are not read.
- Kept from before: the opening law, the pool engine's measuring (flood fill, splits, merges),
  catch-up for measured pools out of reach, and the block-seen feeds, used only where no graph
  covers a body (a world without a map, the tests).
- Open: a junction at a chute's top can leave a lip a cell short (BUGS 28; 1 lip in 44,401); a
  graph inspector page (G1's tool) not built yet; G4–G6 (edits as graph changes, floods, currents).
