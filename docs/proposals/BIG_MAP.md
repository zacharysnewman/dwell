# A 10× map: feasibility

**Status:** written 2026-11-11, not adopted. Nothing in the game changes. The owner (2026-11-11): "we
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

## 6. Phases (each ≤ 45 minutes)

- **M0 — the radius as one value.** Derive `lod.maxLevel` and the big map's `cellM`s from the radius
  in their generators; scale the island's shape tunables by `islandRadius`; a test that a radius of
  158 km passes every assertion. No change at today's radius (goldens unchanged).
- **M1 — the LOD root for a big disc.** `maxLevel` 14 with the section keys and `CHUNK_OFFSET_Y`
  reworked; a new protocol version; tests at 158 km on both faces.
- **M2 — a 10×-area test build.** A dev build (`?bigmap` or a branch) at 158 km; measure world
  creation, memory and the big map in the browser on the M1 (`npm run bench`); the owner plays it.
- **M3 — audit the shaders** for absolute f32 world coordinates; a test that feeds one at 158 km.
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

1. Which 10×: the area (a 158 km radius, recommended for the first test) or the radius (500 km,
   100× the area)?
2. Is a desktop-only test first acceptable (M0–M2), with the phone after the regional maps (R1–R6)?
3. Per build or per world: should the radius be a world setting (ADR 0054, an optional tag), so a
   big world sits beside today's on the same line, or one value per build?
4. Should the island grow with the disc (one 79 km island), or more islands across a wider sea?
5. Is a fraction of a second's wait for a new region's map acceptable when a player first nears it?
6. Is 1–2 minutes to create a 10×-area world acceptable for the test, before the regional maps?
