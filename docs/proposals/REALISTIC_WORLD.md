# Proposal: a seed-driven Realistic world

**Status:** written 2026-11-11, not adopted. Questions for the owner in §7 (q1, q3, q4, q5 and q7 answered
2026-11-11). R1 built: the plate layout (`shared/worldgen/plates.json`, `dwell-worldgen` `plates.rs`). R2
built (2026-11-12): uplift from the boundaries (`plate_tectonics.rs`, `plates.json` `uplift`) in the
Realistic test world (`shared/worldgen/worlds/realistic.json`, `?worldgen=test:realistic`, not in the New
world form); the default and the starter are unchanged. R3 built (2026-11-12): the climate from latitude, wind
and terrain (`shared/worldgen/climate.json`, `dwell-worldgen` `climate.rs`) in the Realistic world, its rain
feeding the rivers' discharge. R4 built (2026-11-12): biomes chosen by climate alone, the presets' `where` in
°C and annual rain (mm), the Realistic world listing every preset unchanged. §8 (2026-11-12, design only):
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
- **R6 The World option.** `newWorld` on `realistic.json`; its own golden hashes; the memory check on the
  phone tier (the bench `--touch`, BUGS.md #70's table). *Exit:* create, play and reload a Realistic world
  in the browser; a hash file for it; total at load on the phone page within ~10 % of the starter's.
- **Later (not phased):** strata tilt near boundaries; lava vents by rule; a 10× Realistic once the
  world map tiles.

R1–R5 are logic-only and suit cloud sessions (docs/CLOUD_SESSIONS.md); R6's browser check is local.

## 7. Questions for the owner

Open: q9 (waiting on the owner), and §8.10's orbit questions. q2 moved to §8 (orbits by seed). Answered: q1 (by default), q3, q4, q5, q6, q7, q8.

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
   them, or keep the fallback there? Owner asked (2026-11-12): "how does the default biome fill them?" (explained:
   the default is plain temperate grassland with generic trees and rocks, so a cold dry interior and a wet
   tropical coast would both look like ordinary meadow); waiting on the answer.

## 8. Orbits by seed: the sky sets the climate band

**Status:** design only (2026-11-12), nothing built; questions for the owner in §8.10. Owner, 2026-11-12, on
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
and starter keep today's sky. Still open: 4 (how much the two faces' climates differ) and 5 (the gradient across
the disc) — the proposal's defaults until answered.


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
