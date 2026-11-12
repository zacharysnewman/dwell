---
name: record
description: Record a showcase video of Dwell in Chrome on the GPU — cinematic camera moves (dolly, orbit, track, drone rise), a clean frame (no HUD, no text, clouds off, golden hour), clips spliced with cross-fades into one mp4 in the owner's format (9:16, 1080×1920, a minute or less). Use when the owner asks for a video, clips or a recording of something in game; update this skill with what you learn each time.
---

# Record a showcase video

Owner's format (2026-11-12): **portrait 9:16, 1080×1920, h264, 30 fps**, **1 minute or less**, no music,
and **no text of any kind in the video** (no title card, captions or labels — owner: "Don't include any text
or anything in the video"). One exception: text that is part of an in-world effect, like a measuring
line's height numbers (owner: "Text could be in that as part of the 'measure' effect, just no description or
titles"). Send it with SendUserFile: the upload limit is **30 MB**, so re-encode
(`crf 28`) if it's larger. Never commit video files to the repo.

## 1. Run the game

Use the warm bench preview (`docs/DEV_TOOLS.md`, the `bench` skill): `npm run bench:warm` from `client/`
builds once and serves on :4174 (stop it after with `--stop`). Open a local world by link (never menu
clicks): `?fresh=1&world=…&seed=…&at=x,y,z` — **`at`'s y must be above the ground** (a y inside the ground
drops you through to face B, BUGS.md #82). Music off: `localStorage['dwell.music'] = '"off"'` before load.

## 2. Drive Chrome and record

`scripts/driver.mjs` is a small Playwright driver: a headed Chrome with WebGPU, a CDP screencast recorder
and an HTTP control port.

```sh
node .claude/skills/record/scripts/driver.mjs 'http://localhost:4174/dwell/?fresh=1&seed=7&at=…' OUT_DIR 1080 1920
curl -s localhost:4999/eval -d 'd.setTime(0.28)'        # body = JS; `d` is __dwell
curl -s 'localhost:4999/rec/start?name=c1_dolly'      # frames → OUT_DIR/c1_dolly/f00000.jpg …
curl -s localhost:4999/eval -d '<camera move, below>'
curl -s localhost:4999/rec/stop                        # → {frames, seconds, fps}
curl -s localhost:4999/quit
```

(The chrome-devtools MCP `screencast_start/stop` also works, but it only saves inside the project folders
and records at the page's device size; the driver gives a fixed viewport.)

**Clean frame:** hide the HUD — every element but the game canvas, re-applied every 300 ms (the game
re-shows some), e.g. `setInterval(() => document.querySelectorAll('body > *:not(canvas):not(#game)')
.forEach(e => e.style.visibility = 'hidden'), 300)` — check the canvas's id first. Clouds off unless the
shot is about clouds (owner: "The clouds make those screenshots hard to read"): the graphics settings or
`localStorage['dwell.clouds'] = '"off"'` before load (there is no `?clouds=` parameter, `?preset=high` turns them
back on, and the driver starts a fresh profile each launch, so set it again and reload). Golden hour reads best: `d.setTime(0.265–0.3)` sunrise, `0.68–0.73` evening.

**Finding subjects:** `d.creatures()` (near animals: x, y, z, yaw, speed), `d.ecology()` (herds and packs,
their ids and positions), `d.lava('report')`, spots in `client/bench/spots.json`.

## 3. Camera moves

Use **theater mode's camera engine** (`d.theater`, docs/THEATER_MODE.md, docs/DEV_TOOLS.md "Theater"): a
camera path played into the view frame-rate independently, with eased moves, a field of view, the viewer's time
of day and speed ramps — not ad-hoc per-frame `placeCamera` scripts.

```js
// A preset around a subject, from where the camera is now (place it first to choose the side):
d.placeCamera([x, y + 3, z - 20], 0, -5);   // degrees, face-local
const path = d.theater('preset', { name: 'orbit', params: { radiusM: 25, heightM: 8, seconds: 15 }, subject: 'player' });
d.theater('hud', { shown: false });          // the clean frame (one CSS rule; 'stop' shows it again)
d.theater('play', { path });                 // poll d.theater('state', {}).finished; 'stop' at the end
```

Presets (`shared/theater/presets.json`): `orbit`, `dolly`, `track`, `rise` (a crane), `pan`, each with
`seconds`, `fov`, `timeFrom`/`timeTo` (a time-lapse of the light) and its own parameters. For anything else write
a path: keyframes `{t, position, look | target, fov, timeOfDay, speed}` and segments `{move, easing, …}` (cut,
linear, ease, orbit, track, dolly with `vertigo`, crane, pan). A target can be `"player"`, a point, or
`{creature: id}` / `{npc: id}`. Paths are JSON: keep the ones that worked next to the plan for the next video.
The bench's `theater` step plays one inside a scenario.

**The in-game recorder** (theater mode T-3, docs/DEV_TOOLS.md "Theater"): for a quick clip of one path, skip the
screencast — `await d.theaterMode('record', { path, aspect: '9:16', height: 1080, fps: 30, download: false })`
returns `{ bytes (base64 MP4 on Chrome), mime, width, height }` (write the bytes to a file from the driver; with
`download: true` the browser downloads it). The canvas renders at the take's size (9:16 at 1080 is 608×1080; the owner's
1080×1920 is `height: 1920`) with the HUD switched off. It is real time: on a slow page use
the screencast, which is frame-exact. First tried 2026-11-12: a 10 s orbit at 9:16 1080p/30 fps → a 7 MB MP4.
**The HUD:** use `d.theater('hud', { shown: false })` (or theater mode), not inline-style scripts; the page's CSP
blocks injected `<style>` (the T-1 rule never applied for that reason).

- **Dolly in:** the `dolly` preset (`fromM` → `toM`, `heightM` 1–3 m). Low and slow reads best.
- **Orbit:** the `orbit` preset. Keep it **high or on open ground**: close orbits on hills pass through terrain
  and trees (trim, or raise `heightM`).
- **Track / follow:** the `track` preset on the subject (`distanceM` 4–8, `smoothingS` ~0.6): it stays centred,
  and the lag is exponential, the same at any frame rate.
- **Drone rise:** the `rise` preset (`fromM` 5 → `toM` 60–150, `backM`): it looks at the subject as it climbs.
- **Time-lapse of the light:** `timeFrom`/`timeTo` (and `days`) on any preset, or `timeOfDay` on keyframes; a
  `speed` ramp on keyframes for a shot that speeds up part way.
- For portrait, frame tall: subjects centred, more sky or ground above and below, slower horizontal moves.

## 3b. Time-lapse and the player

Owner (2026-11-12): **part of every recording should be gameplay from the player's perspective** (first person,
the arms and held item, actually playing: walking up, chopping, digging, swimming), mixed with the cinematic
shots. Use **full or partial time-lapses** where they suit (a whole clip sped up, or a normal-speed
shot that speeds up mid-way) and **include the player in the video when possible**.

