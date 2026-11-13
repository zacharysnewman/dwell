# A 10× map: feasibility

**Status:** written 2026-11-11; the owner answered §7 the same day (10× radius, a world setting of
named sizes, desktop first). **M0 built** (the size as one per-world value, every derived value at
1× unchanged); **M1 built** (the LOD root holds every named size; protocol v23, a compatibility
question for the owner); **M2's native part built** (the generators, disc, build volume, LOD and
server read the world's size; a 2× world creates natively in 4.6 s at 176 MB); **M2b built** (the
browser: New world → More options' World size on desktop pages, `?size=2x`, every worker and the
client following the world's size), waiting on the owner's desktop test; M3 onward not started. The owner (2026-11-11): "we
will test out a 10x size map soon (could start whenever)". This study adds two things to
[`WORLD_SCALE.md`](./WORLD_SCALE.md) (2026-11-10, which measured a disc 10× *across*): the case of 10×
the *area*, measured, and the whole-map grids built since then (the biome field, the geology's
drainage, the ecology's island, the big map's levels), which are what BUGS.md #70 ran out of memory on.

## 1. What "map size" is today

- **The disc** is `worldRadius` 50 000 m, the island `islandRadius` 25 000 m, both in
  `shared/protocol/constants.json` (data, but a protocol constant: one value per build, not per world).
  `dwell_world::WORLD_RADIUS` and `dwell_protocol::world::ISLAND_RADIUS` carry them into Rust.
- **The world map** (one per face) is a square grid over the whole disc at `hydrology.json`'s
  `grid.cell` 100 m: 1002² = 1.0 M cells. Its size follows the radius; the cell is data.
- **Grids that follow the map:** the biome field's coast, fresh-water and rain grids
  (`biomemap.rs`), the geology's `DrainageField` (`geology.rs`), the ecology's island at
  `ecology.json` `island.cellM` 200 m (501² cells), the big map's levels (`shared/ui/map.json`: 1 m
  round the player, 16 m over the island, 256 m over the disc, 32-cell tiles).
- **Constants that do not follow it** (code, not data): the LOD octree's root, 32 × 2¹² = 131 km
  (`lod.maxLevel` 12, with `lod.rs`'s assertions that the root holds the disc and
  `CHUNK_OFFSET_Y == 1 << 11`); the island shape's absolute tunables (`island.rs`: 5 km radius
  variation, 3 km warp, 4 km coast detail) and the uplift layer's fixed counts of ranges and plates.
- **Other readers of the radius:** the LOD's near-rim check (`dwell-lod` `system.rs`), the fog's
  rim (`dwell-look` `fog.rs`), Chisel's map texel (`dwell-client` `chisel.rs`), the hydrology grid
  (`hydrology/mod.rs`), the biome field's extent; all derive from the constant, none hardcode 50 km.

**What 10× means.** Two readings, very different in cost:

| | Today | 10× area | 10× radius |
|---|---|---|---|
| Disc radius / island radius | 50 / 25 km | 158 / 79 km (×√10) | 500 / 250 km |
| Disc area | 7 850 km² | 78 500 km² | 785 000 km² |
| Map cells a face (100 m) | 1.0 M | 10.0 M | 100 M |

## 2. Whole-map structures: bytes

Per face unless said; a world has two faces, and the server and each of the two world workers in a
browser hold their own copy. *Measured* is `cargo run --release -p dwell-worldgen --example
world_scale` in this cloud container (4 cores, one thread, seed 0, face A), with `worldRadius` and
`islandRadius` edited and `lod.rs`'s root assertion relaxed for the run (not committed); the rest is
the grids' cell sizes × cells.

| Structure | Bytes a cell | Today | 10× area | 10× radius |
|---|---|---|---|---|
| World map in memory (height f32, flags u8, lake u16) | 7 | 7 MB | 70 MB | 700 MB |
| Encoded map (IndexedDB / disk), *measured* | — | 7.1 MB | 70.7 MB | ~710 MB |
| Biome field (coast, fresh, rain, f32) | 12 | 12 MB | 120 MB | 1.2 GB |
| Geology drainage field (distance, height, f32) | 8 | 8 MB | 80 MB | 800 MB |
| A many-biome world's set in one process (BUGS.md #70, native) | — | ~60 MB | ~600 MB | ~6 GB |
| **Peak of one face's build**, *measured* | ~90 | **101 MB** | **871 MB** | ~9–16 GB |
| Ecology island (200 m; 9 B a land cell, ~20 % land) | 9 | 0.45 MB (heap ≤ 3 MB) | ~4.5 MB (≤ ~30) | ~45 MB (≤ ~300) |
| Big map, widest level (256 m), tiles, *measured* | — | 148 | 1 256 | ~12 200 |
| Big map, island level (16 m), tiles (512 m each) | — | ~7 500 | ~75 000 | ~750 000 |
| LOD far range (cache, budgeted: 96 MB on a phone) | — | budget | budget | budget |
| Saved world's map (two faces) | — | 14 MB | 141 MB | ~1.4 GB |

