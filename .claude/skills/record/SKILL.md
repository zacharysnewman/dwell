---
name: record
description: Record a showcase video of Dwell in Chrome on the GPU — cinematic camera moves (dolly, orbit, track, drone rise), a clean frame (no HUD, no text, clouds off, golden hour), clips spliced with cross-fades into one mp4 in the owner's format (9:16, 1080×1920, a minute or less). Use when the owner asks for a video, clips or a recording of something in game; update this skill with what you learn each time.
---

# Record a showcase video

Owner's format (2026-11-12): **portrait 9:16, 1080×1920, h264, 30 fps**, **1 minute or less**, **no music, but
the game's sound effects and ambience on** (owner, 2026-11-13: "showcases shouldn't have music, but sound effects and
ambiance are good"; §0b), and **no text of any kind in the video** (no title card, captions or labels — owner: "Don't include any text
or anything in the video"). One exception: text that is part of an in-world effect, like a measuring
line's height numbers (owner: "Text could be in that as part of the 'measure' effect, just no description or
titles"). Send it with SendUserFile: the upload limit is **30 MB**, so re-encode
(`crf 28`) if it's larger. Never commit video files to the repo.

## 0. The fast path: re-record a video with one command

A video that has been recorded before has a **manifest**, `videos/<video>/video.json` (owner, WORK_QUEUE item 50:
"Yeah build thst"): the world link and its localStorage, the capture size and fps, and the shots in order — each
one's kind (`"path"`: a theater path file; `"take"`: a take script for gameplay, like the portal drop, with its
`args`), its setup (`reload`, `url` for another link — e.g. an `at=` on face B, `timeOfDay`, `view` for `d.setView`,
`hud`, `warmS` to hold a path's first keyframe while the LOD streams, `eval` for anything else) and its splice parts
(`from`/`to`, `speed`, `vf`, `cut`; a `label` names a part of one take). Re-recording needs no agent in the loop:

```sh
node .claude/skills/record/scripts/take.mjs bifacial --only s5 --cache "$SCRATCH/record"   # just the landing, re-spliced
node .claude/skills/record/scripts/take.mjs bifacial                    # records missing/stale shots, reuses the rest
node .claude/skills/record/scripts/take.mjs bifacial --splice-only      # only re-splice (after editing a part's from/to)
node .claude/skills/record/scripts/take.mjs bifacial --dry-run          # what would record or be reused, and the plan
node .claude/skills/record/scripts/take.mjs --clean bifacial            # after the owner approves: delete the clips
```

It starts the warm preview (`npm run bench:warm`, :4174) and the driver (:4999) if they aren't up (and quits a driver
it started; `--keep` leaves it), records each shot named by `--only` (a shot id, or a part label: `s5` is part of
the `portal` take, so the whole take re-records), plus any shot with no cached clip or a **stale** one (its recipe —
world, capture, setup, args, path or script file — changed since; changing a splice part never re-records). Each shot's
frames are encoded to `<cache>/<video>/<shot>.mp4` (h264 crf 18 at the measured frame rate, the splice's timeline)
right after capture and the frames deleted; then `splice.py` splices the whole video to `<cache>/<video>/<video>.mp4`
and the path and length are printed. The cache is `--cache`, `$DWELL_RECORD_CACHE`, or `<tmp>/dwell-record`.
`node --test scripts/take.test.mjs` tests the manifests and plans without a browser (CI runs it).

**Change a shot by editing the manifest**, not the script: e.g. the bifacial shot 5 camera is the `portal` shot's
`args.camB` / `fov` / `up` (`"a"` holds face A's up; `"auto"` takes face B's frame — upright on face B). Shot `s6` (the
drone rise) starts from shot 5's camera: move both together. **An agent is needed only for new camera spots**
(trial and error in the browser): find the spot with the driver, write it into the manifest, then re-run `take.mjs`.
A new video: add `videos/<name>/video.json` with its paths and scripts beside it (the validator names unknown keys
and missing files).

**Footage policy:** raw frames are deleted as soon as a shot is encoded; the per-shot clips stay in the cache until
the owner approves the video (so a re-record of one shot is minutes), then `--clean`. Never commit footage.

## 0a. Camera patterns

Plan shots from [CAMERA_PATTERNS.md](CAMERA_PATTERNS.md) (owner, 2026-11-13: "Use these patterns (document the
patterns)"): the owner's camera spec distilled — the camera model and lens table (Dwell's `fov` is vertical degrees:
24 mm → 53, 35 → 38, 50 → 27, 85 → 16), shot sizes, angles, moves, compounds, cuts and transitions, the trailer
structure, intent → shot, validation checks — then each pattern mapped to theater paths, presets, takes and the
splice, with what the tools can't do yet (roll, handheld noise, depth of field, motion blur, screen-space framing, …).

