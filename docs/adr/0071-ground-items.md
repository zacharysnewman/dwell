# 0071 — Ground items: sticks and stones scattered at chunk load, knocked loose by hitting them

**Status:** Accepted (2026-11-09; built in 0.50.68; amended 0.50.73, one look per item, and 0.50.74,
hit instead of picked up: see the amendments at the end, which replace Decision 5). Builds on ADR
0066 (items, drops, inventory).

## Context

Owner (2026-11-09): "We'll have pickup able small stones on the ground and sticks. These are a new
type of ground item, since they're not the spinny dropped item, but physical objects. Sticks show
up under trees leaves. Rocks near cliff bases and less commonly randomly on grass blocks. E/X to
pickup." Answers:

- Scattered **at chunk load** as a layer: not saved terrain and not a worldgen change (existing
  worlds get them), deterministic from the seed and the chunk; picked-up ones stay gone, saved
  with the world (the tree tombstones' model, docs/TREE_GROWTH.md §5a).
- **E** (keyboard) / **X** (controller) picks one up when the crosshair is on it; otherwise E opens
  the backpack as before.
- They give the **same items as drops** (`dwell:stick`, `dwell:rock`), so they stack together.

What is added later: more kinds and rules (data), by the owner or a mod editing
`shared/ground/ground.json` (a tool could). Fixed in code: the condition vocabulary (`on`, `under`,
`nearWall`), the shapes (`rod`, `lump`), the scatter's hash. Per build: the rules; per world: the
gone set.

## Decision

1. **Data.** `shared/ground/ground.json`: `kinds` (a name, the `item` it gives, a `shape`, a `size`
   in metres, the block whose side `texture` it wears, a `color` tint, a `scale` range) and `rules`
   (a `kind`, the full blocks it lies `on`, optionally `under` — canopy blocks within `withinRows`
   rows above — and `nearWall` — a column `1..=reach` cells away in a compass direction solid for
   `rows` rows from the cell's row: a cliff's foot — and a `density`). `shared/ground/gen.mjs`
   validates every name against the blocks and the inventory's items (a bad entry fails naming
   it; reach is bounded by the client's mesh apron, 16 rows and 6 cells) and emits `dwell-world`
   `ground.gen.rs`; `gen.test.mjs` adds a kind and a rule by data alone. CI regenerates and fails on
   a diff. Shipped: a stick under leaves (8 % of the cells under a canopy), a rock at cliff feet
   (12 %), a rock on open grass (0.3 %).
2. **One scatter function** (`dwell_world::ground::Rules::item_at`): at most one item per open cell
   (air or a walk-through plant, no liquid) over a solid full block; the first rule whose
   conditions hold and whose roll (a splitmix hash of the seed, the face-local cell and the rule's
   index) is under its density places its kind, turned, scaled and offset in the cell by more of
   the hash. Unknown cells (not loaded) fail a condition. Face-local through `FaceFrame`: face B
   is face A's mirror image (a parity test).
3. **Evaluated on the current blocks**, by the server (checks), the client's crosshair and the
   client's mesh jobs (the chunk's sky volume). So they agree where their blocks do, and an item
   whose ground or canopy is gone simply isn't there.
4. **Drawn in the chunk mesh**, not as entities: each item is a few polygons (a box twig; a squat
   tapered hexagonal lump) with a block's side tile and the cell's flood light, appended to the
   chunk's opaque mesh (`dwell-mesh` `mesher/ground_mesh.rs`). Hundreds per view cost no draw call
   or buffer of their own, and the CPU adapter needs no tier of its own (a handful of triangles per
   item, inside the mesh it already draws). **No collision**: a 5 cm twig and a 15 cm stone are
   below what the controller would notice, and walking through them is cheap and harmless.
