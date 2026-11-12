# ADR 0074: Biomes as data, and Chisel, the biome and texture editor

- **Status:** Accepted (2026-11-10; Phase 56 B1 built in 0.51.14, B2 in 0.51.15). Adopts
  [`proposals/BIOME_SANDBOX.md`](../proposals/BIOME_SANDBOX.md) with the owner's answers (its §5).
- **Context:** The owner asked for a biome sandbox (2026-11-09): sliders for a landscape's surface
  blocks, relief, banks, water hue, trees, rocks and structures, a live preview, and export to JSON,
  so an archipelago, Arches National Park or Mount Rainier can be made as biomes; and a texture lab
  beside it. The per-landscape knobs already existed as data, spread over `terrain.json` and the
  rule files, with one landscape for every world.

## The owner's answers (2026-11-10)

1. **Blended biomes from the start:** a biome map with transitions across one world, not one biome
   per world.
2. **The editor lives in both places:** a `docs/prototypes/` page first, then folded into the game
   like the structures sandbox.
3. **Heightfield biomes first** (the archipelago, Mount Rainier); arches and overhangs later (3D
   density).
4. **The texture lab is one tool with the biome sandbox**, textures as a tab.
5. **The editor is called Chisel** (biomes and textures; its prototype page will be
   `docs/prototypes/chisel.html`).

## Design goals (owner, 2026-11-10)

- **Cohesive, not a patchwork** ("Unlike Minecraft we want our world to feel cohesive"). Biomes
  follow the geography: the biome map (B2) is driven by the world's own fields (elevation, distance
  to the coast, rivers and wetness, a slow climate gradient across the island), with wide, gradual
  transitions and no hard borders, so neighbouring places relate (a beach grades into dunes, a river
  valley is lush, highlands go rocky then snowy). A biome's data declares **where it belongs** in
  those terms (its climate, elevation, coast and wetness ranges), not a noise patch.
- **Elevation first, separate from biomes** ("the biome blending should result in a very similar
  world to what we have now … I think it would make sense if the elevation was separate from
  biomes"). The world's shape (the base height field, the uplift, stepped terraces, high flatlands,
  deep valleys, mountains, the taper to the shore, the rivers and lakes) is one world-scale system,
  computed first and biome-independent. Mountains come from the elevation, not from a mountain biome.
