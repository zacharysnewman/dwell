# 0060. Water as a simulated surface grid, and generated water filled by it

- Status: Accepted (the owner, 2026-11-05: the direction, "run whole world", "No beds, water finds its
  way", "Both", every setting as data, creation "preferably under a minute", "Good to go on storage");
  planned, Phase 53
- Date: 2026-11-05
- Design: [`WATER_SURFACE_GRID.md`](../WATER_SURFACE_GRID.md); evidence in
  [`proposals/WATER_MODEL_EVALUATION.md`](../proposals/WATER_MODEL_EVALUATION.md) §5
- Would supersede: ADR 0053 (basins joined by outlets), ADR 0057 (balanced rivers), ADR 0059 (the
  pool graph); would amend ADR 0049 (the world map's terrain) and ADR 0058 (visible water)

## Context

The game's water is basins with one level joined by outlets (ADR 0053), rivers as chains of pools
whose levels and lips the generator draws so that the engine's weir law balances them (ADR 0057,
0059). The water is drawn, then the engine infers what it is doing from the drawing; most water
bugs from 0.42 to 0.48 lived in that inference. A dam, a diversion or a flood is handled by
machinery added for each case.

The evaluation (`WATER_MODEL_EVALUATION.md` §5) prototyped the alternatives: a priority-flood steady
solve, a falling-sand cellular automaton, a pool graph, and a surface grid (a depth per 1 m column,
discharge across each face by the local-inertial law). The grid handled every case asked of it
(dams, diversions, floods, player water) by one law, and with sleeping chunks, a visible-noise band,
smooth waking, pools for still water and held sea outlets it ran on the game's own terrain at 3–22×
real time on a laptop, 99.9% of levels within a few cm of stepping everything. On the game's terrain
the generator's drawn water was not at rest under that law (it drained ~115,000 m³ of surplus in the
first 150 s on one face).

The owner (2026-11-05): keep the map-scale erosion as the carver; let the water system decide where
water stands, at generation and in play; carving at block scale later, as its own system.

## Decision

- **One water model, simulated:** water is a depth per 1 m column; each face between columns
  carries a discharge by the local-inertial shallow-water law (θ-weighting, friction from the whole
  speed, the critical limit, the outflow limiter), stepped in parallel over 16 × 16 chunks. A chunk
  whose levels hold within a band you can't see on ½ m blocks (5 cm) sleeps and costs nothing; it
  wakes smoothly (what it owes comes in over seconds, by depth) when its neighbours' flow drifts or
  an edit or flood front reaches it. Still water (lakes, ponds, a dam's lake once full) is a pool:
  one level from its volume, never stepped. The sea is a held level. Water is conserved exactly.
- **Generation fills the water by the same rules:** the map-scale hydrology (uplift, stream-power
  erosion, fill-and-spill lakes, rain routed to rivers; ADR 0049) stays the carver of valleys. The
  block-scale generator stops drawing river beds, water levels and lips (the owner: "no beds, water
  finds its way"). At world creation, a **whole-world fill** (the owner: run it on the whole world,
  so rivers and lakes are there at first start): fill-and-spill on the block-scale ground (a priority
  flood and discharge routing, the steady solve) from the sources, then a short grid settle so the
  stored water is at rest under the law that runs in play. The result is stored with the world.
- **Sources:** the map's river flows first (springs where rivers rise and where they grow; the sea a
  held level); rain everywhere as a later layer (the owner: "both").
- **Data, not constants:** the band and sleep, the flow physics, pools, and sources are data
  (`shared/water/*.json`), generated for the Rust crates with validation, per world where it matters
  (WorldSettings, ADR 0054).
- **Carving later:** block-scale erosion by the same flow is a future system; until then the only
  carving is the map's.
- **Water is its own layer** (the owner: separate water and land chunks): depth and flows per column
  in water chunks, simulated, saved, sent and drawn apart from the blocks; block edits change the
  ground under it and wake it, water never changes blocks. One surface per column; caves and
  overhangs stay dry for now (the owner: deferred).
- **Storage:** the generated water (the fill, depths and flows) is recomputed from the seed on load;
  only chunks edited, or whose water differs from the fill by more than the band, are saved.
- **Creation:** under a minute (the owner's goal), its stages shown as they run, visually if possible;
  no long spin-up: the fill places the water in one pass and the runtime law finishes the settle.

## Consequences

- **Breaking:** the terrain a seed generates (no drawn beds, levels or lips) and the saved world (a
  water layer) change: a new compatibility line.
- **Rivers without beds** run where the map's valleys put them, as wide shallow sheets on smooth
  valley floors, until carving exists. Accepted by the owner; to be judged in the game.
- **World creation takes longer:** a whole-world fill and settle (to be measured; the steady solve
  is cheap, the settle is minutes natively today). Local worlds are created in the browser (WASM),
  so the fill must be fast enough there, or run once per seed and cached.
- **Determinism (ADR 0010):** the fill is part of generation, so it must be bit-identical natively
  and in WASM: a fixed order, no parallel float reductions whose order varies.
- **Removed:** basins, outlets, the pool graph, virtual pools, the generator's lip plans and chutes,
  and the engine's inference from blocks; the water record and protocol messages change.
- **Network:** clients receive awake chunks' water, not the whole world's.
- **Open before building:** the prototype's last issues at world scale (the awake count climbing,
  ~330 columns over 10 cm on the whole face) are settled in the prototype first, and rivers without
  beds are shown to the owner in a prototype before G1 ships (Phase 53 G0).
