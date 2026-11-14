# A larger world: what 10× wider costs

**Status:** measured 2026-11-10, not adopted. Nothing in the game changes; this is the record of a
measurement and the options it points to, with questions for the owner.

The owner (2026-11-10): "I've been considering expanding our world by up to 10x … I don't know the
performance impact with our new systems. We used to have a much larger world, but it was more
difficult to refine when larger." Clarified: the **disc 10× across** (`worldRadius` 50 km → 500 km,
`islandRadius` 25 km → 250 km; 100× the area).

## 1. How it was measured

`crates/dwell-worldgen/examples/world_scale.rs` (release build, an M1 with 16 GB, one thread, as the
game builds the map): one face's world map built stage by stage (the steps of
`hydrology::from_heights`, timed), then the real build again, the world map's derived parts
(channels, shores), the biome field (ADR 0074's chamfer distance grids), the spawn search
(`TerrainGenerator::spawn_point`), 36 chunks round the spawn and a sample of the big map's
widest-level tiles (ADR 0068: 32 × 32 cells of 256 m, the raster's `column_lod` + `top_state` +
`river_hit`). Peak memory is `/usr/bin/time -l`'s. Scaled by editing `worldRadius` and
`islandRadius` in `shared/protocol/constants.json` (the build-volume sphere left as is) and relaxing
the LOD octree's root assertion (§4), seed 0, face A. 10× was not run: its extrapolated peak (~16 GB)
is the machine's whole memory and its run well over the 5-minute rule; 1×, 2× and 5× fit a clean
trend.

## 2. Numbers

| | 1× (50 km) | 2× (100 km) | 5× (250 km) | 10× (500 km), extrapolated |
|---|---|---|---|---|
| Map grid (100 m cells) | 1002² = 1.0 M | 2002² = 4.0 M | 5002² = 25 M | 10002² = 100 M |
| Base height field | 0.29 s | 1.06 s | 6.9 s | ~29 s |
| Uplift layer (+ coast distance) | 0.15 s | 0.62 s | 4.0 s | ~17 s |
| Erosion | 0.56 s | 2.5 s | 23.9 s | ~110 s |
| Pits, lakes, drainage | 0.10 s | 0.63 s | 4.3 s | ~20 s |
| Rivers, discharge, rain | < 0.01 s | 0.03 s | 0.2 s | ~1 s |
| **Hydrology, one face (whole)** | **1.08 s** | **4.8 s** | **38.9 s** | **~180 s** (∝ cells^1.11) |
| World map derive (channels, shores) | 0.01 s | 0.16 s | 1.8 s | ~17 s |
| Biome field (two chamfer grids) | < 0.01 s | — | 0.69 s | ~3 s |
| **Peak memory, one face's build** | **143 MB** | **628 MB** | **3.9 GB** | **~16 GB** (~158 B a cell) |
| Encoded map, one face (IndexedDB / disk) | 7.1 MB | 28 MB | 177 MB | ~710 MB (1.4 GB a world) |
| Rivers / lakes | 45 / 3 | 318 / 407 | 3106 / 5471 | ~12 k / ~30 k |
| Spawn search | 1.1 s, found | 14 s, **failed** | 161 s, **failed** | minutes, fails |
| Chunk generation at the spawn | 1.0 ms | 2.3 ms | 24–436 ms | worse (§3.1) |
| Big map widest level: tiles | 148 | 532 | 3056 | ~12 200 |
| — a tile, native | 0.4 ms | 0.7 ms | 6.2 ms | worse (§3.1) |

Both faces have their own map, so a new world builds two (in parallel in the browser's workers, so
twice the memory at once).

## 3. What breaks first

### 3.1 Lake shores are a linear scan (a bug at scale, cheap to fix) — fixed in 0.51.21

A profile of chunk generation at 5× is 97 % `worldmap::Shores::query`: every column asks every
lake's distance field whether it covers it, so the cost per column grows with the lake count (3 at
1×, 5471 at 5×). That is why chunk generation (1 ms → 24 ms a chunk, 436 ms at the lake-dense spawn
fallback), the big map's tiles (0.4 → 6.2 ms) and the spawn search grow with the world, where they
should be flat. A spatial index of the lakes' boxes (as `Channels` has for the rivers) makes it flat
again; worth doing whatever the world size, since more lakes come with more biomes too.

