# Dwell — Painterly Textures

One data-driven generator paints every texture in the storybook style of
[`ART_DIRECTION.md`](./ART_DIRECTION.md): block tiles for the atlas, and the materials the characters
and sky accents read by name. Decision record: [ADR 0040](./adr/0040-painterly-textures.md).
Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.1 (block textures).

**Status: built; the game's texture set.** The owner approved it as the default on 2026-10-20
and removed the earlier procedural set on 2026-10-21 (ADR 0040), so every atlas tile, the utility
tiles included, is painted from this data. A second **style**, cel (§10), paints the same materials
in a few flat tones for the pixel-art preview only, as a prototype; the normal look never draws it.

## 1. Brief and answers

Owner, 2026-10-20: textures for everything (blocks, plants, water, sky accents, characters), all
generated. One procedural generator, driven by data, of brush strokes, painted highlights and clean
gradients, baked at build or load time. The character agent and this one share one palette, brush
vocabulary and lighting.

The design questions (CLAUDE.md, "Ask before designing"), answered by the owner's brief:

| Question | Answer |
| --- | --- |
| What is added later? | More materials (entries of the same kind) and new palette colours; occasionally a new stroke kind (code, small) |
| Who adds them, with what? | Whoever designs content, by editing `shared/textures/*.json`, live in the Painterly Texture Lab (`docs/prototypes/painterly-texture-lab.html`), which copies the JSON back |
| Fixed in code | The layer kinds, the stroke shapes and how strokes are placed (§3) |
| Data | The palette, every material and its layers, which atlas tile each block material paints |
| Per build, world or player? | Per build: the textures are baked into the build |

## 2. Data

- **`shared/textures/palette.json`**: `ramps` (name → `{shadow, base, light}`) and `swatches` (name →
  colour). See ART_DIRECTION.md §2.
- **`shared/textures/materials.json`**:
  - `tileSize`: 32, which must equal the atlas's `TILE`.
  - `materials`: name → material:
    - `use`: `block`, `character` or `sky`.
    - `ramp`: the material's ramp.
    - `overlay`: 0–1, how strongly the strokes show over the flat ramp (characters).
    - `wrap`: whether the tile is seamless. True by default; false for sprites.
    - `hang`: a side tile that hangs from each block's top edge (the turf skirts): every ½ m block
      shows the tile's top half and its side faces never merge up or down. False by default: a
      tile then runs in world metres, a 1 m tile across two cells.
    - `alpha`: `opaque` (the default) or `strokes`. With `strokes`, only strokes cover the tile, which
      makes a cutout.
    - `emissive`: 0–4, how much of the surface's lit colour its self-lit colour replaces (1 or
      more: fully self-lit, a flame; [`LIGHTS.md`](./LIGHTS.md) §2): unaffected by the scene's
      light and the exposure, kept out of the night's desaturation. Written per atlas tile as
      `PAINTERLY_EMISSIVE` (the terrain and cutout shaders read it from the `tile_grids` table) and per
      character material (`Material::emissive`). The torch's flame tile `torchTop`, the held
      `flame` and `flame_core` are 1.0.
    - `like`: start from another material's fields (a template; e.g. `cloth_wool_blue` is
      `cloth_wool_red` with ramp `wool_blue`).
    - `over`: paint another material first (e.g. `grass_side` is painted over `soil`).
    - `layers`: the layers, painted in order (§3).
  - `tiles`: atlas tile name → the block material that paints it. Every atlas tile is mapped, and
    nothing else: the registry's `textures` names (`shared/blocks/dwell.json`) and the engine's
    utility tiles (`ENGINE_TILES` in `gen.mjs`: `plain` and `missing`).

**Utility tiles.** Three tiles are not content, and their materials (`plain`, `grid`, `missing`)
use the palette's `chalk` ramp and `missing_*` swatches, outside the content albedo range:

