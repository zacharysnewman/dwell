# Dwell — Art Direction

Dwell's first art direction: a bright, hand-painted storybook look shared by the world (blocks, plants,
water, sky accents) and the characters. This document is the style. The texture system that
paints it is [`PAINTERLY_TEXTURES.md`](./PAINTERLY_TEXTURES.md) ([ADR 0040](./adr/0040-painterly-textures.md)),
and the characters are their own design (ADR 0041, the character design branch). Both read colour and
materials from the same data files:

- `shared/textures/palette.json`: the palette, as named ramps and swatches (§2).
- `shared/textures/materials.json`: the materials, each a stack of painted layers (§3, §4).

**Status.** Owner's brief of 2026-10-20 (the scratchpad brief, summarised in §1). The painterly set is
the game's one texture set: the owner approved it (2026-10-20) and removed the earlier procedural
set (2026-10-21). The
*content* (which blocks exist) is still prototype (ARCHITECTURE.md §6.1). What this document fixes is
the style that content will be painted in.

## 1. The brief

Owner, 2026-10-20: bright, colourful, hand-painted, storybook, after the painterly look of League of
Legends' Summoner's Rift, adapted to a first-person adventure game. Take only the style cues. No
borrowed content (no other game's block set). Procedural worlds look noisy, so **avoid visual
noise**: clean, broad strokes that read at a distance.

The owner's decisions that shape the style:

- First person only. A third-person camera is out of scope.
- Everything is generated: blocks, plants, water, sky accents and characters, by one procedural
  generator driven by data (brush strokes, painted highlights, clean gradients), baked at build time.
- Characters are made from data as chunky boxes and rounded shapes, painted in the same palette and
  brush vocabulary.

## 2. Palette

The palette is a set of **ramps**: three painted values of one material.

| stop | rule |
| --- | --- |
| `shadow` | darker **and cooler** (shifted toward blue-violet) |
| `base` | the local colour |
| `light` | lighter **and warmer** (shifted toward yellow) |

Hue-shifted ramps are what make flat colour read as paint rather than as a darkened texture. A
material never darkens toward black or lightens toward white; it moves along its ramp.

A colour is referenced by name, never by hex: `"meadow.light"` (a ramp stop), `"meadow"` (its base),
`"ink"` (a single swatch), or `"@shadow"` inside a material (a stop of the material's own ramp, so a
template can be recoloured by changing its ramp). The names and values live in
`shared/textures/palette.json`. The groups:

| group | ramps |
| --- | --- |
| Ground | `meadow` (grass), `soil`, `stone`, `sand` |
| Plants and wood | `leaf`, `bark`, `heartwood` |
| Water and sky | `water`, `cloud`; swatches `foam`, `sun`, `star` |
| Cloth | `canvas` (thick canvas), `linen` (a light tunic cloth), `wool_red`, `wool_blue`, `wool_green` (soft wool, velvet) |
| Gear | `leather`, `iron`, `brass` |
| Creatures | `wolf`, `wolf_pale`, `deer`, `deer_cream`, `antler` |
| People | `skin_fair`, `skin_tan`, `skin_brown`, `skin_deep`; `hair_black`, `hair_brown`, `hair_ginger`, `hair_blonde`; swatches `ink`, `ink_soft` (eyes, brows) |

Rules for choosing colours:

- **Saturation a little high, values in the middle.** Albedos stay inside about 0.06–0.92 sRGB. The
  renderer's ACES filmic curve desaturates and compresses brights and crushes darks, so a pure white
  or black loses its paint.
- **Low contrast inside a material, high contrast between materials.** A block's strokes differ by
  one ramp step. The difference between grass, soil and stone is what reads at a distance.
- **Characters stand apart from the ground.** Clothes use the wool reds and blues, brass, canvas and
  linen. No large areas of `meadow` or `leaf` green, so a player reads against grass.

## 3. Brush vocabulary

The generator has a small fixed set of marks. Every material is built from them in data.

| mark | shape | used for |
| --- | --- | --- |
| `fill` | one flat colour | every material's ground |
| `gradient` | two colours up, down, left, right or radial, optionally in flat `steps` | metal, grass tips, a sprite's foot-to-tip shading |
| `dab` | a soft oval | patches of light and shade, leaf clusters, cheeks |
| `streak` | a long lens, tapered at both ends, optionally bent | grain, ripples, sheen, metal light, cracks |
| `blade` | tapered from a root to a point, optionally bent | grass, turf fringes, fur, locks of hair |
| `slab` | a flat-ended bar | stone facets, weave, ladder rails and rungs |
| `ring` | a ring or arc around a centre | growth rings on cut wood |
| `edgeLight` | a painted band along an edge, with a ragged inner line | rim light on turf, leather and metal; bark rims |

How strokes are placed: by `count` on a jittered grid (even coverage, never clumps or bare
patches), or at exact `at` positions. Each stroke picks its colour from the layer's `colors`, its
width, length, angle and bend from ranges, and has a soft edge (`softness`, in texels). `clip`
paints only inside what earlier strokes covered (highlights on leaves, grain on rails).

**Anti-noise rules** (what keeps the world clean):

1. Few, broad strokes: a 32-texel tile carries about 10–30 strokes, most 4–16 texels long. No
   per-texel speckle and no noise octaves.
2. One or two value steps per material (ramp stops at partial opacity).
3. Strokes in a material share a direction (grain, wind, the lie of the fur), with jitter of
   10–40°, so the tile reads as one gesture.
4. Seamless tiles (strokes wrap) for everything that repeats. Sprites (plants on crossed quads) do
   not wrap.

## 4. How materials read

| material | how it reads | built from |
| --- | --- | --- |
| **Grass** | a lit lawn: broad warm and cool patches and a few tufts | `fill` meadow, big soft `dab`s of shadow and light, short `blade`s |
| **Turf edge** (grass side) | soil under a hanging fringe of turf with a lit rim | soil, then darker drip `blade`s, an `edgeLight` band, lighter drips, a light rim |
| **Soil** | soft clods | `dab`s of shadow and light, a few small `slab` pebbles |
| **Stone** | broad faceted planes and a crack or two, not gravel | big flat `slab`s in shadow and light, a few thin `streak`s |
| **Sand** | wind ripples | long low-contrast horizontal `streak`s |
| **Wood** (bark, cut ends, ladder) | vertical grain; rings on the cut end; clean planks | `streak`s along the grain; `ring` arcs; `slab` rails with a `clip`ped light streak |
| **Leaves** | big clusters with gaps, lit from above | covering `dab`s in shadow and base, `clip`ped light `dab`s leaning one way |
| **Grass plants** | separate pointed blades, dark at the foot, light at the tips | `blade`s at fixed roots, a `clip`ped `gradient` |
| **Water** | ripples, not noise | long horizontal `streak`s, a few light glints; the water shader adds the rest |
| **Cloth: canvas, linen** | thick woven cloth | crossing `slab`s at low opacity, a soft top light |
| **Cloth: wool, velvet** | soft, deep colour with a sheen | large soft `dab`s and one or two broad sheen `streak`s |
| **Leather** | supple, with a lit edge | horizontal `streak`s and an `edgeLight` |
| **Metal** | broad painted light streaks, never scratches | a `gradient` from shadow to base, two wide diagonal light `streak`s, a lit edge |
| **Fur** | the lie of the coat, not hairs | `blade`s pointing one way, in shadow and light |
| **Antler, bone** | smooth, slightly streaked along its length | long vertical `streak`s |
| **Skin** | nearly flat and warm | the base with one large soft light `dab` |
| **Hair** | a few locks with a highlight | long `blade`s and a light `streak` |
| **Clouds** (sky accent) | soft bright tops, cool undersides | large soft `dab`s |

Each material also has an `overlay` (0–1): how strongly its strokes show over the flat ramp when a
part is shaded with the ramp alone. Characters use it (§5). Blocks always show their full tile.

## 5. Lighting assumptions

The renderer lights the textures. They are **albedo with painted form, not baked light**:

- **Shading.** Lambert shading from the sun or moon and a hemisphere light per face of the disc,
  following the day/night cycle (`shared/look/daynight.json`, [`DAY_NIGHT.md`](./DAY_NIGHT.md)).
  Textures bake no sun direction. The painted light is only a gentle top light or rim (`edgeLight`,
  at most about 15% in value) and the cool and warm sides of the ramps. Block faces also get the
  mesher's fixed face tints (`dwell-look` `face_tint`), which give the cube its form.
- **Tone mapping.** ACES filmic with exposure, then sRGB. Hence the mid-value, slightly saturated
  palette (§2).
- **Fog.** Height fog mixes toward the sky's colour in output space, by the time of day's fog
  colour. Distant ground is its tiles' **average colour** (the LOD's flat colour), so a material
  must keep one clear hue on average (the tests check the tall grass averages near the grass top,
  so a meadow reads as one green from afar).