## 0c. Two aspects, a fade from black and a title card (the trailer)

The trailer (owner, 2026-11-13: "Let's do vertical and widescreen. Title and game sound, no music.") is one manifest
cut twice. Showcases stay 9:16 and text-free; these keys are for a trailer:

- **`aspects`** `{"9x16": [1080, 1920], "16x9": [1920, 1080]}`: record **square** (`capture.size` [1920, 1920]) and
  crop each aspect out of the same clip, so one take serves both (9:16 keeps the full vertical view, 16:9 the full
  horizontal one: frame the subject in the centre square). take.mjs splices `<video>-9x16.mp4` and
  `<video>-16x9.mp4` (`--aspect 9x16` splices one). A part's **`crop`** `{"16x9": [0.5, 0.35]}` moves that aspect's
  window (0–1, 0.5 the centre) where a subject would be cut off.
- **`splice.fadeIn`** (s): the video fades in from black, picture and sound.
- **`title`** `{"text", "fromEndS", "fadeS", "width": {aspect: share of the frame's width}}`: `scripts/title.mjs`
  renders the text in the game's own wordmark (the main menu's `.wordmark` rule from `client/src/style.css`, the
  rounded display font with its grass-and-earth extrusion) to a transparent PNG, and the splice fades it in,
  centred, `fromEndS` before the end; it goes out with the video's fade to black.

## 0b. Sound: effects and ambience, never music

