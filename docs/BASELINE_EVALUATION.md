# Dwell — Baseline Evaluation: What to Keep, Add and Cut

> **Status: evaluation, 2026-10-12.** Nothing here is built or decided yet. It compares the owner's
> new baseline with the code and docs on `main` (app 0.8.1, generator version 11, protocol v12) and
> proposes the work to get there. Once the owner settles the open decisions (§7), the agreed parts
> become ADRs, a re-plan of `IMPLEMENTATION_PLAN.md`, and changes to `ARCHITECTURE.md` in the
> changes that build them.

## 1. Why

Earlier phases added features nobody asked for in the baseline — biomes, rivers, climate, trees,
ores, sky islands, a planet-scale world — and much of it is hardcoded. The owner wants a smaller,
simpler basis made of systems to build on. The **baseline** (owner, 2026-10-12):

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

### 1.1 What "aligned" means

A requirement is **aligned** only when it exists as an **extendable system**, not as a one-off
hardcoded result (owner, 2026-10-12). The test:

> **If you can't reasonably build a tool to add something to it, it's not an extendable system.**

In practice a system here has:

- **A data source of truth** — a file (JSON or similar) describing the things it holds: blocks,
  shapes, noise layers, surface rules, textures, materials — not constants and branches in C++ or
  TypeScript.
- **One generator or loader** that turns the data into what the server, WASM core and client use,
  so they agree (the pattern `shared/blocks/gen.mjs` and `shared/protocol/gen.mjs` already follow,
  with CI failing on stale output).
- **References by name, not by code constant** — the terrain says `"dwell:sand"`, not
  `Materials::kSand`.
- **Validation and tests over the data**, so a tool (or a person) adding an entry gets an error,
  not a broken world.

A tool means something like the existing prototypes — a page that edits the data file and shows
the result — or a CLI. The baseline content can be small (six blocks, one island, one noise
height field); the **system** under it must still take a seventh block, a second noise layer, or
a new surface rule as data.

A feature that is hardcoded counts as a **gap** even if the result looks right today.

## 2. Summary

Two questions per requirement: does the **result** match, and does it exist as a **system**?

| # | Requirement | Result today | System today | Verdict | Work |
|---|---|---|---|---|---|
| B1 | Bifacial disc | Built (Phase 13) | **Yes** for geometry (constants in `constants.json`, one mirror mapping); face B's terrain is the same hardcoded generator re-seeded | **Partial** | Keep; per-face terrain definitions (§3.1) |
| B2 | 50 km radius | 8,192 km | **Yes** — `world.worldRadius` in `constants.json`, generated into both languages | **Over-built** (result) | Change the data; cut planet-scale-only code (§3.2) |
| B3 | 10 stone layers per face | ~2 km of rock per face; layers hardcoded in `terrain.cpp` (`kFloorLayers`, strata branches) | **No** | **Gap** | Layers as data in the terrain definition (§3.3) |
| B4 | Spherical build limit | Vertical slab + flight ceiling | Bounds are data; the shape (slab) is code | **Gap** | A bounds predicate driven by data (§3.4) |
| B5 | Simple noise terrain | 1,978-line `terrain.cpp`, 72 `constexpr` tunables, materials by C++ constant | **No** | **Gap** (and over-built) | Replace with a data-driven terrain definition (§3.5) |
| B6 | One Voronoi continent | 12–13 continents; parameters `constexpr` in `continents.cpp` | **No** (algorithm reusable, parameters hardcoded) | **Partial** | Layout parameters as data; one island (§3.6) |
| B7 | Playground slopes | Designed, not built; Phase 9 placed slopes built | **Yes** for shapes (corner-height data in `shapes.mjs`); the planned rule table is code-derived | **Partial** | Build 9e with the table as data the playground can edit (§3.7) |
| B8 | WebGPU | WebGL; looks are a closed enum, shaders GLSL spliced in code | **Partly** — the render interface is a system; materials/looks are not | **Gap** | WebGPU + a look/material registry (§3.8) |
| B9 | No biomes | 19 biomes in a C++ `constexpr` array | No (hardcoded table) | **Cut** | Remove (§4) |
| B10 | No features | Trees, boulders, caves, ores, rivers, lakes, ponds built in code | No | **Cut** | Remove; cancel Phase 12 (§4) |
| B11 | Six blocks | 19 blocks + slope/slab families | **Yes** — the registry (ADR 0015) is data; **but** textures are code (closed `TileName` list, painter functions) and worldgen/edits name blocks by C++ constant | **Partial** | Cut to six; textures as data; names not constants (§3.9) |

