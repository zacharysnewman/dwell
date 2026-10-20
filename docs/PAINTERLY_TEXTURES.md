# Dwell — Painterly Textures

One data-driven generator paints every texture in the storybook style of
[`ART_DIRECTION.md`](./ART_DIRECTION.md): block tiles for the atlas, and the materials the characters
and sky accents read by name. Decision record: [ADR 0040](./adr/0040-painterly-textures.md).
Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.1 (block textures).

**Status: built, behind a switch.** `?textures=painterly` shows the painterly set. The default stays
the classic set until the owner approves (§6).

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
| Per build, world or player? | Per build: the textures are baked into the build. The set (classic or painterly) is per page load, by the URL, until the default flips |

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
    - `alpha`: `opaque` (the default) or `strokes`. With `strokes`, only strokes cover the tile, which
      makes a cutout.
    - `like`: start from another material's fields (a template; e.g. `cloth_wool_blue` is
      `cloth_wool_red` with ramp `wool_blue`).
    - `over`: paint another material first (e.g. `grass_side` is painted over `soil`).
    - `layers`: the layers, painted in order (§3).
  - `tiles`: atlas tile name (the registry's `textures` names, `shared/blocks/dwell.json`) → the
    block material that paints it in the painterly set.
  - `keepClassic`: registry tiles that keep the classic painter in both sets (utility tiles:
    `grid`). The atlas's own `plain` and `missing` tiles are never painted.

Colours are palette references, never hex (ART_DIRECTION.md §2).

**Side tiles from the block above.** Owner, 2026-10-20: "I want to see the grass skirt fringe on
dirt that's below sloped grass." On generated hillsides the grass is mostly adaptive slopes, and at
a two-block step or a cliff the face below a slope's low edge is a dirt block's side. A block's
`sideUnder` in the registry (`shared/blocks/dwell.json`; BLOCK_REGISTRY.md) maps a block id above
to the side tile to use: dirt has `{ "dwell:grass": "dirtSkirt" }`. The chunk mesher applies it to
a side face whose cell above holds that block, whatever shape the block takes (slope, half slope,
ridge), unless that block shows a full side on the same face itself (a full grass cube already
hangs its own skirt, so the dirt below it stays plain). Stone or air above: no skirt. On face B,
"above" is toward its sky. The tile `dirtSkirt` is painted by `dirt_skirt`, which is
`"like": "grass_side"`: the same soil and the same skirt strokes, so it lines up with a grass side
beside it and wraps into the next dirt face; it is its own tile so its art can diverge by data. The
classic set paints it with the grass side's painter. The LOD keeps dirt's flat side colour: a skirt
band averaged over a coarse LOD cell would tint whole cliffs green.

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
is that same distance field, so the shader's alpha test traces the true outline up close, like the
classic grass blades. Positions come from a jittered grid of `count` cells inside `band` (shares of
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

`node shared/textures/gen.mjs` validates the palette and materials against the registry's tile
names, then paints each `tiles` entry. It writes:

- `crates/dwell-mesh/src/painterly.rgba`: the tiles' RGBA8 texels, in `tiles` order (4 KiB a tile).
- `crates/dwell-mesh/src/painterly.gen.rs`: `PAINTERLY_TILES` (their names) and `PAINTERLY_TEXELS`
  (`include_bytes!`).

Both are checked in. CI regenerates them and fails on any diff (the protocol job), and runs
`node --test shared/textures/gen.test.mjs`. `--png <file>` also writes a preview sheet of every
material.

**In the client** (`dwell-mesh` `textures.rs`): `TextureSet` is `Classic` or `Painterly`. One texel
lookup serves the atlas (`build_atlas`), cutout detection (`tile_has_alpha`), the tiles' average
colours (`average_tile_color`, the LOD's flat colours) and the hotbar swatches. A painterly tile
replaces its classic painter, and any other tile keeps it. The atlas, its mipmaps and their
coverage-preserving alpha (`render/mod.rs`) are built once at start-up, as before.

**Choosing the set.** `Client.create` reads `?textures=` from the page's query and sets it before
the atlas is built. Each meshing worker gets the page's set in its init message (`textures`, in
`web/jobs.rs` and `jobWorker.ts`) and sets it before its first job, so the LOD colours the workers
bake match the page's atlas. The averages and LOD colour tables are cached per set.

## 5. Validation and tests

A bad entry fails with its path named, for example `materials.json: materials.stone.layers[1].width
must be [min, max] in [0.25, 32]`. The checks cover:

- Unknown keys, kinds, ramps, stops and colours.
- Out-of-range numbers, and a layer with both or neither of `count` and `at`.
- A cycle in `like` or `over`.
- A tile mapped to a missing or non-block material.
- In the block data, a `sideUnder` naming an unknown block, a value that is not a tile name, an empty
  rule set, or a rule on a block without textures (`shared/blocks/faces.test.mjs`).
- A registry tile neither painted nor kept classic, and a painted tile no block uses.

`shared/textures/gen.test.mjs`:

- The checked-in bake is current.
- Painting is deterministic.
- **A material added by data alone** is painted, mapped to a new tile and read by name.
- Templates recolour and keep the strokes.
- Tiles are seamless.
- Cutouts cover only strokes, and sprites keep clear of the tile's edges.
- Every bad case above fails with its message.
- The character materials exist with ramps.

`dwell-mesh` (Rust):

- The query parses, and the default is classic.
- Every baked tile is an atlas tile of the atlas's size, and every registry tile but `grid` is
  painted.
- The painterly atlas differs from the classic only in painted tiles.
- Opaque blocks stay opaque.
- The ladder's rails, rungs and gaps, the tall grass's clear top row and sides, and the leaves'
  coverage are as the meshes assume.
- Painterly averages stay near the classic hues (the far view).
- A dirt side under a sloped grass block takes `dirtSkirt`, on face A and face B; under air, stone or
  a full grass cube it stays `dirt`. A fixture block's `sideUnder` (`test:loam`, by data alone) is
  applied (`mesher_tests.rs`).

## 6. The switch, and flipping it

- `?textures=painterly` picks the painterly set; `?textures=classic` or nothing picks the classic.
- To make painterly the default once the owner approves, move `#[default]` to
  `TextureSet::Painterly` in `crates/dwell-mesh/src/textures.rs` (one line). The test
  `the_query_picks_the_set_and_the_default_is_classic` pins the current default, so it changes with
  it.
- Textures are not part of the public API. They change no world format, protocol or terrain, so
  either set works on any server of the line.

## 7. Performance

The tiles are baked at build time and uploaded once as the same atlas (same size, same mip chain).
The shaders, meshes and draw calls are unchanged, so the frame costs the same in either set. Start-up
copies 12 baked tiles instead of running the noise painters, which is slightly less work.

Measured 2026-10-20 in Chrome on an Apple GPU (Metal), with `npm run bench -- --compare --configs
painterly` (the `painterly` configuration in `client/bench/configs.json`). Each scenario ran in a
fresh page with the same seed and inputs. All numbers are p95, in ms:

| view | config | frame interval | `frame.cpu` | `gpu.frame` |
| --- | --- | --- | --- | --- |
| Spawn, looking at the ground | classic | 18.6 | 4.9 | 8.80 |
| | painterly | 18.6 | 2.1 | 8.49 |
| 600 m up | classic | 18.6 | 2.2 | 9.32 |
| | painterly | 18.7 | 1.5 | 9.34 |
| Over the sea | classic | 18.6 | 2.9 | 11.72 |
| | painterly | 18.7 | 2.6 | 11.79 |

Both sets run at 60 fps with no dropped frames. The GPU times differ by less than run-to-run noise.
The painterly CPU times are lower only because the painterly run comes second in each pair, with the
browser warm.

## 8. The editor

`docs/prototypes/painterly-texture-lab.html` (published, see `docs/prototypes/README.md`) runs
`paint.mjs`'s code. You pick a material, edit its layers and the palette as JSON or with sliders,
see the tile, a 3 × 3 repeat and a block lit from above, and copy the material or palette JSON back
into `shared/textures/`.

## 9. Outstanding and later

- Water's surface is drawn by the water shader from its fluid look (`fluids` in
  `shared/blocks/dwell.json`, ADR 0030), not from the atlas. The painterly `water` tile only changes
  where the tile itself is used (the hotbar, a tile's average), so the sea looks the same in both
  sets. Taking the fluid's colours from the palette would be a change to the fluid data, and is
  left for later.
- The sky does not draw `cloud_puff` yet. Clouds are shaded analytically
  ([`CLOUDS.md`](./CLOUDS.md)).
- Characters are not in the game yet. Their materials are read by the character prototype (ADR
  0041).
- A 64-texel atlas would allow finer brushes up close. That is a change to the atlas's `TILE`, not to
  the data's vocabulary.
