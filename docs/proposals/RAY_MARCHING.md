# Proposal: a ray-marched renderer as an alternative, compared side by side

**Status:** investigated 2026-10-14, not adopted. Owner direction (2026-10-14):

- **Plan ray marching as an alternative rendering method and compare it side by side with the
  raster renderer.**
- **Ray marching for distant terrain is required**; the work is making it skip empty space well
  (§4.3). So the comparison decides the *near field* only; the far field is marched either way.

Remaining owner questions in §9.

The game gets a second, complete terrain renderer: GPU ray marching over a sparse voxel structure
(compute shaders, per-pixel marching, low-resolution marching with upscaling). The two can be
switched at runtime, shown side by side on one screen, and benchmarked against each other on the
same scenarios. Raster stays the default and always ships; the comparison decides what happens next.

This plan comes from five read-only investigations: the current pipeline, the voxel data structure,
traversal and upscaling, moving bodies and compositing, and WebGPU feasibility and budget.

## 1. What the comparison is expected to show

Written down before building so the results can be checked against it.

- Today's frame is **CPU-bound** (encoding and the LOD walk: about 4.3 ms idle, 5.5 ms p50 flying
  low, `docs/DEV_TOOLS.md`). GPU time has never been measured.
- Published software voxel traversal reaches about 100–200 Mrays/s on integrated GPUs (VoxelRT).
  Primary rays at 1080p and 60 fps need 124 Mrays/s, so **full-resolution marching on an integrated
  GPU or phone is expected to be slower than raster** (about 11–20 ms, *estimate*); at half
  resolution per axis with upscaling, about 3–4 ms on an M1-class GPU (*estimate*).
- Malyshau's "Six Ways to Draw Vangers with WebGPU" (arXiv 2608.17390, 2026) drew an editable
  multi-layer height field six ways in one wgpu engine, including two kinds of ray marching, and
  found a fitted triangle mesh fastest on every device measured. Its terrain is 2.5D, so it's a
  pointer, not proof.
- Phones keep raster depth and colour in on-chip tile memory; a compute marcher's G-buffer goes
  through main memory, so phones are expected to favour raster most.
- **Where marching is expected to win**: CPU time (no meshes to encode, no per-chunk draws), edit
  latency (one brick write, no remesh), memory for far terrain, and lighting — exact AO, sun shadows
  and reflections come almost free once the voxels are on the GPU.

If marching loses on speed but wins on looks, the fallback is the hybrid (raster primary
visibility, marched lighting), which this plan builds along the way (§7, step A6).

**Why the far field is the right place to march.** The LOD traversal is the largest CPU span today
(about 2.6–2.9 ms of the frame), plus one draw per section, skirts and clipped stand-ins. A marched
far field replaces all of that with a fixed set of passes whose cost depends on pixels, not on how
many sections are in view, and the structure only changes when a section streams in. Distant
terrain is also mostly empty space above a surface — the case hierarchical skipping is good at —
and far pixels are small, so a coarse level per pixel is enough (§4.3).

## 2. Constraints both renderers must meet

- **Two depth passes** (far, then near at 1024 m; `render/mod.rs` 1262–1388) exist for precision
  across an 8,000 km world. The marcher uses camera-relative, chunk-rebased integer+fraction ray
  state instead.
- **The bifacial world**: face B is mirrored, the camera turns over, lighting is weighted by side
  (`own()` in `render/shaders.rs`). Every "up" is the face's up.
- **Shapes**: cubes only ship today, but the registry has the corner-height shape mechanism, ladder
  plates (inset 0.05 m) and water at 0.875 of a cell; adaptive blocks (Phase 18) are planned. The
  marcher tests rays against the same shape data as the mesher and collision.
- **LOD column surfaces** have exact heights off the cell grid and side tints.
- **The look**: atlas tiles with explicit-gradient mips, face tints, ACES with exposure, fog mixed
  toward the sky, 4× MSAA. The lighting functions (`lambert`, `aces`, `fogged`, `sky()`) are shared
  by both renderers so a difference in the comparison is a difference in visibility, not shading.
- **Raster overlays** on both: players, the outline, debug lines, water, Phase 20's clusters.
- **Device**: `DeviceDescriptor::default()` today (no features, WebGPU default limits), SwiftShader
  in CI, a 96 MB distant-terrain budget on phones.