**Fixed (0.51.21):** `Shores` keeps a grid of buckets 16 map cells (1.6 km) a side, each listing
the lake fields whose box reaches it in the fields' order, so a query reads the few lakes near
its column and breaks ties as the scan did (the goldens are unchanged; a test compares the index
with a scan of every lake on both faces' maps and on 600 overlapping fixture fields). Measured
after, seed 0 (M1, release): chunk generation 1.03 ms a chunk at 1× (unchanged), 1.03 ms at 2×
(was 2.3), 0.87 ms at 5× at the origin (was 24–436); the big map's tiles 0.4 / 0.5 / 0.5 ms
(were 0.4 / 0.7 / 6.2).

### 3.2 The spawn search falls back to a mountain top — fixed in 0.51.21

At 2× and 5× none of the 48 river-side candidates nearest the centre held a level dry patch, so the
search walked its whole 5000-leg spiral round the origin (14 s, 161 s) and gave up on the origin's
ground, 341–347 m up a range (the uplift layer's ranges lie inland, and a wide island's centre is
far from any coast). The search needs a bounded budget and candidates spread over the island (by
cell, not nearest-the-centre first); the spawn's place on a 500 km island is also a design question
(§6).

**Fixed (0.51.21):** at 1× the 48 river-side candidates rarely hold a patch either: the 16 test
seeds all spawned from the origin's spiral (2–14 km out). To keep those spawns, the spiral stays,
bounded at its 5,000 legs, but is probed natively in batches of 16 k patches over the cores, the
first in spiral order kept (the same answer; seeds 0–15 unchanged on both faces, 0 and 7 pinned
by a test; 1× ~3× faster, e.g. seed 8 2.1 → 0.6 s with its map). Past it, a bounded search: at
most 256 suitable cells (dry, between the spawn heights, not a lake), one per 800 m block,
river-side first, each searched 12 legs round, with the patch level, then within 1 and 2 rows;
then the nearest suitable cell's ground, never the origin's. Measured after, seed 0: 2× found at
(−29898, 18850), 105 m, in 2.1 s (was 14 s and a peak); 5× found at (110434, −15442), 77 m, in
2.1 s (was 161 s and a peak). Where a 500 km island's spawn should be stays §6's question.

### 3.3 The world map's size and the browser

- **Memory:** 16 GB for one face's build at 10×; wasm32 has a 4 GB address space, so the browser
  cannot build it at all, and 5× (3.9 GB) is at that limit. A phone tab (iOS Safari is killed at
  roughly 1–1.5 GB) manages about 2× (628 MB) today.
- **Time:** ~180 s a face natively on the M1, ~200 s with the derived parts. WASM ran the 1× map
  in ~1.5 s for both faces at once when ADR 0049 measured it, about native speed per face; a phone
  is typically 2–3× slower than the M1 a thread. With the memory solved, a 10× world would take
  **~3.5 min on the M1 and ~10 min on a phone** to create, before its first chunk; today it is ~1 s
  and ~3 s.
- **Storage:** 1.4 GB of map per world in IndexedDB (7 MB a face today).
- **Tuning:** Chisel redraws its map from a map build; at 10× each tuning step would be minutes,
  not a second. This is the owner's "harder to refine when larger", measured.

### 3.4 Smaller things that assume the 50 km disc

- **The LOD octree's root is 131 km** (`lod.maxLevel` 12 × 32-cell sections, `dwell-world`
  `lod.rs`'s root assertion). 10× needs `maxLevel` 15 (1 049 km); the root is a cube centred on the
  origin, so its Y range grows with it and the `CHUNK_OFFSET_Y == 1 << 11` assertion and the
  section keys need reworking. A protocol change.
- **The big map's widest level** spans `worldRadius × 2` at 256 m cells: at 10× that is 3 900 cells
  across a ~1 000 px panel and ~12 200 tiles. The browser builds tiles at ~40 ms each in the
  background (ADR 0068: 1 600 in 65 s), so ~8 minutes for that level alone; its `cellM` should
  scale with the world (2 048 m keeps ~190 tiles).
- **Lake ids are `u16`** in the map and its codec (65 535 lakes); 10× is extrapolated at ~30 k,
  within it but not by much once biomes add wet ones.
- **The island's shape tunables are absolute** (`island.rs`: radius variation 5 km, warp 3 km, coast
  detail 4 km) and the uplift layer's counts are fixed (ranges, plates per face): a 250 km island
  would be a near-circle with the same few ranges. These are data to scale with the island, not
  costs.

