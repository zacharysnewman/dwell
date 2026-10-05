# Dwell — Block Registry: Namespaced Block States and Palettes

> **Status: [planned]** — the design for implementation **Phase 8**
> ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)). As it lands, the built mechanisms move
> into [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§6.1 voxels and the material table, §6.4 persistence,
> §6.6 LOD cache, §8.3 messages) and this file keeps the rationale.

## 1. Why

Today a voxel is a `u16` **material id** from one global, hand-numbered table
(`server/core/include/dwell/core/voxel.h`, mirrored by hand in `client/src/world/materials.ts` and
checked by a test). Each id has one fixed shape, so anything directional is a separate id
(`ladder_n`, `ladder_e`, `ladder_s`, `ladder_w`). Chunks are already **palette + run-length
encoded** on the wire and on disk (§6.1), but the palette entries are those global numbers, so:

- a saved world is only meaningful with the exact table it was written with — renaming, removing
  or reordering a material breaks every world that used it;
- variants multiply ids by hand (four ladder ids; slopes would need 144 per material);
- there is no way to name a block outside the code (data-driven content, commands, mods later).

Minecraft's pattern solves all three, and the owner has adopted it (2026-10-05):

```
minecraft:oak_stairs[facing=east,half=top,shape=straight,waterlogged=false]
```

A **block** has a **namespaced id** (`dwell:dirt`) and declares typed **properties**; a **block
state** is a block with a value for each property. Chunks keep a **palette of the unique states
they contain** and store voxels as compact indices into it; everything else refers to states by
those indices. This becomes the foundation for slopes (Phase 9, `SLOPE_BLOCKS.md`), flooded
blocks, new vegetation materials (Phase 11c), and any later content.

## 2. Identity: blocks, properties, states

- **Block id:** `namespace:name`, lowercase `[a-z0-9_]`. `dwell:` is the built-in namespace; other
  namespaces are reserved for later content (server-defined or mods).
- **Properties** are declared per block, each typed: an **enum** (`facing = north | east | south |
  west`), a **bool** (`flooded`), or a small **int range**. Each has a default.
- **Canonical state string:** `ns:name[k1=v1,k2=v2,…]` with **every** property present and keys in
  alphabetical order (`dwell:ladder[facing=north,flooded=false]`); a block without properties is
  just `ns:name`. The parser also accepts missing properties (defaults) and any key order; the
  writer always emits the canonical form, so equal states have equal strings.
- **Block definitions are data**, not code: `shared/blocks/*.json` (one file per namespace or
  group) lists blocks, their properties, and their per-state behaviour — render style and texture
  tiles, collision shape (a function of the state, e.g. the slope shape for `shape=wedge,
  facing=east`), density and strength, liquid, climbable and facing, `placeable`, what breaking
  drops, the LOD colour. A generator script emits the C++ and TypeScript registries from it (like
  `shared/protocol/constants.json` → `constants.gen.*` today), replacing the hand-mirrored tables.
  "Content is prototype" (§6.1) now has a concrete home: the real block set replaces these files.

