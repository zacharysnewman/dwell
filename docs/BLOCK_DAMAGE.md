# Block damage: pickaxe shards, shovel scoops

**Status:** adopted ([ADR 0078](adr/0078-block-damage.md), 2026-11-10); D1 and D2 built (0.52.28, §5–6),
D3 (tuning) to come. The owner's direction and answers are §1; §2 was the proposal's sketch,
§5 is what D1 built.

## 1. The owner's direction

- "Tools will do damage to blocks, blocks will retain damage until destroyed or repaired (repair not
  planned for soon)."
- "I want pickaxes to break shards off of the struck stone. Shovels will scoop roundish chunks out of
  dirt/sand/etc."
- Answers (2026-11-10):
  1. **Look only:** the shards and scoops are drawn on the block; its collision stays the full block
     until it breaks (unlike the axe's carved log notches).
  2. **Drops:** for wood, stone and dirt, each hit drops some of the resource, and the destroy drops the
     other half ("drop some of the resource on [hit], on destroy drop the other half of the chunks").
  3. **Hits: tiered**, as data: the right tool at stone tier about 5 hits, metal about 3, the wrong tool or
     bare hands about 10–12; bare hands can't break stone.
  4. **Same in both modes:** creative damages blocks over several hits too. Revised (owner,
     2026-11-11): "I should be able to break any block while holding anything or nothing with the
     exception of tools": in creative only a tool's hits count; anything else breaks in one hit (§7).

## 2. Design sketch

- **Per block, per tool, with a default** (owner, 2026-11-10: "Blocks will need per tool and default
  values"): each block (in its block data, `shared/blocks/*.json`, or a material it names) carries a
  `damage` entry: `default` (hits to destroy with bare hands or any tool not listed; `null` = can't)
  and per tool (by tool class or by item: `pickaxe`, `shovel`, `axe`, or `dwell:metal_pickaxe`) its
  hits, an item entry overriding its class's. The tier speeds in `tools.json` only fill in where a
  block gives a class but not the item. Validation: an unknown tool or class, a bad count, a block
  with no default. A tool for the block or item to come (a mod's) adds by data alone.

- **Data** (`shared/tools/tools.json` `breaking`, 0.52.21): per block material a toughness (hits at
  1×), the tool class that suits it and each tier's speed; per material a **damage look** (`shards`
  for stone, `scoops` for soils, `chips` for wood, …) and its **hit drops** (what a hit flings, and the
  share of the block's drops: half over the hits, half on the destroy).
- **State:** a block's damage is a small per-cell record: hits taken (or 0..1 health) and up to N hit
  marks (the struck face and point, for the look). Held by the server, sent to clients in the chunk
  and as an edit, saved with the world (a new optional chunk section: worlds without it load
  undamaged; older builds on the line ignore it if it can be an optional tag, otherwise a new
  compatibility line; to be decided in the ADR).
- **Look:** at each hit mark the face is drawn with a shard knocked off (stone: an angular facet dent
  with a fresher, lighter rock colour) or a roundish scoop (soils: a shallow bowl), in the mesher as
  extra geometry or in the shader as a dent/normal decal; collision unchanged.
- **Feedback:** the hit's particles (the material's chips) and its recorded sound (0.52.15); the drop
  flies out as a spinning ground item that auto-picks up (0.50.68).
- **Tools:** pickaxe on stone, shovel on dirt/sand/gravel/snow, axe on wood (the axe's felling keeps
  its notches on logs; other wood, e.g. planks, takes chips).

## 3. Phases (≤ 45 min each)

- **D1:** damage state + hits to destroy by tool class and tier (data), saved and synced; hit drops
  and the destroy's half; creative and adventure.
- **D2:** the looks: stone shards, soil scoops, wood chips at the struck points.
- **D3:** tuning with the owner; repair later.

## 4. Open

- The exact hit counts per material (tuning).
- ~~Whether bare hands can break stone at all.~~ Answered (owner, 2026-11-10): **bare hands can't break
  stone** (stone's `default` is `null`; it needs a pickaxe).

## 5. As built (D1, 0.52.28)

- **Data** (owner, 2026-11-10: "Blocks will need per tool and default values"): a block's `damage` in
  its block data (`shared/blocks/*.json`), `{ "default": hits | null, "<class or item>": hits, … }`.
  `default` is the hits with bare hands or any tool not listed (`null`: it can't be broken so); a key is
  a tool class of `shared/tools/tools.json` `breaking.classes` (`pickaxe`, `shovel`, `axe`) or an item
  (`dwell:metal_pickaxe`). Resolution (`dwell_protocol::damage::hits_to_break`): the held item's own
  entry; else its class's entry divided by the item's tier speed (`breaking.tiers`: stone 1.5, metal
  2.5), rounded up; else `default`. A block without `damage` breaks in one hit, as before.
  Prototype values: stone and its ramp `{ default: null, pickaxe: 7 }` (stone pickaxe 5, metal 3, by
  hand or another tool: can't); dirt, grass, snowy grass, sand and their ramps `{ default: 11, shovel: 7 }`
  (shovel 5, metal 3, by hand or another tool 11). Logs keep the axe's chopping (no `damage`: felling
  and its notches unchanged).
- **Generator and validation:** `shared/tools/gen.mjs` reads every block's `damage` (as it does
  `breakWith`), emits `BLOCK_DAMAGE` into `dwell_protocol::chopping`'s table and `client/src/ui/tools.gen.ts`,
  and fails on a missing `default`, a count outside 1..255, an unknown class or item, or a non-object
  (`gen.test.mjs`, with a fixture block by data alone; `damage.rs` tests a fixture block and tool table).
- **State** (`dwell_protocol::damage::Damage`): per cell hit and not yet broken, the state it was hit
  as (a cell holding another state is undamaged), the hits taken, and up to `MAX_HIT_MARKS` (4) hit
  marks (the struck face and a point, for D2). The point is the face's centre until the edit request
  carries the struck point (D2). A changed cell's damage is cleared (`broadcast_changes`, with its cuts).
- **Hits** (server, `tick_edits`; creative without a tool: §7): a Break of a block needing more than one hit by the held item
  (adventure: the held inventory slot; creative: the hotbar's tool, told by `ChopAction::Held`, which
  now carries pickaxes and shovels too) records a hit and leaves the block; the edit is acked as
  refused, so a predicted break gives way. The hit that reaches the count breaks it. A block that
  can't be broken so (stone by hand) is refused. Each hit costs one edit credit (the same rate limit,
  10 a second). Same in creative and adventure (owner).
- **Drops** (where blocks are not unlimited): of the block's certain drops (rules with chance 1 and one
  item, at their least count; a block without `drops` drops itself), half (rounded down) is spread over
  the hits before the last (`hit_share`: hit k of n gives ⌊k·h/(n−1)⌋ − ⌊(k−1)·h/(n−1)⌋), the rest and
  every chance drop on the destroy (`destroy_share`), so the hits and the destroy total the block's
  drops exactly. Today's blocks drop one item, so a hit drops nothing yet: the per-hit drops show once a
  block drops more (D3 tuning, the owner's call).
- **Client:** a Break the data (and the mode, §7) says takes one hit is predicted at once as before; one that takes more
  is not predicted (the server's hits so far aren't the client's), and its last hit shows when the
  server's change arrives.
- **Saved** with the world in its own optional record (`block_damage` table, `Damage::to_bytes`
  version 1): a world without it loads undamaged, an older build on the line never opens it, a newer
  record is kept unread, not overwritten. No new compatibility line.
- **Test switch:** `ServerConfig::one_hit_breaks` (the tests' `flat()` turns it on) breaks every block in
  one hit, for tests that break blocks with one click.

## 6. As built (D2, 0.52.28)

- **Sync** (optional, as the creatures'): a server keeping damage sets `inventory_flags::DAMAGE` in
  `Inventory`; a client that sees it sends `DamageHello` and is sent every damaged cell, then each change
  (`Damage`: per cell its fraction 0..255 and marks; and the cells gone). An older client never sees the
  bit and is never sent `Damage` (an unknown reliable message would close it); an older server never gets
  `DamageHello`. Rather than a section in the chunk message, the damaged cells come in `Damage` after the
  hello, as the axe's `Cuts` do (the chunk message has no optional sections).
- **The struck point:** a break request's optional three bytes (1/255ths of the cell, world-oriented),
  sent only to a DAMAGE server (an older one would read trailing bytes as malformed); the client finds it
  where the aim meets the targeted face. The server records it in the hit mark (the face centre without it).
- **The look:** `damage.look` in the block data: `kind` (`shards` | `scoops` | `chips`), `size` (a mark's
  width in cells at full damage), `color` (the fresh face inside), `crack` (the cracks); validated by
  `shared/tools/gen.mjs`. Stone: shards in a pale fresh rock tone; dirt, grass, snowy grass: dark damp
  scoops; sand: a darker sand scoop. The mesher (`mesher/damage_mesh.rs`) draws each mark as decals 4 mm
  off the face in the opaque mesh: a shard's uneven five-sided facet with a darker inner facet, a scoop's
  rim and darker middle, and past 20 % damage three crack wedges that grow with it, kept on the face.
  Face-local: a face-B chunk's cells, faces and points are turned over as its cuts are (`both_faces` test).
  Collision is unchanged.
- **Feedback:** a hit short of breaking plays the material's recorded `Hit` sound at the struck point
  (a one-hit break keeps its break sound).
- **e2e:** `edit.spec.ts`'s `breakBlock` and `menu.spec.ts` hit until the cell breaks; the palette test
  picks the stone pickaxe before breaking a stone piece.

**Built in 0.52.31 (sounds round 2b, AUDIO.md §2e):** hit particles — a block's `damage.chips`
(`count`, `color`, `speed`, `size`; stone grey chips, soil clods, sand grains) fly out of the struck
face at the struck point; other players' hits are heard and seen (the client reads a hit off each
damage update's changed cell, at its newest mark, and skips its own hits' echoes).

**Scoops and shards as dents (scoops-and-herds):** the owner's note that a scoop read "more as a dark
stain than a shallow bowl". A scoop and a shard are now modelled cut into the face, `damage.look.depth`
deep (cells; optional, default 0.1, validated in [0, 0.5]; dirt and grass 0.14, sand 0.12, stone 0.06),
growing with the damage like the mark's width: a scoop is a spherical cap of two rings of quads and a
floor fan (12 segments), darkest at its damp rim; a shard is five angular facets round an off-centre
apex knocked in (one mark in five a flake left proud). Each piece is still drawn flat 4 mm off the face
(the face's own greedy quad stays whole and collision is unchanged) but lit by its modelled normal
(`ground_mesh::push_polygon_shaded`), so the bowl's far wall catches the light and its near wall falls
into shade. Test: `dwell-mesh` `damage_mesh` `a_scoop_is_a_bowl_recessed_into_the_face` (both faces, the
top and a side: modelled behind the face, deepest at its data depth, walls tilted toward the middle,
drawn just off the face). A geometric recess (cutting the face's quad round the mark) was not done: it
would split the greedy face per damaged cell.

**Not done:** the e2e specs are not run locally; the look's tuning with the owner (the cracks read strong);
in-game screenshots of the new dents (not taken in this change's time box).

## 7. Pieces (0.52.46)

- **Pieces** (owner, 2026-11-11: "8 Rock shards make a rock slab, 8 dirt clumps make a dirt slab, 8 wood
  chunks to make a log slab"): a ½ m cell breaks into 8 pieces of its material, as its block's `drops`
  (`{ "item": "dwell:rock_shard", "count": 8 }`): stone and its ramp → `dwell:rock_shard`; dirt, grass,
  snowy grass and their ramps → `dwell:dirt_clump`; log → `dwell:wood`, relabelled "Wood chunk" (its
  name kept: saves hold it; the bucked log's `buckDrops` keep it too). BLOCK_DAMAGE's split applies:
  half (4) over the hits before the last, the rest on the destroy (stone pickaxe: 1 a hit, 4 on the
  break). Grass, snowy grass and now dirt keep (dirt: gain) the random rock (chance 0.5, "we still
  want rocks to be found in grass and dirt at random"); leaves keep their sticks; tall grass drops a
  `dwell:grass_fiber` every time (owner: "2 grass fibers would be the final ingredient for stone
  tools"). Felled pieces (bodies), sand, snow, ice and the rest keep their drops.

## 7. Creative breaks without a tool in one hit (0.52.51)

- Owner, 2026-11-11: "I noticed I can't break blocks like stone with my fists in creative, that's not
  right. I should be able to break any block while holding anything or nothing with the exception of
  tools."
- **Data:** a mode ability, `instantBreaks` (`shared/modes/modes.json`; `dwell_protocol::modes::Ability::InstantBreaks`),
  creative's only. In a mode with it, a Break holding anything but a breaking tool (an empty hand, a
  block, any item not in `tools.json`'s breaking tools) takes one hit, whatever the block's `damage`
  says: no damage record, no hit drops. A tool held keeps its hits and its "can't" (a pickaxe on stone 5
  or 3 hits; a shovel on stone can't; the axe's chopping unchanged). Adventure lacks it: bare hands
  can't break stone.
- **One rule for both sides:** `dwell_protocol::damage::hits_to_break_for(block, held, instant_breaks)`,
  asked by the server (`tick_edits`, the world's mode) and by the client's break prediction
  (`Game::set_instant_breaks`, from the world's mode at join and when the host changes it), so a creative
  break without a tool is predicted at once and not refused.
- Tests: `dwell-protocol` `damage` (the rule and the modes' data), `dwell-sim` `damage`
  (`in_creative_anything_but_a_tool_breaks_any_block_in_one_hit`, both faces; adventure bare hands and a
  creative shovel still can't break stone), `dwell-client` `game_local`
  (`in_creative_a_block_in_hand_breaks_stone_at_once_as_predicted`: predicted, no refusal).
