# 0032. Procedural, data-driven sound effects in plain Web Audio

- Status: Accepted
- Date: 2026-10-14
- Builds on: [0015](0015-block-registry.md) (blocks by name, the registry generator),
  [0028](0028-rust-port.md) (the Rust game and the TS shell)
- Design: [`AUDIO.md`](../AUDIO.md)

## Context

The game has no audio. The owner's brief (2026-10-14): "make good sound effects, dynamic SFX based
on block type with nice sounding variation", widened the same day to *every user action and every
menu action*, with procedural sound libraries allowed where they help. The owner gave the brief
directly and was not asked CLAUDE.md's design questions; the choices below are defaults, open to
change (AUDIO.md §9 lists the open questions). Sounds must follow the extendable-system rule: data
a tool could edit, one generator, references by name, validation and a by-data test.

## Options considered

**Source of sound**
- *Recorded samples*: realistic, but each needs licensing and download weight, and variation is
  limited to a few takes per sound.
- *Procedural synthesis* (chosen): noise, oscillators and modal partials through filters and
  envelopes; no licensing, a few KB of data, a fresh variant every play.

**Where to synthesise**
- *Tone.js* (MIT, on the order of 100 KB gzipped once its synths and effects are pulled in): good
  scheduling and effects, but a large framework for one-shot effects, and it wraps the
  AudioContext lifecycle that iOS Safari needs handled with care.
- *ZzFX / jsfxr-style generators* (tiny, MIT): parameter sets tuned for retro "chip" effects; the
  material sounds wanted here (crunch, rustle, wood modes, splashes) need layered filtered noise
  they do not model well.
- *fundsp in the WASM* (MIT/Apache): high quality, but synthesising in Rust means rendering buffers
  and shipping them across, or an AudioWorklet running WASM; heavier, and the main menu (the
  launcher) has no WASM at all.
- *Plain Web Audio* (chosen): the browser's native nodes (`AudioBufferSourceNode`,
  `OscillatorNode`, `BiquadFilterNode`, `GainNode`, `ConvolverNode`, `DynamicsCompressorNode`,
  `PannerNode`) cover the vocabulary; **0 KB of library** (the synth, mixer and data add about
  10 KB gzipped of our own code), run on the browser's audio thread everywhere including iOS
  Safari, and behave the same in `OfflineAudioContext` for tests. No dependency means no licence
  notice to add. The vocabulary is the seam: a library could be put behind it later without
  touching the data.

**Where events come from**
- *TS infers them* from the debug state each frame: duplicates the game's knowledge and misses
  per-tick transitions.
- *The Rust game emits them* (chosen): it has the predicted controller state, the block under the
  feet, the edits and the remote players; compact numbers cross the existing wasm-bindgen API once
  a frame.

## Decision

1. Sound effects are **procedural**, synthesised at play time in **plain Web Audio**; no audio
   library and no recorded assets.
2. Sounds are **data**: `shared/sounds/*.json` holds every sound as layers of a fixed vocabulary
   (sources `noise | osc | modal`; filters; envelope; grains; round-robin alternatives; per-sound
   variation ranges and intensity mapping), block sound **sets** (sound per event), the event
   kinds, footstep gaits, game-action sounds, the UI vocabulary, and the sounds of every control.
   `shared/sounds/gen.mjs` validates it and emits TS (the data) and Rust (event kinds, gaits and
   speed ranges); CI fails on stale output.
3. Blocks name their set with an optional `"sound"` field in `shared/blocks/*.json`
   (`"dwell:stone"`), validated by the block generator; without one, the data's fallback set.
4. The **Rust game** emits sound events (footsteps by distance per gait, landing by impact speed,
   jump, water, crouch, sprint, slide, flight, edits local and remote, refused edits) as a side
   channel that never feeds back into the simulation; the TS shell plays them, other players'
   spatialised with a `PannerNode` and the listener at the camera.
5. Game sounds vary per play (seeded RNG, ranges in data, never the same twice in a row); **UI
   sounds do not vary** (`variation: "none"`, validated) and are wired once by delegated listeners,
   so every button sounds the same and new buttons sound without wiring.
6. Master, effects and interface volumes are per-browser settings in an append-only store.

## Consequences

- Adding a material's sounds is a JSON edit; a new synthesis node kind is code (synth, validator,
  docs), deliberately rare.
- Quality depends on hand-tuned parameters; offline WAV renders are how they are reviewed (the
  owner approves them, Phase 28).
- Recorded samples could be added later as a new `sample` source kind without changing sets or
  events.
- Reversing to a library: implement the vocabulary on top of it in `synth.ts`; the data, events
  and mixer stay.