- **Night.** At night the moon and hemisphere light are dim and blue. The cool shadows of the ramps
  keep materials readable. Nothing in the palette relies on emissive light yet.
- **Fire.** A flame is self-lit (it shows its own colour whatever the light) and painted, not
  photographic: a soft teardrop rising from the stick's tip, its colours laid in a few bands from
  deep red-orange tips through orange to a warm yellow core (no white), the top licking into
  separate tongues, a faint warm halo and a few sparks. Its look is data (`shared/look/flames.json`,
  [`LIGHTS.md`](./LIGHTS.md) §6), drawn by the renderer, not a tile.
- **Characters.** Characters are shaded from their material's ramp (shadow, base and light by the
  light's angle or the part's height) with the stroke tile blended in at its `overlay`. They are
  clean and readable against the terrain, with no extra noise. The first-person arms and the full
  body use the same materials, so the style is identical in both.

## 6. What is not decided here

- The real block set. Today's blocks are prototype content. New blocks get materials in this style
  by adding data.
- The sky's own look. The sky and clouds are shaded analytically (`shared/look/clouds.json`). The
  `cloud_puff` material is a sky accent for sprites and the editor, not yet drawn by the sky.
- Larger tiles. The atlas's tiles are 32 texels. A finer brush would need a 64-texel atlas
  (`TILE`), a change to the atlas, not to the style.
