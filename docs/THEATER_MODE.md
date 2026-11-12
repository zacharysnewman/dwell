# Theater mode

Design for WORK_QUEUE item 41 (owner, 2026-11-12). Theater mode is an in-game camera toolset: the player's
character stays put while a free camera flies, follows paths built from presets or keyframes, sets the viewer's
own time of day and time-lapse, hides the HUD and records the canvas to a file. The `record` skill
(`.claude/skills/record/`) drives the same engine for the showcase videos (item 40).

Status: **T-1 built** (the camera path engine `crates/dwell-theater`, the presets file, the client wiring, the
dev API `__dwell.theater` and the bench's `theater` step). T-2 to T-5 planned (§10).

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
- **HUD hidden** while the camera plays or records (one CSS rule: every element but the canvas,
  `src/debug/theater.ts` `setHudShown`); the theater's own panels hide too while recording.

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
- The aim: `look` `[yaw, pitch]` (degrees, face-local to the keyframe's face as `placeCamera` takes them) **or**
  `target`: a point `[x, y, z]`, `"player"`, `{"creature": id}` or `{"npc": id}`.
- `fov` — vertical degrees [5, 150]; unset: the viewer's setting. `timeOfDay` — 0–1 (0 midnight); unset: the
  world's. `speed` — the playback rate there (path seconds per real second, (0, 1000]): 1 is real time, 4 a 4×
  time-lapse of the move. Between keyframes it ramps linearly: a shot that speeds up part way is a speed ramp.
- **A field left out keeps the keyframe before's**, so a pan's second keyframe needs only its look. A look carried
  over the midplane keeps its world direction (`FaceFrame::look_across`).

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

A keyframe aimed at a moving target keeps looking at it (its aim is re-resolved every frame). A tracking move
starts from the keyframe's position and eases onto the target. A target that isn't known (gone, out of range)
holds the last aim.

### 3.3 Easings

`linear`, `smooth` (smoothstep), `inQuad`, `outQuad`, `inOutQuad`, `inCubic`, `outCubic`, `inOutCubic`,
`inSine`, `outSine`, `inOutSine` (`dwell_theater::Easing`). Each maps [0, 1] onto [0, 1]; the in-out ones leave
and arrive at rest, so chained eased segments don't jerk at the joins.

### 3.4 Evaluation

`dwell_theater::pose_at(path, t, targets)` → `Pose { position (world, f64), direction (world unit), yaw_deg,
pitch_deg (face-local, for placeCamera), fov_deg?, time_of_day? }`. The time of day runs **forward** from one
keyframe's to the next's (through midnight when the next is earlier), plus a segment's whole `days`, linear in
path time: a day-to-night time-lapse is two keyframes.

### 3.5 Playing

`dwell_theater::Player` steps a path by the real frame time: path time advances by `dt × speed(τ)` (a midpoint
step of the ramp), a track's position is smoothed exponentially and solved exactly for a target moving straight
through the frame, so a path plays the same at 30 or 144 frames a second (tests pin both). The client steps it
before each frame (`Game::step_theater`) and places the view, the field of view and the viewer's time of day
(`theater_time`, over the dev `setTime` override); `theaterStop` or `releaseCamera` returns to the player's eyes.

### 3.6 Both faces

Paths are world positions; every up-dependent part goes through `dwell_world::FaceFrame` (BIFACIAL_WORLD.md §11):
looks are face-local to the keyframe's face, an orbit's angle/radius/height and a track's offset are face-local to
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
A new preset is a data entry (a test adds one from a fixture file no code names).

## 5. Saving and sharing paths (T-2)

- **The player's store:** paths in the browser's IndexedDB under the shared-stores rules (CLAUDE.md
  "versions and releases": append-only — new fields optional, unknown fields and unreadable records kept on
  rewrite, a test pins it). Per player, not per world: a path names world positions, so it is most useful in the
  world it was made in; each saved path records the world's id and seed as a hint, not a lock.
- **Export / import:** a path's JSON (`Path::to_json`) downloaded as `<name>.camera.json` and imported by file or
  paste, validated with the engine's messages. The format is versioned (`formatVersion`); a newer build reads
  older versions.

## 6. Recording (T-3)

- **MediaRecorder on the canvas** (`canvas.captureStream(fps)`), the HUD hidden, WebM (VP9, or H.264 where the
  browser offers it), downloaded at the end. No audio at first (the showcase format has none).
- **Crops:** 16:9 or 9:16. The canvas is resized for the recording to the chosen aspect at a chosen height
  (720/1080/1440), not cropped after, so nothing is wasted and the LOD selects for the real view.
- **Frame rate:** 24/30/60 (the stream's rate). A path's playback is real-time; a slow page records a lower real
  rate, so the recorder shows the frames it is dropping. An offline mode (step the path by 1/fps per drawn frame,
  frame-exact whatever the page's speed) is the agent's path through the bench (`theater` step + the record
  skill's screencast) until T-3 adds it in game.

## 7. Multiplayer, permissions and the simulation viewpoint

- **Local only:** the camera, the time of day and the time-lapse are this viewer's; nothing goes on the wire. The
  character stays where it was and simulates as an idle player.
- **The host's permission:** a world setting `theaterGuests` (ADR 0054's optional tag, so older builds on the line
  ignore it) sent at join; when off, guests' clients don't offer the mode (the host always may). Enforcement is
  client-side because the mode reveals nothing the client hasn't already streamed — except the viewpoint below,
  which the server checks.
- **The simulation viewpoint (owner, 2026-11-12; T-4):** in **creative** the theater camera's position is sent to
  the server as a *viewpoint*, behind the capability exchange (a capability `theater.viewpoint`), and the server
  treats it as it treats a player's body for simulation: it streams chunks and the LOD around it, spawns and
  simulates creatures and NPCs near it, and runs the near ecology and the water there — so a shot can be far from
  the player. It is a mode ability as data: `theaterViewpoint` in `shared/modes/modes.json`'s `creative`. In
  **adventure** (no ability) the server ignores a viewpoint: the world stays centred on the body and the camera
  sees only what is loaded near the player — no scouting. **The server checks the world's mode, never trusting the
  client**; the viewpoint has a rate limit and counts against the same per-player chunk budget as the body. Theater
  mode itself (paths, presets, keyframes, recording, the hidden HUD) stays available in every mode, adventure
  included. The viewpoint is not built: adding the ability touches the protocol's modes table and the server's
  interest sets (T-4).

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
vertigo dolly holds the subject's size; face B mirrors face A; a midplane crossing keeps its world look; the game's
presets expand; a fixture preset by data alone; bad paths and presets fail with messages; a path round-trips through
JSON. `src/easing.rs` checks every curve; `client/src/debug/theater.test.ts` the dev API and the HUD rule.

## 10. Phases

- **T-1 — the engine (built):** `crates/dwell-theater`, `shared/theater/presets.json`, the client's
  `step_theater`/`ViewOverride::fov_deg`/`theater_time`, `__dwell.theater`, the bench step, the record skill.
- **T-2 — the mode and editor (local):** the toggle (key, Settings, controller), free-fly input, the preset and
  keyframe panels and timeline, the target picker, the path store and export/import, the `theaterGuests` world
  setting.
- **T-3 — recording (local):** MediaRecorder, aspect and size, frame rate, the download; the offline frame-exact
  mode.
- **T-4 — the simulation viewpoint (creative):** the `theaterViewpoint` ability, the `theater.viewpoint`
  capability and message, the server's interest set and chunk budget for it, tests that adventure ignores it.
- **T-5 — an MCP server (nice to have):** a small local stdio MCP server talking to the open page over a local
  WebSocket the page opens only when the player turns it on (find subjects, time of day, preview/play a path or
  preset, record, list saved paths); our own agents would record through it too.
