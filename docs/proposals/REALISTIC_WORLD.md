# Proposal: a seed-driven Realistic world

**Status:** written 2026-11-11, not adopted. Questions for the owner in §7 (q1, q3, q4 and q7 answered
2026-11-11). R1 built: the plate layout (`shared/worldgen/plates.json`, `dwell-worldgen` `plates.rs`), not
wired into any world, so nothing in the game changes yet.

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
- **Blocks:** new rocks need new looks (andesite, schist, limestone that isn't `dwell:stone`). Adding
  blocks is not breaking (ADR 0038), but each is content: §7 q5.

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
- **R3 Climate.** `climate.json`: latitude bands, wind cells; temperature in °C from latitude and height;
  the rain march with the band's wind; the one rain field feeding the discharge (Realistic only). Chisel:
  latitude and wind overlays. *Exit:* the windward side of a seed's main range is ≥ 2× wetter than its
  lee; a colder band's sea-level temperature is lower; the default world's rain and rivers unchanged
  (goldens); the coarse rain grid's memory under the bound.
- **R4 Biomes by climate.** Presets' `where` in °C and rain; the Realistic world lists the presets
  unchanged. *Exit:* for a few seeds, biome shares follow the climate (no desert in a wet band's windward
  side; tundra above the tree line); no per-world biome tuning in `realistic.json`; the starter still
  passes `tests/starter_world.rs`.
- **R5 Geology and materials.** `rocks.json` `plateProvinces`, new rocks (andesite, schist, limestone
  looks if approved), soils by rock × climate. Chisel: the rock overlay by setting. *Exit:* cones sit on
  volcanic stacks, collision cores on granite/metamorphic, basins on sandstone/shale, accreted strips on
  limestone (tests by fixture); soil type changes with climate on one rock; goldens unchanged.
- **R6 The World option.** `newWorld` on `realistic.json`; its own golden hashes; the memory check on the
  phone tier (the bench `--touch`, BUGS.md #70's table). *Exit:* create, play and reload a Realistic world
  in the browser; a hash file for it; total at load on the phone page within ~10 % of the starter's.
- **Later (not phased):** strata tilt near boundaries; lava vents by rule; a 10× Realistic once the
  world map tiles.

R1–R5 are logic-only and suit cloud sessions (docs/CLOUD_SESSIONS.md); R6's browser check is local.

## 7. Questions for the owner

Open: q2's bands (the seed picks one: answered), q5 (new rocks: still with the
owner), q6, q8. Answered: q1 (by default), q3, q4, q7.

1. **Plate count and size for the first region:** 3–5 plates with one or two boundaries crossing the
   island, or more, smaller plates?
   **Not answered (2026-11-11):** R1 uses the proposal's default, 3–5 plates, as a range in `plates.json`
   (`count`), with `spacing` and `reach` for their size.
2. **Latitude bands:** which to offer at first (e.g. subtropical, temperate, subarctic), and should the
   seed pick any band, or should a world creator pick one (seed decides the rest)?
   **Half answered (owner, 2026-11-11):** the seed picks the band. Which bands to offer is open.
3. **The disc's coast:** should plates decide where land and sea are (oceanic plate = sea), keeping only
   a sea margin at the disc's edge, or keep today's central island and let plates shape it?
   **Answered (owner, 2026-11-11):** plates decide land and sea (oceanic plate = sea, continental = land);
   only a sea margin is kept at the disc's edge (`plates.json` `seaMargin`).
4. **Face B:** its own plates from its own seed (a different region), or the same plates mirrored
   through the midplane?
   **Answered (owner, 2026-11-11):** its own plates from its own seed (the face seed): a different region.
5. **New rocks:** add andesite, schist (or gneiss) and a real limestone look as blocks now, or show them
   with today's blocks (stone, sand, dirt) until the art pass?
   **Open:** still with the owner (needed by R5).
6. **Volcanoes in Realistic:** dormant cones only at first, or lava vents by rule (VOLCANOES.md)?
7. **Determinism of tuning:** while Realistic is a test world, may its seeds' terrain change between
   releases (hashes updated with a note), as the starter's does?
   **Answered (owner, 2026-11-11):** yes: while Realistic is a test world its seeds' terrain may change
   between releases, with a note (and its hashes updated). The seed picks the latitude band (q2's second
   half); which bands to offer is still open.
8. **Debug view home:** Chisel's overlays (proposed), or also the in-game big map?
