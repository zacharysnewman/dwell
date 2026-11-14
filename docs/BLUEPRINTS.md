# Blueprints and builder NPCs

**[built: B1–B3 0.60.x; B4 towns 0.61.0 (branch `claude/blueprints-towns`); B5 the storehouse and the builders' trips, first slice (the 0.67 line, branch `claude/storehouse`)]** WORK_QUEUE item 56. Owner, 2026-11-13: **"I also want to add a new version of the human
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

Where the world's mode has the `blueprints` ability (adventure and creative), the game menu's **Structures**
page lists the repository's blueprints (`catalog`) under **Blueprints**, and each saved structure of the
player's library has a **Blueprint** button: picked with a turn (0°, 90°, 180°, 270°), it is placed with its
anchor on the cell a place would fill at the crosshair (a `Blueprints` `Place` message). **Remove the aimed
blueprint** on the same page removes the one the view aims into. The server checks the mode, the limits
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

## 6. Towns (B4) **[built, 0.61]**

The data (`blueprints.json` `towns`):

| Key | Value | Meaning |
|---|---|---|
| `spacingM`, `jitter`, `attempts` | 1200 m, 0.6, 8 | One site at most per grid cell, at a seeded point within `jitter` × half a cell of its centre; points tried in order, the first that suits is the site |
| `heightM`, `maxSlopeDeg`, `footprintM` | [2, 600] m, 12°, 32 m | Its footprint's centre and four corners (±`footprintM`) dry land (no sea, lake, river or stream) that high above sea level, rising at most `maxSlopeDeg` across it |
| `foundM` | 128 m | A living player this near (on its face) founds it |
| `clearM` | 4 m | The clear ground round the layout: no tree, bush or boulder is generated reaching the box round its blueprints (any turn) plus this (0.66, BUGS.md #169) |
| `plans` | `dwell:hamlet` | Picked per site by `weight`: blueprints (a structure, its anchor's offset `at` [x, z] m from the site, its `turn`); the hamlet is a well, two huts, a watchtower and a wall |

**Siting** (`dwell_world::blueprint::TownRules::site_in`, `sites_near`, `layout`) is a pure function of the
world seed, the face (face B hashes its own), the grid cell and the ground: never of another cell, so sites
agree in any order. The ground is `dwell_worldgen::town_ground_for_sized`: the face's height field (`column_at`)
where it is dry land in the band, and clear of the shrines' clear ground by 8 m and of the portals by the
shrines' `portalClear` (`Shrines::crowds`). A world without a height field (the flat world, the sandbox, a
test generator) has no towns.

**Clear ground** (0.66, BUGS.md #169): the server's world's generators (`BifacialTerrain::sized`) site the
towns too (`dwell_worldgen::towns::Towns`, the same `site_in` on the same ground, cached per grid cell) and
place no tree, bush or boulder whose footprint reaches a site's `TownRules::clear_box` (each blueprint's
structure's furthest column from its anchor, so any turn, plus `clearM`), as the trees keep off the shrines'
clear ground. A town is founded on clear ground, its doors and the paths between its buildings included.
The spawn search's own tree check asks without the towns (`trees_in_keeping`): the towns' ground keeps off
the spawn's shrine, which that search is resolving, and a patch clear of every tree is clear of theirs too.

**Founding** (the server, every 2 s): each site within `foundM` of a living player not yet founded is founded
once every blueprint of its plan finds the world's ground under its anchor (within 12 rows of the height field's;
until then it waits): the blueprints are placed as any blueprint (§2, saved, synced), and the site is recorded in
the world's `blueprints` record (format 2: the founded sites after the blueprints; format 1, B1–B3's, still read),
so it is never founded again, built or not. Builders (§5) then come and build them while players are near.

**Compatibility**: the 0.61 line (RELEASES.md "Who raises the line"; the owner asked for a line with towns).
Seeds' terrain is unchanged (blueprints are entities, the blocks edits), but a world opened in 0.61 gets towns
an older build wouldn't found or save.

**Cost** (release, seed 0): the sites within 3 km of the spawn, 16 on face A and 10 on face B, in 30–130 ms the
first time (the shrines' cells sited and cached); a founding check looks at the cells within `foundM` only.

## 6b. Towns on the map (2026-11-14)

The owner: **"Is there a way for me to watch the NPC's build a house?"** → **"Yes let's get town markers and a
recording of builders."** The town sites are a derived layer of the big map and the minimap, like the shrines
(MAP_MARKERS.md "Filters"): the client crate sites them from the seed as the server does
(`BifacialTerrain::town_in` on `town_ground`, each grid cell sited once and cached; `mapTowns`), with a state: 1
while a blueprint of the world's lies within `footprintM` of the site (founded, being built), else 0 (not founded,
or built: a finished blueprint is gone, and the client keeps no founded list). The shell (`client/src/ui/mapTowns.ts`)
draws the markers' vector `town` icon at each site in explored ground (`reveal`), clustered as the shrines, in
`color` or `buildingColor`, behind the "Towns" filter. Its looks are data: `blueprints.json` `towns.map`, validated by
`gen.mjs`. Tests: `dwell-worldgen` `tests/towns.rs` (both faces: the map's sites are the server's; the town link
stands by one), `dwell-client` `map_markers` (the state), `mapTowns.test.ts`.

The dev link **`?spawn=town`** (`town-b`) opens its own world file with the spawn 24 m south of the town site nearest
the face's spawn (`dwell_worldgen::town_spawn_for_sized`, `local::spawn_by_town`), so it founds at once and its
builders come (docs/DEV_TOOLS.md).

## 7. Built, and left

Built (B1–B4): the data and its generator, the four test blueprints, the record and message behind the
capability, the ghost geometry (`dwell_world::blueprint`), the world's list on the server (placed, removed,
completed, saved), the builders, the client's ghost (drawn in the brush preview's slot while no brush is
active), the held Place, the panel, the Structures page's Blueprints list, and the towns (§6).