- `plain`: white, which an untextured state's colour tints.
- `grid`: the sandbox floor. White with a one-texel `chalk.shadow` line along the bottom and left
  edges (two `edgeLight` layers), so every block edge gets one line; mipmapping fades it with
  distance, and the terrain shader's `grid` look option rules the larger cells.
- `missing`: the fallback look of a state whose look or textures this build lacks (synced from a
  server, ADR 0038): magenta and black squares of 8 texels (hard-edged `slab`s at fixed `at`
  positions over a black fill), unmistakable for content.

Colours are palette references, never hex (ART_DIRECTION.md §2).

**Side tiles from the block above.** Owner, 2026-10-20: "I want to see the grass skirt fringe on
dirt that's below sloped grass." On generated hillsides (½ m cells, Phase 41) the grass tops are
flat cells and grass ramps, and at a step or a cliff the face below a grass cell or ramp is a dirt
block's side. A block's
`sideUnder` in the registry (`shared/blocks/dwell.json`; BLOCK_REGISTRY.md) maps a block id above
to the side tile to use: dirt has `{ "dwell:grass": "dirtSkirt" }`, which the generator extends to the grass ramps
(`dwell:grass_ramp`). The chunk mesher applies it to
a side face whose cell above holds that block, whatever shape the block takes (a ramp piece), unless that block shows a full side on the same face itself (a full grass cube already
hangs its own skirt, so the dirt below it stays plain). Stone or air above: no skirt. On face B,
"above" is toward its sky. The tile `dirtSkirt` is painted by `dirt_skirt`, which is
`"like": "grass_side"`: the same soil and the same skirt strokes, so it lines up with a grass side
beside it and wraps into the next dirt face; it is its own tile so its art can diverge by data. The
LOD draws no skirt tile: a coarse riser takes the fringe band's colour by its share of the fine
rise it stands for, which is small on a cliff (ARCHITECTURE.md §6.6 "Colour"), so cliffs stay
dirt and gentle slopes green.

## 3. The generator

`shared/textures/paint.mjs` is a pure ES module with no dependencies. The build, the tests, the editor
page and the character prototypes all run the same code. `paintMaterial(name, materials, palette)`
returns a 32 × 32 RGBA tile, row y = 0 (the bottom) first, as the atlas lays tiles out.

**Layer kinds** (the fixed vocabulary):

| kind | fields |
| --- | --- |
| `fill` | `color` |
| `gradient` | `from`, `to`, `direction` (up, down, left, right, radial), `start`, `end`, `steps` |
| `edgeLight` | `color`, `edge` (top, bottom, left, right, all), `width`, `ragged`, `seed` |
| `dab`, `streak`, `blade`, `slab`, `ring` | `colors`, `count` or `at`, `band`, `jitter`, `width`, `length`, `angle`, `angleJitter`, `bend`, `softness`, `arc` (ring), `seed` |
| any | `opacity` (or a [min, max] range per stroke), `clip` (paint only where already covered) |

**Strokes.** Each stroke is a signed distance field in its own frame (along `angle`, across it). Its
coverage is `clamp(0.5 − d / softness)`, a one-texel anti-aliased edge by default. A cutout's alpha
is that same distance field, so the shader's alpha test traces the true outline up close. Positions come from a jittered grid of `count` cells inside `band` (shares of
the tile's height), with odd rows shifted half a cell and never more columns than strokes (so a thin
band, such as the grass side's hanging skirt, spreads its strokes over the whole width), or from `at` (`[x, y]`, or `[x, y, length]`
for a ring's radius or a fixed length). Random choices use the atlas's int32 hash, seeded per layer,
so a tile is the same on every run.

**Wrapping.** On a wrapping tile, each stroke is also drawn shifted by ±32 texels in x and y, so the
tile is seamless. A sprite (`wrap: false`) draws each stroke once.

**Compositing.** Colour mixes over the layers below in sRGB, by coverage × opacity. With `alpha:
strokes`, alpha is the maximum coverage of the non-`clip` stroke layers. Fills and gradients still
colour the gaps, so filtering at an edge adds no fringe.

