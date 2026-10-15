# Dwell — Sound Effects

> **Status: [in progress, Phase 28]** — built and tested; waiting for the owner's listening review
> of the WAV renders and answers to §9. Decisions in [ADR 0032](./adr/0032-audio.md); the built
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
| Assets | **None recorded**: every sound is synthesised at play time (no licensing, no download, endless variation). |

## 2. Data: `shared/sounds/*.json`

One file per namespace (`dwell.json`). `shared/sounds/gen.mjs` validates it (with messages that
name the entry) and emits `client/src/audio/sounds.gen.ts` (the data the synth plays) and
`crates/dwell-game/src/sound.gen.rs` (the event kinds, gaits and their strides and speed ranges for
the Rust game). CI regenerates both and fails on a diff, as for the block registry.

- **`sounds`** — named sounds. A sound is `{ gain, reverb?, variation, intensity?, layers }`.
  - A **layer** is one *source* through a chain of *filters* under an *envelope*:
    - `source: "noise"` with `color` `white | pink | brown`;
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
  - **`variation`**: `{ pitch (± semitones), gain (± dB), cutoff (± fraction), time (± fraction) }`,
    or `"none"` (UI sounds: identical every time).
  - **`intensity`**: how the event's intensity (0–1: a landing's impact speed, a splash's speed)
    scales `gain`, filter `cutoff` and envelope `time`, each a `[at 0, at 1]` pair.
- **`sets`** — a block's sound set (`"sound": "dwell:stone"` on the block): the sound per event
  (`step`, `land`, `break`, `place`, `hit` required; `enter`, `exit`, `swim` for liquids). A set
  missing an event falls back to the `fallback` set's.
- **`events`** — the event kinds the Rust game emits, in id order: each plays its set's event
  (`set`) and/or a named sound (`sound`); `speed: [min, full]` gives the speed at which it starts and
  at which its intensity reaches 1.
- **`gaits`** — `walk`, `run`, `crouch`, `climb`, `swim`: footstep `stride` (m between steps) and
  `gain`.
- **`actions`** — sounds the TS shell plays for game actions it handles (hotbar, flight, the game
  menu, F3, F4, F6, the touch Break/Place switch).
- **`ui`** — the fixed menu vocabulary: `hover`, `click`, `back`, `tab`, `toggle_on`, `toggle_off`,
  `slider`, `error`, `success`. Every UI sound must have `variation: "none"` (validated).
- **`controls`** — for every action of the controls table (`client/src/ui/controls.ts`), the sounds
  it makes (`event:…`, `action:…`, `ui:…`) or `{ "silent": "reason" }`. A Vitest test fails when a
  controls-table action has no entry, so a new binding needs a sound decision.

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
| `slide` | entering Sliding | — | 1 |
| `rise` / `dive` | flying, vertical speed passes `speed[0]` | — | from speed |
| `break` / `place` | the local player's edit request (immediately), or another player's edit seen in a `VoxelModification` (spatial) | the broken / placed block | 1 |
| `hit` | a break of an indestructible block (the server will refuse it) | the block | 1 |
| `deny` | an edit with nothing targeted | — | 1 |

The local player's own edits sound at once, on the request (the client does not predict edits); the
server's echo of them is recognised by cell and not played again. Events cross to JS as compact
numbers: `takeSounds()` returns a `Float64Array` of 7 numbers per event (`kind, block state,
intensity, x, y, z, flags`; flags bit 0 = spatial, bits 1–3 the gait). `listener()` returns the
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
- Settings: **Master**, **Effects** and **Interface** volume sliders in the game menu's Settings
  page (a Sound group), kept in `dwell.audio` (append-only: unknown fields kept on rewrite).

## 7. Testing

- Vitest: data validation (every rule has a failing fixture), a new set and block mapping added by
  data alone, variation (seeded determinism, ranges respected, no immediate repeats, `none` exact),
  event decoding and mapping (block → set, gait → gain), the controls coverage, UI delegation (a
  new button gets the click), the settings store.
- Rust: footstep cadence from distance travelled, landing intensity from impact speed, jump,
  crouch, splash, edits (immediate, echo suppressed, remote spatial, indestructible → hit).
- Offline renders (`npm run sfx:render -- --out <dir>`): every sound of every set and every action
  and UI sound rendered in Chromium with `OfflineAudioContext` through the real mixer (the page is
  built from `client/src/audio/render.ts` and served with `vite preview` on port 4178); asserted
  non-silent, peak < 1.0 and a sensible length; written to WAV for listening (`set_<set>_<event>`,
  `event_*`, `action_*`, `ui_*`), with 10-step walk and run sequences per surface
  (`steps_<set>_<gait>_x10`), four break and four place variations per set (`break_<set>_x4`,
  `place_<set>_x4`) and landings soft to hard (`land_<set>_soft_to_hard`), plus `summary.txt`.
- Debug hooks: `window.__dwell.sounds()` in a game and `window.__dwellSounds()` on any page with
  sound list the latest sounds played.

## 8. Not covered

- Chat and screenshots do not exist yet; when they are added, the controls coverage test asks for
  their sounds.
- Ambience (wind, water flow, birds) and music are out of scope.

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
