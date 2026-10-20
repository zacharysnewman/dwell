# 0038. Block registry sync: the server sends its registry at join, the client maps ids at the boundary

- Status: Proposed (the owner's decision of 2026-10-16 — "we need registry sync; have it go out at
  the same time with 0.12.0" — with the defaults below; amended the same day by the owner: **never
  refuse a join over registry differences** — the server wins and the player is asked; open
  questions at the end)
- Date: 2026-10-16
- Builds on: [0015](0015-block-registry.md) (namespaced states, runtime ids, string palettes on
  disk), [0014](0014-versioned-releases.md) (compatibility lines)
- Design: [`BLOCK_REGISTRY.md`](../BLOCK_REGISTRY.md) §4 "Wire" and §8 "Registry sync"
- Ships in: the next minor, **0.14.0** (a protocol change, protocol 14: a new compatibility line; first planned for 0.12.0)

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
and rules, not only on its name. Two changes bound for the same line add fields the sync must carry: #82
(adaptive blocks, open: an `adaptive` flag and derived top shapes) and transparent blocks
([ADR 0036](0036-render-modes-and-cross-geometry.md), on main: render
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
must-understand tag it does not know names it in the join prompt (it cannot honour it).

**Where the registry travels.** Inside `Welcome` (grows the one message every client parses first;
the 1 MiB reliable limit caps the registry) or as **its own message before `Welcome`** (chosen),
split into parts, on the control stream: everything before `Welcome` is ordered on that stream as
today, so no chunk or edit can arrive before the mapping exists, and `Welcome` stays the join point.

**Known names whose definitions differ.** Refuse the join, or use the server's definition. The
first draft refused; the owner chose (2026-10-16) **the server wins**: the server is authoritative
and the client only views and predicts against it, so a difference cannot harm other players, and
refusing would split players over content the server decides. The client adopts the server's
shape, rules and collision for the session (prediction stays exact, because the server is what it
predicts) and **asks the player** before entering, listing what differs.

## Decision

1. **The `BlockRegistry` message** (S→C, reliable, control channel, type 80), sent by the server
   after `ClientAuth` verifies and before `Welcome`: `u16 part, u16 parts, u32 length, bytes`. The
   parts concatenate to the **registry payload**; the server splits payloads into parts of
   at most 64 KiB less 64 bytes, so each message fits one WebRTC data channel message (64 KiB)
   and travels whole (larger messages are fragmented, ARCHITECTURE.md §8.2).
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
   | 1 | rule flags `u32`: solid (bit 0), indestructible, flooded, climbable, liquid, placeable (5), replaceable (7); bit 6 reserved for #82's `adaptive` | must |
   | 2 | occupancy `u8` (empty, full, shaped) and shape `u16` (index into the shapes) | must |
   | 3 | density `f32` (kg/m³) | must |
   | 4 | facing `u8` | must |
   | 5 | climb speed scale `f32` | must |
   | 6 | selection box `f32` × 6 | must |
   | 7 | `placeOn`: `u8` count, block names `str` | must |
   | 8 | `needsSupport` `u8` (0 none, 1 below) | must |
   | 0x81 | look name `str` | look |
   | 0x82 | colour `u32`, opacity `f32` | look |
   | 0x83 | textures (top, side, bottom `str`) | look |
   | 0x84 | grid (`u8` every, `f32` strength) | look |
   | 0x85 | fluid look (`u16` index into the fluid names) | look |
   | 0x86 | sound set `str` | look |
   | 0x87 | render mode `u8` (opaque, cutout, translucent) | look |
   | 0x88 | cross geometry (`f32` height, `u8` quads, `f32` offset, rotation, height min, max) | look |
   | 0x89 | wind (`f32` strength, speed) | look |
   | 0x8a | fade (`f32` start, end) | look |

   An absent field takes the generator's default. Unknown must-understand tags, or a set flag bit
   the client does not know, are ignored and listed in the join prompt (decision 4).
3. **The client's check: adopt, never refuse** (replaces "the registry hash must match"). On
   `Welcome`, with the registry complete, the client maps every server state and records each
   difference, in three groups:
   - **Different on this server** — the build has the canonical string but a must-understand field
     differs (shape, rule flags, density, facing, climb speed, …): the client registers a **synced
     copy** of the state with the server's shape and rules and its **own look**, and maps the
     server id to it. Its build id stays as it was (its own worldgen still makes it), so locally
     generated terrain would hold the build's rules: while any state differs the client sends a
     `WorldgenCheck` of 0, and the server sends every chunk explicitly (mapped to the copies).
   - **New on this server** — the build lacks the name: a synced state with the received
     definition and the received look if the build has that look and its textures, otherwise the
     fallback look (decision 7).
   - **Only in your version** — build states the server does not list: harmless (the server never
     sends them); listed for information, and the session never sends them in edits.
   Cases the client cannot represent **degrade instead of refusing**, and are listed with the
   state: a shape outside the build's shape vocabulary becomes the full cube (or empty, for an
   empty occupancy) — collision then differs from the server's for that state, which the list says;
   an unknown must-understand field or flag bit is ignored and named. The only refusals left are a
   malformed payload (a protocol error) and a table that would pass `u16`.
