# 0053. Water flow: basins with one level, joined by outlets

- Status: Accepted (the owner's answers, 2026-11-03); planned as Phase 52 (W1–W5)
- Date: 2026-11-03
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md); builds on ADR 0049 (the world map's rivers) and
  ADR 0052 (bodies floating, the water drag)

## Context

The owner (2026-11-03) wants water to flow: rivers dammed fill and flood over, a dam lets some
through, lakes drain, rivers are diverted, and the flow carries floating objects; "so we don't need
to update every voxel in real time". Water today is a static block; the world map carves rivers
with a discharge and a flow direction per 100 m cell but nothing moves.

## Options

1. **A per-cell liquid simulation** (cellular automaton or shallow water on every liquid cell).
   Local detail (waves, sloshing), but cost scales with the water, rivers and lakes are millions of
   cells, and conserving water across unloaded chunks is hard.
2. **Basins with one level, joined by outlets** (the owner's choice): a connected body of liquid is
   one basin with a level and volume; outlets pass flow by the weir law, blocks leak by their data;
   rivers are chains of pools; fill, spill, merge and split; the cells are drawn from the levels at
   row crossings. Cost follows change; no waves.
3. **Fixed river paths** with flows along them. Cheap, but a dug channel can't capture a river.

## Decision

Option 2. Liquids and the blocks' leaks are data; the engine is a fixed vocabulary (basins,
outlets, leaks, streams, runoff, sinks, currents). Water comes in as the world map's runoff and
leaves to the sea, the world's edge or (later) evaporation, so it is conserved. Basins near players
are simulated; the rest are frozen and fast-forwarded when next live. Rates are real, counted in game
time (the day/night clock's: a 20-minute day, 72× real time) × a water speed world setting, fast by default. The
sea is a reservoir at sea level: a sink, and a source for land below it. Currents act on bodies through the water drag relative to the current,
and swimmers are carried at the current's speed less their swim. Placed water is a small basin.
Water only at first; lava, lahar, jets (pressure), evaporation and weather later.

## Consequences

- A new saved-world table (basins) and protocol messages (levels, currents), and the generator's
  water state: a new compatibility line (0.42.0, the owner raises it), opened at W2.
- No flood waves or sloshing; a river's surface steps in ½ m rows. A flood wave would be a separate,
  local per-cell solve, not planned.
- Large basins' flood fills are the expensive case and are budgeted per tick.
