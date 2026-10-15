# 0031. Data-driven structures, built in an in-game sandbox world

- Status: Accepted
- Date: 2026-10-14
- Replaces: the structure text format (`fill` lines, `crates/dwell-world/src/structure.rs`) once the
  JSON loader exists (the playground file is converted)
- Builds on: [0015](0015-block-registry.md) (blocks by name, the registry generator),
  [0025](0025-baseline-world.md) (the baseline's decision B10, no terrain features, which stands),
  [0026](0026-terrain-definitions.md) (terrain definitions, where placement rules would live),
  [0014](0014-versioned-releases.md) (compatibility lines)
- Design: [`STRUCTURES.md`](../STRUCTURES.md) (the owner's answers in §7)

## Context

The owner wants a simple flat white sandbox to build things in (trees, rocks), saved locally, that
feeds a structure system. Today a structure is a test fixture: a text file of `fill` boxes stamped at
absolute coordinates (the movement playground), with no origin, variants, generator or stale check.
Worldgen places no features, by the baseline's B10. Creative tools are single-block; there is no
white block, and no block is unbreakable since bedrock went.

The owner's answers (2026-10-14): what gets added later is open — tree variations, bushes, later
mountains and volcanoes that may need curve-driven generation; only the owner adds structures, built
in the sandbox, exported and committed; worldgen should place them eventually, decided once
structures exist; the in-game sandbox first, so a structure looks as it will in the world; the floor
is a solid, unbreakable surface, a block if that's easier; box select, fill, copy/paste with rotate
and mirror, and undo first; hand-built structures up to about 32³.

## Options considered

- **The tool**: an in-game `sandbox` world type (chosen: the real renderer, mesher and later
  adaptive shapes; saving is a world file; the tools serve normal play later), a standalone editor
  page (fast, but its own approximate renderer), or both on one format (possible later; the format is
  the contract).
- **The floor**: a sandbox special case in code, or **a registry block marked unbreakable in its
  data** that the server's edit validation checks (chosen: data, usable later for other unbreakable
  ground; S2a reuses the existing `indestructible` property rather than a new `breakable` flag).
- **The format's growth**: a closed format, or **a versioned format with a `kind` and open, named
  vocabularies** (chosen), so new kinds, variant operations and placement conditions are added
  without changing existing files.

## Decision

1. **Structures are JSON data, per build**, in `shared/structures/<name>.json`, named like blocks
   (`dwell:boulder_a`): a format version, a **`kind`** (`"voxels"`, hand-built, is the only one at
   first; a parametric kind for mountains and volcanoes would most likely be a terrain-definition
   node, ADR 0026), `size`, `anchor`, a palette by block name (`null` keeps what's there, explicit air
   carves), y-layers of palette characters (RLE optional), and `variants` as named operations
   (90° rotations, mirroring, weighted block choices). The vocabularies (kinds, variant operations,
   later placement conditions) are open lists: a new entry is a new name plus its engine code, with
   no change to existing files. Hand-built structures are capped at 32³ in validation for now.
2. **One generator and loader.** `shared/structures/gen.mjs` validates names against the registry,
   palette characters, layer dimensions, the anchor, the kind and the variant operations, failing with
   a message naming the entry; it emits a canonical form and a hash, and CI fails on stale output. A
   Rust loader replaces `Structure::parse`; the playground is converted. A fixture structure stamps
   by data alone in a test.
3. **Only the owner adds structures.** They are built in the sandbox, exported and committed to
   `shared/structures/`. Players don't author or share structures; nothing is per world or per player
   beyond the sandbox's working copy.
4. **A `sandbox` world type in the game**: flat ground of an unbreakable white floor block
   (e.g. `dwell:sandbox_floor`, full, not placeable), flight on. The block is marked with the
   registry's existing **`indestructible`** property (S2a: reused, not a new `breakable` flag); the
   server's edit validation rejects breaking or replacing such a state. The block is appended to the
   registry, so existing ids are kept.
5. **A structures store in the browser**: OPFS `dwell/structures/<name>.json`, one file per
   structure in the committed format, separate from the sandbox's world file so the library outlives
   it and every version can read it. It is a shared browser store, so it is append-only (optional new
   fields, unknown fields kept on rewrite, unreadable files left alone; a test pins it). Export and
   import as JSON (download, clipboard) are the path into the repository.
6. **Editing tools** in the game: box select, fill, copy and paste with rotate and mirror, and undo
   (a client stack of region diffs), over a server-validated region edit capped at about 32³ per
   request; the brush later.
7. **Worldgen placement is deferred** (owner: decided once structures exist). This ADR does not
   revisit B10: worldgen still places no features. When placement is decided, a new ADR revisits B10
   in part, puts placement rules in the terrain definition (not the structure), and ships it on **a new
   compatibility line**, because it changes the terrain a seed generates (structure hashes in the
   definition's hash; the owner raises `package.json`).

## Consequences

- Authoring, saving and exporting structures change no terrain: no compatibility line for Phase 27's
  S1–S3. The region edit message is an additive protocol change, checked against the protocol rules
  when it is built; the floor block changes the registry hash.
- The registry's existing `indestructible` property (kept for Phase 20's structural anchor) is the
  edit check (S2a): one property, not two.
- Adaptive blocks (Phase 18) will reshape authored natural blocks; building in the game shows it.
- Phase 27 of the implementation plan (S0, S1, S2a, S2b, S3; S4 deferred) builds it.
- Reversing it: the sandbox is a world type and the store a separate directory; removing either
  leaves worlds and terrain untouched.
