# Dwell — Sound Effects

> **Status: [in progress, Phase 28]** — built and tested; waiting for the owner's listening review
> of the WAV renders and answers to §9. The music (§2a, §6a; the owner's Starry Sunshine) is
> built and tested, waiting for the owner's listen. Decisions in [ADR 0032](./adr/0032-audio.md); the built
> mechanisms are summarised in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §5.2 and §6.1 (the
> registry's `sound` field). The sounds themselves are **prototype content** (§6.1), like the
> block set: they exercise the system and are expected to be replaced.

## 1. Brief and defaults

Owner's brief (2026-10-14): *"make good sound effects, dynamic SFX based on block type with nice
sounding variation"*, widened the same day: *every user action and every menu action needs a sound
effect*; procedural sound libraries are allowed where they help. The owner gave the brief directly
and was not asked CLAUDE.md's "ask before designing" questions, so the answers below are
**defaults**, open to change (§9 lists the questions):

| Question | Default |
|---|---|
| What is added later? | More sound sets (one per material family), more events per set, more game actions and UI sounds; rarely a new synthesis node kind. |
| Who adds them, with what? | A developer or designer editing `shared/sounds/*.json` (and `"sound"` on a block in `shared/blocks/*.json`); a sound editor page like `docs/prototypes/` could edit the same JSON later. |
| Fixed in code | The synthesis vocabulary (noise, oscillator, modal partials, filters, envelopes, grains, layers, alternatives), the event kinds the game detects (the list is data, the detection is code), the bus/mixer, spatialisation. |
| Data | Every sound's parameters, every variation range, which sound each set plays per event, each gait's stride and loudness, each event's speed range, the UI vocabulary's sounds, which sound each control makes. |
| Per build, world or player? | Per build (like the block registry). Volumes are per browser. |
| Assets | **None recorded** for effects: every sound is synthesised at play time (no licensing, no download, endless variation). The one recorded asset is the music (§2a), the owner's own. |

## 2. Data: `shared/sounds/*.json`

One file per namespace (`dwell.json`). `shared/sounds/gen.mjs` validates it (with messages that
name the entry) and emits `client/src/audio/sounds.gen.ts` (the data the synth plays) and
`crates/dwell-game/src/sound.gen.rs` (the event kinds, gaits and their strides and speed ranges for
the Rust game). CI regenerates both and fails on a diff, as for the block registry.

- **`sounds`** — named sounds. A sound is `{ gain, reverb?, variation, intensity?, layers }`.
  - A **layer** is one *source* through a chain of *filters* under an *envelope*:
    - `source: "noise"` with `color` `white | pink | brown | crackle` (crackle: sparse decaying
      pops, about 30 a second, mostly small — band-passed, a fire's crackle; it loops too);
    - `source: "osc"` with `wave` (`sine | triangle | square | sawtooth`), `freq` and an optional
      glide `to`;
    - `source: "modal"` with `freq` and `partials: [[ratio, gain, decay], …]`: decaying sine modes
      (wood knocks, stone tocks, UI chimes; no envelope needed);
    - `filters: [{ type: lowpass | highpass | bandpass | peaking | notch, freq, q?, gain?, to? }]`
      (`to`: an exponential sweep over the envelope);
    - `env: { attack, hold?, decay }` (s; exponential decay to −60 dB);
    - `gain`, `at` (start offset, s), `pan`, `chance` (probability the layer plays);
    - `grains: { count: [min, max], spread, gain: [min, max], pitch? }`: the layer repeated as a
      cloud of short bursts (crunch, rustle, sand hiss, rubble, bubbles);
    - `alts: [overrides…]`: round-robin alternatives for the layer's fields.
    - `tremolo: { rate, depth }` (loop sounds only): the layer's level wavers at `rate` Hz by
      ± `depth` / 2 (a scrape's grit).
  - **`loop: true`**: a continuous sound (the slide's scrape), played on a voice until stopped:
    noise and oscillator layers only, sustained (no `env`, `grains`, `at`, `chance` or sweeps).
    A loop layer may have a `pan` (the night's crickets either side). Its `intensity` (0–1) moves
    while it plays: `gain`, filter `cutoff` and `pitch` (a factor on
    every source's rate and tremolo; loop sounds only), each gliding with a 60 ms time constant;
    below 0.05 the voice fades to silence.
  - **`variation`**: `{ pitch (± semitones), gain (± dB), cutoff (± fraction), time (± fraction) }`,
    or `"none"` (UI sounds: identical every time).
  - **`intensity`**: how the event's intensity (0–1: a landing's impact speed, a splash's speed)
    scales `gain`, filter `cutoff` and envelope `time`, each a `[at 0, at 1]` pair.
- **`sets`** — a block's sound set (`"sound": "dwell:stone"` on the block): the sound per event
  (`step`, `land`, `break`, `place`, `hit` required; `enter`, `exit`, `swim` for liquids;
  `slide` — a loop sound — and `slide_stop` for surfaces; `burn` — a loop sound — for a burning
  light source: the torch's `dwell:torch` set has the wood's steps and landing, its own place
  (a wooden tock and the flame catching) and break (a snuff), and `torch_fire`). A set missing an
  event falls back to the `fallback` set's, which must have every loop event's sound — unless the
  event says `"fallback": false` (`burn`: a set without its own plays nothing; stone does not
  burn).
- **`events`** — the event kinds the Rust game emits, in id order: each plays its set's event
  (`set`) and/or a named sound (`sound`); `speed: [min, full]` gives the speed at which it starts and
  at which its intensity reaches 1. `loop: "start" | "update" | "stop"` makes it part of a
  continuous sound on the mover's voice: `start` plays its (loop) sound on the voice, `update`
  (playing nothing of its own) moves the voice's intensity and position, `stop` fades the voice
  and plays its one-shot, if it has one (a fire's stop has none). Validated: a start's sounds are
  loop sounds, every other event's are not. For `hurt` the `speed` range is in health points (the
  damage taken).
- **`ambient`** — the ambient sounds' tunables, emitted into `sound.gen.rs` as `ambient::…`:
  `fire` (`radiusM`: burning light sources crackle within it; `max`: the nearest this many),
  `night` (`stars`: [starts, full] on the sky's stars, 0 by day, 1 by night; `aloftM`: [starts,
  gone] over sea level, no crickets aloft). There is no wind loop: it was removed (owner,
  2026-10-21: too intense).
- **`gaits`** — `walk`, `run`, `crouch`, `climb`, `swim`: footstep `stride` (m between steps) and
  `gain`.
- **`actions`** — sounds the TS shell plays for game actions it handles (hotbar, flight, the game
  menu, F3, F4, F6, the touch Break/Place switch).
- **`ui`** — the fixed menu vocabulary: `hover`, `click`, `back`, `tab`, `toggle_on`, `toggle_off`,
  `slider`, `error`, `success`. Every UI sound must have `variation: "none"` (validated).
- **`controls`** — for every action of the controls table (`client/src/ui/controls.ts`), the sounds
  it makes (`event:…`, `action:…`, `ui:…`) or `{ "silent": "reason" }`. A Vitest test fails when a
  controls-table action has no entry, so a new binding needs a sound decision.

### 2a. Music: `shared/music/*.json`

The owner's request (2026-10-21): *"Add this music of mine as the ambient music playing (might need
some dynamic handling since audio isn't allowed immediately on browsers unless there's input first
to ensure it actually plays when it can). I'm renaming it through to "Starry Sunshine"."* Given
directly, without CLAUDE.md's questions; the defaults below are open to change.

| Question | Default |
|---|---|
| What is added later? | More tracks, and conditions for when each plays (a biome, the night, the menus, a cave). |
| Who adds them, with what? | The owner: an audio file beside `music.json` and an entry in it. |
| Fixed in code | The `when` vocabulary (only `always` so far), the streaming player and its unlock. |
| Data | Each track's title, author, file, volume, loop, fades and condition. |
| Per build, world or player? | Per build; the Music volume per browser. |

One file per namespace (`music.json`), its audio files beside it:

```json
"tracks": { "starry_sunshine": { "title": "Starry Sunshine", "author": "Zachary Newman",
  "file": "starry-sunshine.mp3", "volume": 0.45, "loop": true, "fadeInS": 4, "fadeOutS": 2,
  "when": "always" } }
```

- `file`: a lower-case `.mp3` or `.m4a` (formats every browser streams) of at most 20 MB, beside
  the data; `volume` 0–1 under the Music volume; `loop`; `fadeInS` / `fadeOutS` (0–60 s: the
  fade-in at the start, the fade-out on a stop and over the end of a track that does not loop);
  `when`: where it plays, one of the vocabulary (`always`). The first track (namespaces in order,
  then as listed) whose `when` holds plays.
- `shared/music/gen.mjs` validates it (messages name the track) and emits
  `client/src/audio/music.gen.ts`, each track with its published name `<file>-<8 hex of its
  SHA-256>.<ext>`; CI regenerates it and fails on a diff, so a changed audio file without a
  regeneration fails too.
- **Starry Sunshine** (`dwell:starry_sunshine`): the owner's composition, uploaded as "Pitch Black
  Dungeon" and retitled; its ID3 title rewritten to "Starry Sunshine" and the old cover art
  dropped, the audio frames untouched (3:33, MP3, 48 kHz stereo, 4.9 MB). First-party, so not in
  `THIRD_PARTY_NOTICES`: the repository's `LICENSE` covers it as one of its assets.

Blocks: `"sound"` on a block in `shared/blocks/*.json` names a set (`dwell:stone`); the block
generator checks that it exists and emits it in the TS `BLOCK_DEFS`. Blocks without one use the
`fallback` set. Air has none.

## 3. Synthesis (`client/src/audio/synth.ts`)

Plain Web Audio, no library (ADR 0032 §Library). Each play builds a short-lived node graph: a
noise buffer source (2 s seeded white/pink/brown buffers per context, started at a random offset),
an `OscillatorNode`, or one sine per modal partial; then `BiquadFilterNode`s; then a `GainNode`
envelope (linear attack, hold, exponential decay); grains repeat that per burst. Nodes stop and are
released when their envelope ends. The same code renders into an `OfflineAudioContext` for tests and
the WAV renders.

The **mixer** (`mixer.ts`): effects bus and UI bus → master gain → a soft limiter (a
`WaveShaperNode`: linear to 0.7, then a tanh knee that never passes full scale; Chrome's
`DynamicsCompressorNode` adds its own make-up gain, which pushed renders past full scale) → output.
Effects also feed a small room (a `ConvolverNode` with a generated 0.55 s stereo impulse: early
reflections and a darkening tail), at each sound's `reverb` send (kept low: the world is outdoors).
Envelopes hold their gain at 0 before they start (a param's intrinsic 1 would pass a source's first
sample as a click).

**Levels** were set from the renders: each sound's `gain` was tuned to an A-weighted loudness
target for its category (footsteps quieter than landings and breaks, hits and player sounds
quieter still, UI sounds soft), with single plays peaking at or below about 0.6 so overlapping
sequences stay under the limiter's knee. Footsteps are then scaled by their gait's `gain`.

## 4. Variation (`variation.ts`)

Per play, a seeded RNG (mulberry32) draws pitch, gain, cutoff and time offsets uniformly within the
sound's ranges, the round-robin alternative per layer (never the previous one), grain counts, times
and gains, and the noise start offsets. A draw too close to the previous play of the same sound
(pitch within 0.25 semitone) is redrawn, so no sound plays twice in a row. Footsteps alternate left
and right (a small stereo offset for the local player). `variation: "none"` draws nothing: the sound
is identical every time (grains fixed by the seed of the sound's name).

## 5. Events (Rust → JS)

The Rust game (`crates/dwell-game/src/sound.rs`) watches the local player's predicted state each
tick and each remote player's interpolated state each frame, and turns changes into events:

| Event | When | Block (set) | Intensity |
|---|---|---|---|
| `step` | every gait stride of distance on the ground (walk, run, crouch) | under the feet | gait gain |
| `climb` | every climb stride on a ladder | the ladder | gait gain |
| `swim` | every swim stride swimming | the water | gait gain |
| `land` | grounded after falling at ≥ `speed[0]` | under the feet | from impact speed |
| `jump` | entering Jumping from the ground | under the feet | 1 |
| `enter` / `exit` | the body entering / leaving water | the water | from speed |
| `crouch_on` / `crouch_off` | the crouch flag changes | — | 1 |
| `sprint` | entering Running | — | 1 |
| `slide` | starts a slide's voice: Sliding (steep ground, PLAYER_CONTROLLER.md §6.8, or a sprint slide, §6.9, which also plays the crouch sound as it starts) at ≥ `speed[0]` (0.5 m/s), or onto another surface mid-slide | under the feet | from speed (full at 14 m/s) |
| `slide_speed` | the slide's speed changed (at most every 50 ms, by ≥ 0.02); 0 while paused | — | from speed |
| `slide_end` | not sliding for 0.15 s (`SLIDE_HOLD`: a flicker at a ramp's foot keeps the voice), a teleport, or the player gone from view | the slide's surface (`slide_stop`) | the speed it slid at |
| `rise` / `dive` | flying, vertical speed passes `speed[0]` | — | from speed |
| `break` / `place` | the local player's edit request (immediately), or another player's edit seen in a `VoxelModification` (spatial) | the broken / placed block | 1 |
| `hit` | a break of an indestructible block (the server will refuse it) | the block | 1 |
| `deny` | an edit with nothing targeted | — | 1 |
| `impact` | a hit on a wall or a ceiling (below): the speed toward a solid block touching the body drops in a sample to ≤ 35 % of its fastest in the last 70 ms | the block hit (its `land`) + a body thud | from that fastest speed (2 → 25 m/s) |
| `hurt` | the server's `PlayerEvent` Damage, any player's | — | from the damage (0 → 80 points) |
| `death` / `respawn` | the server's `PlayerEvent` Death / Respawn, any player's | — | 1 |
| `burn` / `burn_level` / `burn_end` | a burning light source's loop voice starting, moving, stopping (below) | the source's block (its `burn`) | by distance |
| `night` / `night_level` / `night_end` | the night's voice (below) | — | by the stars |

**Impacts** (`ImpactSounds`, owner 2026-10-21: "hard hits even sliding into a wall is fair game").
The controller's `IMPACTED` event (PLAYER_CONTROLLER.md §6.10) is not on the wire, so the hit is
read from the sampled motion the way landings are: for each face-local direction a body can hit
(±x, ±z, up; landings are `land`'s), the speed toward it is tracked; when a solid block touches
the body there (the body probe `Surroundings`: the cell 6 cm past its side at mid-height, or 6 cm
over its head) and the speed toward it drops in one sample to at most 35 % of its fastest in the
last 70 ms, that fastest speed is the hit's — the controller's `max(0, −v · n)` for the cell's
face, so a 25 m/s launch into a wall sounds at full intensity and hurts, and a sprint into it at
about 7 m/s is a soft thud that does not. Then that direction is quiet for 70 ms (one hit per
contact; pressing on into the wall is silent). Climbing and swimming hit nothing; flight sounds
but never hurts. Braking beside a wall is no hit: the probe only sees a wall the body touches.
Ours are read from the prediction's ticks; another player's from each snapshot's latest state
(exact positions — an interpolated body reaches the wall late), and held by the interpolation
delay (`INTERP_DELAY_MS`) so they sound when its body is drawn at the wall. The hurt is not
predicted: it comes from the server's Damage event (another player's held by the same delay),
so it never sounds for a hit the server did not count.

**Fires** (`FireSounds`): every frame the burning light sources near the listener — placed ones
(`VoxelWorld::light_sources_near`, the flame's cell), the torch in our hand (at the listener) and
in other players' hands (at their hand) — a light with a `flame` (LIGHTS.md §6) burns; the
nearest `ambient.fire.max` within `ambient.fire.radiusM` each hold a loop voice (`FIRE_VOICE` on)
while they stay among them, at a level `(1 − (d / r)²)²` (full beside it, 0 at the radius; the
panner's rolloff on top). A nearer one takes a farther one's voice. **Night**
(`AmbientSounds`): one voice at the listener (`NIGHT_VOICE`), its level the stars' ramp, fading as
the height over sea level rises (`aloftM`; no crickets aloft), × the sky light at the eye (none in
a cave). Loops start above 0.03 and stop at 0.01; updates go at most 10 times a second, on a change
of 0.02 or a move of 0.25 m. The shell remembers each loop's start and restarts a voice an update
finds not playing (a hidden tab or a context still starting), so the long loops survive both.

The local player's own edits sound at once, on the request (the client does not predict edits); the
server's echo of them is recognised by cell and not played again. Events cross to JS as compact
numbers: `takeSounds()` returns a `Float64Array` of 7 numbers per event (`kind, block state,
intensity, x, y, z, flags`; flags bit 0 = spatial, bits 1–3 the gait + 1, bits 4 and up the voice
+ 1 — 0 the local player, others their player id, fires from 65536, the night
131073). `listener()` returns the
camera's position, forward and up. Audio is a side channel: nothing read back affects the
simulation, and it reads only the predicted state the game already has.

## 6. The shell (`client/src/audio/`)

- Game actions the shell handles play their `actions` sound: a hotbar change (keys, wheel, tap),
  flight on and off and its speed, the game menu opening and closing (Esc, ☰, Resume, the backdrop),
  the F3 overlay, the F4 map and its zoom, F6's copy (success, or error), the touch Break/Place
  switch. Their buttons carry `data-sound="none"` so they do not also click.
- `engine.ts`: the AudioContext (created and resumed on the first pointer, key or touch gesture —
  browser autoplay rules, iOS Safari's "interrupted" state), the buses, volumes, muting while the tab
  is hidden, spatial playback (`PannerNode`, HRTF on desktop and equal-power on phones, inverse
  distance rolloff, events past 48 m dropped), and the listener following the camera.
- `events.ts`: decodes the event numbers, resolves block state → block → set → sound (the
  generated `BLOCK_DEFS`), applies the gait gain and the step alternation.
- `uiSounds.ts`: **delegated** listeners on the document (installed once by the game page and the
  main menu, which the launcher also shows, in plain TS without WASM): hover/focus on any button,
  link, tab or slider; click on any button (back on close/cancel/back buttons, tab on
  `role="tab"`, toggle on/off on `aria-pressed` buttons); slider ticks throttled to one per 60 ms;
  `data-sound="…"` overrides, `data-sound="none"` silences. New buttons sound without any wiring.
  `success` and `error` are played explicitly (a world created, a copy done, a form error).
- Settings: **Master**, **Effects**, **Interface** and **Music** volume sliders in the game menu's
  Settings page (a Sound group), kept in `dwell.audio` (append-only: unknown fields kept on
  rewrite; `music` is optional, a record without it reads the default, 60 %).

### 6a. Music playback (`client/src/audio/music.ts`)

- **Streamed**: an `<audio>` element (`preload="auto"`, `loop` from the data) plays the file; it is
  never decoded whole into a buffer, so the 4.9 MB file does not hold up loading. Once the page's
  AudioContext runs, the element is routed into it (`createMediaElementSource` → its own gain →
  the mixer's **music** bus → master → the limiter; no room). Until then it plays straight out,
  its `volume` set to master × music × the track's volume × the fade.
- **Autoplay**: `start()` calls `play()` at once. If the browser refuses (no user activation yet:
  a page opened by its address, iOS always), the player listens for the first `pointerdown`,
  `keydown`, `touchend` or `click` (as the engine's unlock) and, inside that gesture, resumes the
  engine's context (creating it if needed: the engine's own unlock, iOS's silent-buffer trick
  included), routes the element into it and calls `play()` — iOS Safari allows both only there.
  The listeners go once a `play()` succeeds; a gesture that is not user activation (Escape, a
  modifier) leaves them armed for the next. A page reached from the main menu has the activation
  already (same-origin navigation), so the music starts with the page.
- **When it plays**: from the first screen on (the main menu, the launcher's included; the loading
  screen; the game), as ambience everywhere. The position is kept per tab (`sessionStorage`
  `dwell.musicAt`: the track, its time and the clock, saved when the page hides — which a
  navigation does first) and picked up by the next page if within 30 s, so the menu → game
  navigation carries on rather than starting over (it fades in again over `RESUME_FADE_S`).
- **Fades**: the track's `fadeInS` at its first start; 1 s (`RESUME_FADE_S`) when it comes back from
  a hidden tab or volume 0; `fadeOutS` on `stop()` (the hook for a later change of track) and over
  the end of a track that does not loop. Stepped every 50 ms while fading; the routed gain glides
  (`setTargetAtTime`, 50 ms).
- **Hidden tab**: paused (and its position saved) while hidden, played again on return; a start the
  browser refused while hidden is retried on return.
- **Volume 0** pauses the element (nothing streams or decodes); raising it plays again.
- **Off for tests**: `?music=off`, or `dwell.music` set to `"off"` in the browser's storage (as
  `dwell.loading`): the e2e suite's storage (`playwright.config.ts`, the launcher's
  `playwright.site.config.ts`) and the bench (`scripts/bench.ts`) set it, so no run streams or
  decodes the music.
- **Shipping**: the file is served once per site, at `/dwell/media/<name>-<hash>.mp3`, not in each
  version directory (RELEASES.md §4): a release build (base `/dwell/v/<version>/`) loads it from
  the site root's `media/` (`__MEDIA_BASE__`, `scripts/media.ts`), the build and the launcher emit
  it into their own `media/`, and the Pages workflow's `site.ts media` moves the builds' copies to
  the root (one copy; a name already there is the same bytes, a mismatch fails the deploy). A local
  build and `vite preview` serve it from `/dwell/media/` as well; the dev server serves it from
  `shared/music/`. The name changes with the bytes, so Pages' 10-minute caching never serves a
  stale file.
- Debug hook: `window.__dwellMusic()` (on any page with sound) gives the track, its state
  (`blocked`, `playing`, `paused`, `muted`, …), whether it is routed, its time and output level.

## 7. Testing

- Vitest: data validation (every rule has a failing fixture), a new set and block mapping added by
  data alone, the music (§2a, §6a: its data's rules, a fixture track by data alone, the unlock —
  blocked, then started by the first gesture, once —, the hidden tab, volume 0, the fades, the
  routing, the position across pages, `?music=off`, the media files moved once per site), variation (seeded determinism, ranges respected, no immediate repeats, `none` exact),
  event decoding and mapping (block → set, gait → gain), the controls coverage, UI delegation (a
  new button gets the click), the settings store.
- Rust: footstep cadence from distance travelled, landing intensity from impact speed, jump,
  crouch, splash, edits (immediate, echo suppressed, remote spatial, indestructible → hit);
  impacts (a wall hit at 25 m/s a loud impact plus the server's hurt, at 8 m/s a soft thud and no
  hurt; every side and the ceiling once; snapshots sound as ticks; no hit stopping in the open,
  along a wall, swimming or climbing; the body probe on both faces), and the same through the real
  controller on both faces (`dwell-game/tests/impact_sounds.rs`: launched into a wall at 25 and
  8 m/s, sprinting into it — the sound as loud as the controller's own impact speed); health
  events; fires (heard within the radius, capped at `max`, nearest first, a nearer one taking a
  voice, a held one at the listener, updates throttled); wind and night by height, speed, stars
  and the sky light.
- Offline renders (`npm run sfx:render -- --out <dir>`): every sound of every set and every action
  and UI sound rendered in Chromium with `OfflineAudioContext` through the real mixer (the page is
  built from `client/src/audio/render.ts` and served with `vite preview` on port 4178); asserted
  non-silent, peak < 1.0 and a sensible length; written to WAV for listening (`set_<set>_<event>`,
  `event_*`, `action_*`, `ui_*`), with 10-step walk and run sequences per surface
  (`steps_<set>_<gait>_x10`), four break and four place variations per set (`break_<set>_x4`,
  `place_<set>_x4`), landings soft to hard (`land_<set>_soft_to_hard`), each surface's slide
  from slow to fast for 4 s ending in its stop (`slide_<set>_slow_to_fast`; the intensity moved
  every 50 ms as the game does), each set's burn and each event's loop held 4 s from faint to full
  (`burn_<set>_faint_to_full`, `loop_wind_faint_to_full`, `loop_night_faint_to_full`), plus
  `summary.txt`.
- Debug hooks: `window.__dwell.sounds()` in a game and `window.__dwellSounds()` on any page with
  sound list the latest sounds played.

## 8. Not covered

- Chat and screenshots do not exist yet; when they are added, the controls coverage test asks for
  their sounds.
- Ambience beyond the night (water flow, birds, rain) is out of scope; music is one track
  everywhere (§2a), with no conditions or playlist yet.
- No sound of its own: crossing between the faces (its somersault is silent), knockback (a push, not a
  hit; any damage with it sounds as the hurt), other players joining or leaving.

## 9. Open questions for the owner

1. Is fully procedural right, or do you want recorded samples for some materials later (the data
   format could gain a `sample` source)?
2. Should remote players' footsteps be audible (they are now, spatialised), and how far?
3. Footstep loudness and cadence per gait (walk 2.0 m, run 2.7 m, crouch 1.1 m between steps):
   tune after listening?
4. Should UI sounds be on by default, and at what level relative to effects (now 60 %)?
5. Should the main menu (launcher) have its own volume control, or only the in-game Settings?
6. iOS's silent switch mutes Web Audio: acceptable?
7. Who will author sound sets later — is a sound editor page (like the Adaptive Block Lab) wanted?
8. Music: is Starry Sunshine's level right (`volume` 0.45 under a 60 % Music default)? Should it
   play in the main menu too (it does), or only once in a world? When more tracks come, what
   decides which plays (biome, time of day, menus)?