- Whether the world gets an ink outline (§7). It is a preview, off by default.

## 7. The ink outline (preview)

Owner, 2026-10-20: *"I see the Character Lab has edge lines, I like that. What would that look like
applied to the whole game?"* The Character Lab draws a thin dark outline in the palette's `ink`
(CHARACTERS.md). This section is the same line drawn over the whole frame, as a screen-space pass.
It is **off by default**. `?outline=silhouette`, `creases` or `blocks` turns it on for the page, and
`__dwell.setOutline(name)` switches it live. Players see no change until the owner picks a mode.

### 7.1 Modes are data

`shared/look/outline.json` holds the ink (a palette reference, `"ink"`) and a list of named
**modes**. `shared/look/outline.mjs` validates the file, resolves the ink from
`shared/textures/palette.json` and generates `crates/dwell-look/src/outline.gen.rs`. CI fails if the
generated file is stale. A new mode is one more entry. It might be thicker lines, a softer ink or a
nearer fade. Each mode has these fields:

| field | meaning |
| --- | --- |
| `edges` | which edges it draws: `depth` (silhouettes), `normal` (creases), `block` (block edges) |
| `widthPx` | the line's width in CSS pixels (× the pixel ratio on the screen) |
| `fadeStartM`, `fadeEndM` | full strength to the start, none from the end (metres from the eye) |
| `strength` | the ink's opacity |
| `depthThreshold`, `creaseThreshold` | where a silhouette and a crease begin (each reaches full strength at twice the value) |

| mode | edges | width | fade | strength |
| --- | --- | --- | --- | --- |
| `silhouette` | depth | 1.5 px | 32–128 m | 0.85 |
| `creases` | depth, normal | 1.5 px | 24–96 m | 0.8 |
| `blocks` | depth, normal, block | 1 px | 12–48 m | 0.6 |

### 7.2 The edge rule

The composite finds edges in the shared frame's **view depth** alone (the frame has no normal or id
targets). It reads one sample per tap over a
3 × 3 grid, with the taps half the line's width apart. Inverse depth is linear across the screen on
any plane, so a flat face at any angle has no edge.

- **Silhouettes:** the second difference of inverse depth across the pixel is large compared with
  the nearest tap.
- **Creases:** the slope of inverse depth changes, along the middle row or column or between the
  outer ones. The outer comparison is what finds a block's side against the ground it stands on,
  where the depth barely jumps. The surface's normal turns there, but the frame doesn't store it.
- **Block edges:** the voxel cell behind the surface changes from the pixel to a neighbour. The
  position and normal are reconstructed from the depth. These are the frame's stand-in for block
  ids.

`dwell-look`'s `outline.rs` is the CPU twin of the silhouette and crease rules and the fade. Its tests
pin that a flat face has no edge, that silhouettes and creases are found, and that **far pixels get
no outline**.

