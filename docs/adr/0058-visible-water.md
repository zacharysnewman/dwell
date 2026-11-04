# 0058. Visible water: water moves between pools only where it is seen to

- Status: Accepted (the owner, 2026-11-04); built (0.47.0); amends ADR 0057
- Date: 2026-11-04
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md) §18

## Context

ADR 0057 made generated rivers rest by the engine's one law, but kept unseen flows to make the
totals balance: each found body took in the world map's flow at its own cells (`feeds`), found
bodies were linked so a body below counted what the body above sent it (`links`), a body with no
way out of its own passed its flow on into the ground (a seep), and cells beside a body as found
held it like walls (`rest`, `held`). The owner, 2026-11-04: "Water is invisibly filling nearby
pools, essentially the visible water doesn't seem to match what's happening or what's present in
state. Water should have to visibly connect pools, or they shouldn't be connected." Their seed
3695715780 dam showed it: the pool below the dam kept the river's full flow coming in from nowhere,
while the pool above it took in none.

## Options and the owner's answers

1. Where a river's water comes into the simulated stretch: **a visible source at its edge** (chosen),
   or that and a small unseen rain into every pool.
2. The pools below a dam: they drain away as a real river does, the bed refilling when the dam
   overflows; **standing pools** (chosen) — each drains to its lip and holds what stands below it.
3. Where a river leaves the simulated stretch: **a visible pour off its edge** (chosen), or keep the
   seep at its last pool.
4. Generated lips a cell wider or narrower than the lip above (so a pool settles a few cm off its
   drawn level once found): **exact lips in the generator** (chosen), calibrated generated lips, or
   let the pools settle.

## Decision

- A body takes in only what is seen pouring into it: water standing as generated (no body's) higher
  beside it, over it, or beside the open cells over it (the top of a fall), each cell it pours into
  taking one face's flow by the opening law (`Water::feeds_of`). Water from a found body comes by
  that body's own ways out. A feed ends when its source is found (a body's now) or gone; feeds are
  found again when a body's container changes (`refeed`). No links.
- Where a body runs into the unloaded world away from its ways out and nothing is seen coming in,
  the river comes in from there at the flow its lips are sized for (`discharge_at`).
- A body seeps only where its exit is beside the unloaded world; elsewhere it fills until it spills
  where it is seen to.
- The generator plans each lip as a whole (`terrain.rs` `LipPlan`): its band (every column within
  `lipReach` above the step and every channel column beside the river a row lower), and a crest
  chosen from the centre out that pours into exactly `lip_cells` cells of the pool below — counted
  as the engine counts ways out, one per cell poured into — trading or walling a cell where a
  diagonal step would pour into one too many. A river's lip flow is its head's discharge in whole
  cells, the same down to each river joining it, which adds its own (`channels.rs` `step_flows`);
  its channel is at least √2 × its lip cells + 4 m wide; lip columns keep flat tops.
- A found body within `foundSnapM` (4 cm) of its found level is drawn at that level: drawing only.
- Rest cells a body's water rises over are held no longer (a dam's flood floods the ramps it covers).

## Consequences

- Below a dam the river drains to standing pools until the dam spills; a pool with nothing seen
  coming in drains to its lip.
- The terrain a seed generates changes (lips, widths): a new compatibility line (0.47.0). Saved
  worlds' links are read past.
- Kept for now: `rest`/`held` for generated banks and risers the grid doesn't always show, and the
  fronts down the map's rivers through unloaded country (W2d). Open: on a steep diagonal cascade
  a lip's band runs into the next and a lip can still come out a cell off (docs/BUGS.md 27).
