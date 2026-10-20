# A ray-marched renderer as an alternative, compared side by side

**Status:** Adopted ([ADR 0029](./adr/0029-ray-marched-renderer.md)), 2026-10-14. Built as Phase 25 of the
[implementation plan](./IMPLEMENTATION_PLAN.md) (steps A0–A7). Owner direction (2026-10-14):

- **Plan ray marching as an alternative rendering method and compare it side by side with the
  raster renderer.**
- **Two complete renderers, all distances each**: ray-marched terrain (full-detail chunks and every
  LOD level) against the current mesher (chunk meshes and LOD meshes). Marching has to reach the
  horizon, so the work is making it skip empty space well (§4.3).

The owner's answers to the plan's questions are in §9.

The game gets a second, complete terrain renderer: GPU ray marching over a sparse voxel structure
(compute shaders, per-pixel marching, low-resolution marching with upscaling). The two can be
switched at runtime, shown side by side on one screen, and benchmarked against each other on the
same scenarios. The comparison runs **locally only**: the ray-marched renderer and compare mode are
built into local builds (the dev server, a locally previewed build, the bench) and not into deployed
builds, which keep the mesher until the decision (A7).

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

**Distance is where marching can win.** The LOD traversal is the largest CPU span today (about
2.6–2.9 ms of the frame), plus one draw per section, skirts and clipped stand-ins. Marched terrain
replaces all of that with a fixed set of passes whose cost depends on pixels, not on how many
sections are in view, and the structure only changes when a chunk or section streams in. Distant
terrain is mostly empty space above a surface — the case hierarchical skipping is good at — and far
pixels are small, so a coarse level per pixel is enough (§4.3). Near terrain is where marching is
expected to struggle (big on screen, every pixel a full-detail ray); the comparison reports near and
far separately (`look-down` and `jump-onto-block` against `fly-high` and `look-horizon`).

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
- `?renderer=raster|march|compare` in local builds only (a build-time switch, like the dev
  telemetry, keeps the marcher out of deployed builds until A7). The settings menu shows the active
  renderer (the baseline's D4 asks for that); the F3 overlay shows which is drawing.
- Shared frame code: the offscreen HDR target, depth, composite (fog, tone mapping), overlays, GPU
  timing. Each renderer only fills colour/G-buffer and depth.

## 4. The ray-marched renderer

### 4.1 Data: a fixed-depth brickmap

A two-level brickmap with a 64-tree node per chunk, under a clipmap per LOD level. Dwell's 32³ chunk
is exactly one 4×4×4 node of 8³ bricks.

```wgsl
struct LevelInfo { origin: vec3<i32>, dims: vec3<u32>, slot_base: u32, cell_log2: u32 };
@group(0) @binding(0) var<storage, read> levels: array<LevelInfo, LEVEL_COUNT>;
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

1. **Beam prepass**, 1/8 resolution: one conservative ray per 8×8 tile writes a start distance
   (built in A4b, with the temporal start: see "A4b as built").
2. **Primary march**, at the render scale (1, 0.75, 0.5): bitmask long-step marching over nodes,
   branchless Amanatides–Woo DDA inside bricks (its step mask gives the face normal free), analytic
   shape planes in shaped cells. Writes a G-buffer: albedo (atlas lookup with the hit's face UV and
   ray-cone mip level), normal, linear distance, cell id.
3. **Upscale** to output resolution when the scale is under 1: depth- and normal-aware, then FSR 1
   (EASU + RCAS) or TAA-U with reprojection. Both are compared (§5). Built in A4 (see "A4 as
   built"): the march writes hit records and a separate pass shades them at the render scale; FSR 1
   or a depth- and normal-aware bilateral upsample then fills the output, and the view depth takes
   the nearest marched pixel. TAA-U is still to come.
4. **Shading**: the shared `lambert`/`aces`/`fogged`/`sky` on the G-buffer, in the shared composite.
5. **Depth for overlays**: the march writes linear distance to `r32float` (depth formats can't be
   storage textures or copied into from `r32float`); a full-screen pass writes
   `@builtin(frag_depth)` into the depth buffer, then players, outline, lines and water raster on
   top with a normal depth test. With 4× MSAA the overlay edges alias against marched terrain:
   per-sample shading or TAA instead.

### 4.3 Empty-space skipping (what makes every distance work)

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
5. **Start rays late.** The beam prepass (one conservative ray per 8×8 tile at 1/8 resolution) and,
   with the eye still, last frame's distances give each ray a start distance, so most rays begin a
   few cells before their hit (built in A4b: "A4b as built").
6. **End rays early.** The fog distance is the max range; past about 99 % fog the colour is the
   sky's. A step cap (128 on phones) returns fog colour if reached — visible only on the worst
   grazing rays, and counted in the histogram.

**Grazing rays are the hard case.** Rays skimming just above flat ground pass through mixed bricks
the whole way, so masks alone help little; column heights (3) and brick distances (4) are what keep
them bounded. `look-horizon` in the bench is the scenario that measures it.

**Measured on the CPU reference marcher** (`crates/dwell-march`, `cargo run --release -p
dwell-march --example march_steps`, seed 1, 2026-10-14): chunks within 12 of the spawn and LOD
levels 1–12 (13 × 13 sections each), 1080p at 70°, 4 px, cap 1,024. Steps per ray, median / p95 /
p99:

| Rays | Masks only | + skipping, column grids on sections only | + column grids on chunks too |
|---|---|---|---|
| Horizon (pitch −6°..+1°) | 41 / 927 / 1,025 (3.4 % capped) | 41 / 330 / 455 | 8 / 68 / 83 |
| Down (pitch −90°..−30°) | 4 / 6 / 7 | 4 / 6 / 7 | 3 / 5 / 5 |
| 3 km up (pitch −30°..0°) | 79 / 131 / 157 | 44 / 60 / 71 | 44 / 60 / 71 |

Column grids help near grazing rays as much as far ones, so **chunks carry them too** (a deviation
from item 3, which named LOD sections): 2.7 KiB on a chunk node's ~25 KiB, and every horizon ray
then fits a 128-step cap. Memory measured: 28 KiB per non-trivial chunk node, 6 KiB per LOD
section node (with its column grid); 35 MiB for that whole view.

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
desktop (discrete GPU), an iPhone from the last three years, a recent Android flagship (the two phones
that must hold 60 fps; owner, 2026-10-14), and SwiftShader (correctness only, never speed). Phones need the bench to drive a device; until then, compare mode's live overlay on the
phone is the check.

### Decision criteria

No single metric decides it (owner, 2026-10-14: "measure all"). The report puts every metric in §5
side by side — speed (CPU, GPU, frame intervals), looks (image agreement, the owner's look), view
distance, memory, edit latency, streaming — split into near and far views, and the owner decides at
A7. The outcomes below are a guide to what each result would lead to, not a rule:

| Outcome | Then |
|---|---|
| Marched terrain at its best config is at or under the mesher's p95 on desktop **and** phone, with image agreement passing and the owner preferring or accepting its look | Make it the default per device class where it wins; the mesher stays as the fallback |
| It wins on desktop only | Default on desktop; the mesher on phones |
| It loses on speed everywhere but its lighting (A6) is wanted | Keep the mesher; ship marched lighting on top (the hybrid) |
| It loses and its lighting isn't wanted | Keep the mesher; the marcher stays behind the flag or is removed |

A mixed outcome (e.g. marching wins far and loses near) is visible in the split report; what to do
with one is also the owner's call at A7.

## 6. Moving bodies and other content

- **Moving voxel bodies aren't built yet** (Phases 20–22 are planned), and **they wait for the
  renderer decision** (owner, 2026-10-14), so cluster rendering is built once, for the chosen
  renderer. If the mesher stays: greedy meshes with a quaternion per draw (today's draw data is
  yaw only), composited by depth. If marching is chosen: a small brick volume per body in the pool
  and a top-level grid over up to 512 body bounds, so marched rays (and shadow rays) see them;
  Teardown's box-rasterized marching is the middle option. A0's ADR records the dependency, and
  the implementation plan's Phase 20 then waits for A7.
- **Water** stays a raster pass over both renderers at first; under the marcher it later becomes a
  hit type with reflection and refraction rays ([`WATER_RENDERING.md`](./WATER_RENDERING.md) W6).
- **Raster under both**: players, held items, particles, debris, outline, debug lines. The UI is DOM.

## 7. Plan

Raster stays the default throughout; every step leaves the game shippable.

| Step | Work | Exit check |
|---|---|---|
| **A0** | ADR: a second renderer behind a runtime switch (amends ADR 0028's renderer description and ARCHITECTURE §5); owner answers §9 | ADR accepted |
| **A1** | **Shared frame + GPU timing** (also [`WATER_RENDERING.md`](./WATER_RENDERING.md) W2): offscreen `rgba16float` target, stored sampleable depth (handling the far/near split and MSAA), fog and tone mapping moved to a composite pass; timestamp queries in the telemetry; `bench.ts` passes `--enable-webgpu-developer-features` | Bench before/after; raster screenshots match today (or the owner approves the difference) |
| **A2** | **The seam and compare mode**: payload-generic terrain calls, the `?renderer=` switch and setting, compare mode's split/blink/difference with an empty "march" side; `bench --compare` with the configs table and report | Compare mode runs with raster on both sides and reports zero difference |
| **A3** | **Marched terrain at every distance**: the generated GPU state table; brick jobs in the workers for full-detail chunks and for LOD sections (palette bricks with column max-heights); the pool, allocator and clipmap levels; the CPU reference marcher; primary march at full resolution with the hierarchy's masks and cone LOD; overlays via the frag_depth pass; steps-per-ray telemetry. `march` mode draws all terrain with no meshes | Reference-marcher tests; GPU-vs-reference per-pixel test in CI; named views (near and distant) agree with the mesher; first comparison report (desktop) |
| **A4** | **Empty-space skipping** (§4.3): column min-max heights, brick distances, beam prepass, temporal start, fog and step-cap exits | Steps-per-ray p99 down on `look-horizon` and `fly-high`; comparison report on desktop and a phone |
| **A5** | **Resolution and shapes**: render scales 0.5/0.75 with FSR 1 and TAA-U; shaped cells (analytic planes) | Full comparison report on every device (§5) |
| **A6** | **Lighting under both**: half-res sun-shadow and AO rays from the G-buffer (march) or from raster's depth (hybrid), bilateral upsample, temporal accumulation; a quality setting | Comparison with lighting on and off; owner look |
| **A7** | **Decision** by the criteria in §5; update ARCHITECTURE.md and the plan for whichever path is chosen | Owner decision recorded in the ADR |

Moving bodies in the marcher (§6) and water rays ([`WATER_RENDERING.md`](./WATER_RENDERING.md) W6) follow A7 if marching is kept.

Each step updates `docs/ARCHITECTURE.md`; the steps are Phase 25's deliverables in
`docs/IMPLEMENTATION_PLAN.md`. The shared frame (A1) also serves Phase 19 (looks and textures) and
Phase 26 (water).
Nothing here changes public API (world files, seeds' terrain, protocols), so no new compatibility line.

### A1 as built (2026-10-14)

- **Depth across the split and MSAA:** an `r32float` *view depth* colour target (metres along the
  view axis), written by every opaque fragment in both passes — the far pass clears it, the near
  pass draws over it — so it has one meaning across the split. WebGPU resolves neither depth formats
  nor `r32float`, so it is multisampled like the scene and stored; readers `textureLoad` its
  samples (the composite averages the covered ones; the lines test each). Rejected: storing both
  passes' depth buffers (two textures, two projections, twice the bytes) and `r16float`/`rg16float`
  (resolvable, but too little range and precision out to 5e7 m). The marcher's §4.2 step 5 can write
  the same target (distance ÷ |ray|) and needs no second depth path. A W3 refraction pass that wants
  a single-sample depth adds a small resolve (min or covered-mean) of this target.
- **Colour:** `rgba16float`, linear, premultiplied by coverage (alpha 0 where nothing is drawn), so
  the composite puts the sky behind by coverage and no sky pass is drawn into the scene.
- **Lines** (outline, debug) are drawn over the composite in output colour, single-sampled, with a
  per-sample depth test in the shader.
- **Image vs. before** (bench screenshots on an M-series Mac, 4× MSAA; HUD masked; identical runs of
  the old build differ by 0): spawn horizon max 33/255, mean 0.11, 1.3 % of pixels > 2; 1 km up max
  44, mean 0.41, 5.4 % > 2 — antialiased edges only (tone mapping now follows the resolve); 9 km up
  over the sea max 35, mean 2.8, 36 % > 2 — the sea is greyer, water now blending in linear light
  over the floor; looking down at a block max 128 on the outline only (no longer multisampled:
  crisper and darker), mean 0.08. Needs the owner's approval (A1's exit check).
- **Cost:** frame time unchanged within noise (the frame is CPU-bound: idle p50 7.3 → 7.6 ms,
  flying 8.2 → 8.4 ms unthrottled on a loaded machine; `draw.render` +0.1 ms). GPU, idle at the
  spawn (1280×720): far ≈ 1.3–1.6 ms, near ≈ 0.9–1.3, composite ≈ 0.7–1.3, whole frame ≈ 2.1–2.8 ms
  p50; on Apple's tile GPU the passes overlap, so they add up to more than the frame.
- **Timestamps:** Chrome rounds them to 65.536 µs unless started with
  `--enable-webgpu-developer-features` (`--enable-features=WebGPUDeveloperFeatures` does not lift it).

### A2 as built (2026-10-15)

- **The seam** (`crates/dwell-client/src/render/seam.rs`): `Scene` is a `TerrainRenderer` over
  `Terrain { raster, march }` payloads plus the shared calls. Each renderer implements
  `TerrainRenderer` for its own payloads (`set_chunk`, `set_section`, `None` removes,
  `show_sections`, `show_stand_ins`, `clear`, `gpu_bytes`); `Routed` hands each part to its
  renderer while the `Selection` draws it. One meshing job per chunk or section carries a
  `Payloads` mask (mesh, bricks or both) rather than two jobs, so compare mode costs one worker trip.
  The streamer and the LOD system pass any payload through. A switch clears the renderers no longer
  drawn and, when the payloads change, rebuilds every chunk (`ChunkStreamer::remesh_all`) and every
  drawn LOD section (`LodSystem::rebuild_all`, regenerating cells dropped after meshing).
- **The marcher's interface** (`render/march.rs`, a placeholder until A3): payloads `ChunkBricks` and
  `SectionBricks` built in the workers by `build_chunk_bricks(padded, mirror)` and
  `build_section_bricks(cells, mirror)` (the mesher's inputs) and carried by the jobs codec;
  `MarchRenderer::draw(encoder, targets, timestamp_writes)` fills one side's frame targets (linear
  colour premultiplied by coverage, view depth; 0 = miss), timed as `gpu.march`. The placeholder
  fills its side magenta; `march::BUILT` stays false (so `__dwell.renderers()` omits `march` and the
  bench skips its configurations) until the marcher draws terrain.
- **Compare mode**: a set of frame targets per side, each side's passes scissored to its part
  (split) or not drawn (blink's hidden side); the composite assembles the sides with their lines.
  The difference view composites both sides and classes each pixel: coverage differs (red; it stands
  in for cell id and face until the marcher writes ids), relative view depth > 1e-3 (orange),
  composited colour ΔE > 2.3 (yellow); counted with atomics in the fragment shader and read back a
  frame or two late. `?compare=raster` puts raster on both sides: the exit check read 0 of 921 600
  px in every class (Mac, Chrome on the GPU, seed 7's spawn).
- **Local builds only**: the shell's build-time `__RENDERER_SWITCH__` (vite.config.ts; off when the
  release workflow sets `DWELL_VERSION`). The WASM module carries the seam and placeholder in every
  build; deployed pages can't select them.
- Not yet: edit-to-visible latency and the bench's id/depth agreement (moved to A3, they need the
  marcher's output). The placeholder's clear-only pass reports no GPU time (its timestamps don't
  advance), so its overlay shows "–".

### A3b as built (2026-10-15)

- **The WGSL marcher** (`crates/dwell-client/src/render/march/wgsl.rs`): a line-for-line
  transcription of `dwell_march::march` over the words `Scene::gpu` uploads — clipmap levels and
  slots, node masks, brick DDA over raw and palette bricks, column min-max grids, empty-brick
  distances, cone LOD, the step cap and `max_t` exit, camera-relative integer + fraction ray state.
  One compute pass (8 × 8 workgroups) writes three single-sample storage textures: lit linear colour
  (`rgba16float`, alpha 1 on a hit, 0 for the sky), view depth (`r32float`, metres along the view
  axis, 0 for none) and a cell id + face (`r32uint`, `cell_id`: hashed level and cell, face + 1 in the
  low 3 bits). Shading uses the raster renderer's `lambert` (`shaders.rs` `lighting_source()`, shared
  verbatim) and the meshers' colours: at level 0 the chunk mesher's vertex colour × face tint × the
  atlas texel (`textureSampleLevel`, mip from the ray cone's footprint and the slant), above it the
  LOD mesher's linear flat colour × face tint; on face B the top and bottom swap (`face_up`) and
  side textures flip. Fog, tone mapping and the sky stay in A1's composite.
- **Generated, not copied**: every constant the shader knows (layout sizes, tags and masks, the
  histogram and debug layout, the face tints, the atlas geometry, the midplane in grid cells) is
  written from its Rust source; a test parses the module with naga and compares each constant with
  the Rust value, and checks the uniform struct's size equals what its writer fills. To make the GPU
  reproduce the reference exactly, `dwell_march::level_for` now takes ⌊log₂⌋ from the exponent bits
  (the GPU's `log2` is approximate), and the shader takes 2^−L from the bits too.
- **The state table** (`looks.rs`): per state its march class (from `StateTable`), the chunk
  mesher's colour, its top/side/bottom atlas tiles and the LOD mesher's three flat colours, built from
  the registry's data (a test adds a state by data alone).
- **Uploads** (`gpu_scene.rs`): the GPU buffers mirror `dwell_march::Scene` word for word — the
  allocator stays on the CPU, so a brick's pool offset is the same on both. `Scene::take_dirty` names
  the slots, nodes, column grids and pool ranges changed since the last frame; each (coalesced) is a
  `queue.writeBuffer`. A level whose box grows re-sends the levels and slots. Buffers double when
  they fill (re-filled from the CPU copy; no `copy_buffer_to_buffer` needed, since the CPU holds every
  word) and are capped at the device's `max_storage_buffer_binding_size` (128 MiB by default; the
  device's own limit is read); a build that would pass it is refused and counted
  (`march.pool_full`). **Binding layout** (group 1): 0 uniforms (the 13 levels, `LEVEL_COUNT`, and the camera: 752
  B), 1 slots, 2 nodes, 3 node column offsets, 4 column grids, 5 the brick pool, 6 the state table,
  7 the stats buffer (atomics: the histogram, and per-pixel debug records for the test) — 7 storage
  buffers of the default 8, 3 storage textures of 4. Group 0 is the frame (uniforms, atlas, sampler);
  group 2 the outputs. Measured at seed 7's spawn (chunks radius 8 × 7 rows, sections radius 6 on
  levels 1–12): 1,156 chunks and 4,283 sections, 3,337 nodes, a 16 MiB pool, 33.5 MiB of buffers.
  An edit uploads its node, its changed bricks and its column grid (under 8 KiB; a test).
- **Worker jobs**: the bricks are made by A2's masked meshing jobs (`Payloads::BRICKS`):
  `build_chunk_bricks(padded, mirror)` takes the 32³ interior of the mesher's padded voxels,
  `build_section_bricks` the section's cells; both in world orientation, so `mirror` changes nothing
  (the shader turns face B's textures). They travel as `NodeBuild::to_words`.
- **Behind the seam** (`renderer.rs`, `MarchRenderer`): a section is placed by its origin and cell
  size (level = log₂ cell, node = grid ≫ (L + 5)); every held section stays in the clipmap (cone
  LOD picks the level per ray; a coarser level is the stand-in where a finer one has no data).
  `draw` marches into its storage textures and then copies them into the side's shared-frame targets
  with a full-screen pass that writes every sample (compute can't write multisampled targets); the
  march's compute pass begins the `gpu.march` timestamp and the copy ends it. `BUILT = true`.
- **GPU vs reference** (`march/check.rs`): 12 crafted views at 64² — chunks (from above, oblique,
  grazing, from outside the data box, from inside solid), the same 4,000 km from the origin, face B
  from below, and LOD sections on levels 1–3 under chunks (near, high, horizon) — read back id,
  depth and a per-pixel debug record (outcome, steps, level, cell, face, `t`, state, the ray's
  direction), and march each ray on the CPU with the GPU's own direction. **Apple M1 (Metal,
  native test):** 49,152 px, 37,153 hits, ids differ 0, records differ 0, `t` within 2 ULP on
  6,195 hits (Metal's `1/d` isn't correctly rounded), depth error 2.6e-7 relative, ray directions
  within 1.8e-7 of the CPU's. **SwiftShader (headless Chromium, the CI probe):** every record
  bit-identical, `t` included; depth error 1.5e-7.
- **Measured, 1280 × 720, Apple M1, Chrome on the GPU** (`bench --compare`, seed 7, unthrottled
  off): `gpu.march` p50 **30.6 ms** at the spawn looking at the horizon (`idle-spawn`), 30.0 ms
  `look-horizon`, **8.9 ms** p95 `look-down`, 31.1 ms p95 `fly-high`; raster's whole GPU frame
  (`gpu.frame`) 2.6–2.9 ms p50 on the same views. CPU: `draw.render` 0.3–0.4 ms for the marcher
  against 2.2–3.2 ms for raster. Steps per ray on the GPU (the histogram): horizon p50 15–18, p95
  64–79, p99 66–87, none capped (cap 512); looking down p50 6, p95 8, p99 9; the `?marchtest=1`
  views: near-down p50 11 / p95 72 / p99 79, far-horizon 18 / 24 / 26. So the step counts match the
  CPU reference's measurements (§4.3), and the cost is per step: about 0.8 G iterations/s, too slow
  for 60 fps at full resolution — A4's work (profile the inner loop: the column search runs on every
  iteration; the beam prepass and temporal start), with A5's lower render scales.
- **Image against the mesher** (`bench --compare`, ΔE > 2.3): `look-horizon` 3.1 % of pixels,
  `look-down` 0.01 %, `fly-high` 19.6 % (the far LOD: see below), `idle-spawn` in
  `compare.html`. In game, `?renderer=compare` difference view at the spawn: coverage differs on
  2,981 px, depth on 28,599, colour on 15,917 of 921,600.
- **Where the looks differ**: LOD column surfaces (exact heights off the grid, side tints) are not
  in the bricks yet, so distant terrain is terraced at its level's cell size and its step sides show
  as dark lines, bands of them near the horizon where grazing rays see mostly sides; the mesher
  draws smooth column surfaces. Where the cone LOD moves up a level the ray hits the coarser cell on
  the plane it last crossed, which shows as faint arcs on flat ground seen from above. Water is not
  drawn by the marcher (it stays a raster pass; the overlay pass is A3's remaining work).
- **Not yet** (A3's rest): overlays over marched terrain (`frag_depth` pass: players, outline,
  water), raster ids for an id-based difference view and bench agreement, edit-to-visible latency,
  LOD column heights in the bricks. Clipmap level boxes only grow (a long flight keeps every slot
  it ever reserved; recentring is A4's).

### A4 as built (2026-10-15)

**Where the time went.** A3b's march cost about 1 ms per mean step over 921,600 rays at 720p on
an M1. That is roughly 1,000 lane-cycles per ray-step, so the march was latency-bound, not
ALU-bound.

The heatmap of steps per ray (`perf.rs`, `DWELL_PERF_IMAGES`) showed where the steps went:

- **Sky rays dominated:** 60–90 steps each, against 5–15 on the ground. They crossed every level's
  ALL_AIR nodes, about 6–7 per level, out to the coarsest level's box. With half the screen sky at
  the spawn, that gave a mean of 38 steps.
- **Divergence did not matter:** steps per 8×4 SIMD group, counted at its longest ray, were within
  3 % of the mean, because neighbouring rays take near-identical paths.

**Attribution.** Native Metal (`perf.rs`, seed 7's spawn, 1280×720): each variant turns off part of
the per-step work and is interleaved with the others in rounds, because the GPU was shared with
other agents. Each figure is the minimum of 20 frames:

| Variant (on the vectorised march) | Spawn | Down | 540 m up |
|---|---|---|---|
| Everything | 11.4–17.8 ms | 2.6–2.9 ms | 16–20 ms |
| Without the column search | 19.7 (steps ×3) | 2.1 | 18 (steps ×1.6) |
| Without the level-rows skip | 24 (steps ×2.9) | 2.8 | 44 (steps ×3.6) |
| Without the shade pass | −0.3 | −0.3 | −0.3 |
| An empty march (stores only) | 0.3 | 0.3 | 0.3 |

Workgroups of 8×8, 8×4, 16×8, 16×16 and 32×2 made no difference beyond the noise.

**What helped, in order** (native Metal, minimum of 20 frames):

1. **Vectorised, branch-light inner loop:** spawn 37 → 21 ms, down 4.9 → 2.4 ms, at the same steps.
   - Plane distances, box exits, `cell_at` and the brick DDA became vec3 selects. Each component
     keeps the reference's own arithmetic, so the per-pixel check stays exact.
   - The column search walks from the last mip instead of from the coarsest. Clearing is
     monotone in the mip: a finer region's span and exit lie inside the coarser one's.
   - The occupancy word is cached across DDA steps, and uniform bricks are tested before the
     columns.
2. **Each level's occupied rows:** spawn mean steps 38 → 13 (p95 87 → 27, p99 96 → 39); 540 m up
   39 → 11 (p99 89 → 29); time about 21 → 14 ms at the spawn.
   - `Scene::rows` keeps, per level, the grid rows its occupied cells span, from each node's column
     grid. The levels' words carry them (`occ_lo`, `occ_hi`).
   - A ray above or below everything its level holds leaves the level's box in one skip. The box is
     clipped per ray to `band_end × |d|` around the eye: the point where the cone LOD moves past the
     level. That way the skip never hides a coarser level's data beyond the band (an earlier cube
     clip, inscribed in the band's sphere, left 50–80 sky steps).
   - The reference march (`march_steps`, 1080p): horizon p95 68 → 22, 3 km up median 44 → 9.
3. **Shading in a pass of its own:** spawn about 16 → 12 ms, with noisy measurements. The march
   writes a hit record (state, face, level, face-B bit); `shade_main` lights it, so the march
   kernel carries fewer live values.
4. **Render scale with an upscaler.** At 0.5 the march does a quarter of the rays, and its cone LOD
   follows the coarser pixels.
   - **FSR 1:** EASU then RCAS, ported to WGSL from `ffx_fsr1.h`'s float path. Both run on colour
     tone-mapped into 0..1 by `c / (1 + max c)`, which is undone after RCAS.
   - **Bilateral:** a joint upsample whose four bilinear taps are weighted by view depth and face.
   - The view depth takes the nearest marched pixel; the colour stays premultiplied by coverage.
   - Chosen with `?scale=0.5|0.75&upscaler=fsr1|bilateral` (local builds); the canvas stays at full
     size.

**What did not help.** The workgroup shape, as above. Computing the mip offset from a formula
instead of a `switch` made no difference. Removing the per-pixel id store did not help either: the
three stores and the empty dispatch cost 0.3 ms.

**Not done (A4b).** These are the next levers. After the level-rows skip, the sky takes 15–20 steps
and the ground 5–10, and the cost is in each step's chain of dependent loads (slot → node →
columns → brick → state).

- **The beam prepass** and **the temporal start distance:** built in A4b (below).
- **Clipmap recentring:** the level boxes still only grow (A3b's note). That costs slot memory on
  long flights, not march time.

**The cone-LOD rings** (the owner's concentric arcs, `march-far-down`):

- **Cause:** where the cone LOD moved up a level, a ray in the finer level's air could find itself
  inside an occupied coarser cell (LOD cells round heights up to their size). It hit that cell on
  the plane it had last crossed, usually a vertical one. Every such hit drew a dark side face at the
  switch distance: a ring around the eye.
- **The rule now, in the reference and the WGSL together:** a switch to a coarser level waits while
  the coarser cell at the ray is occupied and the level the ray marched at has data there. The ray
  stays at the finer level and tries again at the next iteration. The coarser surface is then
  entered through its own faces.
- **The test:** `cone_lod_switches_leave_no_seam_on_flat_ground` (`tests/march.rs`) marches 600
  rays along a radial line over flat ground. The levels' tops are at 5, 6 and 8 m, so they disagree,
  as LOD rounding does. Every hit must be a top face at one of the levels' heights. On the old rule
  it failed with about 60 side-face hits at 100–250 m, and it passes now. An aligned scene must also
  show a smooth depth.
- **What remains:** the levels' heights still disagree by up to a coarse cell, because the bricks
  don't carry LOD column heights yet (A3c). A switch can still show as a step in depth (a terrace
  edge), but no longer as a ring of side faces. The near field is unchanged: levels 0 and 1 meet
  past 100 m.
- **Screenshots:** before and after, in the PR.

**Correctness.** The GPU-against-reference per-pixel check stays exact: on an M1's Metal, 12 views,
ids differ 0, records differ 0, `t` within 2 ULP, depth error 2.9e-7. The reference's tests all pass,
the new seam test among them.

**Measured in Chrome** (`bench --compare idle-spawn look-horizon look-down fly-high --unthrottled`,
M1, 1280×720). `gpu.march` is the march through to the copy into the shared frame, p50 ms.
`gpu.frame` is raster's whole GPU frame. The machine was shared with other agents' benches, so each
run's load average is given, and every row compares with raster measured in the same run:

The A4 build ran at a load average of 5–7, a quiet machine (2026-10-15, 22:46–23:00).
`gpu.march` has few samples per configuration (3–15; looking down, 300–800): its timer reads back
only when a ring buffer is free. Each cell is `gpu.march` p50 / frame interval p50, in ms:

| Scenario | raster (`gpu.frame` / frame) | A3b `march` | A4 `march` | A4 `march-half-fsr` | A4 `march-075-fsr` | A4 `march-half-bilateral` |
|---|---|---|---|---|---|---|
| idle-spawn | 2.1 / 6.6 | 35.5¹ (30.6²) | **17.7** / 18.6 | **5.5** / 6.3 | 12.7 / 13.3 | 7.6 / 5.7 |
| look-horizon | 2.0 / 6.8 | 43.2¹ (30.0²) | ≈ 20³ / 19.5 | **8.7** / 7.2 | 13.2 / 13.9 | 6.8 / 6.7 |
| look-down | 1.4 / 6.5 | 8.9² (p95) | **4.5** / 4.9 | **2.5** / 4.4 | 3.7 / 4.2 | 1.6 / 4.7 |
| fly-high | 3.7 / 4.6 | 31.1² (p95) | **19.8** / 21.1 | **8.4** / 8.4 | 13.7 / 14.9 | 5.5 / 7.2 |

¹ The A3b build in this session, at load 10 rising to 40, beside raster's 2.4–3.2 ms in the same
run. Its later scenarios' screenshots time out at its 40–60 ms frames.
² A3b's own run on a quiet machine (as built, above).
³ No sample in the quiet run. 23.6 in a run at load 44–66. The frame interval, 19.5, bounds it.

Earlier runs of the A4 build at load 4–66 read the same within the noise: idle-spawn 18.9,
look-down 4.9, fly-high 21.4, half-scale FSR 5.4 / 9.3 / 2.4 / 10.3.

**Steps per ray** (the GPU histogram from the native harness, the same views):

| View | A3b p50 / p95 / p99 | A4 p50 / p95 / p99 |
|---|---|---|
| The spawn | 10 / 87 / 96 | 10 / 27 / 39 |
| The horizon (−3°) | 7 / 87 / 96 | 7 / 27 / 39 |
| Looking down | 3 / 5 / 5 | 3 / 5 / 5 |
| 540 m up | 30 / 81 / 89 | 9 / 22 / 29 |

**Against the goal** (raster's ≈ 3 ms at 720p):

- At full resolution the marcher is still about 8× raster's GPU frame at the spawn (≈ 18 against
  2.1 ms), down from about 12–15×; on the 540 m flight it is about 5×, down from about 11×.
- Looking down it is 4.5 ms against A3b's 8.9, which beats raster's frame interval there (4.9
  against 6.5 ms, a CPU-bound frame).
- At half resolution with FSR 1 it is 2.5–8.7 ms, and the frame interval matches raster's or beats
  it on every view: 6.3, 7.2, 4.4 and 8.4 ms, against raster's 6.6, 6.8, 6.5 and 4.6.
- The 0.75 scale costs about 0.7× full resolution, not 0.56×: at that scale the cone LOD's levels
  fall almost where they do at full resolution, so the rays take the same steps.
- The next levers are in "Not done" above, with lower per-step latency on Tint (Chrome's
  compiler; Naga's Metal output is 1.3–1.6× faster on the same views).

### A4b as built (2026-10-19)

After A4a, each ray took 15–20 steps through the sky and 5–10 to reach the ground, and every step
cost a chain of dependent loads. A4b starts rays late, so most of those steps are never taken.

**The beam prepass** (`dwell_march::beam`, WGSL `beam_main`): a compute pass before the march, one
thread per 8×8 tile at the render scale.

- **The cone.** At the same distance `t`, a pixel ray of the tile is within `t · |d − d_c|` of
  the tile's centre ray. The largest `|d − d_c|` is at a corner of the tile, and that is the
  beam's spread κ, widened by 0.1 %. Over a segment `[t0, t1]`, every pixel ray lies in the box
  around the centre ray's segment, widened by `κ · t1` plus a slack for `f32` rounding.
- **A segment is cleared** only if that box holds no occupied cell at any level a pixel ray there
  could stop at. That is every level from 0 up to the first level, from the cone level at `t1`
  upward, that has data all over the box. The cone level grows with `t`, and the LOD-switch rule
  can keep a ray at a finer level it marched before, so finer levels count too.
- **Per level, the cheapest test first:** the level's box and occupied rows; then each node the
  box meets (at most 3 a side); the node's column grid, at the mip whose regions the box fits two
  a side; then the bricks; and in a mixed brick, the occupancy bits a row at a time.
- **The walk** doubles segments from 1 m until one is blocked, then bisects down to one
  cone-level cell, with at most 48 segments. Every pixel ray of the tile then starts at the
  distance cleared (`Scene::march_from`).
  - Bisecting after the first blocked segment, instead of growing again after every cleared one,
    took the pass from about 5 ms to 1 ms natively.
  - A coarser stop (4 cells instead of 1) cost more pixel steps than it saved.

**The temporal start.** When last frame drew the same targets, the beam prepass can also start a
tile from last frame's distances. It does so only if all of these hold:

- the eye moved at most 1 mm;
- the field of view and size are the same;
- no bricks were uploaded since (edits, streaming, a rebind).

Each march workgroup (one tile) reduces its rays' clear distances into a tile texture: a hit's
`t`, the farthest distance on a miss, and 0 at the step cap. The next frame's prepass projects
the tile's corners into last frame's image, widens the footprint by 2 px, and reads the tiles it
meets (at most 3 a side). The start is the least of those distances, minus two cone-level cells
and the eye's shift. The tile takes whichever start reaches farther, the beam's or the temporal
one.

- **Why the eye must be still.** With the eye and the data unchanged, the set of surfaces the eye
  sees doesn't change, so a newly visible surface (disocclusion) can't occur. Every cell is wider
  than 2 px at its cone level, so it can't hide between last frame's samples. The margin covers
  the slant across a cell.
- **Disocclusion when the eye moves.** A surface hidden behind a near occluder can appear in a
  strip as wide as the parallax, which is about 10 px at 5 m walking at 6 m/s. Neither a gather
  nor a scatter of last frame's depths bounds that strip. So the temporal start is off whenever
  the eye moves, and the beam alone applies.
- **Earlier versions.** Taking the temporal start alone, without the beam, cost more than it
  saved looking down: its margin is wider than the beam's. Reading the full-resolution distances
  over a 12² region per tile cost 0.4 ms. The tile reduction removed both costs.

**Correctness.**

- **The per-pixel GPU check** (`check.rs`, Apple M1 Metal) reads back the start each GPU ray used
  and marches the reference from it. Each view is drawn after the same view turned by 3°, so
  temporal starts are exercised. Across 12 views and 49,152 px: ids differ 0, records differ 0,
  `t` within 2 ULP, and no start passes the hit of a march from the eye. 5,248 rays took the
  temporal start.
- **Outcome changes.** 368 rays end differently from a march from the eye. All of them are in the
  two section views, at LOD switches: those views build each level's terrain in its own cells, so
  the levels disagree by 2–8×. The switch rule waits at the finer level only where the iterations
  fall, and a later start moves the iterations. Both outcomes are pictures the rule allows.
- **Generated terrain** (seed 7's spawn, 1280×720, `DWELL_PERF_CHECK=1`, every pixel marched on
  the CPU from the eye): no start passes a hit on any view. The outcome changes at the spawn, the
  horizon and looking down: none. 540 m up: 1,352–2,113 (0.15–0.23 %), all from the beam (the
  same count without the temporal start), at LOD switches.
- **The CPU tests** (`tests/march.rs`) cover random scenes at three cone widths, a single voxel
  that only some of a tile's pixel rays hit, and flat ground whose levels disagree both ways
  (coarser above finer, and finer above coarser). Their starts never pass a hit. The tests fail if
  the cone's spread is dropped, or if levels finer than the cone level are left out of the check.

**Measured natively** (Metal, `perf.rs`, seed 7, 1280×720, minimum of 20 frames, load 3–7). Each
cell is the march through the shade pass, in ms, with steps per ray as mean / p95:

| View | A4a (no beam) | beam | beam + temporal | the beam pass alone |
|---|---|---|---|---|
| The spawn | 13.9–14.1 (11.5 / 23) | 5.3–5.4 (3.0 / 16) | 4.3 (2.1 / 5) | 1.0 |
| The horizon (−3°) | 11.3–13.9 (11.0 / 23) | 5.6 (3.1 / 16) | 4.3–4.4 (2.3 / 5) | 0.9–1.0 |
| Looking down | 2.7–3.0 (3.2 / 4) | 2.3–2.5 (2.3 / 3) | 2.3–2.6 (2.3 / 3) | 0.1 |
| 540 m up | 15.3–16.3 (10.0 / 16) | 9.1–9.3 (3.7 / 8) | 9.3 (3.7 / 8) | 1.3–1.6 |

The beam pass alone is timed without the temporal start (`beamonly` variant).

**Measured in Chrome** (`bench --compare idle-spawn look-horizon look-down fly-high --unthrottled`,
M1, 1280×720, load 3–4, 2026-10-19).

- Each cell is `gpu.march` p50 in ms (beam pass through the copy), with steps per ray as mean /
  p95.
- `march-nobeam` is A4a's march; `march-notemporal` has the beam alone.
- Raster's column is its whole GPU frame (`gpu.frame` p50), now with the water passes of Phase 26.
- Every view but `fly-high` holds the camera still, so the temporal start applies there;
  `fly-high` moves, so it falls back to the beam.

| Scenario | raster | march-nobeam | march-notemporal | march | march-half-fsr |
|---|---|---|---|---|---|
| idle-spawn | 5.0 | 21.3 (12.2 / 27) | 10.7 (3.7 / 18) | **8.5** (2.5 / 9) | **4.9** |
| look-horizon | 4.7 | 19.2 (11.2 / 25) | 8.3 (3.2 / 15) | **6.6** (2.3 / 5) | **3.9** |
| look-down | 4.1 | 4.8 (3.3 / 4) | 3.6 (2.3 / 3) | **3.5** (2.3 / 3) | **2.2** |
| fly-high | 3.6 | 23.9 (7.7 / 11) | 17.5 (3.8 / 8) | **17.5** (3.8 / 8) | **6.6** |

**Against the goal:**

- **Full resolution.** The march is 2.2–2.9× faster at the spawn and the horizon, and within
  1.3–1.7× of raster's whole frame there. It is faster than raster looking down.
- **540 m up.** Only 1.4× faster: about 3.8 steps a ray still cost 17 ms in Chrome and 9 ms
  natively. Each step there costs about twice what it does at the spawn, which points to the
  per-iteration chain at the LOD levels (the level search, the switch rule's extra lookups, the
  column-mip walk, palette bricks) and to divergence: as 8×4 SIMD groups the rays take 5.0 steps
  against a mean of 3.7.
- **Half scale with FSR 1.** At or under raster's GPU frame at the spawn, the horizon and looking
  down (4.9, 3.9 and 2.2 ms against 5.0, 4.7 and 4.1), and 1.8× it 540 m up.
- **Next levers.** The per-step cost at the LOD levels, and temporal reuse while the eye moves,
  which needs a disocclusion test or a conservative reprojection.

### A4c: the survey (2026-10-19)

The owner asked for the marcher to be optimized in every case (moving fast, seeing far, standing
still), with temporal reuse while moving if it can be done without artefacts. This survey of the
common ways to speed up ray marching was written first. Each technique is rated by its fit to
Dwell's layout (the 64-tree node per chunk over 8³ bricks, a clipmap of node slots per LOD level,
column min-max grids per node, the beam prepass, A4b's temporal start).

**Where the time goes, measured first** (native Metal, `perf.rs`, seed 7, 1280 × 720, with the
eye still and the temporal start on; terrain generator v15, about twice A4b's relief, so the
figures are not A4b's):

- Rays take 2.3–3.8 iterations on average after the beam and temporal starts. The beam pass itself
  is 0.1–2.5 ms; 540 m up it is the largest single pass.
- 540 m up, the march costs about 2.3 ms per mean iteration, against about 0.8 at the spawn and
  looking down. Counting each kind of iteration (a `CAT` counter per site, in profiling variants):
  per ray there, 2.1 column-region skips in mixed bricks, 8.6 column-region tests (about 4 per
  skip: the walk up and down the column mips), 3.8 level lookups and only 1.3 voxel steps.
- **Turning the column grids and level rows off** (`nocolumns`) made the march 540 m up *faster*
  (9.0 against 10.7 ms) at 2.6× the iterations (9.9 against 3.8), and looking down 0.5 ms faster at
  the same iterations. So the per-iteration cost is the column machinery, not the step count. It is
  latency-bound: about 400 lane-cycles per column test, one dependent load each.
- Loading every column mip at once (independent loads into a private array) was 20 % *slower*
  everywhere: the array spills registers. Caching a node's column offset, or skipping the column
  search at level 0, changed nothing beyond the noise. The cost is sensitive to registers and to
  divergence, not to the arithmetic.
- Divergence: per 8 × 4 SIMD group the longest ray takes 1.0–1.4× the mean iterations (1.4×
  540 m up).

| Technique | Fit to Dwell | Expected gain | Cost | Risk | Decision |
|---|---|---|---|---|---|
| **Temporal reprojection while moving** (Hi-Z of last frame's clear distances: Uludag's Hi-Z traversal applied to the start distance) | The beam's segments already have a conservative footprint: project it into last frame's image and read a min pyramid of last frame's tile distances. Disocclusion is handled by construction: only space last frame's rays crossed is trusted | Moving views at the spawn back to the still-eye step counts (6 → 3 iterations a ray) | A reduce pass, 4 loads per segment test, the changed nodes in the uniforms | LOD bands: last frame vouches only for the level it marched at | **Built (A4c 1)** |
| **Keeping an unchanged frame** (same eye, view, size, data, settings) | Trivial: the march's outputs persist in its targets | Standing still and untouched: the march skipped, only shading runs | A comparison per frame | None: bit-identical by construction | **Built (A4c 2)** |
| **If-if traversal** (Aila & Laine 2009): one loop whose iterations are each one voxel step or one hierarchy step, instead of a DDA loop nested in the hierarchy loop | The march is while-while today (the brick DDA nested in the node loop); lanes in a brick hold their SIMD group while the rest skip | 10–30 % where lanes diverge (540 m up, grazing) | Control flow only; same arithmetic, so the per-pixel check stays exact | Registers | Tried: 0–15 % slower, dropped |
| A cheaper column search: one load per iteration, a coarse heightfield phase before the fine one, or the column grid addressed by node index | The column grid is what costs at the LOD levels (above) | Up to 2× per iteration 540 m up | Layout change in `dwell_march` and the WGSL | Registers again; the reference changes with it | Its cheapest form built: steep rays skip the search in mixed bricks (**A4c 3**); the layout changes not yet |
| A top-level sparse 64-tree over the nodes (VoxelRT's Tree64), a flatter level stack | Dwell already has the 64-tree inside a chunk; the clipmap slots are a flat array per level | Small now: after the beam the sky takes few steps | A new top structure, edits and streaming | Medium | Not now |
| Distance fields per voxel in mixed bricks | Empty-brick distances exist (§4.3 item 4); with them off the march is unchanged within noise | Small: 1–2 voxel steps per ray remain | A byte per voxel | Low | Not now |
| Per-pixel temporal start (each pixel's own last distance, not the tile's least) | Rays then start nearer their hit with the eye still or turning | Modest: the 2-cell margin dominates at distance | A full-resolution distance target | Low | Maybe, after the above |
| Checkerboard or interleaved marching (Rainbow Six Siege, El Mansouri 2016) | Half the rays each frame, the rest reprojected | About 40 % | Reprojection and hole filling | Artefacts in motion (the owner wants none); half scale with FSR 1 exists already | Not chosen |
| Variable-rate or foveated marching; a lower resolution for distant pixels | One ray per 2 × 2 where a tile's rays all stop at coarse cells | 20–40 % far | A coarse pass and a fill | Silhouettes; overlaps the cone LOD | Not chosen |
| Subgroup-coherent traversal, ray sorting (Garanzha & Loop 2010; Meister et al. 2025), persistent threads | Divergence is 1.0–1.4× (above), so the bound is about 30 % | Small | Subgroups aren't mapped by wgpu's web backend nor on iOS; sorting's overhead | High | Not chosen |
| Compressing the per-step data | The march is latency-bound, not bandwidth-bound | Small | Layout | Low | Not chosen |
| Early termination at LOD distance | The fog distance and the level-rows skip already end sky rays | Small | — | — | Already there |

**The choice.** The temporal start while moving and the kept frame first (the moving and the
still cases, cheap and exact), then the if-if loop (the far case's divergence), then the column
search's cost if it is still what dominates far away. (As built, below: the if-if loop measured
slower and was dropped; the column search's cost was cut by its slope rule.)

### A4c as built (2026-10-19)

**1. The temporal start while the eye moves.** A4b trusted last frame's distances only with the eye
still, since a surface hidden behind a near occluder can appear in a strip as wide as the
parallax. A4c trusts them for the space last frame's rays actually crossed, wherever this frame's
eye is:

- **The footprint.** A stretch `[t0, t1]` of a tile's pixel rays is the piece of the cone between
  its four corner rays. Its 8 corner points, taken relative to last frame's eye, are projected
  into last frame's image; the piece projects inside their rect, widened by 2 px. Every point of
  it is within `t1 + s` of last frame's eye, where `s` is how far the eye moved. So the stretch is
  clear if last frame's least clear distance over the rect, less `s` and A4b's margin of two
  cone-level cells, reaches `t1`. Nothing a ray now sees can be nearer than that without having
  been in last frame's cleared space, so disocclusion needs no test: a newly visible surface lies
  behind last frame's distances.
- **A min pyramid** of last frame's tile distances (a reduce pass, `reduce_main`, after the march)
  reads the rect in 4 loads, at the mip where it spans at most 2 texels a side.
- **Changed data.** The nodes put or removed since last frame (`Dirty::regions`, as level-0 boxes,
  `Dirty::boxes`, at most 16 in the uniforms: merged per level, then into one, beyond that) are
  not trusted: a stretch whose box meets one is left to the box test. Streaming no longer turns the
  temporal start off.
- **Cone-LOD bands.** Last frame's rays only vouch for the level they marched at, and a point is up
  to `s` nearer or farther than it was, so its cone level can differ. A stretch is trusted only
  inside one band from both eyes (ending `s` before the band's end); the `2s` around each band's
  end is the box test's.
- **The LOD-switch rule.** A ray the rule held at a finer level than its cone level vouches only
  for the finer level from there: the march's clear distance now stops where that first happened
  (`wait_t`). This also cut the still-eye outcome changes in the crafted section views (356 → 48).
- **One jump per band.** Testing every segment of the beam's walk cost the beam pass more than the
  march saved (the beam pass is latency-bound: one thread per tile, a dependent chain of tests).
  The walk now tries, once per band, to jump from where it is to the tile's own last distance less
  the margin (`temporal_goal`); if the footprint's least distance falls short, it jumps as far as
  that reaches (a shorter stretch's footprint lies inside it) and otherwise leaves the band to the
  box test. Within `8 s` of the eye it uses the box test alone (the footprint is too wide there).
  The tile's corner rays are kept as vectors in last frame's axes: an array of them spilled.
- **Switches:** `?marchmoving=0` (A4b's still-eye start); the bench's `march-a4b`.
- **Correctness.** The per-pixel GPU check draws each view after three last frames: the view
  turned (A4b's), and turned from an eye 1.5 m behind and 1.5 m ahead. **Red → green:** before the
  band rule it found 3 starts past a hit (the section views, after a step back, start 39 m against
  a hit at 21 m); with it, none, over 36 views and 147,456 px (ids and records still exact, `t`
  within 2 ULP). On generated terrain with the eye moving 0.5 m a frame (`DWELL_PERF_MOVE`,
  `DWELL_PERF_CHECK`): no start past a hit; outcome changes only 540 m up, 0.05–0.09 % of rays, at
  LOD switches (the beam alone: 0.03–0.04 %).

**2. Keeping an unchanged frame.** With the same eye, view axes, field of view, size, targets and
settings as last frame, and no brick uploaded, the march's outputs (depth, ids, hit records) are
still right, so the beam, march and reduce passes are skipped and only the shade pass runs (the
lights and the time of day can change). Bit-identical by construction; a test pins what counts as a
change (the eye by 1e-6 m, a turn, a zoom, an edit, other targets, a setting; not the lights).
`?marchreuse=0`, the bench's `march-noreuse`. Telemetry: `march.reused`, `march.temporal`,
`march.changed` per frame.

**3. The column search only for rays near level.** In a mixed brick the march searched the column
mips every iteration; for a steep ray the voxel steps across the brick cost less than the search.
The search now runs in a mixed brick only for rays with `|d.y| < 0.15`
(`COLUMN_MIXED_MAX_DY`, in the reference and the WGSL alike, so the per-pixel check stays exact);
empty bricks always search. Tried at 0.05–0.4: 0.15 was best on every view. `?marchsteep=0`, the
bench's `march-nosteep`.

**Tried and dropped** (native, interleaved rounds, the eye moving 0.5 m a frame):

- The if-if loop (Aila & Laine): one loop whose iterations are each a voxel step or a hierarchy
  step. 0–15 % *slower* on every view: divergence was not the cost.
- Testing the box first and last frame's distances only where it fails; trying last frame's
  distances only while the walk grows; ending the walk where they stop: each within ±5 % of the
  chosen walk, or worse.
- The column mips loaded at once, a node's column offset cached, the level-rows skip off, the
  search off at level 0 (see the survey).

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

## 9. Owner answers (2026-10-14)

| # | Question | Answer |
|---|---|---|
| 1 | What decides it: speed, looks, view distance, edit latency? | Not sure yet: **measure all**; no single metric decides (§5) |
| 2 | Which phones must hold 60 fps? | An iPhone from the last three years and a recent Android flagship |
| 3 | Does the shared frame (A1) go first, since water needs it too? | Yes |
| 4 | Keep 4× MSAA, or give way to TAA? | Keep it for now; decide once measured |
| 5 | Does compare mode ship to players? | No: **run locally to compare**. The marcher and compare mode are in local builds only until A7 |
| 6 | Do moving bodies (Phase 20) wait for the decision? | Yes, they wait |

No questions are open for this proposal; A0 turns these answers into the ADR.

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
- A4c's survey: Aila & Laine, Understanding the Efficiency of Ray Traversal on GPUs (HPG 2009; if-if
  and while-while loops, persistent threads): https://research.nvidia.com/publication/understanding-efficiency-ray-traversal-gpus ·
  Uludag, Hi-Z Screen-Space Cone-Traced Reflections (GPU Pro 5, 2014): https://gameenginegems.com/gemsdb/article.php?id=1221 ·
  El Mansouri, Rendering Rainbow Six Siege (GDC 2016, checkerboard rendering): https://gdcvault.com/play/1022990/Rendering-Rainbow-Six-Siege ·
  Garanzha & Loop, Fast Ray Sorting and Breadth-First Packet Traversal for GPU Ray Tracing (2010): https://diglib.eg.org/items/86c62fdb-d534-458a-86fc-8c75d37d019c/full ·
  Meister et al., On Ray Reordering Techniques for Faster GPU Ray Tracing: https://arxiv.org/abs/2506.11273 ·
  Museth, hierarchical DDA in OpenVDB: https://research.dreamworks.com/wp-content/uploads/2018/07/talk_hierarchical_digital_OpenVDB_v4-Edited.pdf ·
  Aaltonen, GPU-based clay simulation and ray tracing in Claybook (GDC 2018): https://www.gdcvault.com/play/1025316/Advanced-Graphics-Techniques-Tutorial-GPU
