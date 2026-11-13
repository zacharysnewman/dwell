# Theater mode

Design for WORK_QUEUE item 41 (owner, 2026-11-12). Theater mode is an in-game camera toolset: the player's
character stays put while a free camera flies, follows paths built from presets or keyframes, sets the viewer's
own time of day and time-lapse, hides the HUD and records the canvas to a file. The `record` skill
(`.claude/skills/record/`) drives the same engine for the showcase videos (item 40).

Status: **T-1 to T-4 built.** T-1: the camera path engine `crates/dwell-theater`, the presets file, the client
wiring, the dev API `__dwell.theater` and the bench's `theater` step. T-2 and T-3 (2026-11-12, finished
2026-11-13): the mode and its panel (§2.1), the free camera and a phone's flying controls, presets and keyframes,
following a creature or NPC by id (§3.5), the path store, recording in real time and frame-exact (§6), and the
host's `theaterGuests` world setting (§7). T-4: the creative simulation viewpoint (§7.1). T-5 (an MCP server) is a
nice-to-have, not planned. Not yet run in a browser on a GPU or a phone (cloud session): the panel's layouts are
checked in headless Chromium, the recording and the touch controls by unit tests.

## 1. The owner's answers (2026-11-12)

- **Presets and keyframes, both.** Presets: pick a target and a move (orbit, dolly, track, rise, pan) with sliders.
  Keyframes: free-fly the camera, drop keyframes with position, look, time of day and speed, choose the move
  between them, preview and play. Paths are saved as data to replay and share.
- **Record to a file** in the browser: the canvas without the HUD, 9:16 or 16:9, a chosen frame rate,
  downloadable.
- **Multiplayer: their camera only.** The world keeps running, their character stays put, time of day and
  time-lapse apply to their view only.
- **Anyone, any world.** A Settings toggle or a key; the host can disable it for guests.
- **Every mode, adventure too** ("Still want theater mode for adventure, just without camera-relative sim"):
  only the camera as a *simulation viewpoint* (§7) is creative-only.
- **Nice to have later:** an MCP server so a player's LLM agent can direct shots (§10, T-5).

What is data and what is code (CLAUDE.md "extendable system"):

| Kind | Where | Who adds |
| --- | --- | --- |
| Camera paths | the player's store / exported JSON (§5), the format in §3 | players, in the editor (T-2) or by hand; agents |
| Presets | `shared/theater/presets.json` (§4) | us, by a data entry; per build |
| Move kinds, easings, target kinds | `crates/dwell-theater` (a small fixed vocabulary) | code, rarely |
| Who may use it, the viewpoint | world settings / the capability exchange, modes.json (§7) | the host; the mode's data |

## 2. The mode (T-2)

- **Toggle:** a key (a free one chosen in `shared/input`, rebindable; F8 is the streaming report) and Settings → Theater; the controller's
  binding in `shared/input/gamepad.json`. Off by default for nothing: it is a viewing tool, available in every
  mode and every world type unless the host turned it off for guests (§7).
- **The character stays put:** input goes to the camera, not the body. The body keeps simulating (gravity, water)
  as an idle player; other players see it standing where it was.
- **Free-fly camera:** WASD + mouse look, up/down, a speed wheel; the same `placeCamera` view the dev tools use
  (the client's `Game::set_view`), so the renderer and the LOD view from the camera. In adventure (and wherever the
  viewpoint is off) the camera sees only what is already streamed around the player's body.
- **Panels:** presets (a target picker — the player, a creature or NPC by clicking it, a point — a move and its
  sliders from the preset's parameter ranges); keyframes (drop / move / delete, a move and easing per segment, the
  timeline with scrubbing); time of day and time-lapse; record.
- **HUD hidden** while the mode is on: one switch (`src/theater/hud.ts` `setHud`) hides every child of `body`
  but the canvas and the toolbar (every HUD part is a child of `body`, listed in `HUD_ELEMENTS`; a test audits the
  sources so a new HUD part fails until listed) and the game's first-person arms; the toolbar hides too while a
  path plays or a take records (Esc stops either).

