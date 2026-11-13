# Water rendering — surface, reflections, refraction, underwater

**Status:** Adopted ([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md)), 2026-10-14. Built as Phase 26
of the [implementation plan](./IMPLEMENTATION_PLAN.md) (steps W1–W5; W1 split into W1a, the fluid data,
and W1b, the water shader; W6, water under ray marching, was dropped with the ray marcher at Phase 25's
A7, [ADR 0044](./adr/0044-mesher-over-ray-marching.md)). Owner answers in §7.

## 1. Water today

*As it was when this was written (2026-10-14). W1a, A1 (= W2) and W1b have since changed the drawing; see §8.*

- **A block, no fluid simulation.** `dwell:water` is one state with `"look": "water"`,
  `"liquid": true`, colour `#3fb2d6`, opacity 0.55 (`shared/blocks/dwell.json`). Worldgen fills open
  space below sea level (0). Shaped blocks can be `flooded`.
- **Meshing.** The greedy mesher emits every water face touching a non-water neighbour (sides too),
  with the top lowered to 0.875 of a cell (`WATER_SURFACE`, `crates/dwell-mesh/src/mesher.rs`). LOD
  water is meshed per section at each column's water level with the same 1/8 m drop.
- **Drawing.** Its own pipeline variant: alpha blended, no culling, no depth write, sorted far to
  near per chunk mesh. Opacity is a hardcoded `WATER_OPACITY = 0.55` (`render/mod.rs`) that
  duplicates the registry's value. The shader is the ordinary terrain shader: no normal detail,
  Fresnel, specular, reflection or refraction.
- **Why reflections and refraction can't be added today.** The frame renders with 4× MSAA straight
  into the canvas, and depth is render-attachment only with `StoreOp::Discard`. No scene colour or
  depth can be sampled. This is the main structural change (the shared frame: W2, built as Phase 25's
  step A1).
- **Submersion.** Only physics knows (`submerged_fraction`, `crates/dwell-physics/src/query.rs`),
  and it counts the whole cell as wet while the drawn surface is at 0.875. An eye test needs the
  cell at `floor(eye)` to be liquid **and**, when the cell above isn't water, `frac(eye.y) < 0.875`
  — otherwise the view goes underwater 12.5 cm early. Bifacial gravity: the test and every effect
  use the face's up.

## 2. Data model (extendable)

A named fluid look in the block registry, referenced by name:

```json
"fluids": { "dwell:water": {
  "absorption": [0.45, 0.09, 0.06], "scatter": "#1d6f8a", "roughness": 0.08, "ior": 1.333,
  "normalScale": 1.0, "waveScale": 2.0, "flowSpeed": 0.3,
  "foam": { "depth": 0.4, "color": "#eef6f8" }, "caustics": 1.0, "surfaceDrop": 0.125 } }
```

- Blocks use `"look": "fluid", "fluid": "dwell:water"`. `shared/blocks/gen.mjs` validates ranges
  and required fields and emits a fluid table, uploaded as a storage buffer indexed by fluid id.
- The mesher writes the fluid id per vertex instead of branching on `Look::Water`;
  `WATER_SURFACE`, `CHUNK_WATER_DROP_M` and `WATER_OPACITY` come from the data (`surfaceDrop`,
  absorption) or go.
- A fixture test adds a second fluid (`test:oil`) by data alone and sees it meshed and drawn with
  its own parameters. The values above are prototype content.

## 3. Techniques

**Surface.** Two or three scrolling normal maps blended (GPU Gems 2; Valve's flow maps), Schlick
Fresnel (F0 ≈ 0.02), GGX sun glint with roughness per fluid. **No vertex displacement on chunk
water**: greedy quads have too few vertices, and moving the top opens seams against banks, flooded
shapes and the chunk/LOD join. Gerstner or FFT waves are evaluated for normals only.

**Absorption and foam.** Beer–Lambert `exp(-σ·d)` with `d` the distance from the water surface to
the scene depth behind it, plus a scatter colour; this replaces the fixed alpha. Shoreline foam
where that distance is under the fluid's foam depth, masked by noise (the approach in voxel shader
packs such as BSL and Complementary).

**Refraction.** Sample the scene colour copy at `uv + n.xz·strength/viewDepth`. If the scene depth
at the offset uv is in front of the water, fall back to the undistorted uv (foreground bleed fix).
WebGPU: a texture can't be sampled while it's an attachment of the same pass, so end the opaque
pass, resolve, then draw water with `LoadOp::Load`. MSAA colour can't be copied to single-sample —
resolve instead. MSAA depth can't be resolved — read sample 0 via `texture_depth_multisampled_2d`,
or write linear depth to a colour target that resolves.

**Reflections.** Raster: screen-space reflections (McGuire & Mara), faded at screen edges into the
analytic sky (`dwell_sky(reflect(v, n))`, already available and free). Planar reflection doubles
opaque draws and only suits one plane (sea level), so it's an Ultra-tier extra at most. (A
ray-marched reflection through a GPU voxel structure was the planned exact path; it went with the ray
marcher, ADR 0044.)

