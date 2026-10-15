# Dev tools: telemetry and the bench

A permanent tool for running the real game in a real browser and reading what it does: per-frame
timings of each part of the frame, background job latencies, GPU uploads, the player controller's
state every tick, the browser's long tasks, screenshots and Chrome performance traces. For the
owner and for agents working on the game (it is how Claude tests and benchmarks in the browser).
It is meant to grow: a new metric is one line in Rust, a new scenario is one entry in a data file,
a new kind of step is one case in `scripts/scenarios.ts` and `scripts/bench.ts`.

Decided with the owner (2026-10-14): a dev hook that stays in the code (not a temporary
injection), always compiled in and always recording (cheap: a few hash lookups per frame), read
through `window.__dwell`; scenarios are per build (checked in), added by hand or by a tool.

## Running it

```sh
cd client
npm run build:wasm && npm run build     # the bench runs the built client
npm run bench -- fly-low-fast           # one or more scenarios by name; none: all of them
```

Options: `--swiftshader` (software WebGPU, for machines without a GPU such as CI or a Linux
sandbox; under Xvfb there), `--unthrottled` (no vsync or frame-rate cap, so frame intervals show
the frame's real cost rather than 16.7 ms), `--base URL` (a served build; by default it reuses
`vite preview` on :4173 or starts one; the server's root or the build's `…/dwell/`), `--out DIR`
(default `bench/out/<timestamp>`, ignored by git), `--file FILE` (another scenarios file, e.g. a
throwaway one in a scratch directory), `--compare` with `--configs A,B` and `--configs-file FILE`
(each scenario once per renderer configuration, side by side: below).

By default it drives the **installed Google Chrome, headed, on the machine's GPU** (Metal on a Mac),
which is what players run. Each scenario runs in a fresh browser context and local world
(`?local=1&seed=…`).

Output per scenario, in `OUT/<scenario>/`:

- `report.json`: the scenario, the GPU adapter, page errors, and for each `record` step: the
  telemetry summary, the raw kept frames, the controller ticks, the long tasks since the last
  `mark`, `__dwell.state()`, `renderStats()` and `memory()`.
- `<name>.png` for each `shot`, `<name>.trace.json` for each `traceStart`/`traceStop` (open in
  Chrome DevTools → Performance → Load profile, or read it with a script).
- `<name>.profile.txt` for each trace: the V8 sampling profiler's self and inclusive time per
  function and thread (the page's main thread, each worker), busiest first (`scripts/profile.ts`;
  Rust functions appear by name, with their hash suffix).
- A summary printed as it runs: frame interval percentiles and fps, each per-frame series' mean,
  p95, p99 and max, totals (jobs done, chunks and sections uploaded, bytes, ticks), each job kind's
  run and queue times, the GPU pass times (`gpu.*`), and long tasks.

## Scenarios (`client/bench/scenarios.json`)

```json
{ "name": "fly-low-fast", "notes": "…", "seed": 7, "window": { "width": 1280, "height": 720 },
  "settings": { "detail": { "distanceM": 96, "pixelError": 2, "memoryMb": 256 } },
  "steps": [ { "do": "settle", "maxS": 120 }, { "do": "fly", "on": true }, … ] }
```

`seed` (default 7), `window`, `settings` (the settings menu's stored values, as in named views:
`fog`, `detail`, `exposure`, `motion` — `{ "speedEffects": false }` turns the motion effects off, for
a before/after of their cost in one build),
`debug` (`?debug=1`), `scale`, `renderer` (`?renderer=`: a renderer by name; a build without the
renderer switch ignores it) and `split` (`near` or `far`: which part of the view it measures, for
`--compare`'s split) are optional. Steps (`scripts/scenarios.ts`, checked: unknown
fields, ranges and names fail with the entry named):