**As built (T-2, `client/src/theater/`):** the key, the controller button and the free camera's tunables are data
in `shared/theater/theater.json` (`F9`, the right stick press; speed, wheel step, run factor, look rates); Settings
→ Theater has the button; a touch screen gets a Theater button (`#theater-open`), flies with a stick and Up and
Down buttons (`touch.ts`, `#theater-touch`: the stick at the bottom left, its full tilt at theater.json
`touch.stickRadiusPx`, a diagonal no faster; Up and Down at `touch.verticalFactor` of the speed; the mapping is
`touchFly`, tested) and looks by dragging anywhere else on the view. In portrait the controls show while the bottom
sheet is collapsed (its chevron), in landscape beside the side sheet; they hide while a path plays. The free camera is `Client.theaterFly` (`game.rs`
`fly_step`, face-local through `FaceFrame`, tested on both faces): WASD, Space/E up, C/Q down, Shift faster, the
wheel sets the speed; the controller's sticks and triggers. The shell's input goes to the camera, not the body,
while the mode is on. The toolbar (`mode.ts`) has three panels: **Presets** (a preset, its sliders from
presets.json's ranges, a target — the player, the creature nearest the crosshair, or the point under it — Preview
and To keyframes), **Keyframes** (+ Key at the camera with speed, fov and time of day; Set, Delete, each segment's
move; Play; Save/Saved…/Export/Import) and **Record**; the timeline strip shows the keys (drag to move, click to
select), the segments' moves and the playhead. The editor's model is `keyframes.ts` `KeyframeEditor` (tested).

### 2.1 The panel (owner, 2026-11-13: "Improve the UI on the theater mode. It's really bad, it doesn't even have a proper X button or responsive layout.")

- **The menu's design system** (#892, WORK_QUEUE item 23): the glass, the block buttons (green primary, red
  record), round chips for one-of choices, a segmented row of tabs, the disclosure chevron for **Advanced**, the
  focus ring on every control; every control has an accessible name (`aria-label` or its label), the panel is a
  `dialog` named by its title.
- **A close X** at the top right (`#theater-close`, "Close theater mode"). The mode is one of the panels
  `ui/panels.ts` owns (`theater`): opening it frees the mouse (a drag on the view turns the camera), the X or Esc
  closes it and locks the mouse again. Esc first stops a take or a playing path (`PanelViews.escape`), and the
  panel stays.
- **Responsive** (`layout.ts` `layoutFor`): at 900 × 560 CSS px or more a compact panel docked at the top left
  (340 px wide) that leaves the view free; smaller, a **bottom sheet** in portrait (the full width, at most 58% of
  the height, 48 px targets, a chevron to collapse it to its header) or a **side sheet** in landscape (the right
  side, the full height). No horizontal scroll at 360 px (a test lays it out in headless Chromium at 360 × 740 with
  every tool and Advanced open).
- **Sections:** Presets — *Target and move* (chips: Me, Creature, Point; a creature list and a Crosshair pick; the
  move), *Settings* (the preset's sliders), Preview and To keyframes. Keyframes — the key list (each key's time and
  the move after it, the selected one marked), + Add key, Delete, Preview (from the selected key), Play; Advanced:
  Set key to camera, the move to the next key, speed, field of view, up, time of day, name, Save, Open, Export,
  Import. Record — *Format* (aspect and frame rate chips), *Take* (Record path, Record free, Download); Advanced:
  the height, frame-exact path takes.
