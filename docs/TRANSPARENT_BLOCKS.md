# Dwell — Transparent Blocks: Render Modes, Cross Geometry and Tall Grass

> **Status: [in progress, Phase 33]** — built on `claude/transparent-blocks` with the defaults in
> §2, recorded here because CLAUDE.md's design questions were not put to the owner first; the open
> questions are in §8. Decision record: [ADR 0036](./adr/0036-render-modes-and-cross-geometry.md).
> The built mechanisms are described in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.1 (render modes,
> the cross look, placement rules), §6.5 (edits) and §5 (`dwell-mesh`, `render/`).

## 1. The brief

Owner, 2026-10-15: "semitransparent block handling with blades of grass billboards as the starter
block". Added the same day: ladders draw a black background between their rails and rungs; make them
see-through with the new mode.

## 2. Defaults used (not yet confirmed by the owner)

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
- **World generation doesn't place grass** (a seed's terrain is unchanged, and the baseline, B10,
  cut features): grass is placed and broken by hand only.

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
  seamless and stays put as the eye moves) plus a faster flutter. `Renderer::set_wind` scales it
  (0 turns it off; the hook for an effects-quality setting, §8).
- **Fade**: `"fade": { "start": 40, "end": 72 }` — plants fade out (fewer samples covered; dithered
  without MSAA) between those distances, which must end within the smallest full-detail distance
  (96 m, `gen.mjs` checks it), so plants are gone before chunks meet the LOD and nothing pops. The
  LOD draws cutout blocks without a solid as air.
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
  server's line-of-sight check and the outline.

"Breaks instantly": every edit is instant in creative today; nothing was needed.

## 6. The starter block: `dwell:tall_grass`

Prototype content (ARCHITECTURE.md §6.1), appended to the block list (earlier ids are kept, so terrain
hashes don't change). Shape empty; cross look, cutout; a procedural tile of several tapering blades
in the grass block's top colours with transparent gaps (`tallGrass`); wind; fade; placeable only on
grass or dirt; replaceable; needs support below; a smaller selection box. Sounds: the sound-effects
work (Phase 28, ADR 0032) isn't on this branch's base; when it lands, tall grass takes the grass
sound set in its data.

## 7. Other systems

- **Physics**: shape empty, `solid: false`: no collider; a test walks a player through.
- **Targeting**: the selection box (above); breaking it is a normal break.
- **Ray marching** (Phase 25, `dwell-march`): not on this branch's base. When it lands, its state
  classification must count `cutout` states as not occluding primary rays (Transparent, or Shaped
  for crosses drawn as geometry), not as solid bricks.

## 8. Open questions for the owner

1. **Compatibility**: tall grass changes the registry hash, so it needs the 0.12 line. Raise
   `client/package.json` to 0.12.0 when it should ship? (The ladder change alone keeps the hash.)
2. **World generation**: should grass grow on generated terrain (which changes a seed's terrain,
   another compatibility decision), and if so by what rule (density, biome-free noise)?
3. **What else comes next** as cutout or translucent data: flowers, leaves, glass, ice? Each is a
   data entry; leaves (cutout cubes) and glass (translucent cubes) work today.
4. **An editor**: should blocks like these be tuned in a page like `docs/prototypes/` (jitter,
   wind, tile), or is the data file enough?
5. **Effects setting**: there is no effects-quality setting yet; wind has a renderer scale. Add an
   Effects setting (wind on or off, later more) to the settings menu?
6. **Grass under water**: placing grass into water replaces the water (it has no `flooded`
   property). Should plants be placeable under water (a `flooded` property), or refused there?
