# Dwell — Transparent Blocks: Render Modes, Cross Geometry and Tall Grass

> **Status: [built, Phase 33; the owner's look outstanding]** — built on `claude/transparent-blocks` with the defaults in
> §2 (CLAUDE.md's design questions were not put to the owner first); the owner's answers, which
> changed some defaults, are in §8. Decision record: [ADR 0036](./adr/0036-render-modes-and-cross-geometry.md).
> The built mechanisms are described in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.1 (render modes,
> the cross look, placement rules), §6.5 (edits) and §5 (`dwell-mesh`, `render/`).

## 1. The brief

Owner, 2026-10-15: "semitransparent block handling with blades of grass billboards as the starter
block". Added the same day: ladders draw a black background between their rails and rungs; make them
see-through with the new mode.

## 2. Defaults used at first (see §8 for the owner's answers)

The coordinator's defaults, in place of answers to "what gets added later, by whom, with what tool,
what stays code, per build or per world":

- **What grows**: more see-through blocks of the existing kinds (foliage, flowers, glass, ice,
  leaves) as **data entries**; new geometries (beside `cross`) and new render modes are **code**, a
  small fixed vocabulary.
- **Who adds them, with what**: the block data file (`shared/blocks/dwell.json`), by hand, like
  every block today; procedural tiles in `dwell-mesh` `textures.rs` until textures become data
  (Phase 19).
- **Per build**: the registry is per build; nothing is per world or per player. The wind's
  strength is per block; a global wind scale is a renderer setting (§5).
- ~~World generation doesn't place grass~~ — reversed by the owner (§8.2): it grows in meadows (§10).

## 3. Render modes

A block's `render` is one of:

| mode | drawn | depth | order | users |
|------|-------|-------|-------|-------|
| `opaque` (default) | every pixel it covers | writes | any (near to far for early rejection) | cubes |
| `cutout` | where the texture's alpha passes 0.5; alpha-to-coverage under MSAA | writes | any | ladders, tall grass |
| `translucent` | blended by its opacity | tests, doesn't write | far to near | water |

`gen.mjs` validates the mode, requires `translucent` for `opacity < 1` (air excepted) and a texture
for `cutout`. The chunk mesher writes one mesh per mode (`ChunkMeshes::{opaque, cutout,
transparent}`). A see-through neighbour hides no face; faces between two cells of one see-through
block are skipped (glass panes, leaves later).

**Edges.** With 4× MSAA the cutout shader does alpha-to-coverage itself: the texel's alpha is
sharpened to its edge over one pixel (`(a − 0.5) / fwidth(a) + 0.5`), and that share of the pixel's
samples is written through `@builtin(sample_mask)`, with the samples chosen per pixel by a noise so
two overlapping half-covering blades cover more than one. Covered samples write alpha 1, so the
frame's premultiplied coverage (the shared frame, A1) stays right where blades meet the sky.
Without MSAA (CPU adapters) it is a plain test.

**Distance.** Averaging alpha down the mips thins a blade a texel wide to half alpha, under the test
— distant grass would vanish. The atlas mips of tiles with transparent texels are rescaled so each
level covers the same share of the tile as the full-size tile (coverage-preserving mipmaps).

## 4. The cross geometry

`"look": "cross"` with options:

```json
"cross": { "height": 0.9, "quads": 2, "jitter": { "offset": 0.2, "rotation": 25, "height": [0.8, 1.0] } }
```

- `quads` (2–4) vertical quads through the cell's centre, evenly turned (two: an X seen from
  above; three is the "extra quad"), each a full cell wide on the diagonal, `height` tall.
- Double-sided: drawn without culling, each face once.
- **Jitter**, seeded by the cell's world position (deterministic, the same on every client and
  every re-mesh): an offset in x and z up to `offset` m, a turn up to ±`rotation`°, and a height
  scale in `height` [min, max].
- **Normals** lean up (mostly up, a little of the quad's own normal), so grass is lit like the
  ground it stands on rather than dark from the side; the back face mirrors only the horizontal
  part. The tint is the face tint of that normal: near the grass top's.
- **Wind**: `"wind": { "strength": 0.08, "speed": 1 }` — the mesher writes each vertex's sway
  (0 at the base, `strength` × the plant's height scale at the top) and the speed into the cutout
  vertex's `plants` stream; the vertex shader leans the top along a slow gust field (travelling
  waves whose wavelengths divide 1,024 m, sampled at the world position modulo that, so it's
  seamless and stays put as the eye moves) plus a faster flutter. `Renderer::set_plant_wind` scales it (`__dwell.plants(wind, shown)` in the dev tools)
  (0 turns it off): the settings menu's **Plant sway** toggle (Motion group; `plantSway` in the
  append-only motion record), off by default when the browser prefers reduced motion.
