# Proposal: a seed-driven Realistic world

**Status:** written 2026-11-11, not adopted. Questions for the owner in §7 (q1, q3, q4, q5 and q7 answered
2026-11-11). R1 built: the plate layout (`shared/worldgen/plates.json`, `dwell-worldgen` `plates.rs`). R2
built (2026-11-12): uplift from the boundaries (`plate_tectonics.rs`, `plates.json` `uplift`) in the
Realistic test world (`shared/worldgen/worlds/realistic.json`, `?worldgen=test:realistic`, not in the New
world form); the default and the starter are unchanged. R3 built (2026-11-12): the climate from latitude, wind
and terrain (`shared/worldgen/climate.json`, `dwell-worldgen` `climate.rs`) in the Realistic world, its rain
feeding the rivers' discharge. R4 built (2026-11-12): biomes chosen by climate alone, the presets' `where` in
°C and annual rain (mm), the Realistic world listing every preset unchanged. R5 built (2026-11-12): rocks from
plate history (`rocks.json` `plateProvinces`) and soils by rock × climate (`vegetation.json` `rockSoils`). R6 built
(2026-11-12), its exit met: Realistic in the New world form's World list (`newWorld`), its own golden hashes, the
big map's debug layers (plates, uplift, rain, rocks); created, played and reloaded in the browser on the M1's GPU,
the phone page's total at load under the starter's (R6 below). Steps by geology (2026-11-13, 0.57.0, WORK_QUEUE item
15): the global `cliffs` volume off and the rock shaping on in Realistic worlds, so the ground steps only where its rock's
jointing says (R5 below). Realistic batch A (2026-11-13, the 0.64 line): landforms by plate pair, segmented rifts, no
sliver plates, ranges varied along their line, and an open rim (§6, after R6). Variety (2026-11-13, WORK_QUEUE item 15, the realistic batch part B): mixed provinces in the
plates' settings (`rocks.json` `plateProvinces.mixing`, each rule's `mix`), regional and local climate noise
(`climate.json` `variety`), basins and swells and turned, warped hills (`plates.json` `variety.relief`) and a
randomized erosion routing in place of D8's grid streaks (`plates.json` `variety.flow`), measured by
`dwell_worldgen::variety` (`tests/realistic_variety.rs`). §8 (2026-11-12, design only):
orbits by seed set the latitude band, the sky and later seasons, in place of R3's picked band.

## 1. The owner's direction

Owner, 2026-11-11: "The realistic world should come from our systems (which may not be properly defined
or built yet to support this) the seed should determine tectonic plates, rain shadow, ground/rock
materials, etc."

The owner's answers to the first questions:

- **Scale:** one region to start: a few plates meeting on the map, e.g. a coastal range, its rain
  shadow, one or two climate zones. "We will test out a 10x size map soon", so nothing here may rule it
  out.
- **Climate:** latitude + prevailing wind + terrain. Temperature from a seed-chosen latitude band and
  the altitude; rain from the winds and the rain shadows over the seed's own mountains.
- **Geology:** from plate history. Volcanic arcs → basalt/andesite; uplifted ranges → granite and
  metamorphic rock; basins → sandstone/shale; old seafloor → limestone. Soils from rock + climate.
- **Worlds:** the starter (`test:starter`) stays a hand-placed test bed. Realistic becomes a new World
  choice; later perhaps the default, on a new compatibility line.

The idea in one line: the starter world shows that the systemic layers (ADR 0075 T1–T5) can make a
convincing place when a person places every part. Realistic is the same layers with the seed doing the
placing, so the parts follow from each other instead of from a JSON file.

**Goal (owner, 2026-11-14):** "realistic only. We're hoping to replace default eventually with realistic once it's
looking really good." The Realistic world is to replace the default world once it looks really good; until then its
look changes are Realistic-only (WORK_QUEUE item 15).

## 2. What exists today: hand-placed or seed-derived

"Hand-placed" means a number in a world file says where or what; "seed-derived" means the seed draws it;
"rule" means it follows from other fields by a rule (no placing, no draw).

### 2.1 Plates and boundaries

