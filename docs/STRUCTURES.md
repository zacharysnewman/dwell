# A structure sandbox and a data-driven structure system

**Status:** Adopted ([ADR 0031](./adr/0031-data-driven-structures.md)), 2026-10-14. Built as Phase 27 of the
[implementation plan](./IMPLEMENTATION_PLAN.md) (steps S0–S4; S2 split into S2a, the floor and the world
type, and S2b, the structures store; S4, worldgen placement, built early for the owner's pines on
2026-10-20 as an interim data file before Phase 17, §8 and [ADR 0042](./adr/0042-structure-placements.md)).
Owner answers in §7.

The ask: a tool with a simple flat white terrain sandbox where you build things (trees, rocks,
etc.), saved locally, that can easily be added to a custom shapes/structures system.

## 1. What existed (2026-10-14, before S1)

- **Structure files, as test fixtures only.** `crates/dwell-world/src/structure.rs` parses a text
  format with one command, `fill x0 y0 z0 x1 y1 z1 <block state>`, blocks by state name, errors with
  line numbers; `with_structure` stamps it at absolute world coordinates. The only file is
  `shared/structures/playground.structure` (the movement playground used by tests). No origin,
  variants, placement rules, generator or stale-check.
- **No features in worldgen, by the owner's choice**: the baseline's B10 ("no trees, rocks, caves")
  cut the old trees and boulders. Their order-independent hashed placement is worth copying.
- **Terrain definitions** (ADR 0026, Phase 17: terrain as JSON in `shared/worldgen/`, a definition
  hash) are planned, not started. Generation today is a version switch.
