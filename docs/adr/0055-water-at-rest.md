# 0055. Water found at rest, fed by the world map

- Status: Accepted (the owner, 2026-11-03: "implement the redesign now"); amends ADR 0053
- Date: 2026-11-03
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md) §2.2, §15, §16

## Context

W1 (0.42.0) found a body of water when an edit first touched it and took as its inflow what its
outlets passed just before: a stand-in for the world map's runoff, sound only if the water was in
balance by the basins' own rules. The generated world's water isn't: the terrain lays it by the map's
river surfaces, ½ m terraces every few metres, with plants, banks and the next step's water beside
it, each an outlet by the basins' rules. In the owner's world (0.42.1) a block set in any river's
bed flooded it without end or drained it; reproduced in seed 1, one pool was found with 61 m³/s
coming in and nothing going out, and a lake with 3,978 outlets and 1,059 m³/s. The owner asked for
a review of the whole system (WATER_FLOW.md §16), then for the redesign.

## Options

1. **Patch the measurement** (skip some outlets, cap inflows). Every patch met a new case: the
   generated water is not a balance to measure.
2. **Rivers as sloped channels** along the map's courses (a stage per reach from its flow), basins
   only for still water. True to a river's sloped surface, but a second engine beside the basins,
   with its own damming and diverting, and a hand-over between them.
3. **The world as found is at rest, fed by the map** (chosen): a found body's open neighbours it
   didn't fill and the leaking blocks it stands against hold it in until an edit touches them; its
   natural outlets (a fall, or the step onto the next pool) together pass the world map's flow
   through it at the level it was found at (a seep with none); natural outlets found running into
   other found bodies link them, and each tick, upstream first, a body passes the more of its map
   flow and what its links bring and takes in its map flow less what they bring.

## Decision

Option 3. One engine (the basins), rivers still chains of pools; the flows come from the world map
(`TerrainGenerator::discharge_at`: a river's discharge in its channel, a lake's outflow), never
from measuring the water. With it: merges only at one level; flow steps short enough that no basin
overshoots; ways found in rounds (bodies found on the way have theirs found the same tick) and
water in transit while a way isn't; basins wait while out of reach or while a chunk of them is
unloaded, passing on what comes in; a basin whose cells another's level changes finds its ways out
again; ramps at the water's written edge take their `flooded` state.

## Consequences

- A found river is the same river with a block in its bed; damming takes the river's flow from the
  pools below; a cut drains what was found at rest.
- The world map's flow is per column through a callback (`Water::with_runoff`), so the engine stays
  free of the generator; a world without a map (flat, sandbox) has none, and its water rests.
- No saved-world, protocol or generator change: a patch on the 0.42 line (0.42.2). The generator's
  own ramps beside its water were flooded in 0.43.0, a new line (the owner, 2026-11-03).
- W2 still owes saved basins, fast-forward and every body near players live from load.
