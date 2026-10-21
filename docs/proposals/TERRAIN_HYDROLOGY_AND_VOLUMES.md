# Proposal: terrain hydrology and terrain volumes

> **Status: proposal, not adopted.** Nothing here is architecture yet (`docs/proposals/README.md`).
> A **prototype of the hydrology layer is built** (`crates/dwell-worldgen/src/hydrology/`,
> `shared/worldgen/hydrology.json`, the viewer `cargo run --release -p dwell-worldgen --example
> hydrology`), so river and lake layouts can be judged as images. It is **not wired into chunk
> generation**: no seed's terrain changes, the goldens are untouched, and no compatibility line is
> needed until it is (§8).

The owner's request (2026-10-21): *"implement procedurally generating realistic elevation and water
flow from mountain to ocean (through lakes sometimes)"*, and *"You can start on the hydrology
prototype"*. This proposal covers two plans that meet in the generator: a **world-scale hydrology
map** (2D, global, computed once per seed and face) and a **3D density function** for the volume
features the owner wants (caves, cliffs, arches, floating islands). They are layers of one design
(§3).

## 1. The owner's answers so far

| Question | Answer (owner, 2026-10-21) |
|---|---|
| Caves | **Rich caves.** |
| Cliffs | **Dramatic cliffs, arches and sea stacks.** |
| Cross-face caves | **Yes**: some caves pass through the disc to the other face. |
| Floating islands | **Yes, eventually** (Phase 12 is only temporarily cancelled). |

Still open (§10): the river scale, how much erosion reshapes the land, static vs flowing water, the
generation time budget, and face B's relationship to face A.

## 2. Why a global map

Every chunk today is a pure function of (seed, chunk coordinate), generated on demand on the server,
the clients and the LOD alike (ARCHITECTURE.md §6.3). Rivers and lakes are not local: whether a
valley holds a river depends on everything uphill of it, and a lake's level on its whole rim. They
cannot be computed per chunk from noise. The plan is the usual one for this: a **coarse map of each
face, computed once per seed** (about a second, §6), cached, and **sampled** by chunk generation,
which stays a pure function of (seed, coordinate) *given the map* — the map is itself a pure function
of the seed, so the determinism contract holds.

## 3. The layered design

1. **Layer 0, the world map (2D, global, coarse).** Per face: the height field sampled on a coarse
   grid (100 m cells, ~1,000 × 1,000 over the disc), eroded, its lakes and rivers found. Output:
   eroded height, water surface, lake ids and levels, discharge, river polylines with width and
   depth, river mouths. **This is the prototype (§4).**
2. **Layer 1, the surface (2D, per column, fine).** Chunk generation samples layer 0 (bicubic
   between map cells) for the base height, adds the existing fine noise (the bumps and the hills'
   short octaves) on top, and cuts river channels and lake beds from the polylines (distance to the
   nearest segment → a channel profile from its width and depth). Water fills a lake to its level
   and a river to its surface. The sea, the shore fill and the ramps stay as they are.
3. **Layer 2, the volume (3D density, per cell).** A density function `d(x, y, z)`: solid where
   `d > 0`. Its base is the layer-1 surface (`d = surface(x, z) − y`), plus 3D terms from a data
   vocabulary of nodes, after Minecraft 1.18's density functions:
   - **cheese caves** (large caverns: 3D noise above a threshold, thinned near the surface),
   - **spaghetti caves** (long tunnels: the zero set of two 3D noises, |n₁| + |n₂| < w),
   - **noodle caves** (thin tunnels: the same with a small width),
   - **aquifers** (water tables in caves: a per-region water level from coarse noise, with lava or
     dry air below a depth),
   - **cliff and overhang shaping** (a 3D noise added near steep surface gradients, thresholded: the
     dramatic cliffs, arches and sea stacks the owner asked for, strongest at the coast where the
     sea undercuts),
   - **cross-face shafts** (a sparse set of vertical tunnels whose density ignores the core's
     never-carved rule, passing through the disc to the other face, §7),
   - **floating islands** (a separate band above the surface: a 3D noise envelope, Phase 12's
     return).
   The vocabulary is fixed code (node kinds: noise, add, multiply, clamp, spline, threshold,
   distance-to-polyline, …); a terrain's graph of them is data, as ADR 0026 (terrain definitions)
   already plans for Phase 17. Hydrology's outputs enter the graph as nodes (`map.height`,
   `map.riverDistance`, `map.lakeLevel`).

