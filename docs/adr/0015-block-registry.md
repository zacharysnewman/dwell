# 0015. Block registry: namespaced block states, generated registries, string palettes on disk

- Status: Accepted
- Date: 2026-10-06
- Resolves: ARCHITECTURE.md Open Decisions #19 (block identity; voxel shapes and slopes under water
  stay with Phase 9's ADR)
- Supersedes: the material-table parts of ARCHITECTURE.md §6.1 (one hand-numbered `u16` table,
  mirrored by hand in TypeScript)

## Context

A voxel was a `u16` index into one global, hand-numbered material table
(`voxel.h`), mirrored by hand in `materials.ts`. Each id had one fixed shape, so anything
directional was a separate id (`ladder_n/e/s/w`), and a saved world's chunk palettes held those
numbers, so renaming, removing or reordering a material broke every world that used it. Slopes
(Phase 9) would need 144 ids per material. The owner adopted Minecraft's pattern on 2026-10-05
([`BLOCK_REGISTRY.md`](../BLOCK_REGISTRY.md)).

## Decision

1. **Identity.** A block has a namespaced id (`dwell:ladder`) and typed properties (enums; booleans
   are `false`/`true`). A block state has a value for each property and a **canonical string**:
   `ns:name[k=v,…]`, every property, keys alphabetical (`dwell:ladder[facing=north,flooded=false]`);
   a block without properties is `ns:name`. The parser accepts any key order and missing properties
   (the first declared value); the writer always emits the canonical form.
2. **Data files, generated registries.** Blocks are defined in `shared/blocks/*.json`
   (properties, collision shape, density, liquid, climbable, launch speed, palette membership, look,
   colour, textures). `shared/blocks/gen.mjs` emits the C++ tables (`blocks.gen.h`) and the
   TypeScript tables (`blocks.gen.ts`) and a shared vector (`vectors.txt`); CI fails when they are
   stale. Behaviour is per state, so every physics path reads one table by runtime id as before.
3. **Runtime ids.** Dense `u16` state ids, assigned by the generator: blocks in the order the files
   declare them (namespaces by name; air first, id 0), each block's states in property order (keys
   alphabetical, the last key varying fastest, values in declared order). The design note proposed
   sorting blocks by name; declared order was chosen so that appending a block keeps every earlier
   id (and golden hash) unchanged. The code names states through generated constants
   (`Materials::kStone`, `Materials::kLadderFacingEast`: non-default property values are appended to
   the name), resolved at generation time; `ParseState`/`stateId` look states up by string at run
   time. Ids are never persisted as meaning.
4. **Registry hash.** FNV-1a 64 over the canonical strings, each followed by `\n`, in id order.
   `Welcome` carries it (protocol v11); a client whose hash differs rejects the server (`Reject`
   semantics, `ProtocolVersion`), because runtime ids on the wire would name other blocks. Both
   sides check the same vector (`shared/blocks/vectors.txt`).
5. **World file (format 3).** A `block_states(world_state_id, state)` table holds each canonical
   string once, ids assigned in order of first use and stable for the life of the file. Chunk blobs
   keep palette + RLE + zstd but their palettes list **world state ids**; `WorldDb` maps runtime ↔
   world ids through the strings on load and save (reading the table inside the save transaction,
   because the I/O thread has its own connection). A string the build's registry does not know makes
   its chunk unreadable (it is generated again); aliases, upgrade rules and an `unknown` placeholder
   are deferred to world upgrades (`FUTURE.md`). **No migration:** files of formats 1 and 2 are
   refused (worlds are version-locked, [ADR 0014](./0014-versioned-releases.md)). The LOD cache keeps
   runtime ids (it is a cache) and is dropped on open when `meta.registry_hash` differs.
6. **Wire.** Chunk palettes on the wire stay runtime ids (server and client run the same build).
   Sending the registry at join (Minecraft's registry sync) is a later, drop-in addition for
   server-defined content.
7. **Paletted in-memory chunks: not adopted yet.** Measured (below) and deferred.

## Measurement: paletted in-memory chunks

5,872 generated terrain chunks (seed 0, 24 × 24 columns ≥ 7 chunks apart, 16 rows, air chunks
skipped), native `-O2`. A chunk-local palette with bit-packed indices (single-value form for
uniform chunks):

| | Plain `u16[32768]` | Paletted |
|---|---|---|
| Memory | 367 MiB (64 KiB each) | 51 MiB (**7.1× smaller**; palettes hold 1–10 states, median 5 → 3 bits) |
| Collision-style scan (every voxel and its six neighbours, 5,872 chunks) | 625 ms | 1,679 ms (**2.7× slower** with a per-voxel `Get`) |

The saving is large and matters for the phone memory budget (Phase 4), but adoption changes the
`Chunk` API every user touches (`voxels()` is read by the WASM bridge, hashing, meshing input and
the generators write it directly), and a naive per-voxel `Get` costs 2.7×; a fair trade needs
row-wise unpacking in the collision builder and the sender first. The meshers run in TypeScript on
padded `Uint16Array`s and do not benefit. Deferred until a memory budget forces it; the numbers
stand as the baseline.

## Consequences

- Saved-world format and the ids on the wire change: a **breaking change** to the saved world
  format and the network protocol, so this needs a new compatibility line (before `1.0.0`, the next
  MINOR); the owner raises `package.json` ([RELEASES.md](../RELEASES.md)).
- The determinism goldens are regenerated once (ids moved: four ladder states and the second
  `flooded` value shifted everything after the slab); each records the registry hash so a failure
  says which changed.
- `flooded` exists on ladders but has no behaviour until Phase 9 gives shaped blocks water in their
  open part.
- Content stays prototype: the data files hold today's placeholder set (§6.1).