| Step | Does |
| --- | --- |
| `wait {s}` | waits |
| `key {code, down}`, `hold {code, s}` | presses/releases a key (`KeyboardEvent.code`), or holds it for `s` |
| `look {yaw, pitch}` | turns the player's look (degrees; yaw 0 = +Z, 90 = +X) |
| `fly {on}`, `flySpeed {level}` | creative flight and its speed level (L flies at ≥ 11 × 2^(L/2) m/s: 6 ≈ 88, 8 ≈ 176) |
| `camera {position, yaw, pitch}`, `releaseCamera` | places the camera (the player stays), or returns it |
| `edit {action}`, `select {slot}` | break/place at the crosshair; picks a hotbar slot |
| `settle {maxS}` | waits until the LOD has had nothing to generate, mesh or ask for, for 1 s |
| `mark` | forgets the telemetry so far: the next `record` measures from here |
| `record {name}` | saves everything above under `name` |
| `shot {name}` | screenshot |
| `traceStart`, `traceStop {name}` | a Chrome performance trace (timeline, GPU, V8, the CPU profiler) and its profile summary |
| `repeat {times, steps}` | runs `steps` `times` times (e.g. `edit-burst`'s 300 edits); no `mark`, `record`, `shot` or trace steps inside |

Scenarios for the renderer comparison ([`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md)
§5), besides `idle-spawn`, `fly-low-fast`, `fly-low-faster` and `jump-onto-block`: `look-horizon`
(grazing view along the ground; far), `look-down` (near), `edit-burst` (300 place/break edits;
near), `fly-high` (about 540 m up, flying; far) and `thermal-10min` (phone-oriented: ten minutes of
flight round a square, its first and last minute recorded, for thermal throttling).

## Comparing renderers (`--compare`)

```sh
npm run bench -- --compare idle-spawn look-horizon [--configs raster-half] [--configs-file F]
```

Runs each scenario once per **configuration** (`client/bench/configs.json`, checked by
`scripts/configs.ts`: unknown fields, ranges, upscaler names and duplicate names fail with the entry
named), each in a fresh page on the same seed and inputs. A configuration is a renderer, a scale
and an upscaler, passed as `?renderer=`, `?scale=` and `?upscaler=` (the last only for `fsr1` and
`taa-u`; `bilinear` is the browser stretching a scaled canvas, today's `?scale=`):

| Config | Renderer | Scale | Upscaler |
| --- | --- | --- | --- |
| `raster` (baseline) | raster | 1 | none |
| `raster-half` | raster | 0.5 | bilinear |
| `march` | march | 1 | none |
| `march-half-fsr` | march | 0.5 | fsr1 |
| `march-half-taa` | march | 0.5 | taa-u |
| `march-075-taa` | march | 0.75 | taa-u |

**Support:** the first run (the baseline's) asks the page what it offers — `__dwell.renderers()` and
`__dwell.upscalers()`, each a list of names; a build without them offers `raster` and the browser's
stretch only. Configurations the build can't run are **skipped with the reason** in the printed
report and `compare.json` (today: every `march` entry — `renderers()` lists `march` only once
`render/march.rs`'s `BUILT` is true, not while the placeholder fills its side magenta), not
failed. A new configuration is one entry in the file; `baseline` names the one the others are
compared with and `deltaE` the colour threshold (CIE76, default 2.3, about just noticeable).

Output, in `OUT/`:

- `<scenario>/<config>/`: each run's `report.json` (with its page address and the build's support),
  screenshots and traces, as a normal run's.
- `<scenario>/<shot>.<config>.side.png`: the baseline left, the configuration right;
  `<shot>.<config>.diff.png`: a heatmap of per-pixel ΔE (dim grey: equal; blue: under the
  threshold; red to yellow to white: over it, white at four times it). Stats per shot: identical %,
  % over the threshold, mean, p99 and max ΔE.
- `compare.json`: the configurations run and skipped, the build's support, per scenario and
  `record` step and configuration the metrics below, the shot diffs, each `settle`'s seconds, page
  errors, and `splits`: per `near`/`far` split and configuration, the worst interval p95/p99 and the
  dropped frames over its scenarios' records.
- **`compare.html`**: the report to look at (its path is printed at the end of the run). One
  self-contained page (inline style and script, images referenced relative to it, so open it from
  `OUT/` or copy the directory): each scenario's metrics table at the top; then per scenario and
  shot, every configuration's screenshot at full size side by side, and per configuration against
  the baseline a **blink** view (click it, or focus it and press Space, Enter or B, to swap the two
  in place), the ΔE heatmap, and its % over the threshold, mean, p99 and max
  (`scripts/compareHtml.ts`).
- A printed table per scenario, configurations side by side.

Every checked-in scenario that records also takes at least one `shot` (a test checks it), so a
comparison always has images.

Metrics per record (`scripts/compareReport.ts`): frame interval p50/p95/p99/max from the kept frames'
timestamps and **dropped frames at 60 Hz** (an interval of n refresh periods drops n − 1; run
`--unthrottled` to see cost rather than vsync); CPU per phase (every telemetry series except counts
and `gpu.*`: mean, p95, p99); **GPU per pass**: every `gpu.<pass>` series or sample when the build
records them (GPU timestamp queries), else none and a note; memory (`memory()` and its total);
upload bytes (total and per frame), chunks and sections; streaming: whether the LOD and the chunks
had anything left to generate, mesh or ask for at the record (and what), and each `settle`'s time
(the first: time to a complete view after spawn).

**TODO — edit-to-visible latency** (RAY_MARCHED_RENDERER.md §5): the telemetry can't tie a frame's uploads to
the edit that caused them (an edit is a request to the server, the reply re-meshes the chunk in a
worker, and the upload is counted per frame), so it isn't measured yet. It needs an edit sequence
number carried through the remesh and a telemetry sample (`edit.visible`, ms from the request to the
first frame drawn with its mesh), which the renderers record the same way.

Not image agreement by cell id, face or depth in the bench yet: the bench compares screenshots'
colour only. In-game compare mode's difference view (below) compares coverage, view depth and
colour on the GPU; cell id and face wait for the marcher to write ids (A3).

## Renderer switch and compare mode (local builds)

The renderer seam ([`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md) §3, §5; ARCHITECTURE.md
§5 `render/`). **Local builds only** (dev server, `vite preview`, the bench): `__RENDERER_SWITCH__`
(vite.config.ts) is off in the release workflow's builds (`DWELL_VERSION` set; force it with
`DWELL_RENDERER_SWITCH=0|1`), where `?renderer=` is ignored and the mesher draws.

