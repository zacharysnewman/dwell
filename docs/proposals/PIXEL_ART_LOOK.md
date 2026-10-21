# Proposal: the 3D pixel-art look (a preview)

**Status:** prototype built as an opt-in preview, **off by default**, not adopted. Nothing here is
Dwell's art direction ([`ART_DIRECTION.md`](../ART_DIRECTION.md) stays the storybook look). The
owner asked (2026-10-21): "Can you prototype these rendering techniques to see what it would look
like in our game", with the *3D Pixel Art Style Spec* (the t3ssel8r-style look: a low-res render,
toon bands, a limited palette with ordered dither, a nearest-neighbour integer upscale). This
proposal says what was built to judge it, what it shows, and the spec's open questions.

**Try it:** `https://dropkickarcade.com/dwell/?pixelart=270` (presets `180`, `270`, `270-warm`,
`270-snap`, `270-cel`, `360`, `360-cel`, `270-raw`; `off` or no parameter is the normal look). In the console,
`__dwell.setPixelArt('180')` switches live. The bench has a configuration per preset
(`npm run bench -- <scenario> --compare --configs pixelart-180,pixelart-270,pixelart-270-warm,pixelart-270-snap,pixelart-360`).

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
- **Pipeline** (`crates/dwell-client/src/render/`, `pixel_art.rs` and the renderer):
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
  4. **Post passes** (`dwell-look/src/pixel_art.wgsl`): at low res, each texel finds the nearest two
     palette colours in OKLab and picks the second where it lies past its Bayer threshold toward it
     (`quantize_fs`); then each canvas pixel loads one texel of that (`post_fs`: nearest-neighbour,
     integer scale, bars outside the picture). `dither` 0 is the plain nearest
     colour; 1 a full two-colour ordered dither; 0.5 dithers only the middle half between two
     palette colours, so a flat area near a palette colour stays solid and the checker appears where
     a gradient or a band edge crosses between two colours. This is the "dither only gradients and
     band edges" rule without a gradient detector.
  5. **Atlas:** point filtering while on, `point` (no mips, the spec's rule) or `point-mips` (nearest
     texel of the nearest mip); and the preset's texture set (`textures`): the painterly set, or the
     **cel** style (§8; [`PAINTERLY_TEXTURES.md`](../PAINTERLY_TEXTURES.md) §10), written into the
     atlas in place when the preset changes.
  6. Motion blur and the wind pass (its edge blur and streaks) are off.
  7. **Camera snap** (optional): the eye moves along the view's right and up onto the grid of
     low-res pixels on a plane `snapDepthM` ahead, and the upscale shifts the picture by the
     remainder (in whole canvas pixels, within the margin). Off in every preset but `270-snap`
     (§4).
- The DOM HUD is unaffected (it is DOM over the canvas). A CPU adapter skips the preview.
- **Both faces:** the post pass is face-free; the bands sit in `lambert_lit`, which is already
  face-aware (each face's sun, moon and hemisphere).

## 2. The presets

| Preset | Rows | Fit | Palette | Dither | Sun bands | Ambient bands | Shadow hue | Edge | Atlas | Textures | Snap |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `180` | 180 | crop | dwell (45) | 0.5 | 3 | 3 | 0.5 | depth + crease, 45% | point-mips | painterly | off |
| `270` | 270 | crop | dwell | 0.5 | 3 | smooth | 0.5 | depth, 40% | point-mips | painterly | off |
| `270-warm` | 270 | crop | hearth (44) | 0.5 | 3 | smooth | 0.6 | depth, 40% | point-mips | painterly | off |
| `270-snap` | 270 | crop | dwell | 0.5 | 3 | smooth | 0.5 | depth, 40% | point-mips | painterly | 8 m |
| `270-cel` | 270 | crop | dwell | 0.5 | 3 | smooth | 0.5 | depth, 40% | point-mips | **cel** | off |
| `360` | 360 | crop | dwell | 0.35 | 4 | smooth | 0.35 | none | point-mips | painterly | off |
| `360-cel` | 360 | crop | dwell | 0.35 | 4 | smooth | 0.35 | none | point-mips | **cel** | off |
| `270-raw` | 270 | letterbox | hearth | 1.0 | smooth | smooth | 0 | none | point | painterly | off |

`270-cel` and `360-cel` are `270` and `360` with the cel textures and nothing else (a test pins it),
so a comparison isolates the texture set.

`dwell` is 45 colours from the storybook palette's ramps and swatches (meadow, leaf, soil, bark,
heartwood — added with the cel style, for its wood tones — stone, sand, water, cloud, flame, the inks) plus a hand-picked sky ramp (7 steps) and a near-black, since
the palette has no sky blues and no darks for night. `hearth` is a hand-made Lospec-style warm palette
(44 colours in the spec's ramps: sky 5, cloud 4, foliage 6, grass 5, bark 4, earth 4, stone 4, water
3, warm light 4, dusk 2, shade 3), hue-shifted warm in the light and cool in the shadow.

