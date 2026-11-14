# Blueprints and builder NPCs

**[built: B1–B3 0.60.x; B4 towns 0.61.0 (branch `claude/blueprints-towns`)]** WORK_QUEUE item 56. Owner, 2026-11-13: **"I also want to add a new version of the human
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
- Gathering for builders (the owner: later); player-placed towns (later).

## 8. Tests

`shared/blueprints/gen.test.mjs` (the data, a fixture entry by data alone, bad entries named);
`dwell-protocol` (the record round-trips, bad records refused); `dwell-world` `blueprint` (both faces: the
ghost cells turned and through the face, the pick only at ghost cells with the block the player has, the
have/need counts, the builder order); `dwell-sim` `tests/blueprints.rs` (both faces: placed and synced to
every capable client, refused without the capability or past the limits, saved and reloaded, completed and
removed, a builder's progress over ticks skipping an occupied cell).