- **States:** while a path plays or a take records only the header shows: a badge (a red blinking dot and the
  take's timer while recording, `REC 0:12`; "Playing" or "Paused"), Pause/Resume while a path plays, Stop, the X.
  A finished take waits for Download (the dev API's takes still download unless `download: false`).
- **The controller** (`layout.ts` `PadNav`): the d-pad moves the focus through the panel's controls (a closed
  Advanced only its summary), left and right step a slider or a choice, A presses, B stops a take or a path or else
  closes; the sticks and triggers still fly the camera.

## 3. Camera paths (T-1, built)

A path is JSON (`formatVersion` 1), read by `dwell_theater::Path::parse` with a message naming the bad entry
(unknown fields fail; `$`-prefixed fields are comments) and written back by `Path::to_json` (export/import).

```json
{
  "formatVersion": 1,
  "name": "volcano at dusk",
  "loop": false,
  "keyframes": [
    { "t": 0, "position": [120, 900, -40], "target": [0, 860, 0], "fov": 60, "timeOfDay": 0.70, "speed": 1 },
    { "t": 12, "position": [-120, 910, -40] },
    { "t": 20, "position": [-120, 1000, -40], "timeOfDay": 0.80, "speed": 3 }
  ],
  "segments": [
    { "move": "orbit", "around": [0, 860, 0], "easing": "inOutSine" },
    { "move": "crane", "upEasing": "outCubic" }
  ]
}
```

### 3.1 Keyframes

- `t` — path seconds, increasing. `position` — world metres (f64), required on the first keyframe.
- The aim: `look` `[yaw, pitch]` (degrees, face-local to the keyframe's face as `placeCamera` takes them, or to
  its `up` face when it holds one) **or** `target`: a point `[x, y, z]`, `"player"`, `{"creature": id}` or
  `{"npc": id}`.
- `up` — `"a"`, `"b"` or `"auto"` (the default): the face whose up the camera holds (WORK_QUEUE item 40, owner:
  "The camera shouldn't flip on the wide shot, show it's upside down."). Under `"auto"` the camera takes the frame
  of the face it is over, as it always has, so a camera on face B's side sees face B upright. With `"a"` (or `"b"`)
  the camera's up, its roll and the keyframe's `look` stay that face's frame wherever the camera is: from face B's
  side in face A's frame face B and a player on it hang upside down under the disc, and crossing the midplane
  doesn't turn the camera over. Within a segment the up is the first keyframe's (a change turns the camera over at
  the next keyframe, as crossing the midplane does under `"auto"`). Orbits' and tracks' offsets stay face-local to
  the target's face; `up` changes only the camera's frame.
- `fov` — vertical degrees [5, 150]; unset: the viewer's setting. `timeOfDay` — 0–1 (0 midnight); unset: the
  world's. `speed` — the playback rate there (path seconds per real second, (0, 1000]): 1 is real time, 4 a 4×
  time-lapse of the move. Between keyframes it ramps linearly: a shot that speeds up part way is a speed ramp.
- **A field left out keeps the keyframe before's** (`up` included), so a pan's second keyframe needs only its look.
  A look carried into another frame — over the midplane under `"auto"`, or to another `up` — keeps its world
  direction (`FaceFrame::look_across`); under a held `up` the frame doesn't change and the look is carried as is.
  Written out (`Path::to_json`), a keyframe writes `up` when it holds one or when the one before did (an `"auto"`
  after a held one, which a left-out `up` would carry on).

### 3.2 Moves (the fixed vocabulary)

One segment per pair of keyframes (`segments[i]` from keyframe i to i+1; a missing one eases):

| `move` | Position | Look | Fields | Default easing |
| --- | --- | --- | --- | --- |
| `cut` | holds keyframe i, then jumps | keyframe i's | — | — |
| `linear` | straight | slerped between the keyframes' aims | — | linear |
| `ease` | straight | slerped | — | inOutCubic |
| `orbit` | around a target: angle, radius and height (face-local) eased between the keyframes' | at the target | `around`, `degrees` (the way round nearest it, whole turns included; else the shortest) | linear |
| `track` | the target + `offset` (face-local m), smoothed over `smoothingS` | at the target | `target`, `offset` (default [0, 2, −6]), `smoothingS` (0.5) | linear |
| `dolly` | straight | at `at` (or slerped) | `at`, `vertigo` (the dolly zoom: the field of view holds the subject's size, needs keyframe i's `fov`) | inOutCubic |
| `crane` | across with the easing, up with `upEasing` | slerped (at a target when the keyframes aim at one) | `upEasing` (outCubic) | inOutCubic |
| `pan` | stays at keyframe i (keyframe i+1 may not move it) | slerped | — | inOutSine |

A keyframe aimed at a moving target keeps looking at it (its aim is re-resolved every frame). A creature or NPC
(`{"creature": id}`, `{"npc": id}`) is followed through a **smoothed** position: the path's optional top-level
`followSmoothingS` (seconds, default 0.35, 0 follows exactly, at most 10; written only when not the default) is the
time constant of an exponential solved exactly for a target moving straight through the frame, so the camera turns
evenly at any frame rate rather than stepping with each update on the wire (`Player`; tests: a creature arriving in
20 Hz steps is followed evenly, the same at 30 and 144 fps). The player's own body is smooth already and isn't
smoothed again. A tracking move
starts from the keyframe's position and eases onto the target. A target that isn't known (gone, out of range)
holds the last aim.

### 3.3 Easings

`linear`, `smooth` (smoothstep), `inQuad`, `outQuad`, `inOutQuad`, `inCubic`, `outCubic`, `inOutCubic`,
`inSine`, `outSine`, `inOutSine` (`dwell_theater::Easing`). Each maps [0, 1] onto [0, 1]; the in-out ones leave
and arrive at rest, so chained eased segments don't jerk at the joins.

### 3.4 Evaluation

`dwell_theater::pose_at(path, t, targets)` → `Pose { position (world, f64), direction (world unit), yaw_deg,
pitch_deg (face-local to `Pose::frame`: the held `up` face's, else the eye's, for placeCamera), fov_deg?,
time_of_day?, up? }`. The time of day runs **forward** from one
keyframe's to the next's (through midnight when the next is earlier), plus a segment's whole `days`, linear in
path time: a day-to-night time-lapse is two keyframes.

### 3.5 Playing

`dwell_theater::Player` steps a path by the real frame time: path time advances by `dt × speed(τ)` (a midpoint
step of the ramp), a track's position is smoothed exponentially and solved exactly for a target moving straight
through the frame, so a path plays the same at 30 or 144 frames a second (tests pin both). The client steps it
before each frame (`Game::step_theater`) and places the view, the field of view and the viewer's time of day
(`theater_time`, over the dev `setTime` override); `theaterStop` or `releaseCamera` returns to the player's eyes.

**The held up in the client.** The placed view (`game::ViewOverride`) carries the pose's `up`; its yaw and pitch
are in that frame (`ViewOverride::face`, which the free-fly and the LOD's world pitch use). The renderer still gets
the **eye's** face, since the sky, fog, clouds, water and shadows are the air the eye is in: a held up on the other
face's side is drawn as the same look continued across (`look_across`) with half a turn of roll
(`ViewOverride::camera`), the same roll the midplane crossing's flip uses (BIFACIAL_WORLD.md §6). The sky is drawn
per world ray, so upside down it is simply where it is: under the disc in face A's frame, face B's sky (its night
and stars) fills the bottom of the frame and the disc's underside the top; nothing in it assumes the camera's up.
`theaterUp(up)` holds the free camera's up (the look kept), so the editor's keys are in the frame they say.

### 3.6 Both faces

Paths are world positions; every up-dependent part goes through `dwell_world::FaceFrame` (BIFACIAL_WORLD.md §11):
looks are face-local to the keyframe's face (or its held `up` face), an orbit's angle/radius/height and a track's offset are face-local to
the target's face, a pose's yaw and pitch are face-local to the eye's face. So a path written on face A and mapped
through face B's frame is A's mirror image (`face_b_mirrors_face_a`), and a path that crosses the midplane keeps its
world look as the eye passes (`a_path_across_the_midplane_keeps_its_world_look`). An orbit's "left" on face B is the
mirror of face A's, as everything on face B is.

## 4. Presets (T-1, built)

`shared/theater/presets.json`: each preset names a move kind (`orbit`, `dolly`, `track`, `crane`, `pan`), an
easing, and its parameters with a `default` (optional) and a `min`/`max` (the UI's slider range; overrides are
clamped to it). The parameter names a move understands are code (`dwell_theater::preset::PRESET_MOVES`); every
move also takes `seconds`, `fov`, `timeFrom`/`timeTo` (the viewer's time of day) and `days`. A preset expands
(`Preset::expand`) around a **subject** (a target and where it is) from **where the camera is** (an orbit, dolly,
track or rise starts on the camera's side of the subject) into an ordinary two-keyframe path, which can then be
edited, saved and shared like any other. The game's presets: `orbit`, `dolly`, `track`, `rise` (a crane), `pan`.
A preset may hold an `up` (`"a"`, `"b"`, `"auto"` the default), and an expansion can override it
(`Context::up`; the `params` of `theaterPreset` and `__dwell.theater('preset')` take `"up": "a"`): it goes on the
first keyframe and the shot holds it; a pan turns from the camera's look in that frame.
A new preset is a data entry (a test adds one from a fixture file no code names).

## 5. Saving and sharing paths (T-2)

- **The player's store:** paths in the browser's `localStorage` (`dwell.theater.paths`, one JSON list;
  `src/theater/store.ts`; deviation from IndexedDB: paths are a few kB and a synchronous store keeps the toolbar
  simple) under the shared-stores rules (CLAUDE.md
  "versions and releases": append-only — new fields optional, unknown fields and unreadable records kept on
  rewrite, a test pins it). Per player, not per world: a path names world positions, so it is most useful in the
  world it was made in; each saved path records the world's id and seed as a hint, not a lock.
- **Export / import:** a path's JSON (`Path::to_json`) downloaded as `<name>.camera.json` and imported by file or
  paste, validated with the engine's messages. The format is versioned (`formatVersion`); a newer build reads
  older versions.

## 6. Recording (T-3)

- **MediaRecorder on the canvas** (`canvas.captureStream(fps)`), the HUD hidden, the first of
  theater.json's `mimeTypes` the browser records (MP4/H.264, then WebM VP9/VP8), downloaded at the end as
  `<path name>.mp4|webm`. **Sound** (owner, 2026-11-13: "showcases shouldn't have music, but sound effects and
  ambiance are good"): `audio: true` on the dev API's take adds the game's effects and ambience, never the music or
  the UI, from the mixer's recording tap (`Mixer.tap`, `AudioEngine.tapStream`, `__dwell.audioTap`: a parallel graph
  with its own master level, underwater muffle and limiter, so the speakers are unchanged), recorded as the first of
  theater.json's `audioMimeTypes` at `audioBitsPerSecond`; off by default (the Record panel's takes stay silent). The
  record skill's screencast takes use the same tap (its §0b). Built (T-3, `src/theater/record.ts`):
  **Record path** plays the keyframes and ends the take with the path; **Record free** records the free camera
  until Esc. The dev API `__dwell.theaterMode('record', {path, aspect, height, fps, download})` returns the file
  (base64) for agents.
- **Crops:** 16:9 or 9:16 (theater.json `aspects`). The canvas is resized for the recording to the chosen aspect
  at a chosen height (720/1080/1440/1920; `recordSize`, both sides even), shown fitted on screen, not cropped after, so nothing is wasted and the LOD selects for the real view.
- **Frame rate:** 24/30/60 (the stream's rate). A path's playback is real-time; a slow page records a lower real
  rate, so the recorder shows the frames it is dropping.
- **Frame-exact takes** (Record → Advanced, or `exact: true` on the dev API's take; a path only): the path moves
  one frame of the rate per **drawn** frame whatever the page's speed (`Client.theaterExact` / `theaterAdvance`,
  `game.rs` `theater_dt`: a frame given no step holds the pose), the canvas is captured once per stepped pose that
  was drawn (`TheaterMode.afterFrame`, `new VideoFrame(canvas)`), stamped at i / fps and encoded by WebCodecs
  (VP8; the next step waits while the encoder is 4 frames behind), and written as a WebM by `webm.ts` `muxWebm`
  (a cluster per key frame, every 2 s). The video is smooth however slow the machine; the world itself (creatures,
  water) still runs in real time, and the take is silent (the sound runs in real time). Needs WebCodecs (Chrome,
  Edge, Safari 16.4+).

## 7. Multiplayer, permissions and the simulation viewpoint

- **Local only:** the camera, the time of day and the time-lapse are this viewer's; nothing goes on the wire. The
  character stays where it was and simulates as an idle player.
- **The host's permission (built):** the world setting `theaterGuests` (`WorldSettings` tag `THEATER_GUESTS`
  0x87, ADR 0054's optional tag, so older builds on the line ignore it: `u8` 0 off; a world that lets guests writes
  none, so its fields and join are byte-identical to before) sent at join and again when the host changes it (Host…
  → *Theater for guests*, saved with the world at once). When off, a guest's client doesn't offer the mode and
  leaves it if it was on (`TheaterMode.setAllowed`); **the host always may**: the hosting page's own player (the
  loopback session) or an op. Enforcement is client-side for the mode itself, which reveals nothing the client hasn't
  already streamed; the server ignores a guest's **viewpoint** while it is off (`Server::theater_allowed`). A
  dedicated server's world keeps the setting it was saved with (no command-line switch yet).
- **The simulation viewpoint (owner, 2026-11-12; T-4):** in **creative** the theater camera's position is sent to
  the server as a *viewpoint*, behind the capability exchange (a capability `theater.viewpoint`), and the server
  treats it as it treats a player's body for simulation: it streams chunks and the LOD around it, spawns and
  simulates creatures and NPCs near it, and runs the near ecology and the water there — so a shot can be far from
  the player. It is a mode ability as data: `theaterViewpoint` in `shared/modes/modes.json`'s `creative`. In
  **adventure** (no ability) the server ignores a viewpoint: the world stays centred on the body and the camera
  sees only what is loaded near the player — no scouting. **The server checks the world's mode, never trusting the
  client**; the viewpoint has a rate limit and counts against the same per-player chunk budget as the body. Theater
  mode itself (paths, presets, keyframes, recording, the hidden HUD) stays available in every mode, adventure
  included.

### 7.1 The viewpoint as built (T-4)

- **Data:** the ability `theaterViewpoint` (`shared/modes/modes.json`, `creative` only; `modes::Ability`), the
  capability `theater.viewpoint` (`shared/protocol/capabilities.json`) and two wire constants in
  `shared/protocol/constants.json` `world`: `theaterViewpointHz` (4: the most viewpoints a second taken from one
  player) and `theaterViewpointMaxM` (4096: the cap from the body). A new mode with the camera viewpoint is a data
  entry naming the ability.
- **The message:** `TheaterViewpoint` 0x6B, C→S reliable on the control channel: u8 0 (cleared) or 1 then three
  f64 (world metres, finite). Sent only to a server that announced the capability, so an older server never sees
  one (it would end the session on an unknown control message); an older client never sends one.
- **The client** (`dwell_theater::viewpoint::Sender`, `Game::step_theater`): while a camera path plays and the
  viewpoint is on (the server has the capability and the world's mode the ability, `Session::theater_viewpoint`,
  set again when the host changes the mode), the eye goes out at most `theaterViewpointHz` a second and only when it
  moved ½ m; one clear follows when the path stops (or the viewpoint is turned off). T-2's free-fly camera will use
  the same sender.
- **The server** (`crates/dwell-sim/src/server/theater.rs`): one viewpoint per player, kept from a client that
  listed the capability; one arriving sooner than the rate waits for its turn (the newest replaces it, so the last
  position always lands); a clear is taken at once. `Server::viewpoint_of` uses it only while the world's mode has
  the ability, the player is joined and on the home grid, pulled within `theaterViewpointMaxM` of the body. Then:
  - **chunks:** the body's view sphere around it, streamed after the body's own view and the respawn preload from
    the same per-player budget; kept from unloading while within the view (or render) radius plus the margin;
    chunk requests within the render radius of it answered; worldgen prefetches and keeps its chunks; when it moves
    or clears, what neither view holds unloads (`StreamStats::viewpoint_sent` counts its chunks);
  - **simulation:** `Server::presence_points` (bodies, then viewpoints) for the LOD's propagation order, creature
    spawning and despawning, the near ecology's groups coming near and staying (`herds::World::near`), the lava's
    near fields and the water's tick (which today steps every awake chunk anyway);
  - **not a body:** animals don't see, flee or notice it, NPCs don't look at it or get bumped by it, nothing
    collides with it, and it carries no edits.
- **Adventure:** the server keeps nothing it uses: the world stays centred on the body; a chunk request near the
  camera outside the body's render radius is refused as before.

## 8. Dev API and the bench (T-1, built)

- `__dwell.theater(op, args)` (`client/src/debug/theater.ts`): `play {path}`, `stop`, `pause {paused}`, `state`
  (`{time, duration, finished}`), `presets`, `preset {name, params, subject}` (→ the expanded path), `hud {shown}`.
- The bench's `theater` step (`client/scripts/scenarios.ts`): `{ "do": "theater", "preset": "orbit", "params":
  { "radiusM": 30 }, "subject": "player" }` or `{ "do": "theater", "path": { … } }`; waits for the end unless
  `"wait": false`; hides the HUD unless `"hud": true`. (The name `camera` was taken by the bench's placing step.)
- The record skill plays paths through it instead of ad-hoc `placeCamera` scripts.

## 9. Tests

`crates/dwell-theater/tests/engine.rs` (both faces through `both_faces!`): every move starts and ends at its
keyframes; a multi-move path at 60 fps has no jumps (per-frame move and turn bounded, the joins included); an orbit
keeps its radius and height and looks at its centre; a track keeps a walking target centred and settles at the
offset; tracking matches at 30 and 144 fps; a speed ramp's real duration and the time of day through midnight; the
vertigo dolly holds the subject's size; face B mirrors face A; a midplane crossing keeps its world look; a held
`up` keeps its frame (look, yaw, pitch) on the other face's side where `"auto"` continues the look across, and a
track keeps it; a preset holds an `up` (data or override); the game's
presets expand; a fixture preset by data alone; bad paths and presets fail with messages; a path round-trips through
JSON (`up` and an `"auto"` after a held one included). `game.rs`
`a_held_up_keeps_its_face_up_across_the_midplane` (both faces): a path with the held face's up on the other face's
side gives that face's camera up (+y world for face A) where `"auto"` gives the face it is over, the forward the
same, the sky's face the eye's. `src/easing.rs` checks every curve; `client/src/debug/theater.test.ts` the dev API and the HUD rule.

The viewpoint (T-4): `crates/dwell-sim/tests/theater_viewpoint.rs` (both faces) — in creative a viewpoint 640 m
from the body streams its chunk and the view around it and the fixture creatures spawn and stay there; adventure
ignores it (no chunks, no creatures, a chunk request there refused); a client without the capability is ignored;
clearing it unloads its chunks and despawns its creatures; a host's switch to adventure turns it off at once, and a
far one is capped. `server/theater.rs` checks the rate (the last one arrives) and the cap;
`dwell_theater::viewpoint` the client's rate, stillness and single clear; `dwell-client` `game_local.rs` that a
playing path sends viewpoints only where the server takes them.

The rest (2026-11-13): `theater_guests_off_ignores_a_guests_viewpoint_but_never_the_hosts` (both faces; the
settings go out again), `world_settings.rs` (the tag round-trips, is written only when off), `session.rs` (a guest's
client follows the host's switch, back on included); `engine.rs` `a_followed_creature_is_smoothed`,
`following_is_frame_rate_independent` (both faces) and the field's round trip; `game.rs` `theater_dt` and the
subjects' JSON; `client/src/theater/panel.test.ts` (happy-dom, and headless Chromium for the 360 px layout): the X
and Esc through the panels, Pause while playing, the layout classes, accessible names, the controller, the creature
picker's `{creature: id}`, the touch mapping and controls, the WebM's blocks at i / fps.

## 10. Phases

- **T-1 — the engine (built):** `crates/dwell-theater`, `shared/theater/presets.json`, the client's
  `step_theater`/`ViewOverride::fov_deg`/`theater_time`, `__dwell.theater`, the bench step, the record skill.
- **T-2 — the mode and editor (built):** the toggle (key, Settings, controller), free-fly input and a phone's
  flying controls, the panel (§2.1), the preset and keyframe panels and timeline, the target picker and following a
  creature or NPC by id, the path store and export/import, the `theaterGuests` world setting.
- **T-3 — recording (built):** MediaRecorder, aspect and size, frame rate, the download; frame-exact path takes.
- **T-4 — the simulation viewpoint (creative) (built):** the `theaterViewpoint` ability, the `theater.viewpoint`
  capability and `TheaterViewpoint` message, the server's interest set and chunk budget for it, tests that adventure
  ignores it (§7.1). Not yet run in a browser (cloud session).
- **T-5 — an MCP server (nice to have):** a small local stdio MCP server talking to the open page over a local
  WebSocket the page opens only when the player turns it on (find subjects, time of day, preview/play a path or
  preset, record, list saved paths); our own agents would record through it too.
