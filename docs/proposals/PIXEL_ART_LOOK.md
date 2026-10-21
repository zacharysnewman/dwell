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
  by face) — a texture-set question, not a renderer one. A cel set was built to answer it (§8).
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
- [x] **Textures:** would a pixel-art look keep the painterly texture set, or get a flat-colour set
  of its own (the spec's "vertex colour and procedural noise")? The owner asked for a cel set; a
  prototype is built (§8).

## 8. A cel texture set (2026-10-21)

Owner: "I think we need to try out a custom texture set for the pixel art look. It looks like it's
better to use fewer colors per texture, with more of a cell shaded look to the textures", and "the
complexity in the textures currently makes them shimmer a lot when moving around." Built as a second
texture **style**, cel ([`PAINTERLY_TEXTURES.md`](../PAINTERLY_TEXTURES.md) §10): the same materials
painted from `shared/textures/cel.json` in at most 4 flat tones a tile, all on the `dwell` palette
(tested), at 16 texels a tile (one per metre, so a texel stays at least a low-res pixel out to about
11 m at 270 rows), hard-edged shapes from a small vocabulary (clumps, blades, bricks, planks, grain
bands, ripples, rings), its mips by **majority tone** (so a distant face keeps exactly its tones and
settles on its main one, rather than averaging into in-between colours that the palette dithers). The
presets `270-cel` and `360-cel` are `270` and `360` with it; nothing else changes.

**Try it:** `https://dropkickarcade.com/dwell/?pixelart=270-cel` and `?pixelart=360-cel` (and
`?pixelart=270` for the painterly set in the same look), or `__dwell.setPixelArt('270-cel')` live.

**Screenshots** (the owner's Mac, Chrome on the GPU, 1920 × 1080, seed 7; load average 7–14 during
the run). Sheets of the current look, `270` (painterly), `270-cel` and `360-cel`, and GIFs of each
motion burst, in the session's scratchpad (not checked in):
`/private/tmp/claude-501/-Users-zacknewman-Projects-dwell/dc7bd3b5-fc5f-4b35-a6f4-014a26f2aadb/scratchpad/pixelart/cel/deliverables/`
(`px-hills-noon.*`, `px-pines.grove`, `px-dusk.*`, `px-torch-cave.*`, `px-water.*`, `px-face-b.*`,
`px-shimmer.eye`/`.ground`, `shimmer.<burst>.<config>.gif`); tile close-ups (painterly, painterly
snapped to the palette, cel) in `…/pixelart/cel/tile-closeups.png`.

**Shimmer, measured.** The bench's `burst` with `drift` (the camera placed at eye height over the
spawn at noon and moved on each frame; 16 frames): turning 0.75° a frame, walking 5 cm a frame
toward the horizon and looking 35° down, turning while looking down, and a slow creep (1 cm,
0.05°). *Changed* is the bench's flicker (the share of pixels whose brightness moves over 3/255
frame to frame); *sparkle* is the share that changes by over 24/255 and changes back the next frame
(within 8/255): aliasing and dither crawl, rather than an edge passing once.

| Burst | Current: changed / sparkle | `270`: changed / sparkle | `270-cel` | `360-cel` |
|---|---|---|---|---|
| Turn | 69.2 % / 1.77 % | 42.2 % / 4.96 % | 37.8 % / 4.51 % | 38.0 % / 4.20 % |
| Walk | 41.2 / 0.33 | 20.0 / 1.50 | 15.4 / 1.51 | 15.0 / 1.21 |
| Walk, looking down | 46.0 / 0.31 | 22.2 / 1.46 | 15.1 / 1.88 | 14.0 / 1.64 |
| Turn, looking down | 50.0 / 0.46 | 25.6 / 2.00 | 18.7 / 2.01 | 17.8 / 1.92 |
| Creep | 26.4 / 0.12 | 13.1 / 0.92 | 10.3 / 0.76 | 9.9 / 0.56 |

`270`'s numbers repeated to within 0.3 % between two runs. A first cel pass (more, smaller clumps and
averaged mips) was *worse* than painterly: sparkle 5.13 % turning and 2.38 % walking looking down,
with 15.6 % of pixels moving over 24/255 against painterly's 7.3 %. Fewer, larger shapes and the
majority-tone mips brought it to the table's numbers.

**What it shows**

- **Fewer pixels change: 10–32 % fewer than painterly at the same preset** (walking 15 % against 20 %,
  looking down 15 % against 22 %). The flat areas hold still. In painterly `270` the grass is a field
  of dither dots between two greens that swims when you move (`shimmer.walk-ground.pixelart-270.gif`);
  in cel it is one flat green with whole shapes on it (`px-shimmer.ground`).
- **Sparkle is about the same.** Lower turning (4.5 against 5.0 %) and creeping (0.76 against
  0.92 %), higher walking looking down (1.9 against 1.5 %). Most of what remains is not the
  textures: the block edges' stair steps, the screen-fixed dither on lit slopes, tall grass and
  leaves, and the ½ m risers of every hillside, which are a few pixels tall at 270 rows whatever
  they are painted with. The current look sparkles least of all (0.1–1.8 %), because nothing is
  quantized. **The texture set alone does not fix first-person shimmer.** It moves it from the
  surfaces to the edges.
- **The changes that are left are bigger.** A cel shape is a whole tone darker than its ground, so a
  moving edge changes its pixels by more (mean Δ 6.6 against 5.9 looking down). It reads as shapes
  sliding, not as noise, but it is visible.
- **It reads as pixel art.** It looks more designed than quantized painterly: the stone's blocks,
  the bark's grain bands, the log's rings, the turf skirt's drips and the leaves' lit clusters read
  at 270 rows (`tile-closeups.png`, `px-torch-cave.placed-torch-room`), where painterly quantized
  becomes soft speckle.
- **Tile repetition shows.** With so few shapes a tile, the 1 m repeat is plain on flat ground (a
  grid of the same three dark clumps, `px-shimmer.ground`), and the stone's mortar reads as a tiled
  floor in the cave. Variants per block (a hash of the position picking one of two or three cel
  tiles) would break it. Two or three more pattern kinds, and no change to the vocabulary's shape,
  would cover it.
- **Hillsides turned from the sun go darker** than in painterly (`px-pines.grove`): the cel tiles'
  shadow shapes and the toon bands stack. The palette, not the textures, decides night and dusk as
  before (§4).
- **360-cel is the calmest** on every burst (sparkle 0.56–4.2 %). The finer grid makes each moving
  stair step smaller.

**Cost.** The cel texels add 19 KB to the WASM module (6.763 → 6.782 MB) and **442 bytes gzipped**
(flat tones compress to almost nothing), so they ship in the module rather than loading on demand.
Switching sets rewrites the atlas's mip chain once (in place, the same texture). The frame cost is
the same: the same atlas size, the same samplers.

**Open questions for the owner**

- [ ] Keep the cel set as the pixel-art look's textures, with per-block variants to break the
  repeat? Or go further, to flat ramp colours by face with no texture, which would sparkle least?
- [ ] Shimmer: the textures are only part of it. The rest needs the edges and dither to hold
  still (world-space dither, a coarser LOD under the look, or the slower camera §4 describes).

## 7. If adopted

An ADR and a phase: the palettes as a proper palette design (per time of day), the preset as a
player setting (an append-only field of the settings record) rather than a query, the cel texture set (§8) or its successor, the quantize by a 3D lookup table, the edge as a palette step, and
dither in world space if the screen-fixed pattern is judged a problem. Until then this stays a
preview behind `?pixelart=`, like `?outline=`.
