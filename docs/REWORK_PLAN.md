# Dwell — Baseline Rework Plan

> **Status: planned, not started (2026-10-12).** The owner's simplified baseline, evaluated in
> [`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md), built as five sub-phases **R0–R5** that run
> before the physics phases (14–16) of [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md). This
> file holds the rework's deliverables and exit criteria; the plan's **Progress** table carries its
> status. Tick boxes here under the same rules as the plan (`CLAUDE.md`): in the commit that
> completes them, exit criteria only once verified.

## Goal

Replace the over-built, hardcoded world with a small baseline built on **extendable systems**:

- A **bifacial disc of 50 km radius**, a **10-layer stone core per face**, a **spherical build
  volume** around the origin.
- **Simple noise terrain** with **one Voronoi island** that keeps clear of the rim — described by a
  **terrain definition** (data), not by code.
- **Natural slopes** from the Shape Playground's rules (adaptive blocks), with the rule table as data.
- **WebGPU** rendering, with block looks and textures as data.
- **Six blocks**: grass, dirt, stone, sand, water, ladder. **No biomes, no features.**

**The system rule** (owner, 2026-10-12): a part counts as done only when it exists as an extendable
system — *if you can't reasonably build a tool to add something to it, it's not an extendable
system.* Every sub-phase below that adds content does it through data with a generator, names
instead of code constants, and validation (`BASELINE_EVALUATION.md` §1.1). The baseline's content
is still prototype (`ARCHITECTURE.md` §6.1).

## Rules for every sub-phase

- **Docs in the same change.** Each sub-phase updates `ARCHITECTURE.md` (and its sub-specs) as it
  lands, removes descriptions of what it removes, and updates status tags (`CLAUDE.md`).
- **Determinism stays.** ADR 0010's rules (integer-hashed noise, integer wavelengths, `+ − × /`,
  strict IEEE) hold for everything the terrain definitions can express; goldens run natively, in
  WASM and in the client's worldgen module.
- **Tests with the change**; bug fixes found on the way get a red → green regression test.
- **One compatibility line.** R1–R4 change the terrain a seed generates, the registry and protocol
  constants: together they are a new compatibility line (`0.9.0` under today's numbering). Pushes to
  `main` meanwhile are dev builds; **only the owner raises `client/package.json`**, once R1–R4 are
  in (or earlier if they choose to ship a step on its own line). R5 changes no public API.
- **Licence notices** if a dependency changes (`python3 shared/licenses/gen.py`).

## Order

```
R0 decisions & ADRs ─► R1 cut ─► R2 world shape & build volume ─► R3 terrain definitions ─► R4 adaptive blocks
                         └───────────────────────────────────────────────────────────────► R5 WebGPU, looks & textures
```

R5 depends only on R1 (fewer blocks, no tints to port) and may run alongside R2–R4.

---

## R0 — Decisions and ADRs

**Goal:** Settle the open decisions and record the baseline's architecture before code changes.

**Status:** Not started.

Defaults (`BASELINE_EVALUATION.md` §7), used by this plan unless the owner changes them:

| # | Decision | Default |
|---|---|---|
| D1 | "10 layers of stone" | A fixed core of 10 stone layers per face from the midplane; higher ground is stone under a soil skin |
| D2 | Build sphere radius | 50 km (= the world radius) |
| D3 | Flight at the sphere | Held at the boundary; falling unaffected |
| D4 | WebGL fallback | WebGPU first; Three's WebGL2 backend only as an automatic fallback, all shaders in TSL |
| D5 | Face B's terrain | Same definition as face A, own seed (own island); selectable per face |
| D6 | Launch pads | Remove the block, `launch_speed` and its tests |
| D7 | Movement playground world | Becomes a structure file the controller tests load |
| D8 | Terrain scale | Island 20–30 km radius, ≥ 8 km sea to the rim, sea level `h` = 32, land to `h` ≈ 96–160 |
| D9 | Midplane | y = 0 |
| D10 | Terrain data scope | Fixed node kinds in code; parameters and composition in data; no scripting |

Deliverables
- [ ] The owner confirms or changes D1–D10; this table and `BASELINE_EVALUATION.md` §7 record it.
- [ ] **ADR 0025 — the baseline world**: 50 km bifacial disc, midplane at 0, 10-layer core, spherical
  build volume, six blocks, no biomes or features. Supersedes ADR 0011's scale (keeps its precision
  decisions) and ADRs 0017–0022; amends ADR 0023 (thickness, midplane) and ADR 0016 (placed slope
  families removed).
- [ ] **ADR 0026 — terrain definitions**: terrain as data (layout, height nodes, layers, spawn rule),
  generated into C++/TS, its hash identifying the generator.
- [ ] **ADR 0024** amended (rule table as data, with overrides) and accepted.
- [ ] **ADR 0027 — WebGPU renderer**: supersedes ADR 0002's backend (Three.js and the render
  interface stay); the look registry.
- [ ] `adr/README.md` index; `ARCHITECTURE.md` §12 (Open Decisions) and the plan updated: Phase 12
  cancelled, Phases 9e/10/11's outstanding items replaced by this rework.

Exit criteria
- [ ] ADRs 0024–0027 are Accepted and indexed; the decisions table has the owner's answers.

---

## R1 — Cut

**Goal:** Remove everything outside the baseline, so later steps build on a small codebase. The
terrain stays the current generator, cut down (an interim, hardcoded step that R3 replaces).

**Status:** Not started.

Deliverables
- [ ] **Blocks** (`shared/blocks/dwell.json`): keep air, grass, dirt, stone, sand, water, ladder;
  remove bedrock, launch_pad (and `launch_speed`, its controller code and scenarios, D6),
  stone_slab, every slope/slab family (and shape picking in the hotbar), sandstone, gravel, snow,
  ice, log, leaves, coal/iron/gold ore. Regenerate the tables; the registry hash changes. Keep the
  `shapeFamilies` mechanism in `gen.mjs`, unused.
- [ ] **Textures**: remove the removed blocks' tiles from `render/textures.ts`.
- [ ] **Worldgen cuts** (`server/core/worldgen`): rivers, lakes, ponds, waterfalls, `Column::water`,
  the valley floor `V` and `D` (`rivers.*`); climate, the biome table, rain shadows, snow lines
  (`biomes.*`, `SampleClimate`); the ridged cascade, belts, ranges, massifs; the 3D density and
  overhangs; caves; the stability pass; ores; boulders and trees; slope stage 5b (`slopes.*`);
  multi-continent layout, plates, island plates and the shelf/abyss profile (one island at the
  origin remains in the interim).
- [ ] **Tint pipeline**: `TintGrid`, WASM `dwell_worldgen_tint`, the worker's tint job,
  `TintField`, the `tints` vertex attribute, `TINT_MASKS` and atlas alpha, `LodSurface` tints,
  `tint` in block data.
- [ ] **Test worlds**: the flat world gets the stone core instead of bedrock (`BEDROCK_LAYERS`
  removed); the movement playground becomes a structure file loaded by the controller tests (D7),
  `kGeneratorPlayground` removed.
- [ ] **Tools**: `dwell_worldgen_inspect`'s `biomes` and `valley` modes and the biome map in F4 go
  (height map only).
- [ ] **Tests**: remove `rivers_test`, `climate_test`, `vegetation_test`, `cascade_test`,
  `slopes_test`, `slope_terrain_test`, `water_search.h`, `biome_search.h` and the controller's
  launch-pad scenario; regenerate chunk and LOD goldens; hotbar count in `e2e/edit.spec.ts` = 5.
- [ ] **Docs**: `ARCHITECTURE.md` §6.1, §6.3, §6.6, §7.4; `WORLD_GENERATION.md` cut to what remains
  (and its sky-island part removed with `docs/reference/aether-floating-islands.md`);
  `SLOPE_BLOCKS.md` trimmed to the shape mechanism; `BLOCK_REGISTRY.md` examples;
  `PLAYER_CONTROLLER.md` launch-pad text; `CLAUDE.md`'s hotbar note.

Exit criteria
- [ ] The registry holds exactly air, grass, dirt, stone, sand, water and ladder (a test lists them).
- [ ] No code references a removed block, generator stage or tint (build and grep check).
- [ ] Native, WASM and Vitest suites pass with regenerated goldens; e2e passes (known failures in
  `CLAUDE.md` aside).
- [ ] Chunk and LOD generation time measured against generator version 11 (`… bench`).

---

## R2 — World shape and build volume

**Goal:** The baseline's geometry, with every bound read from one data-driven predicate.

**Status:** Not started.

Deliverables
- [ ] **Constants** (`shared/protocol/constants.json`): `worldRadius` 50,000; `midplaneY` 0;
  `seaLevel` and the core's thickness move to the terrain definition in R3 (interim values here:
  sea level `h` = 32, core 10); `worldBottomY`/`worldMaxY` replaced by `bounds`
  `{ "shape": "sphere", "center": [0, 0, 0], "radius": 50000 }`; `lod.maxLevel` 12;
  `flightCeiling` replaced by the bounds; fewer fly-speed levels. Protocol version bumped.
- [ ] **`InsideBuildVolume`** in `core/voxel.h` and `client/src/world/`: shapes a closed set in code
  (sphere; slab kept for tests), parameters from data. Used by: edits (server rejection, client
  pre-check), generation and storage (chunks wholly outside are air, never stored; `IsAirChunk`,
  `LodBoundsAt`), streaming and LOD requests, flight (held at the boundary, D3).
- [ ] **Bifacial re-tune**: the mirror constant, `kMinChunkY`/`kMaxChunkY` from the bounds, the
  flip band and `CORE_ANCHOR_LAYERS` (8) checked against the 10-layer core.
- [ ] **Thin-crust LOD**: sections straddling the midplane at every level draw the viewer's face
  only, without holes or the other face showing through.
- [ ] Far-origin test runs moved to ~49 km; F4 zoom range narrowed.
- [ ] Docs: `ARCHITECTURE.md` world bounds, bifacial, §6.6, §7.4; `BIFACIAL_WORLD.md` geometry.

Exit criteria
- [ ] Edits outside the sphere are rejected by the server and not sent by the client (unit tests;
  one e2e edit at the boundary).
- [ ] No chunk or LOD section wholly outside the sphere is generated, stored or requested (tests).
- [ ] Flight stops at the boundary (controller scenario, native and WASM).
- [ ] LOD meshing of straddling sections at levels 1–8 shows no holes and none of the other face
  (mesher tests over a 20-layer crust).
- [ ] Walking over the rim still carries a player across the midplane to face B (controller
  scenario).
- [ ] Manual: a look in a browser from the ground, from altitude and from face B.

---

## R3 — Terrain definitions

**Goal:** Terrain as data. The interim generator from R1 is replaced by an evaluator of terrain
definitions; the baseline world is `dwell:baseline`.

**Status:** Not started.

Deliverables
- [ ] **Format** (`shared/worldgen/*.json`, schema documented in `ARCHITECTURE.md` §6.3):
  - `layout` — `voronoiIsland`: site `cell`, `warp`, `land.radius` and `radiusNoise`,
    `rimClearance`, `coastDetail`; an extensible `kind`.
  - `height` — land and sea lists of nodes: `fbm` (wavelength, octaves, gain, amplitude), the coast
    distance profile, constants; `mask` by name (`inland`, `coast`, …); sum, scale, clamp.
  - `core`, `surface` (conditioned layer lists: `underwater`, coast distance, height above sea
    level, slope), `fill`, `seaLevel`, `liquid` — all blocks by name.
  - `spawn` — a rule (e.g. nearest dry, level land to the origin).
- [ ] **Generator** `shared/worldgen/gen.mjs`: validates (block names against the registry, node
  kinds, parameter ranges, integer wavelengths), emits C++ tables and TS for tools; CI fails on
  stale output. A **definition hash** (FNV-1a 64, like the registry hash) identifies the generator:
  worlds record it beside the generator version; `Welcome` carries it.
- [ ] **Evaluator** in `server/core/worldgen`: layout → height field (4-column lattice, bilinear) →
  fill by layers; `GenerateLod` and point queries (`ColumnAt`, `GroundY`, spawn) from the same
  fields. Reuses the Voronoi coast distance from `continents.cpp`, parameterised.
- [ ] **Generators are definitions**: `GeneratorFor`'s switch replaced by a definition lookup; the
  flat world is a definition with no noise.
- [ ] **Per-face definitions** (D5): each face names a definition and a seed stream.
- [ ] **`dwell:baseline`**: one island (D8), the 10-layer core, stone under 3–4 layers of dirt and
  grass, sand at the coast and under water, sea level `h` = 32. Tuned with the inspect tool.
- [ ] **Tools**: `dwell_worldgen_inspect` takes a definition file (`--def path`) for height maps,
  disc images, `view` screenshots and `bench`. Stretch: a definition editor page with a live
  preview in `docs/prototypes/`.
- [ ] Goldens for `dwell:baseline` (surface, coast, sea, core rows on both faces, the rim, near the
  sphere's edge); the old generator code deleted.
- [ ] Docs: `ARCHITECTURE.md` §6.3 rewritten (pipeline, determinism, the format);
  `WORLD_GENERATION.md` becomes the definition guide (or is folded into §6.3).

Exit criteria
- [ ] Adding a noise layer or a surface layer is a data change only: a test loads a modified
  definition and sees the terrain change, with a different hash.
- [ ] Invalid definitions (unknown block, unknown node kind, non-integer wavelength) fail the
  generator with a message (tests).
- [ ] Land never lies within `rimClearance` of the rim (sampled over seeds); the origin is land.
- [ ] Every column has its core layers on both faces and no water voxel has air below it.
- [ ] Goldens agree natively, in WASM and in the client module.
- [ ] Generation time per chunk and per LOD section reported against R1's.
- [ ] Manual: the owner reviews whole-disc and first-person images of both faces.

---

## R4 — Adaptive natural blocks

**Goal:** The Shape Playground's slopes on all natural terrain — Phase 9e
([`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md)), simplified by the baseline (every solid block is
natural) and with the rule table as data.

**Status:** Not started. Replaces Phase 9e.

Deliverables
- [ ] **Shapes**: the 12 corner and 6 ridge shapes added to `shapes.mjs` as corner-height data;
  geometry tests (closure, winding, volume, coverage).
- [ ] **The table as data** (`shared/blocks/adaptive.json`): generated from the five rules, plus
  per-neighbourhood **overrides**; the generator checks shape names and runs the exhaustive no-holes
  adjacency test; C++ and TS readers from shared vectors.
- [ ] **The Shape Playground** exports overrides in that format (republished to the same link,
  `docs/prototypes/README.md`).
- [ ] `"adaptive": true` on grass, dirt, stone and sand (block data).
- [ ] **Meshing, collision, targeting**: derived shapes in the chunk mesher, terrain collision,
  `RayEnterShape`, `SurfaceHeightAt`; edits re-shape and re-mesh neighbours.
- [ ] **Terrain and LOD**: point queries, spawn and water read the shaped surface; the rule applied
  on LOD cells; flooded shaped blocks at shores.
- [ ] Docs: `ARCHITECTURE.md` §6.1, §6.5, §6.6; `ADAPTIVE_BLOCKS.md` reduced to rationale and tables;
  ADR 0024 as built.

Exit criteria
- [ ] The table equals the five rules plus overrides for all 6,561 neighbourhoods, in C++ and TS,
  and reproduces every block of the owner's playground export.
- [ ] No holes between any two neighbouring derived shapes at equal height (exhaustive test).
- [ ] Controller scenarios on every new shape (ridges, saddles, half-and-half corners, 45° ramps; no
  launches, hops or sliding at rest), native and WASM; the divergence check passes.
- [ ] e2e: placing and breaking natural blocks re-shapes neighbours identically on two clients.
- [ ] LOD seams against chunks checked; generation and meshing times against R3's.
- [ ] Manual: the owner walks the terrain and builds with natural blocks, comparing with the
  Adaptive Block Lab.

---

## R5 — WebGPU, looks and textures

**Goal:** Render with WebGPU, and make how blocks look a system: looks registered by name and
textures described by data.

**Status:** Not started. Needs R1 only.

Deliverables
- [ ] **CI first**: Playwright's Chromium runs WebGPU headless (flags and a software adapter) for
  e2e and `client/scripts/shots.ts`; a smoke test proves a WebGPU frame renders in CI.
- [ ] **`WebGPURenderer`** behind `client/render` (async init); the WebGL2 backend as automatic
  fallback (D4), reported in F3.
- [ ] **TSL ports**: height fog and per-face light (`heightFog.ts`), the sky (`sky.ts`), chunk and
  LOD materials, tone mapping and exposure; no GLSL left.
- [ ] **Look registry**: each look (`cube`, `shaped`, `water`, `ladder`, …) is a module registering a
  TSL material and a meshing style by name; `mesher.ts`'s branches on look names replaced by the
  registry; `gen.mjs` validates block looks against it.
- [ ] **Texture data**: tile recipes (parametric noise: palette, scales, features) or image files in
  `shared/blocks/` (or a sibling), named by block data; the atlas built from them and sized to fit;
  `TileName` and the painter functions removed. Stretch: a recipe editor page in `docs/prototypes/`.
- [ ] Re-verify batched terrain, the near/far depth passes and the LOD's draw path on WebGPU.
- [ ] Docs: `ARCHITECTURE.md` §2.1, §5; `render` interface notes; ADR 0027 as built.

Exit criteria
- [ ] e2e suites pass on WebGPU in CI, and on the WebGL2 fallback.
- [ ] A new texture or look is added by data plus (for a look) one module — a test adds a fixture
  look and texture without touching the mesher.
- [ ] Screenshots match the WebGL build's look (owner review of before/after).
- [ ] Frame time on a desktop and a phone, WebGPU against the WebGL build.
- [ ] Manual: Safari (macOS, iOS) and an Android device render with WebGPU.

---

## What does not change

Server core and threading, the player controller (minus launch pads), networking, persistence,
streaming, menus, hosting, the master server, identity, versioned releases and the launcher, and
Phases 14–17 (which wait for this rework, as they waited for Phases 6–13).
