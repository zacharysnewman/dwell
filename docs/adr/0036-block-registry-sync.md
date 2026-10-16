# 0036. Block registry sync: the server sends its registry at join, the client maps ids at the boundary

- Status: Proposed (the owner's decision of 2026-10-16 — "we need registry sync; have it go out at
  the same time with 0.12.0" — with the defaults below; open questions at the end)
- Date: 2026-10-16
- Builds on: [0015](0015-block-registry.md) (namespaced states, runtime ids, string palettes on
  disk), [0014](0014-versioned-releases.md) (compatibility lines)
- Design: [`BLOCK_REGISTRY.md`](../BLOCK_REGISTRY.md) §4 "Wire" and §8 "Registry sync"
- Ships in: **0.12.0** (a protocol change: a new compatibility line)

## Context

Runtime state ids are dense `u16`s assigned by `shared/blocks/gen.mjs` from the order of the data
files. They travel on the wire unchanged (chunk palette + RLE, `VoxelModification`, `RegionEdit`,
`BlockEditRequest`, `LodData`), so `Welcome` carries the **registry hash** (FNV-1a over the canonical
strings in id order) and the client refuses any mismatch. Adding one block therefore splits builds:
a 0.11.x client cannot join a server built with one more block, even though nothing it knows has
changed. The owner wants server mods that add blocks, as Minecraft has (2026-10-16), and every
content change today forces a compatibility line.

World files already store **string palettes** (`block_states` table, ADR 0015), so the disk side
is id-independent; only the wire and the handshake are not. `BLOCK_REGISTRY.md` §4 planned the fix:
"the server sends its registry (the canonical strings) at join and the client maps them to its own
runtime ids (Minecraft's registry sync)".

Clients **predict** movement and edits (PLAYER_CONTROLLER.md), and generate terrain locally
(`ChunkForm::Generated`, client-side LOD), so client and server must agree on every state's shape
and rules, not only on its name. Two open PRs bound for 0.12 add fields the sync must carry: #82
(adaptive blocks: an `adaptive` flag and derived top shapes) and #95 (transparent blocks: render
modes, `cross` geometry, wind, fade, a selection box, and the placement rules `placeOn`,
`replaceable` and `needsSupport`).

## Options considered

**Where ids are translated.**

- (a) **Server ids on the client, with a per-session runtime table.** The client stores and meshes
  the server's ids directly, looking every state up in a table built from the received
  definitions. No translation of incoming messages; but every hot-path lookup (meshing, the
  controller's collision and climbing queries, edit prediction, ray casts) goes through a
  session-owned table instead of the build's static one, and everything the client produces itself
  — every `Generated` chunk and every client-generated LOD section, which is most of the terrain —
  is generated in build ids and would have to be translated per chunk anyway. In local mode and in
  native tests the server and client share one process, so "the" table cannot be a global.
- (b) **Map at the boundary into the client's own ids** (chosen). The client keeps the build's id
  space (the generated table, so its own worldgen, meshing and physics are untouched) and extends
  it with **synced states** — states the server has and the build does not, appended after the
  build's ids with their received definitions. The session translates every id in every message
  as it arrives (server → client) and as it leaves (client → server), through two flat `u16`
  arrays. Locally generated terrain needs no translation; the common case (same build, or a server
  that only appended blocks) is an identity map up to the build's length, and the session skips
  translation entirely when the whole map is the identity.
- (c) **Names on the wire** (string palettes in every chunk message). Id-free, but every chunk and
  edit carries and parses strings, the definitions still have to be sent for unknown states, and
  the hot paths still need dense ids, so it is (b) with more bytes.
- (d) **Keep the hash check** and bump the line for every content change. What this ADR removes.

**How definitions are encoded.** (i) A fixed per-state struct with a format version: compact, but
every new field (as #82 and #95 add) is a format bump and an old client cannot say which field it
lacks. (ii) JSON: self-describing but heavy, and needs a JSON parser in the WASM core for one
message. (iii) **Tagged fields** (chosen): each state's definition is a list of `(tag, length,
bytes)` fields; a tag's top bit says whether a client may ignore it (looks) or must understand it
(shape and rules). New fields are new tags, defaults are implied by absence, and a client meeting a
must-understand tag it does not know refuses the join naming it.

**Where the registry travels.** Inside `Welcome` (grows the one message every client parses first;
the 1 MiB reliable limit caps the registry) or as **its own message before `Welcome`** (chosen),
split into parts, on the control stream: everything before `Welcome` is ordered on that stream as
today, so no chunk or edit can arrive before the mapping exists, and `Welcome` stays the join point.

**Known names whose definitions differ.** Use the server's definition (server wins) or refuse.
The client's own worldgen, adaptive shape rules and looks are compiled against its definitions, so
silently taking the server's would mispredict; **refuse, naming the state and the field**.

## Decision

1. **The `BlockRegistry` message** (S→C, reliable, control channel, type 80), sent by the server
   after `ClientAuth` verifies and before `Welcome`: `u16 part, u16 parts, u32 length, bytes`. The
   parts concatenate to the **registry payload**; the server splits payloads above 64 KiB.
   `Welcome` keeps its `registry_hash` field, now the **definitions hash**: FNV-1a 64 over the
   payload bytes, for logging and for checking local mode (same build ⇒ same hash).
2. **The registry payload** (format 1; layout pinned by golden vectors, `shared/protocol/vectors.txt`):
   ```
   u16 format (1)
   u16 shape count, shapes        — each baked shape (ShapeInfo and its faces), in the server's order
   u16 fluid count, fluid names   — the fluid looks states reference, by name
   u16 block count, blocks        — in server id order
     str name (ns:name), u8 property count, properties (str name, u8 value count, str values)
     then one definition per state, in property order (keys alphabetical, last fastest)
   definition: u8 field count, fields (u8 tag, u16 length, bytes)
   ```
   States' server ids are their order in the payload; their canonical strings follow from the
   block and its properties (ADR 0015), so names are not repeated per state. Field tags (the top
   bit set: a look the client may ignore):

   | Tag | Field | Kind |
   |---|---|---|
   | 1 | rule flags `u32`: solid, indestructible, flooded, climbable, liquid, placeable; reserved bits for #82 `adaptive` (6) and #95 `replaceable` (7) | must |
   | 2 | occupancy `u8` (empty, full, shaped) and shape `u16` (index into the shapes) | must |
   | 3 | density `f32` (kg/m³) | must |
   | 4 | facing `u8` | must |
   | 5 | climb speed scale `f32` | must |
   | 6–9 | reserved for #95: selection box, `placeOn` (block names), `needsSupport` | must |
   | 0x81 | look name `str` | look |
   | 0x82 | colour `u32`, opacity `f32` | look |
   | 0x83 | textures (top, side, bottom `str`) | look |
   | 0x84 | grid (`u8` every, `f32` strength) | look |
   | 0x85 | fluid look (`u16` index into the fluid names) | look |
   | 0x86 | sound set `str` | look |
   | 0x87–0x8a | reserved for #95: render mode, cross geometry, wind, fade | look |

   An absent field takes the generator's default. Unknown must-understand tags, or a set flag bit
   the client does not know, refuse the join with the field's tag in the message.
3. **The client's check** (replaces "the registry hash must match"). On `Welcome`, with the
   registry complete, the client maps every server state:
   - **Known by name** (the build has the canonical string): its must-understand fields must equal
     the build's, byte for byte, or the join is refused: *"dwell:sand differs from this build
     (shape)"*. The client draws it with its own look.
   - **Unknown**: its shape must equal one of the build's shapes (shapes are a fixed vocabulary of
     the engine, `CLAUDE.md`; a mod that needs a new shape is refused, *"test:wedge uses a shape
     this build lacks"*), and every must-understand field must be known. It becomes a **synced
     state**: appended to the client's table with the received definition (found again by name and
     definition on a later join, so reconnecting does not grow the table).
   - State 0 must be `dwell:air`, and the table may not exceed `u16`.
   Refusals are a client-side `Rejected` with the new reason `Registry` and a message naming the
   state; nothing about the server's registry is trusted beyond the session.
4. **Mapping at the boundary.** The session holds `to_client: Vec<u16>` (server id → client id) and
   `to_server: Vec<u16>` (client id → server id, `NONE` for client-only states). It translates
   in place: incoming `ChunkData` (explicit voxels), `VoxelModification`, `LodData` cells; outgoing
   `BlockEditRequest.material`, `RegionEdit` (material, from, paste cells — brushes are pastes). An
   incoming id outside the server's registry is a malformed message. An outgoing client-only state
   (a block the server lacks, e.g. picked from the build's palette) is not sent; the edit is
   dropped as the server would reject it. When both maps are the identity the session skips the
   pass. Hot paths — meshing, the controller, edit prediction — keep reading one table by id: the
   build's generated array for build ids, and a lock-free append-only extension for synced ones
   (`dwell_blocks::info` adds one comparison).
5. **`WorldgenCheck`.** The client generates the verification chunk in its own ids, translates it to
   server ids and hashes that, so a server whose ids differ (blocks inserted or appended) still
   matches when the terrain is the same; a cell the server cannot name sends hash 0 (every chunk
   explicit), as a failed check does today. Generated chunks and LOD sections are produced and kept
   in client ids, never translated.
6. **Unknown looks.** A synced state whose look or textures the client lacks is drawn with a
   **fallback look**: a magenta/black checker tile on its shape (a texture tile of its own, so it
   cannot be mistaken for content). Its shape, collision and rules always come from the received
   definition, so prediction stays exact. A synced state whose look and textures the build has (a
   mod block reusing `stone`) draws with them; server-sent textures (resource packs) wait for
   Phase 19's textures-as-data.
7. **Server-side states.** The server's registry is the build's states plus states registered at
   run time from data (`dwell_blocks::runtime::register`: mods, and the test fixture), appended in
   the same table. A world file naming a state the server does not have registers a
   **placeholder** under that exact string (a full, solid, indestructible cube with the fallback
   look), so the world plays, syncs it to clients like any other state, and saves the string back
   unchanged.
8. **Local mode and the sandbox** go through the same messages and session: the in-process server
   sends its registry, the session maps it (the identity), and the client checks the definitions
   hash against its own build's.
9. **Compatibility.** The new message and the changed check are a protocol change: protocol 13 for
   the whole 0.12 bundle (#82 already raised 12 → 13; whichever of the 0.12 PRs merges first keeps
   13 and the others do not bump again). From 0.12 on:
   - **non-breaking**: adding blocks or states anywhere in the data (ids may shift — they are
     mapped), adding a value to an existing property, changing looks (colour, textures, sound
     sets, fluid looks, grid);
   - **still breaking**: removing or renaming a state (including adding or removing a property,
     which renames every state of the block), changing an existing state's shape or rules (the
     must-understand fields), changing the shapes themselves or the adaptive shape rules (code that
     both sides run), changing the terrain a seed generates, and changing the registry payload's
     format.

## Consequences

- Server mods can add blocks from data without a client build, and content changes stop forcing
  compatibility lines. The `fixture` registry gains a server-only block, and a native
  client–server test proves it arrives by sync, draws with the fallback look, collides exactly,
  can be targeted and broken, and reaches other clients.
- Every client-bound id passes through the session's maps: chunk messages cost one table lookup per
  explicit voxel (none for `Generated` chunks, none at all under the identity map).
- `dwell_blocks` grows a run-time extension of its table; code that iterated `STATES` to build
  caches (the mesher's styles, the march state table, the palette) reads the extended table and
  rebuilds when it grows.
- The registry hash in world files (names in id order) stays: it still keys the LOD cache, which
  holds runtime ids.
- **Follow-up:** server-sent looks and textures (Phase 19, textures as data); a runtime shape table
  so mods can add shapes; listing synced placeable states in the client's palette; world upgrades
  (aliases for renamed states) in `FUTURE.md`.
- **Reversal:** the hash check could return by refusing any non-identity map; nothing else
  depends on the maps.

## Open questions (defaults taken)

1. Placeholder states for unknown names in world files: **indestructible** (protects a mod's blocks
   while the mod is missing) rather than breakable.
2. A known name with a different definition: **refuse** rather than take the server's.
3. Synced placeable states are **not** added to the client's block palette yet (the palette is
   built from the build's blocks): players can break a mod's blocks but not yet place them. A
   palette fed by the synced registry is follow-up work.