- **Time-lapse in the game:** drive the clock (`d.setTime(t)` stepped per frame for day→night), speed up the
  world's processes by their dev knobs (`?growth=N` for trees and grass, the world's water speed, `d.lava('rate',
  …)`), then record at normal speed. **Time-lapse in the splice:** a clip's `speed` (e.g. 4) or a ramp
  (`speedFrom`/`speedTo`) in the plan — or `setpts` by hand — for a shot that speeds up partway.
- **The player in frame:** a third-person camera on the player's own body (behind-over-the-shoulder, or a
  slow orbit around them while they walk, chop or swim), or first person with the arms and the held item
  doing something. The player's position is `d.player()`/the snapshot; walk them with the input steps or a
  scripted `d.walk` if available (check `client/src/main.ts`'s dev API and add a small tested step if needed).

## 4. Splice

`scripts/splice.py` cuts each clip (start, end), scales and crops to the format, cross-fades 0.8 s, fades out,
and writes one mp4 plus a still per clip (leave `title` out of the plan: no text in the video):

```sh
python3 .claude/skills/record/scripts/splice.py plan.json
```

`plan.json`: `{"out": "…/video.mp4", "size": [1080, 1920], "fps": 30, "clips": [{"src": "OUT_DIR/c1_dolly", "from": 0, "to": 15}, …]}` — `src` is a frame
folder from the driver (or an mp4). Then check the size; re-encode with `crf 28` if over 30 MB.

## 5. Judge and report

Look at a still from every clip before sending. Delete the raw footage and segments once the owner has the
video (owner: "Delete that footage so it doesn't take up hard drive space"). Report honestly what shows well and what didn't happen or
looks broken, and log bugs in `docs/BUGS.md` (the first video found four: #79–#82). Clean up: stop the
preview and the driver, remove any `client/node_modules` symlink or built `client/src/pkg` in a worktree.

## Lessons so far

- 2026-11-12, deer and wolves (landscape, 85 s): dollies and high orbits worked; close orbits on hills clipped
  terrain; animals mostly stand and mill, so 15 s clips feel static — plan action (chases, fleeing) or shorter
  clips; on day one wolf packs were kilometres from herds, so no hunt happened (WORK_QUEUE item 39 adds roaming
  packs and deer that flee from players).
- 2026-11-12, Realistic world and old-growth forest (portrait, 5 clips, ~55 s): theater paths with `ease` (drone
  passes) and `crane` (rises through a canopy) worked first time. Paths need `"formatVersion": 1`; yaw is face-local
  (`FaceFrame::look`): 0 = +z, 90 = +x, 180 = −z. The big map is **M** (`d.press('KeyM', true)`, then `Minus` to zoom
  out), not F4 (the old height overlay). Warm a far drone path's start first (cut to its first keyframe, ~10 s) or
  it starts on bare LOD. `?at=` into a dense forest put the eye inside the ground or a crown; the world's own spawn
  (a beach at the forest's edge) gave the best walk-in (`d.press('KeyW', true)` with `d.look` easing the view). The
  screencast fell to ~15 fps on a high rise over the forest; with the theater HUD off no arms showed in first person.
- 2026-11-12, Realistic world + forest (owner): "didn't show the forest for half of it. And it was slow frame rate in the
  forest though it looks really pretty." Give the subject most of the video, not half; in a heavy scene (the forest)
  the page runs under 60 fps, so record frame-exact (step the theater path per captured frame or slow the clock and
  speed up in the splice) rather than real time, and check the clip's fps before splicing.