- **A flat world type exists** (`generate_flat_chunk`, world type `flat` in `client/src/local/`;
  the launcher's `?new=1&…&type=` route). There is no white block.
- **Creative tools are single-block**: place/break at one request per 100 ms, server-validated; an
  infinite palette in the hotbar; creative flight. No box select, fill, copy/paste, mirror or undo.
- **Local saving**: each world is a redb `.dwellworld` file in OPFS, autosaved; the index in
  localStorage is append-only. Export/import is planned.
- **Prototype pages**: `docs/prototypes/shape-playground.html` already saves locally and
  exports/imports JSON, but renders with three.js, not the game.

## 2. Tool options

| Option | For | Against | Effort (*estimate*) |
|---|---|---|---|
| **(a) A `sandbox` world type in the game** — flat white ground, flight, selection tools, "Save as structure" | The real renderer, mesher and (later) adaptive shapes: what you see is what worldgen stamps. Saving is free (a world file). The tools are useful in normal play later | Bulk edits need a server-validated `RegionEdit` message (additive, but the protocol is public API); undo as reverse region edits; DOM UI | 2–3 weeks |
| **(b) A standalone editor page** (`docs/prototypes` style) | Fast, no protocol or release impact, trivial undo and saving | Its own approximate renderer; can't show adaptive shaping; must read `shared/blocks/dwell.json` to avoid duplicating colours | 3–5 days |
| **(c) Both, one file format** | The format and its loader are the contract; either tool exports it | Two tools to keep | (b) then (a) |

**Recommendation:** (c), starting with **(a)** if fidelity to the game matters (it usually does for
trees and rocks, and adaptive blocks will reshape them), or **(b)** if you want something to build
in this week.

## 3. Structure format

**Built (S1, 2026-10-14).** JSON in `shared/structures/<name>.json` (test fixtures in
`shared/structures/test/`), named like blocks:

```json
{ "format": 1, "name": "dwell:boulder_a", "kind": "voxels", "size": [5, 4, 5], "anchor": [2, 0, 2],
  "palette": { ".": null, "s": "dwell:stone", "d": "dwell:dirt", "_": "dwell:air" },
  "layers": [["sssss", "sssss", "..."], ["..."]],
  "variants": { "rotate": [0, 90, 180, 270], "mirror": ["x"],
                "choices": { "s": [["dwell:stone", 3], ["dwell:sand", 1]] } } }
```

The schema, as `shared/structures/gen.mjs` validates it (every error names the file and the entry,
e.g. `shared/structures/x.json: palette."s": unknown block "dwell:nope"`):

| Field | Rule |
|---|---|
| `$comment` | Optional, ignored. Any other unknown field is an error. |
| `format` | `1`. A later format is a new number the generator reads. |
| `name` | `namespace:name`, `[a-z0-9_]`; the name part matches the file name; unique. `test:` is for `test/` only. |
| `kind` | An open list; only `"voxels"` (hand-built cells) exists. The fields below are the `voxels` body. |
| `size` | `[x, y, z]`, each 1–32 (owner: hand-built up to about 32³; larger features are generator nodes). |
| `anchor` | `[x, y, z]` inside `size`: the cell `place` puts at the given position — for ground features, the lowest cell, which sits on the first air above the ground. |
| `palette` | Single printable ASCII characters (not space) → a block state name in any form `parse_state` takes, or `null` (keep what is there; the mask). Explicit `dwell:air` carves. Names are stored canonicalised. |
| `layers` | `size[1]` y-slices, bottom up; each `size[2]` z-rows (north to south); each a string of `size[0]` characters (west to east), all in the palette. |
| `variants` | Optional; named operations, an open list, applied in this order: |
| `variants.rotate` | Distinct values of 0, 90, 180, 270: quarter turns clockwise seen from above (north → east) about the anchor; one is picked. |
| `variants.mirror` | Distinct axes of `x`, `z`: one of "none" and these is picked; mirroring is applied before rotating. |
| `variants.choices` | Palette character (not a `null` one) → non-empty `[[state, weight], …]`, positive integer weights: each voxel with that character draws its block independently (per voxel, not per structure; a structure-wide pick would be a new operation). |

- **Canonical form and hash.** The generator writes each structure as a canonical text (palette
  sorted by character, canonical state strings, variant values sorted) and its FNV-1a 64 hash into
  Rust tables (`crates/dwell-world/src/structures.gen.rs`, fixtures in
  `crates/dwell-world/tests/fixtures/structures.gen.rs`); the Rust loader rebuilds the text and
  refuses a table whose hash disagrees. Key order and whitespace in the file don't change the hash.
- **Placement and variants** (`dwell-world` `structure.rs`): `load(name)`, then `place(at, seed)`.
  The orientation and the choices come from the seed by integer hashing (splitmix64; choices hash
  the unturned cell, so a cell draws the same block in every orientation). States with a facing
  property (the registry's `facingProperty`, read through its `facing` column) turn and mirror with
  the geometry. No scaling (it destroys voxel detail; author separate sizes).
- **Not yet:** run-length encoding (not needed at 32³; it would be a new layer encoding), shaped
  states (slope corners) under rotation — no shaped block exists since Phase 15 — and placement.
- **The playground** is `shared/structures/playground.json` (22 × 6 × 14, within the cap), placed
  with its anchor at the origin; a test pins it against the old `fill` boxes cell for cell.
- **Placement rules live in the terrain definition, not the structure**, so one structure serves
  several definitions:

```json
"features": [{ "structure": "dwell:boulder_a", "cell": 24, "chance": 0.3, "jitter": 0.8,
  "on": ["dwell:grass"], "maxSlope": 2, "heightAboveSea": [1, 80], "mask": "inland" }]
```

  Jittered-grid spacing (`cell`, `jitter`; minimum spacing for free), `chance`, surface block,
  slope, height band, a named mask — conditions ADR 0026 already lists for surface layers.

- **One generator** (`shared/structures/gen.mjs`, built in S1): validates names against the
  registry, palette characters, layer dimensions against `size`, the anchor, the cap, variant names,
  values and weights; emits a canonical form and a hash; CI fails on stale output and runs
  `gen.test.mjs`. The Rust loader replaced `Structure::parse` (the playground file is converted).
- **Tests**: a fixture structure stamps by data alone, bad entries fail naming the entry, variants
  are deterministic and turn a ladder with the geometry (S1); with placement (S4), a fixture
  definition, and chunks generated in random order hash the same.
- **Determinism**: position, variant and choices from a hash of seed, feature index and cell
  (integer maths, ADR 0010). Each chunk visits the cells within the feature's maximum extent, so a
  tree across a chunk border agrees in any order; ground height from the height field, never from
  neighbour chunks. The LOD applies the same placements at coarse cells or skips small features
  below a documented level. The air-chunk test accounts for feature extents.

## 4. Saving

**Built (S2b, 2026-10-14).**

- The sandbox world persists in its own `.dwellworld` file.
- Saved structures go in a **separate store**: OPFS `dwell/structures/<name>.json` (the name's part
  after the namespace), one file each, in the exact format above, so the library outlives the
  sandbox world and every version can read it (`client/src/local/structureStore.ts`). It is a
  shared browser store, so append-only rules apply: new fields are optional; a rewrite (saving over
  a structure, renaming it) keeps the fields this build doesn't know; a file it can't read (broken
  JSON, a newer `format`, a block or rule this build lacks) is listed as kept and is never
  rewritten or deleted. `structureStore.test.ts` pins it.
- **One set of rules.** The format's rules are `shared/structures/format.mjs` (no Node built-ins):
  `gen.mjs` runs it in CI and the client runs it over its own registry (`BLOCK_DEFS`; a test checks
  it equals the one `gen.mjs` reads from `shared/blocks/vectors.txt`). The client ignores top-level
  fields a newer build added; `gen.mjs` still rejects unknown fields, since the repository has one
  version.
- **Save** (the game menu's Structures page, `ui/structuresPanel.ts`): the box is the region tools'
  selection (B, N, §5) when there is one; else everything built near the player — any state but air
  and the sandbox floor, from 48 m either way across and 8 m below to 48 m above the feet, read a
  chunk tile at a time — or two typed corners (each can be filled from the aimed-at block). It needs
  a name (`dwell:` unless a namespace is typed; `test:` is refused) and takes an anchor, the bottom
  centre by default. The states are read from the client's world (`Client.voxels`, refused where a
  chunk isn't loaded); air and the floor become kept cells (`null`, written `.`), every other state
  gets a palette character (its block's initial where free). Over 32 a side, or with a name taken
  (asks to replace), it says so.
- **Load** puts the structure on the region tools' clipboard in paste mode (`Client.loadStructure`):
  the ghost box follows the crosshair, R and M turn and mirror it, and a right click pastes it as one
  `RegionEdit`, validated by the server like any paste, with undo. Kept cells take what the world
  holds there when the paste is sent (a paste sets every cell of its box). Like a copy, it stands on
  its bottom centre: the anchor is not used by the paste yet (S3b).
- **Export / import JSON** (file download, clipboard copy; import a file or pasted JSON) is the path
  into the repo: an exported file is the stored file, which dropped into `shared/structures/` passes
  `gen.mjs` unchanged (`structures.test.ts` validates an export with the generator's `loadDir`);
  commit it, CI validates it, a definition references it by name.

## 5. Editing tools

Needed (none exist): box select by two corners with an outline (extends the existing target-cell
outline) and its size shown; set anchor; fill, hollow, replace; copy, paste with a ghost preview,
rotate and mirror on paste; undo/redo (a client-side stack of region diffs); eyedropper; brush size
for organic shapes; save, load, export, import. In the game these need a server-side region edit
capped at about 32³ per request.

*Built:*

- **S3a:** box select, fill, hollow, replace, copy and paste with rotate and mirror, and undo/redo
  (ARCHITECTURE.md §6.5).
- **Phase 34:** VoxelSniper-style **brushes** for organic shapes, in [`BRUSHES.md`](./BRUSHES.md):
  - brushes: ball, disc, cylinder, cube, ellipsoid, splatter, line;
  - painting: set, replace, paint surface, erase, with masks;
  - sculpting: smooth, erode, melt, fill/inflate, flatten, raise, lower.

  They aim up to 128 m along the view and show a see-through preview of the exact cells a stroke would change. A brush's size runs from 1 to 16.
  Presets are data, in `shared/brushes/brushes.json`. A stroke goes as region-edit pastes whose
  untouched cells are `KEEP`, and is one undo step.

- **S2b:** save, load, export and import (`IMPLEMENTATION_PLAN.md` Phase 27).

Still to build: the anchor, and the eyedropper (the brushes' replace pick covers part of it).

## 6. Plan and compatibility

| Step | Work | Effort (*estimate*) |
|---|---|---|
| **S0** | Owner answers §7; ADR for data-driven features (reverses B10 if worldgen places them), the format, the placement vocabulary, the gating | 1–2 days |
| **S1** | Structure JSON, Rust loader, `gen.mjs` validation and hash, fixture tests; convert the playground | 3–4 days |
| **S2** | Sandbox world type (flat, the unbreakable white floor block with the `breakable` registry flag, flight on), the structures store, export/import | 3–4 days |
| **S3** | Selection, fill, copy/paste/rotate/mirror, undo; the `RegionEdit` message with server validation and tests; DOM UI | 1.5–2 weeks |
| **S4** | Placement in worldgen: hashed grid placement, conditions, LOD handling, goldens. *Built 2026-10-20 (owner's pines), before Phase 17, in an interim data file (§8)* | 1–1.5 weeks |
| Alt | Standalone editor page exporting the same JSON, instead of or ahead of S2–S3 | 3–5 days |

**Compatibility.** Authoring and saving structures changes no terrain. **Placing** one from
worldgen changes the terrain a seed generates — public API — so S4 ships on a new compatibility
line (the next minor, 0.11), with goldens regenerated; the owner raises `package.json`. Gate it by
including structure hashes in the terrain definition's hash (ADR 0026 §4), and keep sandbox-only
structures out of every definition until promoted. S4 belongs after Phase 17: bolting it onto the
hardcoded `terrain.rs` would be work the baseline rework replaces. S3's `RegionEdit` is an additive
protocol change. *Checked (S3):* a server rejects an unknown message type as `Malformed` and closes
the session, and a `protocolVersion` bump would split the 0.10 line, but every decoder ignores
unknown `Welcome` flag bits — so it ships within the line behind a `regionEdits` Welcome flag that
newer servers set and clients wait for (ARCHITECTURE.md §10.5).

**Risks**: structures across chunk borders and LOD agreement; generation cost (each chunk scans
nearby cells); rotating facing states; adaptive blocks (Phase 18) reshaping authored blocks;
Phase 20 physics treating generated trees as unsupported; a standalone page's look drifting from the
game; bulk edits stressing streaming and save size.

## 7. Questions for the owner

1. What gets added later: more structures only, new variant kinds, new placement conditions, or
   structures with logic (e.g. trunks that grow to the ground)? *Owner, 2026-10-14: all of those
   might be — tree variations, bushes, and later mountains and volcanoes that may need specific
   curve configurations; the big ones are certainly future.* So the format is versioned and its
   vocabularies are open lists: each file has a `"kind"` (`"voxels"`, hand-built, is the only one
   at first; a parametric kind driven by curves can be added for mountains and volcanoes, most
   likely as a terrain-definition node, ADR 0026), and variant and placement entries are named
   operations validated by `gen.mjs`, so a new one is a new name plus its engine code, with no
   change to existing files.
2. Who adds them and with what: only you, in the sandbox, then a commit — or players too (custom
   structures per world, shared on servers)? *Owner, 2026-10-14: only the owner — built in the
   sandbox, exported, committed to `shared/structures/`.*
3. Is the library per build (committed in `shared/`, part of terrain), per world (saved in the world
   file, pasted by players) or per player (the browser library)?
   *Follows from 2: per build, committed in `shared/structures/`; the browser library is the
   sandbox's working copy.*
4. Should worldgen place them at all, given B10 cut trees and rocks? If yes, now or after Phase 17?
   *Owner, 2026-10-14: yes eventually, but not yet — decided once structures exist.* S4 (placement)
   is deferred; S1–S3 don't depend on it. *Owner, 2026-10-20: now — generate the two pines in
   groves* (§8).
5. In-game sandbox or standalone page first? How closely must it match the game's renderer and
   adaptive shapes? *Owner, 2026-10-14: the in-game sandbox first (option (a)), so a structure looks
   as it will in the world.*
6. What is "white": a new prototype block (`dwell:white`), or a sandbox-only look? *Owner,
   2026-10-14: a solid, unbreakable surface that can be built on; a block is fine if that's easier
   than hardcoding something.* So: a registry block (e.g. `dwell:sandbox_floor`: white, full,
   not placeable) marked with the registry's existing `"indestructible": true` (S2a reuses it rather
   than adding a `breakable` flag; it was unused since bedrock went), which the server's edit
   validation checks — data, not a sandbox special case, and usable later for other unbreakable
   ground. It is appended to the registry, so existing ids are kept.
7. Which editing tools are needed first (box select, fill, copy/paste, mirror, undo, brush)?
   *Owner, 2026-10-14: box select, fill, copy/paste with rotate and mirror, and undo; the brush
   later.*
8. Is it acceptable that promoting a structure into worldgen ships as a new minor that changes
   existing seeds' terrain?
   *Deferred with 4.* *Answered by the 2026-10-20 ask: the pines ship on a new minor (0.19.0,
   generator version 17).*
9. What's the largest structure you expect: a tree at about 16³, or buildings at 64³ and more? It
   decides the format (layers or RLE) and the placement search radius. *Owner, 2026-10-14: trees
   and rocks up to about 32³ by hand; perhaps something like a volcano later, but generated by a
   tool rather than hand-built.* So the hand-authored format is layers (RLE optional), capped at
   32³ in validation for now; very large features are a generator concern (a terrain-definition
   node), not a bigger structure file.

## 8. Worldgen placement (S4), built 2026-10-20

The owner asked for the two hand-built pines (`dwell:pine_tree`, `dwell:tall_pine_tree`) in the
generated world; the answers and the measurements are in
[`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §2, the decision in
[ADR 0042](./adr/0042-structure-placements.md). Against §3 and §6's plan:

- **The rules are data apart from the structures**, as §3 sketched, but in an interim file,
  `shared/worldgen/placements.json` (beside `decorations.json`), not a terrain definition: Phase 17
  doesn't exist yet (a deviation from "S4 belongs after Phase 17"; the owner asked now). Its fields:
  `structures` (names → weights), `on`, `footprint` (`on` or `anchor`), `grid`, `density`, `groves`
  (`wavelength`, `coverage`, `outside`), `height`, `rise` (`full`, `low`), `maxSlopeDeg`, `spacing`,
  `rotate`. `shared/worldgen/gen.mjs` validates them (named errors; structures through this
  system's own `loadDir`) and emits `placements.gen.rs`; `gen.test.mjs` adds an entry by data alone.
- **Determinism** as §3 planned: one hashed candidate per grid cell; the chunk visits the cells
  within three reaches and the spacing, so trees across chunk borders agree in any order; ground
  from the height field (a margin of columns computed per chunk), never from neighbour chunks.
  Overlaps: of two candidates whose boxes come within the spacing, the lower priority hash is
  dropped (whether or not the other survives: local, no chains).
- **LOD**: the chunks' own trees drawn at level 1; a canopy tint above (WORLD_GENERATION.md §2).
  The air test leaves room for the tallest structure.
- **Gating**: no terrain-definition hash exists yet, so the generator version (17) and the goldens
  gate the change; the structures' hashes are not folded into anything (a change to a placed
  structure's file changes the goldens, which CI catches).
- The structures' own `variants` are not used by placement; turns come from the placement's
  `rotate` (the pines declare none), and `choices`, if a placed structure has them, are drawn from
  the tree's priority hash.
