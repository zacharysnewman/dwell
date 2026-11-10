# Proposal: the biome sandbox and texture lab (tune with sliders, preview live, export JSON)

**Status:** recorded 2026-11-09 from the owner's request; queued **after the current work queue is
done**. Not architecture until an ADR adopts it. The owner asked for no further questions about it
until the other queued work is finished; the open questions below wait until then.

## 1. The request (owner, 2026-11-09)

> "I want to work on a biome sandbox. We should be able to configure what blocks should be on the
> top layer, how much elevation variation, how steep water banks are, water hue, placed structures,
> trees, rocks, etc. Essentially we should be able to make archipelago or Arches National Park, or
> Mount Rainier as biomes. Probably need more thought but it should just be sliders to tune and then
> we can export biome settings to json and you can configure it. So we'd need to be able to preview
> in mostly real time."

## 2. What it is

- **A biome is one data file**, e.g. `shared/worldgen/biomes/<name>.json`. It holds everything that
  makes a landscape read as that place. Each slider maps to one field.
- **An editor page:** the sliders on one side, a live 3D preview of a patch of terrain on the other.
  It regenerates the patch as you drag, within about ¼–1 s, and has **Export JSON** / **Import JSON**.
- **The game reads the same file** through the worldgen generator (`shared/worldgen/gen.mjs` →
  `dwell-worldgen`), so an exported biome is something the generator can use, not a mock-up.
- **The targets:**
  - an **archipelago**: many small islands, shallow sea, sand banks, palms or low trees;
  - **Arches National Park**: red sandstone mesas, buttes, arches and fins, sparse scrub, dry washes;
  - **Mount Rainier**: one great glaciated peak, snow line, conifer forest, meltwater streams.

## 3. What a biome would hold (first cut; one slider or picker per field)

| Group | Fields |
|---|---|
| **Surface** | top block, sub-surface block and depth, stone block; bands by height/slope (snow line, rock on steep faces) |
| **Elevation** | base height, relief amplitude, wavelength, octaves, ridge sharpness, terracing/mesa steps, one dominant peak (height, radius) |
| **Coast & water** | sea level share, island count/size (archipelago), bank steepness, beach width, water hue/clarity (a fluid look, `docs/WATER_RENDERING.md`), river density |
| **Features** | arches/fins/buttes (needs the 3D density work, `TERRAIN_HYDROLOGY_AND_VOLUMES.md` §2), cliffs, caves |
| **Life & scatter** | trees (which kinds, density, clustering), ground items (sticks, rocks: ADR 0071 rules), tall grass/plants, structures (which, how often, `docs/STRUCTURES.md`) |
| **Sky & light** (maybe) | fog colour/density, sun tint: a biome's mood |

Most of these already exist as numbers in `shared/worldgen/terrain.json` (`hills`, `detail`, `bands`,
`seaFloor`), `hydrology.json`, `decorations.json`, `placements.json` and `shared/ground/ground.json`.
The sandbox groups them per biome and puts a slider on each, rather than inventing a second system.

## 4. The preview: how to get "mostly real time"

- **Generate a small patch** (e.g. 256 × 256 m, then grow it) in the job workers, with the real
  generator built as WASM, and mesh it with the real mesher. Coarse LOD first, refined behind, so a
  slider drag shows within a frame or two.
- **Cache by field:** a change to surface blocks or water hue re-colours without re-running the
  height field; a change to relief re-runs heights only. Order the fields so the common drags are
  cheap.
- **A free camera and a "walk it" button** that opens the patch as a sandbox world (like the
  structures sandbox) to try it at player scale.
- **Live in the game too, later:** the in-game sandbox could reload a biome file while running.

## 5. Things to settle when the queue is done (not asked yet, by request)

1. **One biome per world, or several blended across the map?** Blending (a biome map with
   transitions) is far more work than one-biome worlds and a per-world biome choice.
2. **Where the editor lives:** a page in `docs/prototypes/` (like the Shape Playground), or a menu
   item in the game (like the structures sandbox), or both.
3. **Structures and trees as content:** pick from the existing sets, or author new ones per biome.
4. **Arches need 3D density** (overhangs): build the density pass from
   `TERRAIN_HYDROLOGY_AND_VOLUMES.md` first, or start with heightfield-only biomes (archipelago,
   Rainier) and add arches after.
5. **Compatibility:** a changed default biome changes what a seed generates (a new line); biome
   files a world picks at creation and saves avoid that for later edits.
6. **Water hue per biome:** per-world fluid look overrides (sent at join like `WorldSettings`).

## 5b. A texture lab beside it (owner, 2026-11-09)

"I talked about a biome lab, we also want a texture lab." A start exists: the **Painterly Texture
Lab** (`docs/prototypes/painterly-texture-lab.html`, published at
https://claude.ai/artifact/4CTkNW4G6p8p67MkSNMUJA, Phase 39, ADR 0040). It runs the game's own painter
(`shared/textures/paint.mjs`): you pick a material, edit its brush layers and the palette with
sliders and colour chips, see the tile, a 3 × 3 repeat, a lit block and its far colour, and copy the
JSON back into `shared/textures/`. What a texture lab next to the biome lab would add (to settle when
the queue is done, with the biome questions):

- **Live in the biome preview:** a material edited in the texture lab repaints the biome patch
  straight away (one page, or two pages sharing the data).
- **Export like the biome:** download/import the material JSON as a file, not only copy and paste.
- **New materials, not only edits:** add a material and a block that uses it, by data, and see it in
  the preview (the block registry's add-by-data path).
- **Water and sky colours** from the palette (PAINTERLY_TEXTURES.md §9: the fluid's colours are
  separate data today), so a biome's water hue and its textures are tuned in one place.
- **Item and model textures:** the skinned models' palette remap (ADR 0070) and item looks
  (`shared/inventory/inventory.json` `look`) shown and tuned in the same lab.

## 6. Rough phases (each ≤ 45 min for an agent, to be refined)

1. **B1:** the biome file format, its generator and validation; today's terrain expressed as the
   default biome (same output, a golden test).
2. **B2:** the editor page with the preview patch (heights + surface), Export/Import.
3. **B3:** water bank steepness, hue, the archipelago sliders.
4. **B4:** trees, ground items, structures in the biome.
5. **B5:** the in-game "new world from biome" choice.
6. **B6+:** 3D features for arches, blended biomes, per the answers to §5.
