# 0030. Fluid looks as registry data, and a water shader with refraction, underwater and reflections in quality tiers

- Status: Accepted
- Date: 2026-10-14
- Builds on: [0015](0015-block-registry.md) (the block registry and its generator),
  [0028](0028-rust-port.md) (the wgpu renderer), [0029](0029-ray-marched-renderer.md) (the shared
  frame, A1, and the ray-marched renderer, under which water becomes a hit type)
- Design: [`WATER_RENDERING.md`](../WATER_RENDERING.md) (the owner's answers in §7)

## Context

Water is one block (`dwell:water`, `"look": "water"`, `"liquid": true`, opacity 0.55) drawn by the
ordinary terrain shader with alpha blending: no normal detail, Fresnel, specular, reflection or
refraction. Its opacity is duplicated as a hardcoded `WATER_OPACITY` in the renderer, and the mesher
branches on the water look with a hardcoded surface drop (`WATER_SURFACE`, 0.875 of a cell).
Reflections and refraction can't be added as the frame is: it renders with 4× MSAA straight into the
canvas, and depth is discarded, so no scene colour or depth can be sampled. Only physics knows
whether the eye is submerged, and it counts the whole cell as wet while the drawn surface is at
0.875, so an underwater view built on it would start 12.5 cm early.

## Options considered

- **Where fluid looks live**: per build in the block registry (chosen), or per world (fluids travel
  with saves, but a world format change for rendering data). An editor page (like
  `docs/prototypes/`) was declined: the values are edited in the data file, and a page can be added
  later without changing the format.
- **What a fluid is**: looks only (chosen; owner: other fluids such as lava or oil are expected, as
  looks), or looks plus behaviour (glow, viscosity, flow, damage), which would make the fluid entry
  a code-backed system now.
- **Surface geometry**: vertex waves (Gerstner, FFT) on chunk water (rejected: greedy quads have too
  few vertices and moving tops open seams against banks, flooded shapes and the chunk/LOD join), or
  **still geometry with ripples in the shading only** (chosen; owner: water doesn't move for now).
- **Reflections on raster**: planar reflection (doubles opaque draws, one plane only; an Ultra extra
  at most) or **screen-space reflections faded into the analytic sky** (chosen); under ray marching,
  reflection rays through the voxel structure.

## Decision

1. **Fluid looks are registry data, per build.** A `fluids` table in `shared/blocks/` names each
   fluid (`dwell:water`): absorption, scatter colour, roughness, IOR, normal and wave scales, flow
   speed, foam, caustics and the surface drop. Blocks reference it by name (`"look": "fluid",
   "fluid": "dwell:water"`). `shared/blocks/gen.mjs` validates fields and ranges, failing with a
   message naming the entry, and emits a fluid table, uploaded as a storage buffer indexed by fluid
   id. The mesher writes the fluid id per vertex instead of branching on the water look;
   `WATER_SURFACE`, `CHUNK_WATER_DROP_M` and `WATER_OPACITY` come from the data or go. The values
   are prototype content.
2. **Looks only.** A new fluid is a new entry and nothing else; a fixture test adds one (`test:oil`)
   by data alone and sees it meshed and drawn with its own parameters. Behaviour (flow, viscosity,
   damage, glow as light) is out of scope.
3. **The eye-submerged rule matches the drawn surface**: the eye is underwater when the cell at
   `floor(eye)` is liquid and, if the cell above isn't the same fluid, `frac(eye.y)` is under the
   fluid's surface height (0.875 for water); tested against the face's up on both faces. It feeds a
   frame uniform (underwater flag, surface height, fluid id). A regression test pins the
   cell-against-surface mismatch, seen failing first.
4. **Still geometry.** The surface stays where it is; ripples are scrolling normal maps, Schlick
   Fresnel and a GGX glint in a dedicated water shader.
5. **The shared frame is the prerequisite** ([0029](0029-ray-marched-renderer.md) A1, built once):
   the opaque pass ends and resolves into an offscreen HDR target with stored, sampleable depth, and
   water draws after it with `LoadOp::Load`. On it: refraction with a depth check against foreground
   bleed, Beer–Lambert absorption from the water depth behind the surface (replacing the fixed alpha),
   shoreline foam; underwater, per-channel absorption fog, the water line where the near plane
   crosses the surface, Snell's window and total internal reflection, caustics, god rays and a
   distortion; screen-space reflections faded into the sky.
6. **Quality tiers** in the settings menu: Low (normal maps, Fresnel, glint, sky reflection,
   constant-depth absorption, underwater fog; no copies), Medium (half-res colour and depth copy:
   refraction, depth absorption, foam, caustics), High (plus half-res SSR and god rays), Ultra
   (full-res SSR; ray-marched reflections once the marcher exists). Low is the phone default,
   tentatively (owner: revisit once measured and seen on the two target phones).
7. **Under ray marching** (after [0029](0029-ray-marched-renderer.md)'s A3, and only if marching is
   kept at A7) water becomes a hit type with reflection and refraction rays, replacing SSR and the
   copy on the high tiers.

## Consequences

- No public API changes: fluids are rendering data and the 0.875 surface stays; no new
  compatibility line. The registry hash is unchanged unless a state changes.
- Moving tone mapping into the composite changes the shipped look slightly; the owner compares
  before/after screenshots. MSAA is off on SwiftShader, so e2e screenshots differ from GPU runs.
- Phone bandwidth for the copies is why Low has none; the bench measures each tier.
- Phase 26 of the implementation plan (W1a, W1b, W2–W6) builds it; W2 is Phase 25's A1.
- Reversing a tier or effect is a shader and settings change; the fluid data stays.