The LOD's cache and the edited chunks of a save scale with the view and with play, not with the
map. Everything else above is a whole-map grid, so it grows with the area: 10× area is 10× each.
The phone's limit (Safari kills a tab at ~1–1.5 GB with the GPU's share; the starter world is 850 MB
since 0.52.68) is already close at 1× for a many-biome world, so **at 10× area no whole-map design
fits a phone**: one face's build alone peaks at 871 MB natively, and the browser builds both faces
at once.

## 3. Generation time

Same run (this container is about 2× slower a thread than the M1 `WORLD_SCALE.md` used):

| Step | Today | 10× area, *measured* | 10× radius, extrapolated |
|---|---|---|---|
| Hydrology, one face (erosion is 60 %) | 2.0 s | 30.2 s | ~450 s (∝ cells^1.18) |
| World map's derived parts (channels, shores) | 0.02 s | 0.76 s | ~25 s |
| Biome field (chamfers) | 0.08 s | 1.2 s | ~15 s |
| Spawn search (bounded since 0.51.21) | 0.66 s | 4.5 s | ~5 s (bounded) |
| Big map widest level, native | 0.1 s | 0.9 s | ~9 s |
| **World creation, one face** | **4.1 s** | **62 s** | **~9 min** |

On the M1, roughly half these times; a phone is 2–3× slower than the M1 a thread. So a 10×-area
world takes about **30 s on the M1 and 1–2 minutes on a phone** to create, before its first chunk
(both faces in parallel; ~1.7 GB of peak memory at once). Chisel's map redraw for each tuning step
grows the same way. Chunk generation stays flat (the lake index of 0.51.21); this run did not time
it at 10× area (the build-volume sphere was not scaled, so the spawn's chunks generated empty).

### 3.1 Named sizes, measured (BUGS.md #91, 2026-11-12)

Each named size's map has today's ~1 M cells (its cell × the scale), so a face should cost about
the same at every size. It did not: a 10× world sat for minutes on "Shaping the world map" in the
owner's browser, and natively **one 10× face took 43.7 s** (1×: 1.9 s) and a **10× server 114 s at
a 12.4 GB peak** — past a browser's 4 GB of WebAssembly memory, so the map workers never finished
(0/2) and the local server's worker died ("Could not connect"). Two steps grew with the radius:

- **The uplift** (17.4 s): `uplift_count` gives a 10× world 300 ranges, and every land cell measured
  its distance to every segment of every range (~21 000 segment tests a cell; 1×: ~200). Now an
  index of the spines by where they reach (`SpineIndex`, 8 km squares; a spine reaches at most
  `max(width, apron.width)` m, beyond which its strength is exactly 0): ~570 a cell, the same
  strengths exactly.
- **The channels' index** (21 s, and the 12 GB): 64 m squares under a 10× map's ~1 km segments and
  their km-wide valleys listed each segment in ~6 000 squares. The square is now 64 m × the scale
  (`Channels::bucket`; the query measures each listed segment's reach itself, so only the candidate
  lists change): ~90 entries a segment (1×: ~10; a 10× map's rivers are wider and 8× as many).

`cargo run --release -p dwell-worldgen --example world_size -- <size>` in this cloud container (4
cores), a cold map cache, after the fix. One face's map by step (the builder's progress), then
both faces' maps with the terrain, the spawn search, and a native server's start to listening:

| Size | base | uplift | erosion | lakes | rivers | shores | **one face** | both faces | spawn | server (peak) |
|---|---|---|---|---|---|---|---|---|---|---|
| 1× | 0.38 | 0.21 | 1.11 | 0.19 | 0.03 | 0.01 | **1.95 s** | 2.2 s | 0.7 s | 3.6 s (195 MB) |
| 2× | 0.36 | 0.46 | 1.19 | 0.22 | 0.04 | 0.05 | **2.32 s** | 2.4 s | 1.2 s | |
| 4× | 0.31 | 0.60 | 1.22 | 0.21 | 0.04 | 0.10 | **2.48 s** | 2.7 s | 0.0 s | |
| 8× | 0.32 | 0.57 | 1.42 | 0.24 | 0.05 | 0.38 | **2.98 s** | 2.8 s | 0.0 s | |
| 10× | 0.38 | 0.63 | 1.34 | 0.28 | 0.04 | 0.44 | **3.11 s** | 3.1 s | 0.0 s | 6.2 s (484 MB) |
| 10× before | 0.30 | 17.39 | 1.31 | 0.30 | 0.05 | 24.14 | **43.68 s** | | | 114 s (12.4 GB) |

`tests/map_build_budget.rs` pins it by op counts per cell (both faces): the uplift's segment tests
a land cell and the channel index's entries a segment at 10× within a small factor of 1×'s. Left:
the 10× channel index still holds ~16 M entries (1×: ~0.24 M), part of the page's memory (#76); the
WASM build is 2–3× slower than native a thread, so a 10× map is ~8 s a face in the browser, both
faces at once.

## 4. What breaks

- **f32 precision.** An f32's step at 50 km is 3.9 mm, at 158 km 1.6 cm, at 500 km 3.1 cm. It does
  not reach the geometry: the renderer draws camera-relative from f64 positions, physics is f64
  (`rapier3d-f64`), and the shaders' world-anchored patterns take wrapped origins
  (`WORLD_SCALE.md` §3.5). A shader that is handed an absolute x or z as f32 would swim by ~1.6 cm
  at 10× area; not every shader has been audited. **No origin rebasing is needed.**
- **Wire formats.** Positions are `posfix` i32 at 1/256 m (±8 388 km), cells i32, chunks i32: fine
  at either size. Bodies' `offset: [i16; 3]` is relative to their reference cell, not absolute.
  Lake ids are u16 in the map and its codec: 1 612 lakes at 10× area, ~30 k extrapolated at 10×
  radius (near the 65 535 limit once wetter biomes add lakes).
- **Chunk keys** are i32 chunk coordinates: fine.
- **The LOD's levels.** The root (131 km) holds a disc up to 65 km. 10× area (316 km across) needs
  `maxLevel` 14 (524 km), 10× radius 15 (1 049 km). The root is a cube, so its Y range grows with it,
  and the `CHUNK_OFFSET_Y == 1 << 11` assertion and the section keys need reworking: a protocol
  change either way.
- **The minimap** reads only round the player (64 m): unaffected. **The big map's** widest level
  goes from 148 to 1 256 tiles at 10× area, its island level (16 m) from ~7 500 to ~75 000; at the
  browser's ~40 ms a tile in the background (ADR 0068) that is ~50 minutes for the island level.
  Its `cellM`s should follow the radius (the widest at 1 024 m keeps ~80 tiles).
- **The island's shape** at 79 km with today's absolute tunables is a smoother near-circle with the
  same few ranges: the tunables should scale with `islandRadius` (data, not a cost).
- **Travel.** Across the disc at 5 m/s: 100 km today ≈ 5.6 h, 316 km ≈ 17.6 h, 1 000 km ≈ 56 h. The
  ecology's herds and the water's far fast-forward cover 10× the cells per day step.
- **Saved worlds.** 141 MB of map per world in IndexedDB at 10× area (14 MB today), before any edits.
- **Compatibility.** A new radius changes the terrain a seed generates: a new compatibility line.

## 5. A design that works on a phone

The rule: **nothing scales with the whole map at full detail.** Three parts, the first enough for a
desktop test of 10× area.

1. **The radius as one value.** `lod.maxLevel`, the big map's `cellM`s, the island's shape tunables
   and the uplift's counts derived from `worldRadius` / `islandRadius` (by a generator, checked in
   CI), so a 10×-area build is one edit of `constants.json`. Then 10× area runs on a desktop today
   (62 s, 871 MB a face natively; the browser's 4 GB WASM memory holds one face's build, so the two
   faces build in their own workers).
