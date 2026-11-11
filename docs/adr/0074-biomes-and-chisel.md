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
   the world's parameters. Its weight in a column is `weight` × each range's fit; **the default biome is a fallback** (B3, 0.51.16): it takes only what is left of 1 after the others (× its own fit), so a biome that fits a place takes it at an ordinary weight of 1; the weights are then normalised. `bands` is
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
- **B5** trees, ground items and structures per biome. *(built in part, 0.52.39: a biome's
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
  squares up to 1 km (`trees_sparse`, the chunks' trees exactly). Deviations, left for later: ground
  items per biome (the scatter runs in the mesher and the server from voxels alone, with no biome at
  hand: it needs a per-chunk biome table on `GroundJob` and the server's pick-up check); spacing per
  biome (the overlap test is per rule); the player's saved structures (the sandbox store) as placement
  sources (placement reads the build's `shared/structures` only).)*
- **B6** Chisel's texture tab (the Painterly Texture Lab folded in).
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
- **B8** arches (3D density, overhangs).

## Consequences

- A biome can be added by data alone (a test does, with a fixture biome that changes the generated
  columns' tops); Chisel will edit the same files.
- `HEIGHT`/`SURFACE` stay as the default's for the code that reads them as constants (the world's
  water shaping, the soil depth); the column's relief, rock band and bands read the biome map's.