## 3. The renderer seam

The game already talks to rendering through narrow interfaces, which makes a second renderer a
sibling rather than a fork:

- `Scene` (`crates/dwell-client/src/game.rs:52`) — camera, players, outline, debug lines, LOD
  visibility, stats.
- `TerrainView<M>` (`crates/dwell-game/src/chunk_stream.rs:143`) — the streamer is already generic
  over the terrain payload `M`; today `M = ChunkMeshes`.

Plan:

- Split `Scene`'s terrain calls into a payload-generic part. Raster takes `ChunkMeshes` and
  `SectionMeshes`; the marcher takes `ChunkBricks` and `SectionBricks` (§4). Everything else
  (camera, players, outline, lines, stats) is shared code.
- The worker job kind follows the active renderer: mesh jobs for raster, brick jobs for the marcher,
  both in compare mode. CPU chunk and LOD cell data are already kept, so **switching renderer at
  runtime rebuilds the GPU data from them without a reload**.
- `?renderer=raster|march|compare` and a Settings → Graphics choice (the baseline's D4 already asks
  for the active renderer to be shown in the settings menu). The F3 overlay shows which is drawing.
- Shared frame code: the offscreen HDR target, depth, composite (fog, tone mapping), overlays, GPU
  timing. Each renderer only fills colour/G-buffer and depth.

## 4. The ray-marched renderer

### 4.1 Data: a fixed-depth brickmap

A two-level brickmap with a 64-tree node per chunk, under a clipmap per LOD level. Dwell's 32³ chunk
is exactly one 4×4×4 node of 8³ bricks.

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
  workgroup memory on the default limits). An edit rewrites one brick in place. LOD sections use the
  same node type at cell size 2^L, so far terrain is the same code at a coarser level.
- **Rejected:** pointer octrees (deep; slower in benchmarks; inserts reallocate), SVDAGs (smallest,
  but edits need hash-consing; Aokana doesn't support edits at all), 3D textures (no bindless, 2048³
  cap). 3D textures do suit per-body volumes for moving clusters (§6).
- **Built on the CPU** in the workers that mesh today: split a chunk into 64 bricks, classify each
  (air, uniform, mixed), pack mixed bricks (palette-packed for LOD), upload with
  `queue.writeBuffer`. Deterministic and testable without a GPU. Adaptive shapes are resolved on the
  worker into a per-brick shape plane, never derived from neighbours in the shader. LOD column
  surfaces (exact heights, side tints) are carried as a per-column height and tint in LOD bricks.
- **Allocation:** a free-slot stack for fixed-size level-0 bricks; `render/arena.rs`'s `FreeList`
  for LOD palette bricks. Without bindless, one pool buffer grown by `copy_buffer_to_buffer`, or 2–4
  bound pages.
- **Memory at today's view distance** (*estimate*): about 47 MiB for level 0, about 18 MiB for
  palette-packed LOD, about 1.5 MiB of nodes and slots: **65–70 MiB**, inside the default 128 MiB
  binding. (Raster's comparable figure is its mesh arena plus the LOD cache; the comparison measures
  both.)

### 4.2 Passes

1. **Beam prepass**, 1/8 resolution: one conservative ray per 8×8 tile writes a start distance.
2. **Primary march**, at the render scale (1, 0.75, 0.5): bitmask long-step marching over nodes,
   branchless Amanatides–Woo DDA inside bricks (its step mask gives the face normal free), analytic
   shape planes in shaped cells. Writes a G-buffer: albedo (atlas lookup with the hit's face UV and
   ray-cone mip level), normal, linear distance, cell id.
3. **Upscale** to output resolution when the scale is under 1: depth- and normal-aware, then FSR 1
   (EASU + RCAS) or TAA-U with reprojection. Both are compared (§5).
4. **Shading**: the shared `lambert`/`aces`/`fogged`/`sky` on the G-buffer, in the shared composite.
5. **Depth for overlays**: the march writes linear distance to `r32float` (depth formats can't be
   storage textures or copied into from `r32float`); a full-screen pass writes
   `@builtin(frag_depth)` into the depth buffer, then players, outline, lines and water raster on
   top with a normal depth test. With 4× MSAA the overlay edges alias against marched terrain:
   per-sample shading or TAA instead.

