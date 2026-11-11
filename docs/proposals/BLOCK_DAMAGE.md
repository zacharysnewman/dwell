# Block damage: pickaxe shards, shovel scoops (proposal)

**Status:** proposal (owner's direction and answers, 2026-11-10). Not architecture until an ADR adopts it.

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
