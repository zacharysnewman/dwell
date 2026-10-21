# Proposal: the 3D pixel-art look (a preview)

**Status:** prototype built as an opt-in preview, **off by default**, not adopted. Nothing here is
Dwell's art direction ([`ART_DIRECTION.md`](../ART_DIRECTION.md) stays the storybook look). The
owner asked (2026-10-21): "Can you prototype these rendering techniques to see what it would look
like in our game", with the *3D Pixel Art Style Spec* (the t3ssel8r-style look: a low-res render,
toon bands, a limited palette with ordered dither, a nearest-neighbour integer upscale). This
proposal says what was built to judge it, what it shows, and the spec's open questions.

**Try it:** `https://dropkickarcade.com/dwell/?pixelart=270` (presets `180`, `270`, `270-warm`,
`360`, `270-raw`; `off` or no parameter is the normal look). In the console,
`__dwell.setPixelArt('180')` switches live. The bench has a configuration per preset
(`npm run bench -- <scenario> --compare --configs pixelart-180,pixelart-270,pixelart-270-warm,pixelart-360`).

## 1. What was built

A post-process prototype over the existing renderer, in the extendable-system shape the repo asks
for: the palettes and presets are data, one generator, validation and tests over the data.

- **Data:** [`shared/look/pixelart.json`](../../shared/look/pixelart.json) — named palettes (ramps of
  2–8 colours, dark to light; a colour is a palette reference into
  `shared/textures/palette.json` or a hex) and presets. `shared/look/pixelart.mjs` validates it (a bad
  entry fails with its path) and generates `crates/dwell-look/src/pixelart.gen.rs`; CI fails on stale
  output. A test adds a palette and a preset by data alone.
- **Maths** (`crates/dwell-look/src/pixel_art.rs`, unit-tested): the grid on the canvas (integer
  scale, crop or letterbox, a one-pixel margin), the camera snap, OKLab, the 4 × 4 Bayer threshold,
  the quantize-and-dither rule (a CPU twin of the shader), the toon `band`, and the uniform blocks.
- **Pipeline** (`crates/dwell-client/src/render/`):
  1. The frame draws at the low-res size: the scene targets, the composite and the lines all run at
     it, the composite into a low-res picture instead of the canvas. The projection is widened by the
     margin so the rows that show keep the FOV setting.
  2. **Toon bands** in `lambert_lit` (every lit surface: terrain, LOD, water, characters): the sun's
     and moon's share, N·L × the shadow factor, on `bands` levels; optionally the ambient on
     `ambientBands` levels. **Hue-shifted shadows:** the unlit share takes `shadowTint`'s hue
     (normalised to luminance 1, so it shifts hue, not value) by `shadowHue`.
  3. **Edge:** the existing ink-outline code, with one low-res pixel of tap, black ink at
     `edgeDarken` (so the fill darkens and keeps its hue, and the palette snaps it to a darker step),
     inked only on the near side of a silhouette.
  4. **Post pass** (`dwell-look/src/pixel_art.wgsl`): every canvas pixel loads one low-res texel
     (nearest-neighbour, integer scale), finds the nearest two palette colours in OKLab and picks the
     second where the texel lies past its Bayer threshold toward it. `dither` 0 is the plain nearest
     colour; 1 a full two-colour ordered dither; 0.5 dithers only the middle half between two
     palette colours, so a flat area near a palette colour stays solid and the checker appears where
     a gradient or a band edge crosses between two colours. This is the "dither only gradients and
     band edges" rule without a gradient detector.
  5. **Atlas:** point filtering while on, `point` (no mips, the spec's rule) or `point-mips` (nearest
     texel of the nearest mip).
  6. Motion blur and the wind pass (its edge blur and streaks) are off.
  7. **Camera snap** (optional): the eye moves along the view's right and up onto the grid of
     low-res pixels on a plane `snapDepthM` ahead, and the upscale shifts the picture by the
     remainder (in whole canvas pixels, within the margin).
- The DOM HUD is unaffected (it is DOM over the canvas). A CPU adapter skips the preview.
- **Both faces:** the post pass is face-free; the bands sit in `lambert_lit`, which is already
  face-aware (each face's sun, moon and hemisphere).

## 2. The presets

| Preset | Rows | Fit | Palette | Dither | Sun bands | Ambient bands | Shadow hue | Edge | Atlas | Snap |
|---|---|---|---|---|---|---|---|---|---|---|
| `180` | 180 | crop | dwell (40) | 0.5 | 3 | 3 | 0.5 | depth + crease, 45% | point-mips | 8 m |
| `270` | 270 | crop | dwell | 0.5 | 3 | smooth | 0.5 | depth, 40% | point-mips | 8 m |
| `270-warm` | 270 | crop | hearth (44) | 0.5 | 3 | smooth | 0.6 | depth, 40% | point-mips | 8 m |
| `360` | 360 | crop | dwell | 0.35 | 4 | smooth | 0.35 | none | point-mips | 8 m |
| `270-raw` | 270 | letterbox | hearth | 1.0 | smooth | smooth | 0 | none | point | off |

`dwell` is 40 colours from the storybook palette's ramps and swatches (meadow, leaf, soil, bark,
stone, sand, water, cloud, flame, the inks) plus a hand-picked sky ramp and a near-black, since the
palette has no sky blues and no darks for night. `hearth` is a hand-made Lospec-style warm palette
(44 colours in the spec's ramps: sky 5, cloud 4, foliage 6, grass 5, bark 4, earth 4, stone 4, water
3, warm light 4, dusk 2, shade 3), hue-shifted warm in the light and cool in the shadow.

`270-raw` is the spec's pipeline with none of the extras (no bands, no edge, full dither, the
letterbox): what the post pass alone does to the existing look.

## 3. Screenshots

RESULTS

## 4. Observations

OBSERVATIONS

## 5. Cost

COST

## 6. Open questions for the owner

From the spec, with what the preview suggests:

QUESTIONS

## 7. If adopted

ADOPT
