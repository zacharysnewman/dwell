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
`vite preview` on :4173 or starts one), `--out DIR` (default `bench/out/<timestamp>`, ignored by
git), `--file FILE` (another scenarios file, e.g. a throwaway one in a scratch directory).

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
  run and queue times, and long tasks.

## Scenarios (`client/bench/scenarios.json`)

```json
{ "name": "fly-low-fast", "notes": "…", "seed": 7, "window": { "width": 1280, "height": 720 },
  "settings": { "detail": { "distanceM": 96, "pixelError": 2, "memoryMb": 256 } },
  "steps": [ { "do": "settle", "maxS": 120 }, { "do": "fly", "on": true }, … ] }
```

`seed` (default 7), `window`, `settings` (the settings menu's stored values, as in named views),
`debug` (`?debug=1`) and `scale` are optional. Steps (`scripts/scenarios.ts`, checked: unknown
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

Timer resolution: without cross-origin isolation the browser's clock is coarsened to 0.1 ms, so
sub-0.1 ms phases read 0 or 0.1.

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