Left:
- Towns' plans beyond the hamlet (more plans are data); paths and roads between their blueprints; player-placed
  towns (the owner: later).
- A ghost preview while choosing where to place one (it is placed straight at the crosshair, as a turn picked
  on the page; the paste ghost box could follow the crosshair first, as for a structure).
- A blueprint's ghost is drawn only while aimed into (within four reaches); a brush in use hides it.
- A builder shows no building pose of its own (it stands; the place swing is the players').
- Gathering for builders: B5's later phases (§9); player-placed towns (later).

## 8. Tests

`shared/blueprints/gen.test.mjs` (the data, a fixture entry by data alone, bad entries named);
`dwell-protocol` (the record round-trips, bad records refused); `dwell-world` `blueprint` (both faces: the
ghost cells turned and through the face, the pick only at ghost cells with the block the player has, the
have/need counts, the builder order); `dwell-sim` `tests/blueprints.rs` (both faces: placed and synced to
every capable client, refused without the capability or past the limits, saved and reloaded, completed and
removed, a builder's progress over ticks skipping an occupied cell).

## 9. The storehouse and the work loop (B5) **[first slice built, the 0.67 line]**

WORK_QUEUE item 57. Owner, 2026-11-14: **"Ideally long term, I'd like this to work a lot like age of empires for
NPCs, they gather materials drop them at a storehouse, perhaps each villager drops their materials in a storehouse.
They should have a storehouse (probably needs something for that) for holding materials. It would be fun if they
displayed their inventory like this (screenshot). For now we can make the store house hold unlimited resources until
the villagers actually harvest and drop the resources. Villagers can only hold 10 resources items at a time. The
villagers would know what building they're working on, what resources it needs, when grabbing resources would take
what they need next, walk to the building they're working on, build it, go back to the storehouse, rinse and
repeat."** (the screenshot: a villager carrying a tall stack of logs on their back). Then: **"Ours will eventually use
an even more in depth system where villagers have to plant trees, cut them down, other NPCs will cart them away, to
town store house, and there other NPCs use those materials to build. Make sense."**

### 9.1 The owner's answers, and what is data

The owner's spec stands in for the design questions (CLAUDE.md "ask before designing"):

| Question | Answer |
|---|---|
| What is added later | more jobs (foresters plant and fell, carters haul to the storehouse; builders take from it and build), carts, a counted stock filled by deposits, more storehouse designs |
| Who adds them, with what | the repository's data: `blueprints.json` (`jobs`, `storehouse`, `carryLook`, a plan blueprint's `role`), structures for the buildings, `characters.json` for the carried look |
| Fixed in code | the job kinds' vocabulary (`build` now), the stock modes (`unlimited` now), the transfer API, the trip state machine |
| Per world, per build | the storehouses, their stock and the builders' loads are the world's (saved); the rules are the build's |

### 9.2 What is built (the first slice)

- **A storehouse** is a town plan blueprint with `"role": "storehouse"` (the hamlet's `dwell:bp_storehouse`, a 5 × 4 m
  open shed, the plan's first blueprint: the generator fails a storehouse that isn't first). Founding a town places it
  and records a storehouse (`Storehouse`: its box, its face, the blueprint building it, its stock). Its own blueprint
  is built as B3's, by a builder from its own hands; **the town's other blueprints wait** (no builders) until it is
  built. A storehouse whose blueprint a player removes is given up (the town then builds as B3's).
- **Its stock** (`storehouse.stock`) is `unlimited` for now: it gives whatever is asked and keeps whatever is put
  (`dwell_world::stock::Stock::Unlimited`). A counted stock (`Stock::Counted`: what was put in, no more) is built and
  tested in the API and the save, so harvesting's deposits slot in without a new format.
- **One transfer API** (`dwell_world::stock`): every holder (a villager's `Load`, a storehouse's `Stock`, later a
  cart's) is a `Holder` (`available`, `room`, `give`, `take`), and items move between holders only through
  `transfer(from, to, block, n)` (and `transfer_list`, `Load::empty_into`): as many as one has and the other has room
  for. A forester's load to a cart, a cart's to a storehouse and a storehouse's to a builder are the same call.
- **Jobs** (`jobs`): a name, a `kind` (`build` is the only one built; `haul`, `fell`, `plant` fail the generator as
  later phases), `carryMax` (10), `mixedLoads` (true) and `waitS` (3 s). The builders' creature does `builder.job`
  (`dwell:builder`).
- **The builder's loop** (the `build` job, `dwell-sim` `server/blueprints.rs`), for a blueprint within
  `storehouse.rangeM` (64 m) of a built storehouse on its face (the nearest): at the storehouse (within
  `storehouse.reachM`, 2.5 m, of its box) it puts back what it still carries and takes its next load: the next
  `carryMax` cells of its blueprint in build order (`Ghost::next_load`: §5's order, mixed blocks when `mixedLoads`,
  else the run of the first block only); it walks to the site (round the blueprint's box and the storehouse's), places
  **only what it carries** (`Ghost::builder_next_of`), one piece a cell at `paceS`; when empty, or when nothing it
  carries can go in (a player filled the cells), it walks back. A storehouse lacking what is needed (a counted stock)
  makes it wait `waitS` and look again. When its blueprint completes it **moves on to the nearest blueprint the same
  storehouse serves** (keeping what it carries), else goes; what it carried goes back to the storehouse. A walk that
  doesn't move for 2 s, or a leg longer than 60 s, counts as stuck and the work goes on from where it is (as B3's).
  A blueprint no storehouse serves (a player's own, far from towns) is built as B3's, unlimited, no trips.
- **Saved with the world**: the blueprints record's format 3 (`dwell_protocol::blueprints::SavedWork`) adds the
  storehouses (id, face, box, building blueprint, stock: unlimited or counted by state name) and every builder's load
  (by blueprint; a builder sent away for want of a player near keeps its load for when it comes back).
- **The carried stack** (`carryLook`, the `npcs.carry` capability): a Creatures message ends with each carrying NPC's
  load (block state names and counts, in load order, after the poses); the client draws a cube of each piece's block
  (`characters.json` item `carried`, the block's own tiles) stacked on the back from the model's carry joint
  (`joint`, the Peasant's chest `spine.003`, `at` from it in the body's frame), in layers of `perLayer` side by side,
  `pieceM` a side, rising with the count; `kinds` gives a block its own size (logs 0.24 m). It moves with the body (the
  joint's place each frame, the body's turn), and an empty load draws nothing. Without the capability (either side)
  nothing is sent and nothing is drawn.

### 9.3 Compatibility

The storehouse is a new blueprint in the hamlet's plan, so a town's clear ground (`TownRules::clear_box`, §6) is larger
and the trees a seed generates near towns change: **the 0.67 line**. The blueprints record's format 3 is read by this
line only (older lines' format 1 and 2 are still read). `npcs.carry` is a capability: older clients get no carry part.

### 9.4 Later phases (not built)

The owner's long-term direction: villagers have **jobs** (as data): **foresters** plant trees and fell them (felling
also clears a town's footprint; BUGS.md #169's clear ground stays instant at generation until then), **carters** haul
what is felled or mined to the storehouse in carts, **builders** take from the storehouse and build (built). The
seams: a job `kind` per role (the generator names `haul`, `fell`, `plant` as later), the `Holder` trait for carts, the
counted `Stock` and its saved form for deposits, the storehouse as the one hand-off point. Also later: mining,
villagers choosing their jobs, more storehouse designs and several per town, a builder's carrying pose (the stack is
drawn; the arms keep the walk clip).

### 9.5 Tests

`shared/blueprints/gen.test.mjs` (the jobs, the storehouse, the carry look and a plan's role: fixture entries by data,
bad entries named); `dwell-world` `stock` (the transfer API: a load's cap, an unlimited and a counted stock),
`blueprint` (both faces: a load is the next cells in build order at most `carryMax`, mixed or a run; a builder given
only what it carries; the stack rises with the count; the hamlet's storehouse first); `dwell-protocol` (format 3
round-trips, 1 and 2 still read; the carry part follows the poses, bad counts refused); `dwell-sim`
`tests/blueprints.rs` (both faces: a builder fetches from the storehouse, never carries more than 10, loads only at
the storehouse, places only what it carried, makes the trips and finishes the hut; the stack sent is its load; a new
town builds its storehouse first and the others wait; a save keeps the storehouse and a builder's load) and
`server/blueprints.rs` (the way round a box, red on B3's); `dwell-client` (both faces: the load's pieces from the
wire, a cube per piece behind the back rising with the count).