### 3.5 What does not break: render and physics precision

- **The renderer draws camera-relative.** Positions are f64 on the CPU and each draw's geometry is
  offset by the eye before it becomes f32 (`render/camera.rs`: "positions stay precise ~8,000 km
  out because the renderer draws camera-relative"; `render/mod.rs` subtracts `self.eye` before the
  cast). The shaders' world-anchored patterns take wrapped origins (the water's `wrapped_origin`,
  the wind's `rem_euclid(WIND_PERIOD)`), so they do not lose precision either. Were anything to
  pass an absolute world x or z to a shader as f32, it would quantise to 2⁻⁵ m = 3.1 cm at 500 km
  (1.6 cm at 250 km): a swimming pattern, not shaking geometry. No such path was found, but not every
  shader was audited.
- **Physics is f64** (`rapier3d-f64`), and the wire's positions are i32 at 1/256 m: ±8 388 km.
- **Chunk coordinates** are i32 chunks; no limit near 500 km.

## 4. Options

1. **Coarser map cells per scale** (the cheapest; `hydrology.json`'s `cell` is already data). Keep
   ~1 M cells: 1 km cells at 10× cost what today's map costs (~1 s, 143 MB, 7 MB). But rivers and
   lakes are traced on the grid: at 1 km a river's course is 10× coarser, lakes smaller than a few
   km² vanish, and the chunk-level shaping (banks, channels) would draw on a much blurrier map. A
   middle ground: 250 m cells at 10× is 16 M cells, ~25 s and ~2.5 GB, still too much for a phone.
2. **A coarse whole-world map, fine regional maps on demand** (recommended if the world grows). The
   whole disc at 1 km (continents, ranges, the big rivers and their discharge, the biomes' climate,
   the big map's widest levels), then 100 m maps of regions (e.g. 51.2 km squares, 512² cells,
   ~0.3 s and ~40 MB each) built when a player nears them, each a pure function of the seed, the
   region and the coarse map: its edges take the coarse map's inflows and the coarse rivers' courses
   are kept, so neighbouring regions meet. A player sees only the regions round them; a 500 km world
   then costs about what today's does to create, and Chisel tunes one region at a time. The cost is
   design work: seams between regions (overlapping margins, rivers fixed at region edges), the cache
   becoming per region, and a new compatibility line.
3. **Grow less than 10×.** 2× (100 km) works today on the M1 (5 s a face, 628 MB) once §3.1 and §3.2
   are fixed, and on a phone at the edge of its memory; 3× would not on a phone.

Independently of the size: fix the lake index (§3.1) and the spawn search (§3.2) (both done in 0.51.21), and make the big
map's widest `cellM`, the LOD root and the island's shape tunables follow the world radius from data
(§3.4), so the radius is one value in `constants.json`.

## 5. Recommendation

Do not raise the radius on the current design: past ~2× the world map no longer fits a browser,
and at 10× it takes minutes and gigabytes even natively. If a larger world is wanted, build option 2
(a 1 km whole-world map with 100 m regions on demand) as its own phase and compatibility line, after
the cheap fixes of §3.1–3.2, which pay off at today's size too. Rendering and physics precision are
not the constraint.

## 6. Questions for the owner

1. Is the goal more distance to travel, or more varied places? (Option 2 serves both; more biomes on
   today's disc serves the second at no cost.)
2. Which size is enough: 2× (works with small fixes), or 10× (needs regional maps)?
3. Should the island still fill most of the disc at 10×, or a few islands or continents across a
   wider sea (the archipelago knobs, `docs/proposals/SYSTEMIC_TERRAIN.md`)?
4. Is it acceptable that a new world's first visit to an area waits a fraction of a second for its
   region's map (option 2), as chunks wait today?
5. Where should players spawn on a 500 km island: near the centre, near a coast, or chosen?