Systems that already pass the test and stay as they are: the block registry, protocol constants,
the shape table, block properties driving behaviour (`climbable`, `liquid`, `placeable`, `shape`,
`density`), the render interface, the LOD octree, versioning (§6).

## 3. Requirement by requirement

### 3.1 B1 — Bifacial world: partial

**Result: aligned.** Built in Phase 13 ([ADR 0023](./adr/0023-bifacial-world.md),
[`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md)): `BifacialTerrain` runs two generators in face-local
coordinates and mirrors face B into place (`MirrorY`, `MirrorChunkY`, `client/src/world/face.ts`);
the controller works in a face frame; the flip band carries bodies across the midplane; per-face
light and sky.

**System: mostly.** The geometry is data (`midplaneY`, `flipBand`, `coreAnchorLayers` in
`constants.json`) with one mapping on each side, and the mirror of shaped blocks is derived from
registry properties (`MirrorMaterial` swaps `half`), not a list. What is not a system: a face's
terrain is "the terrain generator, seeded with `FaceSeed`" — there is no way to describe face B
differently except by changing code.

**Work:**
- Each face takes a **terrain definition** (§3.5), by name: both faces may use the same
  definition with different seeds (the default), or different ones.
- Re-tune for the thin crust (B3): the mirror constant, `kMinChunkY`, goldens.
- **Risk — LOD across a thin crust.** Today sections up to level 6 (2,048 m) lie on one face; with
  a crust of tens of metres, every section from level ~2 straddles the midplane. The straddling
  path exists (meshed for the viewer's face, the other face culled) but was built for the rim and
  coarse levels; at 8–16 m cells the crust is one or two cells thick, so distant terrain can merge
  both faces or show holes. Needs its own check (§8).
- The rim becomes a wall a few tens of metres tall instead of ~4 km.

### 3.2 B2 — 50 km radius: over-built result, aligned system

`world.worldRadius` is data in `constants.json`, generated into C++ and TS, and `InsideWorldDisc`
bounds every generator — changing the radius is a one-line data change. The cost is what was built
around 8,192 km:

| Built for planet scale | At 50 km | Proposal |
|---|---|---|
| Continent cells of 2,560 km, plates of 256 km, 512 km ocean ring, 300 km gaps (ADR 0017) | Larger than the world | Replace (§3.6) |
| Climate fields of 600–1,200 km, 262 km fBm, ranges, massifs (ADRs 0019, 0021) | Meaningless | Cut (§4) |
| `lod.maxLevel` = 19 (16,777 km root) | Level 12 (131 km) covers the disc | Change the data |
| `flightCeiling` 24,000 km, fly-speed levels to 2^19.5 | Beyond the build sphere | Bound by the sphere (§3.4); fewer levels |
| Double-precision Jolt, 1/256 m fixed-point positions, region-anchored collision, the integer/remainder noise lattice (ADR 0011) | No longer needed (float32 is ~4 mm at 50 km) | **Keep**: built, tested, generic (they work at any radius), and cheap. Revisit only for a measured cost |
| F4 map zoom to 128 km/column; far-origin tests at ~8,000 km | Outside the world | Narrow the zoom; far origin ~49 km |

### 3.3 B3 — Ten stone layers per face: gap

**Today:** stone from the surface to the midplane (~2 km), ores, caves; the bottom layers
(`kFloorLayers` = 4) and the strata (grass/dirt/stone, sand under water, sandstone on dunes…) are
branches in `terrain.cpp` and the biome rows. Not data.

**Proposed: layers as data**, part of the terrain definition (§3.5), read top-down and bottom-up:

```jsonc
"core":    [{ "block": "dwell:stone", "layers": 10 }],          // from the midplane up, never carved
"surface": [                                                     // from the surface down
  { "when": "underwater", "layers": [{ "block": "dwell:sand", "depth": 2 }] },
  { "when": { "coastWithin": 3, "below": 2 }, "layers": [{ "block": "dwell:sand", "depth": 3 }] },
  { "layers": [{ "block": "dwell:grass", "depth": 1 }, { "block": "dwell:dirt", "depth": 3 }] }
],
"fill":    "dwell:stone",                                        // between core and surface layers
"seaLevel": 32, "liquid": "dwell:water"
```

With that, "10 layers of stone" is a number in a file, a gravel layer or a second liquid is an
entry, and an editor page can show a column cut-away as it changes. The conditions (`underwater`,
coast distance, height above sea level, slope) are a small fixed vocabulary in code; the rules are
data.

**Proposed baseline values** (face-local height `h` above the midplane, both faces): the core is
`h` 0–9 (10 layers of stone, so ≥ 20 for the whole disc); stone up to 3–4 blocks under the surface,
then dirt and grass (sand at the coast and under water); **`midplaneY` = 0** (the origin of
generation, also the build sphere's centre, §3.4); **sea level `h` = 32**; sea floor ≥ `h` = 12.
The midplane stays chunk-aligned. Alternative reading (D1): exactly 10 stone layers under the
surface everywhere — with independent faces the crust's thickness would then vary per column;
the fixed core is simpler.

### 3.4 B4 — Spherical build limit: gap

**Today:** bounds are data (`worldBottomY`, `worldMaxY`, `flightCeiling`), but the bound's
*shape* — a vertical slab inside the disc's cylinder — is code (`kMinChunkY`/`kMaxChunkY`, checks
in streaming, edits and flight).

**Proposed:** one bounds predicate, `InsideBuildVolume`, in `voxel.h` and `face.ts`, driven by data:

```jsonc
"bounds": { "shape": "sphere", "center": [0, 0, 0], "radius": 50000 }
```

with the shape a small closed set in code (sphere today; a slab or cylinder would be one more case,
which keeps tests and old worlds' bounds expressible). Everything that bounds the world reads the
predicate: **edits** (server check, client pre-check), **generation and storage** (chunks wholly
outside are air, never stored; `IsAirChunk`, `LodBoundsAt`), **streaming and LOD** (chunk rows and
sections bounded by it), **flight** (held at the boundary, D3; falling unaffected).

**Radius (D2).** Proposed 50 km, equal to the world radius: the rim lies on the sphere's equator,
50 km of build height over the centre, tapering to ~10 km at 49 km out and ~3 km at 49.9 km. The
vertical span grows from 512 chunk rows to ~3,125, nearly all air; check `LodBounds` and the
air-chunk index stay cheap.

### 3.5 B5 — Simple noise terrain: gap

**Today:** generator version 11 is a C++ pipeline (`terrain.cpp`, 1,978 lines, 72 `constexpr`
tunables, ~30 `Materials::k…` references): climate → valley floor + relief (hills, ridged
mountains, belts, ranges, massifs, the ridged cascade) − rivers, lakes, ponds → 3D density with
overhangs → caves → strata by biome → slope stage → stability pass → ores → boulders and trees.
Generators are chosen by a hardcoded switch on the version (`GeneratorFor`: flat, playground,
terrain). Changing anything means editing C++ and bumping the version. Not a system.

**Proposed: a terrain definition** — data, in `shared/worldgen/` beside `shared/blocks/`, generated
into C++ tables (and TS for tools) by a `gen.mjs`, with CI checking stale output and a **hash of
the definition** carried like the registry hash (so a world knows exactly what generated it, and a
changed definition is a new generator version without hand-bumping):

```jsonc
{
  "name": "dwell:baseline",
  "layout":  { … },                        // the island, §3.6
  "height": {
    "land": [                               // summed; each scaled by the named mask
      { "noise": "fbm", "wavelength": 4096, "octaves": 5, "gain": 0.5, "amplitude": 48, "mask": "inland" }
    ],
    "sea":  [ { "profile": "coastDistance", "from": 30, "to": 12, "over": 2000 } ]
  },
  "core": …, "surface": …, "fill": …, "seaLevel": 32    // §3.3
}
```

- **Fixed in code (the engine):** a small set of node kinds — gradient-noise fBm, the layout's coast
  distance and masks derived from it, constants, sum/scale/clamp — evaluated under the existing
  determinism rules (ADR 0010: integer-hashed noise, integer wavelengths, `+ − × /`, the 4-column
  lattice). The pipeline becomes: layout → height field → fill by layers. No 3D density, so no
  overhangs, floating pieces or stability pass.
- **Data (what a tool edits):** which nodes, their parameters, the layers, block names.
- **Tools:** `dwell_worldgen_inspect` already renders height maps, disc images and first-person
  views from the generator — point it at a definition file; a prototype page (like the Adaptive
  Block Lab) can edit a definition and show a preview. Adding a noise layer is then an entry, not a
  code change.
- **Generators become definitions**: the flat world is a definition with no noise; the movement
  playground's obstacle course (hand-placed in `voxel.cpp`, including the launch pad at fixed
  coordinates) is a one-off — replace it with a saved world or a structure file used by tests, or
  cut it (D7).
- **Spawn** follows a rule in the definition (e.g. nearest dry, level land to the origin) rather
  than the current hardcoded search.

**Keep:** determinism and its goldens (native, WASM, client module), `GenerateLod` from the same
fields, the inspect tools, F3/F4.

### 3.6 B6 — One Voronoi continent: partial

**Today** (Phase 10, ADR 0017, `continents.cpp`): a reusable algorithm — jittered Voronoi grids,
domain warp, a signed coast distance from the nearest land and sea sites plus fractal coast detail,
sampled separation tests, a 256 m macro lattice shared with the LOD — but tuned for 12–13
continents, with its sizes and rules as code constants.

**Proposed:** keep the algorithm, put the layout in the terrain definition:

```jsonc
"layout": {
  "kind": "voronoiIsland",
  "cell": 3000,                                    // site grid (m)
  "warp": { "wavelength": 6000, "amplitude": 1500 },
  "land": { "radius": 25000, "radiusNoise": { "wavelength": 20000, "amplitude": 5000 } },
  "rimClearance": 8000,                            // no land this close to the rim, guaranteed
  "coastDetail": { "octaves": 5, "wavelength": 4000, "amplitude": 800 }
}
```

A cell is land when its warped site lies within the noisy radius; the origin's cell always is;
`rimClearance` is enforced by clamping and checked by sampling, as the separation rule is today.
More islands later would be another `kind` or a list of land regions — an entry, not a rewrite.
**Drop:** continent cells, plates and plate edges, island plates, `ContinentRecord`'s climate,
wind and shelf fields, the shelf/abyss profile, `OCEAN_GAP`.

### 3.7 B7 — Slopes from the Shape Playground: partial

The baseline's slopes are the **adaptive natural blocks** of Phase 9e
([`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md), [ADR 0024](./adr/0024-adaptive-natural-blocks.md),
[`prototypes/`](./prototypes/README.md)): five rules fitted to the owner's 250 hand-shaped blocks,
baked into a 6,561-entry table read by the server, meshers, collision and LOD. Nothing is built.

