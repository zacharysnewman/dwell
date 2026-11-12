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
npm run bench -- --warm fly-low-fast             # the warm client (below): built only if stale
npm run bench -- --warm --spot starter:broadleaf # a saved spot's screenshot (Screenshots of a place)
npm run build:wasm && npm run build              # or build this checkout's client by hand…
npm run bench -- fly-low-fast                    # …and run it (one or more scenarios; none: all)
```

**The warm client** (`npm run bench:warm`, `scripts/warmClient.ts`): builds the WASM module and the
client once into a cache keyed by the git tree hash of the sources the build reads (`scripts/warmKey.ts`
`SOURCE_PATHSPECS`: the workspace, `shared/`, the client's page sources, as they are on disk, committed or
not; Markdown, `client/bench`, `client/scripts`, `client/e2e` and `*.test.ts` left out, so a docs- or
bench-only change doesn't rebuild), and serves it with a **long-lived** `vite preview` on **:4174** that
later runs, worktrees and agents reuse. The cache is `$DWELL_BENCH_CACHE`, else the main checkout's
`target/bench-client/` (`<key>/dist` per build, the newest 3 kept; `preview.json` the server's pid, port
and key): shared by every worktree, like the warm `CARGO_TARGET_DIR`. A stale key rebuilds (cargo into
`CARGO_TARGET_DIR`, wasm-bindgen into this checkout's `src/pkg`, `vite build` into the cache, no
typecheck) and restarts the server on the new build. `npm run bench:warm -- --status` prints the key,
whether it is built and the server; **`npm run bench:warm -- --stop` stops the server** (sweep it when
done for the day). `--warm` on a bench run does the same before the run and leaves the server up.

Options: `--cpu-throttle N` (Chrome slows the page's CPU N×, CDP `Emulation.setCPUThrottlingRate`: a phone's CPU-side costs such as HUD and DOM work show up), `--touch` (a touch device: the phone's touch controls and HUD), `--swiftshader` (software WebGPU, for machines without a GPU such as CI or a Linux
sandbox; under Xvfb there), `--unthrottled` (no vsync or frame-rate cap, so frame intervals show
the frame's real cost rather than 16.7 ms), `--base URL` (a served build; by default it reuses
`vite preview` on :4173 or starts one; the server's root, the build's `…/dwell/`, or a release behind the launcher, `https://dropkickarcade.com/dwell/v/<version>/` (the published ones: `/dwell/versions.json`), opened at that path: `scripts/scenarios.ts` `buildPage`), `--out DIR`
(default `bench/out/<timestamp>`, ignored by git), `--file FILE` (another scenarios file, e.g. a
throwaway one in a scratch directory), `--compare` with `--configs A,B` and `--configs-file FILE`
(each scenario once per configuration, side by side: below), `--warm` (the warm client, above),
`--spot NAME` (repeatable; a saved spot in its own world: below), `--spots-file FILE`, `--stall-s S`
(a load not playing within S s, 60 by default, is retried **once** in a fresh page and listed in
`report.json`'s `stalls`; a second stall fails the scenario).

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

`seed` (default 7), `window` (`width`, `height` in CSS pixels, and optionally `pixelRatio`, the device's: 2 draws and shoots a Retina page's pixels, 1 by default), `settings` (the settings menu's stored values, as in named views:
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
| `look {yaw, pitch}` | turns the player's look (degrees; yaw 0 = +Z, 90 = +X, as `camera` and `goto`), and a placed camera (`camera`, `goto`, a placed `shot`) where it stands (`__dwell.look`; it used to turn only the player's look, hidden behind the placed camera) |
| `fly {on}`, `flySpeed {level}` | creative flight and its speed level (L flies at ≥ 11 × 2^(L/2) m/s: 6 ≈ 88, 8 ≈ 176) |
| `puppets {count, at?, face?, held?}` | stand-in players (`__dwell.puppets`): `count` full bodies (≤ 256) in rows ahead of the player's feet offset by `at`, on `face` (1, or −1 for face B), each walking a small circle with a clip of its own (walk, sprint, slide, crouch, idle, jump, swim, climb), holding the hotbar's block (or the axe, when its slot is selected, swinging it every 0.8 s) with `held`; 0 removes them. For the frame cost of N bodies and screenshots of every clip without N clients ([`CHARACTERS.md`](./CHARACTERS.md)) |
| `arms {on}` | your first-person arms shown or hidden (the Show your arms setting, this page only) |
| `spyglass {on, step?}` | the spyglass zoomed to a step (0 the least, `shared/look/spyglass.json`) or out ([`SPYGLASS.md`](./SPYGLASS.md)); `spyglass-far` shoots far terrain unzoomed and at each step |
| `camera {position, yaw, pitch, aroundPlayer?, aboveFeet?, aroundFound?}`, `releaseCamera` | places the camera (the player stays), or returns it; with `aroundPlayer`, x and z are offsets from the player's feet (y absolute, or with `aboveFeet` an offset too: a height over the spawn's ground): a view wherever a flight ended, where the chunks are streamed; with `aroundFound`, x and z are offsets from the last `goto`'s place and y a height over its surface along its face's up |
| `creatureCamera` | places the camera 6 m off the creature nearest the player, 2.5 m up, looking at it (ADR 0073); fails when none is drawn |
| `goto {find, radius?, surface?, minDepth?, minArea?, minCurrent?, face?, height?, aim? \| yaw? \| toward? \| lookAt?, turn?, pitch?, move?, scope?, water?, maxS?}` | finds a place by what is there and puts the camera over it (`__dwell.goto`, [Finding places](#finding-places)): e.g. `{ "find": "water", "scope": "world", "water": "sea", "minDepth": 2, "minArea": 1000, "height": 2, "lookAt": "land", "turn": 180, "pitch": -3, "maxS": 120 }` (the nearest sea at least 2 m deep on the world map, the player moved there, the camera 2 m up with its back to the shore) or `{ "find": "water", "minCurrent": 0.1, "aim": "downstream", "pitch": -15 }`. Prints what it found and saves it in `report.json`'s `gotos`; **finding nothing within `maxS` fails the run** with what it looked for and where, so a scenario never measures the wrong scene. `water-views`, `water-shore`, `water-reflections` and `water-river` find their water this way |
| `edit {action}`, `select {slot}` | break/place at the crosshair; selects a hotbar slot (0 to `hotbarSlots` − 1, shared/inventory/inventory.json: a slot past the hotbar fails the scenario's parse, as it would wrap onto another) |
| `pick {item}` | creative: puts the item named `item` (`"dwell:torch"`, shared/items/items.json) in the selected slot (`__dwell.pick`), failing the run if it is not a creative item; reach an item by `select` then `pick`, not by its place in the item list |
| `region {op, from, to, up?}` | a region edit (the region tools, `__dwell.tool('cornerA@x,y,z')`) of the box between `from` and `to`, offsets in metres from the player's feet (`up` −1 on face B): `fill` with the selected hotbar block or `clear` to air — to build a scene, e.g. `tunnel-a`/`tunnel-b`'s hill and tunnel (DAY_NIGHT.md §11), and `torch-cave-a`/`torch-cave-b`'s tunnel and room at midnight with the torch held and placed (LIGHTS.md; `torch-night-puppets` records 8 torch-bearing stand-ins against none; `torch-scale-a` shoots the 1 m torch beside a stand-in at noon and midnight and the held flame by day and night); `torch-shadows-a`/`torch-shadows-b` place a torch before a stone pillar in the room and hold one too, recording the Torch shadows' cost with `--configs torch-shadows-off,torch-shadows-high`; `felling` builds a tree on the flat world and breaks its trunk, shooting the fall and the settled logs, FELLING.md) |
| `brush {command}` | drives the brush tool as its keys and clicks do (`__dwell.brush`: `toggle`, `preset:<name>`, `size:<n>`, `apply`, …; [`BRUSHES.md`](./BRUSHES.md)), in every world (creative; the sandbox with a `--compare` config `params: {"world": "sandbox"}`) |
| `settle {maxS}` | waits until the LOD has had nothing to generate, mesh or ask for, for 1 s |
| `until {what, maxS}` | waits until the view waits for nothing `behind` the camera (out of its frustum: `__dwell.state().lod.waitingOffView` 0), nothing `nearBehind` it within the full-detail distance (`waitingNearOffView` 0: the full detail behind is there) or nothing `ahead` (`waitingInView` 0), and saves the seconds since the page was opened in `report.json`'s `untils`; every run also saves `join` (page opened → playing, the loading screen's wait and its stages, ARCHITECTURE.md §6.3). `join-behind` times the join's timeline and shoots behind the camera at the start and once settled; `--compare --configs raster,loading-off` runs it with and without the loading screen |
| `carry {speed, yaw, s, lift?}` | stands the player on a 4 × 4 log raft `lift` m (default 1) over its feet and moves the raft at `speed` m/s toward `yaw` (degrees, 0 = +Z) for `s` seconds, holding it `lift` m over the ground or water under and just ahead of it, then waits out the ride (`__dwell.carry`, `LocalCore::carry_local_player`; a local world on face A only): a rig for "carried by a moving body" bugs (BUGS.md #54) |
| `probe {at}` | up to two fixed points (offsets in metres from the feet) the per-frame streaming probe reports: `probe.target` and `probe.control` (how their chunks are drawn, below) and `probe.target_m`, `probe.control_m` (their horizontal distance) |
| `die {maxS}` | kills the local player (`__dwell.die()`, a local world only), waits for its respawn, then (at most `maxS`) until it is playing again, and saves the respawn → playing seconds with the loading screen's wait in `report.json`'s `respawns` (ARCHITECTURE.md §6.3). `respawn-far` flies 700 m from the spawn and dies there; `--compare --configs raster,respawn-preload-off` runs it with and without the respawn preload (`?respawnPreload=off`) |
| `lava {op, args?}` | the local server's lava (`__dwell.lava(op, args)`, VOLCANOES.md §6.3), its JSON printed: `report` its fields (face, centre, origin, `molten` columns, deepest m, vents, the world's worldgen, `steamCells` sent so far), `rate` [k] every vent's rate × k (fill a lake in seconds), `vent` [x, y, z, rateM3s, depthM] a lake vent at a world point, `ventNear` [dx, dz, rateM3s, depthM] one off the player's feet; `lava-lake` shoots the starter's summit lake and lava meeting the sea. A scenario's `query` (e.g. `{"worldgen": "test:starter", "seed": "0", "at": "x,y,z"}`) opens a test world at a point for steps of its own |
| `give {item, count}` | gives the local player `count` of `item` (`__dwell.give`, a local world that uses items, e.g. `?mode=adventure`): `pieces-combine` fills the backpack with pieces to shoot its Combine section |
| `mark` | forgets the telemetry so far: the next `record` measures from here |
| `record {name, budget?, reportOnly?}` | saves everything above under `name`; a `budget` (`{"<series>.<stat>": max}`, stat `mean`, `p50`, `p95`, `p99` or `max`, e.g. `{"frame.cpu.p95": 18.5}`) fails the scenario when a value is over its maximum or not recorded (`overBudget`). `phone-moving` (walk, sprint, fly 8 s each on an iPhone-sized page; run `--touch --cpu-throttle 4` at Low, BUGS.md #72) bounds `frame.cpu`, `jobs.apply` and `mesh_dirty` while moving; with `reportOnly` true a value over it prints `OVER BUDGET (report only)` and the scenario goes on (`forest-perf`, a target not met yet) |
| `shot {name, at? \| above?, yaw?, pitch?, face?, quietFrames?, maxS?}` | screenshot; with a placement an exact one ([Screenshots of a place](#screenshots-of-a-place)): `at` [x, y, z] the eye, or `above` [x, z, height] the eye `height` m over the ground there along `face`'s up; the camera placed, the view settled, then shot; saved in `report.json`'s `shots` |
| `spot {spot, name?}` | a placed `shot` of the saved spot `spot` (`bench/spots.json`), named `name` (default the spot's, `:` → `-`) |
| `traceStart`, `traceStop {name}` | a Chrome performance trace (timeline, GPU, V8, the CPU profiler) and its profile summary |
| `time {t, moon?}` | shows the time of day `t` (0–1, 0 midnight) and optionally the moon's phase on this client (`?time=`, `?moon=`; DAY_NIGHT.md) |
| `timelapse {name, frames, from?, to?, waitS?}` | `frames` screenshots `<name>-00.png`… from time of day `from` (0) toward `to` (1), wrapping past midnight, waiting `waitS` (0.3 s) after each |
| `lod {on}` | draws the distant terrain (LOD) or not, live (`__dwell.setLod`; the sections still stream, so it comes back at once): what the full-detail chunks alone hold. `?lod=off` (or `?lod=0`) runs with no LOD at all |
| `view {clouds?, speedEffects?, distanceM?, pixelError?, smoke?, flowParticles?, flowRipples?, flowFoam?, waterfalls?}` | sets other settings mid-run, not stored (`__dwell.setView`): the clouds (`off`, `low`, `high`), the speed effects (motion blur and streaks), the full-detail distance (m) and the LOD's pixel error, the blasts' smoke, the water's flecks, flow ripples, flow foam and waterfalls; each one given replaces the menu's value. With `water` and `shadows`, one page can record every feature on and off in turn ([Cost profile](#cost-profile-m1-air)) |
| `water {quality?, debug?}` | sets the water quality (`low`, `medium`, `high`) and/or its cost-isolation switches (`?waterdbg=` names, `""` for none) mid-run, through `__dwell.setWater` |
| `burst {name, frames, clock?, maxFlicker?}` | a still camera's burst: `frames` screenshots `<name>-00.png`… back to back (the HUD hidden) and their **flicker** (`flickerOf`: the share of pixels whose brightness moves more than 3 of 255 from one frame to the next, and the mean move), printed and saved in `report.json`'s `bursts`; over `maxFlicker` (a share) the run fails. With `clock` the time of day runs on from it at the world clock's rate, set every animation frame (the bench's is otherwise held), so the light moves as in play. `shadows-still` reads the shadows' flicker against Off's (DAY_NIGHT.md §10). With `drift {position, yaw, pitch}` (after a `camera` step) the camera moves on by that much each frame (metres and degrees, at most 1 m and 1°): z-fighting shows only while the view moves (it stops with the camera), and a drift of millimetres and thousandths of a degree moves the view under a pixel by a fraction of a pixel, so surfaces swap only where they fight in depth. Such a burst is read by its **hue flips** (`hueFlipsOf`, also in every burst's result: the share of pixels, coloured in both frames, whose hue moves over 40° from one frame to the next — grass becoming dirt); over `maxHueFlips` the run fails. `lod-zfight` holds the LOD's z-fighting under a bar (ARCHITECTURE.md §6.6, "Known bug class") |
| `clip-strip {name, vertical?, frames?, views?, at?}` | one axe swing frame by frame (see "Clip probe and clip strip" below): the level (or `vertical`) chop held at `frames` (2–24, default 8) clip times from 0 to the clip's end (`__dwell.holdSwing`), each shot from the `views` (`fp` your eyes; `tp-behind`, `tp-side`: a stand-in 4 m ahead of `at` holding your tool, standing still facing +Z, seen from behind-right and from its right side), `<name>-<view>-00.png`…, and a contact sheet `<name>-sheet.png` (a row per view, each frame labelled with its clip time). Hold the axe first (`give`, `pick`) |
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
| `mapbuild-off` | 1 | `?mapbuild=off`: no big-map background build while the map is shut (ADR 0068, 0.52.13), to measure its cost |
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
| `minimap.scan`, `minimap.draw` | the minimap (ADR 0068): a read of the columns' tops round the player with its draw (on a move of a texel, or every `refreshSeconds`), and a draw alone (a turn); samples, so only the frames that did one |
| `map.liveTile` | the big map's closest levels (ADR 0068, 0.50.76): a tile read from the loaded terrain (`liveTile`), at most `revealed.liveTilesPerFrame` a frame while the map is open at those levels |
| `ticks`, `ticks.count` | the fixed-rate prediction ticks (controller + physics) and how many ran |
| `pieces.standIns` (samples) | the pieces stood in for in the prediction at each snapshot: those that could touch the local player soon ([`EXPLOSIVES.md`](./EXPLOSIVES.md) §3) |
| `game.draw` | the game's draw step: `mesh_dirty` and `lod` inside it |
| `mesh_dirty` | starting chunk mesh jobs (copying padded chunks), within its 4 ms budget |
| `lod` | the whole-world view's update (`LodSystem`): selection, job requests, stand-ins |
| `probe.under`, `probe.ahead` | the streaming probe (`Game::probe_stream`): how the chunk a metre under the feet and the one 10 m ahead (along the way the player last moved) are drawn: 0 as a chunk, 1 loaded but not drawn as one (its light or mesh, or the LOD's section stands in), 2 not loaded; with `probe.x`, `probe.z` (the feet), `pool.gen`, `pool.mesh` (jobs queued or running) and `stream.loaded`, `stream.dirty`, `stream.generating` |
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
| `gpu.far`, `gpu.near`, `gpu.far.water`, `gpu.near.water`, `gpu.shadows`, `gpu.torchshadows`, `gpu.arms`, `gpu.composite`, `gpu.motion`, `gpu.shafts`, `gpu.frame` (samples) | GPU time of each render pass (the sun's or moon's shadow maps, every cascade's pass timed as one, DAY_NIGHT.md §10; the far and near scene passes, your first-person arms' pass (CHARACTERS.md; it resolves the frame's colour when drawn) — on Medium and High water, their opaque halves, with their water halves as `gpu.*.water`, WATER_RENDERING.md §8 W3 — the light shafts at their tier's resolution (`gpu.shafts`, ADR 0064; `?shafts=off|low|high` sets the tier for a run), the composite with the lines over it, or, while the wind is on, the composite into the finished image and the motion pass with the lines, MOTION_EFFECTS.md) and of the whole frame (the far pass's start to the composite's end), from timestamp queries (`render/timing.rs`), one sample per timed frame, read back a frame or two later. Only where the adapter has `timestamp-query`. On tile-based GPUs (Apple, phones) passes overlap, so the passes can add up to more than `gpu.frame` |
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

Cloud plumes (docs/CLOUDS.md §8.7): `__dwell.plume(op, args)` drives the run-time plume list, JSON back: `list`; `add` [name, x, y, z, radius?, height?, density?, riseMps?] a plume of the lava vent's look based at a world point (on that point's face); `near` [name, dx, dz] one 40 m under the eye; `move` [name, x, y, z]; `remove` [name]; `clear`. Drawn on Medium and High clouds only. Water flow (docs/WATER_SURFACE_GRID.md §2c): `__dwell.waterFlowAt(x, y, z)` gives the client's current there (m/s along x and z), the flecks near the eye, and the first few flecks' places. Waterfalls (§2e): `__dwell.waterFalls()` lists the falls the client found near the eye (each one's high column, way, width, lip and landing heights, flow per metre and lip speed). Creatures (ADR 0073): `__dwell.creatures()` lists the creatures drawn (each one's feet, heading and speed). The far ecology (docs/ECOLOGY.md §9): `__dwell.ecology()` gives the far herds and packs the server last sent (`{ day, groups: [{ id, face, species, x, z, size }] }`, face-local metres, every 2 s), or null before the first or from a server without one (a local world's ecology doesn't run yet). Blasts (docs/EXPLOSIVES.md §4): `__dwell.blasts()` lists the blasts whose effects are still going (each one's place and age, s); telemetry `blasts.puffs` counts the smoke, dust and fireball billboards drawn a frame.

From the console: `__dwell.telemetry()`, `__dwell.telemetryFrames()`, `__dwell.ticks()`,
`__dwell.telemetryReset()` (types in `client/src/main.ts`).

The start's stages (ARCHITECTURE.md §6.3, *The start's stages*): `__dwellStages()` (on `window`, from the
page's first moment, before `__dwell`) gives each loading stage that ran — `id`, `beganMs`, `endedMs` (null:
running) and `firstMs` (when its count first moved: the wait before the terrain counts) on the page's clock —
and every stage is a User Timing measure `dwell:stage:<id>` in a Chrome trace's Timings track. Every bench run
prints them after the join (`stages: page 0.00–0.03 s, game 0.03–0.08 s, …`) and saves them in
`report.json`'s `join.stages`, with the start's main-thread long tasks (`join.longTasks`), its frame
gaps over 100 ms in the first 30 s (`join.frameGaps`: a screen that did not repaint, the main thread
held or the GPU process busy) and the renderer's start group by group (`join.createMarks`, the
`dwell:create:<group>` User Timing marks, each at its group's end: `device`, `module`, `layouts`,
`scene-pipelines`, `passes`, `atlas`, `bindings`, `characters`, `renderer`). A cold start: empty
Chrome's Metal shader cache (`$(getconf DARWIN_USER_CACHE_DIR)com.google.Chrome.helper/com.apple.metal`)
before the run; the bench's fresh profile alone is not cold.

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
- `haze=<preset>` draws a distance-haze preset from `shared/look/fog.json` `presets` (`thicker`,
  `aerial`, `aerial-sun`) instead of the fog every client draws, to compare them (DAY_NIGHT.md §4,
  WORK_QUEUE item 29); a bench scenario passes it in its `query`.

For example `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5&time=0.45` opens a new seed 0 world at
the owner's dam site; then `__dwell.fly(true)`, `__dwell.tool('cornerA@…')`, `tool('fill')` and
`placeCamera` set the scene up. `?new=1&seed=<n>&name=<name>` instead adds a new world to the
menu's list and opens it.

**A bigger world** (BIG_MAP.md M2b): `?size=2x` (any size of `constants.json` `worldSizes`: `2x`,
`4x`, `8x`, `10x`; with `seed=`, `spawn=`, `at=`, `fresh=1` as above, alone seed 0) opens a terrain world
of that size, its own world file (`local-g8-s<seed>-x2`), created at it and saved with it; the flat and
sandbox worlds are 1× only. New world → More options' **World size** (desktop pages) makes one in the menu.

**Play a test world** (0.52.27): `?worldgen=test:tall_peak` (any fixture world's name,
`shared/worldgen/worlds/*.json`; with `seed=`, `fresh=1`, `at=` as above, alone seed 0) opens a
terrain world on the fixture's ground, its own world file and record (`test-<name>-s<seed>`, never the
seed's own `local-g8-s<seed>`). Chisel's **Open starter world** (its File group, 0.52.63) replaces the edit with
the starter world (`starter.json` resolved by biomes.mjs `fixtureWorldEdit`, as gen.mjs does for the game), to tune it
and play it. Chisel's **Play in game** (its File group) does the same for the
current edit: a new local world of the view's seed in a new tab (`?play=<id>`), listed in the menu.
The world's record (`dwell.worlds`, `WorldMeta.worldgen`, optional: append-only) keeps the config, the
fixture's name or the edit's JSON, so the world reopens on the same terrain. Only the height field
travels (the fixture's overrides; an edit's world knobs, `chisel::worldgen_height`): the page builds
the world maps from it (`buildWorldMap(seed, face, worldgen)`, `WorldMap::for_height`), cached under
their own keys (`worldMapKeys(seed, worldgen)`: the default key plus the config's hash, so the
seed's own maps are untouched), and every WASM instance installs them as usual, so the server, the
LOD and the water all generate the fixture's ground. From 0.52.37 (ADR 0074 B7) the maps also carry the
config's text (`WorldMap::worldgen`), so those generators put the whole edit on the ground: its biomes,
biome map, rocks, erosion, vegetation and tectonics, as Chisel previews them (`dwell-worldgen`
`tests/worldgen_config.rs` compares the columns, both faces). One difference: the game's terrain volume
(the cliffs) stands over the edit's ground and Chisel's preview has none. From 0.52.38 a guest joining
one (a friend world, or a dedicated server started with `--worldgen <edit.json>` or
`--worldgen test:tall_peak`) is sent the config and builds the same maps; an older build refuses the
world (`dwell-client` `tests/worldgen_guests.rs`). The New world form never sets it. A bench run uses it through a
`--configs-file` configuration's `params` (`{"worldgen": "test:tall_peak", "fresh": "1", "at": …}`).
The starter world (0.52.54), `?worldgen=test:starter`, is a fixture with its own biomes (`biomes`,
`biomeMap`, `geology` in its file): its maps carry the fixture's name and every generator uses the edit it
stands for. Its overview: `STARTER_PNG=<dir> cargo test --release -p dwell-worldgen --test starter_world`;
its tour's `at=` spots: `… --test starter_world tour -- --ignored --nocapture`.

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

## Screenshots of a place

How agents take screenshots: an exact camera, a settled view, a saved spot.

- **A placed `shot`** sets the camera exactly and waits before shooting: flight on, the player
  teleported under the eye (so the full-detail chunks stream round it), the camera placed, then the
  view settled — the LOD with nothing to generate, mesh or ask for, nothing waiting in view
  (`waitingInView` 0) and the chunks meshed, for 1 s — and `quietFrames` (30) more frames; at `maxS`
  (90) it shoots anyway and prints `NOT settled`. `report.json`'s `shots` keeps each one's eye, look,
  ground and seconds.
- **Height over the ground**: `above: [x, z, height]` finds the column's first non-air cell from the
  face's top (face-aware: on face B it reads from the bottom up and `height` is along −y),
  descending 48 m at a time from where the page opened until the column's chunks hold it, so a spot
  whose ground height isn't known still lands `height` m over it (over the canopy under a tree).
- **The face is checked**: the midplane is y = 0, so an `at` whose y is on the other face's side for
  its `face` (default `a`) fails the parse (`y -80 is on face B's side … face is "a"`), instead of
  spawning a face-A spot on face B from a sign slip.
- **Saved spots** (`client/bench/spots.json`, `scripts/spots.ts`): `<world>:<place>` → `world`
  (`test:<fixture>` or `terrain`), `seed`, `time`, `at` | `above`, `yaw`, `pitch`, `face`, and for
  `above` an `openY` (the height its page opens at, over the ground). `npm run bench -- --warm
  --spot starter:broadleaf` opens the spot's world fresh (`?worldgen=…&fresh=1&at=…`) and shoots it
  to `bench/out/<time>/starter-broadleaf/starter-broadleaf.png`; a `spot` step shoots one inside a
  scenario. The starter world's are its tour's (`starter_world` `tour`). A new spot is an entry in
  the file (`spots.test.ts` parses every entry).

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

## Clip probe and clip strip

Owner, 2026-11-11: "Make yourself some tools to properly observe animations, movement directions,
positions over time" and "may need to see multiple perspectives to see animation and understand it".

**The clip probe** (native, no browser; `crates/dwell-client/src/clip_probe.rs`, CLI
`examples/clip_probe.rs`) samples a clip through the same posing the game draws with: the arms'
animator (`Space::Arms`, the swing on its action layer driven to clip time as `Bodies::drive_arms`
does), the held item's turns (`with_item`), `arm_transforms`, the tool drawn as its model on the hold
(`prop_mesh`, `held_item`) and the arms pass's projection (the first person `fov`); and the body's
animator (`Space::Body`, `bone_transforms`: the rig pose the skinned player model retargets) for
third person. It follows points — `tool_head` (the far tenth of the model along the handle),
`tool_tip`, `grip`, `elbow`, `shoulder`, any bone by name, or a part of the held characters item
(`axe_blade`) — and reports them in five views of the same swing:

| View | What | Axes |
| --- | --- | --- |
| `fp` | the first-person screen, what the player sees | NDC x right, y up (−1..1), and the depth |
| `top` | the player's frame from above | right, ahead |
| `side` | from the player's right | ahead, up |
| `front` | facing the player | the player's right on the left, up |
| `tp` | the whole body from behind and to its right (perspective, the rest skeleton drawn) | screen |

```sh
cargo run -p dwell-client --example clip_probe -- chop_level                # table + phases
cargo run -p dwell-client --example clip_probe -- chop_vertical --part tool_head,grip,elbow \
  --svg /tmp/v.svg --views fp,top,side,front,tp                              # the panel sheet
cargo run -p dwell-client --example clip_probe -- fp_break --item none --part grip --csv
```

The clip is an arms role (`chop_level`) or a clip name (`fp_chop_level`); `--item` the inventory item
in hand (default `dwell:stone_axe`, `none` empty-handed); `--samples` (default 31); `--face a|b`,
`--yaw`, `--pitch` place the world space output (`--csv` prints every sample in every space: screen,
the player's frame, eye-relative world, the body). The table gives each sample's first-person screen
place and body place, then the **phases** — from the tool's `swing` timings for a chop (wind-up
`windupS`, strike the arc's `activeS`, recovery), else thirds — each with its travel and dominant way
(`right→left`, `top→bottom`) on screen and on the body, the **peak** screen speed and when, and the
**contact**: where the head is at the arc's middle step (a centred hit), which should be the
crosshair. The SVG draws each view as a panel: every point's path with a dot per sample, a time label
every fifth, arrows along the travel, the phases coloured (amber wind-up, red strike, blue recovery),
the contact circled, the screen's edge dashed in `fp`; the sheet is square (five views: three by two)
so `qlmanage -t -s 1600 -o DIR sheet.svg` makes a PNG with every panel. Saved sheets of the 2026-11-11
fixes: `docs/clip_probe/` (`*_before`/`*_after`: the directions; `*_arms`: the arm-driven refit).

**The camera's sway** (CHARACTERS.md §5.2): for a chop each sample also carries the view's sway at
that clip time (`Sample::camera`, a miss's, `chop::swing_camera`), printed as the table's last column
`camera yaw,pitch,roll ° bob r,u,b cm` and the CSV's `cam_yaw … cam_bob_back`.

**Orientation** (owner, 2026-11-11: "Ensure your tooling helps you tell rotation too"): each sample
also carries how things are turned (`Sample::orient`, `clip_probe::Orient`): the blade's edge facing
(`edge_points`: from the `axe_lashing` out through the `axe_blade`) and the handle's axis (grip to
head), each arm bone's (`arm_r`, `forearm_r`, `hand_r`) local axes in the player's frame and its
angular speed, the hand's roll about its own long axis against the forearm (a wrist flip shows as a
jump), and the angle between the edge's facing and the edge's motion (under 90° the edge leads). The
table adds the columns `edge∠motion`, `hand roll` and `spin arm,fore,hand °/s`; the SVG draws, every
third sample in every view, the handle (a grey line, grip to head) and the edge's facing (a dark red
arrow from the edge), and every sixth each arm bone's axes as a small tripod (x red, y green, z blue).

**Fitting keys** (`--fit`, repeatable): instead of probing, fit a key for the clip at time `t` that
puts the grip and the tool head at given places and, optionally, turns the blade's edge (`axe_blade`
from `axe_lashing`) toward a direction (the strike's way, so the edge leads):

```sh
cargo run -p dwell-client --example clip_probe -- chop_level \
  --fit 0.3/0.55,-0.25,0.25/0.95,0,0.35/-1,0,0.5 --fit 0.355/0.2,-0.25,0.42/0,-0.02,0.75/-1,0,0
cargo run -p dwell-client --example clip_probe -- chop_level --body --fit 0.3/0.35,1.25,-0.05/0.65,1.5,-0.35
```

Places are the player's frame (right, up, ahead of the eye) for first person, the body's (right, up,
ahead of the feet) with `--body`. The blade term uses the same edge facing the probe reports (`edge_points`). It runs coordinate
descent on the game's own posing (`clip_probe::fit`:
the arm, elbow and wrist turns within the rig's limits, plus the arms' root offset, kept small, or the
body's chest twist), each key starting from the one before, and prints the keys as the clip's JSON with
the reached grip, head and blade on stderr. A key far from the one before can land in another pose;
fit it alone (from the hold) if it does.

Tests over it (`clip_probe::chop_swing_tests`) pin the axe: the level strike right → left on screen,
the vertical one top → bottom (monotonic through the strike), the level one level (its up/down under
0.3 of its travel across), the hands travelling at least 0.4 of the head's path through the strike,
a 0.25–0.35 s wind-up drawing the head 0.45 m and the hands 0.35 m back, the edge facing within
50° of its motion through the strike (red on the 0.52.11x clips: 64°), no wrist flip (the hand's
roll under 2000°/s between samples and within 120° over the swing), the head on the crosshair at contact and
the peak speed inside the strike, the wind-up the other way, the recovery back to rest, nothing behind
the eye; in the world on both faces (`both_faces!`) along the view's left and down at any heading; and
on the body (third person) across from its right to its left, or down, in front of it. A new swing
clip gets the same checks by adding a case.

**The clip strip** (browser, the bench's `clip-strip` step) is the visual check after the probe: it
holds a swing at K clip times (`__dwell.holdSwing(vertical, t)`, which poses your arms and every
stand-in at that clip time until released with a negative `t`; stand-ins then stand still facing +Z)
and shoots each from first person and from two third-person cameras (`clipStripCamera`: behind-right,
and the right side) into one contact sheet. Local machine only (a GPU); e.g. a scratch scenario with
`{ "do": "give", "item": "dwell:stone_axe" }`, a `pick` of it, and `{ "do": "clip-strip", "name":
"level" }`. Its face B cameras are mirrored by the unit test only, not yet seen in game.

## Menu panoramas

The main menu's backgrounds (`shared/ui/menu-backgrounds.json`, ARCHITECTURE.md `ui/`) are rendered
from the real game by `npm run panoramas [-- name …]` (`client/scripts/panoramas.ts`, a built client
as for the bench; `--base`, `--out`). For each entry it opens the entry's seed as a local world with
the player at its `position` (`?local=1&seed=…&fresh=1&at=…&time=…&preset=high`; the FOV at
`capture.fovDeg`, arms and speed effects off, no music), waits for the terrain, hides everything but
the game canvas, pauses the look ([Pausing the look](#pausing-the-look): every shot is of one
instant, so the clouds and the water match across them), then places the camera there level and
turns it in `capture.steps` equal steps (yaw falling, so each shot lands right of the last),
waiting for what each way shows. The square shots are stitched in the page into one cylindrical
360° strip whose ends meet (`span` of the focal length above and below the horizon): each column
blends the two shots either side of it, each reprojected from its own perspective along the same
ray (`shotPoint`), with a smoothstep weight from 1 at a shot's centre to 0 at its neighbours'
(`columnShots`), so the feather spans the whole 22.5° step and there is no hard join; any
view-dependent difference (a reflection) fades across the step instead. Encoded as WebP at
`capture.quality` into `shared/ui/menu-backgrounds/`, with a copy at half the size each way
(`<name>-small.webp`, `smallImage`) that the menu loads on a phone (the screen's shorter side under
600 px, `backgroundUrl`: a quarter of the decoded memory); the shots stay in `--out`. With the
defaults (16 shots, 2160 px at 90°, since 0.52.4; 1080 px before) a strip is 6786 × 1944 (the small
copy 3393 × 972), 335–685 KB (the small 135–300 KB). The window is `sizePx` square at a device scale of 1, so it can be larger
than the screen; a run of the three takes ~3½ minutes on the M1.

A new background: find the spot (a `goto` scenario, [Finding places](#finding-places): the three
were found with `water` at `scope: world` for `sea` then the nearest `dwell:sand`, `river` and
`lake`, seed 0), add its entry, run `npm run panoramas -- <name>`, then `node shared/ui/gen.mjs`.


**Random (0.50.44).** `menu-backgrounds.json`'s `default` may be `"random"` (the default since 0.50.44; owner, 2026-11-08): the menu picks a view at random (`nextRandomBackground`), and after each full turn (`turnS`) picks a different one and fades it in over 3 s on a second layer (`MenuPanorama`, `.mm-pano-layer`). Settings offers Random first.

## Streaming report (F8)

**[built, 0.51.23]** For the owner to send the state of a streaming bug from his own game (BUGS.md #54):
**F8**, the Debug page's **Copy streaming report** or the F3 overlay's **Copy streaming** copies one
JSON object (`"kind": "dwell-streaming-report"`), or shows it selected in a text box where the clipboard
is refused; `await __dwell.streamingReport()` returns it (and copies it). Paste it into the bug report or
the chat as it is. The game's part comes from `Game::streaming_report`
(`crates/dwell-client/src/game/streaming_report.rs`, the wasm `Client.streamingReport()`), the shell adds
the rest (`client/src/debug/streamingReport.ts`):

- `version`, `sha`, `channel`, `time`; `device` (the user agent's first 160 characters,
  `hardwareConcurrency`, `deviceMemory`, the GPU adapter's name, `cpuAdapter`, the screen and DPR);
  `settings` (render scale, upscaler, Full detail, Distant detail, Distant memory, the preset or
  `custom`); `world` (local, seed, generator version, the URL's `world` and `mode`, the compatibility
  line); `flySpeedLevel` while flying.
- `player`: feet, velocity, speed, face, flying, its chunk.
- `nearRing`: every chunk within ±3 chunks (`NEAR_RING_CHUNKS`) of the player's in the world's rows,
  `needed` and `drawnFull`; each chunk's coordinate `c`, distance `d` (m), `flags` (loaded, air,
  generating, lit, meshed, meshing, dirty, drawable, drawn — as a chunk at full detail —, settling,
  outsideVolume), its level-1 section's `lod` (known, kind, needsAnswer, generating, meshing, meshed,
  showsChunks, forced, showsNothing), and for a chunk in the volume not drawn in full, `waits`: the
  `reasons`, the face neighbours it waits on (generating or unlit) and those the server never sent, its
  place in the dirty order, and its generation's and mesh job's place in the pool's queue (or
  `"running"`).
- `pool`: per kind of job (`chunk`, `chunk_mesh`, `section`, `section_mesh`, `bounds`, …) queued, in
  flight and the oldest one's age (ms); the focus, `nearJobsM`, the slow-frame scale, the capacity.
- `streamer`: loaded, generating, meshed, dirty, meshing, unlit, settling, resyncs, terrainReady, the
  server's view and render radii, the unloads of the last 30 s and those within 48 m (the nearest 64
  listed with distance and age).
- `lod`: sections drawn by level, chunk sections, nodes, jobs, `cacheBytes` against `cacheBudget` (the
  Distant memory), in-use bytes, the error scale, what the view waits for, the last selection's counts
  and its age.
- `frames`: over the last 10 s, the frame interval's p50/p95/max, frames a second, chunk and section
  uploads a second and their time, jobs applied a second; `creatures`: how many the client holds.

It stays under ~200 KB (`REPORT_MAX_BYTES`: indented where that fits, else one line, else the chunks
drawn in full dropped and counted). Tests: `the_streaming_report_describes_the_near_ring_pool_streamer_and_lod`
(`game_local.rs`), `a_chunk_status_names_the_neighbour_it_waits_on` (dwell-game, both faces),
`streamingReport.test.ts`.

## Pausing the look

A debug pause stops every drawn animation at one instant while the game runs on (owner,
2026-11-08: for the panoramas' seams): `?pause=1`, `__dwell.pause(true)` / `pause(false)` /
`pause()` (reads it), F7, or Settings → Debug → Pause the look. The client keeps one **render
clock** (`crates/dwell-client/src/render_clock.rs`): wall time less the time spent paused, frozen
while paused, resuming from the same instant. Everything drawn reads it: the frame block's time
(`fog.z`: the water's ripples, caustics, carried flow and waves, the plants' sway), the flames,
motes, flecks and lights' flicker, the stars' twinkle and the day clock (`update_day`), the clouds'
drift (the world clock read at render time), the eye's adaptation; the flecks step by no time. The
simulation, the controller, prediction, streaming and the network stay on the wall clock, so the
world can change under a paused look (water levels, a falling piece), and the camera can be moved
(`placeCamera`) to shoot the same moment from another angle.

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
"flight" may have been the same stall. **Why (found 0.50.12):** not the bench nor the held key: the tick
trace kept `move [0, 1]` held while `touchingWall` and `grounded` were true, at 162 m: the climb at level 0
(3 s of Space, ~20 m) left the player below the valley's side, and the flight ran into it. Raising the fly
speed *before* the climb (level 6, 3 s: ~200 m up) flies freely (254 m in a 4 s record; [Render scale and
upscaling](#render-scale-and-upscaling)). The
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

### Phone memory (`memory-phone`, BUGS.md #70)

iOS Safari reloads a tab over its memory limit (~1–1.5 GB with the GPU's share; "A problem repeatedly
occurred"), and the page cannot read Safari's own count, so the phone's budget is checked on the desktop:
the `memory-phone` scenario (a 393 × 852 at 3× page) run with `--touch` (the phone tier: two workers per
pool, the Mobile preset's 128 m and 96 MB) records `memory()` at the end of loading and 30 s later; a test
world by `--compare` with a `params` config (`{"worldgen": "test:starter"}` in a `--configs-file`). At
0.52.68 on the M1: the default world 676 MB at load (592 after 30 s), the starter 850 (776); before it the
starter read 1,467 MB (each instance's own biome grids, the LOD 3.4× over its budget). WASM memories never
shrink, so the reading is each instance's peak. Keep a phone's total well under 1 GB.

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

## The tree performance reference (`forest-perf`)

**THE place every tree, leaf, card or impostor change is measured** (WORK_QUEUE items 30–32; the owner's
target: High at 60 fps on an M1 MacBook Air). The world is `test:forest` (`shared/worldgen/worlds/forest.json`):
gentle land that is the old-growth forest (`preset:old_growth`, ADR 0074) everywhere, ~220 trees a hectare,
old-growth pines up to 26 m and broadleaf up to 14 m over thick tall grass; fixed by its data and seed 0,
so before and after compare the same stand. Its spots (`bench/spots.json`):

| Spot | Where |
| --- | --- |
| `forest:inside` | standing in the densest stand (49 trees in a 40 m square, at 40, −40), eye at a player's height along the trunks |
| `forest:edge` | 32 m over the highest ground 440 m south, just over the crowns, looking north across the canopy |
| `forest:above` | flying 150 m over the stand, looking north and down to the horizon (every LOD tree level) |

The scenario `forest-perf` opens it at `?preset=high` (1280 × 720 by default) and, at each spot, shoots it
(`inside.png`, `edge.png`, `above.png`), then records 8 s (`inside`, `edge`, `above`): `frame.cpu`,
`gpu.frame` and every pass. Its budget is a 16.7 ms frame (`frame.cpu.p95`, `gpu.frame.p95`), **report
only** until item 30 lands: a record over it prints `OVER BUDGET (report only)` and the run goes on; make it
failing (drop `reportOnly`) once High holds 60 fps there. On the M1 Air, before and after a change:

```
npm run bench -- --warm forest-perf
npm run bench -- --warm --compare forest-perf   # a configuration against the baseline, side by side
```

Cloud sessions have no GPU: there it is not run; `cargo test --release -p dwell-worldgen --test forest_world`
checks the world (every column old-growth, the density in range on both faces, deterministic), and its
ignored `spots` test prints the places the spots stand at.

## Render scale and upscaling

The owner's "try the upscaling options and compare performance and screenshots" (2026-11-07), after the
[cost profile](#cost-profile-m1-air) found ~74 % of the dam's GPU frame scaling with the pixel count. Built as
a setting (ARCHITECTURE.md §6, the `render/` row; data `shared/render/upscale.json`): **Render scale**
100 / 75 / 67 / 50 % per axis and **Upscaler** Stretch or Sharpen, Settings → Graphics, kept in
`dwell.renderScale`; 50 % Stretch by default since 0.50.28 (owner: "make the default 50% upscaling instead of 100%"; a stored choice is kept), and the [graphics presets](#graphics-presets) pick it. `?renderscale=0.67&upscaler=sharpen` overrides
it for a visit (the bench's configurations), and `?upscaler=stretch-pass` draws the plain stretch as a pass
on a full-size canvas, to compare the browser's stretch with an internal one.

- **Stretch**: the canvas's drawing buffer is smaller (`upscale::sizes`) and the browser stretches it
  (CSS, bilinear). Every pass runs at the low size, the post passes too (the composite, fog, clouds, ink,
  wind, lines), and Chrome composites a smaller canvas. The DOM HUD stays sharp.
- **Sharpen**: a full-size canvas; the frame draws into a low-res picture, then two passes port AMD FSR 1:
  EASU (edge-adaptive upscale) to full size and RCAS (contrast-adaptive sharpen, `sharpenStops` 0.25) onto
  the canvas, timed together as `gpu.upscale`.
- A CPU adapter ignores the scale (its own half-size buffer); the pixel-art preview takes precedence.
- Not built: temporal upscaling (FSR 2/3 or DLSS style: needs motion vectors, jitter and history, and
  ghosting work on water and leaves) and MetalFX (native Metal only, not in a browser).

**Method.** M1 MacBook Air on AC power, Chrome on the GPU, a 1280×800 page at device pixel ratio 2 (a scenario
window's new `pixelRatio`: the shots are 2560×1600, the Retina screen's pixels; the cost profile used
`?scale=2` at ratio 1, whose shots were downscaled), seed 0 at the dam's spawn (`?at=2364.5,142,7443.5`),
noon, the desktop defaults (water Medium, shadows High, clouds High). One page per configuration, one
scratch scenario (`up`): the spawn's own view along the river, the dam camera (6 m over the river looking down
40°), then a **flight** (fly on, speed level 6, 3 s of Space, W held: ~255 m in each 4 s record over the
valley, the LOD streaming). Each record 4 s after 1.5 s to settle; `--unthrottled`, so a frame shows its
cost; 100 % run first and again last. The 1-minute load was 2.9 at the start (an `ANECompilerService` process
held about one core throughout) and 2.1 at the end.

`gpu.frame` mean / p95 (ms), and drawn frames a second (unthrottled):

| Config | Spawn GPU | Spawn fps | Dam GPU | Dam fps | Flight GPU | Flight fps |
| --- | --- | --- | --- | --- | --- | --- |
| 100 % | 26.33 / 27.45 | 34.4 | 20.00 / 20.66 | 44.4 | 18.63 / 24.38 | 46.2 |
| 100 % again | 26.56 / 27.23 | 33.9 | 20.02 / 20.39 | 44.1 | 18.46 / 24.47 | 46.4 |
| Stretch 75 % | 19.07 / 20.31 | 47.4 | 14.44 / 15.62 | 57.7 | 12.65 / 16.60 | 62.3 |
| Stretch 67 % | 16.26 / 17.53 | 53.9 | 11.87 / 12.48 | 64.1 | 11.16 / 14.95 | 66.4 |
| Stretch 50 % | 12.44 / 13.49 | 63.9 | 8.91 / 9.48 | 82.9 | 7.71 / 10.20 | 80.8 |
| Sharpen 75 % | 20.43 / 21.28 | 43.4 | 15.85 / 16.50 | 53.7 | 14.33 / 18.08 | 56.4 |
| Sharpen 67 % | 18.16 / 18.94 | 47.6 | 14.03 / 14.33 | 58.4 | 12.81 / 16.35 | 59.4 |
| Sharpen 50 % | 14.32 / 14.96 | 57.6 | 10.83 / 11.36 | 65.9 | 9.60 / 12.03 | 72.1 |
| Stretch as a pass, 67 % | 16.39 / 16.96 | 52.1 | 12.06 / 12.59 | 63.6 | 10.79 / 13.42 | 67.6 |

With vsync (no `--unthrottled`; 100 % and the two stretches that came closest, one page each, right after;
load 1.9 → 5.1 over the run): drawn fps, `gpu.frame` mean / p95, and the share of drawn frames that came one
refresh after the one before (interval ≤ 18 ms):

| Config | Spawn | Dam | Flight |
| --- | --- | --- | --- |
| 100 % | 33.9 fps, 26.44 / 27.38, 22 % | 43.9 fps, 19.97 / 20.74, 55 % | 46.4 fps, 18.53 / 24.37, 64 % |
| Stretch 67 % | 54.6 fps, 15.89 / 16.73, 81 % | 59.8 fps, 12.16 / 12.40, 88 % | 59.4 fps, 10.93 / 14.83, 85 % |
| Stretch 50 % | 59.6 fps, 12.20 / 12.82, 87 % | 59.6 fps, 9.10 / 9.50, 97 % | 59.4 fps, 8.10 / 11.75, 97 % |

**Reading it.**

- **The GPU time falls with the pixels**, a little less than in proportion: at 67 % (45 % of the pixels) the
  dam's 20.0 ms is 11.9 (−41 %), the spawn's 26.3 is 16.3 (−38 %); at 50 % (25 %) 8.9 and 12.4. The cost
  profile's estimate for ⅔ (−8.2 ms at the dam) was right.
- **Sharpen costs ~2 ms** on top of the stretch at every scale (EASU and RCAS at 2560×1600: 1.4–2.2 ms by
  the `gpu.frame` difference; the `gpu.upscale` span reads 6–14 ms because the M1's passes overlap and a
  span includes the wait for the scene, as the cost profile found). So Sharpen at 67 % costs about what
  Stretch at 75 % does.
- **The browser's stretch vs an internal pass**: at 67 % the bilinear pass on a full-size canvas costs
  0.1–0.2 ms more GPU (`gpu.frame`) than the CSS stretch at the dam and the spawn (−0.4 in the flight, within
  its noise), and the drawn rate moves −1.8 (spawn), −0.5 (dam) and +1.2 fps (flight); Chrome's compositing of the smaller canvas is not in `gpu.frame`,
  so the drawn rate is its only trace here, and it barely moves. The CSS route is the cheapest, by little.
- **The CPU is not the limit** in any record (2.1–2.9 ms a frame).

**How it looks** (compare.html; the shots at 2560×1600, read at 1:1 crops; the 100 % run again differs from
the first in 24 % of the spawn's pixels by ΔE > 2.3 from the water's and clouds' motion alone, so the ΔE
figures, 30–42 % at the spawn, mostly measure animation):

- **Stretch 75 %**: slightly soft; grass tufts and texture detail lose their crispness, edges still clean.
- **Stretch 67 %**: visibly soft at 1:1: the block edges against the sky and the grass blades blur, the
  water's caustic lines widen and fade; at normal viewing distance on the Air's screen it reads as a soft
  focus rather than as blocks.
- **Stretch 50 %**: blurry: block silhouettes smear over two screen pixels, distant blocks merge, the
  water's sparkle and caustics lose their contrast.
- **Sharpen** (any scale): the edges and the texture's pixel steps come back close to 100 % at 75 % and
  67 %; the caustic lines keep their contrast. At 50 % the sharpening shows: silhouettes against the sky
  are stair-stepped (jaggies two pixels wide) and the far grass tufts look crunchy. No halos (RCAS's limit
  and EASU's clamp hold).
- **Text**: the HUD, the hotbar and menus are DOM at full resolution in every configuration: unchanged.
  Nothing textual is drawn in the canvas (the outline and lines are drawn at the scene's resolution, at their
  CSS width).
- **Shimmer** on block edges was not measured (stills only; the bench's `burst` step could). Expect more
  crawl on far edges at 50 % in both upscalers (fewer samples per edge; the sharpen accents it), little at
  75 %.

**Which holds 60 fps where** (on the Air at 2560×1600, the defaults otherwise):

- **100 %** holds it nowhere measured (34–46 fps).
- **Stretch 75 % / Sharpen 75 %** reach 60 only in the flight (Stretch, 62 unthrottled); the dam is at 54–58
  and the spawn at 43–47.
- **Stretch 67 %** holds 60 at the dam and in the flight (59.8 and 59.4 with vsync); the river spawn,
  the heaviest view (water filling the lower half), is at 55 (its GPU frame 15.9 ms, p95 16.7: on the edge).
  **Sharpen 67 %** costs ~2 ms more: 58–59 unthrottled at the dam and in the flight, 48 at the spawn.
- **Stretch 50 %** holds 60 in every view (59.4–59.6 with vsync, GPU 8–12 ms, room to spare), and so
  would **Sharpen 50 %** at the dam and in flight (66–72 unthrottled) but not quite at the spawn (14.3 ms GPU,
  57.6 unthrottled).

So 67 % is the threshold where 60 begins to hold; the spawn view needs 50 %, or 67 % with one of the cost
profile's other fixes (water Medium's per-pixel cost at reduced resolution, a real Shadows Low: −2 to −3 ms).
The owner's pick between the soft Stretch and the ~2 ms Sharpen is a look question.

Scratch files (not checked in): the scenario and configurations in the session's scratchpad `up/`
(`scenarios.json`, `configs.json`, `summ.py`), the report at `up/un/compare.html`.

## Graphics presets

The owner's "on the graphics one, provide a slider of presets from low, to mobile, to ultra, etc.
prioritize these presets by maintaining beauty as much as possible with each step down, focusing on big
performance gains" (2026-11-07). Settings → Graphics's **Preset** slider runs Low → Mobile → Medium → High →
Ultra; the presets are data (`shared/settings/graphics-presets.json`, loaded and validated by
`client/src/ui/graphicsPresets.ts`: each preset sets every field of a fixed vocabulary by name, each value
checked against its setting's own choices, so a water tier, scale or upscaler that goes away fails with the
entry). Moving the slider applies a preset and stores its id (`dwell.graphicsPreset`, since 0.50.31): the settings it
sets then follow the preset's current definition here, so retuning a preset reaches the players on it; a setting
changed by hand is pinned and reads **Custom** (ARCHITECTURE.md §5, the game menu: defaults or pinned). New players start on High on desktops and Mobile on phones;
the exposure and the water's flow switches are outside the presets. `?preset=<id>` draws a visit at a preset
(the bench's runs below), whatever is stored.

| Preset | Render scale | Water | Sun & moon shadows | Torch shadows | Light shafts | Bloom | Clouds | Full detail | Distant detail | Distant memory |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Ultra | 100 % | Medium (High until 0.50.29) | High | High | High | High | High | 352 m | 2 px | 512 MB |
| High (desktop default) | 50 % Stretch | Medium | High | Low | High | High | High | 256 m | 4 px | 256 MB |
| Medium | 50 % Stretch | Medium | Low | Off | Low | Low | High | 192 m | 6 px | 192 MB |
| Mobile (phone default) | 50 % Stretch | Low | Low | Off | Low | Off | Low | 128 m | 8 px | 96 MB |
| Low | 50 % Stretch | Low | Off | Off | Off | Off | Off | 96 m | 12 px | 64 MB |

**The order of the cuts**, by GPU time saved per beauty lost, from the [cost profile](#cost-profile-m1-air),
[render scale](#render-scale-and-upscaling) and [water cost](#water-cost-m1-air) measurements: the render
scale first (~74 % of the frame scales with the pixels; 50 % keeps the look at a distance and is the owner's
default), then torch shadows (water High, ~0.1 ms over Medium at 50 %, was folded into Medium in 0.50.29), the
sun's shadows to Low and the shafts to Low (Medium), then the water to Low (since 0.50.29 the see-through Low:
blended with no scene copy, caustic lines on the water, no land march, one ripple layer, no streaks; it waits
for Mobile), the clouds, and the draw distance last. Sharpen is in no
preset: at 50 % it costs ~2 ms more than Stretch (the render-scale table), more than the next cut saves; a
player can still pick it. On a phone the detail sliders clamp to its ranges (Ultra's 512 MB to 192), and the
preset still reads as Ultra there.

**Measured** (0.50.28, M1 Air on AC, Chrome on the GPU, a 1280 × 800 page at pixel ratio 2, so 2560 × 1600;
seed 0 from the dam site, `?at=2364.5,142,7443.5`, a scratch scenario that goes to the nearest water and
looks down 40° from 6 m, the river across most of the screen, noon; `npm run bench -- --compare` with one
configuration per preset, `?preset=`; 1.5 s to settle, 4 s recorded; one page each, unthrottled and with
vsync). `gpu.frame` mean / p95 (ms):

| Preset | Unthrottled | Vsync | Drawn fps (vsync) | Memory |
| --- | --- | --- | --- | --- |
| Ultra | 25.78 / 26.26 | 26.18 / 27.48 | 59.9 | 1.96 GB |
| High | 10.36 / 11.10 | 10.52 / 11.12 | 59.8 | 1.20 GB |
| Medium | 9.05 / 9.63 | 8.97 / 9.72 | 59.8 | 1.00 GB |
| Mobile | 6.41 / 7.19 | 7.50 / 9.93 | 59.8 | 0.77 GB |
| Low | 4.49 / 5.22 | 6.54 / 9.41 | 59.6 | 0.69 GB |

**Reading it.** Each step down saves GPU time, and the biggest is the first: Ultra → High is −15.4 ms (2.5×),
almost all of it the render scale. High → Medium −1.3 ms with the river's look unchanged in the shots
(refraction, caustics and reflections kept; the shadows softer); Medium → Mobile −2.6 ms, where the water
turns opaque (Water Low); Mobile → Low −1.9 ms, a plain sky and no shadows. With vsync every preset drew
~60 fps here (the M1 overlaps frames, so even Ultra's 26 ms held 60 with two in flight; it has no headroom),
and the cheap presets read higher with vsync than unthrottled (the GPU clocks down when idle). Memory falls
with the detail: 1.96 GB at Ultra to 0.69 GB at Low. Not measured: a real phone, night (torch shadows), a
flight.

Scratch (not checked in): the scenario and configurations in the session scratchpad `settings/`
(`scenarios.json`, `configs.json`, `an.py`), the runs in `settings/unthrottled/` and `settings/vsync/`
(`compare.html`, each preset's shot beside High's).

## Flight loading (M1 Air)

The owner (2026-11-07, 0.50.12): "Just flying around loading chunks, still getting 45fps even with default
settings + 50% scaling"; their performance line read 41–59/60 fps with 1–16 `GPU-wait` skips, CPU 9.3–11.7 ms
and GPU 18.6 → 7.8 ms over 15 s, at a slider speed of ~36 m/s.

**Method.** The M1 Air on AC power, the bench's Chrome with vsync (what the owner sees), a 1280×800 page at
ratio 2 (2560×1600), `?renderscale=0.5&upscaler=stretch`, the default settings (no `settings`), a fresh
seed-0 world at its own spawn. Per run: 6 s recorded just after the join (`start`), `settle`, fly on, the
speed level set before the climb, Space for a climb, W held, 1 s, then 15 s recorded (`flight`) in a straight
line over unloaded terrain. Levels fly at 8 × 2^(L/2) m/s: 4 (32 m/s, the owner's), 6 (64), 9 (181) and 11
(362, the slider's top). The 1-minute load was 2.1 at the start and 5–8 while the bench ran (the bench's own
workers). Scratch: the session's scratchpad `fl/` (`gen.py`, `an.py` for the per-frame reading, `thr.py`).

| Level | Flown | Drawn fps | Drawn interval p50 / p95 / max | Over 18 ms | GPU-wait skips | `frame.cpu` mean / p95 | `mesh_dirty` | `jobs.apply` | `lod` | Uploads a frame | `gpu.frame` mean / p95 | `pacing.done` p50 / p95 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 | 434 m (then a hill) | 52.4 | 16.7 / 33.3 / 49.9 | 198 of 785 | 112 | 10.96 / 16.9 | 6.08 / 11.0 | 1.80 / 4.1 | 0.97 | 1.12 MB, 4.7 chunks | 12.23 / 15.27 | 18.5 / 36.9 |
| 6 | 960 m | 57.9 | 16.7 / 17.7 / 34.2 | 30 of 869 | 30 | 10.53 / 14.9 | 6.50 / 10.3 | 1.79 / 3.4 | 0.64 | 1.53 MB, 6.3 chunks | 10.25 / 12.08 | 18.0 / 24.8 |
| 9 | 1520 m | 56.0 | 16.7 / 33.3 / 34.3 | 59 of 841 | 59 | 11.55 / 15.8 | 7.53 / 10.9 | 1.86 / 3.9 | 0.69 | 1.32 MB, 5.3 chunks | 8.18 / 10.83 | 18.2 / 33.4 |
| 11 | 4510 m | 57.1 | 16.7 / 17.7 / 51.0 | 37 of 859 | 34 | 9.22 / 15.9 | 5.85 / 9.9 | 1.09 / 3.2 | 0.61 | 0.74 MB, 2.1 chunks + 1.3 sections | 7.70 / 10.65 | 14.7 / 34.1 |
| 4, 2.5 min in (stopped by a hill: nothing loading) | 0 | 59.9 | 16.7 / 18.0 / 18.7 | 43 (≤ 18.7) | 0 | 3.32 / 4.0 | 0 | 0 | 1.08 | 0 | 9.16 / 9.50 | 11.7 / 12.7 |

The `start` records (the 6 s after joining, every level): 54.8–58.8 fps, `frame.cpu` 9.4–10.7, `gpu.frame`
9.6–10.5 (p95 12–14; `gpu.shadows` 4.0–4.3 against 1–2.6 in flight, the kept shadow maps redrawn as chunks
arrive). No long task over 50 ms in any record; `ticks` 0.1 ms except at level 11 (mean 0.7, up to 11 ms: the
prediction's terrain collision loading at 360 m/s). Workers: `job.chunk` queued p50 33–150 ms, p95 170–320 ms
(the generation pool always full while flying), run p50 17–28 ms; `chunk_mesh` queued p50 3–7, run p50 17;
`section` run p50 33–75 ms.

**Reading it.**

- **The bench reproduces the owner's CPU**: `frame.cpu` 9–11.5 ms a frame in every flight (their 9.3–11.7),
  against 3.3 ms once nothing loads. Speed hardly matters: the stream always has more to do than its budget, so
  level 4 costs what level 11 does. It is **all loading work on the main thread**: `mesh_dirty` 5.9–7.5 ms
  (p95 10–11, max 14), `jobs.apply` (taking finished meshes in and uploading them) 1.1–1.9, `lod` 0.6–1.0, the
  draw ~1. The Chrome trace of level 11 (8 s, `fl11t`) gives the main thread 4.4 s busy (55 %): **the flood
  light `LightMap::load` 2.05 s, 47 % of it** (`run_from` 1.65 s self, `rules` 0.23, `set_level` 0.08),
  `padded_box` (copying a chunk out for its mesh job) 0.30, `encode_job` 0.24, `interleave` + `writeBuffer`
  (uploads) 0.28, `LodSystem::update` 0.35, GC 0.09.
- **Why `mesh_dirty` is ~6 ms with a "4 ms" budget**: `stream_budget_ms` gives the light **and** the mesh
  starts 4 ms each (`game.rs`, `chunk_stream.rs` `mesh_dirty`), so up to 8 ms a frame, plus the last chunk's
  overshoot past each deadline (the worst frames reach 14 ms against the 8).
- **The `GPU-wait` skips are the CPU's**, not the GPU's: `gpu.frame` is 7.7–12 ms. A frame skips when two are
  still unanswered (`render/pacing.rs`), and `pacing.done` (submit → Chrome's answer) runs ~18 ms, 6–10 ms past
  `gpu.frame`. Over all 11 records, a frame after one of ≤ 12 ms `frame.cpu` was skipped 22 times in 4,500
  (14 of them at level 4); after one over 15 ms, 45–70 % of the time; over 16 ms, 45–100 %. A frame that ends late submits late, its answer
  comes late, and the next-but-one frame finds two in flight.
- **The owner's lower rate** (41–53 against the bench's 52–58): the same CPU, but at level 4 low over the
  ground the bench also drew only 52.4 (112 skips, plus 123 late animation frames). The cut-off is
  sharp (skips start at ~12 ms of CPU when `pacing.done` is 18 ms), so a slightly longer answer on their
  Chrome (their own profile, other tabs, a larger window) moves it a lot. DevTools were not open. Their GPU
  18.6 / 14.4 ms in the first 5 s was not reproduced (the bench's join reads 9.6–10.5, p95 ≤ 14); a trace
  of theirs, or their `pacing.done`, would show it.
- **The machine is full while loading**: in the trace the job workers ran 42 s in 8 s (about 5.2 of the
  M1's 8 cores: four at ~92 %, five more at 7–54 %), Chrome's GPU process 2.2 s and the main thread 4.4.
  Generation and meshing get 4 workers each (`workerCount`, cores − 2 capped at 4, per pool), so the main thread
  and the GPU process compete with them for cores. `?workers=N` (1–8, a pool) sets it for a visit;
  at level 4, one page each (load 3.9 → 7.3): 4 workers drew 57.6 fps (34 skips, `frame.cpu` 9.98, settled in
  10.2 s), 3 drew 54.4 (83, 10.14, 15.3 s), 2 drew 51.8 (120, 11.30, 18.4 s). **Fewer workers did not help**:
  the main thread's loading work is set by its budgets, not by the cores left over, and the view loads slower.
  The two level-4 pages at 4 workers drew 52.4 and 57.6: one run's noise at this level is ~5 fps.
- **Server and client** (owner: "joining players wouldn't be running that"). In the level-11 trace, per thread
  (cores busy over the 8 s; threads told apart by their functions):

  | Side | Thread | Busy | Doing |
  | --- | --- | --- | --- |
  | Server (local world) | the local server's worker | 0.07 core (~1.2 ms a frame) | ticks, `on_datagram`, collision loads |
  | Server | its jobs worker (most likely: a fifth generation thread) | 0.53 | chunk generation for the server |
  | Server | the water worker | 0.01 | `Water::step` |
  | Client | the page's main thread | 0.54 (~9 ms a frame) | the flood light, uploads, mesh job starts, LOD, the draw |
  | Client | 4 generation workers | 3.64 (0.91 each) | `Volume::cell`, `ColumnFbm3` (the client generates its own terrain from the seed) |
  | Client | 4 meshing workers | 0.97 | `mesh_cells`, `mesh_section_above` |
  | Browser | Chrome's GPU process, compositor, browser | 0.41 | |

  So the server side is ~0.6 of a core and none of the main thread's time; the client side is ~5.2 cores.
  **A joining player pays nearly all of it**: level 4 joining a native `dwell-server` on the same Mac (seed 0,
  `--visibility none`; the page `?local=0&join=…&cert=…&rtc=…&ice=…` from its invite link) drew 55.1 fps with
  71 `GPU-wait` skips, `frame.cpu` 9.40 / 15.5, `mesh_dirty` 5.11, `jobs.apply` 1.60, 1.26 MB uploaded a
  frame, `gpu.frame` 12.26 — inside the local runs' spread (51.8–57.6 fps, `frame.cpu` 9.98–11.30). The local
  server's workers do compete for the M1's 4 performance cores (the 4 client generation workers alone keep 4
  cores ~91 % busy, and the server's jobs worker adds half a core), but the main thread's time did not grow
  with it: fewer workers (above) and no local server (this run) left `frame.cpu` at 9.4–11.3 ms. The main
  thread's extra time is its own loading work.

**Fixes proposed** (ranked by the main-thread time they save while loading; 1 and 4 built in 0.50.14, below):

1. **One stream budget, not two, and a frame-aware one**: the light and the mesh starts share
   `stream_budget_ms`, and the budget shrinks with the frame's time so far (e.g. spend only up to ~9 ms of
   `frame.cpu`). Saves ~2–4 ms a frame on average and caps the p95 near 10 ms, under the ~12 ms where the
   skips start: most of the 30–110 `GPU-wait` skips in 15 s go. The light then lags further behind the
   generation; check the time to settle (the bench's `settle`) and the join's `terrain` stage.
2. **The flood light off the main thread** (`LightMap::load`, 47 % of it while loading): in the mesh worker
   with the chunk's padded neighbours, or its own worker. ~3.5–4 ms a frame; larger work (the light map is the
   main thread's state; edits relight it).
3. **Smaller main-thread pieces of the uploads and the job posts**: interleave the vertices in the mesh worker
   (`interleave` 0.17 s in 8 s), transfer the padded chunk rather than encoding it (`encode_job` 0.24 s): ~0.5 ms
   a frame together.
4. **Skip the LOD selection while it cannot change** (`LodSystem::update` 0.35 s in 8 s, ~0.6 ms a frame,
   still 1.1 ms with nothing loading): the cost profile's finding again.

**Built in 0.50.14: one frame-aware stream budget, and the LOD skipping a selection that cannot change.**
The light and the mesh starts share one budget (`dwell-game` `stream_budget.rs`, ARCHITECTURE.md §7.4): the
loading stops `STREAM_DEADLINE_MS` (8 ms) after the frame's start, whatever ran before it (`jobs.apply`, the
ticks), at least `STREAM_MIN_MS` (2 ms, or 0.15 of a slow page's frame); the light leaves `STREAM_MESH_SHARE`
(0.4) of it to the mesh starts, and each stops before an item its running cost estimate says would pass the
deadline. `LodSystem::update` keeps the last selection while the camera (to the ½ m cell and 0.1°), the
settings, the LOD's messages and job results and the chunks (`drawable_revision`) are unchanged and nothing
waits. Same method as above (vsync, 2560×1600, 50 % Stretch, seed 0), one run each, levels 4 and 9; *before*
is main at 0.50.13 (two runs; the second one's with the lag steps), *after* the 8 ms deadline (a 7 ms one in
brackets). Load 3.6–9 during the runs. Scenarios: the scratchpad's `fl/sb.json` (`fl4s`, `fl9s`: the flight
above, then W released, `until ahead`, `until behind`, `settle`, 1 s recorded standing still).

| Flight (15 s) | Drawn fps | Over 18 ms | GPU-wait skips | `frame.cpu` mean / p95 / max | `mesh_dirty` mean / p95 / max | `jobs.apply` | Chunks uploaded |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Level 4, before | 55.7, 53.9 | 168, 192 | 63, 90 | 9.41 / 15.2 / 19.2, 10.47 / 15.9 / 20.5 | 5.06 / 9.8 / 12.3, 5.73 / 10.5 / 13.2 | 1.59, 1.77 | 4,567, 4,437 |
| Level 4, after | 59.6 (59.8) | 141 (136) | 5 (1) | 9.62 / 11.8 / 15.0 (8.50 / 10.4 / 12.8) | 5.12 / 7.6 / 9.0 (4.51 / 6.7 / 8.2) | 1.47 (1.27) | 3,921 (3,810) |
| Level 9, before | 58.4, 57.2 | 144, 159 | 23, 42 | 10.32 / 14.5 / 17.4, 10.91 / 15.0 / 17.5 | 6.65 / 10.2 / 12.6, 7.08 / 10.6 / 12.4 | 1.68, 1.77 | 5,573, 5,197 |
| Level 9, after | 59.9 (59.9) | 132 (146) | 0 (0) | 9.03 / 10.4 / 14.2 (8.20 / 9.6 / 13.0) | 5.73 / 7.5 / 7.9 (5.16 / 6.6 / 7.8) | 1.26 (1.11) | 3,447 (3,374) |

The "over 18 ms" left after are the rAF's own 18.6 ms intervals (vsync jitter, `raf-late`: 132–141 of them in
every run, before too); the slow intervals the pacing caused (`gpu-wait`) went from 13–63 to 0–3. `gpu.frame` is
unchanged (12.1–12.3 ms at level 4, 8.1–8.7 at level 9).

- **Loading lag.** Fewer chunks are uploaded in flight (−12 % at level 4, −34 % at level 9), but the view
  catches up as soon: after W is released, `until ahead` (no level-1 section in view waiting for chunks) came
  ~2.0 s (level 4) and ~1.8 s (level 9) after the flight's end before, ~2.4 s and ~1.4 s after (from the steps'
  times, ±0.5 s). The uploads dropped are most likely re-meshes (a chunk meshed again as its neighbours are lit),
  which the light's reserve for the mesh starts and the shorter light slices cut; a measured backlog of the
  streamer (unlit and dirty chunks) would tell, and is not in the bench yet.
- **Join.** The `terrain` stage 3.53–4.17 s before, 3.47–3.86 s after (3.89–4.09 at 7 ms); the first `settle`
  (the LOD idle after joining) 8.4–11.0 s before, 8.2–9.6 s after. The `start` record (6 s after joining):
  `frame.cpu` 9.3–10.8 → 9.1–9.6 ms, GPU-wait skips 0–28 → 0.
- **Standing still** (the `after` record, nothing loading): `lod` 0.9–1.0 ms a frame before, 0.00 at level 9
  after (every selection skipped; 0.64 at level 4, still settling there), `frame.cpu` 2.6–3.5 ms.
- **Why 8 ms, not 7:** 7 ms left `frame.cpu` at 8.2–8.5 with the same frame rate; 8 ms brings it to ~9 (the
  target) with a few more chunks a second and still almost no skips (they start at ~12 ms of `frame.cpu`).

**Next** (not built): the flood light off the main thread (fix 2; still about half of `mesh_dirty`), and the
vertices interleaved in the mesh worker (fix 3). One chunk's light (`LightMap::load`) is not split into slices:
the cost estimate stops before a chunk that would pass the deadline, and the worst `mesh_dirty` after is 9 ms.

## Blasts and floods (M1 Air)

The owner (2026-11-07, 0.50.14): "What about our heavy load times dynamite and water floods." Same method as
[Flight loading](#flight-loading-m1-air): the M1 Air on AC power, the bench's Chrome with vsync, 2560×1600 (a
1280×800 page at ratio 2), `?renderscale=0.5&upscaler=stretch`, the default settings, a fresh seed-0 world,
noon; one run each. Load 2–6 (the bench's own workers). Scratch: the session's scratchpad `bf/` (`gen.py`
the scenarios, `an.py` the reading).

- **Blasts** (`bf-red`, `bf-blue`, `bf-yellow`, `bf-chain`, `bf-cliff`): at the spawn, the charge placed
  1.5 m ahead looking down 50° and lit with the torch, then the player flies up and ~15 m back and looks at
  it; 10 s recorded from just after lighting (the blast ~1.5 s in), the server sampled each second. The
  chain is four red charges ~1 m apart, one lit. The cliff is a 7 × 9 × 10 m stone block built by `region
  fill`, the charge in a hole 1–1.5 m into its face (fewer pieces: the rock round it holds most cells).
- **Flood** (`bf-flood`, `?at=2364.5,142,7443.5`, the dam site's river): a `region clear` of a trench 30 m
  across the river, 3 m wide and from 1.5 m under the bed to 2 m over the banks, then 15 s recorded with the
  camera 14 m up and 18 m back: the river pours both ways into the dry trench.

**The client (the page's main thread and GPU; a joining player pays only this):**

| Scenario | Drawn fps | Over 18 ms (over 25) / max | GPU-wait skips | `frame.cpu` mean / p95 / max | `mesh_dirty` p95 | `jobs.apply` p95 | Uploads p95 (MB a frame) | `gpu.frame` mean / p95 | `gpu.shadows` mean / p95 | `pacing.done` p95 | Puffs max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Red, flat | 59.8 | 65 (0) / 18.8 | 0 | 4.33 / 6.7 / 10.9 | 1.4 | 2.3 | 2.3 | 14.0 / 16.7 | 2.5 / 5.2 | 19.9 | 59 |
| Red in a cliff | 59.8 | 70 (0) / 18.8 | 0 | 3.72 / 5.4 / 9.9 | 0.1 | 1.7 | 1.5 | 11.9 / 15.3 | 1.4 / 4.5 | 18.4 | 59 |
| Blue (4 m destroyed, no pieces) | 59.8 | 45 (0) / 18.7 | 0 | 3.69 / 4.4 / 7.4 | 0 | 0 | 0 | 11.5 / 12.1 | 0.5 / 0.5 | 17.6 | 59 |
| Yellow, flat | 59.9 | 27 (0) / 18.6 | 0 | 3.99 / 6.3 / 11.2 | 1.3 | 2.3 | 2.3 | 14.1 / 17.2 | 2.6 / 5.7 | 20.2 | 59 |
| Chain of 4 red | **58.1** | 112 (**22**) / **35.2** | **22** | 4.05 / 7.2 / 11.0 | 2.1 | 2.6 | 2.6 | 14.3 / **18.3** | 2.7 / 5.6 | **30.6** | 176 |
| Flood (15 s) | 59.8 | 117 (1) / 33.3 | 1 | 4.00 / 7.9 / 12.8 | 3.6 | 4.2 | 4.2 | 10.7 / 11.9 | 0.6 / 0.6 | 19.9 | – |

No long task in any record. The "over 18 ms" intervals are vsync's 18.6 ms jitter except where counted over
25.

**The server (the local world's worker; per second from the blast; never on the page's main thread):**

| Scenario | Ticks a second (min) | Tick mean, worst second | Worst tick | Physics peak / sum (ms) | Terrain collision peak / sum | Pieces sum | Water worker longest run |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Red, flat | 59 | 4.64 | 35.5 | 177 / 831 | 126 / 391 | 44 | 2.3 |
| Red in a cliff | 60 | 1.03 | 12.4 | 41 / 105 | 4 / 19 | 4 | 2.3 |
| Blue | 60 | 0.34 | 8.7 (a save) | 4 / 31 | 4 / 12 | 1 | 2.3 |
| Yellow, flat | 60 | 2.06 | 26.9 | 82 / 393 | 23 / 116 | 27 | 2.3 |
| Chain of 4 red | 60 | 4.69 | 46.6 | 164 / 1,063 | 102 / 704 | 63 | 2.4 |
| Flood | 60 | 0.46 | 14.3 (a save) | 3 / 38 | 2 / 11 | 1 | **42.2** (nothing owed) |

Snapshots (the broadcasting) took 4–12 ms in the whole 10 s of every blast, `water+respawn` 3–8. Network bytes
a tick are not in `serverStats()` yet (not measured).

**Reading it.**

- **On the Air at 50 % Stretch the blasts and the flood already hold 60 fps on the client**, except the
  chain of four: 58.1 fps, 22 `GPU-wait` skips. Those are the GPU's, not the CPU's: the frame before each
  skip took 4.4 ms of `frame.cpu` (p50 4.1), `gpu.frame` p95 18.3 ms and `pacing.done` p95 30.6. What the
  pieces add on the GPU is their shadows: blue (no pieces) draws `gpu.frame` 11.5 ms and `gpu.shadows` 0.5,
  red and yellow 14.0 and 2.5 (p95 5.2–5.7), with 1.85 cascades drawn a frame against blue's 1.0: a
  falling piece's shadow-caster identity held its pose, so every flying piece changed the signature of the
  kept far cascades and one was drawn again every frame (`farPerFrame`), all its terrain with it (555
  thousand triangles a frame against 43). Fixed below. The smoke and dust (176 billboards at
  the chain's peak) did not show in a pass of their own (`gpu.near` 7.9 in every blast).
- **The main thread is not the limit in a blast or a flood**: `frame.cpu` p95 5.4–7.9 ms, max ≤ 12.8, well
  under the ~12 ms where the pacing skips start ([Flight loading](#flight-loading-m1-air)). The crater's
  re-meshing runs inside the shared stream budget already (`mesh_dirty` p95 ≤ 3.6 ms; the edits mark chunks
  dirty, `chunk_stream.rs` `write`, and the 0.50.14 deadline starts their jobs), as do the flood's: the
  flood's water arrives as chunk modifications, re-meshed the same way (`jobs.apply` p95 4.2, uploads p95 4.2
  MB a frame while the trench fills).
- **The server keeps 60 ticks a second** in every scenario, but a blast's ticks run 2–5 ms on average for
  4–8 s and its worst 27–47 ms (the blast's own tick: the cells cleared, hundreds of bodies made, and their
  first contacts). The cost is physics (Rapier: 90–180 ms a second for red, 1 s of it in the chain's 10 s)
  and the pieces' terrain collision builds (up to 126 ms a second). On the page this is another core, not
  frame time; a worst tick of 47 ms delays a snapshot by ~3 ticks, which the clients' interpolation absorbs.
- **The flood is cheap**: the water worker's longest run is 42 ms (17 at the breach) but on its own worker,
  owing nothing; the server's tick is unchanged (0.2–0.5 ms).

**Built in 0.50.15: falling pieces are moving shadow casters.** A piece's caster identity is its id and
part, not its pose (`render/mod.rs` `piece_shadow_tag`), so a kept far cascade holding flying pieces is
drawn again every `refreshFrames` (4 on High), as for other players (`Want::Moving`), not every frame; the
nearest cascade, drawn every frame, shows them exactly. A piece that comes or goes still changes the
signature. After (one run each, same scenarios):

| Scenario | Drawn fps | Over 25 ms / max | GPU-wait skips | `gpu.frame` mean / p95 | `gpu.shadows` mean / p95 | Cascades drawn a frame | Shadow triangles a frame | `pacing.done` p95 | `frame.cpu` mean / p95 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Red, before → after | 59.8 → 59.8 | 0 → 0 / 18.8 → 18.7 | 0 → 0 | 14.0 / 16.7 → 13.2 / 16.6 | 2.5 / 5.2 → 1.9 / 5.4 | 1.85 → 1.58 | 555k → 391k | 19.9 → 19.5 | 4.33 / 6.7 → 4.21 / 6.5 |
| Yellow | 59.9 → 59.8 | 0 → 0 / 18.6 → 18.7 | 0 → 0 | 14.1 / 17.2 → 12.8 / 16.2 | 2.6 / 5.7 → 1.7 / 4.9 | 1.85 → 1.52 | 555k → 354k | 20.2 → 19.7 | 3.99 / 6.3 → 4.00 / 6.0 |
| Chain of 4 red | **58.1 → 59.3** | **22 → 7** / 35.2 → 35.3 | **22 → 7** | 14.3 / 18.3 → 13.3 / 17.1 | 2.7 / 5.6 → 1.9 / 5.4 | 1.87 → 1.60 | 568k → 400k | **30.6 → 25.4** | 4.05 / 7.2 → 4.39 / 7.4 |

The server is the same build either way (red: worst tick 35.5 → 33.6 ms, physics 831 → 939 ms over the 10 s;
chain 46.6 → 34.3, 1,063 → 1,013: run-to-run spread). The chain's 7 skips left are the GPU's still
(`gpu.frame` p95 17.1, `pacing.done` p95 25.4); which pass was not isolated.

**The pieces' physics, three levers** (owner, 2026-11-07: "they're not touching anything … Box2D demos do
1000s"; "Yes do those 3 optimizations"). Natively, release, `tests/explosives.rs` `measure_blast_cost` (the
flat world; red flush 186 pieces, red buried 510, yellow flush 100), each lever alone (the env switches were
temporary), tick mean / p95 / worst (ms), physics and terrain collision summed until every piece is a block
again, and that time:

| Lever | Red flush | Red buried | Yellow flush |
| --- | --- | --- | --- |
| As shipped | 0.96 / 2.86 / 14.4; 213 + 106 ms; 11.0 s | 5.29 / 13.79 / 27.6; 2,136 + 169; 8.6 s | 0.59 / 1.59 / 47.7; 144 + 16; 10.4 s |
| CCD off | 0.95 / 2.86 / 13.0; 206 + 107; 11.0 s | 5.30 / 13.80 / 27.6; 2,128 + 171; 8.6 s | – |
| Sleeping allowed | 0.51 / 2.19 / 12.8; 232 + 107; **21.6 s** | 4.96 / 13.97 / 27.6; 2,168 + 178; 9.3 s | 0.70 / 1.60 / 46.3; 143 + 15; 8.4 s |
| No piece contact 0.1 s | 0.91 / 3.47 / 10.3; 374 + 378; **19.6 s** | 5.17 / 13.86 / 17.0; 2,325 + 186; 9.1 s | 0.68 / 1.93 / 16.8; 114 + 54; 7.7 s |
| No piece contact 0.3 s | 1.02 / 3.53 / 5.6; 266 + 400; **16.3 s** | 4.73 / 14.01 / 17.1; 2,343 + 186; 10.0 s | 0.50 / 1.90 / 4.5; 84 + 54; 9.2 s |
| No piece contact 1 s | 1.33 / 4.64 / 6.9; 558 + 382; **16.6 s** | 4.77 / 13.89 / 17.5; 2,335 + 186; 10.0 s | 0.59 / 2.01 / 3.5; 80 + 53; 7.1 s |
| Solver iterations 2 (of 4) | 1.02 / 2.56 / 19.0; 180 + 100; 9.5 s | 4.32 / 13.13 / 15.6; 2,039 + 147; 10.1 s | 0.41 / 1.38 / 10.3; 97 + 14; 12.2 s |
| Solver iterations 1 | 1.30 / 3.86 / 16.2; 332 + 214; 11.2 s | 6.46 / 15.56 / 27.1; 2,554 + 159; 8.1 s | 0.80 / 1.64 / 40.3; 179 + 11; 7.7 s |

- **CCD only for fast pieces: nothing to save.** Turning CCD off for every piece changed nothing within
  noise: Rapier's CCD only works for bodies moving fast enough to need it, which is what the lever would do.
  Left on (no piece tunnels).
- **Sleeping: nothing to save.** A piece turns back into blocks once still for the settle's `sleepTicks`
  (`felling.rs`), when Rapier would put it to sleep; the physics total is the same, and in one run a piece
  resting on sleeping ones jittered until its time ran out (21.6 s). Left off.
- **A short no-contact window: cuts the worst tick natively, costs more in the game.** On the flat ground
  0.3 s took the worst tick from 15/28/48 to 6/17/4.5 ms, but red's flung pieces then pile inside one another
  (settled 11 → 16 s; the shipped `a_surface_blast_…` test left 4 unsettled at 20 s), as EXPLOSIVES.md §3.1
  found at 1 s. Yellow looked safe natively and was tried in the game (0.3 s, yellow only): on the spawn's
  hillside in Chrome, two runs, physics peaked at 162–166 ms in the second after the blast against 82
  without, 491–528 ms over the 10 s against 393, the worst tick unchanged (26–31 ms: the blast's own tick),
  the client the same (59.8–59.9 fps, 0 skips). The pieces overlap when the window ends and the solver pushes
  them apart. **Not adopted** (EXPLOSIVES.md §3.9).
- **`enhanced-determinism`** stays: the controller's prediction is held bit-identical natively and in
  WASM, and the pieces share its `Physics`. It also turns off Rapier's SIMD paths (and the browser's worker
  cannot run its parallel solver anyway); SIMD typically gives ~1.5–2× on the solver, so it may cost ~⅓–½
  of a blast's physics time (estimated, not measured).

**Next** (not built; ranked by what they would save on the Air):

1. **The chain's last skips** (7, GPU): `gpu.frame` p95 17.1 ms at the blasts; the pieces' draws (one each,
   in the scene and the nearest cascade) as one instanced draw would be the next thing to measure.
2. **The blast's own tick** (27–47 ms worst, the same with every lever): spread it — the cells cleared in one
   tick but the pieces' bodies made over the next few ticks (nearest the charge first), and the terrain
   collision builds round them capped per tick (`terrainChunksPerTick` holds only new chunks).
3. **Fewer, larger pieces**: merge neighbouring shell cells flung the same way into one body (a 2 × 2 × 2
   piece is one body where eight were); the pieces' count drives both physics and terrain collision.
4. **A server-only physics build without `enhanced-determinism`** (SIMD; on a native dedicated server also
   the parallel solver): pieces are server-authoritative and the client stands in only for those near it, so
   the server's piece world need not match the client's bit for bit; a larger change (two `Physics` builds).
5. **Baked physics** (docs/FUTURE.md): out of scope.

## Worst-case blasts (M1 Air)

The owner (2026-11-07, 0.50.15): "I'm getting a lot lower than 60FPS for blasts, did you try more than 1
piece of dynamite? And if you blow up next to water it's probably worst case." Same method as [Blasts and
floods](#blasts-and-floods-m1-air) (the Air, vsync, 2560×1600 at 50 % Stretch, default settings, noon, one
run each), but load 5–11 (a build had just run: the numbers are a little pessimistic). Scratch: the
session's scratchpad `wc/` (`gen.py` the scenarios, `an.py` and `srv.py` the reading). The charges are set
by `region fill` with the dynamite slot (offsets from the feet), the middle one lit with the torch, then the
player flies up and back over the water and looks at them; 12 s recorded.

- **By the river** (`?at=2364.5,142,7443.5`, the dam site; the player on the west bank, the river at +x):
  one red 2 m in from the water (`wc-bank1`), one 1 m in (`wc-edge1`), 4 red 3 m apart along the bank
  (`wc-bank4`), 8 red along it (`wc-bank8`), and 8 mixed (4 red, 4 yellow) in two rows
  with the player only ~8 m back (`wc-bank8mix-close`).
- **Dry land** (the spawn): the same 8 mixed in two rows (`wc-dry8mix`).

**Before (0.50.15):**

| Scenario | Drawn fps | Over 25 ms / max | GPU-wait skips | `frame.cpu` mean / p95 / max | `mesh_dirty` p95 | `jobs.apply` p95 | `gpu.frame` mean / p95 | `gpu.near.water` mean / p95 | `gpu.shadows` mean / p95 | `pacing.done` p95 | Puffs max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| One red, 2 m from the river | 58.0 | 23 / 35.3 | 23 | 3.64 / 6.6 / 11.2 | 1.2 | 2.7 | 14.6 / 18.0 | 6.4 / 7.7 | 1.1 / 5.1 | 29.8 | 59 |
| One red, 1 m from it | 58.4 | 18 / 34.9 | 18 | 3.76 / 7.1 / 12.0 | 1.7 | 3.0 | 14.2 / 17.1 | 6.4 / 7.8 | 0.8 / 1.5 | 29.2 | 59 |
| 4 red along the bank | 53.3 | 79 / 35.3 | 79 | 3.75 / 7.9 / 13.2 | 2.0 | 3.8 | 15.8 / 21.0 | 8.0 / 12.8 | 0.9 / 4.9 | 37.5 | 229 |
| 8 red along the bank | **49.5** | 125 / 49.9 | 126 | 4.15 / 8.9 / 12.8 | 4.0 | 4.3 | 17.3 / 25.8 | 9.4 / 17.9 | 1.1 / 5.3 | 47.5 | 432 |
| 8 mixed on the bank, close | **48.9** | 132 / 51.9 | 133 | 4.15 / 9.8 / 12.1 | 4.8 | 4.0 | 17.4 / 27.2 | 9.7 / 19.5 | 0.9 / 5.0 | 49.9 | 459 |
| the same, Smoke and dust off | 58.5 | 18 / 33.5 | 18 | 4.06 / 9.0 / 12.4 | 4.6 | 3.6 | 13.7 / 17.5 | 6.1 / 7.8 | 0.9 / 5.0 | 27.5 | 5 |
| 8 mixed, dry land | 54.9 | 60 / 35.3 | 60 | 5.14 / 8.5 / 12.0 | 3.2 | 3.5 | 14.9 / 20.8 | 4.9 / 9.2 | 2.5 / 5.8 | 35.3 | 467 |

No long task in any record. The water worker's longest run (its own thread) was 26–47 ms by the river (7–11
before the blasts), owing nothing; the server's ticks by the river stayed under 10 ms (the bank charges fling
few pieces: the water spares and shields the cells, EXPLOSIVES.md §3.7–3.8). On dry land the 8 charges' pieces
cost the server 7–11 ms a tick on average for 8 s (physics 2.6 s and terrain collision 1.3 s over the 12 s;
worst tick 66–257 ms, in one run 52 ticks in its worst second) — a worker, not the page's frames.

**Reading it.** The frame drops are the GPU's, and they follow the blasts' **smoke and dust puffs**: 59 for one
blast, 229 for four, 432–467 for eight (22 smoke 1.6–3.2 m wide and 36 dust a red charge, each lit with the
soft shadow lookup a pixel and blended over the others). The main thread stays under the pacing's ~12 ms
(`frame.cpu` p95 ≤ 9.8 ms; the craters' re-meshing in the stream budget, `mesh_dirty` p95 ≤ 4.8). With
Smoke and dust off the close 8 by the river drew 58.5 fps against 48.9. By the river the puffs cost about
twice: with see-through water the flames' mask (before the water) rasterised every puff again only to
discard it (`flame_mask_fs` keeps only flames, embers and motes), and the colour went after the water.

**Built in 0.50.16:**

- **A puffs budget shared by the blasts in flight** (data: `limits.maxPuffs` 64, `limits.minPuffShare` 0.15,
  `shared/explosives/explosives.json`; `dwell-client` `blasts.rs` `Blasts::start`): a blast set off while
  others' puffs are in the air draws the share of its counts left, at least 0.15; fixed at its start, so
  nothing pops; a lone blast draws all of them. Test `blasts_in_flight_share_the_puffs_budget_and_none_pops`
  (red without the budget: 342 puffs at once in a chain of eight).
- **The flames' mask leaves the puffs out** (`render/flames.rs` `mask_ends`, also the flecks and waterfalls,
  which it discarded too). Test `render::tests::the_flames_mask_leaves_the_blast_puffs_out` (red with the old
  range).
- Tried and dropped: one shadow compare a puff pixel instead of the soft grid — within noise (bank 8 53.2 →
  53.6 fps).

**After** (the budget at 120 / 0.2 first, then the final 64 / 0.15 with the mask):

| Scenario | Drawn fps: before → 120 → final | GPU-wait skips | `gpu.frame` mean / p95 (final) | `gpu.near.water` p95 (final) | `pacing.done` p95 (final) | Puffs max (final) |
| --- | --- | --- | --- | --- | --- | --- |
| 4 red along the bank | 53.3 → 55.2 → **57.7** | 79 → 57 → 27 | 14.5 / 17.8 | 8.4 | 31.1 | 89 |
| 8 red along the bank | 49.5 → 53.2 → **56.1** | 126 → 82 → 47 | 14.6 / 17.9 | 9.3 | 34.4 | 111 |
| 8 mixed on the bank, close | 48.9 → 53.4 → **55.8** | 133 → 78 → 49 | 14.7 / 18.9 | 9.2 | 33.7 | 130 |
| 8 mixed, dry land | 54.9 → 58.4 → **58.9** | 60 → 18 → 11 | 13.3 / 16.8 | 4.5 | 26.0 | 132 |

`frame.cpu` was unchanged (p95 8–11 ms). **Not 60 yet by the river**: a single blast there draws 58.0–58.4
(18–23 skips) and the close eight with no smoke at all 58.5, so by the water ~1.5–2 fps of the remaining
gap is the river view itself (`gpu.near.water` ~6.4 ms, `gpu.frame` p95 ~17.5 at rest there) and the rest
the 90–130 puffs still drawn. **Next** (not built): the puffs' lighting per puff rather than per pixel (the
frame's shadow map is fragment-only now), fewer, larger smoke puffs at the same cover; on the server, the 8
dry charges' pieces (worst tick 66–257 ms; the levers in [Blasts and floods](#blasts-and-floods-m1-air)'s
Next, 2–4).

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

## The leaf cards (`?cards=`)

`?cards=1` meshes the leaf cards whatever the renderer's tier says (`shared/look/leaves.json` `cards.tiers`: off on the
phone tier and a CPU adapter), and `?cards=0` meshes none; set at page load, before the world streams in. A bench
`--compare` configuration with `params: {"cards": "1"}` measures their cost on the `--touch` phone page (BUGS.md #72). It
wins over the Leaf cards setting (Settings → Graphics; the presets' `leafCards`, off on Low).

## The GPU heightfield (`?heightfield=`)

`?heightfield=1` draws the near water's surface from the GPU heightfield whatever the tier says
(`shared/look/watertiers.json` `heightfield`), and `?heightfield=0` the meshed tops
(proposals/WATER_GPU_SURFACE.md C6a); set at start, before the world streams in. The `heightfield`
configuration in `client/bench/configs.json` sets it, so `npm run bench -- --compare water-river
water-flood --configs heightfield` measures it against the tier's (`raster`). `water-flood` cuts a
29 × 29 m pit under the river's level beside the seed-7 river and records the 12 s of the fill: chunk
uploads (`upload.chunks`), `jobs.apply`, `frame.cpu` and `gpu.near.water` are its figures.

## Water cost (M1 Air)

The owner's "cheaper water … break down what's most expensive" (2026-11-07). Main at 0.50.17, Chrome on
the GPU, `--unthrottled`, a 1280 × 800 window at pixel ratio 2 (2560 × 1600), the owner's
`?renderscale=0.5&upscaler=stretch` (1280 × 800 drawn) unless named, seed 0, noon, water **High**, load
2–4.5, on AC. One page per view; each piece switched off in turn mid-run (`water` with `debug`, i.e.
`?waterdbg=` names; `view` with `flowParticles`, `flowRipples`, `flowFoam`, `waterfalls`), 1.5 s to settle,
3 s recorded, `gpu.frame` mean. The full analysis, the options and the plan are
[`proposals/WATER_GPU_SURFACE.md`](./proposals/WATER_GPU_SURFACE.md).

Ranked by the dam close-up (6 m over the river, 40° down; water ~⅓ of the screen), ms saved with the piece
off:

| # | Piece | Dam, 50 % | Dam, full 2560 × 1600 | Valley (river ~5 % of the screen) |
|---|---|---|---|---|
| – | The whole frame at High | 9.07 | 20.71 | 10.42 |
| 1 | All water shading (`flat`: a constant colour, copy kept) | 2.02 | 6.62 | 0.67 |
| 2 | Water Low (no copy, no tiles, opaque) | 1.73 | 5.78 | 1.33 |
| 3 | Land reflections (`ssr`) | 0.45 | 1.24 | 0.24 |
| 4 | The fixed part: scene copy + tiles (Low − `flat`, valley) | ~0.6 | | 0.66 |
| 5 | Flow foam streaks | 0.27 | 0.73 | 0.27 |
| 6 | Ripple layers 2–3 | 0.13 | 0.79 | 0.12 |
| 7 | Reflected clouds | 0.09 | 0.49 | 0 |
| 8 | High over Medium | 0.11 | 0.78 | 0.04 |
| 9 | Caustics | 0.04 | 0.17 | 0 |
| – | Refraction, absorption, foam, sparkle, glint, sky, `under`, flecks, waterfalls | noise (±0.2) | noise (±0.3) | noise |

**The waterline's deep view** (the 0.50.17 PR's unexplained +0.7–1.6 ms), at [5051.5, 31.9, 22980.5]: the
deep record (1.5 m under, 8° down) read 5.28, 5.27, 5.23, 5.25 ms across two passes, the surface
6.84 and 6.83. The deep view is cheaper than the surface; the earlier rise was drift on a warm laptop (the
deep record was last in each run). Under water the whole water costs 0.5–0.6 ms (Low, `flat`).

**Moving water on the CPU** (a trench cut across the river at the dam, the water pouring in for 12 s):
923 `chunk_mesh` jobs (77 a second, 12.6 ms each on the workers), main thread `jobs.apply` + `mesh_dirty` +
`upload.chunk` 1.3 ms a frame, `frame.cpu` p95 7.6 ms (2.15 / 3.3 still); still 47 re-meshes a second 3 s
later. Each eighth's change re-meshes the whole terrain chunk. Natively (`dwell-mesh`, a 32³ chunk, a lake
8 cells deep): calm water 2 triangles, a wave across the columns' eighths 4,880 (3,328 of them side faces
between columns), choppy 6,144; the chunk's water mesh 743–936 KB as built.

Not measured: a tall waterfall (none in view; the flood's trench filled level), network bytes of water edits
(no per-message counter), a real phone (the proposal's §1.4 estimates it).

Scratch (not checked in): scenarios and configurations in the session scratchpad `wc2/` (`gen.py` writes
`scenarios.json`: `wc-dam`, `wc-wide`, `wc-wl`, `wc-flood`, `wc-descent`; `dam.json`, `damfull.json`,
`wl.json`; `an.py` summarises), the selected screenshots in `water-cost-shots/` (each view with each piece
off, the flood, the waterline, the descent grid).

## Water tiers (M1 Air)

The tiers rework (0.50.29; WATER_RENDERING.md §4, proposals/WATER_GPU_SURFACE.md C1): the same dam close-up
and valley as "Water cost", 50 % Stretch, `--unthrottled`, one page per view with the tiers switched in turn
(the `water` step by tier name; Medium first and again last), 1.5 s to settle, 3 s recorded, `gpu.frame`
mean. **The owner was playing a game on the Mac** (load 11–76 over the runs), so the absolute ms are high and
need a recheck on a quiet machine; read the differences within one page (each row's run).

| `gpu.frame` mean (ms) | Dam | Valley |
| --- | --- | --- |
| *Before* (0.50.28): High / Medium / opaque Low / High again | 9.62 / 9.33 / 7.87 / 9.68 | 10.94 / 10.79 / 9.62 / 10.92 |
| *After*: Medium / Low / opaque (CPU) / Medium again | 10.05 / 8.98 / 8.52 / 10.61 | 12.54 / 10.92 / 10.54 / 12.41 |
| Low − Medium (after, mean of the two Medium records) | −1.35 | −1.56 |
| Low − opaque (after) | +0.46 | +0.38 |
| With vsync (60 Hz; frames held at 16.7 ms) — before: High / Medium / opaque Low / High again | 9.81 / 9.48 / 8.33 / 9.55 | 11.18 / 10.89 / 9.76 / 11.32 |
| With vsync — after: Medium / Low / opaque / Medium again | 10.93 / 8.54 / 8.42 / 10.30 | 11.82 / 10.33 / 10.32 / 11.91 |
| Low − Medium with vsync | −2.08 | −1.54 |
| A Low with the scene copy (an intermediate build, refraction and absorption kept, no SSR) − Medium | −0.71 | +0.09 (noisy) |

Low draws no scene copy: **no water pass split** (two scene passes, as with no water), and the caustic tile
(`gpu.water.tiles` ~0.45–0.5 ms on the frames it is drawn, every other frame since 0.50.29). Its cost over the
opaque tier is the blend and the surface caustics. The intermediate copy-reading Low saved only about half as
much: the copy and split cost ~0.6 ms even on the M1, and on a phone's tile GPU a split stores and reloads the
whole frame (not visible on the M1's bandwidth).

**A phone** (*estimate*): the owner's iPhone ran ~45 fps floating over the water looking down and ~60 under
it, "at the original defaults", so ~5.5 ms a frame over the surface. **Caveat:** a phone's default water was
Low before this change too, which was then the opaque tier (no copy, no split); if the phone was on it, the
copy was not the cause, and the cost lies in what the opaque tier and Low share (the blended full-screen water
shading, the sky reflection per pixel, the near-surface waterline test). If it was on Medium (set by hand, or
`prefersTouch` false on that device), scaling the M1's Medium − Low (1.35–1.56 ms) by a phone GPU's ~2–3× per
pixel gives ~3–4.5 ms, plus the split's store and reload that the M1 hides: Low should bring it near 60 fps.
Which tier the phone ran needs asking; not measured on a phone.

Screenshots (old High, old opaque Low, new Medium, new Low, new opaque) in the session scratchpad
`water-tiers/shots/`; the runs and `compare.html` per run in `water-tiers/out/` (`before-*`, `final-*`),
scenarios from `water-tiers/gen.py`, `an.py` summarises.

## Adventure vs creative (M1 Air, phone-like)

Owner, 2026-11-08: adventure felt ~15 fps slower than creative (on an iPhone). Measured 0.50.34 on
seed 7 with the `creative` and `adventure` configurations (`bench/configs.json`, `?mode=`) plus the
owner's `?renderscale=0.5&upscaler=stretch`, `--unthrottled --cpu-throttle 4 --touch`:

- Idle at the spawn and sprinting, with no items lying about: no adventure cost beyond noise. The
  HUD's inventory (`hud().inventory` and `Hotbar.setInventory`) is ~18 µs a frame at 4× (a JS CPU
  profile); the hotbar and backpack touch the DOM only on change (a MutationObserver saw the same
  ~1 mutation a frame in both modes); the server sends `Inventory`/`ItemDrops` only on change.
- After breaking 30 blocks (`break-drops`: their drops lie as loose items): the shadows drew
  **161 k triangles a frame against creative's 99 k** (gpu.shadows mean 0.95 vs 0.81 ms). Any loose
  item anywhere counted as a moving caster, so every far cascade was drawn again every
  `refreshFrames` for the item's 300 s life. Fixed in 0.50.35 (`render/mod.rs` `casters_moving`):
  loose items only spin in place and the nearest cascade (drawn every frame) holds them. After:
  99 k vs 99 k, gpu.frame 3.36 vs 3.36 ms, frame.cpu 4.40 vs 4.21 ms (adventure first; the second
  page of a compare runs ~0.5–1 ms CPU slower under throttling, so swap the order to check a gap).

## Chisel, the biome editor (`client/chisel.html`)

A second page of the client build (Phase 56 B3, ADR 0074): `npm run build:wasm`, `npm run dev`, then
`/dwell/chisel.html` (query `?seed=7&size=16384&overlay=1&cx=0&cz=0` sets the view: size in m, overlay
0 blocks, 1 biome weights, 2 temperature, 3 wetness, 4 rainfall: the rain shadow, ADR 0075; its wind
and rain sliders are in the biome map group; overlays other than blocks need two biomes). The left panel edits the biomes (sliders made from
`shared/worldgen/biomes.mjs`'s ranges, block pickers from the registry); the right shows the region
from the real generator in a worker, 64² texels then 256², redrawn ~60 ms after a drag (each pass
~0.1 s on an M1). "Add from preset" adds one of the preset biomes (`shared/worldgen/biomes/presets/`, listed by label
with its description beneath) as a starting point. Export writes `{ biomes, biomeMap }`; Import takes that or one biome file; both are
validated by the rules `gen.mjs` uses. Edits persist in the browser (`dwell.chisel`); Reset returns to
the build's. `document.body.dataset.tiles` counts the tiles drawn, for a script waiting on the preview.

**The world overview** (right of the preview): the whole 100 km disc, island and sea, from the same edit
and overlay, 128² then 512² texels (~0.2 km), redrawn after the detail preview on every edit (the worker
runs the detail first, the overview in 128² chunks that a newer edit cancels, cached by the edit state,
so switching back to an overlay is instant). The white rectangle is the detail region; click or drag on
the overview to move it there. Its status line gives each pass's time (M1, headless Chromium, the
build biomes plus six presets: 128² in 25–34 ms, 512² in 120–290 ms);
`document.body.dataset.overview` is `<pass>:<ms>` once a pass is whole, for a script waiting on it.
Face A only, like the detail preview.

**Relief and the 3D section** (0.52.19; owner, 2026-11-10: "our biome tooling doesn't give you a good 3D
perspective of the terrain, should probably have a more 3D perspective of a section"). The view bar's
**relief** button (on by default; `?relief=0` off) hill-shades the detail preview and the overview
whatever the overlay: each dry texel's colour scaled by its slope's light from a sun in the north-west,
35° up (`chisel_section::hillshade`, central differences over a ring one texel wider than the tile;
water flat), so mesa walls, cones and ridges read. The **3D** button opens a WebGPU view of a square
(256 m to 8 km, the size select beside it; `?s3d=1&sx=&sz=&ssize=`) centred on the detail's centre;
a click on the detail moves it (its square is drawn there in yellow). The worker's `section` lane
(after the detail, before the overview; a newer edit drops the older passes) samples the real generator
(`ChiselPreview.section`: `column_lod`, the detail's path) coarse (4 m, or 256² columns) then fine
(2 m, 1 m for 256 m, or 512² columns), heights snapped to the ½ m cells, and meshes it blocky: one
top quad a column in its top block's colour, a wall down to each lower neighbour, a baked sun shadow
(a march toward the sun over the heights), water quads at each wet column's level drawn translucent.
Sky light plus the same sun, a distance haze; drag orbits, shift- or right-drag pans, the wheel zooms.
Heights are true scale up to 2 km squares, ×2 at 4 km and ×3 at 8 km (the status line says so), so a
700 m cone 7 km wide reads. Trees are not drawn (skipped: the placement is per block, not cheap to
sample at 2 m). M1, Chrome on the GPU, 1 km at 2 m (512² columns, ~0.7–1.5 M triangles): the 4 m pass
in 58–68 ms, the 2 m pass in 300–330 ms, its upload and first draw 6–16 ms;
`document.body.dataset.section` is `<texel>:<build ms>:<draw ms>` for a script.

**The Textures tab** (0.52.42, ADR 0074 B6; `?tab=textures&tex=grass_top`): the painterly materials
and palette (`shared/textures/`) edited with the build's own painter (`paint.mjs`). Pick a block
material; its layers show as sliders (count, widths, lengths, angles, opacity, …; a [min, max] pair as
two) and palette pickers; the palette group shows the material's ramp (or every ramp) as colour
pickers, an art-direction warning outlined in red and listed. The right shows the tile, a 3 × 3 repeat
and the block on a cube, repainted on every change; `document.body.dataset.texture` is
`<material>:<far colour>`. With the 3D section on, an edit re-tints its blocks ~150 ms later. Export
downloads `materials.json` and `palette.json` (drop them into `shared/textures/`, run
`node shared/textures/gen.mjs`); edits persist in the browser (`dwell.chisel.textures`, set aside when
the build's data changes); Reset textures returns to the build's.
