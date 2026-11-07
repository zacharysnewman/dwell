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

## In the game: the performance line

Under the status line (Settings, On screen, Performance; `ui/perfLine.ts`): frames drawn a second, the CPU's
time per frame (`frame.cpu`, the last 60 frames), the GPU's (`gpu.frame`, the last 30 timed frames; `–` where
the browser has no `timestamp-query`, as many phones), the server's ticks a second (from the tick it sends)
and, in a local world, its time per tick. At 60 fps each has 16.7 ms: the one over it holds the frame back
(a GPU over it with the CPU under: draw less; the server under 60/s: the server, e.g. its water).

The frame rate reads **drawn of the browser's animation frames**, e.g. `38/60 fps (22 GPU-wait)` (0.50.2,
owner 2026-11-07: frames lost with every figure under 10 ms). The second number is the animation frames a
second (the display's refresh when the page keeps up: 60, or 120 on a ProMotion screen); the brackets
count the skipped ones by reason over the last 60 frames: `GPU-wait`, two frames still on the GPU
(`render/pacing.rs`), and `no-surface`. So: drawn equal to the animation frames but under the display's
rate, the main thread or the browser (`frame.late`, `frame.gap`, long tasks); `GPU-wait` skips with
`gpu.frame` low, the GPU's answers arriving late (`pacing.done`) or GPU work outside the timed passes.

Measured on an M1 (60 Hz, Chrome, seed 0, 2026-11-07): at 1700×1000 the idle spawn and a level-4 flight
drew 60/60 and 59/60 (`gpu.frame` 11.7 ms, `pacing.done` 16 ms p50, in flight ≤ 1 nearly always, 8 skips
in 10 s of flight). `pacing.done` runs 4–6 ms past `gpu.frame` (Chrome's delivery and the untimed
compositing). With `--unthrottled` (rAF ~100/s) the pacer skips 39 % and draws 61–66 a second, with
`pacing.done` 18–20 ms; three in flight drew 64–70 but doubled `pacing.done` to 31–34 ms (a frame more of
latency, the frames queueing), so the limit there is the GPU's real cost per frame (~14–15 ms with the
untimed work), not late answers, and `MAX_IN_FLIGHT` stays 2.

## Scenarios (`client/bench/scenarios.json`)

```json
{ "name": "fly-low-fast", "notes": "…", "seed": 7, "window": { "width": 1280, "height": 720 },
  "settings": { "detail": { "distanceM": 96, "pixelError": 2, "memoryMb": 256 } },
  "steps": [ { "do": "settle", "maxS": 120 }, { "do": "fly", "on": true }, … ] }
```