5. **Pickup is the server's.** `InventoryAction::Pick { cell }` (only to a server whose
   `Inventory` flags have `GROUND`): the server finds the item there now (not gone), within 5.5 m of
   the eye (block reach + 0.5), adds one of its kind's item (refused when it doesn't fit), adds the
   cell to its gone set and sends `GroundGone` to the clients that said `GroundHello`; they mesh
   the chunk again without it. Creative picks up too (ADR 0066 §7: loose things can always be
   taken). The client's E handler asks the game first (`Client.pickUp`); X is the spyglass's button
   and picks up instead when aimed at one (no second binding on X, so the bindings data's
   uniqueness holds). A prompt under the crosshair names it ("E  Pick up Stick").
6. **Knocked loose.** Breaking the block under an item (a block edit) turns it into a loose item
   where it lay in a mode that uses items (gone in creative, where breaking drops nothing) and adds
   its cell to the gone set, so it doesn't come back. Other edits (regions, blasts, felling) only
   make it vanish while its ground is gone; it can return when the same blocks return.
7. **Wire and save, no new compatibility line.** The Welcome flags byte is full (`vitals` took
   128), so `Inventory`'s flags carry `GROUND` (2): old clients ignore the bit and never send the
   new `InventoryAction` kinds (3 `GroundHello`, 4 `Pick`); old servers never set it. S→C
   `GroundGone` (93) goes only to a client that said `GroundHello`: all gone cells at the hello (at
   least one message), then each change. A new optional `ground_gone` table (one record: version 1,
   count, sorted cells); an unreadable record is kept and not written over.

## Questions (parked; defaults chosen)

1. **Densities**: 8 % under canopies, 12 % at cliff feet, 0.3 % on open grass. More or fewer?
2. ~~**The prompt says "E"**~~ moot from 0.50.74: no prompt (hit them instead).
3. ~~**Touch**~~ answered (owner, 2026-11-09, "Do it"): from 0.50.70 a **Pick up** button showed under
   the crosshair while it was on a ground item; removed in 0.50.74, where touch's Break hits it.
4. **Sending all gone cells at the hello** grows with play; per chunk with the chunk later if it
   ever matters.
