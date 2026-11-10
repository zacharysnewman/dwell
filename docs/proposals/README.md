# Proposals

Plans for work that has been investigated but **not adopted**. Nothing here is architecture:
`docs/ARCHITECTURE.md` does not describe it and `docs/IMPLEMENTATION_PLAN.md` has no phase for it.
A proposal is adopted by answering its owner questions, writing an ADR and adding phases to the
implementation plan; the proposal is then folded into the design doc or deleted.

Open proposals:

| Proposal | What | Status |
|---|---|---|
| [`PIXEL_ART_LOOK.md`](./PIXEL_ART_LOOK.md) | The 3D pixel-art look (t3ssel8r style): a low-res frame, toon bands, a palette with ordered dither, an integer upscale; built as an opt-in preview (`?pixelart=<preset>`) to judge the look | Prototype built, off by default; open questions for the owner |
| [`WATER_MODEL_EVALUATION.md`](./WATER_MODEL_EVALUATION.md) | Pools and lips (today) vs flowing 1-D reaches vs a water surface grid with virtual pipes (Timberborn's model): recommends against reaches as the main model (a second model with seams), and a sandbox prototype of the grid before deciding | Written 2026-11-04; questions for the owner |
| [`WATER_GPU_SURFACE.md`](./WATER_GPU_SURFACE.md) | Cheaper water that stays pretty: what each water feature costs on the M1 Air (measured), cheaper-but-pretty options per costly piece, a phone tier, the water surface as a GPU heightfield (smooth, no re-meshing, an optional levels message), waterfalls like the owner's reference | Written 2026-11-07; open questions for the owner |
| [`TERRAIN_HYDROLOGY_AND_VOLUMES.md`](./TERRAIN_HYDROLOGY_AND_VOLUMES.md) | A world-scale hydrology map per seed and face (stream-power erosion, priority-flood lakes, rivers to the sea), then a 3D density function for rich caves, cliffs, arches, sea stacks, cross-face caves and floating islands | Hydrology prototype built (a viewer of images, not wired into generation); open questions for the owner |
| [`BIOME_SANDBOX.md`](./BIOME_SANDBOX.md) | A biome sandbox: sliders for the top blocks, relief, water banks and hue, trees, rocks and structures, a live preview patch, export to JSON (archipelago, Arches, Mount Rainier as biomes) | Recorded 2026-11-09; queued after the current work, questions parked until then by the owner's request |

Adopted proposals, and where they went:

| Proposal | Adopted | Now |
|---|---|---|
| Ray marching (`RAY_MARCHING.md`): a ray-marched renderer compared side by side with the mesher | 2026-10-14, [ADR 0029](../adr/0029-ray-marched-renderer.md), Phase 25; dropped at A7 (2026-10-20, [ADR 0044](../adr/0044-mesher-over-ray-marching.md)) | [`docs/RAY_MARCHED_RENDERER.md`](../RAY_MARCHED_RENDERER.md) (a note; the design is in the `archive/ray-marcher` branch) |
| Water (`WATER.md`): fluid looks as data, refraction, underwater, reflections | 2026-10-14, [ADR 0030](../adr/0030-fluid-looks-and-water-rendering.md), Phase 26 | [`docs/WATER_RENDERING.md`](../WATER_RENDERING.md) |
| Structures (`STRUCTURES.md`): a structure sandbox and a data-driven structure system | 2026-10-14, [ADR 0031](../adr/0031-data-driven-structures.md), Phase 27 | [`docs/STRUCTURES.md`](../STRUCTURES.md) |
| Tree growth (`TREE_GROWTH.md`): growing a hand-built tree from its extracted skeleton, and variations of it | 2026-10-14, [ADR 0033](../adr/0033-tree-growth.md), Phase 30 | [`docs/TREE_GROWTH.md`](../TREE_GROWTH.md) |
| Half-height voxels (`HALF_HEIGHT_VOXELS.md`): ½ m cells, stored cardinal ramps, shaped at generation | 2026-10-19, [ADR 0042](../adr/0042-half-height-cells.md), Phase 41, for a playtest on the branch `claude/half-height-voxels` | [`docs/HALF_HEIGHT_GRID.md`](../HALF_HEIGHT_GRID.md); the proposal is kept beside it as the investigation's record |
| Water flow (`WATER_FLOW.md`): basins with one level joined by outlets; damming, draining, diverting, currents | 2026-11-03, [ADR 0053](../adr/0053-water-flow.md), Phase 52 | [`docs/WATER_FLOW.md`](../WATER_FLOW.md) |
| Water as data (`WATER_GRAPH.md`): the pool graph from the world map, virtual pools, one model for all water | 2026-11-04, [ADR 0059](../adr/0059-water-graph.md), Phase 52 G1–G3 | [`docs/WATER_FLOW.md`](../WATER_FLOW.md) §19 (the proposal kept as the investigation's record) |
| Water surface grid (`WATER_SURFACE_GRID.md`): water simulated as a surface grid (sleeping chunks, pools, a held sea), its own layer; generated water filled by it at world creation, no drawn beds, levels or lips | 2026-11-05, [ADR 0060](../adr/0060-water-surface-grid.md), Phase 53 | [`docs/WATER_SURFACE_GRID.md`](../WATER_SURFACE_GRID.md) |
| Balanced rivers (`BALANCED_RIVERS.md`): generated river pools at the levels the engine's one law balances, and sloped water edges | 2026-11-04, [ADR 0057](../adr/0057-balanced-rivers.md), Phase 52 W6 | [`docs/WATER_FLOW.md`](../WATER_FLOW.md) §17 |

Numbers marked *estimate* in a proposal are extrapolated from published figures or arithmetic, not
measured on Dwell. Measure them with the bench (`docs/DEV_TOOLS.md`) before relying on them.
