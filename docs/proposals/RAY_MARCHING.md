# Proposal: ray-marched voxel rendering

**Status:** investigated 2026-10-14, not adopted. Owner questions open (§8).

Should Dwell's terrain be drawn by GPU ray marching over a sparse voxel structure (compute shaders,
per-pixel marching, low-resolution marching with upscaling) instead of, or as well as, rasterized
chunk meshes? This plan comes from five read-only investigations: the current pipeline, the voxel
data structure, traversal and upscaling, moving bodies and compositing, and WebGPU feasibility and
budget.

## 1. Conclusion

**Don't replace the raster renderer yet. Add ray marching beside it, for what it does better.**

- Today's frame is **CPU-bound**: encoding (WASM→JS calls) and the LOD walk, about 4.3 ms idle and
  5.5 ms p50 flying low (`docs/DEV_TOOLS.md`). Ray marching doesn't remove that work, and GPU time
  has never been measured.
- Published software traversals reach about 100–200 Mrays/s on integrated GPUs (VoxelRT, below).
  Primary rays at 1080p need 124 Mrays/s at 60 fps, so full-resolution primary marching on an
  integrated GPU or a phone would cost about 11–20 ms (*estimate*), worse than today. Malyshau's
  "Six Ways to Draw Vangers with WebGPU" (arXiv 2608.17390, 2026) compared six renderers of an
  editable multi-layer height field in one wgpu engine, including height-field and
  voxel-accelerated ray marching, and found a fitted triangle mesh fastest on every device measured.
  Its terrain is 2.5D, not a full 3D voxel world, so this is a pointer, not proof.
- Phones lose most: their raster depth and colour stay in on-chip tile memory, and a compute
  marcher's G-buffer round trip through main memory is mostly bandwidth.
- What ray marching gives cheaply is **lighting**: exact ambient occlusion, sun shadows without
  shadow maps, reflections, and later one-bounce GI. Rays for those are short or start on a surface,
  so they are cheap, and they run at half resolution with no loss to voxel edges.

So the plan is a **hybrid**: raster keeps primary visibility; a GPU copy of the voxels serves
shadow, AO and reflection rays at half resolution; then, if the bench says so, far terrain and
finally primary visibility move to the marcher. Each step is behind a runtime switch and raster
always ships.

## 2. Constraints the port must keep (from the current pipeline)

- **Two depth passes** (far, then near at 1024 m; `render/mod.rs` 1262–1388) exist for precision
  across an 8,000 km world. A marcher needs camera-relative, chunk-rebased integer+fraction ray state.
- **The bifacial world**: face B is mirrored, the camera turns over, lighting is weighted by side
  (`own()` in `render/shaders.rs`). Every "up" is the face's up.
- **Shapes**: cubes only ship today, but the registry has the corner-height shape mechanism, ladder
  plates (inset 0.05 m) and water at 0.875 of a cell; adaptive blocks (Phase 18) are planned. A
  marcher needs analytic ray-vs-shape tests from the same shape data as the mesher and collision.
- **LOD column surfaces** have exact heights off the cell grid and side tints; a plain downsampled
  octree loses that look.
- **The look**: atlas tiles with explicit-gradient mips, face tints, ACES with exposure, fog mixed
  toward the sky, 4× MSAA. The lighting functions (`lambert`, `aces`, `fogged`, `sky()`) can be
  reused verbatim on a marched G-buffer.
- **Raster overlays**: players, the outline, debug lines, water, and Phase 20's clusters.
- **Device**: `DeviceDescriptor::default()` today (no features, WebGPU default limits), SwiftShader
  in CI, a 96 MB distant-terrain budget on phones.

## 3. The GPU voxel structure

**A fixed-depth two-level brickmap with a 64-tree node per chunk, under a clipmap per LOD level.**
Dwell's 32³ chunk is exactly one 4×4×4 node of 8³ bricks.

```wgsl
struct LevelInfo { origin: vec3<i32>, dims: vec3<u32>, slot_base: u32, cell_log2: u32 };
@group(0) @binding(0) var<storage, read> levels: array<LevelInfo, 20>;
@group(0) @binding(1) var<storage, read> slots: array<u32>;  // node | NONE | ALL_AIR | ALL_SOLID(state)
struct Node {
  mixed:   vec2<u32>,         // 64-bit mask: brick has a payload
  uniform: vec2<u32>,         // 64-bit mask: brick is one solid state
  brick:   array<u32, 64>,    // payload index, or the uniform state id
};                            // 272 B
@group(0) @binding(2) var<storage, read> nodes: array<Node>;
@group(0) @binding(3) var<storage, read> pool: array<u32>;  // bricks: occ[16] + u16 states (+ u8 shapes)
@group(0) @binding(4) var<storage, read> states: array<StateGpu>;  // generated from shared/blocks
```