`seed` (default 7), `window`, `settings` (the settings menu's stored values, as in named views:
`detail`, `exposure`, `clouds`, `water` (`quality`, and optionally the flow's `flowRipples`, `flowFoam`, `flowParticles` switches, e.g. `false` to see one without the others), `motion` and `timeOfDay` (0–1, 0 midnight: `?time=`; the fog is fixed look data, not a setting) —
`{ "speedEffects": false }` turns the motion effects off, for a before/after of their cost in one
build),
`debug` (`?debug=1`), `scale`, `split` (`near` or `far`: which part of the view it measures, for
`--compare`'s split) and `face` (`"b"`: the player spawns on face B, the dev link `?spawn=b` —
BIFACIAL_WORLD.md §11; default `"a"`) and `portal` (`true`: the player spawns on the rim over the ladder of the portal nearest the centre, on `face`: the dev link `?spawn=portal` or `portal-b`, BIFACIAL_WORLD.md §12; `portal-a`, `portal-b` shoot a portal's openings, its shaft and the climb across the midplane, `portal-motes` its motes) are optional. Steps (`scripts/scenarios.ts`, checked: unknown
fields, ranges and names fail with the entry named):

| Step | Does |
| --- | --- |
| `wait {s}` | waits |
| `server {name, s}` | samples the local server's figures (`__dwell.serverStats()`) once a second for `s` seconds: its ticks' mean time, the worst `advance` call, and the water's (where it runs, its clock, owed and dropped time, its longest run, the batches queued, awake chunks, its copy of the world's size); printed, and saved in `report.json`'s `servers` under `name`. With a `--compare` configuration of `params: {"water": "inline"}` it measures the water on the server's thread against its own worker (WATER_SURFACE_GRID.md §2d) |
| `key {code, down}`, `hold {code, s}` | presses/releases a key (`KeyboardEvent.code`), or holds it for `s` |
| `look {yaw, pitch}` | turns the player's look (degrees; yaw 0 = +Z, 90 = +X, as `camera` and `goto`) |
| `fly {on}`, `flySpeed {level}` | creative flight and its speed level (L flies at ≥ 11 × 2^(L/2) m/s: 6 ≈ 88, 8 ≈ 176) |
| `puppets {count, at?, face?, held?}` | stand-in players (`__dwell.puppets`): `count` full bodies (≤ 256) in rows ahead of the player's feet offset by `at`, on `face` (1, or −1 for face B), each walking a small circle with a clip of its own (walk, sprint, slide, crouch, idle, jump, swim, climb), holding the hotbar's block with `held`; 0 removes them. For the frame cost of N bodies and screenshots of every clip without N clients ([`CHARACTERS.md`](./CHARACTERS.md)) |
| `arms {on}` | your first-person arms shown or hidden (the Show your arms setting, this page only) |
| `spyglass {on, step?}` | the spyglass zoomed to a step (0 the least, `shared/look/spyglass.json`) or out ([`SPYGLASS.md`](./SPYGLASS.md)); `spyglass-far` shoots far terrain unzoomed and at each step |
| `camera {position, yaw, pitch, aroundPlayer?, aboveFeet?, aroundFound?}`, `releaseCamera` | places the camera (the player stays), or returns it; with `aroundPlayer`, x and z are offsets from the player's feet (y absolute, or with `aboveFeet` an offset too: a height over the spawn's ground): a view wherever a flight ended, where the chunks are streamed; with `aroundFound`, x and z are offsets from the last `goto`'s place and y a height over its surface along its face's up |
| `goto {find, radius?, surface?, minDepth?, minArea?, minCurrent?, face?, height?, aim? \| yaw? \| toward? \| lookAt?, turn?, pitch?, move?, scope?, water?, maxS?}` | finds a place by what is there and puts the camera over it (`__dwell.goto`, [Finding places](#finding-places)): e.g. `{ "find": "water", "scope": "world", "water": "sea", "minDepth": 2, "minArea": 1000, "height": 2, "lookAt": "land", "turn": 180, "pitch": -3, "maxS": 120 }` (the nearest sea at least 2 m deep on the world map, the player moved there, the camera 2 m up with its back to the shore) or `{ "find": "water", "minCurrent": 0.1, "aim": "downstream", "pitch": -15 }`. Prints what it found and saves it in `report.json`'s `gotos`; **finding nothing within `maxS` fails the run** with what it looked for and where, so a scenario never measures the wrong scene. `water-views`, `water-shore`, `water-reflections` and `water-river` find their water this way |
| `edit {action}`, `select {slot}` | break/place at the crosshair; picks a hotbar slot |
| `region {op, from, to, up?}` | a region edit (the region tools, `__dwell.tool('cornerA@x,y,z')`) of the box between `from` and `to`, offsets in metres from the player's feet (`up` −1 on face B): `fill` with the selected hotbar block or `clear` to air — to build a scene, e.g. `tunnel-a`/`tunnel-b`'s hill and tunnel (DAY_NIGHT.md §11), and `torch-cave-a`/`torch-cave-b`'s tunnel and room at midnight with the torch held and placed (LIGHTS.md; `torch-night-puppets` records 8 torch-bearing stand-ins against none; `torch-scale-a` shoots the 1 m torch beside a stand-in at noon and midnight and the held flame by day and night); `torch-shadows-a`/`torch-shadows-b` place a torch before a stone pillar in the room and hold one too, recording the Torch shadows' cost with `--configs torch-shadows-off,torch-shadows-high`; `felling` builds a tree on the flat world and breaks its trunk, shooting the fall and the settled logs, FELLING.md) |
| `brush {command}` | drives the brush tool as its keys and clicks do (`__dwell.brush`: `toggle`, `preset:<name>`, `size:<n>`, `apply`, …; [`BRUSHES.md`](./BRUSHES.md)), in every world (creative; the sandbox with a `--compare` config `params: {"world": "sandbox"}`) |
| `settle {maxS}` | waits until the LOD has had nothing to generate, mesh or ask for, for 1 s |
| `until {what, maxS}` | waits until the view waits for nothing `behind` the camera (out of its frustum: `__dwell.state().lod.waitingOffView` 0), nothing `nearBehind` it within the full-detail distance (`waitingNearOffView` 0: the full detail behind is there) or nothing `ahead` (`waitingInView` 0), and saves the seconds since the page was opened in `report.json`'s `untils`; every run also saves `join` (page opened → playing, the loading screen's wait and its stages, ARCHITECTURE.md §6.3). `join-behind` times the join's timeline and shoots behind the camera at the start and once settled; `--compare --configs raster,loading-off` runs it with and without the loading screen |
| `die {maxS}` | kills the local player (`__dwell.die()`, a local world only), waits for its respawn, then (at most `maxS`) until it is playing again, and saves the respawn → playing seconds with the loading screen's wait in `report.json`'s `respawns` (ARCHITECTURE.md §6.3). `respawn-far` flies 700 m from the spawn and dies there; `--compare --configs raster,respawn-preload-off` runs it with and without the respawn preload (`?respawnPreload=off`) |
| `mark` | forgets the telemetry so far: the next `record` measures from here |
| `record {name}` | saves everything above under `name` |
| `shot {name}` | screenshot |
| `traceStart`, `traceStop {name}` | a Chrome performance trace (timeline, GPU, V8, the CPU profiler) and its profile summary |
| `time {t, moon?}` | shows the time of day `t` (0–1, 0 midnight) and optionally the moon's phase on this client (`?time=`, `?moon=`; DAY_NIGHT.md) |
| `timelapse {name, frames, from?, to?, waitS?}` | `frames` screenshots `<name>-00.png`… from time of day `from` (0) toward `to` (1), wrapping past midnight, waiting `waitS` (0.3 s) after each |
| `lod {on}` | draws the distant terrain (LOD) or not, live (`__dwell.setLod`; the sections still stream, so it comes back at once): what the full-detail chunks alone hold. `?lod=off` (or `?lod=0`) runs with no LOD at all |
| `view {clouds?, speedEffects?, distanceM?, pixelError?, smoke?, flowParticles?}` | sets other settings mid-run, not stored (`__dwell.setView`): the clouds (`off`, `low`, `high`), the speed effects (motion blur and streaks), the full-detail distance (m) and the LOD's pixel error, the blasts' smoke, the water's flecks; each one given replaces the menu's value. With `water` and `shadows`, one page can record every feature on and off in turn ([Cost profile](#cost-profile-m1-air)) |
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
| `frame.late` | how late the animation frame's callback started after its timestamp (other script, or the browser, holding the main thread) |
| `frame.gap` | from the end of the last `frame()` to this one's start: the page's other script and idle time (at 60 Hz with a 4 ms frame, ~12 ms) |
| `pacing.in_flight` | frames submitted and not yet answered by `on_submitted_work_done` when the frame asked to draw (`render/pacing.rs`) |
| `draw.paced`, `draw.no_surface` | 1 on a frame skipped because `MAX_IN_FLIGHT` were in flight, or for want of a surface texture |
| `pacing.done` (samples) | each frame's time from submit to the GPU's `on_submitted_work_done` answer (ms): the GPU's time plus Chrome's delivery; with two in flight a draw is skipped once this passes two animation frames |
| `net` | the session's network pumping (twice a frame) |
| `jobs.poll`, `jobs.apply`, `jobs.done` | collecting finished jobs; applying them (chunks into the world, meshes to the GPU); how many |
| `ticks`, `ticks.count` | the fixed-rate prediction ticks (controller + physics) and how many ran |
| `pieces.standIns` (samples) | the pieces stood in for in the prediction at each snapshot: those that could touch the local player soon ([`EXPLOSIVES.md`](./EXPLOSIVES.md) §3) |
| `game.draw` | the game's draw step: `mesh_dirty` and `lod` inside it |
| `mesh_dirty` | starting chunk mesh jobs (copying padded chunks), within its 4 ms budget |
| `lod` | the whole-world view's update (`LodSystem`): selection, job requests, stand-ins |
| `torchshadow.drawn` (count) | Lights whose torch shadow maps were drawn this frame (six passes each; kept maps aren't counted; `render/point_shadows.rs`, [`LIGHTS.md`](./LIGHTS.md) §4) |
| `lights.drawn` (count) | Dynamic lights drawn this frame: held torches, yours and other players', the nearest `maxLights` (`render/lights.rs`, [`LIGHTS.md`](./LIGHTS.md) §3) |
| `falls.drawn` (count) | Waterfalls drawn this frame (`render/waterfalls.rs`; docs/WATER_SURFACE_GRID.md §2e), at most `maxFalls` |
| `falls.find` (ms) | Finding the falls near the eye (`dwell_world::falls::falls_near`), every `refreshS` (0 on the other frames) |
| `flecks.drawn` (count) | Flecks drifting on moving water drawn this frame (`flecks.rs`, `render/flames.rs` `fleck_quads`; docs/WATER_SURFACE_GRID.md §2c), at most each liquid's `particles.count` (`cpuCount` on a CPU adapter) |
| `flames.drawn` (count) | Flames drawn this frame: placed light sources' and held torches' (yours in the arms view), at most `maxFlames` within `drawM` (`render/flames.rs`, [`LIGHTS.md`](./LIGHTS.md) §6) |
| `lod.fading`, `lod.retiring` (counts) | LOD swaps cross-fading this frame and the outgoing draws they add (`dwell-lod` `fade.rs`, ARCHITECTURE.md §6.6 Swaps) |
| `draw.acquire`, `draw.render`, `draw.present` | the surface texture, encoding and submitting the frame, presenting |
| `draw.skipped` (counter) | frames not drawn because two were still on the GPU (`render/pacing.rs`); per frame as `draw.paced` |
| `gpu.water.tiles` (samples) | the water's noise tile (once) and caustic layers (each Medium/High frame with water; `render/water_tiles.rs`) |
| `upload.chunk`, `upload.section`, `upload.chunks`, `upload.sections`, `upload.bytes` | creating GPU buffers for chunk and LOD section meshes, how many, and their bytes |
| `job.<kind>.wait`, `job.<kind>.run` (samples) | per job kind (`chunk`, `chunk_mesh`, `bounds`, `section`, `section_mesh`, `verify`, `map`, `diff`): time queued in the pool before a worker had room; time from posting it to a worker until its result came back (the worker's run and the transfers; the pool collects results between frames) |
| `gpu.far`, `gpu.near`, `gpu.far.water`, `gpu.near.water`, `gpu.shadows`, `gpu.torchshadows`, `gpu.arms`, `gpu.composite`, `gpu.motion`, `gpu.frame` (samples) | GPU time of each render pass (the sun's or moon's shadow maps, every cascade's pass timed as one, DAY_NIGHT.md §10; the far and near scene passes, your first-person arms' pass (CHARACTERS.md; it resolves the frame's colour when drawn) — on Medium and High water, their opaque halves, with their water halves as `gpu.*.water`, WATER_RENDERING.md §8 W3 — the composite with the lines over it, or, while the wind is on, the composite into the finished image and the motion pass with the lines, MOTION_EFFECTS.md) and of the whole frame (the far pass's start to the composite's end), from timestamp queries (`render/timing.rs`), one sample per timed frame, read back a frame or two later. Only where the adapter has `timestamp-query`. On tile-based GPUs (Apple, phones) passes overlap, so the passes can add up to more than `gpu.frame` |
| `gpu.gaps`, `gpu.gaps.copy` (samples) | The timed frame's time between its passes: each pass's start after the previous pass's end, summed (`render/timing.rs` `samples`), the untimed work inside `gpu.frame` (copies, clears, the GPU idle between passes); `.copy` the part just before the water halves, the scene colour's copy that WebGPU cannot time by itself. On Apple GPUs both read ~0: the passes overlap (each begins before the last ends), so the untimed time is outside `gpu.frame` (Chrome's compositing, `pacing.done` − `gpu.frame`) and a pass's cost is its ablation, not its span ([Cost profile](#cost-profile-m1-air)) |
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

Water flow (docs/WATER_SURFACE_GRID.md §2c): `__dwell.waterFlowAt(x, y, z)` gives the client's current there (m/s along x and z), the flecks near the eye, and the first few flecks' places. Waterfalls (§2e): `__dwell.waterFalls()` lists the falls the client found near the eye (each one's high column, way, width, lip and landing heights, flow per metre and lip speed). Blasts (docs/EXPLOSIVES.md §4): `__dwell.blasts()` lists the blasts whose effects are still going (each one's place and age, s); telemetry `blasts.puffs` counts the smoke, dust and fireball billboards drawn a frame.

From the console: `__dwell.telemetry()`, `__dwell.telemetryFrames()`, `__dwell.ticks()`,
`__dwell.telemetryReset()` (types in `client/src/main.ts`).

The start's stages (ARCHITECTURE.md §6.3, *The start's stages*): `__dwellStages()` (on `window`, from the
page's first moment, before `__dwell`) gives each loading stage that ran — `id`, `beganMs`, `endedMs` (null:
running) and `firstMs` (when its count first moved: the wait before the terrain counts) on the page's clock —
and every stage is a User Timing measure `dwell:stage:<id>` in a Chrome trace's Timings track. Every bench run
prints them after the join (`stages: page 0.00–0.03 s, game 0.03–0.08 s, …`) and saves them in
`report.json`'s `join.stages`.

## Interactive use

The same hooks work in any browser session: open `http://localhost:4173/dwell/?local=1&seed=7`
(`npx vite preview` in `client/`), then call `__dwell.*` from the console, or from an agent's
browser tools (e.g. the Chrome DevTools MCP: `evaluate_script`, `performance_start_trace`,
`take_screenshot`).

**Into a world by link, no clicks** (0.44.6): a local world link (`?local=1&seed=<n>`, `&world=` for
another generator) opens that seed's world file. Add:

- `fresh=1` — the world starts again from its seed: its file deleted first, whatever build saved it
  (no version lock, no prompt; every load of the link starts over).
- `at=x,y,z` — the player starts at that world point (feet, metres), the world's spawn moved there,
  however the world was left (`LocalCore::create_spawning`'s `at`).
- `time=0.45` (the time of day), `debug=1`, `lod=off` and the other page parameters as usual.

For example `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5&time=0.45` opens a new seed 0 world at
the owner's dam site; then `__dwell.fly(true)`, `__dwell.tool('cornerA@…')`, `tool('fill')` and
`placeCamera` set the scene up. `?new=1&seed=<n>&name=<name>` instead adds a new world to the
menu's list and opens it.

**The water report** (0.44.4): `dwell_sim::water::Water::report(at, radius)` describes the water near a
cell as JSON — each basin within `radius` cells: its id, level (world m), volume, inflow, what arrives,
whether it is filling or draining (`active`) and owes time, its bounds, its steady state as found
(`flow`, natural outlets' `law`, `level`), where water comes into it (`feeds`: each `at` a cell, `q`)
and its ways out that run (each one's `from` cell, `flow`, `kind` — face, floor, leak — target basin,
a pool of the graph nobody measured (`"pool river:row"`) or `lost`, `natural`, `contact`), and the
world's totals (`brought`, `lost`, `virtuals`: pools of the graph off their rest, ADR 0059). In a local world the block
dump (F6, or the debug overlay's button) carries it as `water` (the player's feet, 48 cells; the
worker asked, a second at most), and `__dwell.waterReport(x, y, z, radius?)` returns it (asked of
the water worker, ADR 0062); a native test or probe calls `server.water_report(...)` (or, inline,
`server.water().report(...)`). A diagnostic only: nothing in the game reads it.

**The server's figures** (G5, ADR 0062): `__dwell.serverStats()` gives the local server's last
second, as the local-mode worker reports it each second: `ticks`, `tickMeanMs` (each `advance`
call's wall time over the ticks it ran), `advanceMaxMs` (the worst call), `water` (`worker` or
`inline`), `phasesMs` (each tick phase's time summed over the second, ms, by `TICK_PHASES`' names —
`terrain`, `players`, `physics`, …; the worker's `performance.now` times them, 0.49.33; `terrain`, the
terrain collision's loads and rebuilds, apart from `players` since 0.49.34; since 0.49.35 the tick's
tail is `stream` (chunks sent), `worldgen`, `lod` and `save` (the autosave's batch and its encoding and
commit, spread over ticks in the browser, ARCHITECTURE.md §6.4), once one `stream+lod+save`), `counts` (what the ticks did
over the second, `Server::tick_counts`: `collisionBuilds`, terrain-collision chunk builds, and
`generatedOnTick`, chunks the tick generated itself rather than the server-jobs worker, and since
0.49.42 `collisionCells`, full-cube cells put into or taken out of the terrain collider, a little a
tick but those near players) and `waterStats` — `queue` (batches sent to the water and not yet answered), `runs`,
`ranS`, `owedS`, `droppedS` (water s: the last run's time run, what it still owes, all dropped past
`maxOwedS`), `busyMs` and `maxBusyMs` (a run's wall time, the longest), `waterTimeS` (its clock),
`awakeChunks`, `waterChunks`, `cellBytes` and `cellChunks` (its copy of the world), `overtaken`
(writes dropped because an edit got there first). Natively `Server::water_stats()`. Natively `Server::tick_phases()` gives each phase of the tick's wall time (ms, summed since the last call: `collect`, `edits`, `terrain`, `players`, `pieces`, `physics`, `settle`, …, `stream`, `worldgen`, `lod`, `save`; `TICK_PHASES`) and `Server::body_count()` the bodies; `dwell-sim` `tests/collapse.rs`'s measurements print them for a collapse and a blast (`measure_dynamite_at_the_dam`: dynamite at seed 0's dam site, each second's tick mean and worst, bodies, terrain-collision builds and phases). `?water=inline`
runs the water on the server's thread instead of its worker, and `?serverjobs=inline` the server's
chunk generation and LOD sections on its tick instead of the server-jobs worker (WATER_SURFACE_GRID.md
§2d).

**The edit log** (0.46.1, owner 2026-11-04): the local server keeps its last 256 block changes that
aren't the water's own (edits, region fills, collapses, settles; `Server::edit_log_json`), oldest
first, each one's `tick` (the server's, 60 a second), `cell`, `from` and `to` (canonical state
strings) and `reason`. The local world's water report carries it as `edits`, so the block dump has
it at `water.edits`: what was done near a reported spot, in order, to replay in a native probe.

## Finding places

Tests and benchmarks find their scene by what is there, not by coordinates that go stale when the
terrain changes (0.49.15; the water scenarios used to fly to a coast that a terrain change moved).
`crates/dwell-client/src/find.rs` does the search; the page exposes it as hooks and the bench as
the `goto` step.

- `__dwell.find(query, options?)`: the nearest loaded cell matching `query`, searched outward from
  the player's feet (or `from`) among the chunks the client holds, within `radius` m (256). A
  query is a block by name through the registry (`dwell:sand`; a name without a namespace is
  `dwell:`'s, so `water` is `dwell:water`, which matches its cells at any fill and flooded cells),
  a state (`dwell:ladder[facing=north]`), `fluid` (any fluid) or `land` (any solid block: with
  `surface`, dry ground, the shore seen from the water); an unknown name throws. Filters:
  `surface` (default true: open above — a fluid's top under air, a block's top in the open air,
  so `dwell:sand` is the beach, not the sea bed; a cell under a chunk not streamed in yet is not
  one), `minDepth` (m of matching cells under its
  surface), `minArea` (matching surface cells joined to it at its row: a pool, not a puddle),
  `minCurrent` (m/s: running water), `face` (`a`, `b`, `any`; default the start's). It returns
  `{ cell, at, surfaceY, face, block, depth, area, distance, current, flow }` (`at` the surface
  over the cell's centre, `flow` the mean moving current within 3 columns) or null. All of it is
  face-local: on face B "above" is −y and the surface is mirrored as it is drawn.
- `__dwell.goto(query, options?)` (a promise): finds it as `find` does and places the camera
  `height` m (1.6; negative: under the surface) over it, looking `aim` (`downstream`, `upstream`
  or `across` its flow; still water has none, and it keeps trying while the currents arrive),
  at a `yaw`, `toward` a point, or at the nearest `lookAt` from it among the loaded chunks (e.g.
  `land`: the shore; it keeps trying while the chunks there stream in), turned `turn` degrees more, at `pitch` (0, or toward the point). `move` moves the player
  there too, flying (a local world), so the full-detail chunks stream round it. `scope: 'world'`
  first searches the world map (the generator's columns, loaded or not: `water` any, `sea`,
  `river`, `lake`, at least `minDepth` deep, within `radius`, 30 km) outward from the player in
  rings, moves the player there and searches its cells once they stream in. `maxS` keeps trying
  (0; 60 for the world scope). It rejects with what it looked for when nothing is found.
- `__dwell.teleport(feet)`: moves the local player's feet there, at rest (a local world, on the
  face it is on; the local core's `teleport_local_player`, through the worker, not a network
  message). The client follows: a player the server moved while the prediction waits for terrain
  takes the server's place (`game.rs`), so it waits for the terrain there.
- `__dwell.camera()`: the placed camera (`placeCamera`, `goto`) or null; `view()` is the player's
  look, not a placed camera's.

Yaw is the same everywhere (`look`, `view`, `placeCamera`, `camera`, `goto`, `FaceFrame::look`):
degrees, 0 = +Z, 90 = +X; pitch is degrees toward the face's up. A placed camera doesn't turn the
player, so `view()` after `placeCamera` still reads the player's own look.

Seed 0's river at `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5` is a known river by link (the
player there with yaw 0 looks down it); `goto` finds water wherever the terrain puts it.

## Adding to it

- A metric: `telemetry::span("name", || …)` / `add` / `sample` / `count` where it happens; add it
  to the table above.
- A scenario: an entry in `bench/scenarios.json`.
- A comparison configuration: an entry in `bench/configs.json`.
- A step kind: its type and parser case in `scripts/scenarios.ts` (with a test), its action in
  `scripts/bench.ts`, a row in the steps table.
- A hook the steps need: a method on `__dwell` (`client/src/main.ts`) over a `Client` export
  (`crates/dwell-client/src/web/app.rs`).
- A kind of place to find: a filter on `find.rs`'s `Query` (with a both-faces test), its option
  in `client/src/debug/find.ts` and the `goto` step's parser.

## Comparing builds

Serve the other build on another port (for example a `git worktree` of the older commit, built with
`npm run build:wasm && npx vite build` and served with `npx vite preview --port 4174`) and run the
same scenarios against both: `npm run bench -- <scenario…> --base http://localhost:4174 --out A`,
then without `--base` into `B`, and compare the two `report.json` files' `telemetry.frames`
(and the screenshots, pixel by pixel). A worktree needs its own `target/`: cargo names a path
crate's artifacts the same wherever it is, so sharing one directory mixes the two builds.

## Native harness (`crates/dwell-native`)

A perf harness, not the native app (owner, 2026-11-07: "build a native app version and compare, if
you don't have the same debug tools as in browser, port them to work there too"): the same Rust
client, renderer and local-mode server as the browser build, in a native window (winit, wgpu on
Metal), so native and Chrome can be compared on one machine. No menus, HUD or audio; a dev tool,
never shipped or released (not in `shared/licenses`' shipped set). ARCHITECTURE.md §3 and §5.1.

```sh
export CARGO_TARGET_DIR=target-native        # its own target dir, if another build is running
cargo build --release -p dwell-native
target-native/release/dwell-native --seed 0 --at 2364.5,142,7443.5 --fresh
target-native/release/dwell-native --scenario pace-fly --scenarios FILE --out DIR
```

Options (`--help`): `--seed` (default 7, the bench's), `--at x,y,z` (the dev link `?at=`), `--world FILE`
(a redb file on disk; default the temp directory's `dwell-native-<seed>.redb`), `--fresh`, `--flat`,
`--size WxH` (points; default 1280×720), `--pixel-ratio` (default 1, the bench's Chrome at DPR 1),
`--present fifo|mailbox|immediate` (immediate is the bench's `--unthrottled`), `--workers N` (client
job threads of each kind; default min(4, cores − 2), as the page), `--server-threads N` (the server's
generation and LOD threads; default 1, as the one server-jobs worker).

Play: click the window to capture the mouse, Esc releases it; keys are the browser's (winit's key
codes are the DOM's `KeyboardEvent.code` names, fed to the same `KeyboardMouseInput`): WASD, Space,
Shift, F flies, digits pick, left and right click break and place. Once a second the perf line
(frames drawn, the frame's CPU time without the vsync wait, `gpu.frame`, the server's ticks a second
and mean tick time) is printed and shown in the window's title.

Telemetry: the same `telemetry` module records the same names (frame phases, `gpu.*` pass times
from Metal's timestamp queries, job waits and runs, the controller's ticks). Two differ by nature:
`draw.acquire` natively includes the vsync wait (wgpu's `get_current_texture` blocks until a
drawable is free; Chrome's does not, its compositor presents), so `frame.cpu` there reads ~16 ms at
60 Hz; `frame.busy` (native only) is `frame.cpu` without that wait, the figure to compare with
Chrome's `frame.cpu`. And `draw.render` natively includes wgpu's validation and Metal encoding on the
main thread, which Chrome does in its GPU process. The server thread reports what the local-mode
worker posts each second (ticks, mean tick, worst advance, the tick's phases and counts), saved per
record in the report's `servers`.

`--scenario NAME` runs a scenario from `client/bench/scenarios.json` (or `--scenarios FILE`) in a fresh
world and writes `OUT/NAME/report.json` in the bench's shape (`records` with `telemetry`, `frames`,
`ticks`, `state`; `settles`; `join.playingS`; `servers`), plus `native` (the skipped steps and the
settings). Steps done: `wait`, `key`, `hold`, `look`, `fly`, `flySpeed`, `mark`, `record`, `settle`,
`camera`, `releaseCamera`, `time`; skipped (browser-only so far): `shot`, `traceStart`/`traceStop`,
`goto`, `burst`, `repeat`, `puppets`, `die`, `until` and the rest. The world map is computed per
process and cached on disk (`$DWELL_MAP_CACHE`, else the temp directory): point `DWELL_MAP_CACHE` at
an empty directory to time a cold start as the bench's fresh browser context has it.

### Native vs Chrome (M1 MacBook Air, 60 Hz, 2026-11-07)

The same commit (0.50.0 plus the harness) both ways: Chrome by `npm run bench` (headed, DPR 1,
1280×720, vsync), native by `--scenario` (1280×720, pixel ratio 1, Fifo), on AC power. Each pair ran
back to back, after waiting for the 1-minute load to fall under 2.5 (it read 1.6–2.5 at each start; the
machine was shared with other builds), twice; the cells are run 1 / run 2. Scenarios (seed 0, fresh
world): `pace-idle` (8 s standing at the spawn once settled), `pace-fly` (10 s flying at level 4,
yaw 90, pitch −5) and `dam-view` (8 s at the dam site, `?at=2364.5,142,7443.5` / `--at`). The native
cold start had an empty `DWELL_MAP_CACHE`. Chrome's server tick figures are not in its report
(its records have no `server` step); its perf line read ~0.1–0.2 ms a tick in play.

| | Chrome idle | Native idle | Chrome flight | Native flight | Chrome dam | Native dam |
| --- | --- | --- | --- | --- | --- | --- |
| Frames drawn a second | 59.8 / 59.8 | 59.9 / 60.0 | 59.8 / 59.9 | 60.1 / 59.9 | 59.9 / 59.8 | 61.2 / 61.0 |
| Frame interval p50 (ms) | 16.7 / 16.7 | 16.65 / 16.63 | 16.7 / 16.7 | 16.69 / 16.66 | 16.7 / 16.7 | 16.56 / 16.60 |
| Frame interval p95 (ms) | 18.6 / 18.7 | 17.2 / 18.0 | 18.6 / 18.6 | 18.0 / 17.9 | 18.6 / 18.6 | 17.7 / 17.7 |
| CPU per frame, mean (ms) ¹ | 3.65 / 3.55 | 2.42 / 4.61 | 4.29 / 4.33 | 4.52 / 4.70 | 3.74 / 3.91 | 4.70 / 4.58 |
| CPU per frame, p95 (ms) ¹ | 4.1 / 4.0 | 2.9 / 5.5 | 11.1 / 10.9 | 7.7 / 8.1 | 4.3 / 4.4 | 5.5 / 5.5 |
| `draw.render` mean (ms) | 2.14 / 2.05 | 1.64 / 3.38 | 1.90 / 1.92 | 2.78 / 2.96 | 2.11 / 2.18 | 3.38 / 3.27 |
| `gpu.frame` mean (p95) (ms) | 9.8 (11.2) / 9.8 (11.0) | 10.8 (11.9) / 10.3 (11.4) | 9.5 (13.4) / 9.5 (13.5) | 10.3 (14.4) / 10.2 (14.1) | 11.8 (12.5) / 11.8 (12.9) | 12.6 (13.4) / 12.5 (13.3) |
| Server tick mean (ms), ticks/s | – | 0.17 / 0.10, 60 | – | 0.14 / 0.12, 60 | – | 0.12 / 0.13, 60 |
| Start to playing (s) ² | 7.11 / 7.24 | 7.06 / 5.64 | 7.40 / 7.19 | 5.70 / 5.75 | 9.05 / 6.32 | 4.70 / 4.69 |
| Settle (s) | 15.0 / 15.3 | 17.0 / 12.9 | 15.5 / 15.2 | 12.6 / 12.4 | 17.0 / 15.0 | 12.3 / 13.1 |

¹ Chrome's `frame.cpu`; native `frame.busy` (without the vsync wait). ² Chrome: page opened to
`terrainReady`; native: process start to the same.

What it says (M1 Air at 60 Hz): both hold 60 fps on these views, and the frame is the GPU's
(9.5–12.5 ms of 16.7), which native does not change: it is the same WGSL on the same Metal GPU,
and natively it reads 0.5–0.8 ms more (Chrome's Dawn may schedule passes differently; not looked
into). The CPU per frame is about the same: native skips WASM and the JS boundary but does wgpu's
validation and Metal encoding on the main thread (`draw.render` 2.8–3.4 ms against Chrome's ~2 ms,
whose GPU process does that work), so the means come out even; the flight's spikes are lower
natively (p95 ~8 against ~11 ms: chunk uploads and job results applied faster). Native frame pacing
is a little tighter (interval p95 17.7–18.0 against 18.6) and the start and settle are faster
(4.7–5.8 s to playing against 6.3–9.1 s; the world map and the server's generation on native
threads). A gain in drawn frames would need a GPU-bound case where native is cheaper, which these
views do not show; the GPU-bound comparison below looks for one.

### Native vs Chrome, GPU-bound (M1 MacBook Air, 2026-11-07)

The owner's "GPU heavy" follow-up: a view where the GPU is the limit, so any native gain in drawn
frames would show. Same commit (0.50.5) both ways, on AC power, each pair back to back after the
1-minute load fell under 2.5. The Air's full-screen drawing buffer, 2560×1600: Chrome a 1280×800
viewport at `?scale=2` (a `--compare` configuration's `scale`; the page's pixel ratio is
min(DPR, 2) × scale), native `--size 1280x800 --pixel-ratio 2`. Settings High (`settings`: water
`high`, shadows `high`, clouds `high`), the clock held at noon (`time` 0.5). Two views, seed 0,
fresh world spawned at the dam (`at` / `--at 2364.5,142,7443.5`): `gpu-dam`, the camera 6 m over
the river by the dam looking down at 40° (`camera`, the water at its heaviest: refraction,
caustics, reflections across most of the screen), 8 s; and `gpu-fly`, flying at level 6 for
10 s (yaw 90, pitch −10; motion blur on). Unthrottled runs twice (cells run 1 / run 2), the
vsync run once. The scenarios were a scratch file, not in `client/bench/scenarios.json`
(`goto` is browser-only, so the dam view is a fixed `camera`).

Unthrottled (Chrome `--unthrottled`, native `--present immediate`):

| | Chrome dam | Native dam | Chrome flight | Native flight |
| --- | --- | --- | --- | --- |
| Frames drawn a second | 42.8 / 42.4 | 39.7 / 39.7 | 54.8 / 55.1 | 51.0 / 51.4 |
| Drawn-frame interval p50 (ms) | 22.7 / 22.8 | 24.9 / 25.0 | 21.9 / 21.6 | 18.8 / 18.8 |
| Drawn-frame interval p95 (ms) | 26.1 / 39.2 | 25.4 / 25.2 | 24.5 / 24.4 | 19.3 / 19.1 |
| `gpu.frame` mean (ms) | 20.8 / 21.1 | 25.1 / 25.2 | 14.9 / 14.8 | 19.6 / 19.5 |
| `gpu.frame` p95 (ms) | 21.4 / 22.0 | 25.7 / 25.7 | 19.8 / 20.0 | 24.9 / 24.8 |
| `gpu.composite` mean (ms) | 16.3 / 16.5 | 19.3 / 19.3 | 7.8 / 7.8 | 11.1 / 11.1 |
| CPU per drawn frame, mean (ms) ¹ | 3.96 / 4.06 | 2.07 / 2.08 | 3.87 / 3.75 | 2.10 / 2.07 |
| CPU per drawn frame, p95 (ms) ¹ | 4.80 / 5.00 | 2.16 / 2.16 | 7.50 / 7.60 | 2.28 / 2.13 |
| Loops skipped a second ² | 45 / 45 | 3149 / 3126 | 42 / 43 | 3015 / 3024 |

Vsync (Chrome default, native `--present fifo`):

| | Chrome dam | Native dam | Chrome flight | Native flight |
| --- | --- | --- | --- | --- |
| Frames drawn a second | 42.3 | 40.7 | 57.1 | 52.2 |
| Drawn-frame interval p50 (ms) | 17.7 | 24.4 | 16.7 | 18.5 |
| Drawn-frame interval p95 (ms) | 35.2 | 24.7 | 18.8 | 19.2 |
| `gpu.frame` mean (ms) | 20.8 | 24.5 | 14.8 | 19.0 |
| `gpu.frame` p95 (ms) | 21.4 | 25.1 | 19.6 | 24.0 |
| `gpu.composite` mean (ms) | 16.3 | 19.7 | 7.8 | 11.8 |
| CPU per drawn frame, mean (ms) ¹ | 4.29 | 1.68 | 4.03 | 1.88 |
| CPU per drawn frame, p95 (ms) ¹ | 5.40 | 1.98 | 8.70 | 2.09 |
| Loops skipped a second ² | 18 | 1585 | 3 | 1207 |

¹ Chrome's `frame.cpu`, native `frame.busy`, over the frames that drew (the raw frames with a
`draw.render`). ² `draw.skipped`: loop passes that did not draw because two frames were still on the
GPU (`render/pacing.rs`). Chrome's loop is `requestAnimationFrame`; the native loop is winit's
`ControlFlow::Poll`, which spins, so its count is thousands a second and not comparable. Drawn
frames a second are `gpu.frame`'s sample count (one per drawn frame) over the record's seconds.

What it says: **no, native does not draw more frames when the GPU is the limit; it draws a few
fewer.** At the dam both are GPU-bound at about 40 fps (Chrome ~42, native ~40, 6% fewer); in the
flight Chrome ~55, native ~51 (7% fewer). The same WGSL takes 3.5–4.5 ms more GPU time natively (dam ~20.8 against
~24.5–25 ms), and almost all of it is in the full-screen passes that write the final image:
`gpu.composite` (+3–3.5 ms) and in flight `gpu.motion` (+3.5 ms), while the scene passes (`near`,
`near.water`, `far`) cost the same. The CPU per drawn frame is half Chrome's natively (~2 against
~4 ms, p95 ~2.2 against 5–9 ms) but at 20–25 ms of GPU a frame that does not move the frame rate.
Under vsync the picture is the same: Chrome ~42 fps at the dam with frames alternating one and two
refreshes (interval p50 17.7, p95 35 ms), native ~41 fps at an even ~24.4 ms (Fifo in a window
does not hold it to whole refreshes); in flight Chrome 57, native 52. Neither reaches 60 at the dam. Not looked into: why the
full-screen passes cost more natively. Leads are the surface
(native writes straight into the `CAMetalLayer` drawable, Bgra8Unorm; Chrome into its canvas
texture, which its compositor then draws), and the native loop spinning a core (`ControlFlow::Poll`)
on a chip whose CPU and GPU share power and memory bandwidth. Whatever native gains, it is not GPU
time, so the GPU budget is the one to spend work on.

## Cost profile (M1 Air)

The owner's "can you truly benchmark the game to determine our most costly pieces … each feature by CPU and
GPU" (2026-11-07), on the target machine (M1 MacBook Air, 60 Hz, target a steady 60 fps), before any
upscaling work. Main at 0.50.6 plus this change's telemetry, Chrome on the GPU, on AC power, `--unthrottled`
(so a frame shows its cost, not the vsync wait), the full-screen drawing buffer 2560×1600 (a 1280×800 window
at `?scale=2`), seed 0, the clock at noon. Defaults on a desktop: water Medium, shadows High, clouds High,
speed effects on, full-detail distance 256 m, LOD pixel error 4; "High" is the same with water High. The
1-minute load was held under 3.3 (a system process, `ANECompilerService`, held one core during part of the
session; the dam pair ran under 2.5).

**Method.** The per-pass `gpu.*` spans cannot be added up on the M1: its passes overlap (each begins before
the one before ends), so `gpu.near.water` or `gpu.composite` reads 13–16 ms at the dam while the whole frame is
19.8. The new `gpu.gaps` (the time between passes) reads 0.00 in every record: nothing in `gpu.frame` is
outside a pass. So **a feature's GPU cost here is its ablation**: one page per scene, settled, then every
feature switched off in turn mid-run (the new `view` step with `water` and `shadows`), 1.5–3 s to settle and
4 s recorded each, the default recorded again at the end (drift: +0.3 ms at the dam). The scenarios and
configurations were scratch files (`cp-dam`, `cp-fly`, `cp-idle`, `cp-build`, `cp-blast`, `cp-trace`), not
checked in.

### The dam, 2560×1600 and 1280×800

The camera 6 m over the river by the dam looking down 40° (the water at its heaviest), `gpu.frame` mean (ms):

| Record | Full 2560×1600 | Δ | fps | Half 1280×800 | Δ |
| --- | --- | --- | --- | --- | --- |
| Default | 19.84 | | 44.6 | 8.78 | |
| High (water High) | 20.73 | +0.89 | 42.9 | 9.24 | +0.46 |
| Water Low | 14.87 | −4.98 | 56.3 | 7.19 | −1.58 |
| Water flecks off | 19.62 | −0.23 | 44.7 | 8.74 | −0.04 |
| Shadows Low | 19.72 | −0.12 | 44.7 | 8.72 | −0.05 |
| Shadows Off | 16.95 | −2.90 | 50.9 | 7.53 | −1.24 |
| Clouds Low | 20.07 | +0.23 | 43.9 | 8.77 | 0.00 |
| Clouds Off | 19.41 | −0.43 | 45.6 | 8.66 | −0.12 |
| Speed effects off | 19.76 | −0.09 | 44.7 | 8.74 | −0.03 |
| LOD pixel error ×2 (8) | 19.98 | +0.13 | 44.6 | 8.92 | +0.14 |
| Full-detail distance ½ (128 m) | 19.05 | −0.80 | 45.9 | 7.92 | −0.86 |
| Default again | 20.14 | +0.30 | 43.6 | 8.95 | +0.17 |

Half the resolution per axis (a quarter of the pixels) takes the frame from 19.8 to 8.8 ms: with a cost
fixed per frame F and one per pixel P, F + P = 19.8 and F + P/4 = 8.8, so **P ≈ 14.7 ms scales with the
pixels and F ≈ 5.1 ms does not**. The water's and the shadows' costs are mostly per pixel (5.0 → 1.6,
2.9 → 1.2 at a quarter of the pixels); the full-detail distance's is geometry (0.8 both ways).

### Over the valley, 2560×1600

Meant as a level-6 flight; **the bench's flight stalled in every attempt** (three runs: look pitch −10, +2 and
+8, yaw 90 and 0): the player moved 25–50 m in the first record and then hung still (`state().feet` the same
in every later record, `gpu.motion` drawn only in the first), so this is a view from ~20 m over the valley at
the dam, looking along it to the far terrain, not a flight; nothing streamed (no jobs, no uploads) except when
the LOD came back. The 0.50.6 GPU-bound comparison's `gpu-fly` used the same steps (look pitch −10), so its
"flight" may have been the same stall. Why the held key stops after a `record` was not looked into. The
earlier flights in [the performance line](#in-the-game-the-performance-line) (1700×1000, level 4: 59/60 fps)
moved.

| Record | `gpu.frame` | Δ | fps |
| --- | --- | --- | --- |
| Default | 16.63 | | 51.4 |
| High (water High) | 16.38 | −0.25 | 51.9 |
| Water Low | 15.15 | −1.49 | 55.2 |
| Water flecks off | 16.25 | −0.39 | 52.2 |
| Shadows Low | 16.29 | −0.35 | 52.4 |
| Shadows Off | 14.24 | −2.39 | 57.7 |
| Clouds Low | 16.02 | −0.62 | 52.6 |
| Clouds Off | 15.68 | −0.95 | 54.2 |
| Speed effects off (still) | 16.62 | −0.01 | 51.2 |
| LOD pixel error ×2 | 16.47 | −0.16 | 51.9 |
| Full-detail distance ½ | 14.68 | −1.95 | 56.6 |
| Default again | 16.49 | −0.15 | 51.6 |

The idle spawn at the dam (the player's own view along the river, arms drawn, `cp-idle`): `gpu.frame` 26.3 ms
at the default and 28.6 at High, 34 and 32 fps: the heaviest view measured (more water on screen than the
dam close-up). Building (`cp-build`, 40 place-and-break at the spawn): 18.8 ms, 47 fps.

### The CPU

The main thread per loop (`frame.cpu`, mean / p95 ms) is small next to the GPU everywhere measured:

| Scene | `frame.cpu` | `lod` | `draw.render` | `ticks` | `jobs.apply` | `mesh_dirty` | uploads | Long tasks > 16 ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Dam, default | 2.51 / 4.1 | 1.16 | 1.05 | 0.08 | 0 | 0 | 0 | 0 |
| Dam, High | 2.65 / 4.5 | 1.23 | 1.10 | 0.08 | 0 | 0 | 0 | 0 |
| Build (40 place + break) | 2.89 / 6.1 | 1.01 | 0.86 | 0.11 | 0.51 / 2.6 | 0.21 / 2.0 | 0.50 / 4.0 | 0 |
| LOD switched back on (sections arriving) | 2.93 / 5.0 | 1.64 | 0.73 | 0.05 | 0.31 / 1.4 | 0 | 0.30 / 1.3 | 0 |
| Red dynamite blast, 8 s from lighting | 2.83 / 7.3 | 0.98 | 0.79 | 0.08 | 0.33 / 2.2 | 0.39 / 3.2 | 0.32 / 2.6 | 0 |
| The 12 s after it (pieces settling) | 1.47 / 3.8 | 0.90 | 0.34 | 0.02 | 0.08 / 0.6 | 0.02 | 0.07 / 0.4 | 0 |

A Chrome trace of the dam at High (3 s, `cp-trace`): the page's main thread busy 505 ms (168 ms a second,
~2.9 ms a loop), of which **`LodSystem::update` 214 ms (42 %)** — `touch_kids` 45 ms self, `visit` 138
inclusive, run every frame though nothing changed — and `Renderer::render_frame` 180 ms (36 %; its
`shadow_candidates` 40, `mesh_of` 32, `record` 22, `wasm-to-js` 51 inclusive). The workers: 31 ms in the
3 s. Turning shadows off takes `draw.render` from 1.05 to 0.85 ms (the shadow casters' selection and draws).

Workers (`job.<kind>.run`, mean ms per job, the worker's run plus transfers): building, `chunk_mesh` 8.4
(395 jobs in 19 s), `section` 10.7, `section_mesh` 6.8; the LOD's sections arriving, `section` 15
and `bounds` 15.5. None over a frame on the main thread: they run off it.

The server (local world, its worker; `server` step, per second). In play without pieces: 60 ticks a second,
the tick's mean 0.15–0.27 ms; `players` 2–5 ms and `physics` 2–4 ms a second, `save` 3 ms every ~5 s (the
worst `advance` 3 ms). The water worker on the settled river: ~300 runs a second, its longest 6.3 ms, nothing
owed or dropped. **The red dynamite blast** (`cp-blast`: placed and lit at the seed 0 spawn's feet, the player
killed by it and respawned, the camera 14 m back): still 60 ticks a second, but the tick's mean rises from 0.2
to 3.9–6.5 ms for the first 4 s and the worst `advance` to 61–68 ms; per second `physics` 94–236 ms and
`terrain` (the pieces' collision builds) 45–149 ms, `pieces` 6–9; in the next 12 s physics 31–169 and terrain
8–128 ms a second, the tick back under 1.3 ms after ~8 s. That is the local server's worker, off the page's
main thread: the page saw no long task, `frame.cpu` p95 7.3 ms (`mesh_dirty` p95 3.2: the crater's chunks
re-meshed, 582 `chunk_mesh` jobs at 14 ms on the workers). On the GPU the pieces add shadow casters
(`gpu.shadows` 0.5 → 2.2–2.5 ms span). **Flooding was not measured**: the dam view's spawn stands in the
river (the dynamite would not place there), and the blast at the dry spawn has no water to let in; a breach
scenario needs a `goto` to a bank first (left for a follow-up).

### Memory (`memory()`, the dam spawn at 2560×1600)

1.58 GB in all: GPU meshes 550 MB, the drawing buffers 393 MB (at 2560×1600; a quarter at 1280×800),
the world-generation workers 350 MB (four at 75–79 MB, four at 9–10), the client core 185 MB, the server
79 MB, the JS heap 19 MB.

### What is untimed

`pacing.done` (submit to the GPU's answer) runs 11 ms past `gpu.frame` at the dam (31.0 against 19.8) and
6 ms at half resolution (14.6 against 8.8): with two frames in flight it includes the queued frame, and
Chrome's own work (the canvas's compositing into the page, the present) is outside our command buffer and
untimeable from the page; it grows with the drawing buffer. Inside `gpu.frame` nothing is untimed on the M1
(`gpu.gaps` 0.00). One gap in the timing: with the LOD switched off (`lod {on: false}`) no `gpu.frame` is
recorded (the frame's far pass is missing, so the frame total is dropped), so the LOD's GPU cost is
read from `near-half` and `pacing.done` instead.

### Ranked: each feature's cost on the Air

GPU ms are ablation deltas at 2560×1600 (feature on minus off), CPU ms the main thread's per frame unless
named; share is of the 16.7 ms budget at the default, GPU.

| # | Piece | Runs on | GPU ms, dam / valley (default) | GPU at High | CPU ms | Share of 16.7 ms (GPU, dam) |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **The pixel count** (every per-pixel pass at 2560×1600) | GPU | ~14.7 of 19.8 (½ resolution: 8.8) | ~15 | Chrome's compositing of the canvas grows with it (`pacing.done` − `gpu.frame` 11 → 6 ms) | 88 % |
| 2 | Base scene: near chunks, LOD terrain, sky, fog, the composite, arms (no off switch; the rest of the frame) | GPU passes `near`, `far`, `composite`, `arms` | ~10.5 / ~9.4 | same | main thread: `draw.render` 0.8–1.1 (encoding), `lod` 1.0–1.2 (the LOD's selection, every frame) | 63 % |
| 3 | **Water, Medium** (refraction, caustics, reflections; vs Low) | GPU (`near.water`, `far.water`, `water.tiles`) | **5.0 / 1.5** | +0.9 / −0.25 more (High) | main ~0; the water grid on the server's water worker ~300 runs a second, longest 6.3 ms | 30 % |
| 4 | **Sun shadows, High** (vs Off) | GPU (`shadows`, `lodshadows`, the receivers' sampling) | **2.9 / 2.4** (Low saves only 0.1–0.35) | same | main 0.2–0.3 (`draw.render` 1.05 → 0.85: casters selected and drawn) | 17 % |
| 5 | Full-detail distance 256 m (vs 128) | GPU (near pass geometry) | 0.8 / 1.95 | same | ~0 | 5 % |
| 6 | Bodies: a red dynamite blast's pieces | server worker; GPU shadow casters | `gpu.shadows` span 0.5 → 2.2–2.5 | same | server: physics 94–236 + terrain collision 45–149 ms a second (tick 3.9–6.5 ms mean, worst 68 ms); main: `mesh_dirty` p95 3.2, `frame.cpu` p95 7.3 | ~10 % (while it lasts) |
| 7 | Clouds, High (vs Off) | GPU (sky in the composite) | 0.4 / 0.95 | same | ~0 | 2 % |
| 8 | Water flecks (vs off) | GPU | 0.2 / 0.4 | same | ~0 | 1 % |
| 9 | LOD pixel error 4 (vs 8) | GPU, LOD workers | 0.1–0.2 (noise) | same | ~0 settled | <1 % |
| 10 | Speed effects (the motion pass) | GPU | 0 still (not drawn); in motion not isolated (the flight stalled) | | ~0 | – |
| 11 | Chunk meshing and LOD sections (building, streaming) | workers | – | | `chunk_mesh` 8–18 ms a job, `section` 11–23, `section_mesh` 6–10, off the main thread; main: `jobs.apply` 0.3–0.5 (p95 2.6), uploads 0.3–0.5 (p95 ~4) while they arrive | – |
| 12 | The controller and prediction | main | – | | `ticks` 0.05–0.11 | – |
| 13 | The server at rest (players, physics, water, save) | server worker | – | | tick 0.15–0.27 ms, 60 a second; `save` 3 ms every ~5 s | – |
| 14 | Network | main | – | | `net` 0.02 | – |
| – | Stars | GPU | 0 at noon (night only; not measured) | | | |
| – | Fog, the HUD, sounds | GPU shaders / DOM / audio | no switch: fog is inside the terrain and composite shaders, the HUD inside Chrome's compositing, audio on its own thread; not measured | | | |

Memory: 1.58 GB (above); the drawing buffers' 393 MB scale with the pixel count too.

### What to fix first to hold 60 fps on the Air

At the default the dam reads 19.8 ms of GPU (44 fps), the valley 16.6 (51 fps), the river spawn 26.3 (34
fps); the CPU is 2.5–2.9 ms of the 16.7 everywhere, the server under 0.3 ms a tick except in a blast. So the
GPU's per-pixel work is the frame, and in that order:

1. **Draw fewer pixels** (the queued upscaling): ~74 % of the dam's GPU frame scales with the pixel count.
   At ¾ per axis (1920×1200, 56 % of the pixels) the estimate is 19.8 → ~13.3 ms (−6.5) at the dam and 26.3 →
   ~18 at the spawn; at ⅔ (1707×1067) −8.2 ms. The only change that reaches 60 fps in every view measured.
2. **Water Medium's per-pixel cost**: 5.0 ms at the dam (1.6 at a quarter of the pixels). Draw its
   refraction, caustics and reflections at half resolution, or drop the scene-colour copy where the water
   does not refract: estimated −2 to −3 ms where water fills the screen.
3. **Make Shadows Low a real tier**: High costs 2.4–2.9 ms, Low saves only 0.1–0.35. Fewer or smaller
   cascades and no distant shadows on Low, and Low as the default on an integrated GPU: estimated −1.5 to
   −2 ms.
4. **The full-detail distance**: 128 m saves 0.8–2 ms (the most over far terrain). A default of ~192 m on an
   integrated GPU: estimated −0.5 to −1 ms.
5. **The CPU is not the limit on the Air**, but the LOD's selection runs every frame though nothing changes
   (`lod` 1.0–1.2 ms, 42 % of the main thread in the trace: `touch_kids`, `visit`): skipping it while the
   camera and the sections are still saves ~0.8 ms a frame (more on phones).

Not worth it for frame rate: clouds (0.4–1 ms), flecks (0.2–0.4), the LOD pixel error, the speed effects
when still. The blast's cost is the server's (ticks of 4–6.5 ms, a 68 ms worst, still 60 a second) and
+1.7 ms of shadow casters on the GPU while the pieces fly.

Not covered here: a moving flight (the bench's held key stalled, above), a flood after a breach, the
first-person arms and the stand-in characters on their own, night (stars, torches). Re-run the same
scratch scenarios after each fix to read its gain.

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
