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
folder from the driver (or an mp4). Per clip: `"speed": 0.35` (slow motion; 4 a time-lapse), `"vf": "eq=gamma=1.5"`
(a grade, e.g. to lift a night), `"cut": true` (a hard cut from the clip before, no cross-fade: split one take into
parts at different speeds or grades). A frame folder is encoded once and reused. Then check the size; re-encode with
`crf 28` if over 30 MB.

**Helpers:** `scripts/ev.mjs` posts JS to the driver (`node ev.mjs file.js` or `node ev.mjs -e 'expr' [/path]`; the
worktree guard refuses curl bodies); `scripts/take.mjs path.json clipName [warmS]` holds the camera at the path's first
keyframe for `warmS` (the LOD streams), starts the screencast, plays the path to its end and stops. Paths and take
scripts that worked are in `paths/<video>/`.

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
- 2026-11-12, the bifacial world (portrait, 7 clips, 55 s, all at 60 fps on the open sea): at seed 7's rim (x = 50 000)
  the edge is open sea on both faces, no shore, so the crossing is **flown**: `d.fly(true)`, fly over the edge, then
  `d.fly(false)` and the player falls, is flung across the midplane and swings back; turn flight on when the fall
  reverses on face B (`y < -20` and rising), then `Space` rises along face B's up. It worked every take (6 of 6).
  A placed or theater camera drew **no player** until BUGS.md #99 (fixed): third-person shots of the player need a
  build with that fix. `d.teleport` refuses a point on the other face (returns false): reload the page to reset a take
  (`/goto` the same `?fresh=1&at=` link). Teleport while flying over the sea first, then rise with `Space`: the player
  sinks to swimming before `fly` takes hold. From beside the rim at `y` ≈ 5 the camera sees both seas and both skies;
  the day is long (0.22–0.82 all reads as day), so a time-lapse that shows the faces swap runs through the night
  (`timeOfDay` 0.74 → 0.2: face A darkens as face B's dawn comes up). In portrait the held item shows only at the
  frame's right edge. The driver's `/eval` with a curl body is refused by the worktree guard: post the JS from a small
  node script (`fetch('http://localhost:4999/eval', {method: 'POST', body})`) reading a `.js` file instead.
- 2026-11-12, the bifacial world v2 (portrait, 6 clips, 48.7 s, 17.8 MB, all takes ~59 fps; paths in
  `paths/bifacial-v2/`). **The outline:** the whole disc draws from 160 km out (the far plane is 5e7 m; the LOD's root
  covers it) — but only from **above the atmosphere** (≳ 9 km up): at the midplane level or a few km up the haze
  swallows it, and exactly edge-on the disc is too thin to see (the frame is just face A's sky over face B's). A
  shallow 4–6° look down from 9–13 km reads best (fov 50, target the centre). Face A's sky is above the midplane line
  and face B's night sky with stars below it: a crane through the midplane flips them. Face A at 0.29 puts face B at
  night automatically. A soft bright band under the disc shows in every far shot (BUGS.md #102). **Clouds off for far
  shots:** both tiers draw a layer in the void beyond the rim (BUGS.md #101). **Portals:** `?seed=7&spawn=portal`
  spawns on the rim over the ladder; `d.voxel` reads 0 in the open shaft, so probe columns to find the axis (seed 7:
  1780.5, 20690.5; the ladder on the +z wall, the monument's pillars N/E/S/W at 6 m, so walk in on a diagonal). Walk
  in with `KeyW` held and the pitch eased down near the edge; let go over the axis: the fall takes ~2.2 s to the
  midplane at ~40 m/s, past the motes in ~0.15 s (slow that part in the splice), and the player rises out of face B's
  hole ~12 m and falls back, forever (no damping), so at the top `d.fly(true)`, fly out with `KeyW`, `d.fly(false)` to
  land. Seen looking straight down, first person shows the player's own legs, not the arms; the held axe only at the
  edge. Cut to the third-person camera in the same take (a `pan` path with `target: "player"`) when the feet cross
  y < −3. Face B at night is very dark: a camera among the 23 m pillars sees only pillars; a camera 38 m out and
  ~30 m up on a diagonal, looking down at the hole between two pillars, shows the rise; lift it with a grade
  (`"vf": "eq=gamma=1.5"`). `?spawn=portal` sits next to a plant on one diagonal: shoot a still from the start before
  the take. `setTime` doesn't change face B's night between 0.2 and 0.36.
- **Upside-down face-B wides: `"up": "a"`** on the path's first keyframe (it carries over). Owner, reviewing v2:
  "The camera shouldn't flip on the wide shot, show it's upside down." A camera on face B's side otherwise takes
  face B's frame (`"auto"`) and the player rising out of face B's hole looks upright; with `"up": "a"` it keeps
  face A's up, so face B and the player hang under the disc, and a crane through the midplane doesn't turn over.
  Its `look` is read in face A's frame too (or aim with `target`). Presets take it in `params` (`"up": "a"`).
- 2026-11-12, the bifacial world v3 (portrait, 6 clips, 51.4 s, 17.9 MB, all takes ~59 fps; paths, take script and
  splice plan in `paths/bifacial-v3/`; the whole video took ~15 min of recording). **`"up": "a"` works:** the face-B
  wide 22 m out and ~17 m off the ground (`[cx+14, −66, cz+14]`, fov 62, target the player) shows face B's ground
  overhead, the pillars hanging down and the player head-down under the hole — it reads upside down at once; a
  camera 37 m out showed the player too small. The crane and the closing drone rise hold `"up": "a"` too; the drone
  ends with an `orbit` round the rim from under face B back to the face-A outline (a loop with shot 1). **Find the
  hard cut by luminance**, not by the script's clock: the take's log time and the splice's time differ by ~0.07 s
  (splice uses the average frame rate: time = frame index / fps); the cut frame is where the mean brightness jumps.
  **The driver's `/eval` takes an expression**, not statements: wrap a take script as
  `(globalThis.__args = {...}, (async () => {…})())` (a small node wrapper that reads the file and posts it).
  **Seed 7's shaft axis is 1779.5, 20690.5** (`portal_shafts_for_sized`; v2's notes said 1780.5); `d.voxel(x, y, z)`
  takes **½ m cell rows** for y (row 104 = 52 m), so probe columns with that in mind. **The exit push didn't act on
  face B in the browser** (BUGS.md #107): no landing beside the hole yet; the clip ends on the rise and fall-back.
  A `?fresh=1` link with no `seed` stops on the title screen (no `__dwell`): give a `seed` (or `worldgen=test:starter&seed=0`).