## 4. The bake and the atlas

`node shared/textures/gen.mjs` validates the palette and materials against the atlas's tiles (the
registry's tile names and the engine's), then paints each `tiles` entry. It writes:

- `crates/dwell-mesh/src/painterly.rgba`: the tiles' RGBA8 texels, in `tiles` order (4 KiB a tile).
- `crates/dwell-mesh/src/painterly.gen.rs`: `PAINTERLY_TILES` (their names) and `PAINTERLY_TEXELS`
  (`include_bytes!`).

Both are checked in. CI regenerates them and fails on any diff (the protocol job), and runs
`node --test shared/textures/gen.test.mjs`. `--png <file>` also writes a preview sheet of every
material.

**In the client** (`dwell-mesh` `textures.rs`): one texel lookup into the baked tiles serves the
atlas (`build_atlas`), cutout detection (`tile_has_alpha`), the tiles' average colours
(`average_tile_color`, the LOD's flat colours) and the hotbar swatches; a name the set doesn't paint
takes the `plain` tile. The atlas, its mipmaps and their coverage-preserving alpha (`render/mod.rs`)
are built once at start-up; the averages and the LOD colour table once per context (the page and
each meshing worker), from the same baked texels, so they agree with no message between them.

## 5. Validation and tests

A bad entry fails with its path named, for example `materials.json: materials.stone.layers[1].width
must be [min, max] in [0.25, 32]`. The checks cover:

- Unknown keys, kinds, ramps, stops and colours.
- Out-of-range numbers, and a layer with both or neither of `count` and `at`.
- A cycle in `like` or `over`.
- A tile mapped to a missing or non-block material.
- In the block data, a `sideUnder` naming an unknown block, a value that is not a tile name, an empty
  rule set, or a rule on a block without textures (`shared/blocks/faces.test.mjs`).
- An atlas tile not painted, and a painted tile the atlas doesn't have.

`shared/textures/gen.test.mjs`:

- The checked-in bake is current.
- Painting is deterministic.
- **A material added by data alone** is painted, mapped to a new tile and read by name.
- Templates recolour and keep the strokes.
- Tiles are seamless.
- Cutouts cover only strokes, and sprites keep clear of the tile's edges.
- Every bad case above fails with its message.
- The utility tiles: `plain` white, `grid`'s one-texel edge line, `missing`'s checker.
- The character materials exist with ramps.

`dwell-mesh` (Rust):

- Every atlas tile is painted at the atlas's size (the utility tiles too), and nothing else is.
- The atlas places every tile with its wrapped gutter.
- The utility tiles: `plain` white, `grid`'s line and near-white average, `missing`'s checker.
- Opaque blocks stay opaque.
- The ladder's rails, rungs and gaps, the tall grass's clear top row and sides, and the leaves'
  coverage are as the meshes assume.
- The tall grass averages near the grass top (the far view).
- A dirt side under a sloped grass block takes `dirtSkirt`, on face A and face B; under air, stone or
  a full grass cube it stays `dirt`. A fixture block's `sideUnder` (`test:loam`, by data alone) is
  applied (`mesher_tests.rs`).

## 6. Compatibility

Textures are not part of the public API. They change no world format, protocol or terrain, so a
change to the data works on any server of the line.

## 7. Performance

The tiles are baked at build time and uploaded once as one atlas with its mip chain. Start-up copies
the baked tiles; nothing is painted at run time.

Measured 2026-10-20 in Chrome on an Apple GPU (Metal), with `npm run bench -- --compare`, before
this set became the only one: it ran at 60 fps with no dropped frames, and its GPU times matched the
procedural set it replaced to within run-to-run noise (p95 `gpu.frame` 8.49 ms at spawn looking at
the ground, 9.34 ms 600 m up, 11.79 ms over the sea).

## 8. The editor

`docs/prototypes/painterly-texture-lab.html` (published, see `docs/prototypes/README.md`) runs
`paint.mjs`'s code. You pick a material, edit its layers and the palette as JSON or with sliders,
see the tile, a 3 × 3 repeat and a block lit from above, and copy the material or palette JSON back
into `shared/textures/`.

## 9. Outstanding and later

- Water's surface is drawn by the water shader from its fluid look (`fluids` in
  `shared/blocks/dwell.json`, ADR 0030), not from the atlas. The painterly `water` tile only changes
  where the tile itself is used (the hotbar, a tile's average). Taking the fluid's colours from the palette would be a change to the fluid data, and is
  left for later.
- The sky does not draw `cloud_puff` yet. Clouds are shaded analytically
  ([`CLOUDS.md`](./CLOUDS.md)).
- Characters are not in the game yet. Their materials are read by the character prototype (ADR
  0041).
- A 64-texel atlas would allow finer brushes up close. That is a change to the atlas's `TILE`, not to
  the data's vocabulary.

## 10. A second style: cel (a prototype for the pixel-art preview)

**Status: prototype, off by default.** Owner, 2026-10-21: "I think we need to try out a custom
texture set for the pixel art look. It looks like it's better to use fewer colors per texture, with
more of a cell shaded look to the textures", and "the complexity in the textures currently makes them
shimmer a lot when moving around." The pixel-art preview ([`proposals/PIXEL_ART_LOOK.md`](./proposals/PIXEL_ART_LOOK.md))
found that the painterly strokes turn to speckle once quantized; cel is the flat-tone set it
recommended. Only the preview's `270-cel` and `360-cel` presets draw it (`textures` in a preset,
`shared/look/pixelart.json`); nothing else changes. It is not adopted: an ADR would adopt it with the
pixel-art look.