Every take records **the game's own sound** — the effects bus (footsteps, chops, splashes, the room's reverb) and the
ambience beds — and **never the music or the UI** (owner, WORK_QUEUE item 40: "showcases shouldn't have music, but sound
effects and ambiance are good"; "Yes it's for the recordings"). How it works:

- **The tap** (`__dwell.audioTap({ music: false })`, `client/src/audio/mixer.ts` `Mixer.tap`): a MediaStream of the
  master mix built as a **parallel graph** — the effects and ambience buses also feed the tap's own master level,
  underwater muffle and limiter — so the speakers are unchanged and the music and UI buses never reach it (music and UI
  only with `{ music: true }` / `{ ui: true }`). The driver also starts Chrome with
  `--autoplay-policy=no-user-gesture-required`, so the AudioContext runs without a click.
- **The driver** (`/rec/start?name=…&audio=1`) records the tap with MediaRecorder from just before the screencast starts
  to just after it stops, and saves `<shot>.webm` beside the frame folder with `<shot>.audio.json` (its start in epoch
  seconds: the screencast frames' clock).
- **take.mjs** muxes each shot's sound into its clip when it encodes the frames (`scripts/audio.mjs` `encodeArgs`):
  aligned to the first frame (delayed or its head trimmed by the start difference), padded and cut to the clip's
  length, resampled to **48 kHz stereo AAC**.
- **splice.py** cuts the sound with each part, **concatenates it on hard cuts, cross-fades it with the picture**
  (`acrossfade`, the same 0.8 s as `xfade`) and fades it out with the last second. A part's speed: by default (`"auto"`)
  the sound is kept at 1×, **time-stretched** (pitch kept, `atempo`) for a speed from 0.25 to 2 (slow motion, a gentle
  speed-up) and **muted** on a time-lapse past 2× (or a crawl under ¼).
- **The manifest:** `"audio"` on the video (default `true`), on a shot (`false`: that shot's parts are silent; it
  changes the recipe, so the shot re-records), and on a splice part (`"auto"`, `"keep"`, `"stretch"`, `"mute"`). Mute a
  part whose sound is wrong for it (a dev knob's noise, a time-lapse that you want stretched instead: `"stretch"`).
- `node --test scripts/take.test.mjs` checks the alignment, the mux arguments and the splice's audio graph
  (`splice.py plan.json --dry-run` prints its ffmpeg commands) without a browser; with ffmpeg it also splices a
  test clip end to end and checks the sound lasts as long as the picture.
- **Check it by ear** before sending: a take whose page had no AudioContext (an old build without `audioTap`) records
  no `.webm`, and its clip is silent in the splice (the driver's `/rec/start` reply says "with sound (effects,
  ambience)" when it taps).

## 0d. Wait for the terrain (every shot)

Owner, 2026-11-14: the trailer was "really bad right now because the terrain looks bad and half of it isn't loaded
in". `take.mjs` now waits before each shot records until the view is **settled** — the bench's placed-shot rule
(`viewIdle`: the LOD generating, meshing and asking for nothing with nothing waiting in view, and the chunks meshed)
held for 1 s — after a path shot's first-keyframe hold (`warmS`, now 2 s by default) or a take's setup phase. A
shot's `setup.settleS` caps the wait (default 60 s; `0` skips it); the log prints `view settled 12.3` or `view NOT
settled 60.0` (the take goes on). By hand in the driver: `curl -s localhost:4999/eval -d "$(node -e "import('./.claude/skills/record/scripts/take.mjs').then(m => console.log(m.settleScript()))")"`.

## 1. Run the game (new camera spots)

Use the warm bench preview (`docs/DEV_TOOLS.md`, the `bench` skill): `npm run bench:warm` from `client/`
builds once and serves on :4174 (stop it after with `--stop`). Open a local world by link (never menu
clicks): `?fresh=1&world=…&seed=…&at=x,y,z` — **`at`'s y must be above the ground** (a y inside the ground
drops you through to face B, BUGS.md #82). Music off: `localStorage['dwell.music'] = '"off"'` before load (the driver
sets it; the recording tap leaves the music out anyway, §0b).

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
`{creature: id}` / `{npc: id}` (ids from `__dwell` client's `theaterSubjects()`: `[{id, name, npc, at}]`; the camera
follows a creature smoothed, the path's `followSmoothingS`, default 0.35 s). Paths are JSON: keep the ones that worked next to the plan for the next video.
The bench's `theater` step plays one inside a scenario.

**The in-game recorder** (theater mode T-3, docs/DEV_TOOLS.md "Theater"): for a quick clip of one path, skip the
screencast — `await d.theaterMode('record', { path, aspect: '9:16', height: 1080, fps: 30, download: false })`
returns `{ bytes (base64 MP4 on Chrome), mime, width, height }`; add `audio: true` for the game's sound on the take
(effects and ambience through the same tap, never the music: theater.json `audioMimeTypes`) (write the bytes to a file from the driver; with
`download: true` the browser downloads it). The canvas renders at the take's size (9:16 at 1080 is 608×1080; the owner's
1080×1920 is `height: 1920`) with the HUD switched off. It is real time by default; on a slow page add
**`exact: true`** (a path only): the path then moves one frame of `fps` per drawn frame and each frame is captured
and encoded by WebCodecs at i / fps, so the WebM it returns (`mime: 'video/webm'`, VP8, silent) is smooth however
slowly the page draws (THEATER_MODE.md §6; the world itself still runs in real time, so creatures move faster in it
than the camera on a slow page). Splice it like any clip (`src` an mp4/webm); add the sound from a real-time take or
leave it silent. Not yet tried on a GPU (built in a cloud session, 2026-11-13): check a still and the length
(`ffprobe`) the first time. The screencast stays the way to record gameplay (no path). First tried 2026-11-12: a 10 s orbit at 9:16 1080p/30 fps → a 7 MB MP4.
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
parts at different speeds or grades). With `"audio": true` on the plan each clip carries `"audio": "keep" | "stretch" |
"mute"` (take.mjs fills these from the manifest, §0b) and the sound follows the cuts, fades and speeds. A frame folder is encoded once and reused. Then check the size; re-encode with
`crf 28` if over 30 MB.

**Helpers:** `scripts/ev.mjs` posts JS to the driver (`node ev.mjs file.js` or `node ev.mjs -e 'expr' [/path]`; the
worktree guard refuses curl bodies); `scripts/take.mjs path.json clipName [warmS]` (the old single-path take) holds the
camera at the path's first keyframe for `warmS` (the LOD streams), starts the screencast, plays the path to its end and
stops. The driver takes `$RECORD_STORAGE` (localStorage set before each load) and `/viewport?w=&h=`. Earlier videos'
paths are in `paths/<video>/`; a video that will be re-recorded gets a manifest (section 0).

## 5. Judge and report

Look at a still from every clip before sending. Delete raw frames right after encoding (take.mjs does), keep the
per-shot clips until the owner approves the video, then delete them (`take.mjs --clean <video>`; owner: "Delete that
footage so it doesn't take up hard drive space"). Report honestly what shows well and what didn't happen or
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
- 2026-11-12, the bifacial world v4 (portrait, 6 clips, 54.3 s, 18.8 MB, takes 56–60 fps; paths, take script and splice
  plan in `paths/bifacial-v4/`; recording to splice took ~13 min). **The landing works** (#107 fixed): walking in from
  `[cx−9, 53, cz−9]` at yaw 45, the player drops, rises ~12 m out of face B's hole (apex y −56.3) and lands 7.3 m from
  the axis on the facing diagonal (face B's ground there is y −44.5), the same in 5 of 5 drops. **`d.state()` has no
  `grounded`**: detect a landing by the feet's y staying still for ~8 polls away from the axis (v3's script waited on
  `grounded` and never saw one). **The pillars hide the player** from most face-B cameras: one on the perpendicular
  diagonal had a pillar in front of the landing; one 20 m back on the incoming diagonal and 21 m up showed a ~30 px
  player. What worked: on the **landing diagonal, 14 m out and 13.5 m up** (`[cx+10, −58, cz+10]`, fov 50, `pan` on
  the player, `"up": "a"`): the head-down rise with the far trees on face B's horizon, then the player standing on the
  ground overhead beside the hole. Start the closing drone rise from that same camera aimed at the landing spot, so the
  cross-fade continues the shot. Far trees (#848) read as dark tree clumps from 15 m and as small dark squares seen
  straight down from ~150 m at night: fine at the frame's size. The worktree guard refuses python/curl commands with
  shell variables or loops: write a small script file with the paths inside and run it plainly.
- 2026-11-12, bifacial v5 with `take.mjs` (portrait, 54.4 s, 18.0 MB; owner on v4: "The camera is upside down for the
  out of the hole shot"): only the crane (s2) holds face A's up; shot 5 is `"up": "auto"` (upright on face B), camB
  [1796.3, −50.5, 20707.3]: ~17 m from the landing (7.3 m out on the +x+z diagonal, feet y −44.5) and 6 m up, framed
  between two pillars, so the player is ~210 px tall (a first try 3 m nearer gave ~260 px). s6's first keyframe follows
  camB (`"auto"`: it turns over edge-on at the rim). **Timings:** the warm client build took 34 s; then each shot took
  its length + ~10 s warm + 35–55 s to encode (s1 37 s, s2 39 s, portal 40 s, s6 55 s at `-preset medium`). Recording
  all four shots plus the splice took ~5 min. The first run failed after s1 with "fetch failed": Node's http server drops
  an idle keep-alive socket after 5 s, and the encode had left it idle 37 s. Fixed: `fetchRetrying` retries a stale-socket
  error once, and the driver keeps sockets 10 min. After a camB move, `--only s5` took **2 min 34 s**, inside the
  3-minute target, although it re-records s6 too (s6's first keyframe is camB, so it is stale). The intermediate encode
  is now `-preset veryfast -crf 16` (the splice re-encodes) and still takes 30–40 s a shot, the largest share of the time
  left. Encoding while capturing, or VideoToolbox, would cut it further.
- 2026-11-12, water and underwater, video 2 (portrait, 6 shots, 51 s, 15.0 MB at the manifest's `"splice": {"crf": 28}`;
  manifest `videos/water/`, seed 7). **Finding water:** `d.goto('water', { scope: 'world', water: 'sea' | 'river',
  move: true, maxS: 60 })` goes to the world map's nearest one (seed 7's spawn → a river at 923, 176, 6464, high in
  the hills; the nearest sea is ~17 km away at 11738, 19660, with a sandy beach and a lagoon; its river is a flat
  channel through the sand, no stepped pools). Probe the shore by columns (`d.voxel(x, row, z)`, water reads 15) before
  choosing a wade line. **Golden hour is ~0.8 on seed 7** (0.7–0.75 still read as afternoon; at 0.8 the sun sits low
  with a glint path). **Swimming:** a swimmer floats (feet ~1.3 m under the level), so W with the pitch down doesn't
  take the head under: hold `KeyC` (crouch swims down) — it reaches the bed and holds the player there while looking
  up. The surface crossing reads best as a theater path from 1 m above to 2 m below at the level (`s4`); the
  third-person treading shot needs no path tuning (target `"player"`, start 3 m behind). The first-person frames show
  the block-target box (BUGS.md #113). Timings: the first full `take.mjs water` 4 min 07 s; re-recording four shots
  2 min 25 s–2 min 49 s.
- 2026-11-12, water v2 (portrait, 5 shots, 57.1 s, 20.0 MB, with sound; owner on v1: "Water video for waterline should
  go straight down slowly into the water. Should do sunset for the water surface fly over. Water should include a river
  follow. The water video should include sound."). **Finding a river follow:** `d.goto(..., {water: 'river'})` only
  finds the nearest river column; to find one worth following, scan the world map natively (a throwaway
  `dwell-worldgen` example: `BifacialTerrain::for_generator(GENERATOR_TERRAIN, seed).local(Face::A).column_at(x, z)`
  on a 64 m grid, trace each river column downstream to the lowest neighbouring water, rank by the drop; ~10 s a seed).
  Seed 7's best: a hill river in a deep valley from 11645, −11938 (level 153) to 12044, −11548 (108), 70+ level steps
  over ~1 km, ending in a lake at ~84 m, not the sea (no seed-7 river traced to the sea from a 64 m grid); seeds 0 and
  42 gave nothing better in the first pass. The camera ~35–45 m over the water, target 2 keyframes ahead
  (`videos/water/s2_river_follow.json`), clears the valley walls; a camera off the river's line sat inside a cliff.
  **The waterline:** a vertical path, slow through the surface (a linear 32.5 → 31.5 m over 5 s between eased legs):
  the frame at the level shows the waterline across it, half sky, half the lit floor. **Sound:** the tap records the
  mix quietly (clips −49 to −61 dB mean, peaks −28 dB): the manifest's `"splice": {"gainDb": 26}` lifts the whole
  track with a limiter (→ −26 dB mean, peaks −0.7 dB). **The target box:** `view: {"targetOutline": false}` in a
  shot's setup (BUGS.md #113 fixed). Timings: the full `take.mjs water` ~4 min; `--only s3` 1 min 52 s; a re-splice
  1 min 15 s.
- 2026-11-13, the trailer v1 (`videos/trailer/`, 19 parts, ~53 s, two aspects from one square capture; notes per shot in
  each shot's `note`). **Square 1920×1920 captures** ran 17–59 fps (36 typical) and encoded in 25–60 s a shot; the
  full first pass of 18 shots took ~25 min, so re-take with `--only`. **Ground heights:** `d.voxel` read 0 everywhere
  from the driver; `npm run bench -- --warm --spot <spot>` prints each spot's eye and ground ("m over the ground at
  G") in ~20 s a spot: use it to place cameras. **Wrong-face views:** a camera or player that showed stars below and
  terrain overhead (the forest test world opened `at=` 18 m over its ground; a teleport into a mesa wall) — open a
  world above everything and fly down. **Fresh-world lava lakes start empty** (fill with `d.lava('rate', …)` first).
  **`sunLand` refuses off home** (after `moonLand`): reload between them. **`d.die()`** shows a third-person death
  camera, then respawns at the nearest shrine's steps (a free shrine shot). The region tool's outline box and a
  tree in front spoiled the dynamite row: check the line of sight first.