`270-raw` is the spec's pipeline with none of the extras (no bands, no edge, full dither, the
letterbox): what the post pass alone does to the existing look.

## 3. Screenshots

Taken 2026-10-21 on the owner's Mac (Chrome, real GPU, 1920 × 1080 at 1×, seed 7) with the bench
(`npm run bench -- <scenario> --compare --configs …`, the scenarios in the session's scratchpad).
Each sheet is the current look and `180`, `270`, `270-warm`, `360` side by side; the HUD is in
every shot, at full resolution. They are not checked in (as other look reviews, they live with the
session): `/private/tmp/claude-501/-Users-zacknewman-Projects-dwell/dc7bd3b5-fc5f-4b35-a6f4-014a26f2aadb/scratchpad/pixelart/deliverables/`.

| Scene | Sheet |
|---|---|
| The spawn's hills at noon | `px-hills-noon.hills.png`, `px-hills-noon.hills-west.png` |
| A pine grove (mid-morning, shadows on High) | `px-pines.grove.png` |
| Dusk over the hills from 520 m, and toward the low sun | `px-dusk.dusk-high.png`, `px-dusk.dusk-sun.png` |
| A dark cave: no torch, the held torch, placed | `px-torch-cave.dark-no-torch.png`, `px-torch-cave.held-torch.png`, `px-torch-cave.held-torch-wall.png`, `px-torch-cave.placed-torch-room.png` |
| Water at the seed-7 coast | `px-water.low-across.png`, `px-water.above.png` |
| First person with the held torch | `px-torch-cave.held-torch.png` (the arms pass is quantized like the rest) |
| Face B at its noon | `px-face-b.b-ground.png`, `px-face-b.b-west.png` |
| Turning (1.5° a frame) and walking | `turn-contact-sheet.png`, `turn-zoom.png`, `turn.<config>.gif`, `walk.<config>.gif` |

## 4. Observations

**What works**

- **It reads as pixel art at once**, most of all at 270 rows on a 1080p screen (×4). Blocky voxel
  terrain suits it: block faces are already flat, so after the bands they become one or two solid
  palette colours with a dithered seam, which is what hand-made pixel art does.
- **The sky and clouds** are the best part: the smooth gradient breaks into two or three palette
  steps with Bayer checker between them, and the clouds into solid puffs (`px-hills-noon.hills`,
  `px-dusk.dusk-high`). The "dither only between colours" rule keeps flat areas solid without a
  gradient detector.
- **The torch-lit cave** works well: warm floor in solid patches, the walls falling to the dark
  ramp in bands, the flame a few saturated pixels (`px-torch-cave.placed-torch-room`).
- **Water caustics** become a strong graphic pattern (`px-water.low-across`).
- **Face B** needed nothing: the bands use each face's own sun and sky (`px-face-b.*`).
- **It is cheaper**: the scene renders at a sixteenth of the pixels (below).

**What doesn't (yet)**

- **The palette decides everything, and neither palette is right.** `dwell` (the storybook
  palette's ramps) has no dusk or olive greens, so at dusk the whole landscape goes teal
  (`px-dusk.dusk-sun`), and no turquoise, so the sea goes sky-blue. `hearth` holds greens better and
  reads warmest, but its flame picks up a green pixel and its stone goes blue. A fixed palette also
  fights the day/night cycle: night and caves collapse onto the two or three darkest colours.
- **Toon bands on N·L are harsh on voxel hills.** With 3 levels, every slope turned from the sun
  drops a whole band, so half a hillside goes to the darkest green (`px-pines.grove`); the ambient
  bands of `180` make it worse. 4 bands (`360`) is gentler. The cascaded shadows survive but their
  soft PCF edge becomes a stepped band edge.
- **Painterly textures fight the palette.** The painterly tiles are low-contrast strokes of one ramp;
  quantized, each stroke becomes a speckle of two palette colours (grass tops, the held stone).
  The spec wants flat vertex colour with procedural noise; with these textures the look is busier
  than t3ssel8r's. A pixel-art direction would want its own texture set (or none: flat ramp colours
  by face) — a texture-set question, not a renderer one.
- **The 1-pixel edge** (near-side, darkened) is subtle at 270 and invisible past its fade; the
  white region-tool box and the block outline, drawn into the low-res picture, become 1-pixel white
  lines that dominate dark scenes.
- **LOD:** distant terrain is drawn by LOD sections; at 270 rows they are a few pixels each, so the
  LOD's coarser geometry and colour shifts don't show; the far field is mostly flat bands and
  dither. Nothing to fix there; if anything, the LOD could be coarser under this look.
- **Shimmer when moving is the real problem in first person.** Standing at the spawn and turning
  1.5° a frame (90°/s at 60 fps), 33.6 % of the screen's pixels change by more than 24/255 from one
  frame to the next at `270` against 28.7 % in the current look (`180` 33.9 %): not much more in
  area, but each change is a whole 4 × 4 block flipping between two palette colours, so the stair
  steps of every block edge crawl and the dither dots swim over the grass (`turn-zoom.png`,
  `turn.270.gif`). Walking forward is calmer than the current look in area (32 % against 51 %:
  the low resolution hides the grass's motion) but just as crawly in kind. The spec's own answer,
  a slow fixed-pitch camera, is not available to a first-person game.
- **The camera snap does not help in first person.** Snapping the eye to the low-res grid on a plane
  8 m ahead holds that plane still on a pure sideways move, but turning changes the grid every frame,
  forward motion is not snapped, and anything nearer or farther than the plane still crawls. In the
  measurements it made things slightly worse (turning 35.1 % against 33.6 %, walking 34.6 % against
  32.4 %): the eye is never perfectly still, and the snap turns sub-pixel jitter into whole canvas
  pixel jumps of the picture. It is off in every preset but `270-snap`, kept to show it. Orthographic
  or fixed-angle cameras (the spec's model) are where it pays.
- **The dither pattern is fixed to the screen**, so on a turn the checker stays put while the scene
  slides under it (the "shower door" effect). World- or object-space dither would need the pattern
  in the scene passes.

## 5. Cost

Mean GPU frame time (`gpu.frame`, the bench's timestamps; 1920 × 1080 at 1×, Shadows High, 4×
MSAA; load average 4–8 during the runs, so read them as indications):

| Scene | Current | `180` | `270` | `270-warm` | `360` |
|---|---|---|---|---|---|
| Hills at noon | 11.5 ms | 6.95 | 7.03 | 7.14 | 7.45 |
| Pine grove | 16.3 | 7.57 | 8.30 | 8.01 | 8.79 |
| Torch cave (held torch) | 10.2 | 7.50 | 6.92 | 7.43 | 7.09 |
| Open sea | 8.16 | 6.01 | 6.57 | 6.40 | 6.02 |

The preview roughly halves the frame where the scene passes dominate. The post pass
(`gpu.pixelart`: the quantize at low res; the upscale is one load per canvas pixel) reads 1–3 ms,
which is more than a 130 k-pixel loop over 42 colours should cost; Apple's per-pass timestamps are
coarse, and a 3D lookup table of the palette (one texture fetch instead of the loop) would make
it negligible if the look were adopted. A first version quantized per *canvas* pixel and cost 4 ms
at 1080p; quantizing at low res first fixed that.

## 6. Open questions for the owner

From the spec, with what the preview suggests:

- [ ] **Final internal resolution and display scale.** 270 rows (×4 at 1080p, ×5 crop at 1440p)
  reads best in first person; 180 is too coarse to aim and read the ground at, 360 starts to look
  like a low-resolution render rather than pixel art. Phones (DPR 3) would need a cap.
- [ ] **Palette: custom or from Lospec?** Neither palette here is a design: the look needs a
  palette made for it, with ramps for every material, dusk and night, and the sea. The storybook
  palette's 3-step ramps are too short (the spec's ramps are 4–6 steps).
- [ ] **Outlines on or off?** The near-side darkened edge is barely visible at 270; a stronger,
  palette-step edge (one ramp step darker, rather than a darkening) would be the next try.
- [ ] **Engine and render pipeline.** Answered by the project: it is a post pass over the existing
  wgpu renderer (this preview), no engine change.
- [ ] **Day/night: does the palette shift or swap?** With one palette the night collapses onto
  the dark colours. Options: a palette per time of day keyframe (as `daynight.json`'s), blended by
  swapping at a threshold; or quantizing before the exposure and time-of-day grade.
- [ ] **First-person shimmer:** is it acceptable for this to be an optional look (a setting) rather
  than the game's look? The crawl on turning is inherent to a first-person camera at low resolution.
- [ ] **Textures:** would a pixel-art look keep the painterly texture set, or get a flat-colour set
  of its own (the spec's "vertex colour and procedural noise")?

## 7. If adopted

An ADR and a phase: the palettes as a proper palette design (per time of day), the preset as a
player setting (an append-only field of the settings record) rather than a query, a texture set for
it if the owner wants one, the quantize by a 3D lookup table, the edge as a palette step, and
dither in world space if the screen-fixed pattern is judged a problem. Until then this stays a
preview behind `?pixelart=`, like `?outline=`.