- **Why this one.** Fixed depth means a stackless hierarchical DDA (no subgroups and 16 KiB of
  workgroup memory on the default limits). An edit rewrites one brick in place. LOD sections use
  the same node type at cell size 2^L.
- **Rejected:** pointer octrees (deep; slower in benchmarks; inserts reallocate), SVDAGs (smallest,
  but edits need hash-consing; Aokana doesn't support edits at all), 3D textures (no bindless, 2048³
  cap). 3D textures do suit per-body volumes for moving clusters (§5).
- **Built on the CPU**, in the workers that mesh today: split a chunk into 64 bricks, classify each
  (air, uniform, mixed), pack mixed bricks (palette-packed for LOD), upload with
  `queue.writeBuffer`. Deterministic and testable without a GPU. Adaptive shapes are resolved on
  the worker into a per-brick shape plane, never derived from neighbours in the shader.
- **Allocation:** a free-slot stack for fixed-size level-0 bricks; `render/arena.rs`'s `FreeList`
  for LOD palette bricks. Without bindless, one pool buffer grown by `copy_buffer_to_buffer`, or
  2–4 bound pages.
- **Memory at today's view distance** (*estimate*): about 47 MiB for level 0 (about 1,400 chunks with
  surface), about 18 MiB for palette-packed LOD, about 1.5 MiB of nodes and slots: **65–70 MiB**,
  inside the default 128 MiB binding. Unpacked LOD would push the total past it.

## 4. Traversal, acceleration and upscaling

Ranked by value for effort:

1. **Bitmask long-step marching** over 64-tree nodes (`countOneBits`/`firstTrailingBit` on
   `vec2<u32>` masks; WGSL has no u64), with a **branchless Amanatides–Woo DDA** inside bricks. The
   DDA's step mask gives the face normal free, which the per-face look needs.
2. **Camera-relative, chunk-rebased ray state.** Hit positions come from integer cell + face + t,
   never `floor(origin + t·dir)` (that is where acne and seam cracks come from). Secondary rays start
   from the hit cell's face, not `p + ε`.
3. **Ray start from raster depth**: secondary rays start at the G-buffer surface; primary rays (if
   ever) start at rasterized chunk bounds (Teardown, Aokana).
4. **Full-resolution geometry, half-resolution lighting**, joint-bilateral upsampled with full-res
   depth and normal, plus temporal accumulation. Voxel silhouettes stay exact; only soft lighting
   is upscaled. This is the main answer to "lower resolution with upscaling".
5. **Temporal reprojection** (motion vectors are needed for physics bodies anyway), invalidated on
   edits and disocclusion.
6. **A tile beam prepass** (one conservative ray per 8×8 tile writes a start depth) — only for
   marched primary visibility.
7. **Analytic shape planes** in leaf cells (2–6 planes per shape, a generated shape table).
8. **FSR 1** (EASU + RCAS, two small passes) or **TAA-U** — only if primary visibility goes
   low-res. FSR 2/3 in WGSL is not worth porting.
9. **Subgroup fast paths** — optional: about 68 % of devices, almost none on iOS, and wgpu 30's web
   backend doesn't map the feature.

Early termination everywhere: a step cap (e.g. 128 on mobile), the fog distance as max range, and
the sky colour when the cap is hit. Grazing horizon rays dominate the p99 step count on a flat world.

## 5. Moving bodies, entities and compositing

- **Moving voxel bodies aren't built yet** (Phases 20–22 are planned). Build them as raster meshes
  first, as planned (`EntitySpawn` layout → greedy mesh), and generalise the per-draw yaw to a
  quaternion (needed either way). Later, give each body a small brick volume in the pool and a
  top-level grid over up to 512 body bounds, so shadow and AO rays see them. Teardown's
  box-rasterized marching is the middle option. Sequence this so cluster rendering isn't built twice.
- **Depth for raster overlays.** WebGPU depth formats can't be storage textures and can't be copied
  into from `r32float`. The marcher writes linear distance to an `r32float` storage texture, and a
  full-screen pass writes `@builtin(frag_depth)` into the existing depth buffer, per projection of
  the two split passes. With 4× MSAA that edge aliases: shade per sample or replace MSAA with TAA.
- **Raster stays for** players, held items, particles, debris, the outline, debug lines and water.
  The UI is DOM and unaffected.
- **Edits** get faster: one brick write, visible next frame, with no mesher in between.

## 6. Measurement and tests

- **GPU timing first.** Request `timestamp-query` when the adapter has it (about 99 %), put
  `timestamp_writes` on each pass, resolve into a ring of three `MAP_READ` buffers, report
  `telemetry::sample("gpu.<pass>", ms)`. Chrome rounds to 100 µs without
  `--enable-webgpu-developer-features`; `bench.ts` should pass it. Also useful for raster today.