Each layer only reads the one before: layer 0 never needs layer 2, so caves cannot change rivers
(a cave river would be an aquifer, not part of the surface network).

## 4. The hydrology prototype (built)

`dwell_worldgen::hydrology`, face-local, one thread, deterministic:

1. **Input.** `sample`: each face's own generator (`TerrainGenerator::column_lod`, the LOD's
   sampling, with octaves finer than a cell dropped) at the centre of each cell of a square grid
   covering the disc (`grid.cell` m, data; 100 m → 1,002 × 1,002 with a border of sea). The **sea**
   is the grid's border and every cell at or below sea level connected to it; a hollow below sea
   level cut off from it is land (a depression: a lake, or filled).
2. **Erosion** (`erosion.rs`). Stream-power fluvial erosion after Cordonnier et al. 2016 with Braun
   & Willett's 2013 implicit solver: each iteration floods the surface, accumulates the
   rain-weighted drainage area down the flood tree, and solves
   `h' = h + U·Δt − K·Δt·A^m·((h' − h_r') / d)^n` receivers first — exactly for n = 1, by a
   bracketed Newton iteration otherwise. Uplift `U` is `uplift` where the original ground is at
   least `upliftReference` m high and in proportion below, so the mountains stay where the height
   field put them. Cells under a depression's water are not eroded: they take the uplift and fill
   by `lakeFill` of their depth each iteration (sediment settling), so small basins silt up into
   flats and only large, deep ones last; outlets incise, so lakes also drain over time. An optional
   talus pass moves a quarter of the excess down slopes steeper than `talus.maxSlope`.
3. **Lakes** (`lakes.rs`). Priority-flood depression filling (Barnes, Lehman & Mulla 2014, the
   improved variant with a queue for depression cells). A depression is a lake when its area is at
   least `minArea` km² and its depth at least `minDepth` m; a smaller one is **breached** (a channel
   cut at its floor's height along its drainage path until the ground falls below it, stopping at
   another depression) or **filled** to its spill level (`smallPits`; deeper than `maxBreachDepth`,
   it is filled). A lake is flat at its spill level, and its cells drain across it breadth first to
   the cell the flood entered by, and from there to its **outlet**, so each lake has exactly one.
   No lake is endorheic yet (a rule for closed basins, evaporation against inflow, is a later
   addition).
4. **Flow.** D8 routing: each dry cell drains to its flood parent, its lowest neighbour on the way to
   the sea; accumulation in Braun & Willett's stack order, weighted by rain (`rain.model`: a fixed
   vocabulary, `uniform` only; orographic rain is the next model, the hook is the per-cell rain
   factor). Discharge is in m³/s from `rain.amount` (m/yr). D∞ or multiple-flow-direction routing
   would smooth the accumulation image but not the rivers, which need one path; not done.