- **Fade**: `"fade": { "start": 40, "end": 72 }` — plants fade out (fewer samples covered; dithered
  without MSAA) between those distances, which must end within the smallest full-detail distance
  (96 m, `gen.mjs` checks it), so plants are gone before chunks meet the LOD and nothing pops. The
  LOD draws cutout blocks without a solid as air.
- **No ink**: `"outline": false`, the cross plants' default (`true` outlines one): the ink outline
  draws neither on the plant nor on its edges against what is behind it (ART_DIRECTION.md §7.4).
  Per vertex in the cutout mesh's `bare` stream; the cutout pass flags the view depth.
- **Sitting on the support** (owner, 2026-10-20: *"Make the grass align (sit down) when over a half
  block"*): `"cross": { "sitOnSupport": true }`, the default. Adaptive natural tops are often lower
  than their cell's top (a level field rests at half height; slopes and pyramids are lower still
  under parts of the cell), so a plant at its cell's floor floated up to ½ m above the ground. A
  plant that sits is drawn on its support's solid: each foot is lowered by
  `dwell_blocks::shape::plant_sink` (the gap between the support cell's top and its solid's top
  under that foot, `solid_span_at(shape, x, z).hi`; on face B the mirror) and the top by the same
  amount, so a plant on a slope follows it foot by foot, keeps its height, and still sways only at
  the top. Over a full cube, or nothing, nothing changes. The mesher reads the support's derived
  shape exactly, even in the apron's bottom row; an edit beside the support re-meshes the plant's
  chunk through the usual neighbour marking (an edit marks every chunk within one cell of it, and
  every cell that changes a support's shape is within one cell of the plant). `false` keeps the
  plant on its cell's floor (the fixture `test:reed` shows it by data alone). Registry sync: an
  optional last byte of the `CROSS` look field, written only when false, so payloads for sitting
  plants are unchanged.
- Crosses have `"shape": "empty"`: no collision, players walk through.

## 5. Placement rules

Generic block data, checked by the server's edit validation (`dwell_world::edit::check_block_edit`):

- `"placeOn": ["dwell:grass", "dwell:dirt"]` — the block below (toward its face's ground) must be
  one of these; otherwise `EditCheck::NoSupport`.
- `"replaceable": true` — placing into its cell replaces it, and clicking it to place puts the new
  block in its cell rather than beside it.
- `"needsSupport": "below"` — when the block below changes so the rule above no longer holds
  (broken, or replaced), the block is removed too (left as air, or water if flooded), in the same
  `VoxelModification` batch (`dwell_world::edit::unsupported_after`).
- `"selectionBox": [x0, y0, z0, x1, y1, z1]` — a smaller box in the cell for targeting, the
  server's line-of-sight check and the outline (`dwell_world::edit::target_box`). On face B it is
  turned to stand on that face's ground; a plant that sits on its support (§4) sinks it by the
  support's `plant_sink` under the box's middle, so it may reach into the support's cell, where the
  targeting ray also meets it.
- `"medium": "air" | "water"` — what the block may stand in. **Air** (tall grass): placing it into
  a liquid is refused (`EditCheck::WrongMedium`), a region edit skips those cells, and an air
  plant that would end up flooded gives way to water in the same batch (water doesn't flow yet,
  so today only edits can bring it). **Water**: placed only into water cells, holding the water
  (it needs a `flooded` property, `gen.mjs` checks; placement sets it true), and the mesher draws
  the water in its cell around it. Absent: no rule. No water plant exists yet: the test fixture
  `test:kelp` shows one working by data alone.

"Breaks instantly": every edit is instant in creative today; nothing was needed.

## 6. The starter block: `dwell:tall_grass`

Prototype content (ARCHITECTURE.md §6.1), appended to the block list (earlier ids are kept, so terrain
hashes don't change). Shape empty; cross look, cutout; a procedural tile of several tapering blades
in the grass block's top colours with transparent gaps (`tallGrass`); wind; fade; placeable only on
grass or dirt; replaceable; needs support below; a smaller selection box. Sounds: the sound-effects
set is the grass block's (`dwell:grass`).

## 7. Other systems

- **Physics**: shape empty, `solid: false`: no collider; a test walks a player through.
- **Targeting**: the selection box (above); breaking it is a normal break.
- **Ray marching** (Phase 25, `dwell-march`): its state classes count every state whose `render`
  isn't `opaque` (ladders, tall grass, future glass) as `Transparent`, like water: never occupied
  for primary rays, drawn by a raster pass over either renderer (a test pins ladders and tall grass).
- **Region edits** (Phase 27 S3): a fill's cells that need support but have none (tall grass filled
  into the air, or stacked on itself) are not kept; clearing the ground takes the plants with it.
- **Sounds** (Phase 28): tall grass uses the grass sound set (`"sound": "dwell:grass"`).

## 8. The owner's answers (2026-10-16)

1. **Compatibility**: tall grass ships in the 0.13.0 bundle (with wood and leaves) (a new compatibility line). The ladder
   change ships on its own as a 0.11 patch (0.11.18).
2. **World generation grows tall grass** (§10): a partial reversal of the baseline's B10 ("no
   special terrain features") **for plants only** — trees, rocks, caves and the rest stay cut.
3. **Leaves, glass and ice** will use `cutout` and `translucent` — **future work**: not added now
   (a cutout cube and a translucent cube already work by data, shown by the test fixtures).
4. **An editor page** for blocks like these (jitter, wind, tiles) is wanted later — **future work**
   (in the plan, Phase 33's follow-ups).
5. **Wind sway setting**: yes — a **Plant sway** toggle in the settings menu's Motion group, kept in
   the motion record (append-only), off by default when the browser prefers reduced motion (§4).
6. **Plants under water**: only plants made for it — block data **`medium`** (§5): tall grass is an
   air plant; a water plant is placed only into water and drawn inside it. No underwater plant is
   added yet; a test fixture (`test:kelp`) proves the rule.

## 9. Browser check and cost (2026-10-15)

Chrome on the GPU (Apple M-series, 1280 × 720, 4× MSAA), a local terrain world (seed 7), tall grass
placed through the edits like a player (a field of about 150 plants), ladders on stone posts and in
the sea. Blade edges are smooth curves up close (the hand-filtered alpha over the distance-field
tile), thin and pointed; no dark sides from any direction; plants vary in offset, turn and height;
they sway; from a distance the field reads as grass and fades out before 88 m; ladders show the
sky, the wall and the water through their gaps, and a flooded ladder sits in unbroken water.
Remaining nits for the owner's look: a few single samples of distant blade tips near the horizon,
small notches where two leaning blades' tips meet, and the crossed quads' straight lines at the
foot of a dense clump.

**Cost** (`__dwell.plants(1, true|false)`, alternated three times at one camera pose, 5 s each,
unthrottled; the machine was shared, so single runs varied by ±1 ms): `gpu.near` +0.33, +0.40 and
−0.18 ms with the plants drawn (about +0.3 ms), frame CPU unchanged within noise, +4 draw calls,
+656 triangles. Memory: 656 bytes per plant (8 vertices of 76 bytes, 12 indices), in the cutout mesh
of its chunk only.

## 10. Tall grass in world generation (owner, 2026-10-16)

A **decoration rule** is data — `shared/worldgen/decorations.json`, validated against the block data
by `shared/worldgen/gen.mjs` into `crates/dwell-worldgen/src/decorations.gen.rs` (CI regenerates and
fails on a diff, and runs its node tests):

```json
{ "name": "meadow_grass", "block": "dwell:tall_grass", "on": ["dwell:grass"], "density": 0.55,
  "patches": { "wavelength": 40, "coverage": 0.45 }, "maxSlope": 1, "height": [3, 400] }
```

- The block put one cell above the ground (an air plant, shape empty); the ground blocks it stands
  on (each must be in the block's `placeOn`); a density inside noise **patches** (meadows, not a
  lawn); the steepest slope (the most the ground's top differs from its four neighbours'); the
  ground's height range (face-local, above sea level).
- **Deterministic and order-independent**: each cell is decided from the world seed and its
  position alone — the patch noise, a per-cell hash, its ground and its neighbours' tops through
  `column_at`, the chunk path's own arithmetic — so chunks agree across their borders and generate
  in any order, bit-identically in WASM (ADR 0010). The sky test leaves one cell above the highest
  ground for them.
- **Generator version 13**: seed terrain changes (10 of the 39 chunk goldens, the ones whose
  ground grows grass); the LOD hashes don't — the LOD never draws cutout plants without a solid,
  and their distance fade (≤ 88 m) ends before chunks meet it.
- **Interim home** (a deviation): Phase 17's terrain definitions (ADR 0026) don't exist yet, so the
  rule lives in its own file beside the generator's goldens; it is one data entry the terrain
  definitions take over when they come.
- **B10**: the owner's decision is a partial reversal of the baseline's "no special terrain
  features" for plants only (ADR 0036).

Screenshots: a grassy hillside near and far (the PR).

## 11. Future work (the owner, 2026-10-16)

- **Leaves, glass and ice** as `cutout` and `translucent` blocks: data entries on the systems built
  here (a fixture cutout cube and translucent cube already mesh correctly).
- **An editor page** for these blocks' data (cross options, jitter, wind, the tile), like the
  prototypes in `docs/prototypes/`.
- **Water plants**: the `medium: water` rule is built and tested by fixture; the first such block is
  content for later.
