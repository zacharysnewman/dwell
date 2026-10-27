# Proposal: terrain hydrology and terrain volumes

> **Status: layers 0–1 adopted (ADR 0049, 2026-10-21, generator 21):** the hydrology map is the
> terrain (ARCHITECTURE.md §6.3, WORLD_GENERATION.md §4). Layer 2 (the volume) is prototyped as the opt-in
> world type `cliffs` (generator 101, 0.31.4; WORLD_GENERATION.md §5): cliffs and overhangs only, the LOD the
> top surface only; the rest of §7 is still a proposal.
> The paragraph below is the prototype's history.
>
> **Earlier status: proposal, not adopted.**
> A **prototype of the hydrology layer is built** (`crates/dwell-worldgen/src/hydrology/`,
> `shared/worldgen/hydrology.json`, the viewer `cargo run --release -p dwell-worldgen --example
> hydrology`), with an **uplift layer** (mountain ranges that drive the erosion, §4.1), so river
> and lake layouts can be judged as images. It is **not wired into chunk
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
| A larger-scale input | *"Yes prototype mountain range/uplift layer."* (§4.1) |
| River scale | *"Ideally I'd like the river count/big rivers to be emergent with the terrain."* Rivers are not placed: their number and size follow from the uplift layer's drainage areas. |
| Static vs flowing water | *"Moving water"*: rivers are flowing water (a new fluid behaviour when wired in, §10). |
| Face B's relationship | *"No water doesn't connect for now."* Each face's hydrology is its own (as built); no water crosses between faces. |

Still open (§10): how much erosion reshapes the land, the generation time budget, and how big
rivers should be at most.

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
   bracketed Newton iteration otherwise. Uplift `U` comes from the uplift layer (§4.1) when the
   profile has one; otherwise it is `uplift` where the original ground is at least
   `upliftReference` m high and in proportion below, so the mountains stay where the height field
   put them. K is per cell (lower in hard-rock bands, §4.1). Cells under a depression's water are not eroded: they take the uplift and fill
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
   vocabulary, `uniform` or `orographic`, §4.1). Discharge is in m³/s from `rain.amount` (m/yr). D∞ or multiple-flow-direction routing
   would smooth the accumulation image but not the rivers, which need one path; not done.