5. **Rivers** (`rivers.rs`). Dry cells with discharge ≥ `rivers.threshold` m³/s, traced as polylines
   from each source (a river cell no river flows into, or a lake's outlet) down the receivers to the
   sea, a lake, or the river they join. Each vertex carries its surface height, discharge, and
   width and depth from the `geometry` curve (piecewise linear in discharge, held beyond its ends).
   Where a river of at least `mouths.minDischarge` meets the sea, its mouth is marked: a **delta**
   where the last ~500 m fall less than `deltaMaxSlope`, an **estuary** otherwise.
6. **Determinism.** One thread and a fixed order everywhere; strict IEEE arithmetic with only +, −,
   ×, ÷, `sqrt` and `floor` (powers by our own `log2`/`exp2` in `math.rs`, not the platform's
   `powf`); ties broken by cell index. `HydrologyMap::digest` (FNV-1a over every output) is pinned
   for seed 7 on both faces and for seed 1234 with non-default exponents (the general power path),
   and CI runs the test natively and under WASM (`wasm32-wasip1`). The full-disc digests also match
   between native and WASM for all three profiles (checked by hand, §6).
7. **Data.** Every tunable is in `shared/worldgen/hydrology.json`: named **profiles** (`gentle`,
   `medium`, `strong`) and a `default`. `shared/worldgen/gen.mjs` validates them (unknown keys,
   ranges, an even cell size, an increasing width curve, the vocabularies; a bad entry fails naming
   the profile and key) and emits `hydrology/profiles.gen.rs`; CI fails on stale output. A profile
   added by data alone is emitted and can be made the default (`gen.test.mjs`).
8. **Tests** (`crates/dwell-worldgen/tests/hydrology.rs`): on real maps of both faces
   (`both_faces!`), water from every land cell reaches the sea, every lake is flat to its spill
   level with exactly one outflow and no lower dry neighbour, and no cell or river drains uphill;
   face B of the mirror world gives face A's map (face parity); synthetic fixtures check that a
   crater becomes a lake fed by rivers and drained by one that reaches the sea, that small pits are
   filled or breached and not kept, and that erosion cuts the river cells deeper than the rest;
   the profiles and the width curve; the determinism digests.

### 4.1 The viewer

```
cargo run --release -p dwell-worldgen --example hydrology -- --seed 7 --face a --out DIR \
  [--profile medium] [--seed …] [--face …] [--profile …] [--set key=value --label NAME] [--time-only]
```

prints the timing and statistics per seed, face and profile, and writes PNG layers cropped to the
land at one pixel per cell: `original` (hillshaded), `eroded` (hillshaded over a sea-level colour
ramp), `flow` (log discharge), `rivers` (drawn by width, exaggerated to read at 100 m a pixel: radius
0.3 px + width ÷ 40 m), `lakes` (with outlets in red) and `map` (the composite, with deltas orange
and estuaries magenta). `--set` overrides a tunable for tuning without editing the data.

## 5. What the images show

Seeds 1, 7, 42 and 1234, faces A and B, the three profiles (contact sheets per seed; not checked in:
regenerate with the viewer). Ranges over the 8 seed-faces:

| Profile | Iterations × Δt | Lakes | Lake area | Rivers | Highest ground (494–516 m before) |
|---|---|---|---|---|---|
| gentle | 10 × 25 kyr | 145–195 | 97–131 km² | 71–96 | 448–498 m |
| medium | 25 × 25 kyr | 2–16 | 1–10 km² | 37–48 | 399–449 m |
| strong | 50 × 25 kyr | 0–1 | ≤ 0.3 km² | 38–48 | 351–381 m |

(K = 4 × 10⁻⁶, m = ½, n = 1, U = 10⁻⁴ m/yr, lake fill 0.1, 1 m/yr of rain, rivers from 0.4 m³/s.)

- **Erosion works as intended.** The egg-crate relief of today's height field (fBm hills of 1 km
  wavelength) becomes dendritic valley networks with sharp ridges between them, the classic
  stream-power look, from gentle to strong; every river reaches the sea or a lake.
- **gentle** keeps most of the noise's pits as lakes: a hundred-odd ponds in the interior, which
  reads as noise, not as "through lakes sometimes".
- **medium** leaves a handful of lakes, mostly in the high interior, that rivers flow into and out
  of; the drainage is organised into a few dozen rivers reaching the sea.
- **strong** drains every lake (outlets incise faster than basins fill) and turns the interior
  basins into silted flats.
- **The rivers are small.** The island is round, ~25 km in radius, with its high ground spread
  over it, so it drains radially through many short basins: the largest river carries 7–11 m³/s
  at 1 m/yr of rain (a catchment of ~250–350 km²). Larger rivers need a larger-scale input (one or
  two mountain ranges, an uplift map or a continental layout) rather than different erosion.
- **Every mouth reads as a delta**: the coastal plain is flat everywhere, so the slope rule needs a
  second input (the shelf's depth offshore, or wave exposure) to tell estuaries apart.

**Recommendation for the defaults:** `medium` (25 iterations of 25 kyr, K = 4 × 10⁻⁶), as
`hydrology.json`'s default: lakes "sometimes", organised rivers, the relief kept (about −10 to
−20 % at the top), and ~1 s natively, ~1.5 s in WASM. Before wiring it in, give layer 0 a
larger-scale input: a mountain range or two (an uplift map as data) so rivers run from mountains to
the ocean over tens of kilometres, which is what the owner asked for and what the noise alone does
not have.

## 6. Cost

Full disc, 1,002 × 1,002 cells of 100 m, one thread, measured on the owner's Mac (load average
~3 during the runs; release builds; the WASM build under Node's WASI, `tools/wasi-run.mjs`):

| Profile | Native | WASM | of which sampling the height field |
|---|---|---|---|
| gentle | 0.66–0.70 s | 0.89 s | 0.30 s |
| medium | 1.00–1.07 s | 1.45 s | 0.30 s |
| strong | 1.51–1.68 s | 1.88 s | 0.31 s |