**What is already a system:** the shape set is data — each shape is four corner heights
(`SLOPE_SHAPES` in `shapes.mjs`), and the generator derives every polygon, profile, volume and
both languages' tables from them; `adaptive` is planned as a block-data flag. Adding a shape is an
entry, and the Shape Playground is the tool.

**What would not be:** ADR 0024 plans the table as the *output* of five rules in code. Then the
only way to change how one neighbourhood is shaped is to change a rule, and the playground's
hand-shaped exports have no way back in.

**Proposed:**
- The **table is the source of truth, as data** (`shared/blocks/adaptive.json` or similar):
  generated from the five rules, plus **overrides** per neighbourhood. The generator checks that
  every entry names a known shape and that the no-holes adjacency test still passes.
- The **Shape Playground exports overrides** in that format, so the tool that designed the rules
  can keep editing the result.
- Every remaining solid block (grass, dirt, stone, sand) is `"adaptive": true`, by data.
- **Cut** the placed slope and slab families and shape picking (no crafted material is left in the
  baseline) and the generator's slope stage 5b. The `shapeFamilies` mechanism stays in the
  registry generator — a future crafted block can opt in by data — but no block uses it.
- `flooded` stays for shaped blocks under water.

ADR 0024 moves to Accepted with the table-as-data change; ADR 0016 is amended.