5. ~~**Leaves broken over sticks**~~ answered (owner, 2026-11-09, "Keep sticks under cleared
   leaves"): the rules read the generated terrain (amendment 0.50.75 below).

## Consequences

- A new ground item is a kind and a rule in `ground.json`; a new kind of condition or shape is code.
- Tests: the generator's validation and a fixture kind and rule; the scatter's determinism, the
  rules (sticks only under leaves; rocks at cliff feet far more than on grass), water and unknown
  cells, resting on the ground, the crosshair's target and a wall in front, all on both faces and
  face B mirroring A; the gone set's bytes; the server's pick in reach (stacking with a dropped
  rock), refused out of reach, twice or where none lies; knocked loose by breaking its ground; the
  gone set across a reload; the messages only after the hellos; the protocol round trips; the mesh
  job codec; the meshes' faces outward.
- Reversing it: clear the server's `GROUND` flag; the table stays, unread.

## Amendment (0.50.73): one look per item

Owner, 2026-11-09: "Why don't sticks and rocks in your hand and hotbar look like sticks and rocks on
the ground?" The look (`shape` rod or lump, `size`, `texture`, `color`) moved from the ground kind to
the inventory item (`shared/inventory/inventory.json` `look`), the one source of truth: a ground kind
names only its item (and its `scale`), and `shared/ground/gen.mjs` copies the item's look into
`KINDS`, failing when the item has none. The same look draws the loose item
(`dwell_mesh::look_mesh`, every side, the underside too), the hotbar and backpack picture
(`dwell_mesh::look_swatch`, the shape rasterised in its tinted tile) and the hand (`Held::Look`: the
characters item `rod` or `lump` in the look's tile and tint). Deviation: the hand holds the shape at
its own size (characters.json), not the look's `size`, as the hand's items are fixed meshes.

## Amendment (0.50.74): hit them, no pick-up button

Owner, 2026-11-09, replacing the pick-up controls: "make it so if you hit the rock or stick they
drop as the spinning item that auto picks up, no special button to pick up." This replaces
Decision 5's E / X / touch pick-up and the prompt:

1. **The break action** (the mouse's break click, the touch Break button, the controller's break,
   an axe's swing) aimed at a ground item **knocks it loose** when the item is nearer along the
   crosshair's ray than where the ray enters the block the block targeting found
   (`dwell_world::ground::Rules::hit_before`), so a hit on a stick doesn't break the ground under it;
   a block in front of it is broken as before. With the axe held the click knocks it at once (not
   through the swing's sweep). The brush and paste mode keep their own meaning of break.
2. **The server** (`server/ground.rs` `pick_ground`) checks that it lies there, not gone, within 5.5
   m of the eye of a player who may edit, records it gone (`ground_gone`, `GroundGone` to the
   clients) and, in a mode that uses items, drops a **loose item** of its kind's item where it lay
   (ADR 0066's spinning item, collected by walking near it after the pickup delay). **Creative**:
   ADR 0066's rule that breaking drops nothing applies, so the item **simply vanishes** (as one
   knocked loose by breaking its ground already did, Decision 6). The other players see the arms'
   break swing.
3. **Removed**: E's pick-up (E is the backpack only again), the controller's X pick-up (X is the
   spyglass only), the touch **Pick up** button (0.50.70, Question 3) and the "E  Pick up …" prompt,
   with `Client.pickUp` / `groundPrompt` and the HUD's `pickUp` field.
4. **Wire: unchanged and compatible.** The client still sends `InventoryAction` 4 (`Pick { cell }`,
   the name kept from when it picked up) for the hit; only the server's meaning changed. An older
   client on the line pressing E now knocks the item loose instead of taking it; a newer client on
   an older server picks it up as before. No new compatibility line.

Tests: hitting a rock knocks it loose (a loose rock where it lay, not into the inventory, gone from
the scatter, the client told) and walking over it collects it, stacking with a dropped one; a second
hit, one out of reach and one at nothing are refused; creative's hit makes it vanish with no loose
item; the gone set across a reload (both faces where it matters); the break action hits a nearer
item before the block behind it, and a block in front of it first (both faces); E is the backpack's
key only (`backpackKey`).

## Amendment (2026-11-09, 0.50.75): the rules read the generated terrain

Owner: "Keep sticks under cleared leaves." The axe clears leaves (0.50.62), and Decision 3's rules
read the current blocks, so clearing a canopy made its sticks vanish. Now **the rules (`on`,
`under`, `nearWall`) read the terrain as the chunk was generated**; only the item's own cell (open)
and its ground (a solid full block) are read as they are now. So leaves cleared over a stick, or a
cliff broken away beside a rock, leave it; a broken ground still knocks it loose (Decision 6) and a
hit still takes it, and a block built where it lies still hides it. A wall a player builds grows no
rocks.

- `dwell_world::ground::Rules::item_over` / `scatter_over` take `get(cell, generated)`; `item_at` /
  `scatter_chunk` are the same with one reading for both (an unedited world). `target` reads both.
- **Server**: `VoxelWorld::generated_voxel`: an unedited chunk's voxel, else its chunk generated
  again from the world's generator, cached (`Server::ground_generated`, 64 chunks; not saved).
- **Client**: the streamer keeps every edited loaded chunk as generated: a generated chunk's voxels
  are copied before its first edit (a server modification or a prediction); one that arrives
  edited (`ChunkForm::Explicit`) is generated again in a generation worker
  (`ChunkSource::generate_base`, `Purpose::Base`, `on_generated_base`, which re-meshes the chunks
  reading it). A mesh job gets its sky volume as generated (`MeshJob::generated`, `GroundJob`'s,
  empty when nothing in it is edited); the crosshair reads `ChunkStreamer::generated_voxel`. Only
  edited chunks cost memory (64 KiB each while loaded). A client that cannot generate (a native
  server's world) reads them as they are now, as before.
- Wire, saves and worldgen unchanged: no new compatibility line.

Tests: a stick stays under cleared leaves and goes with its ground (`dwell-world`, both faces); a
rock at a generated wall's foot stays when the wall is broken (`dwell-sim` `items`, both faces; the
ground scene's floor and wall are now generated); mesh jobs carry the sky volume as generated once
edited, and an edited arrival is generated again (`dwell-game` `chunk_stream`, both faces).