- **Biomes layer on the elevation, and vary by height** ("a desert biome would turn into frozen
  tundra at higher elevation"): each biome adds **bounded fine relief** (its own small-scale
  amplitude, wavelength, terracing, capped so the world's shape stays) and lists **elevation bands**
  (e.g. desert: low dunes, then rock and mesas, then frozen tundra high up), each band with its
  surface blocks, relief and vegetation; bands blend across their boundaries as biomes blend across
  the map. Blending blends only these surface and feature parameters.

## Decision

1. **The split.** `shared/worldgen/terrain.json` is the world's elevation and water, one world-scale
   system: `inland`, `seaFloor`, `hills`, `bumps` (the base height field the map samples, with
   `hydrology.json`'s uplift and erosion), `rivers`, `mouths`, `lakes`, `spawn`, and the world's
   `defaultBiome`. A **biome** is one data file, `shared/worldgen/biomes/<name>.json`, named
   `namespace:name` (`dwell:default`): `detail` (the bounded fine relief, ±100 m at most, more at
   altitude), `bands` (the surface blocks by height and slope: today's snow line and rock band are
   the default's elevation bands), and references by name: `decorations` and `placements` (rule
   names of `decorations.json` and `placements.json`), `water` (the fluid block its water is drawn
   as). Field names and meanings are kept. B2 reshapes `bands` into a list of elevation bands (the
   default's: shore, ground, rock by slope, snow above the snow line; golden-equal) and adds where a
   biome belongs; unknown fields fail today, so each addition is explicit.
2. **One generator:** `shared/worldgen/gen.mjs` reads `terrain.json` and every biome, validates them
   (unknown fields fail, every number's range, blocks against the registry, rule names against their
   files, a fluid for `water`, no duplicate names, the default must exist) and emits
   `crates/dwell-worldgen/src/terrain.gen.rs`: `HEIGHT` (the world's), `BIOMES` (the default first,
   each biome's surface merged with the world's water shaping) and `SURFACE`, the default's. CI's
   stale check covers it.
3. **The generator reads the biome.** Today's terrain is the default biome: B1 moved values, not
   their meaning, so the worldgen goldens are byte-identical and no compatibility line is needed.
   `TerrainGenerator::with_biome` puts a biome's surface blocks in place of the default's (B1: the
   bands' blocks; the fine relief and the snow line through `SURFACE` until B2 blends biomes per
   column).
4. **Ground items** (`shared/ground/ground.json`) are not in the biome yet: they come with trees and
   structures in B5. Bank steepness (`rivers.bank`, `lakes.bank`, world-level now) is revisited in B4.
5. **The biome map (B2, built 0.51.15).** A biome's `where` places it in the geography: a `weight`
   and optional ranges (`min`, `max`, eased over ± `blend`) of `temperature` (a slow climate gradient
   across the island, along a seeded direction and bent by a noise, less `lapseRate` per 1,000 m of
   elevation: a warm biome's heights are cold), `wetness` (1 at a river or lake, 0 `wetReach` m away),
   `elevation` (m, the world height field's) and `coast` (m to the sea); `terrain.json` `biomeMap` holds
   the world's parameters. Since REALISTIC_WORLD.md R4 (2026-11-12) also `celsius` (°C) and `rain` (annual
   mm), the presets' climate envelope (Whittaker-style; the presets keep no 0..1 `temperature`): in a world
   with a climate (`tectonics.climate`, the Realistic world) both are the climate's (°C from latitude and
   height; the rain its wind marched again over the eroded map, `climate.json` `biomes`), and the 0..1
   temperature and wetness follow from them; elsewhere `celsius` is the 0..1 temperature by
   `vegetation.json` `climate` and `rain` is not read. Its weight in a column is `weight` × each range's fit; **the default biome is a fallback** (B3, 0.51.16): it takes only what is left of 1 after the others (× its own fit), so a biome that fits a place takes it at an ordinary weight of 1; the weights are then normalised. `bands` is
   a list of elevation bands (each its blocks by slope: `ground`, `under`, `rock`, `shoreBlock`; the
   lowest without a `from`), their edges moved by the biome's `shaping` noise and raised on slopes;
   `shaping` also holds the rock band (`rockSlope` easing to `rockSlopeHigh` at `rockHigh` m, today's
   snow line for the default), the shore and the soil depth. The column's fine relief is the weighted
   sum of its biomes'; its surface, rock band and band edges are one biome's, chosen by a dithered
   threshold on the weights (no straight borders). The height field is computed first and is never
   read back. One biome: no map, golden-equal. Not yet per band (deviations, Phase 56): a band's own
   fine relief and decoration/placement rules (the biome's `detail`, `decorations`, `placements` are
   per biome; per band with B5), and the soil depth is the default's world-wide (`soil_rows`).
6. **Compatibility:** a changed default biome, or a new biome on the map, changes what a seed
   generates: a new compatibility line, unless a world names its biomes at creation (B7).

## The plan (each sub-phase ≤ 45 min; Phase 56, `IMPLEMENTATION_PLAN.md`)

- **B1** the biome file format, the generator and validation, today's terrain as the default biome,
  goldens unchanged. *(built, 0.51.14)*
- **B2** the biome map and blending: biomes placed by where they belong (elevation, coast, wetness,
  climate), wide transitions, elevation bands per biome; a one-biome world is golden-equal. *(built,
  0.51.15)*
- **B3** Chisel's page: a live preview, sliders, Export/Import JSON. *(built, 0.51.16: served by the
  client build as `client/chisel.html`, not a standalone `docs/prototypes/` file, since it runs the
  generator's WASM; the validation shared as `shared/worldgen/biomes.mjs`. The world overview (0.52.11; owner, 2026-11-10: "a world map preview for this tool when we can blend between biomes"): beside the detail preview, the whole disc (island and sea, `WORLD_RADIUS`) from the same edit and overlay, 128² (~0.78 km texels) then 512² (~0.2 km), the frame from `chisel::overview_frame` (`ChiselPreview.overview_frame`); the worker runs the detail lane first, splits the overview's passes into 128² chunks, drops a lane's older generation when a newer edit arrives and caches the chunks by the edit state (`client/src/chisel/overview.ts`); the detail region drawn as a rectangle, a click or drag moving its centre. Face A only (Chisel has no face toggle; face B is its mirror).)*
  *The 3D section and relief (0.52.19; owner, 2026-11-10: "should probably have a more 3D perspective of
  a section" — as well as the 2D views): a hill shade on the detail preview and the overview (toggle,
  on by default), and a WebGPU view of a 256 m–8 km square from the same edit: the worker's third lane
  (`section`, after the detail and before the overview, a newer edit cancelling older passes) samples
  `column_lod` coarse then fine, `dwell-client` `chisel_section.rs` meshes it as ½ m-stepped blocks
  with top colours, a baked sun shadow and translucent water; heights exaggerated ×2–3 above 2 km.
  Deviation: no trees in it yet (the placement is per block); no cast shadow on walls beyond the
  baked top shadow.*
- **B4** water banks and hue, and the archipelago. *(built, 0.51.17: a biome's `banks` group
  (`riverSlope`, `lake`, `lakeSlope`, `beach`; `rivers.valleySlope` and `lakes.bank` moved from
  terrain.json into it, same values), blended by weight along the banks; an optional `waterLook`
  (`scatter`, `absorption`) over its fluid's look, resolved and blended per column by
  `TerrainGenerator::water_look_at` from the build's biome data that clients already generate with
  (nothing on the wire: a world's biomes are the build's, so no `WorldSettings` tag); terrain.json's
  world group `islands` (`amplitude`, `wavelength`, `octaves`, `cap`; amplitude 0, off, by default) breaks
  the coast into islands; the example world setting `shared/worldgen/worlds/archipelago.json` and the
  `test:archipelago` fixture biome (sand beaches, turquoise water, the pines as placeholder trees).
  Deviation: the game's water shader does not take the per-column look yet; Chisel's preview does.)*
- **B5** trees, ground items and structures per biome. *(built in part, 0.52.39 and 0.52.41: a biome's
  `placements` are now the rules that stand where it has weight, a placement no biome of the world
  lists standing nowhere, and its optional `trees` (placement name → density in [0, 3], 1 when not
  given, 0 none) multiplies that placement's chance by the biomes' weights (`TerrainGenerator::
  tree_density_at`, folded into the climate's factor on candidates and the LOD canopy; a planter's
  `boost`, the most any biome gives, raises its first roll's ceiling, 1 by default). The default world
  lists only the pines at 1, so it asks nothing new (goldens byte-identical). Two placement rules by
  data: `broadleaf_groves` (the prototype broadleaf `dwell:proto_tree`) and `boulders`
  (`dwell:proto_rock`, the "structures" a biome can pick from the structures store), listed by presets
  only; the presets pick (desert and mesa boulders, boreal and cold rainforest dense pines, meadow,
  savanna, wetland and tropical beach sparse broadleaf as the palm's placeholder). Chisel: a density
  slider per placement rule per biome, a `placements` overlay (structures expected per hectare,
  `placement_density_at`), and the 3D section draws trees and boulders as trunk and canopy boxes for
  squares up to 1 km (`trees_sparse`, the chunks' trees exactly). Ground items per biome (0.52.41): a biome's optional
  `ground` (ground item kind → density in [0, 3]; none: every kind 1, the default biome; given, a
  kind it does not name lies nowhere it rules, as with `placements`) multiplies the kind's rule
  densities by the biomes' weights. The weights reach the scatter as a per-chunk-column table,
  `dwell_world::ground::GroundDensity` (each kind's multiplier at the column's four corners, eased
  bilinearly), from `TerrainGenerator::ground_density_chunk`: the client's mesh jobs compute it in
  the worker that already holds the generator (`GroundJob::density`) and return it with the mesh for
  the crosshair; the server builds the terrain for it lazily, only when a biome of the world has a
  `ground` (`dwell_worldgen::ground_density_for`), and caches it per chunk; all pass it to
  `Rules::with_density`, so they agree. The presets: deserts, mesas and badlands more rocks and no
  sticks, the forests more sticks; `test:desert` rocks ×3. Chisel: a slider per kind per biome; the
  placements overlay colours each structure by its canopy block (trees green, boulders grey) and adds
  the ground items in their looks' colours (their count per hectare a guess, 1% of cells passing
  their rules, as the overview reads no voxels). Deviations, left for later: spacing per
  biome (the overlap test is per rule); the player's saved structures (the sandbox store) as placement
  sources (placement reads the build's `shared/structures` only).)*
- **B6** Chisel's texture tab (the Painterly Texture Lab folded in). *(Built 0.52.42: a **Textures** tab
  beside Biomes runs the build's painter (`shared/textures/paint.mjs`, the one `gen.mjs` bakes with) in
  the page, so an unchanged material previews bit-identical to the atlas; a block material's layers as
  sliders and palette pickers, its ramp's colours with the art direction's warnings (albedo range, a
  shadow warmer or a light cooler than its base), the tile, 3 × 3 and a cube; Export downloads
  `materials.json` and `palette.json` for `shared/textures/`; `?tab=textures&tex=<material>`. The 3D
  section draws a block's far colour, so an edit reaches it as a tint per block (the edited tile's mean
  over the built one's, linear light; `ChiselPreview.set_tints`), unedited blocks exact. The prototype
  page is kept (still generated and tested) until the owner retires it. Left for later: §5b's new
  materials and blocks, water and sky colours, item and model textures.)*
- **B7** Chisel in the game, and the new-world biome choice. *(Started 0.52.27: Chisel's **Play in
  game** opens a new local world on the edit's world knobs, its record keeping the edit as its
  worldgen config so it reopens the same, and `?worldgen=<fixture>` opens a fixture world by link
  (docs/DEV_TOOLS.md "Play a test world"). 0.52.37: the whole edit reaches the game. The parser moved to
  `dwell-worldgen` `config.rs`; a test world's maps carry the config's text (`WorldMap::worldgen`, a
  tagged trailer of the bytes, none on the build's maps), so every WASM instance that installs them (the
  server, the chunk and LOD jobs, the water) builds generators with the edit's biomes, biome map,
  geology, erosion and vegetation, and the map's tectonics are the config's. Deviations: the game's
  cliffs volume stays over the edit's ground; the New world form has no biome choice. 0.52.38: guests
  get the config: a required `WorldSettings` tag `WORLDGEN` (2) with the config's hash on the join's
  fields (not saved), `WorldgenRequest` / `WorldgenConfig` for the text (≤ 512 KiB, past the 1 KB field
  limit), checked against the hash, so a guest builds the host's maps and an older build refuses the
  world; the dedicated server's `--worldgen` (not saved in the world: given on every start).)*

  *Spacing per biome (0.52.65): a biome's optional `spacing` (placement name → blocks, 0..32, each one of
  its `placements`; biomes.mjs `checkSpacing`, `config.rs`) sets the blocks kept clear between two of
  that placement's structures anchored where it is the column's biome, in place of the placement's
  `spacing` (`placements::Spacing`, `TerrainGenerator::tree_spacing`); a pair takes the larger of their two.
  Only when some biome sets one (the default world asks nothing: goldens byte-identical).*

  *Players' saved structures as placement sources (plan, not built): the structures store
  (STRUCTURES.md) keeps a player's saved structure by name in the browser (and the world). A placement's
  `structures` could name `store:<name>` beside committed ones: the generator needs the structure's
  blocks on every instance (server, chunk and LOD jobs) and on guests, so it travels with the world's
  worldgen config (a Chisel edit carrying the structure's palette and cells, under the 512 KiB config
  limit, hashed with it), resolved by `Planter` from the edit, not the build's tables. Questions for the
  owner: per world or per player; whether a saved structure placed by worldgen may be edited after; a
  size cap.*

  *New world form (0.52.64): a **World** choice, Default or a world file that carries `newWorld` (a
  label and a line; `shared/worldgen/worlds/*.json`, checked by gen.mjs; `client/src/ui/worldChoices.ts`):
  the starter is offered. The made world records the file's name as its `worldgen` (the 0.52.27 record,
  append-only), so it reopens on the same ground and a guest gets it at join (0.52.38). A world file's
  `cliffs` (0..1) sets the terrain volume's strength in that world (0: none); the starter has none.*

  *Starter tuning (0.52.63): the biome map's `sharpen` (the pick's exponent on the weights; 1, the default,
  the arithmetic of before) is data in `terrain.json` `biomeMap`, overridable per world and tuned in Chisel;
  the starter world uses 2 (WORLD_GENERATION.md, the starter world). Chisel's **Open starter world** (File) loads
  the starter as an edit, resolved by `biomes.mjs` `fixtureWorldEdit` (moved from gen.mjs, so Chisel and
  the game's fixture agree). A biome's `decorations` are read (0.52.63): in a world of
  more than one biome a column grows only the decoration rules its biome lists (`decoration_at`, the
  biome by the column's palette; rock and snow palettes ask all, as before); one biome asks nothing (the
  default world unchanged). Outstanding in B5–B8: spacing per biome, players' saved structures as placement sources, the in-game editor, the New world form's
  biome/world choice, the textures tab extras.*
- **B8** arches (3D density, overhangs). *(Built in part, 0.52.43: a biome's optional `arches` (`density` sites per km², one chance per 32 m square; `span`, `rise`, `lintel` as [min, max] m; `fin`, the thickest fin cut; `overhangs`), `dwell-worldgen` `arches.rs`. A site is drawn per square from the world position (centre, sizes, the first of four axes), and its centre column's chosen biome gives its numbers; on the height field (the columns as the chunks build them), an axis whose ground `fin`/2 m to both sides lies `rise` + `lintel` under the centre is a **fin**: the opening (a half ellipse across the axis, its floor at the higher side's ground) goes through it, an arch; else, with `overhangs`, an axis low on one side is a **scarp**: the opening is cut into it from the low side's ground, an overhang (the cliffs and the erosion's mesa edges have many; fins are rare on seed 7's terrain, so the overhangs are what most sites make). Chosen over a caprock-lip overhang (adding rock past a softer undercut layer) because it only removes cells: a `lintel` of rock stays over the opening wherever the ground stands that high, and only at a mouth (a column whose ground is under the roof and lintel) is the column cut down to the floor, so the opening is open rather than sealed by a thin shell of the slope; the LOD, spawn, the maps, the water and the 3D section read the height field as before (they do not show the openings, and keep the mouth columns' old tops: Chisel's sliders say Play in game). Deterministic per site, so chunks agree at seams (tested); face-local, so face B's is the mirror (tested). Dry columns only. The default world has none (goldens byte-identical); the `red_rock_mesa` preset has them, and the `test:arches` fixture densely. Left for later: a `stack` filter (arches only in a named bedded rock), fins grown by the erosion for arches (today's terrain has few), the LOD and the 3D section showing the openings (a voxel slice), the caprock lip; trees placed on a mouth column's old top. Checked in game (bench, seed 7, a `?worldgen=` edit of the fixture): a 16 m arch through a stone wall reads as one, sky through it; it is a clean half ellipse, more cut than weathered. **Fins grown by the erosion, and arches in a chosen rock (0.52.66):** a biome's `arches.fins` (`approach`, `width`, `overrun` m; `arches.rs` `Fins`) is a fin rule in T4's cut-back: at a column, the drainage field's `fronts` gives the distance to the nearest drainage cell and to the nearest on the other side (the sources of the 4 × 4 map cells around it, the second more than a right angle from the first); where the two retreated fronts come within `approach` m of meeting, or have met by at most `overrun`, a fin `width` m wide stands on the crest between them (its margin `width`/2 − |d2 − d1|/2, kept if higher than the cut-back's, so walls and talus as at any scarp). With fins, a site whose centre is not a fin slides to one within 12 m (`site_near`: 6 and 12 m along the four axes both ways, the first arch; else the centre's overhang). `arches.rock` keeps only arches whose opening's mid-height lies in that rock of the site's stack (its `rockStack`, else the province's). The red-rock mesa preset has both (`approach` 150, `width` 12, `overrun` 400, `dwell:sandstone`). Measured on the starter's mesa (the 12 km square around the tour's spot, face A, seed 0): 1 arch and 15 overhangs before, 10 and 6 after (`tests/starter_world.rs` `the_mesas_fins_make_arches`; `mesa_arch_counts` prints them). Validated in `biomes.mjs` (`FINS`, the rock against rocks.json and, for a biome with its own `rockStack`, that stack's layers) and `config.rs` (the ranges and the rock's type), tested by data in `gen.test.mjs` and `tests/arches.rs`, with the filter itself tested on both faces (each kept site's mid-height in the named rock; a rock absent at arch heights keeps none); the default world has none (goldens unchanged). Still left: the LOD and the 3D section showing the openings, the caprock lip, an arch on a fin seen in game.)*

**The starter world (0.52.54).** The starter world (0.52.54): `shared/worldgen/worlds/starter.json` (`test:starter`) is a candidate default assembled by data alone: a fixture world may now carry its own `biomes` (by name, the default first, then presets, each a name or `{ "biome": name, ...keys }` replacing those keys; a renamed copy with `name`), `biomeMap` keys over terrain.json's and `geology` groups over rocks.json's; gen.mjs checks each biome and emits the Chisel edit it stands for (`FIXTURE_WORLD_EDITS`), which `config::world_config` uses for that name. The starter: a convergent boundary with a 600 m range and an arc of two Rainier-shaped 4800 m cones (tall_peak's keys; the arc's cones share one height today, so per-cone heights need an engine key), the snow and ice lines, a hotspot chain of four islands offshore east, the rain shadow on (`rainShare` 0.6), rock shaping on (`strength` 1; 0.25 from 0.52.55), and eleven biomes by climate (a broadleaf forest, the mixed forest, boreal, meadow, steppe, the warm coast, sand desert, red-rock mesa with erosion and some gullies, badlands, alpine tundra, wetland) with vegetation, ground and trees opted in, over the default at weight 0.25 as the fallback. Palms are not in the game: the warm coast keeps the beach preset's broadleaf groves. `tests/starter_world.rs`: it parses and generates on both faces, every listed biome wins ≥ 2 % of the land (seed 0: boreal ~20 %, steppe 13–17 %, alpine 11 %, desert 10 %, badlands 9–11 %, mesa 5–6 %, meadow 4–5 %, broadleaf 3–4 %, mixed 4 %, wetland 4 %, coast 3 %, default 9 %), the main cone stands ~4770 m; `STARTER_PNG=<dir>` writes the overview, and the ignored `tour` test prints `at=` spots. The default world is unchanged (goldens byte-identical). The tour (0.52.55, in game on the bench): with the rocks' shaping at full strength every hillside was bare stepped stone, the forests and the boreal hills alike, so the starter's `geology.shaping.strength` is 0.25: the broadleaf forest, the coast and the deserts read as themselves; the boreal spot's terraced hills still have no pines, and the range and the cones read best from far off.

**The old-growth forest (2026-11-12, WORK_QUEUE item 32; owner: "We should have a tall dense forest
biome/climate for performance reference on trees.").** `preset:old_growth`, a temperate rainforest: its
`where` cool and very wet in R4's terms (6–13 °C, 2200 mm of rain and more; the cold rainforest is 4–12 °C
from 1800 mm), gentle lowland detail, the `old_growth` placement only (the old-growth species,
docs/TREE_GROWTH.md §11, at `trees` density 1: ~220 a hectare up to 26 m tall), thick undergrowth (the
meadow grass and the `old_growth_undergrowth` decoration, tall grass nearly everywhere), more fallen
sticks. The tree density, the species' weights and their stages are data (the placement's `grid`,
`density`, `spacing`, `structures`; the species' `growth.wild`; the biome's `trees`), so a tool can thin
or thicken the stand. The Realistic world lists it (by climate alone); the starter lists it in its own
0..1 terms on the cool, wet lowland (temperature 0.34–0.5, wetness 0.6 and up, below 450 m), so the
starter's terrain changed there; the default world is unchanged (goldens byte-identical). The fixture
world `test:forest` (`worlds/forest.json`) is gentle land that is this forest everywhere: the tree
performance reference (docs/DEV_TOOLS.md, the bench's `forest-perf` and `forest:*` spots;
`tests/forest_world.rs`: every column old-growth, the stand's trees a hectare in range on both faces,
deterministic, its density changed by data alone). A placement's `spacing` may now be negative (to −4):
two kept boxes may overlap by that many blocks, crowns interleaving.

## Consequences

- A biome can be added by data alone (a test does, with a fixture biome that changes the generated
  columns' tops); Chisel will edit the same files.
- `HEIGHT`/`SURFACE` stay as the default's for the code that reads them as constants (the world's
  water shaping, the soil depth); the column's relief, rock band and bands read the biome map's.
