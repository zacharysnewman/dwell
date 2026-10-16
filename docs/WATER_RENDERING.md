# Water rendering — surface, reflections, refraction, underwater

**Status:** Adopted ([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md)), 2026-10-14. Built as Phase 26
of the [implementation plan](./IMPLEMENTATION_PLAN.md) (steps W1–W6; W1 split into W1a, the fluid data,
and W1b, the water shader). Owner answers in §7.

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
  depth can be sampled. This is the main structural change ([`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md) step A1).
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
opaque draws and only suits one plane (sea level), so it's an Ultra-tier extra at most. Ray marched:
one reflection ray through the GPU voxel structure — exact, and the recommended path once
[`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md) A3 exists.

**Underwater.** Eye test into a `Frame` uniform (underwater flag, surface height, fluid id).
Per-pixel split where the near plane crosses the surface (Crest's "water line"). Per-channel
absorption fog replaces height fog while submerged. The surface from below (the water mesh is
already double-sided): outside Snell's window (critical angle `asin(1/ior)`, about 48.6°) total
internal reflection; inside, refract to the scene above. Caustics projected on opaque fragments
below the surface by world xz and the sun direction; god rays as a screen-space radial blur (or
shadow-ray samples under ray marching); a small UV wobble in the composite.

**Under ray marching** water becomes a hit type: reflection and refraction rays through the same
structure (exact Snell bend), absorption from the refracted ray's length, foam from distance to
solid, caustics and god rays from shadow rays. No colour copy. Cost moves to divergent secondary
rays: one bounce, a coarse level, a short max distance ending at the sky.

## 4. Quality tiers (settings menu, next to `detailSettings.ts`)

| Tier | Contents |
|---|---|
| Low (phone default) | Normal maps, Fresnel, glint, sky-only reflection, absorption at constant depth; underwater fog only. No copies |
| Medium | Half-res colour and depth copy: refraction, depth absorption, foam, caustics |
| High | Plus half-res SSR and god rays |
| Ultra (desktop) | Full-res SSR; ray-marched reflections when A3 exists |

Copy cost at 1080p (*estimate*): about 25 MB per frame of colour and depth copies, plus the resolve.
Trivial on desktop; real bandwidth and power on phones' tile GPUs, hence half-res copies there.

## 5. Plan

| Step | Work | Exit check |
|---|---|---|
| **W1** | Fluid data in the registry (§2), a dedicated `water_fs` (normal maps, Fresnel, GGX, sky reflection), the eye-submerged uniform with the 0.875 rule. No new passes | Fixture-fluid test; **regression test for the cell-vs-surface mismatch, seen failing first**; owner look |
| **W2** | The frame restructure — [`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md) A1 (built once, shared) | As A1 |
| **W3** | Refraction with the depth check, Beer–Lambert absorption, shoreline foam | Screenshots of shore views; bench |
| **W4** | Underwater: fog, the split line, Snell's window and TIR, caustics, distortion | Named views underwater and half in; owner look |
| **W5** | SSR and the quality tiers, phone defaults | Bench on desktop and a phone |
| **W6** | Under ray marching (after A3, if marching is kept): reflection and refraction rays replace SSR and the copy on high tiers | Parity against W5's views |

Each step updates `ARCHITECTURE.md` (render, `dwell-look`, `dwell-mesh`); the steps are Phase 26's
deliverables in the implementation plan, with W1 split into W1a (the fluid data, the mesher reading it
and the eye-submerged rule) and W1b (the water shader, after A1). None of it changes
public API: fluids are rendering data, and the 0.875 surface stays where it is.

## 6. Risks

- The far/near depth split and MSAA make scene depth awkward (two clears, no MSAA depth resolve).
- Phone bandwidth for copies and stored depth.
- Seams if anyone attempts vertex waves on greedy or LOD water.
- Per-chunk sorting without depth writes mis-sorts water behind water (waterfalls, if they come).
- SSR artifacts at screen edges; reflections of offscreen terrain missing until ray marching.
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
