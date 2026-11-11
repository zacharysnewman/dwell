# Block damage: pickaxe shards, shovel scoops

**Status:** adopted ([ADR 0078](adr/0078-block-damage.md), 2026-11-10); D1 built (0.52.28, §5), D2 (the
looks) and D3 (tuning) to come. The owner's direction and answers are §1; §2 was the proposal's sketch,
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
  4. **Same in both modes:** creative damages blocks over several hits too.

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
- **Hits** (server, `tick_edits`): a Break of a block needing more than one hit by the held item
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
- **Client:** a Break the data says takes one hit is predicted at once as before; one that takes more
  is not predicted (the server's hits so far aren't the client's), and its last hit shows when the
  server's change arrives.
- **Saved** with the world in its own optional record (`block_damage` table, `Damage::to_bytes`
  version 1): a world without it loads undamaged, an older build on the line never opens it, a newer
  record is kept unread, not overwritten. No new compatibility line.
- **Test switch:** `ServerConfig::one_hit_breaks` (the tests' `flat()` turns it on) breaks every block in
  one hit, for tests that break blocks with one click.

**Not in D1 (deferred):** sending the damage to clients (in the chunk and as an edit) — nothing draws it
until D2, and a new S→C message needs the protocol's optional handling, so D2 adds it with the look; the
struck point in the edit request; the crack overlay placeholder. e2e: `edit.spec.ts`'s `breakBlock` and
`menu.spec.ts` now hit until the cell breaks; a palette block that needs a pickaxe (stone) in creative is
broken by hand in `edit.spec.ts` and needs the pickaxe selected or a page switch (not yet run).

