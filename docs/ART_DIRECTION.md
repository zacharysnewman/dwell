# Dwell — Art Direction

Dwell's first art direction: a bright, hand-painted storybook look shared by the world (blocks, plants,
water, sky accents) and the characters. This document is the style. The texture system that
paints it is [`PAINTERLY_TEXTURES.md`](./PAINTERLY_TEXTURES.md) ([ADR 0040](./adr/0040-painterly-textures.md)),
and the characters are their own design (ADR 0041, the character design branch). Both read colour and
materials from the same data files:

- `shared/textures/palette.json`: the palette, as named ramps and swatches (§2).
- `shared/textures/materials.json`: the materials, each a stack of painted layers (§3, §4).

**Status.** Owner's brief of 2026-10-20 (the scratchpad brief, summarised in §1). The painterly set is
behind `?textures=painterly` until the owner approves it; the classic look stays the default. The
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
  must keep one clear hue on average. The generator's tests check the painterly averages stay near
  the classic ones, so the far view keeps its balance.
- **Night.** At night the moon and hemisphere light are dim and blue. The cool shadows of the ramps
  keep materials readable. Nothing in the palette relies on emissive light yet.
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