- **A/B in the bench**: a `renderer` field in scenarios and `?renderer=raster|march`. Raster at
  `scale: 0.5` is the fair baseline for half-resolution marching. Add a 10-minute flight scenario
  for phone thermals.
- **A CPU reference marcher** in Rust over the same brick structure, unit-tested on crafted layouts
  (first hit cell, face, distance, rebasing, both faces). The GPU marcher is checked against it per
  pixel on small offscreen scenes (exact ids, depth within about 1e-4 relative); offscreen compute
  runs headless on SwiftShader.
- **Against raster**: render the named views (`client/views/views.json`) both ways and compare a
  cell-id/face buffer (over 99.5 % match where both hit; differences only at silhouettes), and
  colour within a ΔE threshold.
- `e2e/terrainDraw.spec.ts` and `views.spec.ts` assume one draw per mesh; a marched path needs its
  own expectations.

## 7. Plan

Each step is shippable on its own; raster stays the default until R6.

| Step | Work | Exit check |
|---|---|---|
| **R0** | ADR (changes ADR 0028's renderer description and ARCHITECTURE §5); owner answers §8 | ADR accepted |
| **R1** | **Frame restructure + GPU timing** (shared with `WATER.md`): offscreen `rgba16float` target, stored sampleable depth (handling the far/near split and MSAA), fog and tone mapping moved to a composite pass, timestamp queries in the telemetry and bench | Bench before/after; screenshots match the current look (or the owner approves the difference) |
| **R2** | `Renderer` switch behind the `Scene` trait (`?renderer=`, shown in settings); generated GPU state table from `shared/blocks`; brick building in the workers and the GPU pool for level-0 chunks; the CPU reference marcher | Reference-marcher tests; brick round-trip tests; memory readout in F3 |
| **R3** | **Half-res sun shadow + AO rays** from the G-buffer, bilateral upsample, temporal accumulation; a quality setting (off on low) | Bench on desktop and a phone; owner look |
| **R4** | LOD sections in the pool (palette bricks); **far-terrain marching** for pixels raster leaves empty, at half res; reflections for water (`WATER.md` W6) | Parity of distant terrain against raster views; bench |
| **R5** | Moving bodies in the marcher (after Phase 20 ships them as meshes); player proxies for shadows | Bodies cast and receive shadows |
| **R6** | *Only if R3–R4's numbers justify it:* marched primary visibility with a beam prepass, temporal start depth, FSR 1 or TAA-U at 0.67–0.75 scale; flip the default per device class | Bench parity or better on desktop, phone and CI |

## 8. Questions for the owner

1. Is the goal **looks** (shadows, AO, reflections) or **speed** or **view distance**? The hybrid
   serves the first; the evidence says a full port doesn't deliver the second.
2. Is a hybrid acceptable as the long-term answer, or is a full ray-marched renderer the goal
   regardless of the bench?
3. Minimum devices: which phones must hold 60 fps, and is a lighting quality setting (off on low)
   acceptable?
4. Should R1 (the frame restructure) go first as its own step, since water needs it too?
5. Is 4× MSAA kept, or can it give way to TAA if the composite needs it?
6. Do moving bodies (Phase 20) wait for this, or ship as meshes first (recommended)?

## Sources

- VoxelRT benchmarks (Tree64 93–209 Mrays/s, DDA 31, ESVO 57–137, integrated GPU):
  https://github.com/dubiousconst282/VoxelRT · https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/
- Editable 64-tree in Rust: https://github.com/expenses/tree64
- Aokana (SVDAG + LOD + streaming, ~6 ms on an RTX 3060 Ti, combined with meshes): https://arxiv.org/abs/2505.02017
- Laine & Karras, Efficient Sparse Voxel Octrees: https://research.nvidia.com/publication/efficient-sparse-voxel-octrees
- Kämpe et al., High-Resolution Sparse Voxel DAGs: https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf
- Brickmaps (van Wingerden 2015): https://utrecht-theses9-test.atmire.com/items/64b88d98-99fd-442e-b140-1d34966f1b7b
- Amanatides & Woo DDA: http://www.cse.yorku.ca/~amana/research/grid.pdf · branchless DDA: https://www.shadertoy.com/view/4dX3zl
- Teardown's renderer: https://acko.net/blog/teardown-frame-teardown/ · https://blog.voxagon.se/2024/12/29/year-summary.html
- FSR 1: https://gpuopen.com/fidelityfx-superresolution/ · TAA (Karis 2014): https://advances.realtimerendering.com/s2014/
- WebGPU feature availability: https://web3dsurvey.com/webgpu · subgroups in Chrome 134: https://developer.chrome.com/blog/new-in-webgpu-134
- Chrome developer features (timestamp precision): https://developer.chrome.com/docs/web-platform/webgpu/developer-features
- WebGPU spec (depth formats not storage; copies into depth): https://www.w3.org/TR/webgpu/
