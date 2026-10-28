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
(each scenario once per configuration, side by side: below).

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
`detail`, `exposure`, `clouds`, `water`, `motion` and `timeOfDay` (0–1, 0 midnight: `?time=`; the fog is fixed look data, not a setting) —
`{ "speedEffects": false }` turns the motion effects off, for a before/after of their cost in one
build),
`debug` (`?debug=1`), `scale`, `split` (`near` or `far`: which part of the view it measures, for
`--compare`'s split) and `face` (`"b"`: the player spawns on face B, the dev link `?spawn=b` —
BIFACIAL_WORLD.md §11; default `"a"`) and `portal` (`true`: the player spawns on the rim over the ladder of the portal nearest the centre, on `face`: the dev link `?spawn=portal` or `portal-b`, BIFACIAL_WORLD.md §12; `portal-a`, `portal-b` shoot a portal's openings, its shaft and the climb across the midplane, `portal-motes` its motes) are optional. Steps (`scripts/scenarios.ts`, checked: unknown
fields, ranges and names fail with the entry named):

| Step | Does |
| --- | --- |
| `wait {s}` | waits |
| `key {code, down}`, `hold {code, s}` | presses/releases a key (`KeyboardEvent.code`), or holds it for `s` |
| `look {yaw, pitch}` | turns the player's look (degrees; yaw 0 = +Z, 90 = +X) |
| `fly {on}`, `flySpeed {level}` | creative flight and its speed level (L flies at ≥ 11 × 2^(L/2) m/s: 6 ≈ 88, 8 ≈ 176) |
| `puppets {count, at?, face?, held?}` | stand-in players (`__dwell.puppets`): `count` full bodies (≤ 256) in rows ahead of the player's feet offset by `at`, on `face` (1, or −1 for face B), each walking a small circle with a clip of its own (walk, sprint, slide, crouch, idle, jump, swim, climb), holding the hotbar's block with `held`; 0 removes them. For the frame cost of N bodies and screenshots of every clip without N clients ([`CHARACTERS.md`](./CHARACTERS.md)) |
| `arms {on}` | your first-person arms shown or hidden (the Show your arms setting, this page only) |
| `spyglass {on, step?}` | the spyglass zoomed to a step (0 the least, `shared/look/spyglass.json`) or out ([`SPYGLASS.md`](./SPYGLASS.md)); `spyglass-far` shoots far terrain unzoomed and at each step |
| `camera {position, yaw, pitch, aroundPlayer?, aboveFeet?}`, `releaseCamera` | places the camera (the player stays), or returns it; with `aroundPlayer`, x and z are offsets from the player's feet (y absolute, or with `aboveFeet` an offset too: a height over the spawn's ground): a view wherever a flight ended, where the chunks are streamed |
| `edit {action}`, `select {slot}` | break/place at the crosshair; picks a hotbar slot |
| `region {op, from, to, up?}` | a region edit (the region tools, `__dwell.tool('cornerA@x,y,z')`) of the box between `from` and `to`, offsets in metres from the player's feet (`up` −1 on face B): `fill` with the selected hotbar block or `clear` to air — to build a scene, e.g. `tunnel-a`/`tunnel-b`'s hill and tunnel (DAY_NIGHT.md §11), and `torch-cave-a`/`torch-cave-b`'s tunnel and room at midnight with the torch held and placed (LIGHTS.md; `torch-night-puppets` records 8 torch-bearing stand-ins against none; `torch-scale-a` shoots the 1 m torch beside a stand-in at noon and midnight and the held flame by day and night); `torch-shadows-a`/`torch-shadows-b` place a torch before a stone pillar in the room and hold one too, recording the Torch shadows' cost with `--configs torch-shadows-off,torch-shadows-high`; `felling` builds a tree on the flat world and breaks its trunk, shooting the fall and the settled logs, FELLING.md) |
| `brush {command}` | drives the brush tool as its keys and clicks do (`__dwell.brush`: `toggle`, `preset:<name>`, `size:<n>`, `apply`, …; [`BRUSHES.md`](./BRUSHES.md)), in worlds that allow brushes (sandbox: a `--compare` config with `params: {"world": "sandbox"}`) |
| `settle {maxS}` | waits until the LOD has had nothing to generate, mesh or ask for, for 1 s |
| `until {what, maxS}` | waits until the view waits for nothing `behind` the camera (out of its frustum: `__dwell.state().lod.waitingOffView` 0), nothing `nearBehind` it within the full-detail distance (`waitingNearOffView` 0: the full detail behind is there) or nothing `ahead` (`waitingInView` 0), and saves the seconds since the page was opened in `report.json`'s `untils`; every run also saves `join` (page opened → playing, and the loading screen's wait, ARCHITECTURE.md §6.3). `join-behind` times the join's timeline and shoots behind the camera at the start and once settled; `--compare --configs raster,loading-off` runs it with and without the loading screen |
| `die {maxS}` | kills the local player (`__dwell.die()`, a local world only), waits for its respawn, then (at most `maxS`) until it is playing again, and saves the respawn → playing seconds with the loading screen's wait in `report.json`'s `respawns` (ARCHITECTURE.md §6.3). `respawn-far` flies 700 m from the spawn and dies there; `--compare --configs raster,respawn-preload-off` runs it with and without the respawn preload (`?respawnPreload=off`) |
| `mark` | forgets the telemetry so far: the next `record` measures from here |
| `record {name}` | saves everything above under `name` |
| `shot {name}` | screenshot |
| `traceStart`, `traceStop {name}` | a Chrome performance trace (timeline, GPU, V8, the CPU profiler) and its profile summary |
| `time {t, moon?}` | shows the time of day `t` (0–1, 0 midnight) and optionally the moon's phase on this client (`?time=`, `?moon=`; DAY_NIGHT.md) |
| `timelapse {name, frames, from?, to?, waitS?}` | `frames` screenshots `<name>-00.png`… from time of day `from` (0) toward `to` (1), wrapping past midnight, waiting `waitS` (0.3 s) after each |
| `lod {on}` | draws the distant terrain (LOD) or not, live (`__dwell.setLod`; the sections still stream, so it comes back at once): what the full-detail chunks alone hold. `?lod=off` (or `?lod=0`) runs with no LOD at all |
| `water {quality?, debug?}` | sets the water quality (`low`, `medium`, `high`) and/or its cost-isolation switches (`?waterdbg=` names, `""` for none) mid-run, through `__dwell.setWater` |
| `burst {name, frames, clock?, maxFlicker?}` | a still camera's burst: `frames` screenshots `<name>-00.png`… back to back (the HUD hidden) and their **flicker** (`flickerOf`: the share of pixels whose brightness moves more than 3 of 255 from one frame to the next, and the mean move), printed and saved in `report.json`'s `bursts`; over `maxFlicker` (a share) the run fails. With `clock` the time of day runs on from it at the world clock's rate, set every animation frame (the bench's is otherwise held), so the light moves as in play. `shadows-still` reads the shadows' flicker against Off's (DAY_NIGHT.md §10). With `drift {position, yaw, pitch}` (after a `camera` step) the camera moves on by that much each frame (metres and degrees, at most 1 m and 1°): z-fighting shows only while the view moves (it stops with the camera), and a drift of millimetres and thousandths of a degree moves the view under a pixel by a fraction of a pixel, so surfaces swap only where they fight in depth. Such a burst is read by its **hue flips** (`hueFlipsOf`, also in every burst's result: the share of pixels, coloured in both frames, whose hue moves over 40° from one frame to the next — grass becoming dirt); over `maxHueFlips` the run fails. `lod-zfight` holds the LOD's z-fighting under a bar (ARCHITECTURE.md §6.6, "Known bug class") |
| `repeat {times, steps}` | runs `steps` `times` times (e.g. `edit-burst`'s 300 edits); no `mark`, `record`, `shot`, `timelapse` or trace steps inside |

Scenarios for comparing configurations, besides `idle-spawn`, `fly-low-fast`, `fly-low-faster` and
`jump-onto-block`: `look-horizon`
(grazing view along the ground; far), `look-down` (near), `edit-burst` (300 place/break edits;
near), `fly-high` (about 540 m up, flying; far) and `thermal-10min` (phone-oriented: ten minutes of
flight round a square, its first and last minute recorded, for thermal throttling).

## Comparing configurations (`--compare`)

```sh
npm run bench -- --compare idle-spawn look-horizon [--configs raster-half] [--configs-file F]
```

Runs each scenario once per **configuration** (`client/bench/configs.json`, checked by
`scripts/configs.ts`: unknown fields, ranges and duplicate names fail with the entry named), each in
a fresh page on the same seed and inputs. A configuration is a scale (`?scale=`: the browser
stretching a scaled canvas) and page parameters (`params`: dev switches such as `?outline=`):

| Config | Scale | Params |
| --- | --- | --- |
| `raster` (baseline) | 1 | — |
| `outline-silhouette`, `outline-creases`, `outline-blocks` | 1 | `?outline=` (ART_DIRECTION.md §7) |
| `raster-half` | 0.5 | — |
| `loading-off` | 1 | `?loading=off` (no loading screen on joining, ARCHITECTURE.md §6.3) |

A new configuration is one entry in the file; `baseline` names the one the others are compared
with and `deltaE` the colour threshold (CIE76, default 2.3, about just noticeable).

Output, in `OUT/`:

- `<scenario>/<config>/`: each run's `report.json` (with its page address),
  screenshots and traces, as a normal run's.
- `<scenario>/<shot>.<config>.side.png`: the baseline left, the configuration right;
  `<shot>.<config>.diff.png`: a heatmap of per-pixel ΔE (dim grey: equal; blue: under the
  threshold; red to yellow to white: over it, white at four times it). Stats per shot: identical %,
  % over the threshold, mean, p99 and max ΔE.
- `compare.json`: the configurations run, per scenario and
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

**TODO — edit-to-visible latency**: the telemetry can't tie a frame's uploads to
the edit that caused them (an edit is a request to the server, the reply re-meshes the chunk in a
worker, and the upload is counted per frame), so it isn't measured yet. It needs an edit sequence
number carried through the remesh and a telemetry sample (`edit.visible`, ms from the request to the
first frame drawn with its mesh).

The bench compares screenshots' colour only.

Kept scenarios include `idle-spawn`, `fly-low-fast`, `fly-low-faster`, `jump-onto-block`,
`jump-in-place`, and the motion effects' `sprint-straight` (shots standing, mid-sprint and after) and
`fly-fast-turn` (shots in fast flight, turning and diving: the wind's vanishing point following the
travel), and `climb-fast` (straight up at about 176 m/s with a trace: the chunks above arriving
over the ones below and the ones below dropped in batches; read its long tasks and worst frame —
0.22.9's flood light relights made 1.2–1.6 s long tasks there, 0.22.8 had none over 50 ms).

## Telemetry (`crates/dwell-client/src/telemetry.rs`)

One `Telemetry` per thread (the main thread's is exported). Three kinds of metric, named at the
call site:

- **Per-frame series** — `telemetry::add(name, value)` or `telemetry::span(name, || …)`: summed
  over the frame; kept for the last 3600 frames; summarised as mean/p50/p95/p99/max/sum.
  `interval` is the time between frames (`requestAnimationFrame` timestamps).
- **Event samples** — `telemetry::sample(name, value)`: one value per event (a job's time); the
  last 16384 per name.
- **Counters** — `telemetry::count(name, n)`: running totals.
- **Controller ticks** — one `TickSample` per predicted tick: feet, velocity, the camera's eye height (`eye`, after the step view's smoothing), state, grounded,
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
| `torchshadow.drawn` (count) | Lights whose torch shadow maps were drawn this frame (six passes each; kept maps aren't counted; `render/point_shadows.rs`, [`LIGHTS.md`](./LIGHTS.md) §4) |
| `lights.drawn` (count) | Dynamic lights drawn this frame: held torches, yours and other players', the nearest `maxLights` (`render/lights.rs`, [`LIGHTS.md`](./LIGHTS.md) §3) |
| `flames.drawn` (count) | Flames drawn this frame: placed light sources' and held torches' (yours in the arms view), at most `maxFlames` within `drawM` (`render/flames.rs`, [`LIGHTS.md`](./LIGHTS.md) §6) |
| `lod.fading`, `lod.retiring` (counts) | LOD swaps cross-fading this frame and the outgoing draws they add (`dwell-lod` `fade.rs`, ARCHITECTURE.md §6.6 Swaps) |
| `draw.acquire`, `draw.render`, `draw.present` | the surface texture, encoding and submitting the frame, presenting |
| `draw.skipped` (counter) | frames not drawn because two were still on the GPU (`render/pacing.rs`) |
| `gpu.water.tiles` (samples) | the water's noise tile (once) and caustic layers (each Medium/High frame with water; `render/water_tiles.rs`) |
| `upload.chunk`, `upload.section`, `upload.chunks`, `upload.sections`, `upload.bytes` | creating GPU buffers for chunk and LOD section meshes, how many, and their bytes |
| `job.<kind>.wait`, `job.<kind>.run` (samples) | per job kind (`chunk`, `chunk_mesh`, `bounds`, `section`, `section_mesh`, `verify`, `map`, `diff`): time queued in the pool before a worker had room; time from posting it to a worker until its result came back (the worker's run and the transfers; the pool collects results between frames) |
| `gpu.far`, `gpu.near`, `gpu.far.water`, `gpu.near.water`, `gpu.shadows`, `gpu.torchshadows`, `gpu.arms`, `gpu.composite`, `gpu.motion`, `gpu.frame` (samples) | GPU time of each render pass (the sun's or moon's shadow maps, every cascade's pass timed as one, DAY_NIGHT.md §10; the far and near scene passes, your first-person arms' pass (CHARACTERS.md; it resolves the frame's colour when drawn) — on Medium and High water, their opaque halves, with their water halves as `gpu.*.water`, WATER_RENDERING.md §8 W3 — the composite with the lines over it, or, while the wind is on, the composite into the finished image and the motion pass with the lines, MOTION_EFFECTS.md) and of the whole frame (the far pass's start to the composite's end), from timestamp queries (`render/timing.rs`), one sample per timed frame, read back a frame or two later. Only where the adapter has `timestamp-query`. On tile-based GPUs (Apple, phones) passes overlap, so the passes can add up to more than `gpu.frame` |
| `shadow.ktris` (samples) | Thousands of triangles drawn into the shadow maps this frame, every cascade's (DAY_NIGHT.md §10) |
| `shadow.drawn` (samples) | Shadow cascades drawn this frame: the nearest every frame, plus a far one when its kept map must be drawn again (DAY_NIGHT.md §10, Kept maps) |

Timer resolution: without cross-origin isolation the browser's clock is coarsened to 0.1 ms, so
sub-0.1 ms phases read 0 or 0.1. Chrome rounds GPU timestamps to 65.536 µs unless it runs with
`--enable-webgpu-developer-features`, which the bench passes (`--enable-features=WebGPUDeveloperFeatures`
does not lift the rounding).

The midplane crossing's roll (an experiment switch, owner 2026-10-21; BIFACIAL_WORLD.md §6,
MOTION_EFFECTS.md): `?crossingroll=<seconds>` sets how long the camera rolls upright after crossing
(0–5 s; the default is `motion.json`'s `crossingRollS`, 0.8 s), and `0` or `?crossingroll=snap`
snaps it upright on the crossing frame with no frames in between. `__dwell.setCrossingRoll(s)` sets
it mid-run (false for a value outside 0–5) and `__dwell.crossingRoll()` reads it; a `--compare`
config can pass it as `params: {"crossingroll": "snap"}`. Only the view's roll changes: the look
still continues across and the eye still swings across over `crossingRollS`.

Plants (docs/TRANSPARENT_BLOCKS.md): `__dwell.plants(wind, shown)` scales their wind and hides or
shows every cutout mesh, for an A/B of their cost at one camera pose.

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

## Play advanced (the main menu)

The main menu's **Play advanced** (beside New world) opens a local world by link with
overrides, for anyone: the world type, seed, spawn (face A, face B, either side of the portal),
time of day, moon phase, clock speed, ink outline, pixel art and distant terrain. Each is a URL
parameter the game already reads (`?world=`, `?seed=`, `?spawn=`, `?time=`, `?moon=`,
`?timespeed=`, `?outline=`, `?pixelart=`, `?lod=`), so the panel only builds the address
(`client/src/ui/playAdvanced.ts`, e.g. `?local=1&spawn=portal&time=0.7708`). Only the fields set are
passed; a field left empty keeps the game's default. A time of day holds the sky at that time (the
clock stops on this client, `?time=`), so the clock speed only matters without one. The world is
one file per type, seed and spawn (`local/world.ts` `parseLocalWorld`), saved like any other; a
spawn puts the player at it every time it opens, not where it left (owner, 2026-10-27). The
overrides themselves are not saved: every route drops them (`ui/launch.ts` `ROUTE_PARAMS`), so the
next Play from the menu starts clean. New world offers the same Spawn choices (`spawnChoices`) as
the world's first spawn.

The fields are data: `shared/play/overrides.json`, one entry per parameter (kinds `choice`,
`integer`, `number`, `clock`; a choice's `optionsFrom` takes its options from a look data file's
names, `outline` or `pixelart`, so a new mode there shows with no edit). `node shared/play/gen.mjs`
validates it and writes `client/src/ui/playOverrides.gen.ts`; CI fails on stale output, and a
parameter that chooses what the page opens (`play`, `join`, …) is refused. To add an override,
add an entry for a parameter the game reads.

## Water cost isolation (`?waterdbg=`)

`?waterdbg=caustics,foam` (names from `render/water.rs` `DEBUG_SWITCHES`: `ripple0`–`ripple2`, `glint`, `sky`,
`refraction`, `absorption`, `caustics`, `foam`, `sparkle`, `under`, `flat`, `all`) turns pieces of the water's
shading off, to attribute `gpu.*.water` and `gpu.frame` to them; a `--compare` configuration can set it with
`params` (`{ "name": "no-caustics", "params": { "waterdbg": "caustics" } }`), and a
scenario's `water` step switches it mid-run. On a GPU shared with other processes, compare a pass's `min` (in
every summary) rather than its p50. WATER_RENDERING.md §8 ("Cost") has the method and the numbers.
