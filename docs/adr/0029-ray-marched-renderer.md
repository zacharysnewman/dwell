# 0029. A ray-marched terrain renderer, built complete beside the mesher and compared side by side

- Status: Superseded by [0044](0044-mesher-over-ray-marching.md) (2026-10-20: the owner kept the
  mesher at A7; the renderer was removed, its code kept as the `archive/ray-marcher` tag and branch)
- Date: 2026-10-14
- Amends: [0028](0028-rust-port.md) §3 (the client's renderer: a wgpu renderer that draws terrain
  from meshes) — terrain may be drawn by either of two renderers until the decision below (A7)
- Builds on: [0012](0012-lod-octree.md) (the LOD octree the marcher's clipmap levels follow),
  [0015](0015-block-registry.md) (the state table the GPU reads), [0023](0023-bifacial-world.md)
  (both faces), [0028](0028-rust-port.md) (wgpu, WebGPU only, worker jobs without shared memory)
- Design: [`RAY_MARCHED_RENDERER.md`](../RAY_MARCHED_RENDERER.md) (the owner's answers in §9)

## Context

Today's frame is CPU-bound: encoding and the LOD walk take about 4.3 ms idle and 5.5 ms p50 flying
low (`DEV_TOOLS.md`); the LOD traversal alone is 2.6–2.9 ms, plus one draw per section, skirts and
clipped stand-ins. GPU time has never been measured. Ray marching a sparse voxel structure would
replace the meshes with a fixed set of passes whose cost depends on pixels, not on sections in view,
and makes edits a brick write, far terrain cheaper in memory, and exact AO, shadows and reflections
nearly free. Published figures (VoxelRT, Malyshau's "Six Ways to Draw Vangers") suggest
full-resolution marching is slower than raster on integrated GPUs and phones, so the answer has to be
measured on Dwell, not assumed.

The owner (2026-10-14) asked for ray marching as an **alternative**, compared side by side with the
mesher, with **two complete renderers, all distances each**, and left the decision to measurement.

## Options considered

1. **Keep the mesher only.** No risk, but the CPU-bound frame and the lighting question stay open.
2. **A hybrid only** (raster primary visibility, marched lighting). Smaller, but never measures
   whether marching can draw the world itself; kept as a fallback outcome (A6).
3. **Marching for near or far terrain only.** Each half alone can't answer which renderer is better
   overall, and the seam between the two would be its own work.
4. **A complete second renderer at every distance, compared side by side** (chosen).

Data structures for the marcher: pointer octrees (deep, slower in benchmarks, inserts reallocate),
SVDAGs (smallest, but edits need hash-consing), 3D textures (no bindless, a 2048³ cap; kept for
per-body volumes later), and **a fixed-depth brickmap with a 64-tree node per chunk** (chosen).

## Decision

1. **A second, complete terrain renderer.** GPU ray marching draws full-detail chunks and every LOD
   level; the mesher (chunk and LOD meshes) stays. Both sit behind one seam: `Scene`'s terrain calls
   become payload-generic (`ChunkMeshes`/`SectionMeshes` for raster, `ChunkBricks`/`SectionBricks`
   for the marcher), the workers run mesh or brick jobs for the active renderer (both in compare
   mode), and switching at runtime rebuilds GPU data from the CPU chunk and LOD data without a reload.
   Camera, players, outline, debug lines, water and stats are shared code, and raster overlays draw on
   both.
2. **The structure is a fixed-depth brickmap**: one 64-tree node (4×4×4 bricks of 8³ voxels) per 32³
   chunk, slots per clipmap level, LOD sections as the same node at cell size 2^L. Bricks are built on
   the CPU in the workers (deterministic, testable without a GPU), with adaptive shapes resolved there
   into a per-brick shape plane and LOD column surfaces carried as per-column heights and tints. The
   GPU's state table is generated from `shared/blocks`. A stackless hierarchical DDA (no subgroups,
   WebGPU default limits) marches it; hit positions come from integer cell, face and t, ray state is
   camera-relative and chunk-rebased.
3. **Empty-space skipping is the core of the work**, so marching reaches the horizon: cone LOD (the
   coarsest level still sub-pixel, the LOD's pixel-error rule), the hierarchy's masks, column
   min-max heights, per-brick empty distances, a beam prepass and temporal start distance, and fog and
   step-cap exits; each is measured by a steps-per-ray histogram in the telemetry.
4. **A shared frame first** (A1): an offscreen HDR target, stored sampleable depth, fog and tone
   mapping in a composite pass, and GPU pass timing (timestamp queries) in the telemetry. The lighting
   functions are shared, so a difference in the comparison is a difference in visibility. 4× MSAA is
   kept for now (owner); TAA replaces it only if measurement says so.
5. **Compared side by side, locally only.** Compare mode (`?renderer=compare`: split, blink and a
   difference heatmap, each side with its own live timings) and `npm run bench -- --compare` (fixed
   configurations of renderer, scale and upscaler; a report of every metric, near and far views
   split). The marcher and compare mode are built into **local builds only** (dev server, locally
   previewed builds, the bench) behind a build-time switch; deployed builds keep the mesher until A7.
   Correctness: a CPU reference marcher in Rust, a GPU-against-reference per-pixel test on SwiftShader
   in CI, and raster-against-march image agreement on the named views.
6. **"Measure all"; the owner decides.** No single metric decides (owner): speed (CPU, GPU, frame
   intervals), looks, view distance, memory, edit latency and streaming are reported side by side on
   the devices in the design doc (including an iPhone from the last three years and a recent Android
   flagship, which must hold 60 fps), and the owner decides at **A7**, recorded by amending this ADR's
   status. The design doc's outcome table is a guide, not a rule.
7. **Moving voxel bodies wait for the decision.** Phase 20 (voxel awakening) starts after A7, so
   cluster rendering is built once, for the chosen renderer.

## Consequences

- Two terrain paths must agree, including shapes; both read the shape registry, and image agreement
  catches drift. Compare mode doubles GPU memory (a shorter view distance there on phones).
- `e2e/terrainDraw.spec.ts` and `views.spec.ts` get per-renderer expectations.
- No public API changes (world files, seeds' terrain, protocols): no new compatibility line.
- Phase 25 of the implementation plan (steps A0–A7) builds it; Phase 26 (water) shares A1; Phase 20
  waits for A7.
- Reversing it: the marcher is behind a build-time switch and never ships; deleting it leaves the
  mesher and the shared frame (which water uses) in place. If marching wins, the mesher stays as the
  fallback for the device classes where it loses, and `ARCHITECTURE.md` §5 is updated at A7.
