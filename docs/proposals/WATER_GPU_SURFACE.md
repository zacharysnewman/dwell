# Proposal: cheaper water that stays pretty (costs, options, a GPU water surface, waterfalls)

**Status:** written 2026-11-07, not adopted. Measured on the M1 Air; nothing in the renderer was changed
for it (the bench's `view` step gained switches for flow ripples, flow foam and waterfalls).

The owner (2026-11-07): "cheaper water in general, preferably without sacrificing it looking pretty. But
our water might be a bit overdone at max settings. We could break down what's most expensive." And the
goal: "pretty water that can run in mobile, but I'm good if it's a bit stylized. We can use cheap routes
if the results are decent and cohesive."

Numbers marked *estimate* are arithmetic, not measurements; measure before relying on them.

## 1. What the water costs today

### 1.1 GPU, feature by feature

**Method.** Chrome on the GPU, the owner's setting (a 1280 × 800 window at pixel ratio 2, so a
2560 × 1600 canvas, `?renderscale=0.5&upscaler=stretch`: 1280 × 800 drawn), `--unthrottled`, seed 0,
noon, water **High**, load 2–4.5, on AC. One page per view. Each piece is switched off in turn mid-run
(`water` step with `debug`, the `?waterdbg=` names; `view` for flecks, flow ripples, flow foam and
waterfalls), settled 1.5 s, recorded 3 s; `gpu.frame` mean. The default at the start and the end of a
run differ by up to 0.13 ms, so differences under ~0.15 ms are noise. The M1's passes overlap, so a
pass's own span (`gpu.near.water`) doesn't add up; a piece's cost is the frame's change with it off
(DEV_TOOLS.md "Cost profile").

Views: **the dam close-up** (6 m over the river by the dam, looking 40° down; water about a third of the
screen), **the valley** (25 m over the dam looking along the river, 14° down; water ~5 % of the screen),
**the waterline spot** ([5051.5, 31.9, 22980.5], the sea: the eye at the surface, and 1.5 m deep along
the floor), **a flood** (a trench cut across the river at the dam: water pours in and keeps moving
for the whole record).

| Switched off (High otherwise) | Dam close-up | Dam, full 2560 × 1600 drawn | Valley | Deep (1.5 m) | What it does |
|---|---|---|---|---|---|
| — (High) | **9.07** | **20.71** | **10.42** | **5.27** | |
| Water **Medium** | −0.11 | −0.78 | −0.04 | −0.04 | 12 SSR steps, 3 cloud octaves, fine caustic layer nearer |
| Water **Low** | **−1.73** | **−5.78** | **−1.33** | **−0.61** | no scene copy, no tiles, no refraction, sky only: the water turns opaque blue |
| `flat` (the water a constant colour; copy and split kept) | **−2.02** | **−6.62** | −0.67 | −0.48 | the whole per-pixel water shading |
| `all` (every piece below at once, the base lighting kept) | −1.03 | −3.48 | −0.42 | | |
| Land reflections (`ssr`) | **−0.45** | **−1.24** | −0.24 | | the screen-space march |
| Flow foam streaks (setting) | **−0.27** | **−0.73** | **−0.27** | | the white dashes along the current |
| Ripple layers 2 and 3 (`ripple1,ripple2`) | −0.13 | **−0.79** | −0.12 | | |
| Reflected clouds (`clouds`) | −0.09 | −0.49 | 0.0 | | |
| Caustics (`caustics`) | −0.04 | −0.17 | 0.0 | −0.06 (mean of 2) | the tile already; looking 40° down |
| Refraction, absorption, foam, sparkle, glint, sky, `under` | within ±0.2 (noise) | within ±0.3 (noise) | within ±0.15 | `under` −0.13 | |
| Flecks, flow ripples, waterfalls (settings) | within noise | flow ripples −0.27; the rest noise | within noise | | no tall fall was in view: see §4 |
| The water tiles pass (`gpu.water.tiles`, own span) | 0.41 | 0.41 | 0.42 | 0.39 | drawn every Medium/High frame with water in view |