- `?renderer=raster|march|compare` picks the terrain renderer; compare mode puts raster on the left
  and, by default, the marcher on the right — `?compare=raster` puts raster on **both** sides (the
  check that compare mode itself adds no difference). The marcher is a placeholder until A3: its
  side is magenta and its labels say "not built".
- The settings menu's foot shows the renderer and a picker that switches it at runtime (no reload:
  the terrain is rebuilt from the voxels and LOD cells held). F3 shows `renderer …` before the
  render stats.
- Keys (controls.ts): **F7** cycles split → blink → difference; **F8** blinks (swaps the side
  shown); in split view drag the divider. A label per side shows its renderer, CPU time, GPU time
  (`gpu.far`+`gpu.near`, `gpu.right.far`+`.near` for a raster right side, `gpu.march`), GPU memory,
  scale and upscaler. Compare mode's timings carry both renderers' cost: use the bench for decisions.
- Difference view: red where one side covers the pixel and the other doesn't, orange where view
  depth differs by more than 1e-3 relative, yellow where the composited colours differ by ΔE > 2.3,
  grey (the left side, dimmed) where they agree. The counts are read back:
  `__dwell.compare().difference` is `{ pixels, coverage, depth, colour }` (a frame or two late).
- Hooks: `__dwell.renderers()` and `upscalers()` (what the build offers, for `bench --compare`),
  `renderer()`, `setRenderer(name, compare?)`, `compare()` (view, divider, overlay lines, the
  difference counts; null outside compare mode), `setCompare(view?, divider?)`.

