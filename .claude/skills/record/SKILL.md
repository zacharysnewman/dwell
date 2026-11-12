---
name: record
description: Record a showcase video of Dwell in Chrome on the GPU — cinematic camera moves (dolly, orbit, track, drone rise), a clean frame (no HUD, no text, clouds off, golden hour), clips spliced with cross-fades into one mp4 in the owner's format (9:16, 1080×1920, a minute or less). Use when the owner asks for a video, clips or a recording of something in game; update this skill with what you learn each time.
---

# Record a showcase video

Owner's format (2026-11-12): **portrait 9:16, 1080×1920, h264, 30 fps**, **1 minute or less**, no music,
and **no text of any kind in the video** (no title card, captions or labels — owner: "Don't include any text
or anything in the video"). Send it with SendUserFile: the upload limit is **30 MB**, so re-encode
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
`?clouds=off`. Golden hour reads best: `d.setTime(0.265–0.3)` sunrise, `0.68–0.73` evening.

**Finding subjects:** `d.creatures()` (near animals: x, y, z, yaw, speed), `d.ecology()` (herds and packs,
their ids and positions), `d.lava('report')`, spots in `client/bench/spots.json`.

## 3. Camera moves

`d.placeCamera([x, y, z], yaw, pitch)` places the camera (radians; the player's eyes until cleared). Drive
it every frame from the page with `requestAnimationFrame` so the move is smooth regardless of the recorder:

```js
// A move: t from 0 to 1 over `s` seconds; pose(t) → [pos, yaw, pitch].
(s, pose) => new Promise(done => { const t0 = performance.now(); const f = () => {
  const t = Math.min(1, (performance.now() - t0) / (s * 1000)); const [p, yaw, pitch] = pose(t);
  d.placeCamera(p, yaw, pitch); t < 1 ? requestAnimationFrame(f) : done(); }; f(); })
```

- **Dolly in:** lerp the position toward the subject at eye height (1–3 m), yaw fixed on it. Low and slow reads best.
- **Orbit:** angle = a0 + t·span around the subject at radius r and height h, yaw = angle + π (look in). Keep it
  **high or on open ground**: close orbits on hills pass through terrain and trees (trim, or raise h).
- **Track / follow:** each frame read the subject from `d.creatures()` and sit behind-side of it at 4–8 m,
  yaw toward it, eased (lerp 0.1 a frame) so it doesn't jitter.
- **Drone rise:** from 5 m to 60–150 m over the subject, pitch easing from −10° to −60°.
- For portrait, frame tall: subjects centred, more sky or ground above and below, slower horizontal moves.

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
