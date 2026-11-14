# ADR 0076: Recorded sound samples beside the procedural sounds

- **Status:** Accepted (2026-11-10; built in 0.52.15). Supersedes in part [ADR 0032](./0032-audio.md)
  ("no recorded assets"). Design: [`AUDIO.md`](../AUDIO.md) §2b.
- **Context:** The owner (2026-11-10) bought Imphenzia's *Universal Sound FX* (Unity Asset Store,
  ~10,000 sounds) and asked to use it "over most of our procedural FX", with a few exceptions, likely
  the slide. Answers: a budget of about 5 MB compressed, a few mono variations per event; a per-block
  sound material choosing the samples of step, land, break and place.

## Decision

- A sound in `shared/sounds/*.json` may list `samples` (`files` in `shared/sounds/samples/`, a `gain`,
  a `pitch` range) and `play` (`samples`, the default, or `procedural`). Its `layers` stay: they play
  until the samples are decoded, when one fails to load, and on `play: "procedural"`. Loop sounds
  cannot have samples (the slide, fires and the waterfall stay procedural).
- `shared/sounds/validate.mjs` checks the names, that each file exists, and that all of them together
  are at most 5,000,000 bytes; `gen.mjs` emits content-hashed published names into `sounds.gen.ts`.
  The build emits them into `media/` beside the music (`client/scripts/media.ts`), shipped once per
  site. The engine fetches and decodes every sample once, on the first gesture (they are small).
- The block's existing `"sound"` (its sound set) is its **sound material**; a new material is a new
  set with sample-backed sounds, added by data alone. No second block field.
- Files are converted from the pack with ffmpeg to mono AAC (`.m4a`, 64 kbps), which every target
  browser decodes (Opus in Ogg is not decoded by older Safari).

## Consequences

- About 0.8 MB of samples ships per site (well under the budget); first plays before decoding finish
  sound procedural.
- The picks were made by filename and category, unheard: the owner swaps them by data (AUDIO.md §2b
  lists them). The pack's licence is the Unity Asset Store EULA (Standard); the owner confirms it covers
  the game's commercial use.
- To reverse: `play: "procedural"` on a sound, or remove its `samples`.