### 4.3 Empty-space skipping (what makes the far field work)

The cost of a ray is its step count; almost all of those steps are through air. Every layer below
removes steps, ranked by value. Each one is measured by a **steps-per-ray histogram** (median, p95,
p99, and the share hitting the cap) that the marcher writes to telemetry and the bench reports.

1. **Cone LOD: the coarsest level that is still sub-pixel.** A ray's footprint grows with distance
   (`distance × pixel angle`). The ray marches at the level whose cell is about one pixel wide, the
   same pixel-error rule the LOD uses today (4 px desktop, 8 px phones), and moves up a level as it
   goes further. Each level then covers a distance band of a fixed number of cells (about 176–350),
   so the step count grows with the **number of levels** (log of distance), not the distance. This is
   the single largest saving and the reason a world 8,000 km across is marchable at all.
2. **The hierarchy's own masks.** Whole slots marked `ALL_AIR`, empty bricks in a node's 64-bit mask
   and empty voxels in a brick's 512-bit mask are each skipped in one step; with the 2×2×2-coalesced
   variant VoxelRT measured about 20 iterations per ray on average.
3. **Column heights: skip everything above the ground.** Distant terrain is a surface with air above
   it. Each LOD section keeps a small max-height grid per column (it already has exact column
   surfaces, §2), with a min-max mip over it. A ray above the max height of a region crosses the whole
   region in one step — the classic min-max height-field traversal. Rays looking up past the world's
   max height exit to the sky immediately.
4. **Distance per empty brick.** Each empty brick stores the Chebyshev distance (in bricks) to the
   nearest non-empty one, computed when the node is built; a ray in an empty brick jumps that many
   bricks at once. One byte per brick, built in the worker. Most useful for grazing rays, where the
   masks alone still step brick by brick.
5. **Start rays late.** The beam prepass (one conservative ray per 8×8 tile at 1/8 resolution) and
   last frame's reprojected hit distance give each ray a start distance, so most rays begin a few
   cells before their hit. In the hybrid split, rays also start at the end of the raster near field.
6. **End rays early.** The fog distance is the max range; past about 99 % fog the colour is the
   sky's. A step cap (128 on phones) returns fog colour if reached — visible only on the worst
   grazing rays, and counted in the histogram.

**Grazing rays are the hard case.** Rays skimming just above flat ground pass through mixed bricks
the whole way, so masks alone help little; column heights (3) and brick distances (4) are what keep
them bounded. `look-horizon` in the bench is the scenario that measures it.

Rules that keep it correct and bounded: hit positions from integer cell + face + t (never
`floor(origin + t·dir)` — the source of acne and seam cracks); secondary rays start from the hit
face; a step cap (128 on phones) with the fog distance as max range and the sky colour on the cap;
temporal start depth from last frame, invalidated on edits and disocclusion. Subgroups are an
optional fast path only (about 68 % of devices, almost none on iOS, not mapped by wgpu 30's web
backend).

## 5. Side-by-side comparison

Three ways to compare, each answering a different question.

### In the game: compare mode (`?renderer=compare`)

Both renderers draw the same frame from the same camera and `Scene` state:

- **Split** (default): raster left, marched right, a draggable divider. Each renderer draws only its
  side (scissor), so the cost is about one full frame of each rather than two.
- **Blink**: a key swaps the whole screen between the two each press — the quickest way to see
  small shifts.
- **Difference**: a heatmap of where the two disagree — cell id or face differs (red), depth
  differs by more than a threshold (orange), colour ΔE over a threshold (yellow).
- An overlay on each side shows that renderer's live CPU time, GPU time (timestamp queries), GPU
  memory and its settings (scale, upscaler).
- Compare mode keeps both sets of GPU data, so memory is the sum; its frame timings are labelled
  "compare mode" and not used for decisions. Decisions use the bench.

### In the bench: `npm run bench -- --compare <scenario…>`

Runs each scenario once per configuration, in fresh pages, on the same seed and inputs, and writes
a comparison report (`OUT/compare.json` and a printed table) plus side-by-side and difference
images for every `shot` step. Configurations, so like is compared with like:

