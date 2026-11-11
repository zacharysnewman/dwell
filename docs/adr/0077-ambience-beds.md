# ADR 0077: Ambience beds and creature sounds as data

- **Status:** Accepted (2026-11-10; built in 0.52.20). Builds on [ADR 0076](./0076-recorded-sound-samples.md).
  Design: [`AUDIO.md`](../AUDIO.md) §2c (ambience) and §2d (creatures).
- **Context:** The owner (2026-11-10) asked for a second round of sounds from the *Universal Sound
  FX* pack: wind (stronger up high and in the open), the sea near the coast, rivers and streams near
  moving water, birds in daytime forests (quieter at night), leaves, fire crackle, wolves and deer,
  the falling tree, the player's breathing, gasp and strain, piece impacts and blast debris — with
  *"ongoing/ambient/looping sounds should be quieter so they're not really disruptive, more flavor
  than function."* Saved for later: rain/thunder, pickup/inventory/UI/coins, eating/drinking, the
  ladder's creak. Not asked CLAUDE.md's questions; defaults: more beds and conditions are added by
  data (a developer or the owner editing `shared/sounds/*.json`); the input vocabulary and the probe
  are code; per build; the Ambience volume per browser.

## Decision

- **Ambience** (`ambience` in `shared/sounds/*.json`): looping beds, each `{ files, gain, when }`, its
  level `gain` × every condition's ramp (`{ input, from, full, floor? }`) over the listener's
  surroundings, never above `cap`, gliding over `fadeS`; on its own **ambience bus** under a new
  **Ambience** volume (Settings → Sound, default `volume` 0.35: quiet). Beds are recorded loops
  (looped `AudioBufferSourceNode`s, a random start point); ADR 0076's "loop sounds are procedural"
  still holds for the event loops (slide, fire, waterfall).
- **The surroundings** are read by the Rust game four times a second (`dwell-game` `ambience.rs`
  `probe`, face-local through `FaceFrame`): a 9 × 9 grid of columns over `probe.radiusM` gives the
  height above the sea and the ground, the share open to the sky, the share and distance of water,
  the fastest current over it (the client's currents), the share of tree blocks (`probe.trees`, by
  name); the clock gives daylight and the fires the nearest one. Handed to the shell as 9 numbers
  (`Client.ambience()`), in a fixed order both sides test (`INPUTS`).
- Sea vs river vs stream is told by **the water's share and its current**, not the world map: wide
  water is the sea or a lake, running water a river, a narrow strip a stream. A deviation from "the
  world map's flow"; the probe is cheap and works on edited worlds.
- **Creatures** (`creatures` in the data): per model, a `stride`, a `step`, `startle` and `call`
  sound and `callEveryS`. The Rust game's `CreatureSounds` emits `creature_step` per stride walked,
  `creature_startle` when one breaks into a run, `creature_call` at a drawn interval, within
  `radiusM`; the event's `block` field carries the model's index (data order, emitted into
  `sound.gen.rs`). A new creature's sounds are data alone.
- **The body**: `VitalSounds` turns the vitals into `exhausted` (heavy breathing once stamina runs
  out), `gasp` (the air rising again after it fell under 0.7) and `strain` (the air crossing 0.25).

## Consequences

- About 1.45 MB of samples ships (of the 5 MB budget); the beds are 0.46 MB of it.
- The ambience costs a 9 × 9 × ≤ 96-cell read four times a second on the main thread (loaded chunks
  only, no generation).
- The picks are unheard (by filename): the owner tunes gains, conditions and files by data.

## Amendment (0.52.31, sounds round 2b)

- A `fire` bed (`fireM`, nothing beyond 8 m, gain 0.2) was added by data alone, as this ADR
  allowed; the per-source `burn` loops stay.
- New set events `impact` (a piece landing: stone, wood, soil) and `brush` (foliage moved through;
  no fallback), and ground kinds name `step`/`hit` sound events (a stick's snap). AUDIO.md §2e.