The full-resolution column is the same dam view drawn at 2560 × 1600 (render scale 1, as in the cost
profile): four times the pixels, the water's pieces 3–6× (they are per pixel), so the small pieces show
above the noise.

What the table says:

- **The water is ~2 ms of a 9 ms frame where it fills a third of the screen** (High against `flat` or
  Low), and ~1.3 ms where it is a thin river in a valley. That 1.3 ms with only 5 % of the screen as
  water is the **fixed part**: the scene-colour copy and the tiles (`flat` keeps them and saves only
  0.67; Low drops them and saves 1.33), paid whenever any water is in view.
- **The one large piece is the land reflection** (0.25–0.45 ms; 1.24 at full resolution), then **the
  flow foam** (0.27 ms in both views, 0.73 at full: it samples along the flow on every water pixel),
  then the extra ripple layers (0.12; 0.79 at full) and the reflected clouds (0.49 at full).
  Everything else is noise at these views, including refraction, absorption, caustics (from the tile
  since 2026-11-06), shoreline foam, sparkle and glint.
- **High and Medium nearly cost the same** at the owner's 50 % (0.11 ms; 0.78 at full resolution):
  High's extra SSR steps and cloud octaves matter only along the water at grazing angles.
- **Low is cheap because it stops being water**: it loses the see-through (the floor, caustics,
  absorption). The see-through itself is cheap (refraction and absorption measure as nothing); what Low
  saves is the copy and tiles plus the reflections, flow foam and ripple layers it also drops.
- **Under the surface** the whole water is 0.5–0.6 ms (deep view, Low and `flat`).

Screenshots of each record (the same views with each piece off) are in the scratchpad run directory
listed in DEV_TOOLS.md "Water cost (M1 Air)".

### 1.2 The waterline's deep view (recheck)

The waterline PR (0.50.17) read 0.7–1.6 ms more at 1.5 m deep after the change than before, unexplained.
Rechecked twice in one run, the deep record first, then after a surface record, then deep again:

| Record | `gpu.frame` |
|---|---|
| Deep, first | 5.28 |
| Deep, after its ablations | 5.27 |
| Surface (eye at the surface) | 6.84 |
| Deep, second | 5.23 |
| Deep, after its ablations | 5.25 |
| Surface, second | 6.83 |