### 3.8 B8 — WebGPU: gap

**Today:** Three.js 0.186.1 `WebGLRenderer` (ADR 0002) behind Dwell's `client/render` interface —
that interface is a system (game code never touches Three.js) and was designed for a backend swap.
What is not: how blocks are drawn. A block's `look` is one of a closed list (`cube`, `shaped`,
`water`, `ladder`, … checked in `gen.mjs` and branched on in `mesher.ts`), and the shading — height
fog, per-face light, sky — is GLSL spliced into Three's shaders with `onBeforeCompile` or a
`ShaderMaterial`. A new kind of surface is a code change in several places.

**Proposed:**
- **`WebGPURenderer` with TSL node materials** (async `init()`; no `onBeforeCompile` there). Port
  fog, per-face light, sky and the chunk/LOD materials to TSL; the biome-tint path goes (§4).
- **A look registry**: each look is a module that registers a TSL material and a meshing style
  under a name; block data names its look; the registry generator validates names against it. A
  future shader is one module plus a data reference — the "future shaders" the baseline asks for
  get a place to go.
- **Platforms:** current Chrome/Edge, Electron, Safari 26 (macOS, iOS — so Capacitor's WKWebView),
  Android WebView (Chrome 121+) have WebGPU; Firefox's support varies by platform; older devices
  have none. **Fallback (D4):** proposed WebGPU first, with Three's WebGL2 backend as an automatic
  fallback running the same TSL materials (no GLSL left). The alternative is WebGPU only, with an
  "unsupported browser" screen.
- **Risk — CI:** Playwright's headless Chromium needs flags and a software adapter for WebGPU; prove
  the e2e suite and the screenshot script on it first.
- Re-verify batched terrain, the near/far depth passes, tone mapping and exposure on the new backend.

A new ADR supersedes ADR 0002's backend choice (Three.js and the render interface stay).

### 3.9 B11 — Six blocks: partial

**The registry is a system** ([ADR 0015](./adr/0015-block-registry.md)): blocks are data in
`shared/blocks/dwell.json`, generated into C++/TS with a registry hash, and behaviour comes from
properties — climbing from `climbable`, swimming from `liquid`, the hotbar from `placeable`,
collision from `shape`. Ladders and water already work this way. Adding a block is an entry.

**Where it leaks (one-offs to fix):**

| Leak | Where | Fix |
|---|---|---|
| Textures are code: a closed `TileName` union, one painter function per tile, a fixed 8 × 8 atlas | `client/render/textures.ts` | Texture definitions as data (parametric noise recipes or image files) named by block data; the atlas built from them and sized to fit; a recipe editor is then a tool |
| Worldgen picks blocks by C++ constant | `terrain.cpp`, `biomes.cpp` (~70 references) | Names in the terrain definition (§3.3, §3.5) |
| Breaking a flooded shape leaves `Materials::kWater` | `block_edit.cpp` | The liquid a block is flooded with is data (or the definition's `liquid`) |
| Empty-shape edit checks use `ShapeOf(Materials::kStone)` | `block_edit.cpp` | Use the full-cube shape, not a block |
| Launch pad placed at fixed coordinates | `voxel.cpp` | Goes with the block (below) |

**The block set:** keep air, grass, dirt, stone, sand, water, ladder. **Cut** bedrock (only the
test worlds' `BEDROCK_LAYERS`; give them the stone core), launch_pad (and, D6, its `launch_speed`
property and controller scenario), stone_slab and every slope/slab family, sandstone, gravel,
snow, ice, log, leaves and the three ores, with their texture tiles. Knock-on: the hotbar goes from
14 to 5 slots (update `e2e/edit.spec.ts`); the registry hash changes; old palettes no longer
resolve (a new compatibility line, §9).

## 4. What to cut

These are over-built **and** hardcoded; none passes the system test, so there is nothing to keep
as a mechanism. Each cut removes code, tests, goldens and the docs describing it.

| Cut | Code | Docs and decisions |
|---|---|---|
| Rivers, lakes, ponds, waterfalls, terraced water, valley floor `V` and `D` | `worldgen/rivers.*`, `Column::water`, `rivers_test`, `water_search.h`, LOD channel sampling | ADRs 0018, 0020 superseded; WORLD_GENERATION.md §3.2–3.3, §3.10 |
| Climate, the 19-biome table, rain shadows, snow lines, biome tints | `worldgen/biomes.*`, `SampleClimate`, `TintGrid`, WASM `dwell_worldgen_tint`, the worker's tint job, `TintField`, the `tints` attribute, atlas alpha masks (`TINT_MASKS`), `LodSurface` tints, `tint` in block data, `climate_test`, `vegetation_test`, `biome_search.h` | ADRs 0019, 0022 superseded; WORLD_GENERATION.md §3.5–3.7, §3.12 |
| Mountains: ridged cascade, belts, ranges, massifs, overhangs | `DampedRidges2` (and `Perlin2d` derivatives if unused), `cascade_test`, the 3D density | ADR 0021 superseded; WORLD_GENERATION.md §3.4, §3.11 |
| Caves, ores, boulders, trees, the stability pass | Pipeline steps 4, 6, 7, 8 | §6.3 rewritten |
| Multi-continent layout, plates, islands, shelf/abyss | Most of `continents.cpp` (§3.6) | ADR 0017 amended; WORLD_GENERATION.md §2 rewritten |
| Placed slopes, the slope stage | `slopes.*`, slope/slab families, shape picking | ADR 0016 amended; SLOPE_BLOCKS.md trimmed |
| Sky islands (Phase 12, not started) | — | Phase 12 cancelled; WORLD_GENERATION.md §4; `docs/reference/aether-floating-islands.md` |
| The movement playground world (unless kept as a structure file, D7) and the launch pad | `voxel.cpp` playground, `kGeneratorPlayground` | PLAYER_CONTROLLER.md scenarios that use them |
| Removed blocks and their tiles | §3.9 | BLOCK_REGISTRY.md examples |

Roughly: `terrain.cpp` goes from ~2,000 lines to a small definition evaluator; `rivers.*` and
`biomes.*` (~840 lines) go; `continents.*` (~620) halves; six worldgen test files give way to tests
over definitions.

## 5. What to add (new systems)

The baseline needs fewer features but four systems that do not exist yet:

| System | Data | Tool it enables | Replaces |
|---|---|---|---|
| **Terrain definitions** (§3.5) | `shared/worldgen/*.json`: layout, height nodes, layers, sea level, spawn rule | `dwell_worldgen_inspect` on a definition; a preview/editor page | `terrain.cpp`'s pipeline and constants; the generator switch |
| **Build volume** (§3.4) | `bounds` (shape, centre, radius) | — (a value, but one predicate everywhere) | The slab checks spread over streaming, edits, flight |
| **Adaptive table as data** (§3.7) | Rule-generated table + overrides | The Shape Playground exports into it | Rules only in code |
| **Looks and textures** (§3.8, §3.9) | Look modules registered by name; texture recipes/images in data | A texture/recipe editor; new looks without touching the mesher's branches | The closed `look` and `TileName` lists, GLSL splicing |

## 6. What stays unaffected

Already systems, untouched by the baseline:

- Server core, fixed-step loop, threading, Jolt; the planned physics phases 14–16.
- The player controller, prediction and reconciliation; ladder climbing and swimming, driven by
  block properties (minus the launch pad, D6).
- Networking, channels, message formats (protocol bumps for changed constants and registry).
- Persistence (SQLite, string palettes), edits, streaming, the LOD mechanisms.
- Menus, friend worlds, dedicated servers, the master server, identity.
- Versioned releases and the launcher; the Phase 7 palette, tone mapping and sky (ported with B8;
  the sky colours could join the look data later).

## 7. Open decisions for the owner

| # | Decision | Proposal |
|---|---|---|
| D1 | What "10 layers of stone" measures | A fixed core of 10 stone layers per face from the midplane; higher ground is stone under a soil skin (§3.3) |
| D2 | Build sphere radius | 50 km, equal to the world radius (§3.4) |
| D3 | At the sphere's surface in flight | Held at the boundary; falling unaffected |
| D4 | WebGL fallback | WebGPU first, Three's WebGL2 backend only as an automatic fallback, all shaders in TSL (§3.8) |
| D5 | Face B's terrain | The same definition as face A with its own seed (its own island), selectable per face (§3.1) |
| D6 | Launch pads | Remove the block, the `launch_speed` property and its tests |
| D7 | The movement playground world | Replace with a structure/saved-world file the controller tests load, or cut |
| D8 | Terrain scale | Island ~20–30 km radius, ≥ 8 km of sea to the rim, sea level `h` = 32, land to ~`h` = 96–160 — tuned with the inspect tool |
| D9 | Midplane at y = 0 | Yes: the origin of generation is the centre of the disc, the midplane and the build sphere |
| D10 | How far "data" goes for terrain | A fixed vocabulary of node kinds in code, parameters and composition in data (§3.5) — not a general scripting language |

## 8. Proposed order of work

A re-plan for `IMPLEMENTATION_PLAN.md`, replacing the outstanding parts of Phases 9–13 (to agree
before editing the plan):

1. **Cut** (§4) and shrink the block set (§3.9). Mostly deletion; goldens regenerated.
2. **Terrain definitions and the build volume** (§3.3–3.6): the definition format, its generator
   and hash, the evaluator in `server/core`, `dwell:baseline` (one island, 10-layer core, sea level,
   50 km disc), the sphere predicate everywhere, constants changed in `constants.json`; inspect tool
   reads definitions. Checks: disc radius; land never within `rimClearance` of the rim; every column
   has its core layers on both faces; no edit accepted outside the sphere; a test that a definition
   change changes the hash; generation time against version 11.
3. **Adaptive natural blocks** (9e, with the table as data, §3.7); check the thin-crust LOD (§3.1)
   at the same time.
4. **WebGPU, looks and textures** (§3.8, §3.9): prove WebGPU in headless CI first; then TSL looks,
   the look registry and texture data behind the render interface; frame time on a desktop and a
   phone against the WebGL build.

Each step updates `ARCHITECTURE.md` (§6, §5, §7.4) and its sub-specs in the same change, and ADRs
record the baseline (superseding ADR 0011's scale and 0017–0022), the terrain definitions, the
adaptive table as data, and WebGPU.

## 9. Compatibility and releases

Steps 1–3 change the terrain every seed generates, the block registry (and so saved palettes) and
protocol constants: a **new compatibility line** (`CLAUDE.md`, ADR 0014). Before 1.0.0 that is the
next minor, **0.9.0**, raised in `client/package.json` by the owner, not by an agent. Steps 1–3 can
share one line if they land together; step 4 changes no public API. Worlds of the 0.1–0.8 lines keep
opening in their own versions through the launcher.