Zero-difference check (Phase 25's A2 exit): open `?renderer=compare&compare=raster`, then
`__dwell.setCompare('difference')`; after a frame or two `__dwell.compare().difference` reads 0 in
every class.

Kept scenarios include `idle-spawn`, `fly-low-fast`, `fly-low-faster`, `jump-onto-block`,
`jump-in-place`, and the motion effects' `sprint-straight` (shots standing, mid-sprint and after) and
`fly-fast-turn` (shots in fast flight, turning and diving: the wind's vanishing point following the
travel).

## Telemetry (`crates/dwell-client/src/telemetry.rs`)

One `Telemetry` per thread (the main thread's is exported). Three kinds of metric, named at the
call site:

- **Per-frame series** — `telemetry::add(name, value)` or `telemetry::span(name, || …)`: summed
  over the frame; kept for the last 3600 frames; summarised as mean/p50/p95/p99/max/sum.
  `interval` is the time between frames (`requestAnimationFrame` timestamps).
- **Event samples** — `telemetry::sample(name, value)`: one value per event (a job's time); the
  last 16384 per name.
- **Counters** — `telemetry::count(name, n)`: running totals.
- **Controller ticks** — one `TickSample` per predicted tick: feet, velocity, state, grounded,
  crouched, wall and ceiling contact, the input's movement, jump and crouch; the last 3600.

What is recorded now:

| Name | What |
| --- | --- |
| `frame.cpu` | the WASM `frame()` call, start to end |
| `net` | the session's network pumping (twice a frame) |
| `jobs.poll`, `jobs.apply`, `jobs.done` | collecting finished jobs; applying them (chunks into the world, meshes to the GPU); how many |
| `ticks`, `ticks.count` | the fixed-rate prediction ticks (controller + physics) and how many ran |
| `game.draw` | the game's draw step: `mesh_dirty` and `lod` inside it |
| `mesh_dirty` | starting chunk mesh jobs (copying padded chunks), within its 4 ms budget |
| `lod` | the whole-world view's update (`LodSystem`): selection, job requests, stand-ins |
| `draw.acquire`, `draw.render`, `draw.present` | the surface texture, encoding and submitting the frame, presenting |
| `upload.chunk`, `upload.section`, `upload.chunks`, `upload.sections`, `upload.bytes` | creating GPU buffers for chunk and LOD section meshes, how many, and their bytes |
| `job.<kind>.wait`, `job.<kind>.run` (samples) | per job kind (`chunk`, `chunk_mesh`, `bounds`, `section`, `section_mesh`, `verify`, `map`, `diff`): time queued in the pool before a worker had room; time from posting it to a worker until its result was polled (the worker's run, the transfers and the wait for the next frame) |
| `gpu.far`, `gpu.near`, `gpu.composite`, `gpu.frame` (samples) | GPU time of each render pass (the far and near scene passes, the composite with the lines over it) and of the whole frame (the far pass's start to the composite's end), from timestamp queries (`render/timing.rs`), one sample per timed frame, read back a frame or two later. Only where the adapter has `timestamp-query`. On tile-based GPUs (Apple, phones) passes overlap, so the passes can add up to more than `gpu.frame` |

Timer resolution: without cross-origin isolation the browser's clock is coarsened to 0.1 ms, so
sub-0.1 ms phases read 0 or 0.1. Chrome rounds GPU timestamps to 65.536 µs unless it runs with
`--enable-webgpu-developer-features`, which the bench passes (`--enable-features=WebGPUDeveloperFeatures`
does not lift the rounding).

From the console: `__dwell.telemetry()`, `__dwell.telemetryFrames()`, `__dwell.ticks()`,
`__dwell.telemetryReset()` (types in `client/src/main.ts`).

## Interactive use

The same hooks work in any browser session: open `http://localhost:4173/dwell/?local=1&seed=7`
(`npx vite preview` in `client/`), then call `__dwell.*` from the console, or from an agent's
browser tools (e.g. the Chrome DevTools MCP: `evaluate_script`, `performance_start_trace`,
`take_screenshot`).

## Adding to it

- A metric: `telemetry::span("name", || …)` / `add` / `sample` / `count` where it happens; add it
  to the table above.
- A scenario: an entry in `bench/scenarios.json`.
- A comparison configuration: an entry in `bench/configs.json`.
- A step kind: its type and parser case in `scripts/scenarios.ts` (with a test), its action in
  `scripts/bench.ts`, a row in the steps table.
- A hook the steps need: a method on `__dwell` (`client/src/main.ts`) over a `Client` export
  (`crates/dwell-client/src/web/app.rs`).

## Comparing builds

Serve the other build on another port (for example a `git worktree` of the older commit, built with
`npm run build:wasm && npx vite build` and served with `npx vite preview --port 4174`) and run the
same scenarios against both: `npm run bench -- <scenario…> --base http://localhost:4174 --out A`,
then without `--base` into `B`, and compare the two `report.json` files' `telemetry.frames`
(and the screenshots, pixel by pixel). A worktree needs its own `target/`: cargo names a path
crate's artifacts the same wherever it is, so sharing one directory mixes the two builds.
