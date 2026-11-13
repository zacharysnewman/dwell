# Blueprints and builder NPCs

**[in progress]** WORK_QUEUE item 55. Owner, 2026-11-13: **"I also want to add a new version of the human
NPC, same looks, no sword capable of building blueprints like the ones you can save right now. In adventure
you can place "blueprints" but not as actual blocks as an entity that shows ghost blocks. When you hold
build, it auto places only within the correct placement, only the correct blocks where you're aiming like
our drag building today. If you don't have a block, it won't place it, it will show blocks you don't have as
red. Should also show n/n logs, etc all required blocks on hover in a compact UI. NPCs will use this to build
towns and cities from blueprints I provide. Feel free to add simple test ones."**

## 1. The owner's answers (2026-11-13)

| Question | Answer |
|---|---|
| The builders' blocks | unlimited for now; gathering later |
| Where towns come from | worldgen sites for now ("Eventually both, but just world gen sites for now"); player-placed later |
| Who sees a placed blueprint | everyone in the world; anyone can build it; saved with the world |

What is added later, and how: more blueprints are structure files (`shared/structures/`, or a player's own
library, STRUCTURES.md §9); more builder kinds are creatures with a `builder` block (`shared/creatures`); the
pace, reach, limits and looks are `shared/blueprints/blueprints.json`; town plans are data in the same file.
Per world (the placed blueprints), per build (the repository's blueprints and rules).

## 2. What a blueprint is

A **blueprint** is an entity, not blocks: a structure's cells (its palette and cells, ≤ 32 a side, the one-file
`voxels` body of STRUCTURES.md §3) placed at a face-local anchor cell, on a face, turned by a quarter turn.
It is drawn as **ghost blocks** (the brush preview's see-through faces, BRUSHES.md §3): a cell the viewer
holds the block for in the ghost's colour, one they don't in **red**. A cell already holding its block is
not drawn. When every cell holds its block the blueprint is **complete and disappears** (the server removes it
and tells everyone): the blocks are the record; nothing else is kept.

The record (`dwell_protocol::blueprints::Blueprint`): `u32 id`, `str name` (the structure's, for the panel),
`u8 face`, `i32 x y z` (face-local anchor cell), `u8 turn` (quarter turns clockwise seen from above, 0–3),
`u8 sx sy sz`, `i32 ax ay az` (the anchor inside the box), `u8` palette count then each state name, then the
cells as runs (`u16 run`, `u8` palette index + 1; 0 keeps what is there). The record carries the cells, not a
name to look up, so a player's own library structure (on their device only) is buildable by everyone. Names a
build doesn't know leave their cells out (kept).

## 3. Placing one (B1)

Where the world's mode has the `blueprints` ability (adventure and creative), a structure picked from the
inventory's **Blueprints** list (the repository's test blueprints and the player's own library) follows the
crosshair as the paste ghost does; **R** turns it, a **right click** places it as a blueprint (a `Blueprints`
`Place` message), **Delete** while aiming into one removes it. The server checks the mode, the limits
(`limits.perWorld`, the size) and that the box lies in the world, gives it an id, saves it in the world's
optional `blueprints` table and sends it to every client with the **`blueprints.world`** capability. A client
without it sees nothing (an older client), and an older server is never sent one.

## 4. Building from one (B2)

Holding **Place** while aiming into a blueprint's box builds it (`dwell_world::blueprint::pick`): along the
view ray, the first ghost cell that is free (air, a replaceable plant, water), within reach, beside a solid
block or a built cell (a place needs a face), and whose block the player has; one piece every
`player.paceMs`, as a one-cell region fill of the ghost's own state (the drag building's path, ITEMS.md §5,
so the server checks reach and pays the item as for any place). A cell whose block the player lacks is never
placed and shows red. Only ghost cells, only their block: a held Place aimed into a blueprint never builds
anything else.

**The panel**: aiming into a blueprint shows a compact panel, its name, each required block's have / need
("log 12/40": what the player holds of it / the cells still to build of it), and the cells left.

## 5. Builder NPCs (B3)

A **builder** is the villager's look without the sword: a creature (`dwell:builder` in `shared/creatures`, the
Peasant with its colours, no `held`, no fight) with a `builder` behaviour from `blueprints.json` `builder`:
it works the blueprint it is given, at `paceS` a piece, with unlimited blocks, walking (the creatures' walk)
to within `reachM` of its next cell. Its order (`dwell_world::blueprint::builder_order`): the lowest row
first, and within a row the cells resting on something solid before the ones that don't, so foundations go
in before walls and supports before spans. A cell a player filled with something else is skipped (occupied);
one already holding its block counts as built. The server places its piece as the growers do (an edit
broadcast to everyone, saved with the world).

## 6. Towns (B4)

**[planned]** Deterministic town sites per face (a grid with spacing and siting rules, as the shrines',
SHRINES.md), each a town plan (blueprints laid out by data) whose builders spawn there and build over time.
Siting changes nothing a seed generates by itself (the blueprints are entities and the blocks are edits), but
the owner asked for a new compatibility line with it.

## 7. Tests

`shared/blueprints/gen.test.mjs` (the data, a fixture entry by data alone, bad entries named);
`dwell-protocol` (the record round-trips, bad records refused); `dwell-world` `blueprint` (both faces: the
ghost cells turned and through the face, the pick only at ghost cells with the block the player has, the
have/need counts, the builder order); `dwell-sim` `tests/blueprints.rs` (both faces: placed and synced to
every capable client, refused without the capability or past the limits, saved and reloaded, completed and
removed, a builder's progress over ticks skipping an occupied cell).
