# Resurrection shrines

**[built, 0.58]** WORK_QUEUE item 53. Owner, 2026-11-12: "And then we want to add 'resurrection shrines'
these should work like Guild Wars. You respawn at the nearest shrine when you die." / "We'll have to
spread these pretty thick across the map likely."

## 1. The owner's answers

| Question | Answer (2026-11-12) |
|---|---|
| How far apart | ~2 km |
| Which shrine | the nearest one to where you died, on the face you died on |
| On the map | yes; the map can toggle them off (filters) |
| What it is | a built structure with a glow |
| Its shape | "a 8x8m terraced and filled stone circle with stepping pillars forming a half circle. Only the tallest pillars (half of them) are connected at the top. Forming a quarter circle." |
| Its facing | "prefer to rotate it so it faces out from hillsides" |
| The spawn | "use the original spawn point … then target the nearest shrine to that point for initial spawn, so all spawns are at shrines" |

What is added later, and how: more rules or a different shape by editing `shrines.json` (a tool could
write it, as the prototypes' pages do); another world type's rules by `worlds` (keyed by a world
setting's name, `shared/worldgen/worlds`). Per build (the terrain a seed generates); the reveal on
the map is per player (their explored cells); the filter is per browser.

## 2. Data (`shared/worldgen/shrines.json`)

| Key | Value | Meaning |
|---|---|---|
| `spacing`, `jitter` | 2000 m, 0.6 | One shrine at most per `spacing` cell, at a seeded point within `jitter` × half a cell of its centre |
| `attempts` | 16 | Points tried in a cell, in order; the first that suits is its shrine |
| `inland`, `height`, `maxSlopeDeg` | 80 m, [2, 900] m, 8° | The footprint: ≥ 80 m from the coast, dry (no river, lake or stream), its ground 2–900 m above sea level, its rise across it at most the width × tan 8° |
| `portalClear`, `clear` | 250 m, 3 m | Clear of the portals' axes; no tree within 3 m of the footprint |
| `flatBelowDeg` | 2° | Flatter ground takes the seed's quarter turn, steeper the downhill one |
| `structure`, `design` | `dwell:shrine` | The structure, built from `design` by `gen.mjs` (§3) |
| `lodCellM` | 16 | The LOD draws it at levels whose cells are at most 16 m |
| `stand` | [0, 2] | Where a player stands up: the column 2 m from the anchor on the open side (turned with it), on its floor |
| `searchCells`, `spawnWithin` | 3, 1500 m | A death searches the cells within 3 of its own; the spawn's shrine lies within 1500 m of the spawn search's point, else one is built at the point |
| `worlds` | `test:archipelago` | Overrides per world setting name (not `design`) |
| `map` | explored, `#7fd6ff`, 9 px | The big map's reveal (`explored`: shown once its 8 m explored cell is; `all`), colour and icon size |

`gen.mjs` validates every key (ranges, the structure, the stand on a floor open above, the overrides)
and emits `crates/dwell-worldgen/src/shrines.gen.rs`; `gen.test.mjs` covers it.

## 3. The shrine (`design`)

`gen.mjs` `shrineStructure` builds `shared/structures/shrine.json` (½ m rows): a round platform of
`block` (the columns whose centres lie within `radius` 4 m of its centre: 8 × 8 m), `base` 1 m filled
from its bottom row, then a ½ m step for each of `steps` [3, 2] m (the terraced dais). `pillars.count`
9 pillars on the back half's arc (`arcRadius` 3.5 m, 180°–360°), `lowest` 1.5 m at the arc's ends,
`rise` 0.5 m taller each one toward its middle (the −x half rasterised, the +x half its mirror, the
middle pillar the two columns on the axis). The middle half of them (5, the tallest, spanning 90°: a
quarter circle) are joined at their tops by a `lintel.thickness` ½ m lintel along the arc, each arc
column's at the nearest joined pillar's top, so the joined tops step as the pillars do. The light
(`light.block` torches) stands on the dais's top on the columns within 1 m of the centre. The count must
be 1 more than a multiple of 4 so the joined half spans exactly a quarter circle.

It stands like a portal's monument: anchored on the site, its bottom row on the lowest ground under
its footprint, over air and replaceable cells only, so on the higher side the ground fills it and it
never floats. Turned a quarter turn about +y: the open half faces the quarter nearest downhill (the
height field's fall across the footprint, ±reach in x and z; the structure is drawn on the block grid,
so four turns, not eight), or the seed's turn where the ground is flatter than `flatBelowDeg`.

## 4. Siting (`dwell-worldgen` `shrines.rs`)

A cell's shrine is a pure function of the face's seed, the cell, the rules, the portals' sites and the
face's height field (`column_at`, `coast_at`): never of another cell's. Chunks ask for the shrines whose
clear ground reaches them (`Shrines::near`) and stand their structures through the monuments' path
(`TerrainGenerator::monuments_near`: chunks, the sky test, the LOD's bounds and cells), so they generate in
any order and agree (a test generates a shrine's chunks in two orders). Each face sites its own (face
B from its own seed; the mirror world's face B has face A's, mirrored: a parity test).

**The spawn's shrine.** `TerrainGenerator::spawn_point` with shrines is the stand point of
`spawn_shrine`: the grid shrine nearest the spawn search's point (`spawn_search_point`, the search as
before) within `spawnWithin`, else a shrine built at the point itself, turned by its ground. That one is
resolved on demand and cached per world; chunks look for it only where no grid shrine lies within
`spawnWithin` less two reaches (it exists only where none is that near), so most never run the spawn
search. The trees keep off the grid's shrines; the spawn's own stands on the spawn's patch, already
clear of them. Face B's `?spawn=b` is its face's shrine the same way. Portal spawns (`?spawn=portal`)
and the moon keep theirs: they stand at a portal's rim or on another world by design.

## 5. Respawn (`dwell-sim` `server.rs`)

`kill` takes the respawn point from `ShrineRespawn` (`dwell_worldgen::shrine_respawn_for_sized`): the
shrine nearest the death on the death's face (`FaceFrame::at_height`), its stand point plus the players'
spread and half a metre (a spread may stand on a higher step), in **every game mode**. A world without
shrines (a test's generator, the flat world, the sandbox) or a death with none within `searchCells`
respawns at the world spawn as before. The respawn preload streams that shrine's ring while the player
is dead; the client guesses the same shrine from its own terrain (`nearest_known_shrine`, without the
spawn search) for its meshing priorities, the spawn's when it has no terrain. Nothing new on the wire.

## 6. The map (`client/src/ui/mapShrines.ts`)

The big map asks the client crate for the shrines in the cells round its view's centre (`mapShrines`,
from the client's terrain, sited as the server sites them) and draws a glowing ring for each one in an
explored cell (`reveal` `explored`: Guild Wars' map reveal; with the `mapRevealed` ability, creative,
or `reveal` `all`, every one). The ✦ button is the filter: it hides or shows the shrine layer,
remembered per browser (`dwell.map.shrines`, a convenience; failing storage keeps it for the page).
The map markers' auto markers (branch `claude/map-markers`) were not merged when this was built; the
shrine layer is its own filter until they are, when it becomes one of their kinds.

## 7. Compatibility

Shrines change the terrain every seed generates and move the spawn: a new compatibility line, 0.58
(RELEASES.md "Who raises the line"). Changing any rule but `map`, `stand` and `searchCells` changes the
terrain again.

## 8. Cost

Measured natively (`cargo run --release -p dwell-worldgen --example shrine_cost`, seed 0): siting a cell
takes 1.0–1.8 ms (the island's 169 cells ~170–300 ms a face, each once per process, on demand); the
spawn search the spawn's shrine needs is ~0.6 s on face A, run once per world per process and only by
the spawn, the respawn or a chunk with no grid shrine within ~1.5 km. Seed 3's face A has no grid shrine
within 1.5 km of its spawn search's point, so its spawn's shrine is built there.

## 9. Left

- The spawn's facing: the player stands up at the stand point with the default yaw, not turned out the
  open side.
- The map's first ask for a wide view sites up to 81 cells on the main thread (~100 ms natively, more
  in WASM; cached after); a worker could take it.
- A client's job worker that generates a chunk or near LOD section far from every grid shrine (open sea)
  runs the spawn search once (~0.6 s natively) to know whether the spawn's shrine reaches it.
- A dedicated glowing stone (an emissive block) in place of the torches, once the look is chosen.