All under the ~2 s target. Each erosion iteration is ~25 ms natively (a flood of the ~200,000 land
cells, the accumulation, the update and a talus pass). The cost is per seed and face, once: a server
computes it at world creation (or caches it beside the world file); a client computes its own copy
(it is deterministic) or receives it (§9). Memory: ~40 MB per face at 100 m in f64 while computing,
~25 MB for the kept map (f32 heights and discharge, u32 receivers and lake ids).

## 7. The volume layer (planned)

- **Multi-surface columns and the ramp rework.** Today a column has one top (its height), and the
  half-height grid's generation rule (`ramps.rs`, HALF_HEIGHT_GRID.md §4) picks a ramp from the
  column's height and its 8 neighbours'. With overhangs, arches and caves a column has several
  surfaces. The rule must become per surface: a cell is a top when it is solid with air above it;
  its "neighbour heights" are the neighbours' nearest top within a few cells of it (or none: an
  edge). Ramps under overhangs and in caves follow the same rule, so the vocabulary stays (gentle,
  steep, flat); only the input changes from a height map to the density's occupancy.
- **The LOD's limits.** The LOD samples columns (one surface per column, `LodSurface`). Caves are
  invisible from afar and can be ignored by it, but cliffs, arches, sea stacks and floating islands
  are not: the LOD needs either a few surfaces per column (top plus undersides) or 3D sampling of
  the density at its cell size. The cheap rule: the LOD evaluates the density's **surface part**
  (layers 0–1 plus the cliff shaping and the floating islands, not the caves) at its resolution,
  with the noise octaves finer than a cell dropped, as today.
- **Cross-face caves.** The core (`CORE_LAYERS`) is never carved today, and the anchor for structural
  integrity is inside it (ARCHITECTURE.md §6.3). A shaft through the disc needs a core exception:
  a sparse, deterministic set of shafts (positions from a hash, a few per 100 km²), and the anchor
  rule changed to "the core except the shafts". Gravity flips at the midplane already, so a player
  falling down a shaft comes out standing on the other face's cave floor.
- **Cost.** A density function costs per cell, not per column: a 32³ chunk is 32,768 evaluations of
  a graph of a few noise nodes, about 10–30× today's per-column cost. Minecraft's approach applies:
  evaluate the density on a coarse lattice (every 4 × 8 × 4 cells) and interpolate, and skip
  chunks a bound shows are all solid or all air (the sky test exists already).
- **Determinism.** The same rules as noise.rs: integer lattices, strict IEEE, no platform maths;
  the 3D noise needs the same integer/remainder lattice split as `lattice` (ADR 0011).

## 8. Compatibility

Wiring layer 0 (or 1 or 2) into chunk generation **changes the terrain a seed generates**: a new
generator version, regenerated goldens, and a **new compatibility line** (the next MINOR before
1.0.0, `docs/RELEASES.md` §3), which the owner raises. The same holds for any later change to
`hydrology.json`'s default profile once it is wired in. Until then the prototype changes nothing a
world sees.

## 9. How it would be wired in

1. A terrain definition (Phase 17) names the hydrology profile it uses; the map becomes part of the
   generator's state (`BifacialTerrain` holds one per face, computed lazily on first use, behind an
   `Arc`).
2. Chunk generation samples it (layer 1); the LOD samples it too, at its cell size.
3. The server computes the map at world creation; clients compute their own copy in a worker (the
   ~1.5 s in WASM would be on world join) or receive it (the eroded height and water surface at
   f16, ~4 MB per face, compressed): an open question for the owner (§10), since the latter is a
   protocol change.

## 10. Open questions for the owner

1. **River scale.** Today's island gives rivers of up to ~10 m³/s (streams 3–40 m wide). Should
   rivers be larger (a few long rivers from a mountain range, which needs a larger-scale input),
   and should the biggest be navigable?
2. **How much erosion reshapes the land.** gentle, medium or strong (§5), or in between? Should the
   noise's character survive (gentle) or should the land read as eroded (medium, strong)?
3. **Static vs flowing water.** Should rivers be still water at a sloping surface (stepped, like
   the lakes) or flowing water with currents (a new fluid behaviour)?
4. **Generation time budget.** Is ~1 s on the server at world creation and ~1.5 s on a client's
   first join acceptable, or should clients receive the map?
5. **Face B's relationship.** Today face B has its own seed and its own island. Should its rivers be
   independent (as in the prototype), mirror face A's, or connect to them (through cross-face caves
   or over the rim)?