**The design questions** (CLAUDE.md, "Ask before designing"), answered from the brief: what is added
later is more recipes (one per material, or none: a default per use) and occasionally a pattern kind
(code, small); whoever designs content adds them in `shared/textures/cel.json`; the pattern kinds
are code; the style is per build, picked per preset.

**Data: `shared/textures/cel.json`.**

- `tileTexels` (16): texels per tile edge, a divisor of the atlas's `TILE` (32), each cel texel
  repeated to fill it. Low on purpose: at the 270-row preview (the default 75° FOV) a 1 m face covers about
  175 / d low-res pixels at d metres, so a 16-texel tile keeps a texel at least a pixel to about 11 m,
  and the mips (below) take over beyond.
- `maxTones` (4): the most colours a tile may use; the bake fails over it, naming the tile.
- `mips`: `mode` (each 2 × 2 of an opaque tile halves to its majority tone, a tie to the tone that
  covers more of the tile) or `average`. With `mode` a distant face keeps exactly the tile's tones and
  its small shapes drop out level by level until it is its main tone, rather than averaging into
  in-between colours that the palette dithers, which crawl as the camera moves. Cutout tiles average
  (and keep their coverage) as before.
- `palette` (`dwell`) and `paletteExempt` (the utility tiles): every tone of every other tile must be
  a colour of that pixel-art palette, so a face lit white quantizes to itself (the bake fails
  otherwise). The `dwell` palette gained the `heartwood` ramp for the ladder and the log ends.
- `recipes`: material → `{ like?, layers }`. A material without one takes the recipe of the material
  it is `like` (`dirt_skirt` → `grass_side`), else `defaults.<use>`; so a material added to
  `materials.json` has a cel tile at once, and the characters (`defaults.character`: base, a ragged
  shadow band at the bottom and a lit rim at the top) need no recipe each.
- Tones are palette references as in §2 (`@stop` is the material's own ramp) or `"clear"`, a
  cutout's gap (transparent, its colour the material's base so filtering adds no fringe).

**The layer kinds** (`shared/textures/cel.mjs`, the fixed vocabulary; sizes in whole texels, y up,
every shape hard-edged and wrapped on a wrapping tile):