| Config | Renderer | Scale | Upscaler |
|---|---|---|---|
| `raster` | raster | 1 | — |
| `raster-half` | raster | 0.5 | browser bilinear (today's `?scale=`) |
| `march` | march | 1 | — |
| `march-half-fsr` | march | 0.5 | FSR 1 |
| `march-half-taa` | march | 0.5 | TAA-U |
| `march-075-taa` | march | 0.75 | TAA-U |

Metrics per config:

| Metric | Source |
|---|---|
| Frame interval p50/p95/p99, dropped frames at 60 Hz | existing rAF telemetry (`--unthrottled` and vsynced) |
| CPU per phase (`draw.render`, `lod`, jobs) | existing telemetry spans |
| GPU per pass | new timestamp queries (`gpu.<pass>`) |
| GPU memory, CPU memory | renderer stats, `memory()` |
| Upload bytes per frame and per edit | existing upload counters |
| Edit-to-visible latency | new: edit request → first frame showing it |
| Streaming: time to a complete view after spawn and while flying | existing chunk/LOD stats |
| Image agreement against `raster` | cell-id/face match %, depth error, colour ΔE histogram |

Scenarios: the existing `idle-spawn`, `fly-low-fast`, `fly-low-faster`, `jump-onto-block`, plus new
ones for this comparison — `look-horizon` (grazing rays, the marcher's worst case), `look-down`
(its best), `edit-burst` (place and break a few hundred blocks), `fly-high` (far terrain), and
`thermal-10min` for phones.

### Correctness under both

- A **CPU reference marcher** in Rust over the same brick structure, unit-tested on crafted layouts
  (first hit cell, face, distance, rebasing, both faces, shapes). The GPU marcher is checked against
  it per pixel on small offscreen scenes (exact ids, depth within about 1e-4 relative); offscreen
  compute runs headless on SwiftShader, so this runs in CI.
- **Raster vs march** on the named views (`client/views/views.json`): over 99.5 % cell-id match where
  both hit, differences only at silhouettes, colour within a ΔE threshold. This is the same check as
  the bench's image agreement, run as an e2e spec at a small resolution on SwiftShader.
- `e2e/terrainDraw.spec.ts` and `views.spec.ts` assume one draw per mesh; they get per-renderer
  expectations.

### Devices

Every bench comparison runs on: a Mac (M-series), a Windows laptop (integrated GPU), a Windows
desktop (discrete GPU), an iPhone, an Android phone, and SwiftShader (correctness only, never
speed). Phones need the bench to drive a device; until then, compare mode's live overlay on the
phone is the check.

### Decision criteria

The far field is marched regardless (owner, 2026-10-14). Its bar is to be **at or under raster LOD's
cost** (CPU + GPU) on every device, with the distant views' image agreement passing; if it isn't,
the work is more skipping (§4.3), not a fallback.

For the **near field** (full-detail chunks), agreed before the numbers come in (to be confirmed by
the owner, §9):

| Outcome | Then |
|---|---|
| Marched near field at its best config is at or under raster's p95 on desktop **and** phone, with image agreement passing and the owner preferring or accepting its look | Fully marched terrain, default per device class where it wins; raster chunks stay as the fallback |
| It wins on desktop only | Fully marched on desktop; raster near field + marched far field on phones |
| It loses on speed everywhere but its lighting (A6) is wanted | Raster near field + marched far field + marched lighting |
| It loses and its lighting isn't wanted | Raster near field + marched far field |

## 6. Moving bodies and other content

- **Moving voxel bodies aren't built yet** (Phases 20–22 are planned). Build them as raster meshes
  first, as planned, under both renderers (composited by depth), and generalise the per-draw yaw to
  a quaternion (needed either way). Later, give each body a small brick volume in the pool and a
  top-level grid over up to 512 body bounds, so marched rays see them; Teardown's box-rasterized
  marching is the middle option. Sequence this so cluster rendering isn't built twice.
- **Water** stays a raster pass over both renderers at first; under the marcher it later becomes a
  hit type with reflection and refraction rays (`WATER.md` W6).
- **Raster under both**: players, held items, particles, debris, outline, debug lines. The UI is DOM.

## 7. Plan

Raster stays the default throughout; every step leaves the game shippable.

| Step | Work | Exit check |
|---|---|---|
| **A0** | ADR: a second renderer behind a runtime switch (amends ADR 0028's renderer description and ARCHITECTURE §5); owner answers §9 | ADR accepted |
| **A1** | **Shared frame + GPU timing** (also `WATER.md` W2): offscreen `rgba16float` target, stored sampleable depth (handling the far/near split and MSAA), fog and tone mapping moved to a composite pass; timestamp queries in the telemetry; `bench.ts` passes `--enable-webgpu-developer-features` | Bench before/after; raster screenshots match today (or the owner approves the difference) |
| **A2** | **The seam and compare mode**: payload-generic terrain calls, the `?renderer=` switch and setting, compare mode's split/blink/difference with an empty "march" side; `bench --compare` with the configs table and report | Compare mode runs with raster on both sides and reports zero difference |
| **A3** | **Far-field marcher (required)**: the generated GPU state table, LOD sections as palette bricks at their levels with column max-heights, brick jobs in the workers, the pool and allocator, the CPU reference marcher; the march draws everything beyond the full-detail chunks, which raster draws over it by depth (the clipmap leaves the chunk region empty, so rays pass through it); overlays via the frag_depth pass; steps-per-ray telemetry | Reference-marcher tests; GPU-vs-reference per-pixel test in CI; distant named views agree with raster LOD; `fly-high` and `look-horizon` comparisons on desktop |
| **A4** | **Empty-space skipping** (§4.3): cone LOD, column min-max heights, brick distances, beam prepass, temporal start, fog and step-cap exits; tuned until the far field meets its bar on every device | Steps-per-ray p99 and far-field cost at or under raster LOD on desktop and phone |
| **A5** | **Near-field marcher (for the comparison)**: level-0 chunk bricks, shapes (analytic planes), low resolution with FSR 1 and TAA-U, render scales 0.5/0.75; `march` mode draws all terrain | Full comparison report on every device (§5) |
| **A6** | **Lighting under both**: half-res sun-shadow and AO rays from the G-buffer (march) or from raster's depth (hybrid), bilateral upsample, temporal accumulation; a quality setting | Comparison with lighting on and off; owner look |
| **A7** | **Near-field decision** by the criteria in §5; retire raster LOD (the far field is marched in every outcome); update ARCHITECTURE.md and the plan | Owner decision recorded in the ADR |

Moving bodies in the marcher (§6) and water rays (`WATER.md` W6) follow A7 if marching is kept.

Each step updates `docs/ARCHITECTURE.md` and gets deliverables in `docs/IMPLEMENTATION_PLAN.md` when
adopted; likely alongside Phase 19 (looks and textures), which the shared frame (A1) also serves.
Nothing here changes public API (world files, seeds' terrain, protocols), so no new compatibility line.

## 8. Risks

- Full-resolution marching may be slower than raster on mobile and iOS (expected, §1); the
  comparison exists to measure it.
- Two terrain paths (meshes and bricks) must agree, including adaptive shapes — both generated from
  the shape registry, and the image-agreement check catches drift.
- Compare mode doubles GPU memory; on a phone it may not fit at full view distance (shorter
  distance in compare mode).
- Temporal techniques ghost on fast bodies and edits.
- Precision far from the origin if rebasing is wrong (the reference marcher's tests cover it).
- Upload bandwidth for bricks on edits and re-bakes.
- The 4× MSAA edge between marched terrain and raster overlays.
- No subgroups on iOS, no 64-bit atomics anywhere on WebGPU.

## 9. Questions for the owner

1. **Near-field decision criteria**: are the outcomes in §5 the right ones? What matters most for
   the near field — speed, looks, or edit latency?
2. **Minimum devices**: which phones must hold 60 fps under the chosen renderer?
3. Should the shared frame (A1) go first, since water needs it too?
4. Is 4× MSAA kept, or can it give way to TAA if the composite needs it?
5. Should compare mode ship in release builds (players could look), or only dev builds and the bench?
6. Do moving bodies (Phase 20) wait for the decision, or ship as meshes first (recommended)?

## Sources

- VoxelRT benchmarks (Tree64 93–209 Mrays/s, DDA 31, ESVO 57–137, integrated GPU):
  https://github.com/dubiousconst282/VoxelRT · https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/
- Malyshau, Six Ways to Draw Vangers with WebGPU: https://arxiv.org/abs/2608.17390
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
