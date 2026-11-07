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
| `flecks.drawn` (count) | Flecks drifting on moving water drawn this frame (`flecks.rs`, `render/flames.rs` `fleck_quads`; docs/WATER_SURFACE_GRID.md §2c), at most each liquid's `particles.count` (`cpuCount` on a CPU adapter) |
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

Water flow (docs/WATER_SURFACE_GRID.md §2c): `__dwell.waterFlowAt(x, y, z)` gives the client's current there (m/s along x and z), the flecks near the eye, and the first few flecks' places.

From the console: `__dwell.telemetry()`, `__dwell.telemetryFrames()`, `__dwell.ticks()`,
`__dwell.telemetryReset()` (types in `client/src/main.ts`).

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
`inline`) and `waterStats` — `queue` (batches sent to the water and not yet answered), `runs`,
`ranS`, `owedS`, `droppedS` (water s: the last run's time run, what it still owes, all dropped past
`maxOwedS`), `busyMs` and `maxBusyMs` (a run's wall time, the longest), `waterTimeS` (its clock),
`awakeChunks`, `waterChunks`, `cellBytes` and `cellChunks` (its copy of the world), `overtaken`
(writes dropped because an edit got there first). Natively `Server::water_stats()`. Natively `Server::tick_phases()` gives each phase of the tick's wall time (ms, summed since the last call: `collect`, `edits`, `players`, `pieces`, `physics`, `settle`, …; `TICK_PHASES`) and `Server::body_count()` the bodies; `dwell-sim` `tests/collapse.rs`'s measurements print them for a collapse and a blast. `?water=inline`
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