**Family convention for shaped variants** (decided in Phase 9's ADR, recommended here): plain
blocks stay plain (`dwell:stone`), and shaped forms are separate blocks generated per shapeable
material, carrying only the properties that matter to them — `dwell:stone_slab[half,flooded]`,
`dwell:stone_slope[facing,flooded,half,shape]` (9 shapes × 4 facings × 2 halves × 2 flooded = 144
states) — each linked to its base material (textures, density, drops). This avoids meaningless
property combinations (a full cube has no facing) while keeping one material identity.

## 3. Runtime ids and the registry

- At startup, the **registry** enumerates every block in a fixed order (namespace, then name) and
  each block's states in property order (keys alphabetical, values in declared order), giving each
  state a dense **runtime state id** (`u16`; ~2,000 states expected, well under 65,536). Hot paths —
  generation, meshing, collision, the chunk arrays — use runtime ids exactly as they use material
  ids now.
- Runtime ids are **never persisted** as meaning. They are a function of the registry's content,
  summarised by a **registry hash** (FNV-1a 64 over the canonical strings in id order).
- Lookups: state ↔ string, block → its states, state → property values, and "the same state with
  `facing=east`" (property updates, for placement and rotation) — all table lookups.
- The worldgen code resolves the block states it uses by name once at initialisation.

## 4. Storage, wire and LOD

**Chunks in memory.** Unchanged at first: a `u16` runtime id per voxel. **Paletted containers**
(Minecraft's in-memory form: a chunk-local palette plus bit-packed indices, 4–8 bits per voxel,
with a single-value form for all-air / all-stone chunks) would cut chunk memory 2–4×, which matters
for the phone memory budget (Phase 4), at some cost on random access. Measure on the meshing and
collision paths before adopting; it is a deliverable gated on that measurement.

**World file (SQLite, ADR 0006).** The persistent form becomes strings:

- A per-world **`block_states`** table: `(world_state_id INTEGER PRIMARY KEY, state TEXT UNIQUE)` —
  each canonical state string stored once.
- Chunk blobs keep the palette + RLE + zstd encoding, but their palettes list **world state ids**
  (indices into `block_states`), not runtime ids. On load, each world state id maps to a runtime id
  through its string; on save, runtime ids map back. Ids are world-local and stable for the life of
  the world file, whatever the code's registry does.
- **No migration.** Worlds are locked to the app version that created them (Phase 6,
  [`RELEASES.md`](./RELEASES.md) §6), and saves from before the version launcher are not carried
  over (owner, 2026-10-05), so no existing world file is converted: a world written before the
  registry keeps opening in its own build. The storage suite's golden world file is regenerated in
  the new format.
- **Deferred to world upgrades** ([`FUTURE.md`](./FUTURE.md)): a data table of aliases and upgrade
  rules (`dwell:old_name → dwell:new_name`, property renames, defaults for added properties), and a
  placeholder for unknown states (`dwell:unknown`, keeping the original string so saving writes it
  back). String palettes on disk are what make these possible later.
- The **LOD cache** (`lod_sections`) is a cache: it is dropped when the registry hash changes,
  instead of being migrated.

**Wire.** Runtime ids stay on the wire (palette + RLE of `u16`, unchanged), because server and
client must already run the same build (protocol version). `Welcome` adds the **registry hash**;
a mismatch is rejected like a protocol mismatch. The verification chunk (`WorldgenCheck`) already
proves identical generation, and it hashes runtime ids, so it implies the same registry too.
*Later*, for server-defined content: the server sends its registry (the canonical strings) at join
and the client maps them to its own runtime ids (Minecraft's registry sync); the hash makes that a
drop-in addition.

**Determinism.** Worldgen output is runtime ids, so the golden hashes depend on the registry:
adding or reordering blocks changes them (regenerate, as for a generator version bump). Pure
additions at the end of the order keep earlier ids; the golden test also records the registry hash
so a failure says which changed.

## 5. What uses it

- **Ladders** become `dwell:ladder[facing,flooded]` (one block, 8 states).
- **Slopes and slabs** (Phase 9): the shaped families above; **flooded** = water in a shaped
  block's open part (Minecraft's `waterlogged`): the mesher draws water there, the controller's swim
  layer treats it as water, and the generator may place flooded slopes on lake and sea floors. This
  resolves the "slopes under water" question.
- **Vegetation variants** (Phase 11c) are new blocks in the data files.
- **Placement and editing:** the creative palette lists blocks; placement chooses property values
  (facing from the player, `half` from the hit face); `VoxelModification` carries runtime ids as
  now. Commands and tools can name blocks by string (`/fill … dwell:stone_slab[half=top]`), parsed
  by the registry.

## 6. How Phase 8 is checked

- The canonical string round-trips for every state (parse → state → write is identity); parsing
  accepts any key order and missing defaults; invalid strings are rejected with a clear error.
- The C++ and TypeScript registries are generated from the same files and produce identical state
  orders and registry hashes (a test on both sides against the same vector).
- A world saved, closed and reopened in the same build is identical (terrain, edits, every voxel's
  canonical string); the regenerated golden world file is read by the native and browser storage
  suites.
- Determinism goldens pass (regenerated once if runtime ids change); protocol golden vectors for
  `Welcome`'s registry hash; e2e "break and place every palette block" still passes.
- If paletted containers are adopted: chunk memory and meshing time measured before and after.