| Part | Today | Where |
|---|---|---|
| The one plate boundary (line, angle, offset, kind, range height) | Hand-placed, one per world | `terrain.json` `tectonics` (`kind` 0 off in the default), `worlds/starter.json`, `tectonics.rs` |
| Trench, rift, transform offset | Hand-placed with the boundary | `tectonics.rs` |
| Uplift layer's ranges (random-walk spines), highlands | Seed-derived, but not plates: random spines with no motion or history | `hydrology.json` profile `uplift` (the default), `hydrology/uplift.rs` |
| Uplift layer's `plates` (Voronoi points with random motions, convergent rise, divergent sink) | Seed-derived, prototype profile `uplift_plates` only; not used by any world | `hydrology/uplift.rs` `plate_points` |
| Range keeps rising against erosion | Rule (boundary's uplift `rate`) | `hydrology/mod.rs` `generate_with_height` |

The seed already draws plates in the prototype profile, but they drive only the erosion's uplift, not
the arcs, the rock or the climate. While `tectonics` is on, the uplift layer's ranges and plates are off.

### 2.2 Cones and hotspots

| Part | Today | Where |
|---|---|---|
| Arc cones (count, distance behind the line, spacing, height, decay, Rainier shape) | Hand-placed relative to the boundary | `terrain.json` `tectonics` `arc*`, `cone*`, `ridge*`, … |
| Column detail on cones (cleavers, crags, cirques) | Count hand-placed; positions seed-derived per map | `Tectonics::peak_details` |
| Hotspot chain (start x/z, heading, spacing, count, decay) | Hand-placed | `tectonics` `hotspot*` |
| Lava vents | Hand-placed per cone | `tectonics.lavaVents`, VOLCANOES.md §6 |

### 2.3 Climate

| Part | Today | Where |
|---|---|---|
| Temperature | A 0..1 gradient across the island along a **seed-drawn direction**, bent by noise, minus a lapse rate; °C by a linear scale | `biomemap.rs` `geography`, `terrain.json` `biomeMap`, `vegetation.json` `climate` |
| Latitude | None: no band, no poles, no equator; the gradient's ends mean nothing | — |
| Prevailing wind | Hand-placed: one `windDirection` per world | `terrain.json` `biomeMap` |
| Rain and rain shadow | Rule: the wind marched over the map's smoothed ground (T1) | `biomemap.rs` `rainfall`, `smooth_ground` |
| A second rain model (the erosion's discharge) | Rule, separate data and code: `orographic` in one prototype profile | `hydrology/rain.rs`, `hydrology.json` `uplift_rock_rain` |
| Wetness | Rule: rivers/lakes distance, plus rain × `rainShare` | `biomemap.rs` |

Two rain models that don't know about each other, and a wind that is world data, not the seed's.

### 2.4 Biomes

| Part | Today | Where |
|---|---|---|
| Which biomes a world has | Hand-placed list | `worlds/starter.json` `biomes`, `biomes/presets/*.json` |
| Where each biome sits | Rule over the geography (`where`: temperature, wetness, elevation, coast) | `biomemap.rs` `raw_weight`, `fallback_weights` |
| Per-world tuning of each biome's `where` | Hand-placed (the starter retunes most presets' `where` so each of its biomes gets ≥ 2 % of the land, `tests/starter_world.rs`) | `worlds/starter.json` |

The biome map is already a rule; the variety is forced by tuning each world's list.

### 2.5 Rock and ground materials, soils

| Part | Today | Where |
|---|---|---|
| Rock types, stacks | Data (5 rocks, 6 stacks) | `rocks.json` |
| Provinces (which stack where) | Seed noise by weight, ignoring tectonics | `rocks.json` `provinces`, `geology.rs` |
| Tectonic provinces | Rule from the hand-placed boundary: volcanic near cones, granite core behind the line | `rocks.json` `tectonic`, `geology::Resolved::with_tectonics` |
| A biome's own stack | Hand-placed (`rockStack` in the mesa and badlands presets) | `biomes/presets` |
| Soil type and depth | Rule from rock + slope + wetness | `vegetation.json` `rockSoils`, `soil` |
| Surface blocks | The biome's bands; soil blocks opt-in | biome files, `soilBlocks` |

No metamorphic rock, no andesite, no sea-floor history; climate does not enter the soil type.

### 2.6 Rivers and drainage

| Part | Today | Where |
|---|---|---|
| Uplift, stream-power erosion, lakes, rivers | Rule from the height field and uplift (seed-derived) | `hydrology/*`, `hydrology.json` |
| Rain into the discharge | Uniform in the default profile | `hydrology.json` `rain` |
| Drainage lines for T4 erosion | Rule from the map | `geology::DrainageField` |

Already the most systemic part. The gap is the rain: the rivers should carry the climate's rain.

### 2.7 Trees and vegetation

| Part | Today | Where |
|---|---|---|
| Species (oak, pine, palm) and their climate envelopes | Data | `shared/trees/*.json`, `vegetation.json`, ADR 0079 |
| Where trees grow | Rule (climate envelope × biome's `trees`) | `vegetation.rs`, `placements.rs` |
| Which placements a biome lists | Hand-placed per world | `worlds/starter.json` |

Close to ready: the climate already thins trees; only the per-world lists are hand-made.

## 3. The gaps: what each system needs

Each is an extendable system (CLAUDE.md): **its vocabulary in code** (the node kinds and rules, small
and fixed), **its tuning in data** (`shared/worldgen/*.json`, checked by `biomes.mjs`, emitted by
`gen.mjs`, CI failing on stale output), referenced by name, tested by adding a fixture entry by data
alone. All work face-local and on both faces (face B is A's mirror, each face its own seed draw).

### 3.1 A plate layout generator (new, `plates.json`)

- **From the seed:** a plate count (a range in data, e.g. 3–5 for a region), each plate's kind
  (`oceanic` | `continental`), a centre, a motion vector, and an age. Plates as a warped Voronoi (or
  power diagram) over the map, so boundaries curve.
- **Boundaries follow by rule:** for each pair of neighbours, the relative motion across the boundary
  gives `convergent` | `divergent` | `transform`, and for convergent which side subducts (oceanic under
  continental; the older under the younger when both are oceanic; a collision range when both are
  continental).
- **Hotspots:** a seed count from data, each a point fixed under the moving plate; the chain follows
  the plate's motion (today's `hotspotHeading` becomes the plate's own vector).
- **Vocabulary in code:** plate kinds, the three boundary kinds and their landforms (today's `tectonics.rs`
  range, trench, rift, transform, arc cones, shields). **Data:** counts, size ranges, speed ranges, the
  probabilities of each kind, and each boundary kind's profile (today's `rangeHeight`, `width`,
  `trenchDepth`, `arcDistance`, … become per-kind entries, scaled by convergence speed).
- **Output:** a small vector description (plates, boundary polylines with kind and speed, hotspots),
  not a grid. This is cheap at any map size (§4).
- **Replaces:** the uplift layer's `plates` and random spines for this world, and the one hand-placed
  boundary. `tectonics.rs` learns to take many boundary segments instead of one line (today's "Not yet:
  one boundary and one chain").

### 3.2 Uplift from boundaries

- The strength field the erosion reads (`hydrology/uplift.rs`) is a sum over boundary segments by their
  kind's profile, faded by distance, plus a plate's base level (continental high, oceanic low). The
  coast then **emerges** from the plates (oceanic plate = sea) instead of from `islandRadius`'s disc
  taper, within the disc.
- Open: whether the island taper stays as a floor (the disc must still end in sea) — §7 q3.

### 3.3 A climate model (`climate.json`, folding `biomeMap`'s climate keys)

- **Latitude band:** the seed picks a band (data: a list of named bands with weights, e.g. subtropical,
  temperate, subarctic, each a latitude range). The map spans part of it along a seed axis (north).
  Sea-level temperature by latitude from a data curve (°C per degree), minus the lapse rate by altitude.
  This replaces today's unitless 0..1 gradient with a meaning.
- **Prevailing wind by latitude:** data cells (trade winds, westerlies, polar easterlies), each a
  latitude range and a direction; the band picks the wind. A seed jitter of a few degrees. `windDirection`
  stays as an override key for test worlds.
- **Orographic rain and rain shadow:** today's T1 march (`biomemap::rainfall`) on the seed's heights, now
  with a real wind. Optional later: moisture by sea temperature (warm sea, wetter air), a coast effect.
- **One rain field:** the climate's rain also feeds the hydrology's discharge, so rivers are big on the
  wet side and the lee's rivers are fed by the range. The two models (`hydrology/rain.rs` and
  `biomemap::rainfall`) become one. This changes the river layout, so it is Realistic-only (§5).
- **Order problem:** rain needs the heights, the heights need erosion, erosion needs rain. Proposal: rain
  from the **uplifted, uneroded** ground (smoothed over `windSmoothM` as today) feeds the erosion; the
  biomes' wetness then uses rain re-marched on the eroded map. Two cheap passes, no loop.

### 3.4 A geology layer from plate history (extends `rocks.json`)

- **Rock provinces by rule from the plates**, replacing the provinces' noise for this world: within an
  arc → volcanic (basalt, andesite: new rock types); a collision or uplifted core → granite and a
  metamorphic rock (schist/gneiss: new); a basin (low strength, between ranges or a rift floor) →
  sandstone/shale beds; old oceanic crust lifted onto land (an accreted strip at the trench side) →
  limestone. The province's noise stays as variation inside a province.
- **Data:** `rocks.json` gains `plateProvinces`: a list of rules, each a setting (`arc`, `core`,
  `basin`, `rift`, `accreted`, `hotspot`, `default`) → a stack by name (with weights). The settings are
  the code's vocabulary; the stacks and rocks are data.
- **Strata:** the stacks already repeat with height; add a tilt/fold near a convergent boundary (a dip
  angle from the boundary's speed) later, not in the first slice.
- **Soils from rock + climate:** `vegetation.json` `rockSoils` gains a climate axis: a soil rule is a
  rock (or rock class) × a temperature and moisture range → a soil type, depth and blocks (e.g. granite +
  wet + cool → thin acidic soil; shale + dry → clay; limestone + wet → deep loam). Today's rock-only
  table is the case of one climate range.
- **Blocks (owner, 2026-11-11, §7 q5):** no new rock blocks for now. The settings map to today's blocks:
  stone for collision and uplifted cores, basalt for arcs and hotspots, sandstone and red sandstone for
  basins and rift floors, sand and dirt soils; the accreted strip's limestone and the metamorphic rock are
  stone until blocks exist. Every province names its rock by block name in data (`rocks.json`), so a later
  andesite, schist or limestone block replaces one by data alone (adding blocks is not breaking, ADR 0038).

### 3.5 Biomes chosen by climate only

- A Realistic world lists **all presets** (or a named set in data), unchanged, and lets their `where`
  pick by temperature (°C), rain/moisture, elevation and coast. No per-world retuning to force variety:
  a cold band may have no desert, which is the point.
- The presets' `where` move from the unitless 0..1 temperature to °C (Whittaker-style: temperature ×
  annual rain), so one set of ranges means the same in every band. Today's 0..1 stays readable for the
  starter by its `perUnitC`/`zeroC` scale.
- The default biome stays the fallback where nothing fits.
- Data: an optional `climateSet` (a name → the biome list) so a world says which set, not which tuning.

### 3.6 Lava vents

- Vents placed by rule on the youngest hotspot island and on arc cones by a seed chance in data
  (VOLCANOES.md's states as data). The first Realistic world can leave them off (§7 q6).

### 3.7 What stays the same

The world map, the stream-power erosion, lakes and rivers, T2 shaping, T4 rule-based erosion, the
column detail on cones, the tree species, the chunk generator and the LOD read the same fields as now.
Realistic changes **where the inputs come from**, not how the terrain is built from them.

## 4. Memory and cost

Today's whole-map grids per process, one face (1× disc, 100 m cells, 1002² ≈ 1 M cells):

| Grid | Size today (1×) | At 10× (100 M cells at 100 m) |
|---|---|---|
| World map build (height, flow, lakes, erosion scratch) | ~143 MB peak, ~158 B a cell | ~16 GB: impossible in a browser |
| Encoded world map (stored) | 7.1 MB | ~710 MB a face |
| Biome field (coast and fresh-water chamfer grids, rain) | part of ~60 MB a face for the starter (BUGS.md #70) | ~6 GB |
| T4 drainage field | in the same ~60 MB | ~6 GB |

BUGS.md #70: the starter loaded at 1,467 MB on a phone page, over iPhone Safari's ~1–1.5 GB, because
every generator built its own biome set; fixed to 850 MB by sharing one set per process
(`terrain::shared_biomes`). The server and each of the two world workers still hold a copy. WORLD_SCALE.md
measured the 10× map as out of reach at 100 m cells everywhere.

What Realistic adds, and how each part stays possible at 10×:

- **Plate layout:** vectors (a handful of plates, polylines, hotspots), kilobytes at any scale. No grid.
- **Uplift strength:** evaluated per cell from the vectors; it needs no stored grid beyond what the
  world map builds anyway.
- **Climate:** temperature is a function (latitude, height); no grid. Rain needs the march over a grid,
  but the rain shadow is regional (smoothed over 3 km already), so it can run on a **coarse grid**: 500 m
  cells at 1× (~200²), 1–2 km cells at 10× (250–500², under 1 MB). Bilinear when read.
- **Geology:** provinces by rule from the plates (distance to a boundary, the setting) are functions; no
  grid.
- **Biome field and drainage:** these are the costly grids already. Realistic adds none, but the 10× map
  needs them **tiled and on demand**: regional maps around players (WORLD_SCALE.md's recommendation), a
  coarse whole-world map (1–2 km cells) for the big map and far LOD, fine 100 m tiles built when a
  region loads and dropped when it is far.

The design rule for Realistic: **every new layer is either a function of vectors or a coarse grid
(≥ 500 m cells) that can be tiled.** No new 100 m whole-map grid. Then the 10× work is only the
existing grids' problem, and Realistic does not make it worse. A test pins it: the Realistic world's
extra memory over the default at 1× (the native `world_scale` example's peak) stays under a bound in
data, e.g. +20 MB a face.

## 5. Compatibility

- **A new generator choice.** Realistic is a new world config, `realistic` (a fixture-style world file
  `shared/worldgen/worlds/realistic.json` with `newWorld` so it shows in the New world form, as the
  starter does), saved with the world through the `tag::WORLDGEN` hash (ADR 0074 B7). Its knobs say "draw
  from the seed" (e.g. `plates: { "from": "seed" }`) instead of placing parts.
- **The default world is untouched:** every new layer ships neutral in `terrain.json`, `rocks.json` and
  `vegetation.json`; the worldgen goldens (`shared/worldgen/golden/chunk-hashes.txt`, `lod-hashes.txt`)
  stay byte-identical, as for T1–T5.
- **Its own goldens:** a hash file for Realistic (a few seeds, chunks and LOD sections round the spawn),
  so any change to what a Realistic seed generates is visible in review. While Realistic is offered as a
  test world (like the starter) changes to it are allowed and the hashes move with a note; once players
  keep Realistic worlds, a change to it needs a new compatibility line like any worldgen change.
- **Becoming the default** later is a new compatibility line the owner raises (`package.json`,
  `shared/version/line.json`), not part of these phases.
- **Both faces:** each face draws its own plates from its face seed (as the world map does), so face B is
  a different region on the mirrored frame, unless the owner wants the plates shared through the
  midplane (§7 q4).

## 6. Phases (each ≤ 45 minutes)

Each phase is native first (tests, the `world_scale` example), then one Chisel look. Chisel is where the
debug views go (an overlay and sliders), since it already runs the generator's WASM on the whole disc.

- **R1 Plate layout.** `plates.json` (+ validation, `gen.mjs`, `plates.gen.rs`), `dwell-worldgen`
  `plates.rs`: seed → plates (kind, motion, age), boundaries with kinds, hotspots. Chisel: a plates
  overlay (plates tinted by kind, boundaries coloured by kind, arrows for motion, hotspots).
  *Exit:* same seed same layout, different seeds differ; every boundary's kind matches its relative
  motion (unit test over a fixture of two plates by data alone); counts within the data's ranges; a bad
  entry fails with a message; both faces; default goldens unchanged.
  **Built (2026-11-11):** `plates.json` (count 3–5, reach 25 km with a 2 km sea margin, spacing, the kinds'
  weights and minimums, speed in cm/yr, age in Myr, the warp, `transformBelow`, hotspots, and two-plate
  `fixtures` with the boundary each must give), checked by `gen.mjs` `validatePlates` (a bad entry fails
  naming it) and emitted as `plates.gen.rs`. `plates.rs`: `PlateLayout::for_face(world_seed, face)` draws
  from the face seed; the plates are a warped Voronoi; the boundaries are found on a 500 m scan (`SCAN_CELL`)
  as sample points per neighbour pair, each classified by `classify` from the relative motion across the
  centres' line (closing → convergent, opening → divergent, under `transformBelow` of the relative speed →
  transform; oceanic under continental, the older of two oceanic plates, none for two continents);
  hotspots fixed under a plate with the chain's trail against its motion; `is_land` from the plate kind
  inside the sea margin. Every exit criterion is a test (`plates::tests`, `gen.test.mjs`); the goldens are
  unchanged (nothing reads the layout yet). *Deviations:* the boundaries are sample points in scan order,
  not ordered polylines (R2 orders them when `tectonics.rs` takes segments); a boundary's kind is from its
  plates' centres' line, its mean normal, not the warped local normal. The Chisel plates overlay moved to
  R2 (it needs a WASM export and a Chisel view; not quick).
- **R2 Uplift from boundaries.** `tectonics.rs` takes many segments; the uplift strength from the layout;
  arc cones and hotspot chains placed by rule. Realistic world file added (not yet in the New world form).
  *Exit:* for seed fixtures, ranges stand along convergent boundaries above the plain map, rifts below,
  arcs on the overriding side, hotspot chains older along the motion; the map builds in ≤ 1.3× the
  default's time; goldens unchanged.
  **Built (2026-11-12):** `terrain.json` `tectonics.plates` (0 off; 1 the face seed's layout) turns it on;
  `worlds/realistic.json` (`test:realistic`) is the one world that does. The plates decide land and sea:
  the base field's coast is the layout's signed coast distance (`PlateLayout::coast_distance`: the warped
  Voronoi bisector to the nearest plate of the other kind, capped by the sea margin) in place of the
  island's, so the inland rise, hills and shelf follow the plates. `Boundary::polylines` orders R1's scan
  points (nearest within `link`, the scan's staircase absorbed, thinned to `segment` m). `plate_tectonics.rs`
  adds every boundary's landform by rule (`plates.json` `uplift`): a range on a subducting boundary's
  overriding side with a trench on the diving side, a collision range astride two continents, a rift with
  shoulders on a divergent one, a low ridge on a transform, each scaled by the boundary's rate over
  `speedRef` (clamped to `speedScale`), only on the boundary's own two plates, summed on a 500 m coarse grid
  (`cell`) and read bilinearly (§4: no new 100 m grid). Arcs: one dormant cone per `spacing` m of each
  subducting polyline ≥ `minLength` m (≤ `maxCones`), `distance` m off the line on the overriding plate,
  hardened by `hardness`. Hotspot chains: `islands` shields along the plate's motion, older ones lower and
  wider. The map adds them as T5's boundary (the landform also as uplift; the uplift layer's random ranges
  and plates off). Tests: `plate_tectonics::tests` (polyline order, each kind's landform on the fixtures,
  the coarse grid against the exact field, chains), `tests/realistic_world.rs` on both faces (the plates'
  land/sea on the map, ranges ≥ 0.4 × their height above the plain map — the same plates with every landform
  0 — and land rifts ≥ 0.2 × their depth below it, arc cones on the overriding plate standing over it, chains
  older along the motion), `gen.test.mjs` (bad `uplift` entries). The map builds in 1.09 s against the
  default's 1.58 s (release, best of 3; a release-only test pins ≤ 1.3×): it skips the uplift layer's random
  ranges. Default and starter goldens unchanged. *Deviations:* R1's hotspot trail ran against the plate's
  motion; fixed to along it (a plate carries its older islands off the fixed hotspot, Hawaii). A rift inside
  a continent keeps about half its cut through the erosion (its lakes and deposits fill it), so the test's
  bar is 0.2 of its depth. A rift between an ocean and a continent is the coast itself, its sea side held by
  the map's floor. The plate coast has no fractal coast detail yet (smoother coasts than the island's). The
  chunk generator's own `BaseField` (only its noise offset is read) still uses the island; the portals'
  inland rule reads the island's coast in a Realistic world until R6. Lengths in `plates.json` are for the
  default size (BIG_MAP.md): the layout and uplift scale with the radius when the world size reaches the
  generators (M2), as the island's shape does. **Not done:** the Chisel plates overlay (a WASM export and a
  Chisel view; moved to R3 with the latitude and wind overlays).
- **R3 Climate.** `climate.json`: latitude bands, wind cells; temperature in °C from latitude and height;
  the rain march with the band's wind; the one rain field feeding the discharge (Realistic only). Chisel:
  latitude and wind overlays. *Exit:* the windward side of a seed's main range is ≥ 2× wetter than its
  lee; a colder band's sea-level temperature is lower; the default world's rain and rivers unchanged
  (goldens); the coarse rain grid's memory under the bound.
  **Built (2026-11-12):** `climate.json` (checked by `gen.mjs` `validateClimate`, a bad entry failing with
  its key named; emitted as `climate.gen.rs`): `bands` (subtropical 14–28°, temperate 38–52°, subarctic
  61–72°, by weight; a name, a weight and a latitude range each), a sea-level temperature curve by latitude
  (`seaLevel`, °C), `lapseRate` (6.5 °C/km), wind cells by latitude (`winds`: trade winds from 60°, westerlies
  from 240°, polar easterlies from 70°, compass bearings), `windJitter` (± 15°), the march's `rain` tunables,
  `maxGridBytes` (1 MiB) and the `discharge` factor's [min, max]. `climate.rs`: `Climate::for_face` draws a
  band, a north axis and the wind's jitter from the face seed; a band spans the disc from its south latitude
  at the south edge to its north one at the north edge (game scale); `latitude`, `sea_level_c` and
  `temperature_c` are functions (no grid); the band's wind is the cell at its middle latitude, turned with
  the map's north. The rain is today's T1 march (`biomemap::rainfall`) along that wind on a `CoarseGround`
  (≥ 500 m cells, at most `maxCells` 400 a side, so a bigger map coarsens the grid instead of growing it:
  ~2.5 km cells at 10×, ≤ 640 KB), over the uplifted ground before the erosion (§3.3's order). Where
  `terrain.json` `tectonics.climate` is 1 (only `worlds/realistic.json`) the hydrology weights the discharge by
  it (the rain over its land mean, within [0.05, 4]) in place of the profile's uniform rain; the coarse grid
  and rain live on the `HydrologyMap` only while the world map builds. Tests: `climate::tests` (same seed
  same climate, every band drawn, a colder band colder at sea level, the band spanning the map, the lapse
  rate, each band's wind, a band added by data alone, a ridge across the wind wet windward and dry in its
  lee, the grid's bound at 1× and 10×), `tests/realistic_climate.rs` on both faces (for seeds 1, 7, 42 the
  main range's windward side ≥ 4.8× wetter than its lee on the coarse grid and ≥ 3.6× in the discharge's
  rain; the map's grid under `maxGridBytes`; each face its own climate; the default world's rain uniform and
  no climate), `gen.test.mjs` (the emitted Rust, a band and wind added by data, bad entries); default and
  starter goldens unchanged; the Realistic map still builds faster than the default's (1.40 s against
  2.19 s, release). *Deviations:* the switch is a `tectonics` key (`climate`, beside `plates`), not a
  section of its own. One wind for the whole map (the band's middle latitude's), not a wind per latitude
  across it. The biome field's wetness and the snow line still read `biomeMap`'s wind and 0..1 climate: the
  Realistic world has only the default biome until R4, which re-marches the climate on the eroded map for
  the biomes (§3.3's second pass) and moves the presets to °C. The new rain changes Realistic seeds' terrain
  (allowed while it is a test world, §7 q7); a young hotspot island in the wind's wet air erodes faster (face
  B seed 0's keeps 90 of its 420 m), so R2's chain test measures the landform with the climate off. **Not
  done:** the Chisel plates, latitude and wind overlays (a WASM export of the layout and the climate and a
  Chisel view; moved to R4 with the biome overlay).
- **R4 Biomes by climate.** Presets' `where` in °C and rain; the Realistic world lists the presets
  unchanged. *Exit:* for a few seeds, biome shares follow the climate (no desert in a wet band's windward
  side; tundra above the tree line); no per-world biome tuning in `realistic.json`; the starter still
  passes `tests/starter_world.rs`.
  **Built (2026-11-12):** a biome's `where` gains two ranges, `celsius` (°C) and `rain` (annual mm; checked
  by `biomes.mjs` `WHERE_FIELDS`, read by `config.rs`, emitted by `gen.mjs`), and every preset's climate
  envelope moves to them, Whittaker-style (the presets keep no 0..1 `temperature`; `wetness` stays only as
  "away from rivers" on the dry ones and "by water" on the wetland; `elevation` and `coast` only for
  landforms: the beach, cliffs, wetland, mesa and badlands): snowfield ≤ −6 °C, alpine tundra −7 to −1.5
  °C (the tree line), boreal −1.5 to 6 °C with ≥ 250 mm, cold rainforest 4–12 °C with ≥ 1800 mm, mixed forest
  5–16 °C with ≥ 750 mm, temperate meadow 4–18 °C with 450–1000 mm, steppe 0–15 °C with 200–500 mm, sand
  desert ≥ 14 °C with ≤ 250 mm, badlands ≥ 10 °C ≤ 400 mm, red-rock mesa ≥ 12 °C ≤ 450 mm, savanna ≥ 19 °C
  with 500–2500 mm, tropical beach ≥ 18 °C, coastal cliffs ≤ 15 °C (each with its blend). In a world with a
  climate (`tectonics.climate` 1) `biomemap::BiomeField` reads both from it: the °C from latitude and height,
  the rain from the band's wind marched again over the **eroded** map on the coarse grid (§3.3's second pass;
  `climate.json` `bands[].rainMm` is a band's annual rain where the march rains its flat rate, more windward
  and less in the lee by the march's ratio, capped at `biomes.maxRainMm` 4500), and the 0..1 temperature and
  wetness the vegetation and snow lines read follow from them (`biomes.wetMm`, `moistureShare`); elsewhere
  `celsius` is the 0..1 temperature by `vegetation.json` `climate` and `rain` is not read.
  `worlds/realistic.json` lists `dwell:default` (the fallback) and all 14 presets as bare names. The
  starter keeps its 0..1 tuning: the three presets it took unchanged (tropical beach, sand desert, alpine
  tundra) now carry their pre-R4 `where` in `starter.json`, so its generated edit is byte-identical;
  default goldens unchanged. Tests (`tests/realistic_biomes.rs`, both faces, seeds 1, 7, 42): the world lists
  the default and every preset unchanged; its geography is its face seed's climate; no dry biome (desert,
  badlands, mesa) anywhere the rain is the band's wet-coast rain or more, and on the main range's windward
  side (≥ 2× its lee's rain) none at such points; above the tree line no forest and the tundra has weight,
  and the tundra is chosen only below it; on the same ground a colder band has more boreal/tundra/snowfield
  and fewer warm biomes (subarctic > 30 % cold); a fixture biome with only a rain envelope is placed by it.
  `gen.test.mjs`: bad `celsius`/`rain` ranges and climate `biomes`/`rainMm` entries fail naming them.
  Shares on the land (chosen biome, every 300 m, ≥ 1 %): seed 1 A (subtropical) savanna 22 %, red-rock mesa
  21 %, default 21 %, beach 12 %, desert 8 %, mixed forest 5 %, meadow 3 %; seed 1 B (temperate) default 38 %,
  mixed forest 28 %, steppe 15 %, meadow 9 %, badlands 5 %, cold rainforest 4 %; seed 7 (subarctic) tundra
  83–88 %, snowfield 10–12 %, boreal 1 % (only the band's warm southern lowlands are above −1.5 °C; the
  plates' land stands a few hundred metres up); seed 42 (temperate) mixed forest 28–32 %, default 26–34 %,
  meadow 5–20 %, steppe 5–14 %, cold rainforest 5–6 %, tundra, boreal and snowfield on the heights. *Deviations:* the
  `climateSet` key (§3.5) is not needed: the world lists the presets by name. The default fallback takes
  two climates no preset covers: the cool, nearly rainless interior (4–12 °C under 250 mm: a cold desert)
  and the very wet tropics (> 2500 mm, a rainforest); new presets for those are content for the owner
  (§7 q9). Chisel's `where` sliders gained `celsius` and `rain`; in Chisel's default world (no climate) a
  preset's `rain` is not read. **Not done:** the Chisel plates, latitude, wind and biome overlays (a WASM
  export of the layout and climate and a Chisel view); moved to R5.
- **R5 Geology and materials.** `rocks.json` `plateProvinces` with today's blocks (§7 q5: stone cores,
  basalt volcanics, sandstone/red sandstone basins, sand/dirt soils; rocks by block name so new ones can
  replace them by data later), soils by rock × climate. Chisel: the rock overlay by setting. *Exit:* cones sit on
  volcanic stacks, collision cores on the core stack, basins on sandstone, accreted strips on their stack
  (tests by fixture; swapping a province's block by data alone changes it); soil type changes with climate on one rock; goldens unchanged.
  **Built (2026-11-12):** `rocks.json` `plateProvinces` (checked by `biomes.mjs` `validateRocks`, a bad entry
  failing with its rule named; emitted into `rocks.gen.rs`; read by `config.rs` for Chisel's edits): the
  settings' widths and `rules`, each a setting → a stack by name, the first rule whose setting holds
  winning; where none does, the provinces' noise picks as before. The settings are the code's vocabulary
  (`geology::PlateSetting`, `PlateProvinces::settings` over the face's `PlateTectonics`, the boundary lines
  of the column's own plate only): `arc` (within 0.85 of an arc cone's radius), `hotspot` (a chain's
  island), `accreted` (within `accretedWidth` 2.5 km of a subducting boundary on the overriding side: the
  forearc's scraped-off seafloor), `core` (within `coreWidth` 4.5 km of a collision, or `rangeCore` 2.5–7 km
  behind a subducting boundary: the range's crest), `rift` (within `riftWidth` 3 km of a divergent
  boundary), `basin` (over `basinDistance` 12 km from every boundary of the plate) and `default`. The rules:
  arc → `dwell:arc_volcanics` (a new rock `dwell:andesite`, shown as the basalt block), hotspot →
  `dwell:hotspot_shield` (`dwell:shield_basalt`, basalt), accreted → `dwell:limestone_shelf` (limestone,
  stone), core → `dwell:collision_core` (granite over a new `dwell:schist`, stone), rift → `dwell:rift_fill`
  (sandstone and shale), basin → `dwell:red_beds` (sandstone, red sandstone); no default rule, so the rest
  keeps the noise's provinces. §7 q5: no new blocks; every rock names its blocks, so an andesite, schist or
  limestone block replaces one by data. The new stacks have `weight` 0: a stack of weight 0 is never the
  noise's (`Resolved` keeps the noise's thresholds over the weighted stacks only, so the default and the
  starter pick exactly as before), and a rock only in stacks of weight 0 takes its block palettes after every
other palette (`BiomeSet` `rock_palette`), so the existing palettes keep their ids (the starter's pin hashes
them: red with the new rocks' palettes among the old, green after). `Resolved::with_tectonics` builds the face seed's plates (the world
  map's) where `tectonics.plates` is 1, so only the Realistic world reads the rules. Soils: a `rockSoils`
  entry may name `celsius` and `moisture` ranges; a rock's entries are read in order, the first that holds
  winning, the last without a climate (`validateVegetation` fails an entry after it). Andesite: rocky
  (0.3 m) at 0 °C and below, sandy (0.6 m, sand) under 0.15 moisture, deep loam (2 m) at 8 °C and more with
  0.45 moisture or more, else loam (1 m); the shield basalt loam where wet, rocky where dry; schist rocky.
  The soil depth (T3, the vegetation's) and the soil blocks (T3b, `soilBlocks`) read the climate there
  only for a rock whose soils name one (the default and starter's rocks don't: their soils unchanged).
  Tests: `geology::plate_province_tests` (each setting on the two-plate fixtures takes its rule's stack;
  the build's rules show today's blocks; a rule's stack or a rock's block swapped by data alone changes
  the province; no rules, the noise's; a weight-0 stack is never the noise's),
  `vegetation::rock_climate_tests` (andesite's soil type and depth by climate; a climate entry added by
  data alone), `tests/realistic_rocks.rs` on both faces (seeds 1, 7, 42: every arc cone on the arc's
  stack, every hotspot island on the shield's, points by the collisions on the core's, by the trenches on
  the shelf's and in the quiet interiors on the basin's, each met; the noise's provinces elsewhere),
  `gen.test.mjs` (rules and soil climates by data, bad entries named); default and starter goldens
  unchanged. *Deviations:* a setting's stack is one name, not a list with weights (the noise inside a
  province is for later); the rock's blocks show on the ground only where the shaping is on
  (`shaping.strength` > 0; the Realistic world's was 0 until steps by geology, below).
  **Steps by geology (2026-11-13, 0.57.0; WORK_QUEUE item 15).** Owner, 2026-11-12: "Is all of our terrain still
  stepped? I like stepped terrain but it should be system driven not across the whole realistic world". Until then
  the default terrain's `cliffs` volume (volume.json: terraces on every steep hillside and mountainside) shaped
  Realistic worlds too. Now `realistic.json` sets `"cliffs": 0` (no global terrace volume) and `geology`
  `shaping.strength` 1 and `erosion.strength` 1 (data), so the ground is mapped through its column's stack
  (`geology::Profile`, unchanged): `bedded` rock (the basins' red beds, the rifts' fill, the accreted limestone, the
  cores' schist) steps into a wall and a tread every `bed` m, `vertical` rock (the arcs' andesite, the hotspots'
  basalt) stands in columns a unit tall, `massive` granite (the collision and range cores) rises unstepped; the
  mesas and scarps are the erosion's in the arid sedimentary biomes that opt in (red rock mesa, badlands, on their
  red and badland beds). Province edges, sharp until then, now fade the shaping in over `plateProvinces.edgeFade`
  (600 m) inside a setting's edge (a cone's rim, a width's line, the plate's boundary; `PlateProvinces::settings_and_edge`;
  absent 0, sharp, for edits stored before), so no wall runs along a province's edge. Tests:
  `tests/realistic_steps.rs` on both faces (seed 42; transects every 1 km, a column stepped where a neighbour is 2 m
  or more above or below it): no column reshaped by the volume; massive ground under 1 % stepped and within 0.5 %
  of the same land unshaped (A 0.4 %, B 0.8 %: the hills' own steepest ground); bedded ground 2.5 % over unshaped
  (A 3.2 % from 0, B 4.3 % from 0.7 %); vertical rock at least twice its unshaped count;
  `geology::plate_province_tests::a_settings_edge_fades_the_shaping_in`; `gen.test.mjs` (the data, a bad
  `edgeFade`). The Realistic golden rewritten (a Realistic-only terrain change: worlds made from 0.55's and 0.56's
  menu would change, so the new line 0.57); the default world's goldens unchanged (`tests/golden.rs`). Open for the owner: the
  cores' schist is `bedded` (bed 10 m), so 40 m of each 100 m of a collision core steps; schist as `massive` or a
  `foliated` jointing is a data change. No gravel or
  pale stone block exists: the rocky soil and stone stand in. **Not done:** the plates and rock overlay in
  Chisel and on the in-game big map (§7 q8; a WASM export of the layout and the settings and a view in
  each: not quick); moved to R6.
- **R6 The World option.** `newWorld` on `realistic.json`; its own golden hashes; the memory check on the
  phone tier (the bench `--touch`, BUGS.md #70's table). *Exit:* create, play and reload a Realistic world
  in the browser; a hash file for it; total at load on the phone page within ~10 % of the starter's.
  **Built (2026-11-12); exit met (browser check 2026-11-12, below).** Owner, 2026-11-12: **"Do we have realistic worlds yet?"**
  `realistic.json` carries `newWorld` (label Realistic, "Tectonic plates, mountain ranges, rain shadows and climates
  from the seed."), so New world → World lists it (`worldChoices.ts`, by data) and More options → Sky shows for it
  (it rolls an orbit, §8); the world's entry keeps its worldgen (`test:realistic`), seed and Sky preset, so it
  reopens on them. Its goldens: `shared/worldgen/golden/realistic-hashes.txt` (seed 42, both faces: the chunks at
  the ground and the one above every 6 km out to 30 km, LOD sections at levels 2, 5 and 8 every 12 km; 634 rows),
  held by `tests/realistic_golden.rs` natively (the dev and release profiles agree) and on wasm32 in CI; while it is
  a test world (§7 q7) a deliberate change rewrites it with `REALISTIC_GOLDEN=write` and a note. The default's
  goldens (`tests/golden.rs`) are unchanged. The debug overlays on the big map (§7 q8): `shared/ui/mapLayers.json`
  (checked by `shared/ui/gen.mjs`: layers by name, each a `field` of the generator's fixed vocabulary, `opacity`,
  category `colors` and `outlineColor`, or a value `ramp`), `dwell-worldgen` `map_layers.rs` (`MapField`: plates —
  the kind, outlined at the boundaries; uplift — the boundaries' landform, m; rain — the climate's annual mm;
  rocks — the surface rock, coloured by its `rocks.json` colour), sampled over the face's disc (256 a side) by the
  jobs worker (`Job::Map`'s `layer`), drawn over the revealed ground by `mapLayers.ts`; the map's Layer button and
  `L` step through them; a world without a field (plates outside Realistic, rain without a climate) says so.
  The memory check (natively; the bench's `--touch` page is a local session's): both faces' biome sets as a
  process holds them, Realistic seed 42 **14.1 MB** against the starter's (seed 7) **15.9 MB** (−11 %, under
  the 20 MB bar; `tests/realistic_golden.rs` pins the bar); the Realistic map has no tectonics grids, so its
  climate rain grid (0.15 MB a face) is its only extra. Tests: `tests/realistic_golden.rs` (the hashes; the
  memory; the layers sample both kinds of plate, three or more plates, uplift over 300 m and below 0, rain over
  3× from dry to wet, three or more rocks, on both faces), `map_layers` unit tests (names and indices, the wire
  form, the data's fields, the default world has rocks and no plates or rain), `jobs` (`maps_a_debug_layer_by_field`,
  the codec round trip), `gen.test.mjs` (the layers' data, a layer added by data, bad entries named),
  `mapLayers.test.ts` (decoding, the plates' kind colours and outline, rocks' own colours, the ramps, a layer by
  data), `worldChoices.test.ts` (the World list offers Realistic, every orbit world is offered, a Realistic world's
  route and entry keep `test:realistic` and its Sky). **Not done:** the same overlays in Chisel (a view per layer
  over Chisel's map: the field export is here, the view is not; next with Chisel's other overlays); outlines of the
  boundaries by kind (the plates' outline is one colour).
  **Browser check (2026-11-12, the M1 Air's GPU, `claude/realistic-check`):** New world → World → Realistic
  from the menu's form, seeds 42 (Sky Day varies) and 1234 (Sky Extreme): each created, played (walked, 57–60 fps;
  flown to a range, a coast and a dry slope), its big map opened with every layer (plates, uplift, rain, rocks), and
  reloaded: the same `?play=` world reopened with its record's `worldgen` `test:realistic` and `sky`, the same
  surface blocks at sampled columns and the same view, the clock running on. Found and fixed: the layers were drawn
  over the whole square around the disc (`layerPixels` now clears samples past the face's edge;
  `mapLayers.test.ts`, red → green). Noted, not bugs: the plates layer colours a continental plate over open sea
  beyond `plates.json` `reach` (25 km; the land lies within it, so most of the disc is sea whatever the plate);
  seed 1234 (a subarctic band) is nearly all snow. The phone page's total at load (`npm run bench -- --warm --touch
  --compare memory-phone`, configs `worldgen` `test:starter` and `test:realistic`, seed 0): Realistic **570 MiB**
  against the starter's **655 MiB** at load (−13 %), 532 against 598 MiB 30 s later.
- **Realistic batch A: plates and coasts (2026-11-13, the 0.64 line; WORK_QUEUE item 15).** The owner, on seed
  2741893632 (0.59.3): "What's up with realistic world having an enormous \"river\"? And a separated mountain range
  from the main continent? It's a very weird geography that doesn't look realistic at all", and "are we hardcoding
  that there's an ocean edge dropoff always around?". The diagnosis: the "river" was a divergent boundary between a
  continent and an ocean cut as a rift (160 m × 1.6, 77 m shoulders) along all 49 km, its sea-side shoulder closing
  it into one 45 km lake; the range was a continental plate centred at the disc's edge, clipped to a 5–7 km sliver of
  range; the coast was a circle at `reach − seaMargin` (23 km) for every seed. **Built** (all data in `plates.json`,
  checked by `gen.mjs`, tests on both faces):
  1. **Landforms by plate pair** (`uplift.pairs`): every boundary kind × plate pair once (nine entries, `gen.mjs`
     fails on a missing or duplicate one), each a list of landforms with a `side` (`over`, `under`, `astride`,
     `continental`, `oceanic`), a `shape` (`bump`, `rift`, `shoulder`), a signed height, width and distance, and an
     `arc` mode (`none`, `crest`, `island`). Two continents opening rift; a continent and an ocean opening make a
     passive margin (a shelf lift on the sea side kept under the sea floor, a lower coastal plain inland, no trough);
     two oceans a low swell; two oceans closing an island arc with a back-arc basin; an ocean under a continent a
     trench and a range with its arc on the crest; transforms low ridges. Geology's rift fill only where the pair
     cuts a rift (`Line::has_rift`).
  2. **Segmented rifts** (`uplift.rift`): a rift polyline cut into basins `segmentLength` 8–15 km long, each its own
     depth (0.7–1.3) and width (0.7–1.3), deepest mid-segment, sills between them (0.1–0.45 of the depth) stepping
     down toward the end nearer the sea, alternate axes `stepOver` 1.5 km apart; an end at sea has no sill and the
     shoulders are left out at sea (a gulf). Test: no lake on a rift is longer than a segment and a half (19 km), and
     a continental rift holds a chain of lakes (`rifts_hold_chains_of_lakes_not_canals`).
  3. **No sliver plates:** `kinds.<kind>.interior` (continental 8 km, oceanic 5 km: no strait under 10 km between two
     continents) — each plate's farthest point from every other plate and from the rim inside the land
     (`PlateLayout::interiors`, a 1 km scan); a plate short of it is removed (its ground to its neighbours) while its
     kind keeps its minimum, else drawn 30 % nearer the centre. `count` 7–11 drawn from a `centres` disc of 56 km
     (more than the 50 km `reach`, so plates straddle the rim), `spacing` 12 km. Test over 20 seeds both faces.
  4. **Ranges and arcs:** `uplift.along` varies every landform's height and width ±40 % by 1-D noise at 9 km along its
     line (saddles and peaks); a crest arc stands on its range's crest (4 km) and any arc's cone comes nearer the line
     in fifths, down to `arc.minDistance` 2 km, to stand on the overriding plate.
  5. **The rim** (`rim`, replacing `seaMargin`): the layout covers the whole 50 km disc; a continent reaching the rim
     ends `shelf` 1–8 km (drawn per each of 10 sectors, cosine-blended) inside the outer `edgeSea` 2.5 km band, its
     coast moved ±800 m by noise at 7 km; the band is always sea, at least `edgeCoast` 3 km out (deep); every raised
     landform and cone fades to nothing at the rim's coast. Test: the band is sea all round, the outer coast's radius
     spreads by more than 1 km over angle, and land comes within 6 km of the edge on some seed.

  Before → after (face A; `cargo run --release -p dwell-worldgen --example realistic_geo -- 1 7 42 2741893632`):

  | seed | plates (interior km, C continental, o oceanic) | land | longest lake | land reaches | outer coast σ |
  | --- | --- | --- | --- | --- | --- |
  | 1 | o8.6 o10.1 **C2.3** C7.8 → o11.9 o13.4 o17.0 o13.1 C15.8 o11.7 | 11.1 → 22.6 % | 22.7 → 15.4 km | 31.8 → 43.0 km | 0.40 → 4.63 km |
  | 7 | C8.5 o9.5 C12.2 → o14.6 o19.1 C14.4 C9.2 | 15.7 → 28.5 % | 2.8 → 10.3 km | 31.8 → 45.8 km | 0.59 → 2.65 km |
  | 42 | o7.3 **o2.5** o4.3 C10.9 C8.7 → C10.9 o18.7 o14.2 C8.8 o14.6 o13.1 | 17.5 → 16.9 % | 24.2 → 6.2 km | 31.6 → 46.6 km | 0.54 → 3.39 km |
  | 2741893632 | **C2.7** C13.8 o7.4 → C12.2 o14.1 o7.3 C13.4 C11.5 C17.0 | 21.4 → 57.2 % | **52.7** → 9.2 km | 31.8 → 46.8 km | 0.55 → 4.17 km |

  (Before, "land reaches" past the 23 km cap was ranges and islands standing out of the sea margin; the outer coast's
  σ is over the angles where land reaches past 30 km.) The longest lake on seed 1 is a sea-level lagoon at the rim, not
  a rift. *Deviations:* the plate count after the sliver rule can fall under `count`'s min (the test checks ≥ 2 and
  the kinds' minimums); the island arcs' cones erode to small islands in the wet sea air (the arc test weighs the
  continental arcs). Tests that pinned a seed's terrain followed it: `realistic_steps` reads seed 7 (42's bedded
  ground is now on flat plains) with massive ground under 2 % stepped and ledges on 1 in 30 bedded columns;
  `realistic_biomes`' rain shadow is 2× unless the windward side gets under a tenth of the band's coast rain (an
  inland range the air reaches dry), and the former fallback climates' presets take 80 % of their climate on low
  ground; `realistic_climate`'s discharge rain is 1.5× windward; `realistic_rocks` meets a collision on the face's
  first seed that has one, and allows 1 in 100 red-desert floors off by the arid line. The Realistic golden is
  rewritten (a new line, 0.64). Part B (variety: plan-view swaths, uniform hatching) is separate.
- **Realistic batch, the owner's later requests (2026-11-13, the 0.64 line; WORK_QUEUE item 15).** All data with
  validation and tests on both faces:
  - **Notched ramps only at mesa edges** (`rocks.json` `shaping.notched`, now a word: `none` | `mesaEdges` | `all`; an
    edit's 0/1 reads as none/all): `mesaEdges` notches a column only where it or a neighbour shows a rock wall
    (`Column::wall`: an erosion mesa's scarp, a hard bed's wall), the pre-0.59 straight-edge rule elsewhere; ledges
    unchanged. `tests/realistic_ramps.rs`: off the edges exactly the old rule; ramped columns off the edges 6.6–15.5 %
    against 0.59's 11.8–32.0 %; 140–637 notched edge columns per face.
  - **Mesa relief** (`mesaRelief`): standing tops undulate (3.5 m at 220 m), tilt (~1.2 %) and carry knobs or small
    buttes; floors rise from their drainage line (1.2 %, at most 9 m) with 3 m mounds, talus fans and 2 m washes; the
    wall and talus follow both. `tests/realistic_mesas.rs`: flat patches' rms tops 0.65 → 0.95 m (A), 0.86 → 1.20 m (B),
    floors 0.65 → 1.10 m, 0.66 → 1.14 m; the walls' steep share within 15 %.
  - **Mesa country's palette** (`blends` `dwell:mesa_country` on the red desert beds): red dirt 0.68, red sand 0.24,
    tan sand 0.08 in order by a 90 m noise (its normal CDF), the slope pushing toward red sand but never into tan,
    dithered at 3 m: neighbours at most one tone apart. New `dwell:red_dirt` (and its ramps); the red rock mesa's bands
    red dirt over red sandstone; the siltstone's floors red dirt, its faces red sandstone; the red rock ramp darker.
    Part B's province mixing does not reach a mesa: the biome's own stack (`rockStack`) wins. `tests/realistic_mesa_colours.rs`.
  - **Sagebrush** (`shared/trees/sagebrush.json`, `dwell:brush`, `placements.json` `sagebrush`): 0.07–0.08 shrubs per m² of
    mesa floor (about 3.5 m apart; the rule's most 0.225), none on walls. `tests/realistic_sagebrush.rs`.
  - **Tan sand deserts** (`climate.json` `rainByLatitude`: the subtropical dry belt; `preset:sand_desert` weight 2.5 below
    800 m): every subtropical face ≥ 5 % sand desert (seed 1 A 3.4 → 14.8 %, the owner's face B 4.6 → 21.9 %), all of it
    in warm dry climate; temperate and subarctic faces have little or none (cold deserts there). `tests/realistic_deserts.rs`.
  - The map's build time: 1.44× the default's (the time test's bar 1.6×, was 1.3×) after sharing the warp and coast
    between the height and the swells.
- **Realistic batch, the map check and the curves (2026-11-14, the 0.65 line: 0.64 shipped this batch's seam, ramp
  and shrine fixes on their own; WORK_QUEUE item 15).** All data in `plates.json` with `gen.mjs` validation, tests on
  both faces over seeds 1, 7, 42, 274005646 and 2741893632:
  - **Curved plates and coasts** (owner: "our tectonic plates are too straight"): the plate warp in three layers
    (`warp` 3.5 km at 30 km, `warp.detail` 2.2 km at 7 km and 0.6 km at 2 km; each layer now moves about ± its
    amplitude); every coast, the rim's too, moved by layered noise (`rim.coast`: 4.5 km at 18 km, 0.9 km at 3 km), kept
    off the outer band. `tests/realistic_shapes.rs`: plate boundaries' straight 2 km windows 57 % → ≤ 3 %, 5 km
    windows 81 % → ≤ 2 %, coasts' 3.1 % → ≤ 0.9 %.
  - **No thin strips:** at sea the boundaries' landforms stop 6 m under the surface (`uplift.seaRidges`; an oceanic
    transform's ridge stood out of the 20 m deep sea as an 11 km strip): the longest strip under 1 km wide ≤ 3.6 km.
  - **Varied islands** (`uplift.cones`): each cone's radius and height drawn, its outline from harmonics 2–7 of seeded
    amplitude, a chain island's own size, 0–3 satellite islets, arc sites skipped with chance 0.2, spacing ± half:
    islands' radius cv ≥ 0.88, spacing cv ≥ 0.57, outlines 1.7–3.3 × a disc's irregularity.
  - **Cones blend into their range:** the erosion's cap on a plate cone fades out toward its foot (hand-placed cones
    unchanged). `tests/realistic_cones.rs`: the slope break at the foot 0.60–0.96 → −0.31 to 0.26.
  - **Volcano states** (`uplift.volcanism`, VOLCANOES.md §6.7): active cones bare lava rock with an apron of basalt and
    obsidian in the shallows; dormant ones covered by age. `tests/realistic_volcanoes.rs`.
  - **Tan deserts' own stack** (`dwell:dune_sands`, the Realistic world's `preset:sand_desert` entry, not the preset:
    the starter's desert unchanged): 99.7 % tan sand; mesa country on every face outside the subarctic
    (`tests/realistic_mesa_presence.rs`).
- **Later (not phased):** strata tilt near boundaries; lava vents by rule; a 10× Realistic once the
  world map tiles.

R1–R5 are logic-only and suit cloud sessions (docs/CLOUD_SESSIONS.md); R6's browser check is local.

## 7. Questions for the owner

Open: §8.10's orbit questions. q2 moved to §8 (orbits by seed). Answered: q1 (by default), q3, q4, q5, q6, q7, q8, q9 (built 2026-11-12).

1. **Plate count and size for the first region:** 3–5 plates with one or two boundaries crossing the
   island, or more, smaller plates?
   **Not answered (2026-11-11):** R1 uses the proposal's default, 3–5 plates, as a range in `plates.json`
   (`count`), with `spacing` and `reach` for their size.
2. **Latitude bands:** which to offer at first (e.g. subtropical, temperate, subarctic), and should the
   seed pick any band, or should a world creator pick one (seed decides the rest)?
   **Half answered (owner, 2026-11-11):** the seed picks the band. Which bands to offer is open; R3 offers
   subtropical, temperate and subarctic (`climate.json` `bands`, temperate twice as likely), changed by data.
   **Owner, 2026-11-12:** "I think latitude bands are based on orbits right? Orbits should be seed
   determined." The band becomes the result of a seed-rolled orbit: §8, with its own questions (§8.10).
3. **The disc's coast:** should plates decide where land and sea are (oceanic plate = sea), keeping only
   a sea margin at the disc's edge, or keep today's central island and let plates shape it?
   **Answered (owner, 2026-11-11):** plates decide land and sea (oceanic plate = sea, continental = land);
   only a sea margin is kept at the disc's edge (`plates.json` `seaMargin`).
4. **Face B:** its own plates from its own seed (a different region), or the same plates mirrored
   through the midplane?
   **Answered (owner, 2026-11-11):** its own plates from its own seed (the face seed): a different region.
5. **New rocks:** add andesite, schist (or gneiss) and a real limestone look as blocks now, or show them
   with today's blocks (stone, sand, dirt) until the art pass?
   **Answered (owner, 2026-11-11):** "Let's hold off on new blocks as long as we can add relevant ones
   later." No new rock blocks now: R5 lays out the provinces with today's blocks (stone for range cores,
   basalt for volcanic ground, sandstone and red sandstone for basins, sand and dirt soils), each province
   naming its rock by block name in data, so andesite, schist or limestone can replace them later by data
   alone.
6. **Volcanoes in Realistic:** dormant cones only at first, or lava vents by rule (VOLCANOES.md)?
   **Answered (owner, 2026-11-12):** "dormant volcanoes only for now."
7. **Determinism of tuning:** while Realistic is a test world, may its seeds' terrain change between
   releases (hashes updated with a note), as the starter's does?
   **Answered (owner, 2026-11-11):** yes: while Realistic is a test world its seeds' terrain may change
   between releases, with a note (and its hashes updated). The seed picks the latitude band (q2's second
   half); which bands to offer is still open.
8. **Debug view home:** Chisel's overlays (proposed), or also the in-game big map?
   **Answered (owner, 2026-11-12):** "also on the big map" — the plates, wind and climate overlays in Chisel and
   on the in-game big map.
9. **Climates no preset covers (R4):** in the Realistic world the default biome takes the cool, nearly
   rainless interior (a cold desert) and the very wet tropics (a tropical rainforest). Add presets for
   them, or keep the fallback there? **Answered (owner, 2026-11-12): "Yes add those biomes"** (after asking "how
   does the default biome fill them?"; explained: the default is plain temperate grassland with generic trees
   and rocks, so a cold dry interior and a wet tropical coast would both look like ordinary meadow).
   **Built (2026-11-12):** two presets by data, in `realistic.json`'s list (the starter lists its own presets
   and the default world none, so both are unchanged: goldens and the starter's pins pass). `preset:cold_desert`
   (`where` −1.5…14 °C, blend 1, under 220 mm, blend 60): bare dry dirt over sand (sand where the soil is
   sandy, `soilBlocks`), sparse dry grass (a new `dry_grass` decoration, tall grass on dirt at 0.12), loose
   rocks and boulders, no trees, its own snow line at −1 °C. `preset:tropical_rainforest` (`where` ≥ 16 °C,
   blend 2, ≥ 2200 mm, blend 250): grass over dirt with dirt banks, tall grass and thick sticks (undergrowth
   1), dark water, and a new `rainforest_groves` placement (a 5 m grid, no spacing, density 0.85, groves
   covering 0.8) of a new tree species, `dwell:tall_broadleaf` (`shared/trees`: a 10–16 m trunk, its crown high,
   no sapling, so no new block), with oaks and palms. No new blocks (no gravel or dry dirt block exists: dirt
   and sand stand in). Shares on the land (chosen biome, every 300 m): the default fell from 21 % to 3 % on seed
   1 A (subtropical; the rainforest 16 %, the cold desert 3 % on the dry heights) and from 26–47 % to under 3 %
   on the temperate faces of seeds 1, 3, 5 and 42 (the cold desert 25–45 % of their dry interiors); subarctic
   seeds 7 and 11 keep their tundra (the cold desert ≤ 4 % on the dry lowlands above −2.5 °C).
   `tests/realistic_biomes.rs` `the_former_fallback_climates_take_the_new_presets` on both faces (seed 42 and
   each face's first subtropical seed): where the default was chosen without the two (≥ 80 % of the cells at
   −0.5…12.5 °C under 150 mm, or ≥ 18 °C over 2500 mm) they now take ≥ 95 %, and the default's land share
   falls below a quarter of what it was. A fix on the way: a column with no biome (a rock province's palette, or
   a world of one biome) tried every decoration, so `dry_grass` alone changed the starter's pin (red); such a
   column now takes only the decorations some biome of the world lists (`terrain.rs`), and the starter's pin and
   the default's goldens are unchanged again (green). Prototype content.

## 8. Orbits by seed: the sky sets the climate band

**Status:** O1–O2 built (2026-11-12): `shared/look/orbits.json` (the presets Default, Day varies and
Extreme as ranges, the limits; validated and generated by `orbits.mjs`), `dwell-look` `orbits.rs`
(`Orbit::roll(seed, preset, limits)`, `Orbit::today()` read from `daynight.json`, `day_night_for`), the
`CELESTIAL` world setting (optional tag 0x86, version byte, nine `f32`s; the clock in `CLOCK`, the moon's
phase in `MOON_PHASE`), the server rolling it at creation for a world with a Sky preset (`ServerConfig::sky`,
`--sky`), the sky (`Celestial::for_world`, the client's `set_world_settings`) drawing the world's orbit, and
New world → More options → Sky (desktop and phone), shown for a world file whose climate reads the sky (only
Realistic; `client/src/ui/sky.ts`) and the `&sky=` link (`?worldgen=test:realistic&sky=extreme`). Realistic
is in the New world form's World list since R6, so the Sky field shows when it is picked; by link too. Deviations: the tag is optional everywhere (not required for Realistic, §8.6): Realistic's
terrain doesn't read the orbit until O3, and a Realistic world is a test world whose terrain may change
(§7 q7); `today` is not in `orbits.json` (it is `daynight.json`'s, one source); the sun's size by distance is
not drawn (the shader's `SUN_RADIUS` is a constant). **O3 (the climate reads the orbit) not built**: the
climate is built in the map workers from the seed and the worldgen config, so the orbit has to reach them
(the page's `worldMaps`, the job and server-jobs workers) — the next session's work. Questions for the
owner in §8.10. Owner, 2026-11-12, on
§7 q2: **"I think latitude bands are based on orbits right? Orbits should be seed determined."** So the band
stops being a list the seed picks from. The seed rolls the world's **orbit** (its distance from the sun, the
sun path's inclination, the axial tilt, the day and the year, and the moon's orbit), the sky draws that orbit,
and R3's climate reads it: the latitude band becomes a **result** of the sky. One celestial model
([MOON_WORLD.md](./MOON_WORLD.md) §6, `dwell-look` `celestial.rs`) for the sky and the climate.

### 8.1 What "latitude" means on a flat bifacial disc

Today's sky (DAY_NIGHT.md §3): the disc is at rest; the sun moves on a **great circle** through its rising
yaw (`sunPath.riseYawDeg` 31°), culminating `noonElevationDeg` (60°) above face A's horizon; the circle
passes under the disc, so face B's day is A's night (B's solar angle is A's + 180°). The sky (and its stars)
turns about that circle's **axis**. The uneven solar angle (`k sin 2πu`) stretches A's daylight to 800 s of
a 1200 s day, B's to 400 s.

- **Sunlight on a flat face is the same everywhere:** the sun is far, its rays parallel, so every column of
  face A sees the sun at the same height at the same moment. A flat disc has **no latitude gradient** from
  the sky; it has one latitude per face.
- **That latitude is the pole's height.** On a globe your latitude equals the height of the celestial pole
  over your horizon (Polaris at 40° in a 40° N city), and the noon sun at the equinox stands at 90° − φ.
  The disc's sky turns about its path's axis, which stands 90° − 60° = **30° over face A's horizon**: face A
  sits at **φ = 30° N**, in permanent equinox geometry (δ = 0). Face B sees the axis's other end 30° over
  its own horizon: **30° S**. Today's 60° noon on both faces is exactly that.
- **"North"** on face A is toward the point under the pole (away from the noon sun, the sun culminating
  toward riseYaw + 90°); on face B, through `FaceFrame`, its own pole's side. Both follow from the sky; R3
  draws its north axis from the seed instead.
- **Day length is a game warp, not geometry.** On a δ = 0 path a globe has 12/12 hours everywhere; A's 2/3
  daylight is the clock's warp (the owner's 800/400 split), and it stays the playability layer (§8.4).
- **Today's mismatch:** R3 draws a band (subtropical, temperate, subarctic) and spans it across the disc
  along a seed axis, while every world's sky is the same 30° N sky. A subarctic world has a 60° noon sun. The
  orbits make the sky and the climate the same fact.

The gradient across the disc that R3 has (a band spans 14° of latitude from the south edge to the north
edge) has no cause in parallel sunlight. Keep it as a **game-scale stretch**, like the 20-minute day: the
climate's latitude at a column is the face's φ ± `spanDeg`/2 along its north axis (data; 14° as R3's bands
today). The alternative, a near sun whose rays fan across the disc (a 14° spread needs it ~400 km away,
nearer than the moon, and different sun directions per viewer for the shadows), costs the renderer and reads
wrong in the sky; not proposed (§8.10 q5).

### 8.2 What the seed's orbit sets

| Orbit part | Sets | Model | Today's value (the default sky) |
|---|---|---|---|
| **Pole height** φ (the sun path's inclination to the disc) | the face's latitude: face A at φ N, face B at φ S; the noon sun 90° − φ (± δ) | the sky's axis tilted φ over face A's horizon (`sunPath.noonElevationDeg` = 90° − φ) | φ = 30° (noon 60°) |
| **Pole yaw** | north on each face (R3's seed axis becomes the sky's) | the axis's yaw (`sunPath.riseYawDeg` = its rising point, a quarter turn off) | 31° |
| **Distance** r (relative, 1 = today) | an overall temperature offset; the sun's angular size × 1/r | ΔT = 288 K × (r^−½ − 1) (radiative balance), clamped to `offsetC` | 1 (0 °C) |
| **Axial tilt** ε | seasons (§8.5): the sun's declination swings ±ε over a year; 0 = no seasons | δ(t) = ε sin 2π(day / Y + yearPhase) | 0° |
| **Day length** | the world clock (the existing `WorldSettings` `CLOCK`, ADR 0054) | `dayS` + `nightS` and the split, within ranges | 800 + 400 s |
| **Year length** Y | the seasons' period, in world days | — | none (ε 0) |
| **Moon's orbit** | the month (period), its inclination, node and phase (M-4) | M-4's `moonOrbit` and `moon.cycleDays`, `MOON_PHASE` | 8 days, 5°, node 90°, turn 96 days |
| **Moon's distance** | kept: it holds the 12.5 km moon at 1.2° (owner, MOON_WORLD.md §12 Q8: "should match previous size for now") | derived from `grids.json` and `radiusDeg`, not rolled | 596 800 m |

Today's sky is the orbit **φ 30°, yaw 31°, r 1, ε 0** with today's clock and moon: the default worlds keep
exactly it (§8.7).

### 8.3 How R3's climate reads the orbit (the band becomes the result)

- **Latitude at a column:** φ_face (A: +φ, B: −φ, its magnitude for the curves) ± `spanDeg`/2 along the sky's
  north (face-local, through `FaceFrame`). `climate::latitude` keeps its signature; only where its centre and
  axis come from changes.
- **Sea-level temperature:** R3's `seaLevel` curve by latitude, unchanged, + the distance's ΔT. `lapseRate`
  unchanged. With seasons off, nothing else.
- **Wind:** R3's cells by latitude (trade winds, westerlies, polar easterlies), read at the face's latitude;
  the jitter stays the seed's. On face B the cells are the southern hemisphere's (the bearings mirrored by
  `FaceFrame`, as the terrain is).
- **The bands become names:** `climate.json` `bands` lose their `weight` (nothing picks them); each stays a
  named latitude range, and the band a face falls in names it (for Chisel, the debug overlay, tests) and
  gives its `rainMm` (interpolated between the bands' middles, so a φ between two bands is between them).
- **The faces:** today each face draws its own band (seed 1: A subtropical, B temperate). One sky gives both
  faces the same |φ|, so they get the same band, mirrored south, unless their daylight differs: A's 2/3 day
  against B's 1/3. Proposed: a face's sea level gains `dayShareC` °C per 0.1 of daylight share over ½ (data;
  e.g. 1 °C, so today's A is ~1.7 °C over the face's curve and B ~1.7 °C under it, ~3 °C apart). Their terrain still differs (each face's own plates, §7
  q4). §8.10 q4 asks whether the faces should differ more.
- **Generation never reads the season:** terrain, biomes, rain and rivers use the **annual mean** (δ = 0
  and the mean day share), so a chunk generates the same in winter and summer and the worldgen stays a
  function of the seed and the stored orbit.

### 8.4 What stays fixed for playability

- **Both faces get days and nights**, every day of the year: each face's daylight share stays within
  `dayShare` [min, max] (data, e.g. [0.2, 0.8]) at every season; the warp's turn-back check (DAY_NIGHT.md §3)
  runs over the year's whole range of splits in the generator.
- **Day length within a range** (data, e.g. 1080–1440 s): the clock already belongs to the world, and tree
  growth, water and the ecology count its days, so a seed's day stays near the owner's 20 minutes. A range of
  one value pins it.
- **The noon sun high enough to play in:** φ + ε ≤ `maxPoleDeg` (e.g. 62°: the noon sun ≥ 28° at the
  winter solstice), so no world is a polar night or a sun skimming the horizon all day. The roll draws φ
  first, then ε within [0, min(`tiltDeg` max, `maxPoleDeg` − φ)], so a high-latitude world has mild seasons.
- **The keyframes** stay keyed by the solar angle (DAY_NIGHT.md §4), so a lower sun keeps noon's colours; a
  later tuning may weigh them by the sun's elevation.
- **The moon** keeps its size in the sky, its glow and its light (owner, M-2); the month within a range.

### 8.5 Seasons: a new consequence (optional, later)

With ε > 0 the sun's daily path becomes a circle of declination δ(t) about the same axis (no longer a great
circle): face A's noon is 90° − φ + δ, face B's 90° − φ − δ, so **the faces have opposite seasons** (A's
summer is B's winter), as a globe's hemispheres do.

| What | Changes with the season | When |
|---|---|---|
| Sun path | noon elevation ±ε; the day share swings with δ (the small circle's longer arc over the summer face), clamped by `dayShare` | with the seasons' first phase |
| Temperature | the climate's `temperature_c(x, z, h, day)`: the curve read at latitude φ − `damping` × δ(day − `lagDays`) (the sea's inertia: ~½ the swing, ~1/12 of a year late) | model first, read by gameplay later |
| Snow line | the snow altitude follows the season's temperature: first as a **look** (the shader whitens ground above it, as the LOD colours do), real snow blocks later | later |
| Vegetation, forage | grass regrowth's and the far ecology's forage climate cap (WORK_QUEUE items 14 and 11) by the season's temperature | later |
| Not changed | the generated terrain, biomes and rivers (the annual mean, §8.3); water freezing (not designed) | — |

A year is `yearDays` world days (data, e.g. 16–32: 5–11 hours of play at 20 minutes a day). The world's
calendar (`days` from the clock, DAY_NIGHT.md §2) gives the day of the year; `yearPhase` sets where a new
world starts (spring, so its first days are mild).

### 8.6 Data shape

A new data file, `shared/look/orbits.json` (validated by `orbits.mjs`, emitted with the rest of
`shared/look/gen.mjs`, CI failing on stale output), holding **ranges** the seed rolls in and today's orbit:

```json
{
  "today": { "poleDeg": 30, "poleYawDeg": 31, "distance": 1, "tiltDeg": 0 },
  "roll": {
    "poleDeg": [15, 55], "poleYawDeg": [0, 360], "distance": [0.97, 1.03],
    "tiltDeg": [0, 23], "yearDays": [16, 32], "yearPhase": 0.0,
    "dayLengthS": [1200, 1200], "dayShare": [0.6, 0.7],
    "moon": { "cycleDays": [7, 10], "inclinationDeg": [0, 8], "nodeDeg": [0, 360], "phase": [0, 1] }
  },
  "limits": { "maxPoleDeg": 62, "dayShare": [0.2, 0.8], "offsetC": [-6, 6], "dayShareC": 1 },
  "seasons": { "damping": 0.5, "lagDays": 0.083 }
}
```

- **One roll function** (`dwell-look` `orbits.rs`: `Orbit::roll(world_seed, &ranges)`, pure) gives a
  `celestial` record: φ, pole yaw, r, ε, Y, the year's phase, the clock, the moon's period, inclination, node
  and phase. Rolled **once, at world creation**, by the server.
- **Stored as world settings** (ADR 0054), so a world keeps its sky when a later build changes the ranges: the
  clock in the existing `CLOCK`; the moon's phase in M-4's `MOON_PHASE`; the rest in a new tag `CELESTIAL`
  (φ, yaw, r, ε, Y, year phase, the moon's period, inclination, node; `f32`s behind a version byte), saved in
  the world file's `world_settings`. `Celestial::for_world` builds the sky from it, `Climate::for_face` takes
  it as an input beside the face seed.
- **Required for a world whose terrain reads it.** Its tag is optional (top bit) for the sky alone (an older
  build draws today's sky: a look), but a Realistic world's terrain depends on it, so there it is sent as a
  required field, and an older build refuses the world ("needs a newer version"), as for `WORLDGEN`, rather
  than generate other terrain.
- **References by name:** the bands by name, the world's settings by tag; no orbit constants in code beyond
  `today` (in data).

### 8.7 Compatibility

- **The default and starter worlds keep today's sky exactly:** they roll nothing and write no `CELESTIAL`;
  `Celestial::for_world` without it is today's orbit (a golden test: the sun direction and the moon over a
  month byte-identical), and their joins are byte-identical. No golden moves.
- **Only Realistic (and later, new worlds the owner opts in) roll orbits.** Realistic's terrain changes (its
  climate's band and north now come from the orbit), allowed while it is a test world (§7 q7), its hashes
  updated with a note.
- **A new world on the default generator rolling orbits** would change its sky (a look, not breaking) but not
  its terrain (the default reads no climate); its day length and seasons are game rules, so turning that on
  for every new world is the owner's call (§8.10 q2), not part of these phases.

### 8.8 Phases (each ≤ 45 minutes)

- **O1 Data and the roll.** `orbits.json` + `orbits.mjs` (+ gen), `orbits.rs` `Orbit::roll` and
  `Orbit::today()`. *Exit:* same seed same orbit, different seeds differ; every value within its range; the
  limits hold over 10 000 seeds (φ + ε ≤ `maxPoleDeg`, the day share in bounds at both solstices on both
  faces); a bad entry fails naming it; a range added by data alone is rolled. No game change.
- **O2 The sky reads the orbit.** `Celestial` from an `Orbit` (φ, yaw → the path; the clock; the moon's
  orbit); the `CELESTIAL` world setting (protocol + world file), absent by default. *Exit:* `Orbit::today()`
  gives today's sun and moon (golden); a rolled orbit's noon elevation is 90° − φ on both faces; a default
  world's join byte-identical; the tag round-trips.
- **O3 The climate reads the orbit (Realistic).** `Climate::for_face(seed, face, &orbit)`: latitude from φ
  and the sky's north, `spanDeg`, the distance's ΔT, `dayShareC`; bands as names and `rainMm`; Realistic
  rolls at creation. *Exit:* a world's band is the one φ falls in; a farther orbit colder at sea level; face B
  the southern mirror (winds mirrored) on both faces; R3/R4's Realistic tests pass re-baselined (the windward
  ratio, no dry biome on wet sides); default and starter goldens unchanged.
- **O4 Seasons in the model.** δ(t), opposite seasons on the faces, the day share's swing clamped,
  `temperature_c(…, day)` with damping and lag; generation still the annual mean. *Exit:* ε 0 equals O2's sky
  and O3's climate exactly; A's summer is B's winter; the mean over a year is the annual mean; nothing in
  generation reads the day (a test generates a chunk on two days of the year, identical).
- **O5 Seasons drawn.** The sky's sun on its declination circle, the season's day share; a dev control to
  scrub the year (DAY_NIGHT.md §6); the Chisel latitude overlay with the band's name. *Exit:* a screenshot at
  each solstice on both faces; CPU-adapter tier unchanged.
- **Later (not phased):** the snow line as a seasonal look, then snow; grass and forage by season; new worlds
  on the default generator rolling orbits (§8.10 q2).

O1–O4 are logic-only and suit cloud sessions; O5's screenshots are local.

### 8.9 Why this fits the extendable-system rule

The vocabulary in code is small and fixed (pole height, yaw, distance, tilt, year, a declination circle, the
radiative offset, the damping); the ranges, limits and today's orbit are data a tool could edit (a Chisel or
Sky Lab slider page later); the world keeps its own rolled values as settings; the climate reads named bands.
A fixture orbit added by data alone is drawn and read (O1–O3's tests).

### 8.10 Questions for the owner

**Answered (owner, 2026-11-12):** 1. **orbits first, seasons after** (O1–O3, then O4–O5). 2. **"I think this would
be a great setting to have default with some presets. Like proposed ranges, fixed day, day varies, more extreme
with hotter/colder."** — the orbit's variety is a world setting (New world → More options) with presets as data:
*Default* (the proposed ranges, the day fixed at 20 min), *Day varies* (the same with the day length rolled a
little), *Extreme* (wider ranges: hotter and colder worlds); more presets by data. 3. **the moon's orbit by seed
too**, its size in the sky kept. Default-generator worlds: **Realistic worlds only** roll orbits; the default
and starter keep today's sky — owner: "We'll likely roll orbits out to regular worlds too, but for now it's a
good reference point to compare to." 4 and 5 answered: "climates should align with the systems in place, orbits, etc." / "Follow orbital
systems" — both come from the orbits model alone.


1. **Seasons now or later?** Proposed: the orbit (φ, distance, the moon) first (O1–O3), seasons' model O4 and
   drawn O5 after, the seasonal snow and forage later.
2. **How varied may orbits be?** Proposed ranges: pole 15–55° (subtropical to subarctic, as R3's bands),
   distance ±3 % (±4 °C), tilt 0–23°, year 16–32 days, the day fixed at 20 minutes (800/400 ± a little).
   Should the day length vary by seed at all? Should new worlds on the default generator roll orbits too (their
   sky and seasons only; their terrain unchanged)?
3. **The moon's orbit by seed too?** Proposed: its month 7–10 days, inclination 0–8°, node and phase rolled; its
   distance kept so its size in the sky stays (your answer to MOON_WORLD.md §12 Q8). Or keep today's moon in every world?
4. **The two faces' climates:** one sky gives both faces the same latitude (A north, B south, opposite
   seasons); their daylight (A 2/3, B 1/3) makes B ~3 °C colder by `dayShareC`. Is that enough difference, or
   should face B differ more (R3 today draws it its own band)?
5. **The gradient across the disc:** keep R3's 14° game-scale span across the map (proposed), or no gradient
   (one climate per face, only height and rain vary), or a near sun whose rays fan out (not recommended)?