**Underwater.** Eye test into a `Frame` uniform (underwater flag, surface height, fluid id). The same eye test muffles
the sound (a low-pass on the world's buses, eased at the crossing; docs/AUDIO.md §3a, BUGS.md #104).
Per-pixel split where a lens point along each view ray crosses the surface (Crest's "water line"; built, §12). Per-channel
absorption fog replaces height fog while submerged. The surface from below (the water mesh is
already double-sided): outside Snell's window (critical angle `asin(1/ior)`, about 48.6°) total
internal reflection; inside, refract to the scene above. Caustics projected on opaque fragments
below the surface by world xz and the sun direction; god rays as a screen-space radial blur; a small
UV wobble in the composite.

## 4. Quality tiers (settings menu, next to `detailSettings.ts`)

**As built since 0.50.29** (owner, 2026-11-07; proposals/WATER_GPU_SURFACE.md C1): the tiers are data,
`shared/look/watertiers.json` (cheapest first; `watertiers.mjs` validates it and emits dwell-look
`watertiers.gen.rs`, CI checks it is current; the shell reads the same JSON in `ui/waterSettings.ts`). Each
tier names what it draws (`readsScene`, `opacity`, `surfaceCaustics`, `rippleLayers`, `flowStreaks`, `bankBand`), whether Settings offers it
(`setting`) and whether it is the CPU adapter's (`cpu`, exactly one); its reflections are its entry in
`reflections.json`. The renderer's `WaterQuality` is an index into the list, and the frame uniform's
`water.w` carries the tier's switches as bits (dwell-look `tier_bits`; the WGSL's `water_tier_*`), so a new
tier is a data entry. Removed tiers are `aliases`: a stored or named `"high"` reads as Medium (the settings
store keeps the stored value as it was, append-only).

**The GPU heightfield (C6a, 0.50.69; proposals/WATER_GPU_SURFACE.md §3) [in progress].** A tier's
`heightfield` switch (all off) will draw the near water's surface from a per-chunk tile of levels on
one shared 16 × 16-column grid instead of meshed tops: `dwell-mesh` `heightfield.rs` fills a tile
(18 × 18, the columns and a one-column apron, face-local chunk-local metres, `DRY` where no top
shows) from the eighths the client already has, a corner takes the mean of the wet columns around it
within `heightfield.joinM` (0.2 m) of the highest, and the chunk mesher's `ChunkFlags::water_tops`
drops liquid tops only (sides, falls, pour slopes and flooded cells stay meshed). The vertex shader
(`dwell-client` `render/heightfield.rs`) reads the tiles from an `r32float` atlas with `textureLoad`
and crosses into the world with `face_y`. The draw is built (0.50.72): chunk meshes meshed without tops carry their tile
(`ChunkMeshes::water`, with each column's flood light), the renderer's `HeightfieldGpu` keeps them in
an `rg32float` atlas (group 3 of the water pipelines) and draws a tiled chunk as one `heightfield_vs`
grid draw feeding `water_fs`/`water_scene_fs`; the streamer meshes without tops while the tier has
`heightfield` (`Scene::draws_heightfield`). 0.50.77: the grid is drawn unindexed, six vertices a
column, and a dry column's six collapse to one point outside the view (`dwell-mesh`
`heightfield::grid_vertex` is the shader's reference), so nothing under the ground or past the shore
is shaded and the surface ends at the wet columns' edges; a chunk whose only changes are its water's
levels (each changed cell liquid before and after, the same liquid) gets its tile rewritten on the
main thread (`heightfield::water_tile`, the streamer's `TerrainView::set_water_levels`) and no mesh
job, and is re-meshed once after `WATER_SETTLE_PASSES` (60) quiet mesh passes so its sides and
falls catch up. Still off in every tier: measured in 0.50.78 (`?heightfield=1`, the bench's `heightfield`
configuration) about equal on the GPU on the river (`gpu.near.water` 2.51 → 2.64 ms) and only 18 %
fewer chunk uploads in a flood (`water-flood`), whose re-meshes are mostly its wet front, which the
skip doesn't cover (proposals/WATER_GPU_SURFACE.md C6a). 0.51.5 (W1, W2): the skip covers any
water-only change (each changed cell air or a liquid before and after, the same liquid where both:
the wet front advancing or drying too), a chunk's first water making its tile and drying freeing
it; and the water's sides to open air are drawn from the tile as **skirts**: the same draw's
vertices past the grid's (24 a column, four sides), each side a vertical quad from the surface's
edge (its corners' heights) down to the neighbour's water where that is lower by more than
`joinM`, else, where the neighbour is dry, to the column's own floor (the run of the same water
under its surface, packed into the tile's second channel over the light: `heightfield::pack_column`,
`column_floor`; `skirt_vertex` is the shader's reference, tested on both faces). The chunk mesher
leaves those sides out under the same flag (`skirt_covers`: an open-air neighbour, a cell in its
column's top water, above that side's skirt bottom); falls, pour slopes, the same water's steps and
sides against shaped or see-through blocks stay meshed. Still off: measured in 0.51.5 the flood's
chunk uploads only 10 % fewer and the near water's GPU time 0.7–1.1 ms dearer, with bright strips
along shaped banks and holes in the flooding pit (proposals/WATER_GPU_SURFACE.md C6a). The CPU
adapter's tier may not set it (a per-vertex fetch it doesn't need).

| Tier | Contents |
|---|---|
| Opaque (the CPU adapter only, not in Settings) | Normal maps (one layer), Fresnel, glint, sky-only reflection, absorption at constant depth; underwater fog only. No copies (the old Low) |
| Low (phone default; Low and Mobile presets) | See-through **without the scene copy**: drawn blended in the scene pass (no split), at `opacity` 0.9 of the fluid's own so the floor shows through, the caustic tile's lines added on the water (`surfaceCaustics`, as if on a floor `caustics.depthM` down); one ripple layer, no flow streaks, no land march, one cloud octave reflected. No refraction distortion, no depth absorption, no shoreline foam |
| Medium (desktop default) | The whole look: three ripple layers, flow streaks, SSR (12 steps; a phone's 8) and the clouds reflected (W5a). High was folded in (0.50.29): it measured within 0.1–0.3 ms of Medium |

**Why Low has no copy** (owner, 2026-11-07): on the owner's iPhone, floating at the surface looking down ran
~45 fps and fully under water ~60. The scene copy splits each scene pass with water in two (opaque half,
copy, water half); on a phone's tile GPU each split stores the whole frame out and loads it back, which the
M1's bandwidth hides. Low now draws the water in the same pass as the ground, blended: **zero water pass
splits** (one far and one near scene pass, as with no water), the caustic tile's own small pass before them.

**The bank band** (built, but no tier draws it since Low dropped the copy: it samples the scene copy;
`watertiers.json` `band`; WGSL `bank_band`; a scene-reading tier may set `bankBand`): the land reflected without a march. The
reflected ray's point `reachM` (6 m) out is projected on screen once; where the scene's depth there stands in
front of it (within `depthSlack` of its depth: the bank above the water's edge), four taps of the scene copy
(`spreadPx` across, `blurPx` up) give a soft strip of the bank's colour, darkened by `darken`, mixed in at
`strength` × the surface's reflectance and faded toward the slack's end. Sky and far land past it leave the
sky's reflection. One depth load and four colour taps where the surface reflects enough to see land.

**The caustic tile every other frame** (`causticEveryFrames`, render/water_tiles.rs `caustics_due`): the tile
is redrawn every second frame while the water reads the scene, at once on the first frame water is back in
view. The caustics drift slowly; a frame's lag does not show.

Copy cost at 1080p (*estimate*): about 25 MB per frame of colour and depth copies, plus the resolve.
Trivial on desktop; real bandwidth and power on phones' tile GPUs, hence half-res copies there.

## 5. Plan

| Step | Work | Exit check |
|---|---|---|
| **W1** | Fluid data in the registry (§2), a dedicated `water_fs` (normal maps, Fresnel, GGX, sky reflection), the eye-submerged uniform with the 0.875 rule. No new passes | Fixture-fluid test; **regression test for the cell-vs-surface mismatch, seen failing first**; owner look |
| **W2** | The frame restructure (built once, as Phase 25's A1, and kept: the shared frame) | As A1 |
| **W3** | Refraction with the depth check, Beer–Lambert absorption, shoreline foam; caustics (brought forward from W4) | Screenshots of shore views; bench |
| **W4** | Underwater: fog, the split line (built, §12), Snell's window and TIR, distortion, god rays | Named views underwater and half in; owner look |
| **W5** | SSR and the quality tiers, phone defaults (SSR and the reflected clouds came forward as W5a, §8) | Bench on desktop and a phone |
| ~~**W6**~~ | ~~Under ray marching: reflection and refraction rays replace SSR and the copy on high tiers~~ — dropped with the ray marcher (ADR 0044) | — |

Each step updates `ARCHITECTURE.md` (render, `dwell-look`, `dwell-mesh`); the steps are Phase 26's
deliverables in the implementation plan, with W1 split into W1a (the fluid data, the mesher reading it
and the eye-submerged rule) and W1b (the water shader, after A1). None of it changes
public API: fluids are rendering data, and the 0.875 surface stays where it is.

## 6. Risks

- The far/near depth split and MSAA make scene depth awkward (two clears, no MSAA depth resolve).
- Phone bandwidth for copies and stored depth.
- Seams if anyone attempts vertex waves on greedy or LOD water.
- Per-chunk sorting without depth writes mis-sorts water behind water (waterfalls, if they come).
- SSR artifacts at screen edges; reflections of offscreen terrain missing (screen space).
- Moving tone mapping into the composite changes the shipped look slightly; needs a side-by-side.
- MSAA is off on SwiftShader CI, so e2e screenshots differ from GPU runs.

## 7. Questions for the owner

1. Are fluids **per build** (registry data, recommended) or **per world**? *Owner, 2026-10-14: per
   build, in the block registry.*
2. Do you want an editor page for fluid parameters, like `docs/prototypes/`? *Owner, 2026-10-14: no;
   the values are edited in the data file (a page can be added later without changing the format).*
3. Which tier is the target look on desktop, and is Low acceptable on phones? *Owner, 2026-10-14:
   Low on phones, tentatively; revisit once it's measured and seen on the two target phones.*
4. Are more fluids expected (lava, oil), and do they need different behaviour than looks (glow,
   viscosity) — which would change what's data and what's code? *Owner, 2026-10-14: yes, other
   fluids, looks only.* So the fluid entry (§2) is the whole system: a new fluid is a new entry, and
   the fixture test adds one by data alone. Behaviour (flow, viscosity, damage) stays out of scope.
5. Should waves ever move geometry (open ocean), or stay normal-only? *Owner, 2026-10-14: water
   doesn't move for now* — the surface geometry stays still; ripples are only in the shading.

## 8. As built

### W1b, the water shader (2026-10-15)

- **Fluid id per vertex.** The meshers write a `fluids` stream (one f32 per vertex) on the chunks'
  transparent mesh and the sections' water mesh, and the renderer draws both with a fluid vertex
  format: position, normal, colour, fluid id. That is 40 bytes, down from the 60-byte terrain format
  chunk water used, because the procedural ripples need no uv or tile.
  - *Rejected:* a fluid id per draw (in the draw uniforms). It needs the mesher to split a chunk's
    transparent mesh by fluid, and a draw per fluid wherever several meet (flooded cells, oil beside
    water).
  - *Rejected:* packing the id into the terrain format's tile. That keeps 60 bytes and overloads a
    field.
  - A see-through face that is no fluid's gets `NO_FLUID` and draws its vertex colour at half
    opacity. None exist yet.
- **The fluid buffer** (`render/water.rs` `pack_fluids`): 5 vec4 per fluid, from the registry's
  `FLUIDS`, bound with the frame uniform. Colours are linear and F0 is computed from the IOR. A
  fixture test packs `test:oil` beside water, each with its own parameters.
- **`water_fs`:**
  - **Ripples:** three layers of quintic gradient noise with analytic derivatives, at 1, 0.43 and
    0.17 × the fluid's `waveScale`, each on an integer-turned lattice. Each layer scrolls along its
    own heading at `flowSpeed` × 1, 1.7 and 2.6 (`flowSpeed` 0.3 m/s since 0.29.1: the largest ripple crosses its 2 m in under 7 s; at 0.03 it took 67 s and hardly seemed to move, owner 2026-10-26; `the_ripples_visibly_move` pins it; the caustics keep their earlier pace). They are summed as height slopes, scaled by
    `normalScale`.
  - **Wrap:** each wavelength is rounded to divide 1,024 m. The vertex shader takes a per-draw
    origin wrapped to that period, so the detail stays exact anywhere on the disc.
  - **Anti-shimmer:** a layer fades out while a pixel covers ⅛ to ½ of its wavelength, measured with
    `dpdx`/`dpdy` of the wrapped position. The slope variance it loses widens the GGX glint
    (Toksvig). Far water is calm and keeps its colour, and the sun's highlight spreads into a broad
    sheen instead of sparkling.
  - **Lighting:** Schlick Fresnel, GGX (height-correlated Smith) for the sun and moon with α ≥ 0.05,
    clamped. The sky's reflection is `dwell_sky(reflect(v, n))` taken back into the scene's linear
    light through the inverse of the composite's ACES fit. The matrices and fit are emitted from Rust,
    and a test round-trips them. Reflections below the surface's plane fold back up.
  - **Body and alpha:** the body is the scatter colour lit from the face's up. The alpha is
    `1 − (1 − F)(1 − opacity)`, so what is behind shows through the share neither reflected nor
    hidden.
  - **Face B:** the mirrored surface's normal points down, and so does the rippled one.
  - **From below:** the body colour, with total internal reflection outside Snell's window.
- **Underwater (the W4 stub):** while `water.x` is set, the composite replaces the height fog with
  the fluid's Beer–Lambert absorption and in-scatter. It applies them over the distance in the fluid:
  to what the pixel shows, or to the surface where the ray reaches it first.
- **Quality:** Low, Medium and High in the settings menu (`ui/waterSettings.ts`, `dwell.water`, an
  append-only record). In W1b, Low draws one layer of ripples, and Medium and High draw three. Phones
  default to Low and desktops to High.
- **Data tuning** (prototype content), from screenshots: `dwell:water`'s scatter changed from
  `#1d6f8a` to `#2b9cc0`, because the old value read as black underwater and grey from above. Its
  opacity changed from 0.55 to 0.65, so the shallows' sand doesn't wash the colour out.
  Then, at the owner's request for **bluer** water (a clear, saturated blue, away from turquoise and
  green): the block colour went from `#3fb2d6` to `#2f86e0` (it paints the `water` tile, so
  `crates/dwell-mesh/src/textures.rs`'s `WATER` follows it; the LOD's flat colour is that tile's
  average, and near and far water both shade with the fluid's scatter); scatter from `#2b9cc0` to
  `#1c7ed8`; absorption from [0.45, 0.09, 0.06] to [0.7, 0.28, 0.06] (more red and green absorbed,
  so the underwater fog reads blue rather than teal); opacity from 0.65 to 0.72, so the sand under
  the shallows doesn't grey the blue. Glint, Fresnel and sky reflection are unchanged. Then, for a
  **tropical** look (owner): scatter `#1a8ee0` and absorption [0.45, 0.12, 0.04], near clear
  water's own (red absorbed first, green a little, blue least), so the underwater view reads a
  clear cyan-blue and, once W3 grades the colour by depth, shallows read turquoise and deep water
  blue.
- **Bench:** the `water-views` scenario goes from the seed-7 spawn to the world map's nearest deep
  sea (0.49.15: `goto`, DEV_TOOLS.md "Finding places"; it flew to a coast about 20 km
  east-south-east until a terrain change moved it) and shoots four views: across the sea, toward
  the shore, 1 km up, and with the eye just under the surface. The `camera` step gained `aroundPlayer`
  (x and z offsets from the player's feet), and views and scenarios gained `settings.water`.
- **Cost** (Apple M1, 1280×720, 4× MSAA): measured while other agents' builds loaded the machine, so treat it as noisy. Over the sea (`water-views` `sea`, `--unthrottled`), GPU frame p50 was 2.06 → 2.76 ms (+0.7). The near pass, where the water draws, was 1.02 → 1.94 ms. Frame interval p50 was 9.7 → 10.2 ms (CPU-bound). On `fly-low-fast` (inland, no water) there was no change within noise: GPU frame p50 2.31 → 2.10 ms, interval 8.8 → 8.3 ms.
- **Known, not W1b's:** vertical water faces at sea-floor steps show as faint dashed bands from
  above (they were there before). The near shore is pale over bright sand until W3's depth
  absorption.

### W3, the water reads the scene (2026-10-15)

The owner asked for tropical water: rich clear blue in the deep, luminous turquoise to cyan in the
shallows over sand, never murky or green, with caustics the star under the surface. W3 grades the
colour by depth, which is what makes the shallows work.

- **Frame (`render/frame.rs`, `render/mod.rs`).** On Medium and High, a scene pass with water is
  split in two:
  1. Its opaque half keeps the multisampled colour and the depth buffer, and resolves the colour.
  2. The resolved colour is copied (`copy_texture_to_texture`) into the **under-water copy**, a
     single-sample `rgba16float` allocated only on these tiers.
  3. Its water half loads the colour and depth and draws the fluids with `water_scene_fs`. It reads
     the copy and the view depth as group 2 (`frame::under_layout`: the copy filtered, the view
     depth as the composite reads it, a linear sampler). It doesn't attach the view depth, so it
     can read it.

  The far and near passes split alike, so far water reads the far scene and near water reads both.
  Low keeps W1b's single pass, `water_fs` and no copy. Changing the tier rebuilds the targets.
  The opaque half is timed as the pass (`gpu.far`, `gpu.near`), and the water half as
  `gpu.far.water` and `gpu.near.water`. Only the raster side of the renderer seam splits.
- **`water_scene_fs`:**
  - **Thickness:** the ray's length in the water is `ray_thickness`, the view depth behind minus the
    surface's, scaled along the ray (nothing behind counts as deep). Its depth under the surface is
    the floor point's height below it.
  - **Refraction:** the scene is sampled at a pixel offset from the ripples' slope × the fluid's
    `refraction`, scaled to the view depth and by the thickness up to 1 m (none at the waterline).
    It falls back to the undistorted pixel where the offset sample is in front of the water
    (`refracted_sample`, the foreground-bleed check). Depth is read from sample 0, one load per
    lookup.
  - **Colour, Beer–Lambert (`through_fluid`):** what is behind keeps `exp(−(σ + s)·length)` along
    the ray, where σ is `absorption` and s is the new `scattering`. It was lit by light that came
    down through the water, which keeps `exp(−σ·depth)`. The lit body (the scatter colour) fills the
    rest. So the colour grades from almost clear over sand at the edge, through turquoise (red goes
    first), to blue. The surface's Fresnel reflection, glint and sparkle go over it. The output is
    the whole view through the surface (alpha 1).
  - **From below:** unchanged (the W4 stub).
- **Caustics** (brought forward from W4 at the owner's request). They are procedural, with no
  texture:
  - **Pattern:** one layer of moving Worley cell edges (F2 − F1), warped so the lines wander, plus a
    finer layer when near. Each cell divides the 1,024 m wrap (`caustic_period`). The lattice hash
    uses no trig.
  - **Placement:** evaluated at the floor point, shifted back along the refracted sun, and animated
    with the flow speed.
  - **Brightness:** they fade with depth (`exp(−depth / caustics.depth)`, `caustic_fade`) and where
    a pixel covers a good part of a cell (no aliasing far away). They are focused, not added: a
    little dimmer between the lines, brighter on them.
  - **Light source:** the moon gives faint ones (0.2 of the sun's). In daylight on the face's own
    side, the sun gives them.
  - **Cost:** they are skipped where they would be under 2 % strength.
- **Caustics under the surface.** While the eye is under a fluid on Medium and High, the composite
  lights every opaque pixel below the surface: the seabed, walls, blocks. That light came down
  through the fluid, `exp(−σ·depth)`, and is focused into the same caustics, 1.7× as bright as seen
  from above. Then comes the fluid's fog over the distance to it. The fog takes absorption plus
  scattering. Under the surface the caustics are the main show, and the fog doesn't wash them out
  in the shallows.
- **Shoreline foam:** where the water over what is behind is under the foam depth, thin drifting
  lines lap toward the shore, broken up by noise, with a soft rim where the water meets the bank
  (at most 0.8). On voxel shores the bank is a step, so the rim shows as a thin line along the
  waterline. *Replaced in 0.52.6 by pulsing bands with a solid contact line (§14).*
- **Sparkle** (all tiers): a tight highlight (power 2,400) off a fine, fast ripple layer
  (0.06 × `waveScale`, faded by the footprint), × the fluid's `sparkle`. The glint breaks into
  glitter on the ripples.
- **Data** (`shared/blocks/fluids.mjs`, validated, with every field required):
  - `caustics` became `{ strength, scale, depth }`.
  - New fields: `scattering`, `refraction` and `sparkle`.
  - The fluid buffer has six vec4 per fluid: scattering in `extra.w`, and `optics` = refraction,
    caustic scale, caustic depth, sparkle.
  - The frame uniform gained `wrap` (the eye's x and z wrapped, for the composite's caustics).
  - `dwell:water` (prototype content, tuned from the screenshots): absorption [1.0, 0.16, 0.045],
    scatter `#1496e0`, scattering 0.3, refraction 0.4, sparkle 1.5, foam depth 0.5, caustics
    strength 1.3, scale 1.4 m, depth 4 m.
  - The test `the_builds_water_grades_from_clear_through_turquoise_to_blue_never_green` pins the
    look over sand: green never more than 1.1× blue once the red has gone, and blue over 2× green
    when deep.
- **Distant water:** LOD water draws with the same pipeline over the LOD floor, so the colour
  grades the same way and near and far meet without a seam (`water-shore` `far-1000`). The faint
  dashed bands at sea-floor steps from W1b are gone, because the steps now show through as depth.
- **Bench:** the `water-shore` scenario goes to the `water-views` sea and its nearest beach. It shoots the beach edge
  at a low angle, straight down into the shallows, the deep-to-shallow gradient from 20 m, under
  the surface (the floor, along it, and up), and the far water from 1 km.
- **Cost** (Apple M1, 1280×720, 4× MSAA, `--unthrottled`). The machine was shared with other
  agents' builds and benches (load average 20–70), so these numbers are noisy. Runs under heavier
  load were up to twice these. The views were the shore from 20 m and under the surface in the
  shallows. The cleanest of four runs, GPU p50:

  | Tier | Frame, shore | Water half `gpu.near.water`, shore | Frame, under | Water half, under |
  |---|---|---|---|---|
  | Low | 3.4 ms | (none) | 2.2 ms | (none) |
  | Medium | 10.7 ms | 7.9 ms | 6.3 ms | 1.9 ms |
  | High | 11.5 ms | 5.4 ms | 11.5 ms | (noisy) |

  The frame interval stayed CPU-bound at 11–13 ms on every tier.
  - **Where the cost is:** with `water_scene_fs` returning a constant, the split and copy cost
    nothing measurable. The cost is the shading: caustics about 2 ms, and the rest in the
    three-layer ripples, the sky reflection and the scene reads over a nearly full-screen surface.
  - **The water halves** are timed on their own as `gpu.far.water` and `gpu.near.water`.
  - **Next:** W5's half-resolution copies on Medium, and a measurement on an unloaded machine and
    on phones (Low stays the phone default).
- **Not yet (W4):**
  - The surface from below is still the flat body colour.
  - No water line, no god rays (not brought forward: the caustics took the time).
  - Refraction to the scene above through Snell's window.

### W5a, reflections of land and clouds (2026-10-16; brought forward from W5)

The owner's report: "water doesn't reflect land and clouds." Until now the water reflected the analytic
sky alone. W5's screen-space reflections came forward, and so did the cloud deck in reflections.

- **Clouds in reflections** (`reflected_env`). The reflected ray is evaluated with the composite's
  own `dwell_cloud_deck` and `dwell_clouds`, and the same light (`cloud_lighting`), so day, night and
  dusk follow as they do in the sky.
  - The deck is seen from the surface point, not the eye: its height above that point, and the
    noise point under it.
  - Its footprint (`fwidth` of the deck point, which fades the octaves) is taken off the calm
    surface's mirror ray. Derivatives need uniform control flow, so it is computed at the top of
    the fragment (`reflected_deck_fw`).
  - The result goes through the inverse tone mapping, back into the scene's light, as the sky's
    reflection did.
  - The reflected deck never has more octaves than the sky's (the Clouds setting caps it), and none
    with the clouds Off.
- **Land: screen-space reflections** (`ssr_trace`). The tested twin is `render/reflection.rs`
  `trace`.
  - **The march.** The reflected ray is projected from the surface point to its reach (or to just in
    front of the eye, if it turns back) and clipped to the screen. It is marched in even steps on
    screen through the view depth (A1's `r32float`, sample 0), with 1/depth interpolated linearly
    (perspective-correct).
  - **A hit** is a step that lands behind the scene's surface where that surface lies within the
    step, give or take a thickness (1.5 m + 3 % of its depth). A ray that passes behind something
    much nearer keeps going.
  - Binary refine steps pin the hit between the last two steps. The hit's distance (for the fade
    below) is the scene's depth at the hit pixel, not the march's depth at the step's end: on a long
    reach the far kilometres squeeze into the last pixels of the ray, and the step's end can lie far
    beyond what it met (a mountain 12 km off read as 24 km and faded out, docs/BUGS.md 6).
  - **Water writes no depth**, so a ray over flat water can't hit the water or the floor seen
    through it: the ray stays in front of both (a test pins it).
  - **What it shows** (`ssr_reflected`): the under-water copy (W3's) at the hit, filtered, and fogged
    by the composite's height fog over the hit's distance. It is mixed over the sky and clouds by
    the hit's weight and coverage.
  - **The fades.** The weight fades within 12 % of the screen height of the top and bottom edges, so
    reflections don't pop there, and over the last 30 % of the reach. At the sides it doesn't fade
    (2026-10-26, docs/BUGS.md 6: fading into the sky drew a white haze down both sides). A miss
    is the sky and clouds. A ray leaving through a side marches on with its pixel held in the edge
    column (an edge clamp: the scene taken to go on beyond the edge as it is there) and meets only
    land, a scene point over the reflecting surface by `SIDE_LAND_M` (0.25 m), so a mountain running
    off the screen is reflected to the edge (0.28.1, docs/BUGS.md 9). The floor seen through open
    water lies under the surface and is never met: 0.27.1–0.27.4 took the scene at the exit pixel
    whatever it was, and over open water that drew streaks along the sides.
  - **Fresnel and the sun.** The reflection is Fresnel-weighted like the sky's (Schlick, F0 0.02), so
    a grazing view shows the shore strongly and a steep one hardly at all. Where the scene hides the
    sky, it hides the sun's and moon's glint and the sparkle too.
  - **Far and near.** In the far pass, water reflects the far scene only. In the near pass, it
    reflects both (the view depth and the copy hold both by then).
- **Data** (`shared/look/reflections.json` → `reflections.mjs` → `dwell-look` `reflections.gen.rs`,
  validated, with a CI stale check; prototype tuning):

  | Tier | March steps | Refine | Reach | Cloud octaves | Cloud self-shadow |
  |---|---|---|---|---|---|
  | Opaque (CPU) | 0 (must be: no copy) | 0 | — | 0 (the sky alone) | no |
  | Low | 0 (the bank band instead, §4) | 0 | — | 1 | no |
  | Medium | 12 | 8 | 24 km | 3 | no |

  (High, 28 steps, 48 km, 5 octaves with self-shadow, was folded into Medium in 0.50.29.)

  The values reach the shader in the frame uniform's `reflect` and `reflect_sky` (dwell-look
  `reflection_uniforms`). The thickness, edge fade, distance fade, `rippleScale` and `minReflectance` are
  shared by the tiers; `phone` overrides a tier's entries on a phone (Medium: 8 + 5 steps, 2 octaves;
  "Cost close to the water", below).
  - **`rippleScale` (0.3):** the land's ray is steered by a third of the ripples' slope. With the full
    slope, rays scattered from pixel to pixel and broke the treeline's reflection into dark speckle
    across the near water (seen in Chrome). The sky and clouds take the full ripple.
  - **The reach** was 600 m and 1.5 km at first. That missed the treeline across the lagoon, seen
    near the horizon. The reach costs nothing: the steps are spread over the ray's length on screen.
    It was 2 km and 4 km until 2026-10-26, when tall mountains farther off didn't show in the water
    (docs/BUGS.md 6); now 24 km and 48 km, with more refine steps (8 and 10, were 4 and 6) to pin far
    hits.
- **Cost switches:** `?waterdbg=ssr` and `?waterdbg=clouds` turn each part off (render/water.rs
  `DEBUG_SWITCHES`).
- **Tests:** `render::reflection`:
  - `a_reflected_ray_over_terrain_shows_the_scene_not_the_sky` (a bank across a lake is hit and its
    colour reflected). It failed against the sky-only reflection, then passed.
  - Misses reflect the sky, the water never reflects its own floor, rays toward the eye stay in
    front of it, and hits fade at the edge.
  - `render::tests::the_reflections_follow_the_water_quality_and_the_clouds_setting`.
  - `shaders::tests::the_water_reflects_land_from_the_scene_and_the_sky_with_its_clouds`, and naga
    validation.
  - `shared/look/reflections.test.mjs`.
- **Bench:** the `water-reflections` scenario (Medium) goes to the `water-views` sea. From 1.5 m
  over a lagoon it shoots a sand bank and the treeline behind it at a low angle (recorded, for the
  cost), turned and steeper. Then the beach across open water at several pitches and from the side
  (the edge fades), the clouds in the sea, and the bank at dusk and at night with the low sun over the
  water.
- **Cost** (Apple M1, 1280×720, `--unthrottled`, Medium, the bank view). The switches were toggled
  within one page (the bench's `waterDebug` step), three rounds, so the sides shared the machine's
  load. That load was very heavy (averages of 78–105 from other agents' builds), so this is an upper
  bound and not a check against the 1 ms budget:
  - The water half (`gpu.near.water`) p50 was 11.3 ms with both on and 8.1 ms with both off.
  - SSR alone was about 1.7 ms of that, and the reflected clouds the rest.
  - The frame p50 was 17.5 ms against 12.3 ms.

  The unloaded measurement is outstanding (the plan's W5a exit criteria).
- **Limits** (screen-space): what is off screen, or hidden behind something nearer on screen, isn't
  reflected (it fades to the sky).

### Cost: what the water spends and the fixes (2026-10-19)

Measured with `?waterdbg=` (render/water.rs `DEBUG_SWITCHES`, each name turns one piece off: `ripple0`–`ripple2`,
`glint`, `sky`, `refraction`, `absorption`, `caustics`, `foam`, `sparkle`, `under`, `flat`, `all`) and the
bench's `water` step, which switches tier and switches mid-run. One flight, then round-robin records at fixed
cameras, so a loaded machine's drift spreads over every configuration. Apple M1, 1280×720, 4× MSAA,
`--unthrottled`. Other agents shared the machine (load 20–130), so each figure is a pass's least GPU time over a
4 s record (telemetry's new `min`), the median of 3–6 rounds.

**Before** (W3), shore from 20 m, `gpu.frame`: Low 2.95 ms, Medium 7.35 (+4.4), High 8.15 (+5.2). Under the
surface: Low 1.60, Medium 5.03, High 6.12. The breakdown on Medium's shore view:

| Piece off | Saves | |
|---|---|---|
| Caustics (above the surface) | 2.8 ms | Worley 3×3 per layer, two layers near, an integer modulo and two trig calls per point |
| The three ripple layers | 1.3 ms | four hashed gradients per layer (modulo, sin and cos each) |
| Glint, sky, refraction, absorption, foam, sparkle | under 0.4 each | within noise |
| Under the surface: the composite's caustics | 2.7 ms | the same pattern over every floor pixel |
| The split pass and the scene copy | ~0 | `flat` (a constant colour) costs the same as Low |

**Fixes:**

1. **Cheaper hashing** (`noise_d`, `cell_edge`): the lattice wraps once per lookup and neighbours step without
   a modulo (`wrap_step`); gradients and the points' phases are unit vectors from the hash (`hash_dir`, one
   rsqrt), so a point's wander needs the time's sines once per call. F1/F2 are kept branchless. About −2.7 ms
   on the shore view, −3 ms under the surface (`water.rs` tests pin the wrap and the directions).
2. **Water tiles** (`render/water_tiles.rs`): gradient noise with derivatives over 32 cells at 16 texels per
   cell (512², drawn once), and per fluid this frame's two caustic layers over 16 cells at 32 texels per cell
   (512² `rg16float`, drawn each Medium or High frame with water, `gpu.water.tiles` ≈ 0.35 ms). Ripples,
   sparkle and foam sample the noise tile; caustics sample theirs where a pixel covers a texel or more and were
   evaluated per pixel nearer (blended between), so close up they stayed exact (since 2026-11-06 they sample
   the tile at every distance: "Cost close to the water", below). Periods round to whole tiles
   (`tile_cells`), so the wrap at 1,024 m stays exact; the ripples' largest layer repeats every 64 m, the
   caustics every 22 m, under the other layers' turned lattices. A further −1.7 ms on the shore view.
3. **Overdraw** was checked and is not a cost: chunk and LOD water don't overlap, water under the surface is
   culled against solid neighbours, and the water half draws one layer per pixel.

**After** (the same views, before W5a's reflections merged in): shore Low 2.39, Medium 4.02 (+1.6), High
4.11 (+1.7); under Low 1.55, Medium 3.59 (+2.0), High 3.61. With W5a's reflections merged (0.13.1): shore
Low 3.00, Medium 7.50 (+4.5), High 9.58 (+6.6); under Medium 3.73, High 3.78. The reflections (screen-space
marching and the reflected clouds) are now the largest cost on Medium and High, over the 2 and 3 ms targets:
their own budget is W5a's to tune.

**Stalls** (35–340 ms frames): the main thread blocked inside `draw.raster` while the GPU, shared with other
processes or behind on heavy frames, caught up; recording more commands into a backed-up GPU process waits
for it. They came in bursts on every tier (Low too) whenever another process loaded the GPU, and stopped
when it did. `render/pacing.rs` now keeps at most two frames in flight (`Queue::on_submitted_work_done`):
the app skips a draw while two are (`draw.skipped`), and gives up waiting after 1 s. Under the same load,
the W3 build's records showed long tasks to 557 ms; this build's frame interval stayed under 36 ms.

**Tiers** (then; since 0.50.29 see §4): Low (phones): one ripple layer, no scene reads. **Medium is now the
desktop default**: the whole look. High (folded into Medium in 0.50.29): Medium plus the fine caustic layer to
twice the distance. A CPU adapter (SwiftShader in CI) draws the opaque tier whatever is chosen (`water_for`, #104).

**Look:** the shore, edge and under-the-surface views were compared before and after: the same look, with
the caustic and ripple patterns in other places (new hashes and periods).

### Cost close to the water (2026-11-06)

Owner, 2026-11-06: the water ran fine far off but "starts chugging on mobile hard when you get close to the
water surface", where the floor shows through it. Measured on seed 0's river (the dam site), the eye 1.6 m
over the water, a 2880×1324 canvas on an Apple GPU, `gpu.frame` least of 2.5 s, two rounds:

| Looking 35° down, Medium | ms | Saves |
|---|---|---|
| Everything on | 33.4 | |
| Caustics off | 29.3 | 4.1 |
| Reflections of land (`ssr`) off | 28.0 | 5.4 |
| Reflected clouds off | (after the first two fixes) | 3.7 |
| Refraction, absorption or shoreline foam off | ~33.2 | ≤ 0.2 |
| The three ripple layers' last two off | | ~0.5 |
| `flat` (a constant colour: the split pass and the copy alone) | 13.8 | 19.6 |
| Low | 16.2 | |
| High | 39.7 | |

The GPU was the limit (the CPU's frame was ~2 ms). Refraction itself costs nothing; what costs is what
close-up water does per pixel. **Caustics** were evaluated per pixel wherever a pixel covered less than a
texel of their tile (two layers of Worley cells, 18 hashed points), which close to the water is every
floor pixel. **The land's reflection** marched every pixel's ray, and looking down nearly every ray goes to
the sky and takes all its steps, where the water reflects 2–4 %. **The reflected clouds** were drawn there
too. The flow's ripples and foam streaks (WATER_SURFACE_GRID.md §2c) cost nothing measurable in the water pass;
the streaks are thin dashes along the flow (`lengthM` at most 4.5 `widthM`, or they break into round
blobs), and the drifting flecks are ragged scraps of foam lit as the surface is, drawn in the flames'
pass (a few dozen small quads, none on a CPU adapter).

**Fixes:**

1. **Reflections only where they show:** the land is marched, and the clouds drawn in the reflected sky,
   only where Schlick's Fresnel at the pixel is over `ssr.minReflectance` (0.05, data in
   `reflections.json`), faded in up to twice it (`render/reflection.rs` `land_share`, WGSL
   `reflect_share`). Looking down at water past ~30° neither is drawn; under ~22° both are, in full.
2. **Caustics from the tile at every distance** (`water::CAUSTIC_EXACT_TEXELS` 0): magnified, a texel is
   1/32 of a cell (~4 cm for water) against lines ~13 cm wide; the close-up look was compared before and
   after and reads the same.
3. **A phone's tiers** (`reflections.json` `phone`, overrides of a tier's entries; the renderer's
   `set_phone`, from the shell's `prefersTouch`): a phone's Medium marches 8 + 5 steps and reflects two
   cloud octaves.

**After**, the same view: Medium 24.3 ms (a phone's Medium 22.7), High 27.5. Along the water at 4° down
(where reflections show and are kept): Medium 25.6 → 22.9, High → 26.9. The rest of Medium over Low is
the split pass and copy (`flat`) and the ripples and lighting per pixel; half-resolution water on phones
(W5's half-res copies) is the next step if a phone still needs it.

## 9. Shores: water meets the land (2026-10-20)

Owner, 2026-10-20: along shores the water "piles up" against the land and stands higher than it;
flat, unlit blue walls stand at the banks. Measured on `main` (1 m cells, adaptive shapes; 5 seeds,
8 coast windows of 128² each; `cargo run --release -p dwell-mesh --example shore_water`):

- The sea's top row is y = `SEA_LEVEL` − 1 = 31, drawn at 31.875 (`surfaceDrop` 0.125). 88–92 % of
  the natural top blocks in that row were **dry**, resting at 31.5, 0.375 m under the drawn water.
  945–1,687 per seed stood right beside drawn water. The generator filled water only into empty
  cells below sea level. The open half of a shaped top block below sea level stayed air, and the
  mesher floods a shaped block only when water is over it or in one of its 4 neighbours, so only
  the first row of beach was wet. (The LOD already counted those columns as sea: near and far
  disagreed.)
- Every water side face at the shores (1.8k–2.6k quads per seed) ran to the top of its cell, 32.0.
  That is above the drawn surface, and over the whole side, including behind the solid part of a
  shaped neighbour.
- Those faces turn their front toward the bank, so from the water side the eye sees their back.
  The water shader drew any back face as "seen from inside the fluid" (the W4 stub's flat body
  colour, nearly opaque): the flat blue walls.
- The adaptive rules' shore slopes were not at fault. Once that row is wet, the step slope from a
  wet top block (31.5) to the land above it (32.0) crosses the water at 31.875, mid-slope.

**The rule.**

1. **The sea fills every open space below sea level, including the open part of a shaped top
   block.** The generator (version 17) writes each top voxel below sea level as its block's
   `flooded=true` state. The natural blocks gained a `flooded` property for this. An adaptive
   block may carry that one property and no other (`shared/blocks/gen.mjs` rejects any other).
   A flooded natural block holds water only while it is a shaped top block
   (`dwell_blocks::adaptive::flooded_at`): buried, or shaped full, it has no open part. Edits
   follow the existing flooded rules: breaking it leaves water, and a natural block placed into
   water is flooded. Dry land therefore starts a row up, at or above 32.0, over the water at
   31.875. Water never stands above the dry land beside it. The surface drop stays as it was, so
   physics' whole-cell water and the eye test are unchanged.
2. **A fluid's side face covers only what can be seen.** It stops at the fluid's drawn surface,
   unless more of the fluid, a flooded cell or a solid is over it. Against a shaped neighbour, it
   covers only the part of the side above that neighbour's profile (`fluid_side` in
   `crates/dwell-mesh/src/mesher.rs`; a hanging neighbour counts as open). Water cells' sides
   still merge greedily, except against shaped neighbours.
3. **A side face's back seen from the air takes the outside look** (`seen_from_inside` in the
   water shader). Only a surface's back (a top or a bottom), or any back face while the eye is
   under a fluid, is drawn as seen from inside.
4. **(0.49.5) A surface's back is drawn only from inside the water** (`underside_from_outside`):
   seen from the air, through a stepped river's riser from below the upper pool's level, the
   upper pool's surface from underneath was a flat sheet of the body's colour (owner, 2026-11-05).
   From outside the water that underside isn't drawn; the riser shows the bed behind it.
5. **(0.49.20) A slope over a step is a closed wedge.** The water under a pool's sloped edge
   (`slope_over`) is closed at each end whose neighbour is open and pours no such slope, so rule 4
   never leaves a hole: from outside the water the eye meets an end's front before the slope's back
   (owner, 2026-11-06: triangle holes at a diagonal river's staircase steps; BUGS.md #39).
6. **(0.50.0) Leaves are flooded under water.** Owner, 2026-11-07: "underwater leaves z fighting,
   I think they should be 'flooded' so shouldn't get foam and stuff". Leaves were solid to the water
   and drew no water of their own, so the water beside them drew side faces on the leaves' own
   faces (z-fighting with them) and a surface round them. The leaf blocks have a `flooded` property
   now; a see-through full block with one (leaves, drawn cutout) holds water in its gaps as a ramp
   does in its open part: the water grid floods it while the water stands half a step into its
   cell and dries it when the water goes (`holds_water`, `water/cells.rs`), placement into water
   floods it (`flooded_for`), and the mesher draws its cell's water as the water beside it
   (`Kind::flooded`): no water face toward it, none on its faces inside the water body, the leaf
   drawn inside. Generated trees never stand leaves in water (`stamp_trees` writes over air and
   replaceable cells only), so worldgen is unchanged. Tests:
   `a_flooded_leaf_in_a_pond_draws_no_water_faces_round_it` (`mesher_tests.rs`) and
   `water_risen_over_leaves_floods_them_and_leaves_above_it_stay_dry` (`water/tests.rs`), both
   faces; both failed before.

Tests: `crates/dwell-worldgen/tests/terrain.rs`
(`the_sea_floods_the_open_part_of_every_natural_top_block_below_sea_level`,
`no_shore_column_has_drawn_water_higher_than_the_dry_land_beside_it`) and
`crates/dwell-mesh/src/mesher_tests.rs`
(`fluid_side_faces_stand_only_over_the_open_part_beside_them_and_under_the_fluids_surface`: every
fluid face is tagged as water, none stands above its surface, none against a neighbour's solid).
All of them failed on the code before this change. This changes the terrain a seed generates
(generator version 17, new goldens), so it ships on a new compatibility line.

## Sources

- GPU Gems 2 ch. 19, generic refraction: https://developer.nvidia.com/gpugems/gpugems2/part-ii-shading-lighting-and-shadows/chapter-19-generic-refraction-simulation
- GPU Gems 1 ch. 1, Gerstner waves: https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-1-effective-water-simulation-physical-models
- Tessendorf, FFT ocean: https://people.computing.clemson.edu/~jtessen/reports/papers_files/coursenotes2004.pdf
- Valve flow maps (SIGGRAPH 2010): https://advances.realtimerendering.com/s2010/
- Crest ocean and its underwater water line: https://github.com/wave-harmonic/crest · https://crest.readthedocs.io/en/stable/user/underwater.html
- McGuire & Mara, screen-space ray tracing: https://jcgt.org/published/0003/04/04/
- Stochastic SSR (Frostbite): https://www.ea.com/frostbite/news/stochastic-screen-space-reflections
- Looking through water (foreground bleed fix): https://catlikecoding.com/unity/tutorials/flow/looking-through-water/
- Complementary shaders: https://github.com/ComplementaryDevelopment/ComplementaryReimagined
- Evan Wallace's WebGL water (caustics): https://madebyevan.com/webgl-water/
- GPU Gems 3 ch. 13, volumetric light scattering: https://developer.nvidia.com/gpugems/gpugems3/part-ii-light-and-shadows/chapter-13-volumetric-light-scattering-post-process

## 10. River steps (2026-10-26)

A river's water steps down in ½ m terraces (generator 21). At a step, the upper reach's column (water
over water) stands beside the lower reach's last cell, whose surface is drawn `surfaceDrop` below its
top because air is over it. The face between two cells of the same fluid is normally culled. Here
that left an open strip from the lower surface up to the upper column's next cell, which showed as a
gap (docs/BUGS.md 4). Now, beside the same fluid whose drawn surface is lower, a cell draws its side
face over just that strip, from the neighbour's drawn surface up to its own (`fluid_side` takes a
bottom as well as a top). Test: dwell-mesh `a_river_step_down_closes_the_strip_between_the_two_surfaces`.

## 11. Waterfalls (2026-11-06, Phase 53 G6)

Water crossing a drop moves from the high column to the low one within a step of the grid, so nothing
was drawn where it falls (WATER_SURFACE_GRID.md §2e). The falls are found from the client's water cells
and current table (`dwell_world::falls`) and drawn in the flames' billboard pass, not as water blocks
(`render/waterfalls.rs`): a sheet along the lip, curved as the water is thrown out and falls, half see-
through by its flow, pale streaks running down it; a foam patch and spray puffs where it lands; lit as
the water's surface is (the sky, the sun and moon through the shadow maps), premultiplied, writing no
depth. Looks per liquid in `shared/look/waterfalls.json`. **Cost:** a few quads per fall (6 sheet
segments, the foam, at most `count` × width puffs), the nearest 32 within 64 m; the fragments are the
sheets' pixels, each a few noise taps. A CPU adapter draws them still and without spray. The
**Waterfalls** setting (Water section) turns them off.

## 12. The waterline and the clear underwater view (2026-11-07, 0.50.17)

Owner, 2026-11-07, with a photo of a camera half under clear tropical water (sky and beach above, a
sunlit sandy floor far off below, a crisp, slightly wavy line with a thin bright band between): "I want
a clean waterline like this: currently our fog shows up too early and we don't have a clean line."

**What was wrong.** Underwater was one flag for the whole screen: `frame.water.x` was 1 only when the
eye itself was under the drawn surface (`eye_fluid`), and the composite then fogged every pixel. With
the eye at the surface there was no line: just above it the view showed the surface edge-on (the sand
read dry, as if there were no water), just below it everything was fog. The fog was the fluid's
absorption plus scattering at full rate from the eye (`dwell:water`: about 1.3, 0.46 and 0.35 per
metre in red, green and blue), so the floor was gone within ~5 m.

**The waterline (per pixel).**

- `dwell_world::fluid::eye_fluid(eye, reach, voxel)` also finds the eye's fluid surface when the eye
  is **above** it by at most `reach` (`dwell_look::underwater::line_reach_m`: the largest
  `UnderwaterLook::reach_m` over the looks, the lens × `LENS_RAY_MAX` plus the line's waves and the
  band's depth, 0.50.20), with `under` saying which side the eye is on; face-local through
  `FaceFrame` (test: `an_eye_just_above_the_surface_finds_it_within_the_reach_for_the_waterline`, both
  faces).
- `frame.water.x` (`render/mod.rs` `waterline_state`): 1 when the eye is so deep that no pixel's lens
  reaches the surface (the whole view under: the old single branch, no per-pixel test), 2 near the
  surface with the eye under it, 3 near it with the eye above it.
- In states 2 and 3 the composite decides each pixel by its own ray: `waterline_m` is how far the
  pixel's point on the **lens plane** (`lensM` ahead of the eye along the camera's forward axis: the
  view ray at view depth 1 times `lensM`; until 0.50.20 the point `lensM` along the normalized ray, a
  sphere, below) is above the surface, less the line's waves (two sines over world x and z,
  `line.amplitudeM`, `wavelengthM`, `speed`; the drawn surface stays flat, §3, so the waves are the
  line's only). Negative: the underwater composite and the **band** (below); else the ordinary one.
  The CPU twin is `render/water.rs` `waterline_m` (tests:
  `the_waterline_splits_a_view_from_the_surface_by_each_pixels_ray`,
  `the_waterline_is_a_straight_line_at_any_eye_height_and_roll`,
  `the_waterline_moves_smoothly_with_the_eye_and_enters_without_a_pop`, both faces; the shader is
  pinned to its twins by `the_shader_decides_the_waterline_as_its_cpu_twin_does`).
- The fluid shaders (`water_fs`, `water_scene_fs`) discard the eye's own surface (within 5 cm of its
  height) on the far side of the line (`waterline_hides`): its top where the pixel looks out from under
  it, its underside where it looks out above it, so each half shows its own side. Only in states 2 and
  3 (a uniform branch); branch-free inside, so the derivatives after it stay in uniform control flow.
- `seen_from_inside`, `underside_from_outside` and the wind's pass key on `eye_under()` (states 1 and
  2), as they keyed on the flag before.

**The clearer fog.** `underwater()` applies the fluid's rates only past the look's `fog.clearM` and
scaled by `fog.density`; the registry's absorption and scattering are unchanged, so the water seen
from above is too. `dwell:water`: `clearM` 3, `density` 0.3 (blue reaches 1/e at about 13 m instead of
~3 m). A murky fluid is data: `density` above 1, `clearM` 0. The caustics on the floor were there
already (Medium and High) and now read far.

**Data** (`shared/look/underwater.json` → `dwell-look` `underwater.gen.rs` by
`shared/look/underwater.mjs`; validated with the bad entry named, a fixture murky fluid by data alone,
CI stale check): per fluid by its registry name, a `default` for the rest (`lensM` 0.05, no waves, no
band, a flat underside, the registry's rates from the eye: the look before). Packed into the fluid
table as five more vec4s (`FLUID_VEC4S` 14, 224 bytes a fluid; 12 and 192 until 0.50.20): `under`
(lens, clear distance, density, band depth), `line` (waves, the underside's texture), `band` (tint,
streaks), `rim` (rim px and shade, line px and glow), `band_extra` (glints). `dwell:water` (0.50.20):
`lensM` 0.4, waves 1.8 cm over 0.38 m at 0.3 Hz, the band 4.2 cm deep, tint `#e8f6ff`, streaks 0.45,
glints 0.35, a 3 px rim at ×0.45 over a 2.5 px line at ×1.6, the underside's texture 0.45. (Until
0.50.20: `lensM` 0.3, waves 1.2 cm over 0.9 m, and a thin `#f2fbff` meniscus 4 px either side of the
line, replaced by the band.)

**Cost** (Apple M1 Air, 1280×800 at ratio 2 = 2560×1600, `?renderscale=0.5&upscaler=stretch`, default
settings (High water), seed 0's nearest sea, `gpu.*` p50 over 4 s, one run before and two after). No
new pass or target: the test and fog live in the composite and the fluid shaders.

| View | `gpu.frame` before → after | `gpu.composite` | `gpu.near.water` |
|---|---|---|---|
| Eye at the surface (split) | 6.72 → 6.32 / 6.46 | 3.95 → 3.71 / 3.72 | 2.14 → 1.86 / 1.87 |
| 1.5 m deep along the floor | 6.12 → 7.71 / 6.86 | 4.20 → 5.56 / 4.96 | 3.17 → 3.87 / 3.82 |

At the surface the split costs nothing measurable (the above half is the ordinary composite; the
under half's fog replaces the scene's water). Deep, the code path is the old one plus two ALU ops a
pixel, yet the runs read 0.7–1.6 ms more; it was not isolated within the time box (the deep record
comes last in each run, on a passively cooled laptop) and is worth a recheck with `?waterdbg=`.
Per tier: Low and the CPU adapter draw the same (no caustics there, as before); the test is a
normalize and two sines per pixel only while the eye is within ~0.3 m of the surface, so phones pay
it only then. (0.50.20: no normalize, and the band's ~1 m reach; costs below.)

### 12.1 On the way down: a straight line, the band, the underside (2026-11-07, 0.50.20)

A shot series at one spot with the eye from +1 m to −1 m against the surface (the bench's descent;
`docs/proposals/WATER_GPU_SURFACE.md` §1.2) showed four faults, and the owner then asked for more of
the line: "The band isn't just a white line. It's supposed to be like an actual thing", "it's colored
based on the surroundings with what looks like a darker line over a lighter line. And the underside
of water has a texture". The reference was a stock photo of a camera half under a tropical sea (not
kept: watermarked): the waterline a thick, wavy slab of water seen edge-on, its top edge a wavy
silhouette against the island with a dark rim, under it a band some 5–8 % of the picture tall showing
the surface from just below (horizontal darker and lighter streaks, the underside totally reflecting,
bright glints), then the clear water; overhead the underside shows rippled light and dark cells.

**What was wrong, and the fix.**

1. *The line was an arc at ±0.1 m.* Each pixel tested the point `lensM` along its **normalized** ray:
   those points lie on a sphere round the eye, which the water's plane cuts in a circle. Now the point
   is on the **lens plane** (the view ray at view depth 1 times `lensM`), which the water's plane cuts
   in a straight line at any eye height, pitch and roll; `waterline_m` is linear on the screen.
2. *The line jumped between heights* (none at +0.25 m, a third of the screen at +0.1 m, most of it at
   −0.1 m). With the plane the line's row is linear in the eye's height (`row = (surface − eye) /
   (lensM · tan_y)`): `lensM` 0.4 puts a level view's line at the screen's edge with the eye 0.3 m
   from the surface at the default FOV, so the descent sweeps it bottom to top. Entering the split
   states must change no pixel: the reach is the furthest a lens-plane point can rise or fall,
   `lensM × LENS_RAY_MAX` (2.5: the screen corner's ray length at the widest FOV setting and aspect,
   pinned by `lens_ray_max_covers_every_field_of_view`), plus the waves and the band; the old reach
   (the lens plus the waves) let a steep view flip at once.
3. *The meniscus drew above the split, a white smear over the land.* Under the line the composite
   showed a ray rising from just under the surface through it to the land (the eye a centimetre
   under, the line above the horizon), so the visible colour edge was the horizon, not the line the
   meniscus sat on. Now a ray from under the surface reaching it outside Snell's window is totally
   reflected (fix 4), so the colour edge is the line; and the meniscus is replaced by the **band**.
4. *From below the underside was flat blue with a dark blotch* (the blotch also from above). The
   blotch was the player's own shadow (the body is drawn into the shadow maps in first person,
   DAY_NIGHT.md §10) on the surface's body colour: from below `from_below` lit the underside with the
   shadow, and from above the body took it at full strength. The underside is now lit unshadowed, and
   the body from above takes a shadow at `BODY_SHADOW` (0.35; 0 from 0.50.22, §12.2) of its strength (the glint and sparkle
   keep it all): the body is light scattered under the surface, which a shadow on it darkens only in
   part. **Snell's window**: `from_below`'s alpha is now the Fresnel (out through the window the
   composite's sky or the scene behind shows; outside it, alpha 1, the mirror), and the composite
   tests rays that reach the surface where it isn't drawn (`sees_out`: `1 − rise² < 1/ior²`, the
   critical angle 48.6° from the up for water, the rise bent by the noise tile's slope there): outside
   the window the water's in-scatter mirrored, inside it the sky. The **underside's texture**
   (`underside.texture`): `underside_cells` (`underside_look` from 0.50.22), the caustic cells on Medium and High (the noise tile on
   Low) at the surface point, so perspective-correct on the mesh and in the composite, faded where a
   pixel covers a cell.

**The band** (`waterline_band`; CPU twins `band_t`, `band_edge`, `band_uv`, test
`the_waterline_band_sits_under_the_line_with_a_dark_rim_over_a_bright_line`, both faces). The pixels
under the line within `band.depthM` on the lens plane (the line is its top edge: the surface; 2.1 cm, 3.5 % of the screen level at the default FOV since 0.50.23 — owner, 2026-11-07: "a little smoother … halve the width"; streaks 0.3, glints 0.15, breakup 0.35, mirror warp 10 px; until then 4.2 cm
is 7 % of the screen level at the default FOV). Its colour is its surroundings': the scene just above
the line mirrored across it (one load of the scene colour; the sky where nothing is drawn), at most
60 % of it, over the pixel's own underwater view toward the band's foot, times the look's `tint`.
Textured by the noise tile on a plane receding to the line (`band_uv`: along the line by the screen,
down it by `1/(t + 0.08)`, so cells shrink toward it) as horizontal darker and lighter streaks
(`streaks`) with glints (`glints`); along the top a dark rim (`rimPx`, `rimShade`) over a bright line
(`linePx`, `lineGlow`), in pixels across the line at any pitch and roll (`meniscus_px_m`); faded into
the underwater view over its lower half. The line's waves (1.8 cm over 0.38 m) give a few waves across
the screen, moving.

**The under path is cheaper.** A pixel under the line now takes only the underwater composite and the
band; before, the ordinary composite (sky, clouds, fog) was computed for it too and thrown away. The
composite's derivatives (the cloud decks' footprints, `composited_decks`) are taken first, in uniform
control flow, so the early return is valid WGSL. `composite_underwater` reads the depth and computes
the mirror or the sky behind only where the pixel needs them.

**Cost** (the M1 Air at the owner's setting, `?renderscale=0.5&upscaler=stretch`, 2560×1600, High
water, the descent scenario's records, `gpu.*` p50 over 4 s, one run each):

| View | `gpu.frame` before → after | `gpu.composite` | `gpu.near.water` |
|---|---|---|---|
| Eye at the surface (0 m, split) | 6.01 → 6.10 | 3.36 → 3.40 | 1.71 → 1.71 |
| 1 m under, level | 5.93 → 6.43 | 3.26 → 3.74 | 2.65 → 2.85 |

At the surface the band and the plane cost nothing measurable. At −1 m the eye is now inside the
band's reach (1.07 m), so the composite runs the per-pixel test, the cloud decks' prelude and the
mirror test where the old code took the deep path (+0.48 ms), and the underside's texture adds 0.2 ms
to the water (the caustic cells on the surface's underside). Deeper than ~1.1 m the composite is the
old deep path. Before the cheaper under path the band cost 1.9 ms at −1 m.

**Still off** (the after descent): the band's streaks read as smooth parallel stripes more than broken
ripples; its dark rim steps along the wave on a pixel's worth of noise; looking level from below, the
underside's mirror meets the fog at a hard horizon; from above at +0.5 m a faint trace of the player's
shadow remains (by design, at 35 %).

### 12.2 The underside and the band against the owner's references (2026-11-07, 0.50.22)

The owner gave three more stock photos (watermarked, not kept; described here). Two show the
surface's underside from below: irregular, broken bright cells (white-cyan, brightest in a patch
toward the sun) with darker blue gaps between them, the cells stretching and shrinking in perspective
toward the horizon and fading into a deep, even blue below it; a sand floor with rippled light. The
third is the waterline band: a wavy slab coloured by its surroundings, a darker line over a lighter
one, a textured underside. Measured against them, 0.50.20's "still off" list (§12.1) and the descent:

1. *The underside was a caustic web* (bright lines on a flat blue, cells the floor's size) *and met
   the fog at a hard horizon looking level.* `underside_look` (replacing `underside_cells`) now
   returns the cells and a sun patch. With `underside.breakup` the caustic web turns into **bright
   cells with dark gaps** (the web's lines become the gaps), some cells lit more than others by a
   slow noise over a few cells; `underside.cellScale` (a whole number, so the pattern still wraps)
   makes them smaller than the floor's (2 for water). The **sun patch** brightens the cells toward the
   sun's image through Snell's window (`sun_through`: the sun's direction refracted, its slant from
   the up shrunk by 1/ior, so a low sun shows at the window's rim; `underside.sunPatch`,
   `sunPower`), gone at night (`own`). **The horizon:** the composite's mirror was the in-scatter ×
   the cells at full strength to the last ray that reached the surface; it now goes through the
   fog over the distance to the surface (`underwater(…, to_surface)`), so the texture and the patch
   fade into the fog as the view levels (CPU twin `underside_contrast`, test
   `the_undersides_texture_fades_into_the_fog_toward_level`, both faces, red on the old: contrast 1
   at the horizon), and the cells' footprint uses the true slant (`max(rise, 0.02)`, was 0.15).
   **Hook:** the volumetric light shafts (a separate feature, being designed) should start from
   `underside_look`'s sun patch (the comment there), not from a second sun model.
2. *The band's streaks read as smooth parallel stripes* → `band.breakup`: the ripples warped by the
   noise's slope, shorter along the line, and coming and going under a second, finer noise.
3. *The dark rim stepped along the wave.* The pixels above the line took none of the rim and the
   first under it all of it (the per-pixel under/above test is binary), so its top edge was a
   one-pixel staircase. The pixel just above the line now takes the rim's shade by the share of it
   the line covers (`composite_pixel`'s `rim_above`; CPU twin `line_shade`, test
   `the_rims_top_edge_is_anti_aliased_across_the_line`, red on the old: a 0.55 step at the line).
4. *The mirrored land was literal.* The band's mirror is two taps of the scene above, shifted and
   blurred by the ripples (`band.mirrorWarpPx`, 24 px for water), at most `band.mirror` (0.45 for
   water; 0.6, the old fixed share, by default).
5. *The player's shadow from above at +0.5 m.* `BODY_SHADOW` is gone: the body takes no shadow (it is
   light scattered under the surface); the glint and sparkle still do.

Data (`shared/look/underwater.json`, defaults "as before"): `band.breakup`, `band.mirror`,
`band.mirrorWarpPx`, `underside.breakup`, `underside.sunPatch`, `underside.sunPower`,
`underside.cellScale`; packed into `band_extra.yzw` and a new `underside` vec4 (`FLUID_VEC4S` 15,
240 bytes a fluid). Every tier: Low takes the noise tile's cells with the same breakup, the CPU
adapter the same path (two more noise taps in the band's rows, one in the underside's).

**Cost** (the M1 Air, `?renderscale=0.5&upscaler=stretch`, 2560×1600, High water, the descent's
records, `gpu.*` p50 over 4 s, one run each; "before" is 0.50.20's run from §12.1, same scenario
and machine, an earlier session):

| View | `gpu.frame` before → after | `gpu.composite` | `gpu.near.water` |
|---|---|---|---|
| Eye at the surface (0 m, split) | 6.10 → 6.35 | 3.40 → 3.60 | 1.71 → 1.72 |
| 1 m under, level | 6.43 → 6.78 | 3.74 → 4.03 | 2.85 → 2.97 |

About 0.2–0.3 ms more in the composite (the band's extra noise taps and second mirror tap at the
surface; at −1 m the underside's look with the fog and the patch where the composite draws the
surface), and nothing measurable on the water mesh.

**Honest comparison with the references** (the after descent: `underside-after.png`, a 0 m band
crop): the horizon looking level is now a soft fade into the blue (fixed); the cells are smaller and
broken, cells with gaps rather than a web, but still read as a stylized cell pattern (rounded
polygons) more than the references' stretched, flowing ripples; the sun patch is weak at noon
(the sun overhead puts it inside Snell's window, where the window shows the sky). **Still off:**
Snell's window from below reads as a flat lavender sky with a hard wavy edge (it should read as a
bright white-cyan glow where the sun patch is); the band's texture is less regular but still reads as
fine horizontal lines near the line, where the receding plane compresses it; the band's rim is
smooth at the line but its streak texture just under it still shimmers.

### 12.3 Light shafts, Snell's window's glow, the band's blur (2026-11-07, 0.50.24)

The owner: "The glow rays is volumetric lighting which we want but don't have underwater or outside
the water yet either" (design: [ADR 0064](./adr/0064-light-shafts.md); data `shared/look/shafts.json`).
Under a fluid, `shafts_fs` marches each view ray (to what it shows, the surface, or `reachM`) and at
each point looks up the surface along the refracted sun (`sun_through`): lit where two drifting
noise layers of `cellM` cells are bright, faded over `fadeM` of depth and the reach. Parallel in the
world, the shafts converge on the sun's image on the screen and fan down from the bright surface as
the reference's do, flickering as the cells drift. **Snell's window** takes the fluid's `window` glow
(water: `#c8f4ff`, 0.4) over the sky through it, brightest at its middle, its rim softened into the
mirror over `edge` (0.18 of the window's rim) instead of a hard wavy edge; the **sun patch** is
stronger (`sunPatch` 3 → 4). The **band** (§12.1) is blurred along its rows, three taps 1.5 px apart
(`band_ripples`, `BAND_BLUR_PX`), after the owner's "looks pixelated. Blurring is fine."

Measured at the owner's setting (`?renderscale=0.5&upscaler=stretch`, M-series Mac, 2560 × 1600,
`gpu.frame` p95, Off → Low → High): in the air facing the sun 10.0 → 10.4 → 11.2 ms; under water at
−1 m level 7.0 → 7.7 → 7.0 ms and looking up 7.2 → 6.8 → 7.0 ms (within the run's noise). `gpu.shafts`
itself reads 3–6 ms on Apple's tile GPU, where the passes overlap: the frame's change is the cost.
Against the references: the underwater shafts read as thin soft rays fanning from the surface into
blue water, subtler than the photo's (no bright fog behind them); the window glows white-cyan.
**Still off:** in the air the shafts show as a soft glow round the sun unless trees, hills or a
cloud's edge are close to it on the screen (the radial blur needs an occluder near the light), and
the open water views in the bench have none; the clouds' gaps are found every sixth sample.

### 12.4 The band filtered at its source; the air shafts through trees (2026-11-07, 0.50.25)

**The band.** After 0.50.24's blur the owner still saw speckle: "a little smoother, it looks
pixelated. Blurring is fine." The cause was aliasing, not too little blur: the band's texture lies
on a plane receding to the line (`band_uv`'s `1 / (t + 0.08)`), so toward the line one pixel of the
drawing buffer spans several of the noise's cells. At the owner's 50 % render scale (an 800-pixel
buffer, the band ~28 px tall) its top row covered ~5.5 texture units a pixel across the line, its
streak layers ~5 cycles a pixel, ten times Nyquist. Now each of `band_ripples`' noise layers (the
warp, the streaks, the coming-and-going, the glints, the last counted at twice its frequency since
it is thresholded) is faded to its mean by `band_keep` where the pixel's footprint
(`band_footprint`: along the line `4 / height / (t + 0.08)`, across it the lens plane's metres a
pixel over the band's depth through the same `1 / (t + 0.08)`, computed analytically, so no
screen-space derivatives in non-uniform control flow) puts it past a quarter cycle a pixel, gone by
half a cycle. Near the line the band is its smooth mean colour, its ripples resolve lower down, at
any render scale and pitch; the three-tap blur along the rows (`BAND_BLUR_PX`) stays. CPU twins
`band_footprint`, `band_layers`, `band_keep`; test
`no_band_pattern_is_above_nyquist_where_the_plane_compresses_it` (both faces; red on 0.50.24 with
3.7 cycles a pixel kept). The 50 % Stretch upscale was not the cause: at 100 % the same layers
aliased, one octave further down the band. Cost: none measurable (the same taps, a few more ALU).

**The air shafts.** The bench found the sun through a tree's crown (seed 0, `goto leaves`, the
clock at 0.74 so the sun is low behind it, the camera under the crown looking up into it) and over
open hills with the clouds near the sun. Tuned for visible but soft rays (owner's choice: soft and
subtle): `air.sun` strength 0.5 → 0.6, decay 0.965 → 0.975 (the rays reach further from the gaps),
`discRadius` 0.55 → 0.7 (the glow round the sun wider and softer). In 0.50.27 (owner, 2026-11-07: "I don't feel like the volumetric light rays are very obvious from the sun (should be moon too). I'd like them to be quite a bit stronger.") `air.sun` strength 0.6 → 1.4, reach 0.85 → 0.95, decay 0.975 → 0.985; `air.moon` strength 0.06 → 0.35, reach 0.7 → 0.9, decay 0.96 → 0.98, `discRadius` 0.3 → 0.45. The crown's gaps glow and
streak faintly toward the camera; over open sky the glow is a soft halo, brighter in Low (its
quarter-scale buffer blurs wider). Cost at the owner's setting (`gpu.frame` p95, Off → Low → High):
through the tree 10.45 → 10.82 → 11.97 ms (+1.5, of which ~1 ms a shadow-cascade spike in that run;
`gpu.shafts` p95 2.7 / 3.7 on Apple's overlapping passes), the cloudy sky 11.30 → 11.63 → 12.53 ms
(+1.2), as measured in 0.50.24.

### 12.5 Torch and lamp shafts (2026-11-07, 0.50.26)

The third part of ADR 0064: the shafts pass also draws the nearest placed and held lights' glow
(`shafts.json` `lights`; docs/LIGHTS.md §4), in the buffer's rgb (now `rgba16float`; the sun's,
moon's or fluid's glow moved to its alpha), each light a radial blur toward its screen point over
the view depth behind the flame, weighed by its halo. In the air only: under a fluid the lights
draw none yet. It deviates from the ADR's plan (a march against the torch shadow cube maps, High
only): see the ADR's Decision for why.

Cost at the owner's setting (`?renderscale=0.5&upscaler=stretch`, M-series Mac, 2560 × 1600,
`gpu.frame` p95, Off → Low → High; the bench's `torch-night` and `torch-cave` scenarios, kept in the
PR's scratch file): five torches on open ground at midnight, seen from ~6 m, 11.5 → 11.7 → 12.2 ms;
a torch placed in the torch-cave room before the stone pillar 10.6 → 11.3 → 11.3 ms. `gpu.shafts`
reads 3.6–5.2 ms on Apple's tile GPU, where the passes overlap: the frame's change (+0.2–0.7 ms) is
the cost. **How it looks:** a soft warm halo round each flame, wider and brighter by night, fainter
on Low; in the cave a faint glow round the torch. No distinct rays in these views: nothing stands
between the eye and the flames there; the rays show only past an occluder in front of a light (a
post, a doorway, leaves). A first build sized the halo by `discM` alone, and a torch 2 m away hazed
the whole view (the screen blend lifts the black corners of a cave); it is capped at `maxDisc`
(half a screen height), and its decay is per twelfth of the way, since Low's fewer samples read
brighter than High's when it was per sample.

## 13. Water looks per biome (2026-11-10, ADR 0074 B4)

A biome may name its water's colour and clarity (`waterLook`: `scatter`, `absorption`) over its fluid's
look in the `fluids` table; `dwell_worldgen::terrain::water_look_at` resolves the look per column,
blended by the biomes' weights, from the build's biome data (clients generate terrain from the same
data, so nothing is sent). The default biome has none: today's water exactly. Not yet drawn: the water
shader still takes one look per fluid; feeding it the per-column look (a vertex tint or a look texture)
is outstanding. Chisel's preview tints its water by it.

## 14. River step seams and shore foam as pulsing bands (2026-11-10, 0.52.6)

**Step seams.** Owner, 2026-11-10: "Top of water slopes from up river looking down, have some 'gap'
artifacts": dashed see-through hairlines along a river's ½ m water steps. The step was watertight in
exact arithmetic (`a_water_step_is_watertight_seen_from_upstream`), but the tops of the pools were
greedy-merged into long rectangles while each pour slope (§10, `slope_over`) and each riser between
two fills is one cell wide, so along the step the slopes' and risers' corners fell inside the tops'
long edges (T-junctions), and the rasterizer left a crack at each where the two surfaces meet at an
angle. Now a liquid top within two columns of a step in the same water (a neighbour in its row drawn
at another height, or an open cell over the same water below it; `mesher.rs` `top_on_step`) is drawn
cell by cell (`ALONE_BIT` in the merge mask), so every crease's vertices are shared. Test:
`a_water_step_has_no_t_junctions_along_its_creases` (both faces; red 161 T-junctions, green none).
Tops away from steps still merge. A step that falls on a chunk's border can still have T-junctions
against the neighbour chunk's merged top (the mesher sees one column past its chunk).

**Shore foam as pulsing bands.** Owner, 2026-11-10: the threshold-noise foam (W3's lapping lines
broken up by noise) left gaps; "It should be more like pulsing bands I think". `foam_amount` now takes
the distance to the shore: the depth under the surface over a slope, or the view ray's path through
the water where that is shorter (a wall or a block in the water facing the eye; no extra pass or
texture, the ray thickness the absorption already uses). It draws a solid contact line
(`shore.lineM`) where the water meets land or a block, and `shore.bands` soft bands parallel to the
shore, each born at the edge and rolling out to the fluid's foam depth (block registry `foam.depth`)
over `shore.periodS` while it thins (from `shore.widthM`) and fades, a new one forming as the last
goes. Slow noise shifts each band's timing and width along the shore by up to `shore.wobble`; it never
multiplies the foam, so it cuts no holes. All in `shared/look/waterflow.json` per liquid (`shore`,
validated, the fixture oil adds one by data), packed into the fluid table (`flow_shape.zw`, `shore`;
16 vec4 a fluid). The CPU adapter's tier (and Low) reads no scene, so it draws no shore foam, the
cheapest form. Limits: the wall distance is view-dependent (the far side of a block, seen past it, has
the bed's depth, not the block's distance), so foam behind a block shows where the depth is shallow.
Screenshots: the river banks at seed 7's nearest river from the spawn (bench `goto` river, 2.5 m up,
pitch −25): a solid line along each bank and two thinner bands beside it, no gaps.


## 15. The sun and moon on the water (2026-11-10, 0.52.32)

The owner's question: "why doesn't the moon reflect on the water?" The water reflected `dwell_sky`
(the gradient and the bodies' halos) and lit a GGX glint by the sun's and moon's *light* colours; the
moon's light is dim and its disc was never in the reflected sky, so at night nothing marked where it
was. Now, on a tier with `bodyPath` (`shared/look/reflections.json` `tiers`; Low and Medium):

- **The discs mirrored** (`reflected_bodies`): `dwell_moon` (its phase, maria and earthshine) and
  `dwell_sun_disc` along the reflected ray, under the reflected cloud deck (so clouds hide them), with
  a two-pixel edge. On calm water this is the mirror image; ripples break it up.
- **A glitter path** (`body_paths`): a GGX lobe of each disc's own light (`MOON_COLOR` × the lit
  share `day.stars.w`, the sun's disc colour) off the rippled normal, by `bodies.<body>.pathAlpha`
  widened by the ripples' lost slope variance (Toksvig, as the glint): a streak toward the viewer on
  the line under the body, brighter and tighter on calm water. `strength` scales it; `fullBelowDeg`
  and `goneAboveDeg` fade it with the body's elevation over the fragment's face's horizon (the sun's
  only at low angles: a sunset path; the moon's always). The cloud cover takes `cloudFade` of it
  while the Clouds setting draws clouds (the deck's coverage, `cloud_cfg.x`; not the deck along the
  body's own ray, which would cost a deck evaluation per pixel).
- **The CPU adapter's tier** (opaque) keeps the simple highlight only (the GGX glint);
  `reflections.mjs` fails if its `bodyPath` is set.
- **Both faces:** the path uses `face_rise(s.up, …)` and `own(…, s.up)` of the fragment's face, the
  discs the eye's face's reflected sky, as the sky reflection does.
- **Uniform:** `reflect_sky.y` is now bits (bit 0 the deck's self-shadow, bit 1 the path;
  `reflect_bits`); the tunables are WGSL constants (`SUN_PATH`, `MOON_PATH`, `PATH_CLOUD_FADE`,
  `render/glitter.rs` `wgsl`).
- **Tests:** `render::glitter` (both faces): a body at elevation e is mirrored at −e and the mirror
  ray reflects to the body; the path's peak over a calm surface lies along the viewer–body azimuth and
  runs further along it than across; calm water's path is brighter than rough's. `dwell-look`
  `reflections`: the CPU tier draws no path, the bits stay apart, the sun's path is a low sun's.
  `reflections.test.mjs`: bad `bodies` entries fail with their path. `render::shaders`: the gates and
  the discs under the clouds.
- **Seen (bench, M1, Medium, seed 7's sea; full moon):** the clock maps time t to face A's solar angle
  (`DayNight::solar_deg`, the sun's elevation `sin(angle) × sin 60°`, the moon opposite), so a full
  moon 15° up is t 0.8689 (yaw 40°), 8° up t 0.8525, and the sun 5° up t 0.8208 (yaw 208°). The
  camera must be aimed by `goto`'s `yaw`: a `look` step after a `goto` doesn't turn the placed camera.
  The moon's path runs from under the disc toward the viewer, broken into glints by the ripples; at
  strength 0.25 it was a solid bar near the horizon, so 0.15. The sunset's path (0.05) reads as a warm
  column under the sun. GPU frame 7.06 ms mean over the moonlit sea (not compared against main;
  outstanding). No lake within 700 m of the seed-7 spawn, so no lake view.

## 16. The surface from below: Snell's window onto the real sky (2026-11-11, 0.52.49)

Owner, 2026-11-11: "We need to make underwater surface layer look transparent like the surface and
look more accurate to what light is shining down." Before, the window overhead (§12.2–12.3) showed the
sky along the ray *under* water (not bent out), mixed 0.4 toward a fixed white-cyan glow (shafts.json
`window`) whatever the hour, and multiplied by the underside's full cells: a pale textured sheet,
as bright at midnight as at noon.

- **The sky refracted out (Snell's law).** In the composite (`composite_underwater`), a ray inside the
  window is bent out through the rippled surface (`water_exit`: its slant from the normal grown by the
  IOR, sin θ_air = n sin θ_water, so the whole sky hemisphere is squeezed into the ~97° cone,
  2 × asin(1/n) = 2 × 48.6°; the rim shows the horizon), and the sky is looked up along that ray. The
  normal is the face's up tilted by the ripples' slope (`face_up`, both faces).
- **The sun and moon discs** show through it where they truly are (`dwell_sun_disc`, `dwell_moon` in
  its phase), on a water tier with `bodyPath` (reflections.json; not the CPU's), at the look's
  `window.bodies` strength.
- **Brightness by the real light.** The window's glow (shafts.json `window`) is scaled by the sky's own
  light straight up (its brightest channel), so it is bright at noon, coloured at dusk and dim at
  night; the sun patch (§12.2) still follows the sun's refracted image and is gone at night.
- **Clear, not a sheet.** The underside's cells darken and lighten the window's sky only by the look's
  `window.cells` share (water 0.3; the mirror outside keeps them whole).
- **Outside the window**, total internal reflection as before (§12): the water's in-scatter, textured;
  the floor is not reflected (no copy of it is at hand in the composite).
- **Light shafts** already follow the real sun (ADR 0064, §12.3); unchanged.
- **CPU tier:** no discs; the surface mesh from below is `from_below`'s see-through tinted body by the
  Fresnel (the cheapest), the window's refracted sky in the composite.
- **Data:** `shared/look/underwater.json` per fluid `window: { cells (0–1), bodies (0–4) }`, validated
  by `underwater.mjs` (a bad or missing entry fails), packed as the fluid's 17th vec4 (`Fluid.window`).
- **Tests:** `render::water` `snells_window_is_a_97_degree_cone_and_its_radius_grows_with_depth`
  (`critical_angle`, `window_radius`: 2.27 m at 2 m deep), `the_sky_through_snells_window_is_refracted_out_by_snells_law`
  (both faces: `water_exit`'s angle against Snell's law, the rim at the horizon, outside it none),
  `the_window_shows_the_real_sky_and_bodies_scaled_by_its_light` (the shader's gates, the CPU tier).
- **Seen (bench, M1, Medium, seed 7's sea 2 m under, scratch scenario `underwater-up`):** midday t 0.5
  pitch 80: the window is a clear bright sky with the sun's disc in it and faint cells (main: a white
  sheet of strong cells); sunset t 0.8208 yaw 208 pitch 42: the window is the bright sky, the mirror
  around it; the sun disc was not caught in frame; night t 0.8689 (full moon) yaw 40 pitch 44: the
  window is a dim night blue (main: a bright white-cyan window at night); the moon disc was not caught
  in frame. GPU (HUD) the same as main within noise: midday 3.4 / 3.4 ms, night 6.0 / 6.0 ms.
  Outstanding: the owner's look; the discs aimed in frame at sunset and night; a lake view.

### 16.1 Underwater at night: the window is the sky above (2026-11-11, 0.52.53)

Owner, 2026-11-11: "Check the underwater update at night, and you should be looking from just outside
the water up compared to just under the water up, because that's the mismatch." Benched over seed 7's
sea (M1, Medium, clouds High), 0.3 m and 2 m above and below one spot, straight up and toward the moon
(15° up, t 0.8689, yaw 40), at midnight (full moon), sunset (t 0.8208, yaw 208) and midday; scratch
scenarios in the session's scratchpad (`underwater-night`, `underwater-deep`). Differences found on
0.52.52 (below vs above):

- **No stars, no clouds, a different moon.** The window looked up `dwell_sky` (the gradient and halos)
  and drew its own moon and sun discs (bodies × 1.5, only on a `bodyPath` tier); the sky pass draws
  `dwell_sky_bodies` (stars, moon, sun) under the clouds. 2 m under at midnight the window was a plain
  blue with cells, no stars or clouds.
- **A white-cyan wash.** The window's glow (shafts.json `window`, #c8f4ff at 0.4) was mixed over the sky:
  straight up at midnight the window's red channel was 2.3 × the sky's (the new test's red run).
- **A sun-patch wash.** The underside's sun patch (in-scatter × the patch) was added inside the window
  too, a cyan haze round the sun's image at midday.
- **A glowing blue sea at night.** The water seen from inside (the mirror outside the window, the floor)
  kept full saturation, while the scene above takes the moonlight's grey (`scene_display`): 2 m under at
  midnight the view averaged sRGB (1, 42, 130) against the sky's (16, 22, 45).
- **Cells.** The underside's cells darkened and lightened the window's sky at 0.3.
- **Not a difference:** the fog. Its clear distance (3 m) covers the window's whole cone from 0.3 m (the
  rim's ray is 0.45 m long); at 2 m only the rim's rays pass it.
- **Not changed:** 0.3 m under, the waterline's lens plane (§12, `lensM` 0.4) draws the view above the
  line as from the air, so looking up from just under shows the window only near the line; and the
  light shafts (ADR 0064) still glow over the view.

Fixed:

- **One sky.** `sky_above_with_clouds(dir, clear, deck, fw, bodies)` is the sky above the horizon for the
  sky pass (`sky_behind_with_clouds`, bodies 1) and for the window (`window_sky`, along `water_exit`'s
  ray, the clouds' footprint from the next pixel's bent ray): the stars, the moon in its phase, the sun
  and the clouds, on every tier (the CPU's too, as its sky pass). `window.bodies` scales the bodies
  over the gradient (water 1: as above).
- **No glow over it.** shafts.json water `window.glow` 0; the sun patch stays on the mirror only.
- **Moonlit water.** `moonlit` gives the mirror and what is seen under the surface the time of day's
  saturation (`frame.day.horizon.a`), as `scene_display` does above; the window's sky keeps its colour.
- **Fewer cells.** underwater.json water `window.cells` 0.1.

Seen after (pairs above | below): 2 m under at midnight the window is the starry night sky with the
clouds, squeezed into the cone, inside a dark navy mirror (mean (11, 17, 40) vs the sky's (16, 22, 45));
at midday a clear blue sky with the clouds and the small refracted sun disc (main: a pale white-cyan
sheet); at sunset the warm clouds at the rim. 0.3 m under the strip under the line is a grey-navy, not
bright blue. GPU (bench `gpu.frame` p50, 2 m under looking up): night 5.37 ms vs main 5.35, midday
5.03 vs 4.72 (the window's clouds). Tests: `render::water`
`looking_up_through_the_window_shows_the_sky_seen_from_just_above` (both faces, midnight, the moon 15° up
and midday: the window's sky straight up 0.3 m under within 2 % of the sky pass's straight up, through
the `window_sky` twin; red on 0.52.52's data, green after) and
`the_window_shows_the_real_sky_and_bodies_scaled_by_its_light` (the one sky function, `moonlit`).
Outstanding: the owner's look; the lens plane's air view 0.3 m under; the mirror's brightness at sunset.

## 17. Lava's look (2026-11-11, VOLCANOES.md §6.3)

Lava is not drawn by the water's surface pass: `dwell:lava` cells are opaque blocks (`look: cube`, no
collision, `emits` 13) meshed with the terrain, so the look is a painterly tile and one shader branch,
both data-driven:

- **The tile** (`shared/textures/materials.json` `lava_crust`, the palette's `lava` ramp): dark crust
  plates (`slab`s in the ramp's shadow) over a bright molten ground with brighter dabs between them, the
  cracks. `emissive: 1` makes it self-lit (it gives off its colour whatever the light, so it reads at
  night); its `emits` 13 lights the ground round it through the flood light, as a torch's block does.
- **Molten** (`"molten": true` on a material, checked by `paint.mjs`: it needs `emissive` > 0): the
  generator emits `PAINTERLY_MOLTEN`, and `dwell-mesh` `tile_glow_entry` stores such a tile's glow
  **negated** in the tile table (`tile_grids`' z), so the terrain shader finds it from the tile with
  nothing added to the vertex. `terrain_fs` then stretches the tile over `MOLTEN_SPAN` (3) blocks, so the crust plates are metres across (at one block a tile the first in-game shots read as a checkerboard), drifts it by `MOLTEN_DRIFT` (blocks a
  second, slow) and multiplies the glow of its bright texels (the cracks, not the crust plates) by
  `molten_heat`: two slow world-space waves, 0.7–1.4 ×.
- **The CPU tier** draws it flat: the water tier's bit 10 (`dwell-look` `tier_bits`, every tier but the
  CPU's; WGSL `water_tier_molten`) gates the drift and heat, so the CPU adapter shows the still glowing
  tile (tested: `only_the_cpu_tier_draws_molten_surfaces_still`).
- **Not built yet:** heat per cell (brighter where hotter or deeper: the field's heat is not in the chunk
  cells), and lava as a translucent fluid surface (it stays opaque by design: no transparency).

### 17.1 The crust plates (round 3, 2026-11-11)

Up close the tile (even over 3 blocks) read blocky and patchy, its cracks hatch strokes. On every tier
but the CPU's the molten branch of `terrain_fs` now draws **crust plates in world metres**,
continuous across blocks: a cellular (Voronoi) pattern, `molten_plates`, plates `MOLTEN_CELL` (1.7 m)
across whose seams (the second-nearest centre's distance less the nearest's, under `MOLTEN_SEAM`) glow.
A top face takes the world's x z, a side its along and y. The centres wander slowly and the plane is
warped a little, so the seams bend and the crust flows. The plates' colour is the painted tile darkened
(each plate a little apart), the seams the tile's hue lifted toward yellow-white; the glow (`emissive`)
is a quarter on the crust and up to 1.6 × `molten_heat` on the seams. The CPU tier keeps the still
painted tile. The pattern's constants are shader constants for now (a look file when it is tuned).

### 17.1a Heat stops (lava glow, 2026-11-11)

Owner, 2026-11-11: "Lava looks dark red instead of bright red/orange" / "It should darken as it
solidifies though." Each molten cell is a `heat` state of `dwell:lava` (VOLCANOES.md §6.5), carried to the
shader in its faces' vertex colour; the molten branch reads that stop's row (`lava_stop_look`,
`lava_stop_core`, generated from `shared/fluids/lava.json` `heat`): the seams in the stop's colour with its
core at their middle, at its emissive, seams widening as the crust thins
(`MOLTEN_SEAM + MOLTEN_OPEN × (1 − crust)`), the crust plates darker the more they cover. Lava glows
through `emitted_hue` (the tone curve on the brightest channel, the hue kept). The bloom pass is not built.

### 17.2 Steam and burning (round 3)

- **Steam's look** is data: `shared/fluids/lava.json` `steam` (colour, life, rise, size, opacity;
  `lava.mjs` checks it and emits `DATA.steam`). Its puffs are smoke puffs of their own look
  (`render/blasts.rs` `steam_look`, the smoke table's last entry). Each `Steam` message plays
  `steam_hiss` (shared/sounds, procedural: no fitting sample in the pack) at its first cell.
  Fixed on the way: as first shipped, `Steam` went on the control channel, which the client's session
  does not pass to the game, so no puff ever showed (the bench's `__dwell.lava()` report counted
  thousands of `steamCells` sent); it now goes on the world channel and the session routes it (its test
  red, then green).
- **Burning:** a `Burn` damage hit shows a red-orange screen edge (`burnEdge`: colour, opacity,
  seconds; the HUD's `#burn-edge`, `game.rs` `burn_edge_strength`) and plays `sizzle`.

## 18. River steps as one surface: step cells (2026-11-12, BUGS.md #92)

Owner, 2026-11-12: "I'm seeing some edge triangles and thin lines at slope transitions on downhill water." A held
river's levels step down ½ m (a cell) at a time, in diagonal bands where it crosses the grid on the diagonal. §10's
pour slopes were drawn one per side of a pool's edge cell (`slope_over`), so on a diagonal staircase an open cell
with the upper reach on two sides held two slopes crossing, one touching it only at a corner held none (the slopes
beside ended in vertical wedge triangles), and the lower pool's top stayed under every slope.

Now the water over a step is a **step cell** (`mesher.rs` `step_cell`, `step_water`): an open cell over a pool's
water (a liquid, or a flooded cell whose water stands in it) that touches, by a side or a corner, a column of the same
water one row higher whose top shows (0.05–1.5 cells above the pool's surface, as §10's slopes). It draws one surface:
each corner at the highest such upper level among the three other cells round that corner, else at the pool's
surface. A corner's height depends only on the cells round it, so step cells, the upper tops and the lower tops all
meet at the same vertices (no cracks, no T-junctions); the surface splits across the diagonal that leaves a lone
corner alone, so a diagonal band's two step cells make one plane. The pool's top under a step cell, the upper
column's side toward it and the strip below it (§10) are not drawn: they are inside the water. Its sides are drawn
only against open air that holds no such water (a step at a bank, from the pool's surface up to the slope). Calm pools
and falls are unchanged. Test: `a_diagonal_staircase_river_is_one_continuous_surface` (both faces).

## 19. Underwater toward the ABZÛ reference (2026-11-12)

Owner, 2026-11-12, with a screenshot from ABZÛ: "Underwater reference photo from the game abzu for pretty underwater
with transparent surface, light rays etc.", then "This is more what I want from the underwater surface." What the
reference has: bright saturated turquoise water with a long clear view, a bright see-through surface from below (the
sky in Snell's window, the sun a blown-out disc with light radiating from it, the ripples lit from above, a soft glow
round the window), strong shafts fanning from the surface, caustics on a colourful, well-lit floor.

What was off (bench, M1, High, seed 7, midday; `before/` in the PR): the ceiling outside the window was a darker blue
mirror crossed by a strong dark web of cells, the window a plain sky with no sun glow, the fog a mid blue that closed
in at ~3 m clear, the sand under 4 m of water a uniform blue-green, the shafts faded out by ~14 m.

**Data (all defaults "as before"; `shared/look/underwater.json`, validated by `underwater.mjs`, packed as the fluid's
17th–19th vec4s, `FLUID_VEC4S` 19):**

- `window.sunGlow`, `window.sunPower`: a glow of the sun's light round the sun's refracted image in the window
  (`sun_through`), added before the fog, so the window's sun reads as a blown-out white disc with light radiating
  from it; gone at night with the sun (`own`). Water: 3, power 24.
- `window.rimGlow`: a soft glow of the sky's light straight up round the window's rim, spilling into the mirror
  (from 55 % of the critical cosine to the rim). Water: 0.5.
- `tone`: the colour grade. `fogColor`/`fogShare` pull the fog's in-scatter toward a colour (water: `#16b2d4` by
  0.55, a turquoise; the fluid's own scatter still colours the water from above), `brightness` its brightness
  (1.12), `mirror` the mirror outside the window brighter (1.25: lit water, not a dark lid), `floorLight` the share of
  the absorption the light down to the floor takes (0.35: sand keeps its warm colour under a few metres).
- `fog.clearM` 3 → 6, `density` 0.3 → 0.22 (a longer clear view); `underside.texture` 0.6 → 0.45 and `breakup`
  1 → 0.6 (lighter, more line-like ripple highlights, fewer dark gaps), `sunPatch` 4 → 5.
- `shafts.json` water: strength 2.0 → 2.6, `reachM` 18 → 24, `fadeM` 14 → 24 (the rays reach the deep floor). The
  same samples a pixel, so no cost.

**Shader:** `underwater()` takes the grade; `composite_underwater` adds the sun glow, the rim glow, the mirror's lift
and the floor light's share. Two fixes found on the way: a pixel showing the surface's own mesh (within
`SURFACE_SLACK_M` 0.3 m of the surface along the face's up) takes the composite's surface; and near the window's rim,
what the pixel shows past the surface fades into the mirror by the window's share, as the sky does: with the
brighter mirror, the mesh's opaque (Fresnel) body there showed as hard-edged flat blue blobs. Both faces: the
composite works in `face_up`/`face_y`, the sun glow uses the eye's face's sun.

**Tiers:** the grade, the glows and the floor light are a few ALU ops in the composite on every tier (Low and the
CPU's included; the CPU tier has no discs, but the glow is the sun's light, not its disc). Nothing new is sampled.

**Tests:** `underwater.test.mjs` (the new entries validate, a bad one fails by name, a fixture fluid's grade by data
alone, the default as before, water's direction pinned); `render::water`
`each_fluid_packs_its_underwater_look_and_one_without_takes_the_default` (the new vec4s),
`the_underwater_grade_is_data_and_a_fluid_without_a_look_is_as_before`,
`the_surface_mesh_from_below_takes_the_composites_surface`; the WGSL validates with naga.

**Seen (bench, M1, High, seed 7, midday; a scratch scenario: 4 m under a sea 7 m deep looking up 35°, across 5° and
straight up; 14 m under an 18 m sea up 40° and level; the 1.5 m shallows looking up 20°):** the water is a brighter
turquoise and reads farther; the ceiling is light water with soft ripple highlights rather than a dark web; the
window glows white round the sun's image with the sky and clouds in it; the sand under 4 m keeps a warm green-gold
with its caustics; the shafts reach the deep floor. **Cost** (`gpu.frame` mean / p95 over 5 s, main → this):
4 m under looking across 6.74 / 9.45 → 6.73 / 9.83 ms; looking straight up 5.82 / 8.86 → 6.18 / 9.08 ms
(`gpu.composite` 4.02 → 4.30 ms: the rim glow's zenith look-up on every mirror pixel). **Still off against the
reference:** the sun's glow is a broad white haze rather than a sharp blown-out disc with radiating spokes; the
ripple highlights are fainter than the reference's bright cells; the floor is still the prototype sand, not a
colourful reef; in the 1.5 m shallows a soft blue patch of the surface's mesh still shows near the window's rim.
Outstanding: the owner's look; both faces checked in code and tests, not on screen.

## 20. The shore foam's soft edge (2026-11-13)

Owner, 2026-11-13: "The water foam edge is a little too hard now, the softness of the old version was better and the
consistency of the new one along the edge is better. Combine those concepts." §14's contact line was solid to
`lineM` (0.06 m) and gone 3 cm later: a hard line. The rim before it faded over ~0.2 m of depth but was multiplied by
noise, so it broke into blotches along the shore. The contact line now keeps §14's continuity (noise only nudges its
core's width, never multiplies it) and feathers into the water: full over half of `shore.lineM` (0.02 m), fading by
smoothstep to nothing `shore.featherM` (0.2 m) past `lineM`. The bands' sides soften by `shore.bandSoftness` (0-1; 0
§14's flat top, 1 a fade from the middle out to twice the half-width). Both are data in `shared/look/waterflow.json`,
packed in the fluid table's last vec4 (`shore_soft`: x the feather, y the softness). `dwell_look::waterflow::shore_foam`
is the WGSL `foam_amount`'s mirror; its test pins the profile across the edge (no step over 0.08 between samples 5 mm
apart; 0.52.6's was 0.28) and along it (never under half strength within the feather's first half, whatever the
noise or time). Same cost: one more smoothstep bound.
