# Dwell — Baseline Evaluation: What to Keep, Add and Cut

> **Status: evaluation, 2026-10-12.** Nothing here is built or decided yet. It compares the owner's
> new baseline with the code and docs on `main` (app 0.8.1, generator version 11, protocol v12) and
> proposes the work to get there. Once the owner settles the open decisions (§6), the agreed parts
> become ADRs, a re-plan of `IMPLEMENTATION_PLAN.md`, and changes to `ARCHITECTURE.md` in the
> changes that build them.

## 1. Why

Earlier phases added features nobody asked for in the baseline — biomes, rivers, climate, trees,
ores, sky islands, a planet-scale world. The owner wants a smaller, simpler basis to build on.
The **baseline** (owner, 2026-10-12):

| # | Requirement |
|---|---|
| B1 | Two-sided (bifacial) circular world |
| B2 | World radius 50 km (100 km across) |
| B3 | Simplified underground: 10 layers of stone per face (20 for the whole thickness) |
| B4 | Build limit: a sphere in all directions around the origin of terrain generation |
| B5 | Simple, noise-based terrain generation |
| B6 | One Voronoi-based continent (a big island at this scale) that does not touch the edge |
| B7 | Slopes connecting natural terrain, as in the Shape Playground |
| B8 | WebGPU, not WebGL, for performance and future shaders |
| B9 | No biomes |
| B10 | No special terrain features: no trees, rocks, caves, rivers, etc. |
| B11 | Blocks: grass, dirt, stone, sand, water, ladder — remove the rest |

## 2. Summary

| # | Requirement | Today | Verdict | Work |
|---|---|---|---|---|
| B1 | Bifacial disc | Built (Phase 13): mirrored face B, gravity toward the midplane, flip band, camera turn-over | **Aligned** | Keep; re-tune for a thin crust (§3.1) |
| B2 | 50 km radius | 8,192 km radius (ADR 0011) | **Over-built** | Shrink one constant, then cut what only planet scale needs (§3.2) |
| B3 | 10 stone layers per face | ~2 km of rock per face (4 km crust), midplane at y = −2,048 | **Gap** | New vertical layout (§3.3) |
| B4 | Spherical build limit | A vertical slab −10,240…6,144 plus a 24,000 km flight ceiling | **Gap** | New bound for generation, edits, streaming and flight (§3.4) |
| B5 | Simple noise terrain | 1,978-line `terrain.cpp`: valley floor, three river tiers, ridged cascade, massifs, overhangs, caves, climate | **Over-built** | Replace with a small height field (§3.5) |
| B6 | One Voronoi continent | 12–13 continents from 2,560 km cells, plates, islands, shelf/abyss | **Partial** | Keep the Voronoi + signed coast distance, drop to one island (§3.6) |
| B7 | Playground slopes | Designed and prototyped (Phase 9e, ADR 0024 proposed), **not built**; Phase 9's placed slopes and the generator's slope stage are built | **Partial** | Build 9e; remove placed slope/slab families (§3.7) |
| B8 | WebGPU | Three.js 0.186.1 `WebGLRenderer`, GLSL injected via `onBeforeCompile` (ADR 0002) | **Gap** | Move to `WebGPURenderer` + TSL behind the existing render interface (§3.8) |
| B9 | No biomes | 19-biome table, climate, rain shadows, biome tints (Phase 11c) | **Over-built** | Cut (§4) |
| B10 | No features | Trees, boulders, caves, ores, rivers, lakes, ponds, waterfalls, overhangs built; sky islands planned | **Over-built** | Cut; cancel Phase 12 (§4) |
| B11 | Six blocks | 19 blocks plus slope/slab families of 8 materials | **Over-built** | Cut to air + six (§3.9) |

Everything outside world generation and rendering — the server core, the player controller,
networking, persistence, menus and hosting, the master server, releases, and the planned physics
phases — is unaffected by the baseline and stays (§5).

## 3. Requirement by requirement

### 3.1 B1 — Bifacial world: aligned

Built in Phase 13 ([ADR 0023](./adr/0023-bifacial-world.md), [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md)):
`BifacialTerrain` runs two generators in face-local coordinates and mirrors face B into place
(`MirrorY`, `MirrorChunkY`, `client/src/world/face.ts`), the controller works in a face frame,
the flip band carries bodies across the midplane, the camera somersaults, per-face light and sky.
All of that stays. What the baseline changes around it:

- **The crust becomes ~20 layers instead of ~4 km** (B3). The mechanisms do not depend on the
  thickness, but the numbers around them do: `MIDPLANE_Y`, `WORLD_BOTTOM_Y`, the mirror constant
  (`h = 2 × MIDPLANE_Y − 1 − y`), `kMinChunkY`, the golden hashes. The flip band (4 m) and
  `CORE_ANCHOR_LAYERS` (8, Phase 14) fit inside 10 layers.
- **LOD across a thin crust (risk).** Today LOD sections up to level 6 (2,048 m) lie on one face;
  with a crust of tens of metres, every section from level ~2 straddles the midplane. The
  straddling-section path exists (sections meshed for the viewer's face, the other face culled),
  but it was built for the rim and coarse levels; at 8–16 m cells the crust is one or two cells
  thick, so distant terrain can merge both faces or show holes. Needs its own check (§7).
- **The rim** becomes a wall a few tens of metres tall instead of ~4 km: both seas end at the disc's
  edge. Going over the rim still swings a body across the midplane.
- **Face B keeps its own seed stream** (`FaceSeed`), so it gets its own island. Whether face B
  should mirror face A's island instead is an open decision (§6, D5).

### 3.2 B2 — 50 km radius: over-built, shrink

`WORLD_RADIUS` is one constant (`voxel.h`, `shared/protocol` constants, §7.4) and
`InsideWorldDisc` already bounds every generator, so the disc itself shrinks in one line. The cost
is everything sized for 8,192 km:

| Built for planet scale | At 50 km | Proposal |
|---|---|---|
| Continent cells of 2,560 km, plates of 256 km, a 512 km ocean ring, 300 km ocean gaps (ADR 0017) | Larger than the world | Replace (§3.6) |
| Climate fields of 600–1,200 km, 262 km fBm, 49 km ranges, massifs to 5.6 km (ADRs 0019, 0021) | Meaningless | Cut (§4) |
| `LOD_MAX_LEVEL` = 19 (one 16,777 km root section) | Level 12 (131 km) covers the disc | Lower it; `LodIndex` level 8 can stay |
| `FLIGHT_CEILING` 24,000 km, fly-speed levels up to 2^19.5 | Beyond the build sphere | Clamp to the sphere (§3.4); fewer speed levels |
| `JPH_DOUBLE_PRECISION`, 1/256 m fixed-point wire positions, region-anchored terrain collision (2 km regions), integer/remainder noise lattice (ADR 0011) | float32 is ~4 mm at 50 km: no longer *needed* | **Keep.** They are built, tested and cheap, and removing them is work with risk and no gain for players. Revisit only if they cost measurable performance |
| F4 map zoom to 128 km per column | The whole disc fits in a few zoom steps | Narrow the zoom range |
| Far-origin test runs (`--dwell-origin-x=far`, ~8,000 km) | Outside the world | Move the far origin to ~49 km |

### 3.3 B3 — Ten stone layers per face: gap

Today: `MIDPLANE_Y` = −2,048, sea level 0, ocean floors to −1,800 m, stone from the surface down
to the midplane, ores in it, caves carved through it. The baseline wants a thin, plain crust.

**Proposed layout** (face-local height `h`, counted up from the midplane; the same on both faces):

| `h` | Contents |
|---|---|
| 0 – 9 | **The core: 10 layers of stone**, always present, never carved by terrain |
| 10 – surface | Stone up to 3 blocks under the surface, then dirt, then grass (or sand at the coast and on the sea floor) |
| up to sea level | Water over any lower ground |

So the whole disc is at least 20 layers of stone thick (10 + 10), the sea floor never cuts into the
core, and higher ground is stone with a skin of soil. Proposed numbers, to tune: **`MIDPLANE_Y` =
0** (the origin of generation, which also centres the build sphere, §3.4), **sea level at `h` =
32**, the sea floor no lower than `h` = 12, land up to ~`h` = 96–160. The midplane stays
chunk-aligned, so the mirror still maps chunks onto chunks.

Alternative reading (D1): exactly 10 stone layers under the surface everywhere. With independent
terrain on the two faces the crust's thickness would then vary column by column, and hills would
be dirt-filled or float; the fixed core above is simpler and keeps the faces independent.

Removes: ores (step 7), caves (step 4), the deep-rock/abyss goldens, `BEDROCK_LAYERS` in the test
worlds (§3.9).

### 3.4 B4 — Spherical build limit: gap

Today the world is a vertical slab (`WORLD_BOTTOM_Y` −10,240 to `WORLD_MAX_Y` 6,144) inside the
disc's cylinder, and creative flight goes to 24,000 km. The baseline bounds everything by a
**sphere centred on the origin of generation** — (0, `MIDPLANE_Y`, 0), which is (0, 0, 0) under
§3.3's proposal.

What the sphere bounds (one predicate, `InsideBuildSphere`, beside `InsideWorldDisc` in
`voxel.h` and `face.ts`):

- **Edits** — `VoxelModification` outside it is rejected (server check, client pre-check, §6.5).
- **Generation and storage** — chunks wholly outside are air and never stored; `IsAirChunk`,
  `SkyFloor`, `LodBoundsAt` treat them as empty.
- **Streaming and LOD** — chunk rows and LOD sections are bounded by it instead of the slab
  (`kMinChunkY`/`kMaxChunkY` become the sphere's extent).
- **Flight** — `FLIGHT_CEILING` becomes the sphere (and a matching floor on face B); a player can
  still fall past it (gravity brings them back), or is held at it (D3).

**Radius (D2).** Proposed **`BUILD_RADIUS` = `WORLD_RADIUS` = 50 km**: the disc's rim lies on the
sphere's equator, the build height over the centre is 50 km, and it tapers toward the rim (10 km at
49 km out, ~3 km at 49.9 km). A larger radius (e.g. 64 km) keeps more height near the rim. The
vertical span grows from 512 chunk rows to ~3,125, but almost all of it is air, which streaming and
the LOD already skip; check that `LodBounds` and the air-chunk index stay cheap at that height.

### 3.5 B5 — Simple noise terrain: over-built, replace

Today's pipeline (`ARCHITECTURE.md` §6.3, generator version 11): climate → valley floor `V` +
`D` × relief (hills, ridged mountains, uplift belts, planet-scale ranges and massifs, the
derivative-damped ridged cascade) − river carve, lakes, ponds → 3D density with overhangs → caves →
surface and strata by biome → slope stage 5b → stability pass → ores → boulders and trees.

**Proposed pipeline** (per column, still a pure function of `(seed, version, chunk)`):

1. **Island mask** from the continent layout (§3.6): signed coast distance `s`.
2. **Height** `h(x, z)`: on land, a few octaves of fBm (the existing `noise.h` gradient noise,
   e.g. 4–5 octaves from ~4 km down to ~32 m) scaled up with distance inland, so coasts are low
   and the interior hilly; at sea, a shallow floor falling from the shore to `h` = 12.
3. **Fill**: core, stone, dirt, grass/sand, water (§3.3). A 2D height field — no 3D density, so no
   overhangs, no floating pieces, and no stability pass.
4. **Shapes** come from the adaptive-block rules at use, not from the generator (§3.7).

**Keep** from today: the determinism rules and their tooling (ADR 0010: integer-hashed noise, strict
IEEE, the `+ − × /` rule, `sqrt` only in the layout), the 4-column lattice and bilinear
interpolation, golden hashes run natively, in WASM and in the client module, `GenerateLod` from
the same fields, generator versioning, `dwell_worldgen_inspect` (height and disc images, `view`
screenshots, `bench`), the F3/F4 debug tools.

**Spawn**: the first level, dry spot on the island near the origin (no more "beside water", no
tree-free test).

### 3.6 B6 — One Voronoi continent: partial

Built (Phase 10, ADR 0017, `continents.h/.cpp`): two Voronoi layers on jittered grids, a
domain warp, a signed coast distance from the nearest land and sea plate sites plus fractal coast
detail, guaranteed separation, per-continent records, a 256 m macro lattice shared with the LOD.

**Keep the machinery, change the layout:**

- **One layer of cells** sized for the island (e.g. ~2–4 km). A cell is land when its warped site
  lies within a noisy radius of the origin (e.g. 20–30 km, a low-frequency noise on the angle);
  everything else is sea. The origin's cell is always land.
- **Signed coast distance** as today (half the difference of the distances to the nearest sea and
  land sites), with coast detail scaled down to the island.
- **Clearance from the rim, guaranteed**: no land within `RIM_OCEAN` (proposal: 8 km) of the rim,
  checked by sampling as `continents_test.cpp` does for today's rules.
- **Drop**: continent cells, plates and plate edges/convergence (only Phase 11 used them), island
  plates, `ContinentRecord`'s climate biases, wind and shelf width, the shelf/slope/abyss profile,
  `OCEAN_GAP`.

### 3.7 B7 — Slopes from the Shape Playground: partial

The baseline's slopes are the **adaptive natural blocks** of Phase 9e
([`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md), [ADR 0024](./adr/0024-adaptive-natural-blocks.md),
prototypes in [`prototypes/`](./prototypes/README.md)): the five rules fitted to the owner's 250
hand-shaped blocks, baked into one 6,561-entry table, read by the server, client meshers, collision
and LOD. Designed and approved in the prototypes; **nothing built**.

| Built today (Phase 9) | Baseline |
|---|---|
| Shape mechanism: four corner heights, the shape table (`shapes.mjs` → C++/TS), `block_shape.h` queries, collision polygons, targeting, edit checks, face culling, adjacency test | **Keep** — 9e builds on it, adding 12 corner and 6 ridge shapes |
| Placed slope and slab families (`dwell:<m>_slope`, `_slab`, 144 + states for 8 materials), shape picking in the hotbar | **Cut** — every remaining solid block is natural (grass, dirt, stone, sand), so no crafted material is left to place slopes of |
| Generator slope stage 5b (`slopes.h/.cpp`, the 0.3.1/0.4.1 fixes, `slope_terrain_test`, `slopes_test`) | **Cut** — 9e removes it |
| `flooded` property and water in shapes | **Keep** for shaped blocks under water (shores) |

So 9e becomes core baseline work, and simpler than planned: no crafted-vs-natural split to
handle (ADAPTIVE_BLOCKS.md §8's open details about crafted neighbours, logs and slabs disappear;
ladders are the only non-natural solid-ish block, and they have no solid). Mark all four solid
blocks `"adaptive": true`. ADR 0024 moves from Proposed to Accepted, and ADR 0016 is amended
(placed families removed).

### 3.8 B8 — WebGPU: gap

Today: Three.js 0.186.1 with `WebGLRenderer` (ADR 0002), all of it behind Dwell's `client/render`
interface, which was designed for this swap ("moving to `WebGPURenderer` (TSL shaders) later is a
backend swap behind the same interface"). Three.js-specific code is ~1,600 lines in
`client/render/three/`.

What has to move:

| Piece | Today | WebGPU |
|---|---|---|
| Renderer | `WebGLRenderer` in `ThreeRenderer.ts` | `WebGPURenderer` (async `init()` before the first frame) |
| Height fog and per-face lighting (`heightFog.ts`) | GLSL chunks spliced in with `onBeforeCompile` | TSL nodes on node materials (`colorNode`, a fog node); `onBeforeCompile` does not exist there |
| Sky (`sky.ts`) | GLSL `ShaderMaterial` | `NodeMaterial` with the same sky function in TSL |
| Chunk and LOD materials, texture atlas, alpha masks for biome tints | Standard materials + spliced GLSL | Node materials; the tint path goes away with biomes (§4) |
| Batched terrain (`batchedTerrain.ts`, `?batch=1`), near/far depth passes, tone mapping, exposure | WebGL | Supported by `WebGPURenderer`; re-verify each |
| Screenshot script, e2e (Playwright Chromium) | WebGL in headless Chromium | WebGPU in headless CI needs flags (`--enable-unsafe-webgpu`, a software adapter) — **a risk to CI**; prove it first |

Platforms: current Chrome/Edge, Electron, Safari 26 (macOS and iOS, so Capacitor's WKWebView) and
Android WebView (Chrome 121+) have WebGPU. Firefox's support still varies by platform, and older
iOS and Android devices have none.

**Fallback (D4).** `WebGPURenderer` can run on a WebGL2 backend automatically when WebGPU is
missing, with the same TSL materials. The baseline says "not WebGL"; the proposal is **WebGPU
first, with Three's WebGL2 backend as a fallback only** — the code is written once in TSL, there is
no GLSL left, and players on older devices can still play. The alternative is WebGPU-only, with a
clear "your browser does not support WebGPU" screen.

Needs a new ADR superseding ADR 0002's backend choice (Three.js and the render interface stay).

### 3.9 B11 — Six blocks: over-built, cut

Registry today (`shared/blocks/dwell.json`): air, bedrock, stone, dirt, grass, stone_slab, ladder,
water, launch_pad, sand, sandstone, gravel, snow, ice, log, leaves, coal_ore, iron_ore, gold_ore,
plus slope and slab families of stone, dirt, grass, sand, sandstone, gravel, snow and log.

| Block | Baseline | Notes |
|---|---|---|
| air | Keep | Implicit (state 0) |
| grass, dirt, stone, sand | Keep | Adaptive (§3.7) |
| water | Keep | Static, as today; not placeable |
| ladder | Keep | Climbing stays in the controller |
| bedrock | **Cut** | Only the flat and playground test worlds still use it (`BEDROCK_LAYERS`); give them a stone floor (or the core of §3.3) |
| launch_pad | **Cut** | A movement-test block in the playground world (`voxel.cpp`). Also cut the `launch_speed` property and its controller scenario (PLAYER_CONTROLLER.md "launch-pad knockback"), or keep the property with no block using it (D6) |
| stone_slab, all slope/slab families | **Cut** | §3.7 |
| sandstone, gravel, snow, ice, log, leaves, coal/iron/gold ore | **Cut** | Biomes, beds, frozen seas, trees, ores (§4) |

Knock-on: the hotbar palette goes from 14 to 5 slots (stone, dirt, grass, sand, ladder) — update
`e2e/edit.spec.ts` (`CLAUDE.md` notes the count); the texture atlas shrinks; the registry hash
changes; world files' string palettes no longer resolve removed blocks (a new compatibility line,
§8). The registry mechanism itself (ADR 0015) stays: real content will come back through it.

## 4. What to cut

Grouped by where it lives; each cut removes code, tests, goldens and the docs that describe it.

| Cut | Code | Docs and decisions |
|---|---|---|
| Rivers, lakes, wetland ponds, waterfalls, terraced water above sea level, valley floor `V` and `D` | `worldgen/rivers.h/.cpp`, `Column::water`, `rivers_test`, `water_search.h`; LOD channel sampling | ADR 0018 and 0020 superseded; WORLD_GENERATION.md §3.2–3.3, §3.10 |
| Climate, the 19-biome table, rain shadows, lapse-rate snow, biome tints | `worldgen/biomes.h/.cpp`, `SampleClimate`, `TintGrid`, WASM `dwell_worldgen_tint`, the worker's tint job, `TintField`, the `tints` vertex attribute, atlas alpha masks, `LodSurface` tints, `climate_test`, `vegetation_test`, `biome_search.h` | ADR 0019 and 0022 superseded; WORLD_GENERATION.md §3.5–3.7, §3.12 |
| Mountains: ridged cascade, uplift belts, planet ranges, massifs, overhangs | `DampedRidges2`, `Perlin2d` derivatives (if unused elsewhere), `cascade_test`; 3D density | ADR 0021 superseded; WORLD_GENERATION.md §3.4, §3.11 |
| Caves, ores, boulders, trees, stability pass | Steps 4, 6, 7, 8 of the pipeline | §6.3 pipeline rewritten |
| Multi-continent layout, plates, islands, shelf/abyss | Most of `continents.cpp` (§3.6) | ADR 0017 amended; WORLD_GENERATION.md §2 rewritten |
| Placed slopes and the slope stage | `slopes.h/.cpp`, slope/slab families, shape picking | ADR 0016 amended; SLOPE_BLOCKS.md trimmed to the shape mechanism |
| Sky islands (Phase 12, not started) | — | Cancel Phase 12; WORLD_GENERATION.md §4; `docs/reference/aether-floating-islands.md`; the dome bounds |
| Planet-scale sizing | Constants in §3.2 | ADR 0011 amended (scale), its precision decisions kept |
| Removed blocks | §3.9 | BLOCK_REGISTRY.md examples |

Roughly: `terrain.cpp` (1,978 lines) shrinks to a few hundred; `rivers.*` and `biomes.*` (~840
lines) go; `continents.*` (~620) halves; six worldgen test files are replaced by a smaller set.

**Not cut, though simplifiable later:** the double-precision and region machinery (§3.2), the
LOD octree (still needed: from the ground the view reaches the rim, 50+ km away), the 7-step
pipeline skeleton (fewer stages, same structure).

## 5. What stays unaffected

Aligned with the baseline already, and untouched by it:

- Server core, fixed-step loop, threading, Jolt (§4), and the planned physics phases 14–16
  (awakening, tiered physics, sleep/re-bake) — they gain a simpler world to run in.
- The physics player controller (PLAYER_CONTROLLER.md), prediction and reconciliation, ladders,
  swimming, the face frame — minus the launch pad (D6).
- Networking: WebTransport/WebRTC, channels, message formats (protocol bumps for the changed
  constants and registry, not new messages).
- Persistence (SQLite world files, string palettes), block edits, streaming, the LOD system's
  mechanisms.
- Menus, friend worlds, dedicated servers, the master server on Cloudflare, identity.
- Versioned releases and the launcher (Phase 6), the Phase 7 palette, tone mapping and sky (they
  are rendering, ported with B8).

## 6. Open decisions for the owner

| # | Decision | Proposal |
|---|---|---|
| D1 | What "10 layers of stone" measures | A fixed core of 10 stone layers per face from the midplane; higher ground is stone under a soil skin (§3.3) |
| D2 | Build sphere radius | 50 km, equal to the world radius (§3.4) |
| D3 | At the sphere's surface in flight | Held at the boundary (no flying out); falling is unaffected |
| D4 | WebGL fallback | WebGPU first, Three's WebGL2 backend only as an automatic fallback, all shaders in TSL (§3.8) |
| D5 | Face B's island | Its own island from its own seed stream (as today), not a mirror of face A's |
| D6 | Launch pads | Remove the block, the `launch_speed` property and its tests |
| D7 | Terrain scale | Island ~20–30 km radius, ≥ 8 km of sea to the rim, sea level `h` = 32, land up to ~`h` = 96–160 — to tune in screenshots |
| D8 | Midplane at y = 0 | Yes: the origin of generation is the centre of the disc, the midplane and the build sphere |

## 7. Proposed order of work

A re-plan for `IMPLEMENTATION_PLAN.md`, replacing the outstanding parts of Phases 9–13 (to be
agreed before editing the plan):

1. **Baseline world** (one compatibility line): new constants (radius, midplane, sea level, core,
   build sphere, LOD root, flight bounds); the block set cut to six; the simple generator and the
   one-island layout; cut rivers, biomes, tints, features, caves, ores, placed slopes and the slope
   stage; goldens regenerated; ARCHITECTURE.md §6 and WORLD_GENERATION.md rewritten in the same
   change; ADRs for the baseline (superseding 0011's scale, 0017–0022) and Phase 12 cancelled.
   Checks: the disc is 50 km; the island never comes within 8 km of the rim; every column has 10
   core layers per face; no edit is accepted outside the sphere; generation time against version 11.
2. **Adaptive natural blocks** (Phase 9e, simplified as in §3.7): table, meshing, collision, LOD,
   controller scenarios on the new shapes. Check the thin-crust LOD at the same time (§3.1).
3. **WebGPU renderer** (new ADR): prove WebGPU in headless CI first, then port fog, sky and
   materials to TSL behind the render interface; frame time on a desktop and a phone against the
   WebGL build.

Steps 1 and 2 can be one compatibility line if they land together; step 3 does not change any
public API and can land in either order.

## 8. Compatibility and releases

The baseline changes the terrain every seed generates, the block registry (and so the saved world
format's palettes) and protocol constants: a **new compatibility line** (`CLAUDE.md`, ADR 0014).
Before 1.0.0 that is the next minor, **0.9.0** — raised in `client/package.json` by the owner, not
by an agent. Worlds of the 0.1–0.8 lines keep opening in their own versions through the launcher.