### 7.3 Keeping the distance clean

Voxel terrain is mostly edges, so the main risk is a horizon of noise. Four measures keep it out:

- The line fades out over the mode's distance. The spyglass multiplies the fade by its
  magnification, so a zoomed view keeps its lines.
- The height fog takes the ink toward the sky, as it takes the ground.
- Each tap reads one sample, never the mean of a pixel's samples. A mean would let a few samples
  of a blade's tip or a fading plant's dither pull the pixel forward and dot the grass with lines.
- Pixels past the fade return after one load.

### 7.4 With the rest of the frame

- **Water** writes no view depth, so the frame sees the seabed through it. The ink is dropped below
  the sea's surface when the eye is above it, so the shore gets a line and the seabed doesn't. Water
  above sea level (placed water) still shows its bed's edges. The ink isn't drawn while the eye is
  under water.
- **Clouds:** the ink goes under the cloud deck where the deck is in front of the edge, and it is
  gone inside a cloud.
- **Tall grass** takes no ink (owner, 2026-10-20: *"The grass on creases is mostly black, perhaps
  don't apply it to the grass."*). Block data **`outline`** (a cross option; `false` by default for
  the cross plants, `true` for everything else; `shared/blocks/gen.mjs` validates it and the fixture
  `test:reed` says `true` by data alone) reaches the frame in the cutout mesh's `bare` stream, and
  the cutout pass flags the pixel's view depth: its lowest bit (`frame_depth`, which every writer of
  the view depth calls, so the bit is never noise; about 1e-7 of the depth, below anything a reader
  resolves). No extra target or load: the ink already reads the depth. A flagged pixel gets no ink,
  and the edge rule treats flagged taps as see-through: such a tap takes what the plane through the
  pixel and the opposite tap gives (no edge across that pair), or the pixel's own where both are
  flagged, never nearer than the sky. So the blades, their edges against the ground and against the
  sky draw nothing, and the terraces and blocks among them keep their lines (dwell-look's
  `see_through` is the CPU twin, `plants_that_take_no_ink_are_seen_through` its test). A ridge
  hidden behind a blade loses its line there; the frame has no depth behind a plant.
- **The brush ghost and the block outline** are drawn after the composite, so they sit over the ink.
- **The wind** (motion effects) is drawn over the ink, as a later pass over the finished image, so
  the edge blur's taps carry the ink to the screen's edges (MOTION_EFFECTS.md: post effects that
  sample neighbours read the finished image; before this fix the lines vanished outside the blur's
  inner circle when sprinting).

### 7.5 Cost

`npm run bench -- --compare idle-spawn fly-high --configs raster,outline-silhouette,outline-creases,outline-blocks`.
Apple M1, 1280 × 720 at pixel ratio 1, one run each, on a machine shared with other agents.
GPU means in ms:

| scenario | | off | silhouette | creases | blocks |
| --- | --- | --- | --- | --- | --- |
| idle-spawn | `gpu.composite` | 5.38 | 5.95 | 5.88 | 5.70 |
| idle-spawn | `gpu.frame` | 13.4 | 13.1 | 13.6 | 13.6 |
| fly-high | `gpu.composite` | 10.5 | 10.7 | 9.05 | 9.11 |
| fly-high | `gpu.frame` | 14.4 | 14.5 | 12.5 | 12.6 |

The composite costs **+0.3 to +0.6 ms** at the spawn. From the air nearly every pixel is past the
fade, so the cost is within noise. In both scenarios `gpu.frame` stays within run-to-run noise of
off, which meets the 0.5 ms target. The first version read four samples per tap and cost about
+2 ms. Reading one sample per tap and returning early past the fade brought it down.

### 7.6 For the owner

The screenshots showed every view off and in each mode, with the texture sets of the time, plus the ray marcher
(since removed: ADR 0044).
The views are the ground at the spawn, a hillside, blocks close up, a steep slope, the shore, 600 m
up, the spyglass and the brush ghost. Questions:

1. Which mode, if any? `creases` gives the storybook contour lines on terraces. `blocks` adds a grid
   that reads more like a diagram.
2. ~~Should tall grass be outlined?~~ Answered (2026-10-20): no; block data `outline`, §7.4.
3. Line width and distance: a thicker line and a shorter fade, or the reverse?
4. If one stays: a setting in the menu, and a new compatibility line isn't needed (it is a look only).
