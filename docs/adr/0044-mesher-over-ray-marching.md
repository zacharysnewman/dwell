# 0044. The mesher is the terrain renderer; the ray marcher is removed

- Status: Accepted (the owner's decision at Phase 25's A7, 2026-10-20)
- Date: 2026-10-20
- Supersedes: [0029](0029-ray-marched-renderer.md) (a ray-marched terrain renderer built complete
  beside the mesher, compared side by side until this decision)
- Restores: [0028](0028-rust-port.md) §3 as written: one wgpu renderer that draws terrain from meshes
- Code: the tag and the branch `archive/ray-marcher` (commit `8ed481f`, main at 0.20.3), which hold
  the marcher's last state: `crates/dwell-march`, `crates/dwell-client/src/render/march`, compare
  mode, the bench's march configurations and the design doc in full

## Context

ADR 0029 built a second terrain renderer, GPU ray marching over a brickmap with a cone LOD, beside
the greedy mesher, behind a renderer seam with a runtime switch and an in-game compare mode, in local
builds only. Steps A1–A4c built it complete (every distance, both faces) and made it as fast as
three rounds of empty-space skipping could (beam prepass, temporal start, kept unchanged frames, the
column search's slope rule), measured in Chrome with `bench --compare` and natively. The figures
below are from `RAY_MARCHED_RENDERER.md` as it was at A4c (in the archive branch); the march's
column is `gpu.march` p50 in ms, raster's its whole GPU frame (`gpu.frame` p50), M1, 1280×720.

**A4b** (2026-10-19, beam prepass and temporal start; every view but `fly-high` holds the camera
still, so the temporal start applies there):

| Scenario | raster | march (full res) | march, half res + FSR 1 |
|---|---|---|---|
| idle-spawn | 5.0 | 8.5 | 4.9 |
| look-horizon | 4.7 | 6.6 | 3.9 |
| look-down | 4.1 | 3.5 | 2.2 |
| fly-high (540 m, moving) | 3.6 | 17.5 | 6.6 |

At full resolution the march was 1.3–1.7× raster's *whole* frame at the spawn and the horizon and
about 4.9× it flying 540 m up; it beat raster only looking straight down. Only at half resolution
with an upscaler did it reach raster's frame near the ground, and it was still 1.8× it up high.

**A4c** (2026-10-19, measured under load; `march-noreuse` marches every frame, as a moving camera
at 60 Hz does; `march` also keeps unchanged frames):

| Scenario | raster | march-noreuse | march |
|---|---|---|---|
| idle-spawn | 7.1 | 10.4 | 1.0 |
| look-horizon | 6.8 | 10.9 | 1.0 |
| look-down | 5.5 | 3.0 | 1.1 |
| fly-high (moving) | 6.0 | 18.5 | 19.1 |
| far-view (540 m, still) | 5.0 | 15.1 | 2.1 |
| fly-low-fast (flight) | 5.6 | 9.1 | 2.0¹ |

¹ Unthrottled frames between ticks see the same camera and are kept; at 60 Hz with the camera moving
every frame, `march-noreuse` is the figure.

The march beat raster only where nothing changed between frames (a kept frame costs the shading and
the copy, about 1 ms) or looking down. Whenever the eye moves — walking, flying, every frame that
matters for play — it cost 1.5–3× raster's whole frame. **A4c's far cases** (2026-10-20) then tried
the three largest remaining levers for flying high and seeing far (cheaper LOD steps, a second beam
per 2×2 or 4×4 pixels, half-rate marching for distant tiles); none beat the 11.0 ms the march took
natively 540 m up, so none was kept. The per-step cost is the memory latency of each step's loads,
not their number, and the only measured win left was a coarser cone LOD, which changes the look.

## Decision

The owner (2026-10-20): **"Ray marching is massively slower. We can roll it back… Mesher / raster
is definitely our way forward."**

- The mesher (chunk meshes and LOD meshes, rasterised) is the terrain renderer on every device. No
  per-device-class choice, no hybrid (A6's marched lighting is not built).
- The ray marcher is **removed from main** (a patch release): `dwell-march`, `render/march` (the march, the
  beam and temporal passes, the FSR 1 and bilateral upscalers, the native profile and the per-pixel
  GPU check), compare mode and its difference view, the renderer seam's routing and per-renderer
  payloads, the `?renderer=`, `?compare=`, `?upscaler=` and `?march*=` switches and the shell's
  `__RENDERER_SWITCH__`, the `?marchtest=1` page, `__dwell.renderers`/`upscalers`/`setRenderer`/
  `compare`/`setCompare`, the bench's march configurations and the scenarios made for the marcher
  alone (`far-view`, `look-around`), and the WebGPU probe's march check.
- **Kept**, because the mesher uses them: the shared frame (A1, water's W2: linear colour and view
  depth offscreen, the composite), GPU pass timing, `bench --compare` for raster configurations (a
  configuration is now a `?scale=` and page parameters), and the terrain interface
  (`TerrainRenderer`, now over the mesher's meshes only). `?scale=` stays: it predates the marcher.
- The code stays in history: the `archive/ray-marcher` tag and branch. Bringing marching back would
  start from there with a new ADR.

## Consequences

- One terrain path: no second payload to keep in agreement with the meshes, no compare mode doubling
  GPU memory, smaller WASM. Raster performance work goes to the mesher, the LOD and the draw path.
- Water's W6 (reflection and refraction rays under ray marching) is dropped; screen-space reflections
  stay the reflections (WATER_RENDERING.md).
- Phase 20 (moving bodies), which waited for this decision (ADR 0029's owner answer 6), is unblocked:
  moving bodies are drawn by the raster renderer.
- The face-B "stray dark slabs" bug in the LOD's far meshes, deferred in case marching replaced the
  mesher, now has to be fixed in the mesher (IMPLEMENTATION_PLAN.md).
- No saved world, generated terrain or protocol changes: a patch release.