2. **Coarse whole-map grids.** The whole disc at 1 km: 316² = 0.1 M cells at 10× area (1 M at 10×
   radius, today's cost). It holds what must be whole: the continents and ranges, the big rivers and
   their discharge, the climate's wind and rain shadow, the ecology's island (1 km cells for far herds),
   the big map's widest level and the spawn's candidates.
3. **Fine grids per region, on demand.** The 100 m map, the biome field and the drainage field built
   per region (e.g. 25.6 km squares, 256² cells: by today's ~90 B a cell at the build's peak and
   ~2 s a million cells, ~0.2 s and ~6 MB each natively, with its margin), when a player
   comes within a region of it, as a pure function of the seed, the region and the coarse map: a
   region's edges take the coarse map's inflows and the coarse rivers' courses, with an overlap
   margin, so neighbours meet. A player holds the 3 × 3 regions round them (0.6 M cells, ~16 MB of
   map, biome and drainage grids natively: less than today's whole map), cached per region in
   IndexedDB, evicted by distance. The ecology's near herds and the big map's 16 m level read the regions loaded; the
   far ones read the coarse map. Chisel tunes one region at a time.

The cost of part 3 is the seams (erosion that agrees across region edges, lakes that straddle them),
and a new compatibility line. `WORLD_SCALE.md` §4 option 2 is the same design for 10× radius.

**The owner's answers (§7) change part 2's role.** The test is 10× *radius* (500 km, 100 M cells
a face at 100 m: ~9 minutes and many GB a face, §2–§3), so a desktop test can't wait for the
regional maps to run its whole-map grids at 100 m. Until R1–R4, a bigger world runs its whole-map
grids at a **coarser cell that scales with the radius**: the hydrology `grid.cell` × the size's
scale (1 000 m at 10×: 1 002² = 1 M cells, today's cost and memory), the big map's coarse levels'
cells × the next power of two (no level builds more tiles than today's), the ecology's island the
same way; the fine detail comes from the per-chunk generators, as it does between today's 100 m
cells. What that loses until the regional maps: rivers and lakes smaller than a map cell (a 1 km
cell carries only rivers whose catchment spans many cells; brooks, ponds and small lakes vanish), the
erosion's valleys narrower than ~2 cells, and the coast's and shores' detail between cells; the big
rivers, ranges and climate stay. The sizes are data: `constants.json` `worldSizes` (1×, 2×, 4×, 8×,
10×), each a whole radius scale, so the test steps up through them.

### 5.1 The quick fix: features in metres, not cells (BUGS.md #77, WORK_QUEUE item 52)

Owner, 2026-11-12: "any idea why 10x worlds are super flat with massive rivers and lakes? And no
mountains?" Measured natively (seed 0, face A, `cargo run --release -p dwell-worldgen --example
big_terrain`; `worldmap::relief` measures in metres, so the coarse cell doesn't change what is counted:
a peak is land above 1 000 m that is the highest within 5 km; a range, connected land above 1 000 m of
at least 20 km²):

| size (cell) | | max m | land > 1 500 m | ranges | peaks / 1 000 km² | widest channel (at m³/s) | lakes (share of land) |
|---|---|---|---|---|---|---|---|
| 1× (100 m) | before = after | 2 613 | 6.4 % | 2 | 1.92 | 67 m (8) | 3 (0.11 %) |
| 4× (400 m) | before | 1 961 | 0.3 % | 14 | 1.53 | 469 m (198) | 1 355 (4.2 %) |
| | after | 2 641 | 5.6 % | 14 | 2.37 | 403 m (160) | 13 (1.1 %) |
| 10× (1 km) | before | 1 427 | 0.0 % | 19 | 0.76 | 6 477 m (2 751) | 5 888 (10.5 %) |
| | after | 2 692 | 8.1 % | 52 | 2.83 | 807 m (341) | 1 (0.03 %) |

The causes, and the fix (data: `hydrology.json` `bigWorlds`, applied by `HydrologyParams::sized`, so a
1× map is the same bits; its goldens and the hydrology digests are unchanged):

- **The ranges stood in a ring by the coast.** Their centres were drawn 3–9 km from the coast, in
  metres: a ring on a 250 km island. Now the coast distances × the radius scale (`rangeCoastExponent`
  1), so the ranges (already × the area in number, their widths and lengths in metres) rise across the
  whole island.
- **The erosion cut the ridges down.** A ridge cell drains its own whole cell's area, 100× at 10×, so
  stream-power erosion lowered every ridge by ~ln(scale)·U/K. Now a cell's own share of its area is a
  profile cell's (`ridge_cell`, set only by `sized` above 1×), so ridges keep 1×'s steepness.
- **The coarse map's pits speckled the island with lakes.** The least lake (0.3 km²) was under one
  1 km cell. Now its area × the scale² and its depth × the scale (`lakeAreaExponent` 2,
  `lakeDepthExponent` 1): the smallest lake kept is today's shape in cells; small lakes and ponds wait
  for the regional maps (part 3).
- **The rivers were carved kilometres wide.** A channel's lips (the cells its water pours over) were
  linear in discharge. Now one for one up to `lipsLinearTo` (100 m³/s; 1×'s rivers stay under 20) and
  by √Q beyond (hydraulic geometry), so a 10× world's greatest river is ~800 m across, not 6.5 km.

Pinned by `tests/big_terrain.rs` (4× and 10×, both faces, against 1×'s map: highest land, share above
1 500 m, peaks per area, channel width by √Q, lake share), red with the old rules. Not done in the
quick fix: the uplift's ranges sampled per column at full resolution (the map would have to carry its
spines to the chunk generator, a world-map format change; at 10× a 1 km cell still blurs a ridge's
crest between cells), and the regional 100 m maps (part 3, R1–R4) for the small rivers and lakes.

## 6. Phases (each ≤ 45 minutes)

- **M0 — the radius as one value, per world. [built]** The named sizes as data
  (`constants.json` `worldSizes`: 1×, 2×, 4×, 8×, 10×, generated into `dwell_protocol::world_sizes`
  and the TS `WorldSizes`, validated by `shared/protocol/gen.mjs`); a world's size in its settings
  (`WorldSettings` tag `WORLD_SIZE` 0x83, optional, `u16` scale: saved with the world, sent at join,
  written only for a size above 1× so today's worlds' bytes are unchanged; a world without it is 1×;
  the server's `ServerConfig::world_size` for a new world). Derived from it in
  `dwell_protocol::world_size::WorldSize`: `lod_max_level` (12, 13, 14, 15, 15; `dwell-world`
  asserts the compiled `lod.maxLevel` is 1×'s), the big map's coarse cells
  (`MapTunables::big_map_levels_sized`: 10× is 1, 256, 4 096 m), the hydrology cell and the uplift
  counts (`HydrologyParams::sized`: 1 000 m and × 100 at 10×), the island's shape
  (`IslandShape::sized`, `IslandLayout::sized`), the disc for the hydrology grid
  (`grid_for_radius`). Tested at every named size; at 1× every derived value is today's (goldens
  unchanged). **Left for M1–M2:** the compiled LOD root holds only 1× (`WorldSize::fits_lod_root`;
  the ignored test `every_named_size_fits_the_lod_root` is the failing check M1 turns on), the build
  volume (`bounds.radius`) and `WORLD_RADIUS` readers (fog, Chisel, the LOD's rim) are still the
  build's, and the generators still build 1×: the sized constructors are not yet called from a
  world's settings.
- **M1 — the LOD root for a big disc. [built]** The compiled root holds the largest named size:
  `lod.maxLevel` 15 (1 049 km), checked by `shared/protocol/gen.mjs` against `worldSizes` (a size
  past it fails the generator with the level it needs); `CHUNK_OFFSET_Y` is `1 << (maxLevel − 1)`
  (2¹⁴); a world's own top level is `WorldSize::lod_max_level`. One root for every size, not a root
  per world: section keys mean the same section in every world of a build, and a 1× world's extra
  levels 13–15 are a handful of coarse sections. Every section key moved (the root's corner went
  from −2¹⁶ to −2¹⁹), so the wire's LOD messages changed meaning: **protocol v23**, a new
  compatibility line for the owner to raise. Saved worlds keep loading: their LOD rows were keyed
  under root 12 (`meta.lod_root` absent), so they are dropped at open and rebuilt from the chunks.
  The terrain is unchanged: the regenerated LOD goldens give every section at the same place the
  same hash (levels 1–11; 12–15 are new sections). `every_named_size_fits_the_lod_root` is on.
  **Left for M2:** the build volume (`bounds.radius`) and `in_world`'s disc are still the 1× disc's,
  so a bigger world's far sections aren't generated until the generators read the world's size.
- **M2a — the generators read the world's size, natively. [built]** `dwell_world::WorldBounds`
  (the world's disc, `radius_scale` × the build's, and its build volume, `BuildVolume::sized`: as wide
  as the disc and as tall as today's, a spheroid in exact integers, so every size's chunk rows and LOD
  rows are the same); `lod::in_world_of`. The world map knows its size (`WorldMap::size`, derived from
  its grid, so its bytes are unchanged) and the map cache keys on (seed, size) (`for_seed_sized`;
  the disk key gains `-x<scale>` only above 1×); `WorldMap::compute_sized` runs the profile `sized`
  over `BaseField::sized` (the size's island and disc); `TerrainGenerator::sized` and
  `BifacialTerrain::sized` take the map's size for their disc, build volume and base field; the
  biome field's climate gradient spans the map's disc; the spawn search reads the map, so it follows.
  `dwell_worldgen::*_for_sized` (generator, air test, LOD generator and bounds, spawn, runoff,
  ground density) and `JobRunner::for_world_sized`; the server takes a saved world's size from its
  settings before building its generators (`saved_world_size`: a world without the tag, or with a
  size past this build's LOD root, is 1×), and its streaming, edits' chunk checks and LOD
  propagation use `Server::bounds`; `dwell-server --size 2x` makes a new world at a named size (a saved
world keeps its own; `tests/host.rs`). The LOD system's near-rim check and its in-world test read
  `LodSystem::set_world`. Every 1× path is the old one (the free functions are `WorldBounds::DEFAULT`'s),
  so the goldens are unchanged. Tests: `dwell-worldgen/tests/world_size.rs` (both faces: a 2× world's
  map at 200 m over the 100 km disc with today's cell count, chunks past today's rim generating sea,
  the spawn on its own disc), `bounds.rs`/`world_bounds.rs` (the 2× disc and volume, 1× the build's),
  the LOD's `the_rim_is_the_world_s_own`, and `persistence.rs` (a saved 2× world reopens at 2× with
  2× bounds; one without the tag at 1×). Measured natively in this cloud container (4 cores,
  `cargo run --release -p dwell-worldgen --example world_size -- 2x`, a cold map cache): both faces'
  maps, the terrain and the spawn search — **1×: 3.7 s, 164 MB peak; 2×: 4.6 s, 176 MB peak** (the
  map stays 1002² cells: its cell doubles). **Left for M2b:** the browser — the client's workers
  build their generators and maps from (version, seed) (`dwell-client` `jobs`, the map builder
  worker and its IndexedDB key), so they need the join's `WorldSettings.size` passed in, and
  `LodSystem::set_world` called from it; the dev link (`?size=2x`) and the New world form's "World
  size" (desktop only); Chisel's map texel (Chisel tunes a 1× world); the flat and sandbox worlds
  stay 1×; Realistic worlds' plate layout and climate (`climate.rs`, R2/R3) still read the 1× radius; `portal_spawn_for` (`?spawn=portal`) is 1×'s. The fog has no rim of its own at run time
  (`fog.mjs`'s `2 × worldRadius` is the data's distance cap). Then 4×, 8× and 10× in the browser.
- **M2b — a big dev world in the browser. [built; the owner's desktop test outstanding]** The
  size's way in: New world → More options' **World size** (`worldSizeChoices`: every named size,
  none on a touch page, owner 2026-11-11 "desktop first"; a terrain world's only) and the dev link
  `?size=2x` (its own world file, `local-g8-s<seed>-x2`). The world index keeps a new world's size
  (`WorldMeta.radiusScale`, optional; the world file's settings are its truth), so the page builds
  its maps at it before the world starts: `worldMaps(seed, version, deps, worldgen, radiusScale)`
  keys them apart (`worldMapKeys` with the scale: `dwell_worldgen::config::worldgen_map_key_sized`,
  today's key at 1×, `-x<scale>` above) and builds them over the bigger disc (`buildWorldMap` with
  the scale, `worldgen_map_bytes_sized`; a test world with its own height field is 1× only). The
  local world's worker hands the scale to `LocalWorld` (the server's `world_size` for a new world;
  a saved one reads its own) and the server-jobs worker builds `JobRunner::for_world_sized` from
  the core's (`radiusScale`, the saved world's). The client reads the join's size
  (`Session::world_size`, `WorldSize::playable`: a size this build can't hold is 1×) and passes it
  to the shell's `prepareWorld` (4th argument), its job workers (`radiusScale` in their init,
  `JobWorker.worldSized`, `Runner::world_sized`) and `Game::set_world` (the portals, the streamer's
  build volume `ChunkStreamer::set_volume`, the LOD's `set_world`, the big map's levels
  `big_map_levels_sized` and their reveal check `map_reveal::is_level_sized`); the shell's big map
  takes the levels and the disc from the client (`bigMapLevels`, `worldRadiusScale`,
  `setMapWorldSize`). Every 1× path is the old one: no new field or message at 1×, the same map
  keys. Tests: `game_local.rs` (a local world created at 2× saves and reopens at 2× through
  LocalCore, its maps keyed apart, its client's bounds and big map levels the size's, the terrain
  arriving; a world saved without a size at 1×), `WorldSize::playable`, and Vitest over the form's
  choices (none on touch), the route, the link, the index record, the map cache and the big map's
  sized levels. **Left:** the owner plays 2× on the desktop, then 4×, 8× and 10×, with the bench's
  measurements of world creation, memory and the big map at each (`npm run bench` on the M1); Chisel
  tunes a 1× disc (its map texel is the build's radius); the flat and sandbox worlds stay 1×;
  `portal_spawn_for` (`?spawn=portal`) and Realistic worlds' plates and climate are still 1×'s; a
  `?size=` link's world (like a spawn link's) has no menu record.
- **Fix after M2b (BUGS.md #108, 2026-11-12):** past 2× the terrain stopped drawing at the 1× disc's 50 km
  circle (and at 8×/10× round the spawn): the LOD's section classification (`lod::kind_from_bounds`, in
  `LodSystem::classify` and `BifacialTerrain::generate_lod`) still tested the build's 1× disc. It now classifies in
  the world's (`kind_from_bounds_of`); tests at 4×, 8× and 10× on both faces (`dwell-lod` big-world tests,
  `dwell-worldgen` `tests/world_size.rs`). The block placement check is the next 1× reader (BUGS.md #109).
- **M3 — audit the shaders** for absolute f32 world coordinates; a test that feeds one at 500 km
  (3.1 cm steps).
- **M4 — slimmer grids.** f16 for the biome and drainage grids, one shared drainage height copy;
  measure the starter world on the phone tier before and after (BUGS.md #70's table).
- **R1 — the coarse map.** A 1 km whole-disc map from the same hydrology at `cell` 1000, kept beside
  today's; tests that its rivers match today's big rivers at 1×.
- **R2 — a region's map.** A region's 100 m height field and erosion with the coarse map's inflows at
  its edges and a margin; a test that two neighbours agree along their seam.
- **R3 — lakes and rivers across seams.** Lakes that straddle a seam built whole; a test on a
  fixture lake split by a region edge.
- **R4 — regions in the game.** Generators read regions instead of the whole map; the per-region
  cache, loaded round the player and evicted by distance; the biome and drainage fields per region.
- **R5 — the far readers.** The ecology's far island, the big map's widest levels and the spawn
  search on the coarse map.
- **R6 — 10× area on the phone.** The bench's `--touch` tier at 158 km; the memory table of
  BUGS.md #70 at 10× area; then the owner's iPhone.

M0–M3 make a desktop test possible; R1–R6 make it fit a phone; each R phase is a new compatibility
line only once it ships in the default world.

## 7. Questions for the owner

Answered by the owner, 2026-11-11:

1. Which 10×: the area (a 158 km radius) or the radius (500 km, 100× the area)? — **10× radius**: a
   500 km disc and a 250 km island. Added the same day: "Perhaps even scope it as a 2x, 4x, 8x or
   10x": the size is one of a list of named sizes (data), each a radius multiplier, 1× today's.
2. Is a desktop-only test first acceptable (M0–M2), with the phone after the regional maps (R1–R6)?
   — **Yes**: desktop first, phones after the regional maps.
3. Per build or per world? — **A world setting** (ADR 0054, an optional tag), so a big world sits
   beside today's on the same line.
4. Should the island grow with the disc, or more islands across a wider sea? — **The plates decide
   land and sea** ([`REALISTIC_WORLD.md`](./REALISTIC_WORLD.md), whose R1 plate generator is being
   built in parallel). M0 only scales the uplift layer's plate and range counts with the area; the
   plates' generator itself is not touched here.

Open:

5. Is a fraction of a second's wait for a new region's map acceptable when a player first nears it?
   **Answered (owner, 2026-11-12): "No, no hitches"** — region maps are built ahead of the player (prefetched
   in workers from the direction and speed of travel, including far flight) so nearing a region never waits.
6. Is 1–2 minutes to create a 10×-area world acceptable for the test, before the regional maps?