4. **The join prompt.** When any group is non-empty, the session waits in a `Confirming` state and
   the shell shows a dialog before entering the world (the menu's theme and sounds): the three
   groups, each item the state's name and the fields that differ, **Continue** and **Cancel**, and
   "Don't ask again for this server", remembered in the append-only browser settings store keyed by
   the server's identity and the definitions hash, so a changed server asks again. Identical
   registries show nothing. Cancel closes the connection and returns to the menu. The differences
   are also logged to the console and summarised in the F3 overlay.
5. **Mapping at the boundary.** The session holds `to_client: Vec<u16>` (server id → client id) and
   `to_server: Vec<u16>` (client id → server id, `NONE` for client-only states). It translates
   in place: incoming `ChunkData` (explicit voxels), `VoxelModification`, `LodData` cells; outgoing
   `BlockEditRequest.material`, `RegionEdit` (material, from, paste cells — brushes are pastes). An
   incoming id outside the server's registry is a malformed message. An outgoing client-only state
   (a block the server lacks, e.g. picked from the build's palette) is not sent; the edit is
   dropped as the server would reject it. When both maps are the identity the session skips the
   pass. Hot paths — meshing, the controller, edit prediction — keep reading one table by id: the
   build's generated array for build ids, and a lock-free append-only extension for synced ones
   (`dwell_blocks::info` adds one comparison).
6. **`WorldgenCheck`.** The client generates the verification chunk in its own ids, translates it to
   server ids and hashes that, so a server whose ids differ (blocks inserted or appended) still
   matches when the terrain is the same; a cell the server cannot name, or any state "different on
   this server", sends hash 0 (every chunk explicit), as a failed check does today. Generated chunks and LOD sections are produced and kept
   in client ids, never translated.
7. **Unknown looks.** A synced state whose look or textures the client lacks is drawn with a
   **fallback look**: a magenta/black checker tile on its shape (a texture tile of its own, so it
   cannot be mistaken for content). Its shape, collision and rules always come from the received
   definition, so prediction stays exact. A synced state whose look and textures the build has (a
   mod block reusing `stone`) draws with them; server-sent textures (resource packs) wait for
   Phase 19's textures-as-data.
8. **Server-side states.** The server's registry is the build's states plus states registered at
   run time from data (`dwell_blocks::runtime::register`: mods, and the test fixture), appended in
   the same table. A world file naming a state the server does not have registers a
   **placeholder** under that exact string (a full, solid, indestructible cube with the fallback
   look), so the world plays, syncs it to clients like any other state, and saves the string back
   unchanged.
9. **Local mode and the sandbox** go through the same messages and session: the in-process server
   sends its registry, the session maps it (the identity), and the client checks the definitions
   hash against its own build's.
10. **Compatibility.** The new message and the changed check are a protocol change: protocol 14 for
   0.14 (main is at 13; Phase 16, #114, also bumps to 14: if both ship together they share it,
   otherwise whichever lands second bumps once more). From 0.14 on:
   - **non-breaking**: adding blocks or states anywhere in the data (ids may shift — they are
     mapped), adding a value to an existing property, changing looks (colour, textures, sound
     sets, fluid looks, grid);
   - **no longer refused, but asked**: changing an existing state's shape or rules — a client of
     another build joins after the prompt, with the server's rules; worlds are unaffected (they
     store names);
   - **still breaking**: removing or renaming a state (worlds and the build's worldgen name it;
     adding or removing a property renames every state of the block), changing the shapes
     themselves or the adaptive shape rules (code that both sides run: a client cannot adopt them
     from the server), changing the terrain a seed generates, and changing the registry payload's
     format.

## Consequences

- Server mods can add blocks from data without a client build, and content changes stop forcing
  compatibility lines. No registry difference refuses a join; the player sees what differs.
  The `fixture` registry gains a server-only block, and a native
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
2. A known name with a different definition: ~~refuse~~ — **decided by the owner (2026-10-16):
   the server wins**, with the join prompt (decisions 3–4).
3. Synced placeable states are **not** added to the client's block palette yet (the palette is
   built from the build's blocks): players can break a mod's blocks but not yet place them. A
   palette fed by the synced registry is follow-up work.
