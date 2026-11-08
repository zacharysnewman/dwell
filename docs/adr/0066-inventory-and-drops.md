# 0066 — Inventory and block drops: items as data, drops as light server entities

**Status:** Accepted (2026-11-08; first part built in 0.50.32, see "Built so far"). Builds on ADR 0065
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
   `shared/inventory/inventory.json`'s `items` (`dwell:rock`, `dwell:stick`: a label, a colour and a
   size for their simple look, optional `stack`). A block's item places that block; a non-block
   item places nothing for now. A name in neither (a newer build's item in a saved inventory) is
   kept as it is, so nothing is lost when an older build on the line opens the world, and cannot be
   used until a build knows it.
2. **Inventory data.** `shared/inventory/inventory.json` also holds `hotbarSlots` (9), `backpackSlots`
   (27), `stackSize` (64, an item may lower it with `stack`), `despawnSeconds` (300),
   `pickupRadiusM` (1.5), `pickupDelaySeconds` (a dropped item can't be taken back at once) and
   `maxDrops` (the most loose items a world holds; the oldest go first).
   `shared/inventory/gen.mjs` validates it, and every block's `drops` (below), and emits
   `crates/dwell-protocol/src/inventory.gen.rs` and `client/src/ui/inventory.gen.ts`. CI regenerates
   both and fails on a diff; `shared/inventory/gen.test.mjs` adds an item and a block's custom drop
   by data alone.
3. **Drops are block data.** A block's optional `drops` in `shared/blocks/*.json` is a list of
   `{ "item": name, "count": n or [min, max], "chance": 0..1 }`. Absent: the block drops one of
   itself, except that a stored ramp drops its base material (`of`), a block's extra cell
   (`partOf`) and a liquid drop nothing, and an indestructible block can't be broken. `[]` drops
   nothing. A bad entry fails naming the block and the problem (an item no block or item list
   names, a count or chance out of range). The roll uses the server's entropy.
4. **The inventory model.** A player has `hotbarSlots + backpackSlots` slots, hotbar first; a slot
   is empty or `(item, count)`. Adding fills stacks of that item first, then empty slots, hotbar
   first; what doesn't fit is left (on the ground). Placing a block takes one of that block's item
   (a ramp takes its base) from the held slot if it holds it, otherwise from the first slot that
   does; without one the edit is refused. The client moves items between slots with a
   `move(from, to, count)` (onto an empty slot or the same item it merges, onto another item it
   swaps); the server checks and applies it, and sends the result.
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
   without `unlimitedBlocks` drops every slot at the death point, spread a little.
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
   device key (a version byte, then each slot's index, item name and count), and loose items a new
   `drops` table (the whole set each save, like bodies). Older builds don't open the tables and
   keep them; a record they can't read is left out and preserved. Player records are unchanged.
10. **The UI.** In a world that uses items, the hotbar shows the inventory's first 9 slots with a
    count on each, instead of the creative items; an empty slot holds nothing. A key (`E`,
    data in the input bindings) or a button opens the **backpack screen**: the 36 slots in a 9 × 4
    grid, dragged between (a drag moves the stack, right-drag or a modifier half). Loose items are
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

## Built so far

(filled in below as the work lands)

## Consequences

- A new item is an entry in `inventory.json`; a new drop rule is a block's `drops`; a new item
  behaviour (tools, food) is code with its data.
- Tests: the data's validation and a fixture item and drop by data alone; break → drop → pickup;
  stacking and a full inventory; despawn; placing consumes in adventure and not in creative; death
  drops; save and load of inventories and loose items; loose items fall and rest on both faces.
- Reversing it: give adventure `unlimitedBlocks` again; the tables stay, unread.