The deep view is stable at 5.25 ± 0.03 ms and **cheaper** than the surface view (the under-water view
draws less sky, clouds and far terrain). The earlier +0.7–1.6 ms was the run order on a warm laptop
(the deep record came last each time): **no deep-view cost to fix.** Within the deep view, `under`
(the composite's underwater fog and caustics) is 0.12 ms and caustics 0.06.

**Seen on the way down** (a shot series at the same spot, the eye from +1 m to −1 m against the
surface; owner's request, 2026-11-07), for WATER_RENDERING.md §12's follow-up, not costs: the line
reads straight only with the eye exactly at the surface. At ±0.1 m it is a strongly curved arc, because
each pixel tests the point `lensM` out along its ray, and those points lie on a sphere round the eye,
which the water's plane cuts in a circle. It jumps from absent (+0.25 m) to a third of the screen
(+0.1 m). From −0.25 m down, the underside of the surface is flat blue with a dark blotch: no sky
through Snell's window, and no rippled reflection.

**Fixed in 0.50.20** (WATER_RENDERING.md §12.1): the lens plane (a straight line that moves linearly
with the eye; a pop-free reach), total internal reflection outside Snell's window (which also put the
colour edge on the line, where the meniscus had drawn above it), the waterline band in place of the
meniscus (owner's follow-up), the underside's texture, and the blotch (the player's own shadow on the
water's body). Cost at the owner's setting: 0 m 6.01 → 6.10 ms `gpu.frame`; −1 m 5.93 → 6.43 ms (the
eye now within the band's reach there).

### 1.3 CPU, meshing and upload (water as blocks in eighths)

Clients draw the server's grid as water blocks: full cells, the top cell in eighths of a ½ m cell
(WATER_SURFACE_GRID.md §2c). Measured in the flood (a trench cut beside the river at the dam; water
pours in and keeps moving):

| | Still | Flood (12 s) | 3 s after |
|---|---|---|---|
| `frame.cpu` mean / p95 (ms) | 2.15 / 3.3 | 2.93 / 7.6 | 2.6 / 6.7 |
| `jobs.apply` + `mesh_dirty` + `upload.chunk` (main thread, mean) | 0 | 1.26 | 0.89 |
| `chunk_mesh` jobs (workers) | 0 | 923 at 12.6 ms (77 a second) | 142 (47 a second) |
| `gpu.frame` | 10.67 | 10.97 | 10.77 |

Every change of an eighth rewrites blocks, so **the whole terrain chunk is re-meshed** (ground, plants
and water) on a worker and re-uploaded: 77 chunk meshes a second while the flood runs, about one worker
busy full time, and 1.3 ms a frame on the main thread (applying and uploading), p95 7.6 ms. Moving water
is the one thing in the game today that keeps re-meshing terrain while nobody edits it.

The geometry (native, `dwell-mesh`, one 32 × 32 × 32 chunk with a lake 8 cells deep over stone):

| Lake surface | Water triangles (tops / sides) | Water mesh as built | `mesh_chunk` (whole chunk, native) |
|---|---|---|---|
| Calm (every column at one eighth) | 2 (2 / 0) | < 1 KB | 4.0 ms |
| A wave (a sine 8 m long over the columns' eighths) | 4,880 (1,552 / 3,328) | 743 KB | 3.0 ms |
| Choppy (each column a different eighth) | 6,144 (2,048 / 4,096) | 936 KB | 3.1 ms |
| *The heightfield (§3), any surface* | *2,048 tops at most (4 grid tiles), skirts only at real drops* | *4 × 648 B of heights* | *none* |

Calm water merges into a few greedy quads; a wave (columns at different eighths) can't merge and adds a
side face at every step between columns, so the same lake carries tens of times the triangles while it
moves, and each such change is a full chunk re-mesh.

**Network:** water travels as block edits (the server's writes) and the optional `WaterCurrents`
datagrams; not separated in the telemetry (no per-message byte counter for block edits), so not
measured here.

### 1.4 A phone

There is no iPhone here; this is an *estimate*. The owner's iPhone ran 60 fps at a GPU time of ~3.5 ms on
terrain earlier. A recent iPhone GPU is roughly half to two thirds of the M1's 8-core GPU, and the cost
is per pixel. On the Air at 1280 × 800 drawn, the water's per-pixel part is ~2 ms where it fills a third
of the screen and its fixed part ~0.6–0.7 ms; on a phone drawing a similar pixel count that is ~3–4 ms
and ~1–1.4 ms, so terrain plus close-up High water ≈ 7–8 ms: inside 16.7 ms, but it is half again the
terrain's cost, and phones throttle and drain on sustained load. The 2026-11-06 report (chugging close to
the water on mobile) was before the Fresnel cut and caustic tile.

**A phone should get a "see-through Low"** (§2, option 1): the copy, refraction, absorption and the
caustic tile (cheap, and they are what reads as water), no land reflection, one ripple layer, no flow
foam streaks (flecks and flow ripples kept), half-resolution water on the smallest devices. Today's Low
(opaque blue) should be the fallback for a CPU adapter only.

## 2. Cheaper but pretty: options per costly piece

Ranked by the measured cost at the dam close-up (1280 × 800 drawn), the largest first.

| # | Piece | Cost now | Option | Saves (*estimate*) | Look |
|---|---|---|---|---|---|
| 1 | The fixed part: scene copy + water tiles | ~0.6–0.7 ms whenever water is in view, any amount | **Copy only the water's screen rectangle** (the water meshes' projected bounds, scissored) and **draw the tiles every other frame** (their animation is slow; or at 256²) | 0.3–0.5 ms in views with little water | none |
| 2 | Land reflection (SSR) | 0.25–0.45 ms | **Half-resolution SSR** (march at ½ × ½, upsample by depth), or march every other frame and reproject; on phones off, replaced by a **stylized reflection**: the sky plus a darkened, blurred band of the bank's average colour above the shoreline (a horizon tint, no march) | 0.2–0.35 ms; all of it on phones | half-res: soft reflections, fine on rippled water; the stylized band reads as "reflected bank" at a distance, not up close |
| 3 | Flow foam streaks | 0.27 ms | Sample the streaks from a **pre-advected foam tile** (one 256² tile per fluid advected by a global flow, as the caustics tile) instead of taps along the flow per pixel; or only within `fadeToM` of the eye with a cheaper far version | ~0.2 ms | same dashes |
| 4 | Ripple layers 2 and 3 | 0.12 ms | Bake the three layers into one tile per frame (the tiles pass already exists) | ~0.1 ms | none |
| 5 | High vs Medium | ~0.1 ms | Fold High into Medium (one desktop tier) and spend the difference on nothing; keep High only for grazing views if the owner prefers its reflections | — | High's finer reflections at grazing angles |
| 6 | Whole water shading | ~2 ms (a third of the screen) | **Half-resolution water** for phones: draw the water's shading at ½ × ½ into its own target and composite with a depth-aware upsample (the plan's W5 half-res copies) | ~1.5 ms where water fills the screen (*estimate*: ¾ of a per-pixel cost) | softer ripples and caustics; edges kept by the depth test |
| 7 | Caustics, refraction, absorption, sparkle, glint, foam, reflected clouds | each ≤ 0.1 ms | **Keep.** They are what makes it pretty and are already cheap | — | — |

**The stylized route** ("I'm good if it's a bit stylized"): a phone tier that keeps the see-through and
caustics (cheap and the most "water"), uses the stylized reflection band instead of SSR, one ripple
layer from the tile, flow shown by flecks and flow ripples (no streak taps), and the GPU heightfield
(§3) for smooth moving water. It should cost ~1 ms at the dam close-up where High costs ~2 (*estimate*)
and read as one look with the desktop's.

## 3. The water as a GPU heightfield (the surface's own layer on clients)

**Today** the server writes its grid's levels as water blocks in eighths of a ½ m cell
(WATER_SURFACE_GRID.md §2c), and clients mesh them inside the terrain chunks. Three costs follow from
that, measured in §1.3:

- **A wave re-meshes terrain.** Any water level crossing an eighth rewrites blocks, so the whole
  terrain chunk (its ground, plants and water) is re-meshed on a worker and re-uploaded.
- **Waves are expensive geometry.** Columns at different eighths can't merge into greedy quads, and
  each step between neighbouring columns adds side faces: a wavy lake chunk carries several times the
  triangles of a calm one (§1.3).
- **It looks blocky.** Eighths are 6.25 cm steps, read as terraces when the water moves.

**The proposal.** Give the client the water layer §2a always meant to have, as a heightfield drawn
on the GPU:

1. **One fixed grid mesh**, shared by every water chunk (16 × 16 columns, the grid's chunk): a
   static vertex and index buffer, drawn instanced, one instance per water chunk in view. No CPU
   meshing at all.
2. **A small height texture**: each water chunk a tile of an `r16float` atlas, 18 × 18 texels (the 16
   columns plus a one-texel apron copied from the neighbours), holding each column's level in the
   face's frame (NaN or a sentinel where dry). A level change uploads 18 × 18 × 2 = 648 bytes with
   `writeTexture`, not a chunk mesh.
3. **Smooth, corner-shared heights** (owner, 2026-11-07: "Water can be smooth, no need to keep
   blockyness, either is ok if one is more performant"): the vertex shader puts each grid corner at
   the mean of the wet columns around it whose levels lie within `joinM` (data, ~0.2 m) of each other.
   Corners are shared across chunks through the apron, so there are no seams between tiles.
4. **Real drops keep geometry.** Where two neighbours differ by more than `joinM` (a river's ½ m step,
   a dam's face, a breach front), the corner is not shared: each column keeps its own four corners
   (the grid is built per column, 4 top vertices each), and each column edge has a pre-built skirt
   quad that the vertex shader opens down to the lower side's surface (or the floor where it is
   dry) only there, and collapses to nothing elsewhere. The geometry is fixed (16 × 16 × (4 + 4 × 2)
   = 3,072 vertices, 1,536 triangles at most per chunk); the shader decides which of it shows.
5. **Shores.** A dry column's corners take their wet neighbours' level, and its fragments are
   discarded past the shoreline (the column mask in the same texel), so the water meets the bank as
   today; the existing depth-based foam and absorption already soften the edge.
6. **Physics and the server are unchanged.** The server keeps writing water blocks (physics,
   saving, old clients); a client with the layer simply does not mesh liquid tops and sides from
   blocks (a mesher flag), except where the heightfield can't stand in: flooded shaped blocks and
   water against a shaped neighbour keep their meshed faces.
7. **A compact optional message.** `WaterLevels`: per awake water chunk, its 16 × 16 levels as `u16`
   centimetres over the chunk's lowest (≈ 512 bytes, less with run-length for calm chunks), sent as
   currents are today (the chunks within the flow's reach, when they change, at most a few a tick).
   An optional tag (top bit, as `WorldSettings`), so **no compatibility break**: an old client
   ignores it and draws the blocks; a new client without it (an old server) derives levels from the
   eighths it already has. The eighths remain the fallback and the far truth.
8. **LOD** (owner: "Far away can stay simpler, whatever is most performant and blends consistently"):
   the far water is the same heightfield at a coarser stride: each LOD section samples the atlas's
   mip (levels averaged over 2^k columns), so the near and far water agree on the level at the join
   by construction; beyond the full-detail distance it is flat per LOD tile (no skirts), with the
   cheap tier's shading (no land reflection far off). The LOD sections stop meshing water from blocks.

**What it saves** (*estimates* from §1.3): the main-thread and worker cost of re-meshing terrain for
water (today a wavy chunk re-meshes its terrain too), most of the water's triangles while it moves
(a fixed 512 top triangles per chunk against thousands of steps), and the upload (648 bytes against a
chunk mesh). GPU: a vertex texture fetch per vertex (3,072 per chunk in view, negligible against the
pixels). It costs one new pipeline, an atlas and its allocator, and the message.

**Risks.** The surface no longer matches the blocks exactly (up to an eighth, 6 cm): swimming and
the eye's waterline must read the same level (the composite's `waterline_m` reads the levels, not
the cell); a test pins the heightfield's level to the grid's. Vertex displacement on water was a
risk in §6 of WATER_RENDERING.md (seams against greedy quads): the heightfield avoids it by drawing
water from its own grid only, never moving block-meshed water.

## 4. Waterfalls like the reference

Reference (owner, 2026-11-07): [`waterfall-reference.jpg`](../reference/water/waterfall-reference.jpg),
a tiered tropical cascade: white streaky sheets with vertical streaks, darker water showing through
between them, mist at the base, ripples spreading on the pool. "Mesh or particles, whichever is most
performant."

**Recommendation: mesh for the sheets, a few large mist cards, rings in the pool's shader.**

- **The sheet: a ribbon mesh** (as today, the curve of 6 segments), shaded from one baked **streak
  tile** (a 64 × 256 texture of vertical streaks drawn once at load, like the water tiles): two taps
  scrolling down at the fall's speed and at 0.7 × it, alpha = streaks over a darker translucent base
  in the fluid's deep colour, so dark water shows between white streaks as in the photo. A ribbon is
  one layer of overdraw over exactly the fall's pixels: cost ∝ the sheet's screen area, 2 taps a
  pixel. Particles for the same look need dozens of overlapping quads per metre of lip (the streaks
  are long and continuous): 5–20 layers of overdraw on the same pixels, the worst case for a phone's
  tile GPU.
- **Tiers come free**: the grid already makes a cascade a chain of falls (each drop over
  `minDropM` its own sheet), so a terraced river or a stepped dam draws as the reference's tiers.
- **Mist: 2–4 large soft cards** per fall at the landing (billboards, low alpha, slowly growing and
  fading), not many small puffs; on Medium and High faded against the scene depth (soft particles, the
  depth the water already reads) so they don't cut into the pool. Few cards = little overdraw.
- **Pool ripples: rings in the water shader**: the nearest falls' landings (at most 8, a small
  uniform array, already found every 0.25 s by `dwell_world::falls`) add concentric ring normals
  within a few metres, so the pool's own surface ripples; no geometry, a few ALU per water pixel
  inside the radius only.
- **Data** in `shared/look/waterfalls.json` (streak density, speeds, the base's darkness, mist cards'
  count, size and life, ring speed and reach), validated as today.

**Not measured:** no fall tall enough to draw a sheet was in any measured view (the flood's trench
filled level with the river), so `no-falls` read as noise; today's cost is by construction a few quads
per fall and a few noise taps a sheet pixel (WATER_RENDERING.md §11). Before building §4, measure a
notched dam over a pit (WATER_SURFACE_GRID.md §2e step 1) with the `view` step's `waterfalls` switch,
and keep the new look within that.

## 5. The owner's answers (2026-11-07)

- **Goal:** "cheaper water in general, preferably without sacrificing it looking pretty … our water
  might be a bit overdone at max settings." "Pretty water that can run in mobile, but I'm good if it's
  a bit stylized. We can use cheap routes if the results are decent and cohesive."
- **Smooth surfaces:** "Water can be smooth, no need to keep blockyness, either is ok if one is more
  performant." The heightfield (§3) is smooth and the more performant of the two.
- **Far water / LOD:** "Far away can stay simpler, whatever is most performant and blends consistently."
  The far water reads the same levels at a coarser stride (§3, item 8).
- **Waterfalls:** the reference photo ([`waterfall-reference.jpg`](../reference/water/waterfall-reference.jpg));
  "mesh or particles, whichever is most performant" (§4: a mesh).
- **The waterline** reference, for §12 of WATER_RENDERING.md, is kept beside it:
  [`waterline-reference.webp`](../reference/water/waterline-reference.webp).

## 6. Open questions

1. **Phone tier:** adopt the "see-through Low" (§1.4, §2) as the phone default and today's opaque Low as
   the CPU adapter's only? Or keep today's Low on phones and offer the new tier as Medium?
2. **Desktop tiers:** fold High into Medium (they measure the same except at grazing angles), leaving Low
   / Medium (= the whole look) / maybe a phone tier?
3. **Reflections on phones:** the stylized bank band (no march) or half-resolution SSR?
4. **The heightfield's message:** send the grid's true levels (`WaterLevels`, smoother and exact) or let
   clients derive levels from the eighths they already get (no message, ±3 cm, still smooth)?
5. **Waterfall mist** at the base: soft and pale as the photo, or tinted like the flecks?

## 7. Build plan (cheapest wins first)

| Step | Work | Expected saving (*estimate*) | Risk |
|---|---|---|---|
| **C1** | The see-through phone tier as data (`reflections.json` phone overrides plus the water quality: copy, refraction, absorption, caustic tile; no SSR; one ripple layer; no flow streaks) | ~1 ms at the dam on a phone-sized frame against Medium | low: data and tier switches |
| **C2** | Copy only the water's screen rectangle; tiles every other frame | 0.3–0.5 ms where little water shows | low |
| **C3** | Flow streaks and ripple layers from tiles (one advected foam tile, the ripples baked) | ~0.3 ms where water fills the view | low–medium: the look must match |
| **C4** | Half-resolution SSR on desktop; the stylized reflection band on phones | 0.2–0.35 ms | medium: upsample edges |
| **C5** | Waterfalls per §4 (streak tile, mist cards, pool rings), within today's waterfall cost | — (a look change at the same cost) | low |
| **C6** | The GPU heightfield (§3): the grid mesh, the height atlas, the mesher's flag, levels from eighths first | the flood's 77 chunk re-meshes a second and ~1.3 ms of main thread go to ~0; tens of times fewer water triangles while it moves | medium: shores, drops, the waterline's level |
| **C7** | The optional `WaterLevels` message (exact smooth levels) and the LOD water from the atlas's mips | smoother water; LOD water stops re-meshing too | medium: a new message (optional, no compatibility break) |
| **C8** | Half-resolution water shading for the smallest phones, if C1 isn't enough on a device | ~1.5 ms where water fills the screen | medium |

Each step measures before and after with the scratch scenarios named in DEV_TOOLS.md "Water cost (M1
Air)" and shows the owner the same views side by side.
