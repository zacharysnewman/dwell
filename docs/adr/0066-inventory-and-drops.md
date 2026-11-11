# 0066 — Inventory and block drops: items as data, drops as light server entities

**Status:** Accepted (2026-11-08; built in 0.50.33, see "Built so far"). Builds on ADR 0065
(game modes): adventure loses `unlimitedBlocks`.

## Context

Adventure mode needs items: breaking a block should give something to pick up, and placing should
use it up. The owner's answers (2026-11-07/08):

- Breaking a block in adventure **drops an item on the ground**; walking near **auto-collects** it
  (about 1.5 m, when there's room).
- **Inventory:** a 9-slot hotbar plus a 27-slot backpack (9 × 3), stacks of 64; all three numbers
  are data.
- **No tools yet:** hands break everything. Breaking time as block data is listed as next (there
  is no break-progress mechanic to hang it on today: a break is one click).
- **What a block drops is in that block's data** in the block registry: itself by default, or other
  items, with counts and chances.
- **First drop rules** (owner, 2026-11-08): "Grass drops rocks and dirt and dirt drops dirt. Leaves
  drop sticks." Rocks and sticks are not blocks: the first non-block items.
- "Tall grass drops grass seed half the time. And gems … 1/8 of the time. Value 1 and Value 5 (5% of
  those times). **Wallet is separate from inventory slots, maxed at 99** with starting wallet."
  (owner, 2026-11-08; not another game's names: `dwell:gem` and `dwell:gem_large`, prototype.)
- Dropped items **vanish after a while** (data, 5 minutes).
- On **death** in adventure, everything drops where the player died.
- **Creative is unchanged:** unlimited blocks, nothing drops. The inventory is per player, saved
  with the world; the mode decides whether placing consumes and breaking drops.

Who adds things: the owner or a mod, by editing data (a tool could): new items in
`shared/inventory/inventory.json`, new drop rules in a block's `drops`. Fixed in code: the inventory
model, the drop entity's motion, and where breaks and places are counted. Scope: items are per
build (data), inventories per player per world, the rules per mode.

## Decision

1. **Item kinds are names.** An item is a namespaced name. Every block is an item of its own name
   (`dwell:dirt`), so blocks need no second list; non-block items are listed in
   `shared/inventory/inventory.json`'s `items` (`dwell:rock`, `dwell:stick`, `dwell:grass_seed`,
   `dwell:gem`, `dwell:gem_large`: a label, a colour and a size for their simple look, optional
   `stack`, and `value` for currency). A block's item places that block; a non-block item places
   nothing for now (planting seeds is future). A name in neither (a newer build's item in a saved inventory) is
   kept as it is, so nothing is lost when an older build on the line opens the world, and cannot be
   used until a build knows it.
2. **Inventory data.** `shared/inventory/inventory.json` also holds `hotbarSlots` (9), `backpackSlots`
   (27), `stackSize` (64, an item may lower it with `stack`), `despawnSeconds` (300),
   `pickupRadiusM` (1.5), `pickupDelaySeconds` (a dropped item can't be taken back at once) and
   `maxDrops` (the most loose items a world holds; the oldest go first), and `wallets` (a name, a
   label and a `capacity`) with `startingWallet` (`dwell:pouch`, 99).
   `shared/inventory/gen.mjs` validates it, and every block's `drops` (below), and emits
   `crates/dwell-protocol/src/inventory.gen.rs` and `client/src/ui/inventory.gen.ts`. CI regenerates
   both and fails on a diff; `shared/inventory/gen.test.mjs` adds an item and a block's custom drop
   by data alone.
3. **Drops are block data.** A block's optional `drops` in `shared/blocks/*.json` is a list of
   `{ "item": name, "count": n or [min, max], "chance": 0..1 }`, or a weighted pick:
   `{ "chance": 0.125, "pick": [{ "item", "count", "weight" }, …] }` (tall grass's gems: value 1 ×95,
   value 5 ×5). Absent: the block drops one of itself, except that a stored ramp drops what its
   base (`of`) drops, a block's extra cell
   (`partOf`) and a liquid drop nothing, and an indestructible block can't be broken. `[]` drops
   nothing. A bad entry fails naming the block and the problem (an item no block or item list
   names, a count or chance out of range). The roll uses the server's entropy.
4. **The inventory model.** A player has `hotbarSlots + backpackSlots` slots, hotbar first; a slot
   is empty or `(item, count)`. Adding fills stacks of that item first, then empty slots, hotbar
   first; what doesn't fit is left (on the ground). Placing a block takes one of that block's item
   (a ramp takes its base) from the held slot if it holds it, otherwise from the first slot that
   does; without one the edit is refused. The client moves items between slots with a
   `move(from, to, count)` (onto an empty slot or the same item it merges, onto another item it
   swaps); the server checks and applies it, and sends the result. **The wallet** is apart from the
   slots: currency items (a `value`) add their value to it on pickup, whole items only, up to its
   capacity; a gem that doesn't fit stays on the ground. A full inventory still takes currency.
5. **Drops are light server entities, not Rapier bodies.** A loose item is an id, an item, a count,
   a point, a fall speed and an age. It falls toward the midplane (face-local, through
   `dwell_world::FaceFrame`: `+y` up on both faces) until the cell under it is solid, then rests on
   that cell's top; if that cell is broken it falls again. No collisions with each other, bodies or
   players: thousands cost a cell lookup each tick while falling and nothing while resting. Rapier
   is not justified: a loose item never needs to tumble, stack or be pushed (future: water carrying
   them, `FUTURE`).
6. **Pickup, despawn, death.** Each tick a resting or falling item older than the pickup delay goes
   to the nearest living player within `pickupRadiusM` whose inventory has room (all of it or what
   fits; the rest stays). An item older than `despawnSeconds` is gone. A player who dies in a mode
   without `unlimitedBlocks` drops every slot at the death point, spread a little; the wallet is
   kept (Question 7).
7. **Mode gating.** Without `unlimitedBlocks` (adventure, from this change) breaking drops and
   placing consumes; with it (creative) neither, and loose items already in the world can still be
   picked up. Adventure's `unlimitedBlocks` placeholder is removed from `shared/modes/modes.json`.
8. **The wire: new, optional messages (no new compatibility line).** An older client fails on a
   message type it doesn't know, and an older server rejects one, so neither is sent unasked:
   - Welcome gains flag **`INVENTORY` (16)**: this server keeps inventories. Older clients ignore
     the bit.
   - C→S **`InventoryAction`** (85), control channel, only to a server with `INVENTORY`: `Hello`
     (this client understands inventories: the server sends it the messages below from then on),
     `Move { from, to, count }`, `Select { slot }` (the held hotbar slot).
   - S→C **`Inventory`** (86), control channel, only to a client that said `Hello`: every slot (item
     name and count; names, not ids, so the item namespace needs no registry sync), sent when it
     changes, and whether the mode uses items (adventure).
   - S→C **`ItemDrops`** (87), world channel, only to a client that said `Hello`: loose items
     appeared or moved (id, item name, count, point) and gone (ids); at Hello, all of them.
   An older client on a newer server plays as before (its places beyond what it holds are refused
   in adventure, which it already handles as a refused edit). A newer client on an older server
   sees no flag and shows the creative hotbar.
9. **Save format: new tables, append-only.** Inventories are a new `inventories` table keyed by the
   device key (a version byte, the held slot, the wallet's name and money, then each slot's index,
   item name and count), and loose items a new
   `loose_items` table (one record, the whole set each save, like bodies). Older builds don't open the tables and
   keep them; a record they can't read is left out and preserved. Player records are unchanged.
10. **The UI.** In a world that uses items, the hotbar shows the inventory's first slots (9; 6 since the amendment below) with a
    count on each, instead of the creative items, and the wallet's total beside it; an empty slot
    or a non-block item places nothing. `E` opens the **backpack screen**: the 36 slots in a 9 × 4
    grid, the hotbar's row last; a press picks a stack up and a press (or release, a drag) on
    another slot puts it down (right press or Shift: half). Loose items are
    drawn as small spinning, bobbing copies of the block (a non-block item as a small block in its
    colour).

## Questions (parked; defaults chosen)

1. **Counts and chances of the first rules.** Grass: 1 dirt always and 1 rock at 50 %; dirt: 1 dirt;
   leaves (and snowy leaves): 1 stick at 50 %. Snowy grass as grass. All data; say if other numbers.
2. **Ramps** drop their base material (a stone ramp gives stone) and placing a slope uses one of
   it. OK, or should a slope cost more or less?
3. **Breaking time per block** (hands slower on stone): there is no break progress today, so it is
   listed as next; needs a hold-to-break mechanic first. Want it in the next change?
4. **Felled trees and blasts:** blocks that fall as pieces (a cut tree) or are flung by a blast
   settle back into blocks and drop nothing; you break them again. Should a blast drop items instead?
5. **Throwing items out** (a drop key) and **dropping a stack from the backpack screen** are not
   designed; add?
6. **Loose items in water** sink and rest on the bottom for now; should water carry them?
7. **The wallet on death** is kept (gems are not dropped). Should dying cost gems?
8. **A gem over the wallet's capacity** stays on the ground (whole gems only: a value-5 gem with
   3 left of room is not taken). Or should it be taken and the extra lost?
9. **E for the backpack** is a fixed key in the shell for now; into the input bindings data
   (`shared/input`) and the controller next.
10. **Item looks**: a non-block item is drawn as a small stone block tinted its colour (a flat
    disc in the hotbar). Proper models (a pebble, a twig, a gem) wait for Phase 40's parts.

## Built so far (0.50.33)

- **Data:** `shared/inventory/inventory.json`, block `drops` (grass and snowy grass, leaves and snowy
  leaves, tall grass), `shared/inventory/gen.mjs` with its test (validation naming the entry, an
  item and a block's drop added by data alone), CI regenerating it; adventure without
  `unlimitedBlocks`.
- **Model and wire:** `dwell_protocol::inventory` (`Inventory`, `roll`, the wallet), the three
  messages and the Welcome flag; round trips and the distribution of tall grass's drops pinned.
- **Server:** inventories per player (loaded at join, saved in `inventories`), loose items
  (`dwell-sim` `loose_items.rs`: fall and rest on both faces, despawn, the cap, saved in
  `loose_items`), break → drop and place → consume for block, region and body edits, pickup to the
  nearest player with room, death drops, `Hello` gating (`dwell-sim` `tests/items.rs`).
- **Client:** Hello on the flag, the inventory and loose items kept (`dwell-client`
  `loose_items.rs`), loose items drawn as small turning copies through the pieces' renderer, the
  hotbar showing the inventory with counts and the wallet, number keys and the wheel choosing
  inventory slots (the server told the held slot), a place with no block held refused, and the
  backpack screen (`client/src/ui/backpack.ts`).

**Next:** breaking time per block (needs hold-to-break), items thrown out, the backpack key in the
input data and on the controller and touch, proper item models, planting seeds, bigger wallets,
the dedicated server's give command.

## Consequences

- A new item is an entry in `inventory.json`; a new drop rule is a block's `drops`; a new item
  behaviour (tools, food) is code with its data.
- Tests: the data's validation and a fixture item and drop by data alone; break → drop → pickup;
  stacking and a full inventory; despawn; placing consumes in adventure and not in creative; death
  drops; save and load of inventories and loose items; loose items fall and rest on both faces.
- Reversing it: give adventure `unlimitedBlocks` again; the tables stay, unread.

## Starting items (0.50.40)

Owner, 2026-11-08: "Adventure starts with an axe." `shared/inventory/inventory.json` `startingItems`
(optional; `{ item, count }` each, validated by `gen.mjs`, emitted as `STARTING_ITEMS`) lists what a
player new to a world starts with, in a mode that uses items: one `dwell:stone_axe`. A player with a
saved inventory keeps it; creative players start with nothing (`Inventory::starting`,
`Server::load_inventory`; tests `an_adventure_player_starts_with_an_axe_and_a_creative_one_with_nothing`
and the generator's starting-items test).

## Amendment (0.50.73)

An item may carry a shaped `look` (ADR 0071's rod or lump: shape, size, texture, colour); it is
then drawn as that look everywhere (hand, hotbar, backpack, loose, on the ground). Items without one
keep the colour-and-size look.

## Amendment (0.50.75)

The look vocabulary gains `cluster` (a small heap of tiny lumps: seeds). The grass seed has one
(owner, 2026-11-09: "Seeds also look like a block, instead of a cluster of grass seeds"), held as the
characters item `cluster` (a few seeds in the fist), and drawn the same on the hotbar, loose and on
the ground.

## Amendment, 2026-11-10: a 6-slot hotbar and the inventory view (0.52.23)

Owner: "Cap the hotbar at 6 slots. We need inventory view. For both adventure and creative."
`hotbarSlots` is 6 and `backpackSlots` 30 (5 rows of 6, the hotbar's width): 36 slots as before, so
every saved slot keeps its index and the record and the wire are unchanged (no new compatibility
line). A player saved with 9 hotbar slots loads with old slots 7–9 (indices 6–8) as the backpack's
first three: nothing moves, nothing is lost or dropped, and a held slot past the hotbar becomes the
first (`dwell-protocol` `inventory.rs` `a_nine_slot_hotbar_save_loads_with_slots_7_to_9_in_the_backpack`).
Deviation from the brief (27 backpack slots, the old 7–9 moved into free backpack slots on load): the
same outcome with no load-time move and no full-backpack drop. The creative hotbar has the same 6
slots, picked in the creative inventory view (docs/ITEMS.md §6a).

## Amendment, 2026-11-11: pieces, combining and tool wear (0.52.46)

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
- **Combining** (owner, 2026-11-11): recipes as data, `shared/inventory/inventory.json` `recipes`
  (`name`, `inputs` [{ item, count }], `output` { item, count }), validated by `shared/inventory/gen.mjs`
  (known items, not currency, counts in 1..stackSize, unique names and inputs) and emitted as `RECIPES`
  (Rust, TS). The server applies one (`Inventory::combine`: the inputs from anywhere, the output added,
  nothing changed if an input is short or the output has no room) on `InventoryAction::Combine { recipe }`,
  the recipe's FNV-1a name hash (`inventory::recipe_id`, the same in every build that has it). The
  server sets `inventory_flags::COMBINE` (16); a client sends Combine only to such a server
  (`inventory::combine_request`), an older server never gets it. The backpack screen's Combine
  section lists the recipes the inventory can make with how many times and a button each (adventure:
  creative has no backpack, everything is in its catalogue). Recipes: 8 rock shards → stone (the ½ m
  slab), 8 dirt clumps → dirt, 8 wood chunks → log; 2 sticks + 2 rock shards + 2 grass fibers → a stone
  axe, or the same inputs → a stone pickaxe (the player picks).
- **Tool wear** (owner, 2026-11-11: the stone axe "should survive 32 hits of durability"):
  `shared/tools/tools.json` `durability` (`items`: tool item → hits; `creativeWear`; `breakSound`),
  validated and emitted (`DURABILITY`, `CREATIVE_WEAR`, `WEAR_BREAK_SOUND`). The stone axe 32; every
  other tool unlimited (the owner named only the axe). A hit that counts uses one: a chop, or a block
  break hit (one the server takes, refused ones don't) with the tool held; at none left the tool is
  gone from its slot and the client plays `stick_snap`. Creative: `creativeWear` false, tools don't
  wear. The wear is a slot's `Stack::wear`, carried by moves; it rides in the inventory record as
  entries at slot index 128 + slot with no item, which a build without wear skips as it skips any slot
  past its count: the record stays version 1 on the wire and in saves, no new compatibility line. The
  hotbar and backpack draw a wear bar from the data. Known gaps: a dead player's dropped tools lose
  their wear (loose items carry none); an axe's swing that bounces off a non-wood is not a hit.