| kind | fields |
| --- | --- |
| `fill` | `tone` |
| `edge` | `tone`, `edge` (top, bottom, left, right, all), `width`, `ragged` (extra texels per column) |
| `clumps` | `tone`, `count`, `width`, `height` ([min, max]), `band`, `shape` (rect, blob: corners cut), `light`/`shadow` (the top/bottom row) |
| `blades` | `tone`, `tip`, `count` or `at` (columns), `length`, `width`, `root` (share of the height), `dir` (up, down) |
| `bricks` | `mortar`, `width`, `height` (rows and bricks that sum to the tile, so it wraps), `light`, `shadow` |
| `planks` | `seam`, `direction`, `width`, `light`, `joints` |
| `bands` | `tones` (cycled), `direction`, `width` (grain) |
| `ripples` | `tone`, `under` (a dash below, offset), `count`, `length`, `band` |
| `rings` | `tones`, `center`, `step`, `metric` (round, square), `radius` |
| `rects` | `tone`, `at` ([[x, y, w, h]]) |
| `checker` | `tone`, `size` |
| any | `clip` (paint only covered texels), `seed` |

**The bake.** `node shared/textures/gen.mjs` validates `cel.json` with the materials and paints
every atlas tile into `crates/dwell-mesh/src/cel.rgba` (16² × 4 bytes a tile, in `materials.tiles`
order) and `cel.gen.rs` (`CEL_TILE`, `CEL_TILES`, `CEL_MODE_MIPS`, `CEL_TEXELS`); CI regenerates and
fails on any diff, and runs `node --test shared/textures/cel.test.mjs`. `--cel-png <file>` writes a
sheet of every material in the style.

**In the client.** `dwell-mesh` `TextureSet` (`Painterly`, `Cel`), `set_texel` and `build_atlas_of`
build the atlas from either set in the same layout (the same rectangles, gutters and mip count), so
the renderer swaps the texels of its one atlas texture when the preset's set changes
(`ensure_atlas_set`; `write_atlas`), and no mesh or bind group changes. Everything else reads the
painterly set: the cutout classification (the cel tiles keep it, a test pins it), the glow and hang
(the materials'), the LOD's flat colours and the hotbar's swatches (the cel tiles average within
16/255 of the painterly ones, a test pins it, so the LOD needs no second table). The cel texels cost
19 KB of the WASM module (442 bytes gzipped: flat tones compress to almost nothing), so they ship in
it rather than loading on demand.

**Tests.** `cel.test.mjs`: the bake current; tiles at most `maxTones` colours (and lowering it fails
the bake); every non-exempt tone a `dwell` palette colour (an off-palette tone fails the bake); the
averages near the painterly ones; cutouts hard-edged and opaque tiles opaque; a material added by
data alone painted by the default and by its own recipe; every kind paints, deterministically, and
wraps (or stops at a sprite's edge); every bad entry fails naming it. `dwell-mesh`: the same tiles at
`CEL_TILE`, the same layout and cutouts as painterly, the ladder and tall grass where the meshes
expect them. The renderer: the coverage and gutter tests over both sets, the cel mips keep each
opaque tile's tones and end on its main tone, a cel preset swaps the atlas and `off` swaps it back.
`dwell-look`: `270-cel` and `360-cel` are `270` and `360` with the cel set, and nothing else.

**Measured** (PIXEL_ART_LOOK.md §8): at the same preset, 10–32 % fewer pixels change frame to frame
than with the painterly tiles while turning and walking. Sparkle (a pixel that flips and flips back)
is about the same, since most of it comes from block edges and the dither rather than the textures. A
denser first pass with averaged mips was worse than painterly, which is why the recipes use few,
large shapes and the mips go by majority tone.

**Not done (follow-ups).** Characters: their materials have cel tiles (the `character` default), but
the Character Lab still paints them painterly (the characters are not in the game yet). The
Painterly Texture Lab edits painterly materials only; a cel tab would be the editor for this file.