5. **Rivers** (`rivers.rs`). Dry cells with discharge ≥ `rivers.threshold` m³/s, traced as polylines
   from each source (a river cell no river flows into, or a lake's outlet) down the receivers to the
   sea, a lake, or the river they join. Each vertex carries its surface height, discharge, and
   width and depth from the `geometry` curve (piecewise linear in discharge, held beyond its ends).
   Where a river of at least `mouths.minDischarge` meets the sea, its mouth is marked: a **delta**
   where the slope `mouths.rule` measures over `mouths.reach` m is at most `deltaMaxSlope`, an
   **estuary** otherwise (`riverSlope`: the river's last `reach` m, the original rule; `coastRelief`:
   the highest land within `reach` m of the mouth, §4.1).
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

### 4.1 The uplift layer (built)

`hydrology/uplift.rs`, after Cordonnier et al. 2016, where the uplift map is the main input of the
stream-power erosion and the mountains emerge where it is high. Per seed and face (seeded from the
face's generator seed, so the mirror world's face B is face A's), a map of **tectonic strength**,
0 on the coast's shelf and about 1 on a range's spine, drives three things:

- **the initial ground**: `keepRelief` × the sampled height field + `baseHeight` × strength (the
  height field's coast and sea stay; its egg-crate hills are mostly removed, 0.15 kept);
- **the uplift rate**: `rate` × strength (m/yr), so the ranges keep rising while rivers cut them;
- the coastline is unchanged (the sea is the height field's).

Its parts are data (`uplift` in a profile; each optional):

- **plains**: a floor strength everywhere inland;
- **ranges**: `count` long curved spines per face. A spine's centre is drawn between `minCoast` and
  `maxCoast` m from the sea; its heading turns from a random one toward the coast's direction by
  `align` (an active-margin range runs along its coast); it grows both ways in 500 m steps whose
  heading wanders (`wander`, radians per km, a damped random walk) to a length between
  `length.min` and `length.max`. Across the spine the strength is a smooth bump of half-width
  `width`; along it, the ends taper over a quarter of the length; a domain warp (`warp`) roughens
  the flanks. Each range draws 75–100 % of `strength` and 80–120 % of `width`;
- **apron** (foothills): beyond a range's flank the strength falls from `apron.strength` × the
  spine's to 0 at `apron.width` m, concave. This is what makes long rivers: the land tilts away
  from the mountains for tens of kilometres, so the drainage gathers into a few trunk valleys
  between the ranges instead of running radially to the nearest coast;
- **highlands**: low-frequency noise above a threshold (`cover`), broad plateaus;
- **plates**: `count` Voronoi cells of random points over the land, each with a random motion; at
  the boundary between a point's nearest two plates the strength is the closing speed × a bump of
  half-width `width` (`strength`; the apron too), and an opening boundary **sinks** (`rift`, a
  negative uplift): rift valleys that hold lakes;
- **shelf**: everything fades to 0 at the coast over a width between `shelf.min` and `shelf.max`
  that varies along the coast by noise: narrow shelves are steep coasts.

The parts combine by maximum (rifts add). Also new, independent of the uplift layer:

- **rock bands** (`rockBands`): parallel bands of hard rock (a random strike per seed, warped)
  covering `hardShare` of the land, with erodibility lower by `contrast`: rivers cut gorges through
  them and they stand as ridges and cliffs;
- **orographic rain** (`rain.model: "orographic"`): the relative rain on a land cell is
  1 + `upslope` × its rise into the wind over a kilometre − `shadow` × (the highest ground within
  `fetch` m upwind above it ÷ `shadowHeight`, at most 1), kept within [`min`, `max`]; the wind
  comes from `direction` turned per seed by up to ± `spread` degrees; computed once from the
  initial ground;
- **mouths by coastal relief** (`rivers.mouths.rule: "coastRelief"`): a big river's mouth is a
  **delta** where the highest land within `reach` m of it is at most `deltaMaxSlope` × `reach`
  above the sea (a low coastal plain, a wide shelf), an **estuary** where the valley is cut into
  high ground (a ria). The river's own slope (`riverSlope`, the old rule) cannot tell them apart:
  a big river is graded nearly flat at its mouth everywhere (all measured slopes < 0.001).

Determinism as before: hashed draws, the integer-lattice noise at the cells' integer centres,
only +, −, ×, ÷ and `sqrt`; the wind's direction by a Taylor-series `sin_cos` (`math.rs`). The
digests of the default and both variants are pinned on both faces (`tests/hydrology.rs`) and pass
under `wasm32-wasip1`; the full-disc digests of all 32 runs below (4 seeds × 2 faces × 4 profiles)
match between native and WASM.

The profiles: **`uplift`** (the new default: 3 coast-aligned ranges of 25–45 km, a 35 km apron,
highlands; medium erosion's K with 25 × 50 kyr steps; lake fill 0.03), and two variants,
**`uplift_plates`** (6 plates with rifts instead of ranges) and **`uplift_rock_rain`** (`uplift`
plus rock bands and orographic rain). `gentle`, `medium` and `strong` are unchanged (their
digests too): the height field's uplift, for comparison.

### 4.2 The viewer

```
cargo run --release -p dwell-worldgen --example hydrology -- --seed 7 --face a --out DIR \
  [--profile medium] [--seed …] [--face …] [--profile …] [--set key=value --label NAME] [--time-only]
```

prints the timing and statistics per seed, face and profile, and writes PNG layers cropped to the
land at one pixel per cell: `original` (hillshaded), `eroded` (hillshaded over a sea-level colour
ramp), `flow` (log discharge), `rivers` (drawn by width, exaggerated to read at 100 m a pixel: radius
0.3 px + width ÷ 40 m), `widths` (every river by width, coloured by discharge from pale blue to
plum, the big ones drawn last), `lakes` (with outlets in red), `uplift` (the uplift rate over the
hillshade, the ranges' spines in black, the plates' points in white), `rain` and `rock` (where
they vary) and `map` (the composite, with deltas orange and estuaries magenta). The statistics
include the largest river system (its outlet's discharge, its main stem's length from source to
sea and the lakes on it), the systems over 5, 10 and 20 m³/s and the delta share. `--set`
overrides a tunable for tuning without editing the data.

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

**Recommendation (before the uplift layer):** `medium` (25 iterations of 25 kyr, K = 4 × 10⁻⁶), as
`hydrology.json`'s default: lakes "sometimes", organised rivers, the relief kept (about −10 to
−20 % at the top), and ~1 s natively, ~1.5 s in WASM. Before wiring it in, give layer 0 a
larger-scale input: a mountain range or two (an uplift map as data) so rivers run from mountains to
the ocean over tens of kilometres, which is what the owner asked for and what the noise alone does
not have.

### 5.1 With the uplift layer

The same seeds and faces, full disc at 100 m; ranges over the 8 seed-faces (contact sheets per
seed, not checked in: regenerate with the viewer):

| Profile | Largest river | Its main stem | Systems ≥ 5 / ≥ 10 / ≥ 20 m³/s | Lakes (on big rivers' stems) | Highest ground | Deltas : estuaries |
|---|---|---|---|---|---|---|
| medium (height field) | 7.0–10.8 m³/s | 37–72 km | 1–3 / 0–1 / 0 | 2–16 (0) | 399–449 m | 16 : 0 |
| **uplift** (default) | **10.3–21.5 m³/s** | **64–79 km** | 1–3 / 1 / 0–1 | 0–9 (0–6) | **768–906 m** | 7 : 9 |
| uplift_plates | 12.0–18.0 m³/s | 42–77 km | 2–3 / 1 / 0 | 1–15, up to 170 km² (1–5) | 406–775 m | 7 : 5 |
| uplift_rock_rain | 9.6–20.1 m³/s | 65–80 km | 1–3 / 0–1 / 0–1 | 0–12 (0–3) | 844–1,007 m | 1 : 15 |

(1 m/yr of rain; a main stem is the D8 path of the largest donor upstream from the outlet, through
lakes, so it meanders: about 1.5–2 × the straight distance from source to sea.)

- **Mountain ranges emerge as intended**: two or three snow-capped ranges per face, mostly along a
  coast, 770–900 m high, cut by dendritic valleys; foothills fall away from them across the
  island.
- **Bigger, longer rivers, emergent**: the largest river doubles (10–21 m³/s against 7–11) and runs
  65–80 km from a range to the far coast, collecting the drainage of the basin between the ranges.
  Every face has exactly one system over 10 m³/s, and seed 1234 face A one over 20. The count is
  not set anywhere: it follows from where the ranges fall.
- **The ceiling is the island**: 2,000 km² of land at 1 m/yr of rain is ~63 m³/s in all, so the
  largest river already drains a quarter to a third of a face. "A few" rivers over 10 m³/s per
  face needs more rain (orographic rain helps the windward side; `rain.amount` scales all
  discharge linearly) or more land, not a different erosion.
- **Lakes on the way**: intermontane basins hold lakes when the apron rims close them (seed 7 face
  B: three lakes on its main stem; 1234 A: six). The **plates** variant's rifts make the best
  lakes: long rift lakes up to 170 km² that the big rivers pass through on every face.
- **Deltas and estuaries now both occur**: big rivers reaching the far, low coast end in deltas;
  those reaching a steep coast (a narrow shelf, or through a hard-rock band) end in estuaries. Rock
  bands raise the coastal relief, so `uplift_rock_rain` is mostly estuaries.
- **Rock bands** read as parallel ridges and gorges; **orographic rain** is mild at these settings
  (a wet windward flank of each range, a dry lee), worth tuning when wired in.

**Recommendation:** `uplift` as the default (made so in `hydrology.json`), with the plates' rifts
folded in later if lakes on the big rivers should be common rather than occasional.

## 6. Cost

Full disc, 1,002 × 1,002 cells of 100 m, one thread, measured on the owner's Mac (load average
~3 during the runs; release builds; the WASM build under Node's WASI, `tools/wasi-run.mjs`):

| Profile | Native | WASM | of which sampling the height field |
|---|---|---|---|
| gentle | 0.66–0.70 s | 0.89 s | 0.30 s |
| medium | 1.00–1.07 s | 1.45 s | 0.30 s |
| strong | 1.51–1.68 s | 1.88 s | 0.31 s |

With the uplift layer (the same machine, load average 6–12 during these runs, so the native times
are noisy; the median is given):

| Profile | Native (median) | WASM | of which sampling |
|---|---|---|---|
| medium | 1.13 s | 1.22–1.47 s | 0.31 s |
| uplift | 1.22 s | 1.30–1.48 s | 0.31 s |
| uplift_plates | 1.14 s | 1.24–1.34 s | 0.31 s |
| uplift_rock_rain | 1.29 s | 1.44–1.56 s | 0.31 s |

The uplift layer itself (the coast distance transform, the ranges' distances over the land, the
noise) costs ~0.05–0.1 s; orographic rain another ~0.1 s.

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

1. **River scale (partly answered: emergent).** With the uplift layer the largest river is
   10–21 m³/s (~12–22 m wide on the width curve) and one per face exceeds 10 m³/s; the island's area and the rain
   cap it (§5.1). Should the rain be heavier (2 m/yr doubles every river), or the land larger, for
   a few big rivers per face? Should the biggest be navigable?
2. **How much erosion reshapes the land.** gentle, medium or strong (§5), or in between? Should the
   noise's character survive (gentle) or should the land read as eroded (medium, strong)?
3. **Static vs flowing water — answered: moving water.** Rivers are flowing water with currents:
   a new fluid behaviour (and its rendering, WATER_RENDERING.md) when the map is wired in.
4. **Generation time budget.** Is ~1 s on the server at world creation and ~1.5 s on a client's
   first join acceptable, or should clients receive the map?
5. **Face B's relationship — answered: no connection for now.** Each face's hydrology is its own
   (face B its own seed, island, ranges and rivers); no water crosses between the faces.
6. **Lakes on the big rivers.** Occasional with ranges alone (2 of 8 faces), common with the
   plates' rifts. Fold rifts into the default?
