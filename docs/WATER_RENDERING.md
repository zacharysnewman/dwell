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
  "normalScale": 1.0, "waveScale": 2.0, "flowSpeed": 0.03,
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

**Underwater.** Eye test into a `Frame` uniform (underwater flag, surface height, fluid id).
Per-pixel split where the near plane crosses the surface (Crest's "water line"). Per-channel
absorption fog replaces height fog while submerged. The surface from below (the water mesh is
already double-sided): outside Snell's window (critical angle `asin(1/ior)`, about 48.6°) total
internal reflection; inside, refract to the scene above. Caustics projected on opaque fragments
below the surface by world xz and the sun direction; god rays as a screen-space radial blur; a small
UV wobble in the composite.

## 4. Quality tiers (settings menu, next to `detailSettings.ts`)

| Tier | Contents |
|---|---|
| Low (phone default) | Normal maps, Fresnel, glint, sky-only reflection (no clouds), absorption at constant depth; underwater fog only. No copies |
| Medium | Colour copy (full-res in W3; half-res in W5): refraction, depth absorption, foam, caustics; a cheap SSR (12 steps) and the clouds reflected (W5a) |
| High | Plus a finer, farther SSR (28 steps, W5a), the reflected clouds' self-shadow, and god rays |
| Ultra (desktop) | Full-res SSR |

Copy cost at 1080p (*estimate*): about 25 MB per frame of colour and depth copies, plus the resolve.
Trivial on desktop; real bandwidth and power on phones' tile GPUs, hence half-res copies there.

## 5. Plan

| Step | Work | Exit check |
|---|---|---|
| **W1** | Fluid data in the registry (§2), a dedicated `water_fs` (normal maps, Fresnel, GGX, sky reflection), the eye-submerged uniform with the 0.875 rule. No new passes | Fixture-fluid test; **regression test for the cell-vs-surface mismatch, seen failing first**; owner look |
| **W2** | The frame restructure (built once, as Phase 25's A1, and kept: the shared frame) | As A1 |
| **W3** | Refraction with the depth check, Beer–Lambert absorption, shoreline foam; caustics (brought forward from W4) | Screenshots of shore views; bench |
| **W4** | Underwater: fog, the split line, Snell's window and TIR, distortion, god rays | Named views underwater and half in; owner look |
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
    own heading at `flowSpeed` × 1, 1.7 and 2.6. They are summed as height slopes, scaled by
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
- **Bench:** the `water-views` scenario flies from the seed-7 spawn to the coast about 20 km
  east-south-east, down into the sea, and shoots four views: across the sea, the shallows toward the
  shore, 1 km up, and with the eye just under the surface. The `camera` step gained `aroundPlayer`
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
  waterline.
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
- **Bench:** the `water-shore` scenario flies to the `water-views` coast. It shoots the beach edge
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
  - Binary refine steps pin the hit between the last two steps.
  - **Water writes no depth**, so a ray over flat water can't hit the water or the floor seen
    through it: the ray stays in front of both (a test pins it).
  - **What it shows** (`ssr_reflected`): the under-water copy (W3's) at the hit, filtered, and fogged
    by the composite's height fog over the hit's distance. It is mixed over the sky and clouds by
    the hit's weight and coverage.
  - **The fades.** The weight fades within 12 % of the screen height of any edge, so reflections
    don't pop at the edge, and over the last 30 % of the reach. A miss is the sky and clouds.
  - **Fresnel and the sun.** The reflection is Fresnel-weighted like the sky's (Schlick, F0 0.02), so
    a grazing view shows the shore strongly and a steep one hardly at all. Where the scene hides the
    sky, it hides the sun's and moon's glint and the sparkle too.
  - **Far and near.** In the far pass, water reflects the far scene only. In the near pass, it
    reflects both (the view depth and the copy hold both by then).
- **Data** (`shared/look/reflections.json` → `reflections.mjs` → `dwell-look` `reflections.gen.rs`,
  validated, with a CI stale check; prototype tuning):

  | Tier | March steps | Refine | Reach | Cloud octaves | Cloud self-shadow |
  |---|---|---|---|---|---|
  | Low | 0 (must be: no copy) | 0 | — | 0 (the sky alone) | no |
  | Medium | 12 | 4 | 2 km | 3 | no |
  | High | 28 | 6 | 4 km | 5 | yes (with the Clouds setting's) |

  The values reach the shader in the frame uniform's `reflect` and `reflect_sky` (dwell-look
  `reflection_uniforms`). The thickness, edge fade, distance fade and `rippleScale` are shared by the tiers.
  - **`rippleScale` (0.3):** the land's ray is steered by a third of the ripples' slope. With the full
    slope, rays scattered from pixel to pixel and broke the treeline's reflection into dark speckle
    across the near water (seen in Chrome). The sky and clouds take the full ripple.
  - **The reach** was 600 m and 1.5 km at first. That missed the treeline across the lagoon, seen
    near the horizon. The reach costs nothing: the steps are spread over the ray's length on screen.
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
- **Bench:** the `water-reflections` scenario (Medium) flies to the `water-views` coast. From 1.5 m
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
   sparkle and foam sample the noise tile; caustics sample theirs where a pixel covers a texel or more and are
   evaluated per pixel nearer (blended between), so close up they stay exact. Periods round to whole tiles
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

**Tiers:** Low (phones): one ripple layer, no scene reads. **Medium is now the desktop default**: the whole
look. High: Medium plus the fine caustic layer to twice the distance (reflections at their High settings
come with W5a). A CPU adapter (SwiftShader in CI) draws Low whatever is chosen (`water_for`, #104).

**Look:** the shore, edge and under-the-surface views were compared before and after: the same look, with
the caustic and ripple patterns in other places (new hashes and periods).

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
